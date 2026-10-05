//! Snapshot dependencies and derived state for the native RPC-controlled UI.
//! Rendering reads the replacement Machine by stable video-card IDs each frame.
use super::*;
use marty_core::{
    machine::{MachineBuilder, MachineRomManifest},
    machine_config::MachineConfiguration,
    machine_preferences::MachinePreferences,
    sound::SoundOutputConfig,
    vhd::SnapshotRwFile,
};
use marty_debug_rpc::snapshot::SnapshotExecutable;

pub struct GuiSnapshotFactory {
    machine_config: MachineConfiguration,
    preferences: MachinePreferences,
    roms: MachineRomManifest,
    keyboard: Option<String>,
    pub executable: SnapshotExecutable,
}

impl GuiSnapshotFactory {
    pub fn new(
        machine_config: MachineConfiguration,
        preferences: MachinePreferences,
        roms: MachineRomManifest,
        keyboard: Option<String>,
    ) -> std::io::Result<Self> {
        Ok(Self {
            machine_config,
            preferences,
            roms,
            keyboard,
            executable: SnapshotExecutable::current()?,
        })
    }

    pub fn build(&self, config: &ConfigFileParams) -> Result<Machine, String> {
        MachineBuilder::new()
            .with_core_config(Box::new(config))
            .with_machine_config(&self.machine_config)
            .with_machine_preferences(&self.preferences)
            .with_roms(self.roms.clone())
            .with_keyboard_layout(self.keyboard.clone())
            .with_sound_config(SoundOutputConfig {
                enabled: false,
                ..Default::default()
            })
            .build()
            .map_err(|error| error.to_string())
    }
}

impl Emulator {
    pub(super) fn snapshot_vhd_provider(&self, file: std::fs::File, drive: usize) -> Result<Box<dyn VhdIO>, Error> {
        if self.snapshot_factory.is_none() {
            return Ok(Box::new(file));
        }
        let path = self
            .vhd_manager
            .is_drive_loaded(drive)
            .1
            .ok_or_else(|| anyhow::anyhow!("loaded VHD path missing for snapshot provider"))?;
        drop(file);
        Ok(Box::new(SnapshotRwFile::open(&path)?))
    }

    /// Called synchronously on the pump's restore signal, before another request.
    /// Reset frontend-only input/display/cache state; never reapply machine config.
    pub fn refresh_after_snapshot(&mut self) {
        *self.exec_control.borrow_mut() = ExecutionControl::new();
        self.kb_data = KeyboardData::new();
        self.mouse_data = MouseState::new(self.config.emulator.input.reverse_mouse_buttons);
        self.joy_data = JoystickState::new(self.config.emulator.input.joystick_keys.clone());
        self.machine_events.clear();
        self.stat_counter = Counter::new();
        self.perf = Default::default();
        if self.machine.get_state() == MachineState::On {
            self.display_power.power_on();
        } else {
            self.display_power.power_off_immediately();
        }
        // Restored copies are owned by the new Machine, not by the old media
        // selection. RPC hides media controls and disables external file workers.
        for drive in 0..2 {
            self.vhd_manager.release_vhd(drive);
            self.gui.set_hdd_selection(drive, None, None);
        }
        self.gui.set_hdds(self.machine.bus().hdd_ct());
        self.gui.set_machine_state(self.machine.get_state());
        // Renderers reacquire native buffers/extents/mode/palette on the next
        // frame. Their derived pixels and host performance timing are not guest
        // state. No old device handle or host sound player is retained here.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marty_core::{
        device_traits::videocard::VideoType, machine_config::VideoCardConfig, machine_types::MachineType,
        vhd::DiskCaptureMode,
    };

    #[test]
    fn loaded_gui_factory_restores_native_machine_without_audio_output_queues() {
        let mut config =
            marty_config::read_config(include_str!("../../../../../install/martypc.toml"), Default::default()).unwrap();
        config.machine.no_roms = true;
        config.emulator.audio.enabled = false;
        let description = MachineConfiguration {
            machine_type: MachineType::Ibm5160,
            video: vec![VideoCardConfig {
                video_type: VideoType::CGA,
                video_subtype: None,
                dip_switch: None,
                monitor_emulation: true,
            }],
            ..Default::default()
        };
        let factory =
            GuiSnapshotFactory::new(description.clone(), Default::default(), MachineRomManifest::new(), None).unwrap();
        // Construct the reference independently of the GUI factory.
        let mut reference = MachineBuilder::new()
            .with_core_config(Box::new(&config))
            .with_machine_config(&description)
            .with_roms(MachineRomManifest::new())
            .with_sound_config(SoundOutputConfig {
                enabled: false,
                ..Default::default()
            })
            .build()
            .unwrap();
        reference.load_program(&[0x40, 0xeb, 0xfd], 0, 0x100, 0, 0x100).unwrap();
        let mut control = ExecutionControl::new();
        control.set_state(marty_core::machine::ExecutionState::Running);
        reference.run(100, &mut control);
        let (saved, disks) = reference.snapshot_state_quiesced(DiskCaptureMode::Embed, 0).unwrap();
        assert!(disks.iter().all(Option::is_none));
        let mut restored = factory
            .build(&config)
            .unwrap()
            .prepare_snapshot_restore(&saved, [None, None])
            .unwrap();
        assert!(restored.snapshot_state_quiesced(DiskCaptureMode::Embed, 0).unwrap().0 == saved);
        let mut restored_control = ExecutionControl::new();
        restored_control.set_state(marty_core::machine::ExecutionState::Running);
        reference.run(100, &mut control);
        restored.run(100, &mut restored_control);
        assert!(
            restored.snapshot_state_quiesced(DiskCaptureMode::Embed, 0).unwrap()
                == reference.snapshot_state_quiesced(DiskCaptureMode::Embed, 0).unwrap()
        );
        println!("GUI_FACTORY: loaded cold dependencies restore complete no-audio Machine; independent next100 native cycles agree; no GPU/render/restart claim");
    }
}
