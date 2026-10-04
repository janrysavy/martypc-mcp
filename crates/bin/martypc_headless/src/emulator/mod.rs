/*
    MartyPC
    https://github.com/dbalsom/martypc

    Copyright 2022-2026 Daniel Balsom

    Permission is hereby granted, free of charge, to any person obtaining a
    copy of this software and associated documentation files (the “Software”),
    to deal in the Software without restriction, including without limitation
    the rights to use, copy, modify, merge, publish, distribute, sublicense,
    and/or sell copies of the Software, and to permit persons to whom the
    Software is furnished to do so, subject to the following conditions:

    The above copyright notice and this permission notice shall be included in
    all copies or substantial portions of the Software.

    THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
    IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
    FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
    AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
    LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
    FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
    DEALINGS IN THE SOFTWARE.

    --------------------------------------------------------------------------
*/

/// Definition of [Emulator] struct and related types.
use crate::{JoystickData, MouseData};
use std::ffi::OsString;

use crate::{Counter, KeyboardData};
use anyhow::{anyhow, Context, Error};
use marty_config::{ConfigFileParams, VhdConfigEntry};
use marty_core::{
    cpu_common::CpuOption,
    machine::{Machine, MachineEvent, MachineState},
    vhd::VirtualHardDisk,
};
use marty_frontend_common::{
    cartridge_manager::CartridgeManager,
    floppy_manager::FloppyManager,
    resource_manager::ResourceManager,
    rom_manager::RomManager,
    timestep_manager::PerfSnapshot,
    vhd_manager::VhdManager,
};

/// Define flags to be used by emulator.
#[derive(Default)]
pub struct EmuFlags {
    pub render_gui: bool,
    pub debug_keyboard: bool,
}

/// Define the main [Emulator] struct for this frontend.
/// All the items that the winit event loop closure needs should be set here so that
/// we can call an event handler in a different file.
/// All members are public so that a reference to this struct can be passed around as 'god' state.
pub struct Emulator {
    pub rm: ResourceManager,
    pub romm: RomManager,
    pub romsets: Vec<String>,
    pub config: ConfigFileParams,
    pub machine: Machine,
    pub machine_events: Vec<MachineEvent>,
    //pub exec_control: Rc<RefCell<ExecutionControl>>,
    pub mouse_data: MouseData,
    pub joy_data: JoystickData,
    pub kb_data: KeyboardData,
    pub stat_counter: Counter,
    pub floppy_manager: FloppyManager,
    pub vhd_manager: VhdManager,
    pub cart_manager: CartridgeManager,
    pub flags: EmuFlags,
    pub perf: PerfSnapshot,
}

/// Preserve configured drive slots and reject an unusable disk before execution.
fn mount_named_vhds(
    machine: &mut Machine,
    names: Vec<Option<String>>,
    mut load: impl FnMut(usize, &str) -> Result<VirtualHardDisk, Error>,
) -> Result<(), Error> {
    for (drive, name) in names.into_iter().enumerate() {
        let Some(name) = name else { continue };
        if drive >= 2 {
            return Err(anyhow!("Configured VHD slot {drive} exceeds controller capacity"));
        }
        let vhd = load(drive, &name).with_context(|| format!("Loading VHD {name} for drive {drive}"))?;
        // These are distinct native devices. hdc_mut() means Xebec, not any HDC.
        let result = if let Some(hdc) = machine.hdc_mut() {
            hdc.set_vhd(drive, vhd).map_err(Error::new)
        } else if let Some(hdc) = machine.xtide_mut() {
            hdc.set_vhd(drive, vhd).map_err(Error::new)
        } else if let Some(hdc) = machine.jride_mut() {
            hdc.set_vhd(drive, vhd).map_err(Error::new)
        } else {
            Err(anyhow!("No hard disk controller present"))
        };
        result.with_context(|| format!("Mounting VHD {name} on drive {drive}"))?;
        log::info!("VHD {name} mounted on drive {drive}");
    }
    Ok(())
}

