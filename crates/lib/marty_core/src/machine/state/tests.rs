use super::*;
use crate::{
    cpu_common::{CpuAddress, Register16},
    cpu_validator::ValidatorType,
    device_traits::videocard::VideoType,
    machine_config::VideoCardConfig,
};
use marty_common::types::joystick::ControllerLayout;
struct NoRoms(MachineType);
impl CoreConfig for NoRoms {
    fn get_base_dir(&self) -> PathBuf {
        PathBuf::new()
    }
    fn get_machine_type(&self) -> MachineType {
        self.0
    }
    fn get_audio_enabled(&self) -> bool {
        false
    }
    fn get_machine_noroms(&self) -> bool {
        false
    }
    fn get_machine_turbo(&self) -> bool {
        false
    }
    fn get_service_interrupt(&self) -> Option<u8> {
        Some(0xFA)
    }
    fn get_keyboard_layout(&self) -> Option<String> {
        None
    }
    fn get_keyboard_debug(&self) -> bool {
        false
    }
    fn get_validator_type(&self) -> Option<ValidatorType> {
        None
    }
    fn get_validator_trace_file(&self) -> Option<PathBuf> {
        None
    }
    fn get_validator_baud(&self) -> Option<u32> {
        None
    }
    fn get_validator_port(&self) -> Option<String> {
        None
    }
    fn get_cpu_trace_mode(&self) -> Option<TraceMode> {
        None
    }
    fn get_cpu_dram_refresh_simulation(&self) -> bool {
        false
    }
    fn get_cpu_trace_on(&self) -> bool {
        false
    }
    fn get_cpu_trace_file(&self) -> Option<PathBuf> {
        None
    }
    fn get_title_hacks(&self) -> bool {
        false
    }
    fn get_patch_enabled(&self) -> bool {
        true
    }
    fn get_halt_behavior(&self) -> OnHaltBehavior {
        OnHaltBehavior::Warn
    }
    fn get_terminal_port(&self) -> Option<u16> {
        None
    }
    fn get_controller_layout(&self) -> Option<ControllerLayout> {
        None
    }
}

fn fixture() -> Machine {
    let core = NoRoms(MachineType::Ibm5160);
    let config = MachineConfiguration {
        machine_type: MachineType::Ibm5160,
        video: vec![VideoCardConfig {
            video_type: VideoType::CGA,
            video_subtype: None,
            dip_switch: None,
            monitor_emulation: true,
        }],
        ..Default::default()
    };
    let manifest = MachineRomManifest {
        roms: vec![MachineRomEntry {
            name: "fixture".into(),
            addr: 0xF0000,
            repeat: 1,
            data: vec![0x90; 16],
            ..Default::default()
        }],
        checkpoints: vec![MachineCheckpoint {
            addr: 0x1000,
            lvl: 2,
            desc: "fixture start".into(),
        }],
        patches: vec![MachinePatch {
            trigger: 0x1000,
            addr: 0x1500,
            bytes: vec![0xA5, 0x5A],
            ..Default::default()
        }],
    };
    let mut machine = MachineBuilder::new()
        .with_core_config(Box::new(&core))
        .with_machine_config(&config)
        .with_roms(manifest)
        .build()
        .unwrap();
    // Native 8088 fixture: CLI; program PIT0 and speaker; fill CGA VRAM through
    // REP STOSW; increment RAM, sample PIT and loop. This is not Pyro gameplay.
    let program = [
        0xFA, 0xB0, 0x34, 0xE6, 0x43, 0xB0, 19, 0xE6, 0x40, 0xB0, 0, 0xE6, 0x40, 0xB0, 3, 0xE6, 0x61, 0xB8, 0, 0xB8,
        0x8E, 0xC0, 0xB8, 0x41, 7, 0xBF, 0, 0, 0xB9, 200, 0, 0xFC, 0xF3, 0xAB, 0xFF, 0x06, 0, 0x14, 0xE4, 0x40, 0xA2,
        2, 0x14, 0xEB, 0xF5,
    ];
    machine.cpu.bus_mut().copy_from(&program, 0x1000, 0, false).unwrap();
    machine.cpu.set_reset_vector(CpuAddress::Segmented(0, 0x1000));
    machine.cpu.reset();
    machine
}

fn capture(machine: &mut Machine) -> MachineSnapshot {
    let (saved, data) = machine.snapshot_state_quiesced(DiskCaptureMode::Embed, 0).unwrap();
    assert!(data.iter().all(Option::is_none));
    serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap()
}

