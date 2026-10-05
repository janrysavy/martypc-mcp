//! Complete supported core Machine ownership at a completed native-call boundary.
//! The caller must quiesce execution AND frontend/channel consumers. This module
//! prepares a fresh candidate; frontend/RPC rebind, container integrity, host
//! access/path policy and actual process/Pyro restart remain outer obligations.

use super::*;
use crate::{
    bus::BusState,
    cpu_808x::Intel808xState,
    service_interrupt::ServiceInterruptState,
    vhd::{DiskCaptureMode, VhdIO},
};
use anyhow::{bail, Result};
use sha2::{Digest, Sha256};

macro_rules! machine_state {
    ($( $(#[$attr:meta])* $field:ident: $ty:ty ),* $(,)?) => {
        #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct MachineSnapshot {
            version: u32,
            binding: String,
            cpu: Intel808xState,
            bus: BusState,
            service: ServiceInterruptState,
            patch_installed: Vec<bool>,
            presentable_events: Vec<PresentableDeviceEvent>,
            $( $(#[$attr])* $field: $ty,)*
        }

        impl Machine {
            /// Only while execution and all external receivers are quiesced.
            /// Capture preserves queued presentation events in their native channel.
            pub fn snapshot_state_quiesced(&mut self, mode: DiskCaptureMode, limit: u64)
                -> Result<(MachineSnapshot, [Option<Vec<u8>>; 2])> {
                self.snapshot_core_supported()?;
                let binding = self.snapshot_binding()?;
                let CpuDispatch::Intel808x(cpu) = &mut self.cpu else { bail!("NEC machine snapshots unsupported") };
                let cpu_state = cpu.snapshot_cpu_state().map_err(anyhow::Error::msg)?;
                let (bus, payloads) = cpu.bus_mut().snapshot_bus_state(mode, limit)?;
                let service = self.service_interrupt_manager.snapshot_state();
                self.service_interrupt_manager.preflight_state(&service).map_err(anyhow::Error::msg)?;
                // All fallible captures have completed. The Machine owns both
                // channel ends; send cannot disconnect while its receiver exists.
                let presentable_events: Vec<_> = self.presentable_event_receiver.try_iter().collect();
                for event in &presentable_events {
                    self.presentable_event_sender.send(*event).expect("owned presentation receiver");
                }
                let saved = MachineSnapshot {
                    version: 1, binding, cpu: cpu_state, bus, service,
                    patch_installed: self.rom_manifest.patches.iter().map(|p| p.installed).collect(),
                    presentable_events,
                    $($field: self.$field.clone(),)*
                };
                self.preflight_snapshot_meta(&saved)?;
                Ok((saved, payloads))
            }

            /// Consume a freshly constructed, dependency-validated candidate.
            /// Never call this by moving a live Machine out of its owner before
            /// other host resources are ready. Failure returns no partial Machine.
            pub fn prepare_snapshot_restore(mut self, saved: &MachineSnapshot,
                providers: [Option<Box<dyn VhdIO>>; 2]) -> Result<Self> {
                self.snapshot_core_supported()?;
                self.preflight_snapshot_meta(saved)?;
                self.service_interrupt_manager.preflight_state(&saved.service).map_err(anyhow::Error::msg)?;
                let CpuDispatch::Intel808x(cpu) = &mut self.cpu else { bail!("NEC machine snapshots unsupported") };
                // This is a discarded-on-error candidate, never the live owner.
                cpu.restore_cpu_state(&saved.cpu).map_err(anyhow::Error::msg)?;
                let bus = std::mem::take(cpu.bus_mut()).prepare_bus_restore(&saved.bus, providers)?;
                *cpu.bus_mut() = bus;
                self.service_interrupt_manager.restore_state(&saved.service).map_err(anyhow::Error::msg)?;
                $(self.$field = saved.$field.clone();)*
                for (patch, installed) in self.rom_manifest.patches.iter_mut().zip(&saved.patch_installed) {
                    patch.installed = *installed;
                }
                // Fresh channels retain the candidate's device senders. Outer
                // frontends must reconnect their receivers before a live swap.
                self.presentable_event_receiver.try_iter().for_each(drop);
                for event in &saved.presentable_events {
                    self.presentable_event_sender.send(*event).expect("owned presentation receiver");
                }
                Ok(self)
            }
        }
    };
}

machine_state! {
    state: MachineState,
    options: MachineOptions,
    kb_buf: VecDeque<KeybufferEntry>,
    error: bool,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    error_str: Option<String>,
    turbo_bit: bool, turbo_button: bool,
    cpu_factor: ClockFactor,
    #[serde(with = "crate::snapshot_codec::f64_bits")]
    cpu_clock_period: f64,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    next_cpu_factor: Option<ClockFactor>,
    cpu_cycles: u64, cpu_instructions: u64, system_ticks: u64,
    checkpoint_map: MartyHashMap<u32, usize>,
    patch_map: MartyHashMap<u32, usize>,
    events: Vec<MachineEvent>,
    reload_pending: bool,
    halt_behavior: OnHaltBehavior,
    disassembly: Disassembly,
}

impl MachineSnapshot {
    /// Actual mounted VHD identity for each controller slot, independent of paths.
    /// Unloaded drives have no payload even if native Disk/ATA objects still exist.
    pub fn disk_requirements(&self) -> [Option<crate::vhd::DiskSnapshotRequirement>; 2] {
        self.bus.disk_requirements()
    }
}

impl Machine {
    /// Runtime provider contract, separate from captured cached read_only flags.
    /// Only mounted constructor-enforced RW File providers are accepted.
    pub fn snapshot_rw_files(&self) -> bool { self.bus().snapshot_rw_files() }

    fn snapshot_core_supported(&self) -> Result<()> {
        if self.debug_snd_file.is_some()
            || self.options.record_listing
            || !self.disassembly_listing.is_empty()
            || self.disassembly_listing_file.is_some()
        {
            bail!("active Machine host logging/listing snapshots unsupported");
        }
        #[cfg(feature = "sound")]
        if !self.sound_sources.is_empty() {
            bail!("Machine audio output queues not composed yet");
        }
        Ok(())
    }

    fn snapshot_binding(&self) -> Result<String> {
        let desc = self.machine_desc;
        if !desc.system_crystal.is_finite()
            || !desc.bus_crystal.is_finite()
            || desc.timer_crystal.is_some_and(|v| !v.is_finite())
            || self.machine_config.keyboard.as_ref().is_some_and(|k| {
                k.typematic_delay.is_some_and(|v| !v.is_finite()) || k.typematic_rate.is_some_and(|v| !v.is_finite())
            })
        {
            bail!("nonfinite Machine configuration");
        }
        // ROM dependencies include the actual transformed original bytes and
        // patch definitions. Installed flags are mutable runtime state, separate
        // from original ROM identity: patched RAM must not equal cold ROM bytes.
        let mut fixed_roms = self.rom_manifest.clone();
        for patch in &mut fixed_roms.patches {
            patch.installed = false;
        }
        let bytes = serde_json::to_vec(&(
            self.machine_type,
            &self.machine_config,
            &self.machine_desc,
            self.preferences,
            self.load_bios,
            fixed_roms,
        ))?;
        let mut hash = Sha256::new();
        hash.update(bytes);
        #[cfg(feature = "sound")]
        hash.update(serde_json::to_vec(&self.sound_config)?);
        Ok(format!("{:x}", hash.finalize()))
    }

    fn preflight_snapshot_meta(&self, saved: &MachineSnapshot) -> Result<()> {
        if saved.version != 1
            || saved.binding != self.snapshot_binding()?
            || saved.patch_installed.len() != self.rom_manifest.patches.len()
        {
            bail!("incompatible Machine version/configuration/ROM dependencies");
        }
        // Listing has an uncaptured host sink/map: inspect the incoming state,
        // not merely the cold candidate's disabled listing option.
        if saved.options.record_listing {
            bail!("saved Machine listing snapshots unsupported");
        }
        if !saved.cpu_clock_period.is_finite()
            || saved.cpu_clock_period < 0.0
            || matches!(saved.cpu_factor, ClockFactor::Divisor(0) | ClockFactor::Multiplier(0))
            || saved
                .next_cpu_factor
                .is_some_and(|f| matches!(f, ClockFactor::Divisor(0) | ClockFactor::Multiplier(0)))
            || saved
                .checkpoint_map
                .values()
                .any(|i| *i >= self.rom_manifest.checkpoints.len())
            || saved.patch_map.values().any(|i| *i >= self.rom_manifest.patches.len())
            || saved.events.iter().any(|event| {
                matches!(event, MachineEvent::CheckpointHit(index, _) if *index >= self.rom_manifest.checkpoints.len())
            })
        {
            bail!("invalid Machine clock or ROM map/event index");
        }
        // Native reinstall_roms retains historical maps and queued event levels.
        // Their bounds must be safe, but forcing the current manifest's addresses
        // or levels would reject actual native history. Metadata provenance and
        // external dependencies must be authenticated by the outer loader.
        // Machine::new finishes with set_cpu_factor; its temporary zero period
        // never escapes the constructor. Pending turbo leaves the current factor
        // and period unchanged until run() applies the next factor. Match the
        // exact native two-operation computation, including rounding.
        let mhz = match saved.cpu_factor {
            ClockFactor::Divisor(n) => self.machine_desc.system_crystal / f64::from(n),
            ClockFactor::Multiplier(n) => self.machine_desc.system_crystal * f64::from(n),
        };
        if saved.cpu_clock_period.to_bits() != (1.0 / mhz).to_bits() {
            bail!("inconsistent native Machine CPU period");
        }
        Ok(())
    }
}

#[cfg(all(test, not(any(feature = "cpu_validator", feature = "cpu_collect_cycle_states"))))]
mod tests;