fn apply_vhd_overrides(
    mut names: Vec<Option<String>>,
    overrides: &[VhdConfigEntry],
) -> Result<Vec<Option<String>>, Error> {
    let mut seen = [false; 2];
    for entry in overrides {
        if entry.drive >= seen.len() {
            return Err(anyhow!("Configured VHD slot {} exceeds controller capacity", entry.drive));
        }
        if seen[entry.drive] {
            return Err(anyhow!("Duplicate configured VHD slot {}", entry.drive));
        }
        seen[entry.drive] = true;
        if names.len() <= entry.drive {
            names.resize(entry.drive + 1, None);
        }
        names[entry.drive] = Some(entry.filename.clone());
    }
    Ok(names)
}

impl Emulator {
    #[allow(dead_code)]
    pub fn validate_config(&self) -> Result<(), Error> {
        Ok(())
    }

    /// Apply settings from configuration to machine, gui, and display manager state.
    /// Should only be called after such are constructed.
    pub fn apply_config(&mut self) -> Result<(), Error> {
        log::debug!("Applying configuration to emulator state...");

        // Set the initial power-on state.
        if self.config.emulator.auto_poweron {
            self.machine.change_state(MachineState::On);
        }
        else {
            self.machine.change_state(MachineState::Off);
        }

        // Do PIT phase offset option
        self.machine
            .pit_adjust(self.config.machine.pit_phase.unwrap_or(0) & 0x03);

        self.machine.set_cpu_option(CpuOption::OffRailsDetection(
            self.config.machine.cpu.off_rails_detection.unwrap_or(false),
        ));
        // Load program binary if one was specified in config options
        if let Some(prog_bin) = self.config.emulator.run_bin.clone() {
            if let Some(prog_seg) = self.config.emulator.run_bin_seg {
                if let Some(prog_ofs) = self.config.emulator.run_bin_ofs {
                    if let Some(vreset_seg) = self.config.emulator.vreset_bin_seg {
                        if let Some(vreset_ofs) = self.config.emulator.vreset_bin_ofs {
                            let prog_vec = match std::fs::read(prog_bin.clone()) {
                                Ok(vec) => vec,
                                Err(e) => {
                                    eprintln!("Error opening filename {:?}: {}", prog_bin, e);
                                    std::process::exit(1);
                                }
                            };

                            if let Err(_) = self
                                .machine
                                .load_program(&prog_vec, prog_seg, prog_ofs, vreset_seg, vreset_ofs)
                            {
                                eprintln!(
                                    "Error loading program into memory at {:04X}:{:04X}.",
                                    prog_seg, prog_ofs
                                );
                                std::process::exit(1);
                            };
                        }
                        else {
                            eprintln!("Must specify program start offset.");
                            std::process::exit(1);
                        }
                    }
                    else {
                        eprintln!("Must specify program start segment.");
                        std::process::exit(1);
                    }
                }
                else {
                    eprintln!("Must specify program load offset.");
                    std::process::exit(1);
                }
            }
            else {
                eprintln!("Must specify program load segment.");
                std::process::exit(1);
            }
        }

        self.machine.set_cpu_option(CpuOption::EnableWaitStates(
            self.config.machine.cpu.wait_states.unwrap_or(true),
        ));

        self.machine.set_cpu_option(CpuOption::InstructionHistory(
            self.config.machine.cpu.instruction_history.unwrap_or(false),
        ));

        // Debug mode on?
        if self.config.emulator.debug_mode {
            self.machine.set_cpu_option(CpuOption::InstructionHistory(true));
            // Disable autostart
            self.config.emulator.cpu_autostart = false;
        }

        #[cfg(debug_assertions)]
        if self.config.emulator.debug_warn {
            log::warn!("Debug build. Performance may be affected.");
        }

        Ok(())
    }

    /// Get a list of VHD images specified in the machine configuration.
    /// Returns a vector of Option<String> where Some(String) is the filename of the VHD image, and None is an empty
    /// hard drive slot.
    pub fn get_vhds_from_machine(&self) -> Vec<Option<String>> {
        let mut vhd_names: Vec<Option<String>> = Vec::new();

        let machine_config = self.machine.config();

        if let Some(controller) = machine_config.hdc.as_ref() {
            for drive in controller.drive.as_ref().unwrap_or(&Vec::new()) {
                if let Some(vhd) = drive.vhd.as_ref() {
                    vhd_names.push(Some(vhd.clone()));
                }
                else {
                    vhd_names.push(None);
                }
            }
        }

        vhd_names
    }