fn native_output(machine: &mut Machine) -> (Vec<u16>, Vec<u8>, Vec<MachineEvent>, Vec<PresentableDeviceEvent>) {
    let mut regs: Vec<u16> = [Register16::AX, Register16::CX, Register16::DI, Register16::CS]
        .into_iter()
        .map(|r| machine.cpu.get_register16(r))
        .collect();
    regs.push(machine.cpu.get_ip());
    let memory = [0x1400, 0x1401, 0x1402, 0x1500, 0x1501, 0xB8000, 0xB8001]
        .into_iter()
        .map(|p| machine.cpu.bus_mut().read_u8(p, 0).unwrap().0)
        .collect();
    let events = machine.events.clone();
    let presentation = machine.presentable_event_receiver.try_iter().collect();
    (regs, memory, events, presentation)
}

#[test]
fn whole_machine_snapshot_restores_fresh_owners_then_runs_native_instructions_and_devices() {
    let mut reference = fixture();
    let mut control = ExecutionControl::new();
    control.set_state(ExecutionState::Running);
    let mut peer_control = ExecutionControl::new();
    peer_control.set_state(ExecutionState::Running);
    let mut guest_ram_changed = false;
    for step in 0..24 {
        let saved = capture(&mut reference);
        let mut restored = fixture().prepare_snapshot_restore(&saved, [None, None]).unwrap();
        assert_eq!(
            reference.run([1, 2, 13, 37, 1000, 8192][step % 6], &mut control),
            restored.run([1, 2, 13, 37, 1000, 8192][step % 6], &mut peer_control),
            "native run cycles {step}"
        );
        let expected = native_output(&mut reference);
        let actual = native_output(&mut restored);
        assert_eq!(expected, actual, "native whole Machine outputs {step}");
        guest_ram_changed |= expected.1[0] != 0;
        assert!(
            serde_json::to_value(capture(&mut reference)).unwrap()
                == serde_json::to_value(capture(&mut restored)).unwrap(),
            "complete captured core storage {step}"
        );
    }
    assert!(guest_ram_changed);
    assert!(reference.rom_manifest.patches[0].installed);
    assert_eq!(reference.cpu.bus_mut().read_u8(0x1500, 0).unwrap().0, 0xA5);
    assert_eq!(reference.cpu.bus_mut().read_u8(0xB8000, 0).unwrap().0, 0x41);
    println!("MACHINE_NATIVE:24 fresh configured CPU/bus/service/core owners; native instruction/device cycles, registers/RAM/VRAM/events, ROM patch installation and full captured storage; no disk/frontend/process/Pyro proof");
}

#[test]
fn whole_machine_snapshot_rejects_bad_dependencies_or_nested_state_and_keeps_live_owner() {
    let mut live = fixture();
    let saved = capture(&mut live);
    let wire = serde_json::to_value(&saved).unwrap();
    for name in wire.as_object().unwrap().keys() {
        let mut invalid = wire.clone();
        invalid.as_object_mut().unwrap().remove(name);
        assert!(
            serde_json::from_value::<MachineSnapshot>(invalid).is_err(),
            "required {name}"
        );
    }
    let mut unknown = wire.clone();
    unknown["unknown"] = true.into();
    assert!(serde_json::from_value::<MachineSnapshot>(unknown).is_err());
    for case in 0..8 {
        let mut invalid = wire.clone();
        match case {
            0 => invalid["version"] = 2.into(),
            1 => invalid["binding"] = "wrong".into(),
            2 => invalid["patch_installed"] = serde_json::json!([]),
            3 => invalid["cpu_factor"] = serde_json::json!({"Divisor":0}),
            4 => invalid["checkpoint_map"]["4096"] = 1.into(),
            5 => invalid["bus"]["memory"]["memory"] = serde_json::json!([]),
            6 => invalid["cpu"]["version"] = 2.into(),
            _ => invalid["service"]["version"] = 2.into(),
        }
        assert!(
            fixture()
                .prepare_snapshot_restore(&serde_json::from_value(invalid).unwrap(), [None, None])
                .is_err(),
            "case {case}"
        );
        assert!(serde_json::to_value(capture(&mut live)).unwrap() == wire);
    }
    let mut wrong_rom = fixture();
    wrong_rom.rom_manifest.roms[0].data[0] ^= 1;
    assert!(wrong_rom.prepare_snapshot_restore(&saved, [None, None]).is_err());
    let mut wrong_config = fixture();
    wrong_config.machine_config.memory.conventional.wait_states ^= 1;
    assert!(wrong_config.prepare_snapshot_restore(&saved, [None, None]).is_err());
    live.options.record_listing = true;
    assert!(live.snapshot_state_quiesced(DiskCaptureMode::Embed, 0).is_err());
    println!("MACHINE_REFUSAL:required/unknown schema;8 failed candidates; changed original ROM/config refused; separate live core unchanged; logging unsupported");
}

