use super::*;
use crate::devices::hdc::xtide::*;
use crate::machine::storage::*;
use crate::{
    cpu_common::{CpuAddress, Register16},
    cpu_validator::ValidatorType,
    device_traits::videocard::VideoType,
    machine_config::VideoCardConfig,
};
use marty_common::types::joystick::ControllerLayout;
use std::io::{Cursor, Write};
struct NoRoms(MachineType, bool);
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
        self.1
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
    fixture_with_manifest(None)
}

fn fixture_with_manifest(override_manifest: Option<MachineRomManifest>) -> Machine {
    fixture_profile(override_manifest, false)
}

fn fixture_profile(override_manifest: Option<MachineRomManifest>, disks: bool) -> Machine {
    fixture_clock_profile(override_manifest, disks, false)
}

fn fixture_clock_profile(override_manifest: Option<MachineRomManifest>, disks: bool, turbo: bool) -> Machine {
    let core = NoRoms(MachineType::Ibm5160, turbo);
    let config = MachineConfiguration {
        machine_type: MachineType::Ibm5160,
        hdc: disks.then_some(crate::machine_config::HardDriveControllerConfig {
            hdc_type: crate::machine_types::HardDiskControllerType::XtIde,
            drive: None,
        }),
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
    let manifest = override_manifest.unwrap_or(manifest);
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
    let mut overwritten = fixture().prepare_snapshot_restore(&saved, [None, None]).unwrap();
    assert_eq!(overwritten.cpu.bus_mut().read_u8(0x1500, 0).unwrap().0, 0x11);
    assert_eq!(overwritten.cpu.bus_mut().read_u8(0x1501, 0).unwrap().0, 0x22);
    // Native reinstall_roms replaces the manifest without rebuilding maps.
    // Preserve these native historical maps instead of forcing constructor maps.
    let mut changed = reference.rom_manifest.clone();
    changed.checkpoints[0].addr = 0x2000;
    changed.checkpoints[0].lvl = 7;
    changed.patches[0].trigger = 0x2000;
    reference.reinstall_roms(changed.clone()).unwrap();
    // The earlier queued hit still has its original level2, even though the
    // current manifest now says7. Level equality would erase legitimate history.
    assert!(reference.events.contains(&MachineEvent::CheckpointHit(0, 2)));
    reference.cpu.reset();
    let saved = capture(&mut reference);
    assert!(reference.checkpoint_map.contains_key(&0x1000));
    let fresh = fixture_with_manifest(Some(changed));
    assert!(fresh.checkpoint_map.contains_key(&0x2000));
    assert!(!fresh.checkpoint_map.contains_key(&0x1000));
    let mut restored = fresh.prepare_snapshot_restore(&saved, [None, None]).unwrap();
    assert!(restored.checkpoint_map.contains_key(&0x1000));
    assert!(restored.patch_map.contains_key(&0x1000));
    let mut peer_control = ExecutionControl::new();
    peer_control.set_state(ExecutionState::Running);
    assert_eq!(reference.run(1, &mut control), restored.run(1, &mut peer_control));
    assert!(restored.events.contains(&MachineEvent::CheckpointHit(0, 7)));
    assert!(restored.events.contains(&MachineEvent::CheckpointHit(0, 2)));
    assert_eq!(
        native_output(&mut reference),
        native_output(&mut restored),
        "native historical map execution"
    );
    let wire = serde_json::to_value(saved).unwrap();
    let mut accepted = Vec::new();
    for case in 0..4 {
        let mut invalid = wire.clone();
        match case {
            0 => invalid["options"]["record_listing"] = true.into(),
            1 => invalid["cpu_clock_period"] = 0_u64.into(),
            2 => invalid["cpu_clock_period"] = (wire["cpu_clock_period"].as_u64().unwrap() + 1).into(),
            _ => invalid["events"] = serde_json::json!([{"CheckpointHit":[99,2]}]),
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

fn archive_disk_fixture() -> Machine {
    let mut machine = fixture_profile(None, true);
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target"));
    std::fs::create_dir_all(&target).unwrap();
    let path = target.join(format!("archive-fixture-{}.vhd", uuid::Uuid::new_v4()));
    let controller = machine.cpu.bus_mut().xtide_mut().as_mut().unwrap();
    let geometry = controller.get_supported_formats()[0].geometry;
    drop(crate::vhd::create_vhd(path.clone().into_os_string(), geometry.c, geometry.h, geometry.s).unwrap());
    let raw = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let mut disk = crate::vhd::VirtualHardDisk::parse(Box::new(Cursor::new(raw)), false).unwrap();
    disk.write_sector(&(0..512).map(|i| (i * 13 + 7) as u8).collect::<Vec<_>>(), 0, 0, 0)
        .unwrap();
    let (_, bytes) = disk.snapshot_state(DiskCaptureMode::Embed, 0).unwrap();
    let readonly = crate::vhd::VirtualHardDisk::parse(Box::new(Cursor::new(bytes.unwrap())), true).unwrap();
    controller.set_vhd(0, disk).unwrap();
    controller.set_vhd(1, readonly).unwrap();
    controller.mask_register_write(0);
    machine
}

fn archive_providers(disks: [Option<Vec<u8>>; 2]) -> [Option<Box<dyn VhdIO>>; 2] {
    disks.map(|disk| disk.map(|bytes| Box::new(Cursor::new(bytes)) as Box<dyn VhdIO>))
}

#[test]
fn persisted_archive_closes_reopens_and_restores_native_machine_with_partial_ata_read() {
    use sha2::{Digest, Sha256};
    let mut original = archive_disk_fixture();
    for (register, byte) in [
        (HDC_DRIVE_HEAD_REGISTER, 0xA0),
        (HDC_SECTOR_COUNT_REGISTER, 1),
        (HDC_SECTOR_NUMBER_REGISTER, 1),
        (HDC_STATUS_REGISTER, 0x20),
    ] {
        original
            .cpu
            .bus_mut()
            .io_write_u8(DEFAULT_IO_BASE + register, byte, 0, None);
    }
    for i in 0..17 {
        let port = DEFAULT_IO_BASE
            + if i % 2 == 0 {
                HDC_DATA_REGISTER0
            } else {
                HDC_DATA_REGISTER1
            };
        assert_eq!(original.cpu.bus_mut().io_read_u8(port, 0), (i * 13 + 7) as u8);
    }
    let (saved, disks) = original.snapshot_state_quiesced(DiskCaptureMode::Embed, 0).unwrap();
    let build = [0x31; 32];
    let (archive, checksum) = encode_snapshot_archive(&saved, &disks, build, SnapshotArchiveLimits::default()).unwrap();
    assert!(archive.len() < disks[0].as_ref().unwrap().len() / 2);
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target"));
    let path = target.join(format!("machine-archive-{}.zip", uuid::Uuid::new_v4()));
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.write_all(&archive).unwrap();
        file.sync_all().unwrap();
    }
    let reopened = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(<[u8; 32]>::from(Sha256::digest(&reopened)), checksum);
    let decoded = decode_snapshot_archive(
        &reopened,
        checksum,
        build,
        [None, None],
        SnapshotArchiveLimits::default(),
    )
    .unwrap();
    assert_eq!(decoded.machine, saved);
    assert_eq!(decoded.disks, disks);
    let requirements = decoded.machine.disk_requirements();
    assert!(!requirements[0].as_ref().unwrap().read_only);
    assert!(requirements[1].as_ref().unwrap().read_only);
    let mut restored = fixture_profile(None, true)
        .prepare_snapshot_restore(&decoded.machine, archive_providers(decoded.disks))
        .unwrap();
    for i in 17..512 {
        let port = DEFAULT_IO_BASE
            + if i % 2 == 0 {
                HDC_DATA_REGISTER0
            } else {
                HDC_DATA_REGISTER1
            };
        let expected = (i * 13 + 7) as u8;
        assert_eq!(original.cpu.bus_mut().io_read_u8(port, 0), expected);
        assert_eq!(restored.cpu.bus_mut().io_read_u8(port, 0), expected);
    }
    let mut a = ExecutionControl::new();
    a.set_state(ExecutionState::Running);
    let mut b = ExecutionControl::new();
    b.set_state(ExecutionState::Running);
    for cycles in [13, 1000, 8192] {
        assert_eq!(original.run(cycles, &mut a), restored.run(cycles, &mut b));
        assert_eq!(native_output(&mut original), native_output(&mut restored));
    }
    assert_eq!(
        original.snapshot_state_quiesced(DiskCaptureMode::Embed, 0).unwrap(),
        restored.snapshot_state_quiesced(DiskCaptureMode::Embed, 0).unwrap()
    );
    println!("ARCHIVE_NATIVE:File sync/close/reopen, two mounted VHDs and cached read_only flag (metadata only), partial ATA byte17 continuation with independent sector pattern, CPU/PIT/CGA cycles and full captured storage; no fresh-process/Pyro/frontend proof");
}

#[test]
fn archive_references_require_exact_bytes_and_all_refusals_leave_live_machine_unchanged() {
    let mut live = archive_disk_fixture();
    let (before, raw) = live.snapshot_state_quiesced(DiskCaptureMode::Embed, 0).unwrap();
    let (saved, no_payloads) = live.snapshot_state_quiesced(DiskCaptureMode::Reference, 0).unwrap();
    let build = [0x32; 32];
    let limits = SnapshotArchiveLimits::default();
    let (archive, checksum) = encode_snapshot_archive(&saved, &no_payloads, build, limits).unwrap();
    assert!(decode_snapshot_archive(&archive, checksum, build, [None, None], limits).is_err());
    for slot in 0..2 {
        let mut wrong = raw.clone();
        wrong[slot].as_mut().unwrap()[7] ^= 1;
        assert!(decode_snapshot_archive(&archive, checksum, build, wrong, limits).is_err());
    }
    let decoded = decode_snapshot_archive(&archive, checksum, build, raw.clone(), limits).unwrap();
    let mut restored = fixture_profile(None, true)
        .prepare_snapshot_restore(&decoded.machine, archive_providers(decoded.disks))
        .unwrap();
    assert_eq!(
        restored
            .snapshot_state_quiesced(DiskCaptureMode::Reference, 0)
            .unwrap()
            .0,
        saved
    );
    assert_eq!(
        live.snapshot_state_quiesced(DiskCaptureMode::Embed, 0).unwrap(),
        (before, raw)
    );
    assert!(encode_snapshot_archive(&saved, &[Some(vec![0; 512]), None], build, limits).is_err());
    println!("ARCHIVE_REFERENCE:both required disks hash-checked, altered byte in either slot/missing refs refused; supplied original matching bytes restore; independent live owner unchanged");
}

// Repack with a different writer: controls alter real archive members, never
// the production decoder or an expected string copied out of that decoder.
fn mutate_archive(raw: &[u8], case: usize) -> Vec<u8> {
    use std::io::Read;
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};
    let mut src = ZipArchive::new(Cursor::new(raw)).unwrap();
    let mut dst = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for index in 0..src.len() {
        let mut file = src.by_index(index).unwrap();
        let name = file.name().to_owned();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        if case == 0 && name == "machine.json" {
            bytes.push(b' ');
        }
        if name == "manifest.json" && (1..=3).contains(&case) {
            let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            match case {
                1 => value["build"][0] = 0.into(),
                2 => value["version"] = 2.into(),
                _ => value["unknown"] = true.into(),
            }
            bytes = serde_json::to_vec(&value).unwrap();
        }
        if case == 4 && name == "machine.json" {
            continue;
        }
        dst.start_file(&name, options).unwrap();
        dst.write_all(&bytes).unwrap();
        if case == 5 && name == "manifest.json" {
            dst.start_file("manifest.jsox", options).unwrap();
            dst.write_all(&bytes).unwrap();
        }
    }
    if case == 6 {
        dst.start_file("../outside", options).unwrap();
        dst.write_all(b"no extraction").unwrap();
    }
    let mut bytes = dst.finish().unwrap().into_inner();
    if case == 5 {
        // zip2 rejects duplicate names on write but collapses them on read.
        // Change equal-length local/central names after writing a valid archive.
        let positions: Vec<_> = bytes
            .windows(13)
            .enumerate()
            .filter_map(|(i, part)| (part == b"manifest.jsox").then_some(i))
            .collect();
        assert_eq!(positions.len(), 2);
        for i in positions {
            bytes[i..i + 13].copy_from_slice(b"manifest.json");
        }
    }
    bytes
}

#[test]
fn archive_refuses_corrupt_metadata_wrong_build_duplicate_paths_and_byte_budgets() {
    use sha2::{Digest, Sha256};
    let mut live = fixture();
    let saved = capture(&mut live);
    let build = [0x33; 32];
    let limits = SnapshotArchiveLimits::default();
    let (archive, checksum) = encode_snapshot_archive(&saved, &[None, None], build, limits).unwrap();
    assert!(decode_snapshot_archive(&archive, [0; 32], build, [None, None], limits).is_err());
    assert!(decode_snapshot_archive(&archive, checksum, [0; 32], [None, None], limits).is_err());
    for case in 0..7 {
        let mutant = mutate_archive(&archive, case);
        // Original retained digest catches tampering before ZIP/JSON is trusted.
        assert!(decode_snapshot_archive(&mutant, checksum, build, [None, None], limits).is_err());
        // Even an independently supplied new digest cannot permit inconsistent
        // members, wrong build/version, loose schema or unexpected/duplicate paths.
        let digest: [u8; 32] = Sha256::digest(&mutant).into();
        assert!(
            decode_snapshot_archive(&mutant, digest, build, [None, None], limits).is_err(),
            "case {case}"
        );
    }
    for reduced in [
        SnapshotArchiveLimits {
            archive_bytes: 1,
            ..limits
        },
        SnapshotArchiveLimits {
            metadata_bytes: 1,
            ..limits
        },
        SnapshotArchiveLimits {
            total_bytes: 1,
            ..limits
        },
    ] {
        assert!(decode_snapshot_archive(&archive, checksum, build, [None, None], reduced).is_err());
    }
    assert!(decode_snapshot_archive(&archive, checksum, build, [Some(vec![1]), None], limits).is_err());
    assert_eq!(capture(&mut live), saved);
    assert_eq!(
        decode_snapshot_archive(&archive, checksum, build, [None, None], limits)
            .unwrap()
            .machine,
        saved
    );
    println!("ARCHIVE_REFUSAL:retained external digest/build, altered metadata, required members, strict manifest, duplicate/unknown paths, archive/metadata/total budgets and orphan refs; unchanged positive source and live owner pass");
}

#[cfg(feature = "sound")]
#[test]
fn disabled_host_speaker_queue_preserves_native_timer_and_ppi_execution() {
    use crate::sound::SoundOutputConfig;
    let build = |enabled| {
        let core = NoRoms(MachineType::Ibm5160, false);
        let config = MachineConfiguration {
            machine_type: MachineType::Ibm5160,
            speaker: true,
            ..Default::default()
        };
        let mut machine = MachineBuilder::new()
            .with_core_config(Box::new(&core))
            .with_machine_config(&config)
            .with_roms(MachineRomManifest::new())
            .with_sound_config(SoundOutputConfig { enabled, ..Default::default() })
            .build().unwrap();
        // CLI; PIT2 square wave/reload10; enable PPI speaker/gate; INC AX loop.
        machine.load_program(&[0xfa, 0xb0, 0xb6, 0xe6, 0x43, 0xb0, 10,
            0xe6, 0x42, 0xb0, 0, 0xe6, 0x42, 0xb0, 3, 0xe6, 0x61,
            0x40, 0xeb, 0xfd], 0, 0x100, 0, 0x100).unwrap();
        machine
    };
    let mut enabled = build(true);
    let mut disabled = build(false);
    assert_eq!(enabled.get_sound_sources().len(), 1);
    assert!(disabled.get_sound_sources().is_empty());
    for machine in [&mut enabled, &mut disabled] {
        let mut control = ExecutionControl::new();
        control.set_state(ExecutionState::Running);
        machine.run(10000, &mut control);
    }
    assert_eq!(enabled.cpu_cycles(), disabled.cpu_cycles());
    assert_eq!(enabled.system_ticks(), disabled.system_ticks());
    assert_eq!(enabled.cpu.get_register16(Register16::AX), disabled.cpu.get_register16(Register16::AX));
    assert!(enabled.get_sound_sources()[0].receiver.len() > 0);
    assert_eq!(enabled.bus_mut().ppi_mut().as_mut().unwrap().snapshot_state().unwrap(),
        disabled.bus_mut().ppi_mut().as_mut().unwrap().snapshot_state().unwrap());
    let mut with_audio = serde_json::to_value(enabled.bus_mut().pit_mut().as_mut().unwrap().snapshot_state().unwrap()).unwrap();
    let mut without_audio = serde_json::to_value(disabled.bus_mut().pit_mut().as_mut().unwrap().snapshot_state().unwrap()).unwrap();
    // PitSpeaker.enabled is initialized from sender presence and has no native
    // reads (only snapshot storage); it is also a host output flag.
    assert_eq!(with_audio["speaker_enabled"], true);
    assert_eq!(without_audio["speaker_enabled"], false);
    // Only host sample sink/accumulation/enablement differs. All native channel, gate,
    // latch, reload, phase and timer clocks are compared without exclusions.
    for field in ["speaker_connected", "speaker_enabled", "speaker_buf_bits", "speaker_sample_accum_bits", "speaker_sample_ct"] {
        assert!(with_audio.as_object_mut().unwrap().remove(field).is_some());
        assert!(without_audio.as_object_mut().unwrap().remove(field).is_some());
    }
    assert_eq!(with_audio, without_audio);
    assert!(disabled.snapshot_state_quiesced(DiskCaptureMode::Embed, 0).is_ok());
    assert!(enabled.snapshot_state_quiesced(DiskCaptureMode::Embed, 0).is_err());
    println!("SPEAKER_OUTPUT_POLICY: configured speaker, disabled host queue; native CPU/PPI/PIT clocks and channels equal to audible branch after10000 cycles; PCM accumulation/sink intentionally differ; enabled output refused by snapshot preflight");
}

#[test]
fn configured_turbo_synchronizes_machine_bus_and_cga_before_first_instruction() {
    for turbo in [false, true] {
        let mut machine = fixture_clock_profile(None, false, turbo);
        let divisor = if turbo { 1 } else { 3 };
        assert_eq!(machine.cpu_cycles_to_system_ticks(10), 10 * divisor);
        assert_eq!(machine.cpu.bus().cpu_cycles_to_system_ticks(10), 10 * divisor);
        let mut control = ExecutionControl::new();
        control.set_state(ExecutionState::Running);
        // This program performs PIT I/O and REP STOSW into CGA VRAM.
        // Startup turbo previously used inconsistent clocks for dispatch and device I/O.
        assert!(machine.run(8192, &mut control) > 0);
        assert_eq!(machine.cpu.bus_mut().read_u8(0xB8000, 0).unwrap().0, 0x41);
        let saved = capture(&mut machine);
        let mut restored = fixture_clock_profile(None, false, turbo)
            .prepare_snapshot_restore(&saved, [None, None]).unwrap();
        let mut peer = ExecutionControl::new();
        peer.set_state(ExecutionState::Running);
        assert_eq!(machine.run(4096, &mut control), restored.run(4096, &mut peer));
        assert_eq!(serde_json::to_value(capture(&mut machine)).unwrap(),
                   serde_json::to_value(capture(&mut restored)).unwrap());
    }
}