    /// Mount VHD images into hard drive devices.
    /// VHD images can be specified either in the machine configuration, or in the main configuration.
    /// Images specified in the main configuration will override images specified in a machine configuration.
    /// Main configuration entries target their explicit drive number, independently of list order.
    pub fn mount_vhds(&mut self) -> Result<(), Error> {
        // First, retrieve the list of VHD images specified in the machine configuration.
        let vhd_names = apply_vhd_overrides(self.get_vhds_from_machine(),
            self.config.emulator.media.vhd.as_deref().unwrap_or(&[]))?;

        let manager = &mut self.vhd_manager;
        mount_named_vhds(&mut self.machine, vhd_names, |drive, name| {
            let (file, _) = manager.load_vhd_file_by_name(drive, &OsString::from(name))?;
            VirtualHardDisk::parse(Box::new(file), false)
        })
    }

    pub fn start(&mut self) {
        //self.machine.play_sound_buffer();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use marty_core::{
        bus::{DeviceRunTimeUnit, IoDevice},
        machine::{MachineBuilder, MachineRomManifest},
        machine_config::{HardDriveControllerConfig, MachineConfiguration},
        machine_types::{HardDiskControllerType, MachineType},
    };
    use marty_frontend_common::machine_manager::MachineConfigFileEntry;
    use serde_json::json;

    fn test_machine(hdc: Option<HardDiskControllerType>) -> Machine {
        let mut config = marty_config::read_config(
            include_str!("../../../../../install/martypc.toml"), Default::default(),
        ).unwrap();
        config.machine.no_roms = true;
        let description = MachineConfiguration {
            machine_type: MachineType::Ibm5160,
            hdc: hdc.map(|hdc_type| HardDriveControllerConfig { hdc_type, drive: None }),
            ..Default::default()
        };
        MachineBuilder::new()
            .with_core_config(Box::new(&config))
            .with_machine_config(&description)
            .with_roms(MachineRomManifest::new())
            .build().unwrap()
    }

    fn disk(cylinders: u16) -> VirtualHardDisk {
        // Independent fixed-VHD fixture with a supported native drive geometry.
        let size = usize::from(cylinders) * 4 * 17 * 512;
        let mut data = vec![0u8; size + 512];
        data[..512].fill(0xa5);
        let f = &mut data[size..];
        f[..8].copy_from_slice(b"conectix");
        f[8..12].copy_from_slice(&2u32.to_be_bytes());
        f[12..16].copy_from_slice(&0x10000u32.to_be_bytes());
        f[16..24].copy_from_slice(&u64::MAX.to_be_bytes());
        f[40..48].copy_from_slice(&(size as u64).to_be_bytes());
        f[48..56].copy_from_slice(&(size as u64).to_be_bytes());
        f[56..58].copy_from_slice(&cylinders.to_be_bytes());
        f[58] = 4;
        f[59] = 17;
        f[60..64].copy_from_slice(&2u32.to_be_bytes());
        let checksum = !f.iter().map(|b| u32::from(*b)).sum::<u32>();
        f[64..68].copy_from_slice(&checksum.to_be_bytes());
        VirtualHardDisk::parse(Box::new(Cursor::new(data)), false).unwrap()
    }

    #[test]
    fn native_controller_variants_attach_configured_disk() {
        for kind in [HardDiskControllerType::IbmXebec, HardDiskControllerType::XtIde,
                     HardDiskControllerType::JrIde] {
            let cylinders = if matches!(kind, HardDiskControllerType::IbmXebec) { 615 } else { 614 };
            let mut machine = test_machine(Some(kind));
            mount_named_vhds(&mut machine, vec![Some("test.vhd".into())],
                             |_, _| Ok(disk(cylinders))).unwrap();
        }
    }