#[test]
fn whole_machine_snapshot_continues_pending_turbo_and_presentation_consumers() {
    let mut reference = fixture();
    let mut control = ExecutionControl::new();
    control.set_state(ExecutionState::Running);
    reference.set_turbo_mode(false);
    assert!(reference.run(8192, &mut control) > 0);
    for step in 0..12 {
        reference.set_turbo_mode(step % 2 == 0);
        // Seed pending frontend events on the real native channel. This proves
        // queue continuation, not that these events originated in a guest.
        reference
            .presentable_event_sender
            .send(PresentableDeviceEvent::PowerOff)
            .unwrap();
        reference
            .presentable_event_sender
            .send(PresentableDeviceEvent::PowerOn)
            .unwrap();
        let saved = capture(&mut reference);
        let mut restored = fixture().prepare_snapshot_restore(&saved, [None, None]).unwrap();
        let mut peer_control = ExecutionControl::new();
        peer_control.set_state(ExecutionState::Running);
        assert_eq!(reference.run(4096, &mut control), restored.run(4096, &mut peer_control));
        assert_eq!(
            reference.pit_cycles(),
            restored.pit_cycles(),
            "native pending speed PIT cycles {step}"
        );
        assert_eq!(
            native_output(&mut reference),
            native_output(&mut restored),
            "native pending presentation outputs {step}"
        );
        assert!(
            serde_json::to_value(capture(&mut reference)).unwrap()
                == serde_json::to_value(capture(&mut restored)).unwrap()
        );
    }
    println!("MACHINE_PENDING:12 native pending-turbo PIT continuations and seeded native presentation channel consumers; no guest event origin/frontend/process proof");
}

#[test]
fn whole_machine_review_rejects_unsupported_listing_and_inconsistent_clock() {
    let mut reference = fixture();
    let mut control = ExecutionControl::new();
    control.set_state(ExecutionState::Running);
    reference.run(8192, &mut control);
    // Installed flags describe an action, not immutable RAM: native guest/host
    // RAM writes after installation are allowed. Do not require patch bytes.
    reference
        .cpu
        .bus_mut()
        .copy_from(&[0x11, 0x22], 0x1500, 0, false)
        .unwrap();
    assert!(reference.rom_manifest.patches[0].installed);
    let saved = capture(&mut reference);
    assert!(fixture().prepare_snapshot_restore(&saved, [None, None]).is_ok());
    // Native reinstall_roms replaces the manifest without rebuilding maps.
    // Preserve these native historical maps instead of forcing constructor maps.
    let mut changed = reference.rom_manifest.clone();
    changed.checkpoints[0].addr = 0x2000;
    changed.patches[0].trigger = 0x2000;
    reference.reinstall_roms(changed.clone()).unwrap();
    let saved = capture(&mut reference);
    assert!(reference.checkpoint_map.contains_key(&0x1000));
    let mut fresh = fixture();
    fresh.reinstall_roms(changed).unwrap();
    assert!(fresh.prepare_snapshot_restore(&saved, [None, None]).is_ok());
    let wire = serde_json::to_value(saved).unwrap();
    let mut accepted = Vec::new();
    for case in 0..3 {
        let mut invalid = wire.clone();
        match case {
            0 => invalid["options"]["record_listing"] = true.into(),
            1 => invalid["cpu_clock_period"] = 0_u64.into(),
            _ => invalid["cpu_clock_period"] = (wire["cpu_clock_period"].as_u64().unwrap() + 1).into(),
        }
        let mut candidate = fixture();
        candidate.reinstall_roms(reference.rom_manifest.clone()).unwrap();
        let result = candidate.prepare_snapshot_restore(&serde_json::from_value(invalid).unwrap(), [None, None]);
        if result.is_ok() {
            accepted.push(case);
        }
    }
    println!("MACHINE_REVIEW_BAD_ACCEPTED:{accepted:?}");
    assert!(
        accepted.is_empty(),
        "unsupported listing/zero/inconsistent native clock accepted"
    );
}