    #[test]
    fn empty_slot_does_not_move_slave_to_master() {
        let mut machine = test_machine(Some(HardDiskControllerType::XtIde));
        let mut loaded = Vec::new();
        mount_named_vhds(&mut machine, vec![None, Some("slave.vhd".into())], |slot, _| {
            loaded.push(slot);
            Ok(disk(614))
        }).unwrap();
        assert_eq!(loaded, vec![1]);
        let mut hdc = machine.xtide_mut().take().unwrap();
        // Complete the native ATA reset before issuing commands. This is a
        // device attachment test, not a CPU/PIT timing or firmware experiment.
        hdc.run(&mut marty_core::devices::dma::DMAController::new(),
                machine.bus_mut(), 200_000.0);
        // Native IDENTIFY must see an attached slave, not merely a success return.
        hdc.write_u8(0x306, 0xb0, None, DeviceRunTimeUnit::Microseconds(0.0), None);
        hdc.write_u8(0x30e, 0xec, None, DeviceRunTimeUnit::Microseconds(0.0), None);
        let status = hdc.read_u8(0x30e, DeviceRunTimeUnit::Microseconds(0.0));
        assert_ne!(status & 8, 0, "slave IDENTIFY must have a data block");
        let bytes: Vec<u8> = (0..512).map(|i| hdc.read_u8(
            0x300 + i % 2, DeviceRunTimeUnit::Microseconds(0.0),
        )).collect();
        assert_eq!(u16::from_le_bytes([bytes[2], bytes[3]]), 614);
        assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 4);
        assert_eq!(u16::from_le_bytes([bytes[12], bytes[13]]), 17);
    }

    #[test]
    fn unusable_configured_disks_are_errors() {
        let mut machine = test_machine(None);
        assert!(mount_named_vhds(&mut machine, vec![Some("test.vhd".into())],
                                |_, _| Ok(disk(614))).is_err());
        let mut machine = test_machine(Some(HardDiskControllerType::XtIde));
        assert!(mount_named_vhds(&mut machine, vec![Some("missing.vhd".into())],
                                |_, _| Err(anyhow!("file missing"))).is_err());
        assert!(mount_named_vhds(&mut machine, vec![Some("unsupported.vhd".into())],
                                |_, _| Ok(disk(1))).is_err());
        assert!(mount_named_vhds(&mut machine, vec![None, None, Some("third.vhd".into())],
                                |_, _| panic!("invalid slot must not load a file")).is_err());
        mount_named_vhds(&mut machine, vec![None, None],
                        |_, _| panic!("empty slots must not load")).unwrap();
    }

    #[test]
    fn native_video_cards_require_their_bios() {
        for (video, feature) in [("VGA", Some("ibm_vga")), ("EGA", Some("ibm_ega")),
                                 ("CGA", None)] {
            let entry: MachineConfigFileEntry = serde_json::from_value(json!({
                "name":"test", "type":"Ibm5160", "rom_set":"auto",
                "memory":{"conventional":{"size":655360,"wait_states":0}},
                "video":[{"type":video}],
            })).unwrap();
            let (required, _) = entry.get_rom_requirements(false).unwrap();
            if let Some(feature) = feature {
                assert!(required.iter().any(|r| r == feature), "{video} BIOS missing");
            } else {
                assert!(!required.iter().any(|r| r == "ibm_vga" || r == "ibm_ega"));
            }
        }
    }

    #[test]
    fn main_config_targets_explicit_drive_numbers() {
        let entry = |drive, name: &str| VhdConfigEntry { drive, filename: name.into() };
        assert_eq!(apply_vhd_overrides(vec![Some("master.vhd".into())],
            &[entry(1, "slave.vhd")]).unwrap(),
            vec![Some("master.vhd".into()), Some("slave.vhd".into())]);
        assert_eq!(apply_vhd_overrides(vec![],
            &[entry(1, "slave.vhd"), entry(0, "master.vhd")]).unwrap(),
            vec![Some("master.vhd".into()), Some("slave.vhd".into())]);
        assert_eq!(apply_vhd_overrides(vec![], &[entry(1, "slave.vhd")]).unwrap(),
            vec![None, Some("slave.vhd".into())]);
        assert!(apply_vhd_overrides(vec![], &[entry(2, "third.vhd")]).is_err());
        assert!(apply_vhd_overrides(vec![], &[entry(0, "a.vhd"), entry(0, "b.vhd")]).is_err());
    }
}
