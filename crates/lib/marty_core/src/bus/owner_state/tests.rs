use super::*;
use crate::{
    add_io_device, add_mmio_device,
    devices::hdc::xtide::*,
    vhd::{create_vhd, VirtualHardDisk},
};
use std::{
    io::Cursor,
    sync::atomic::{AtomicUsize, Ordering},
};

fn fixture() -> BusInterface {
    fixture_with_drives(1)
}

fn fixture_with_drives(drive_count: usize) -> BusInterface {
    let mut bus = BusInterface::default();
    let machine_type = MachineType::Ibm5160;
    let config = crate::machine_config::MachineConfiguration {
        machine_type,
        ..Default::default()
    };
    bus.install_devices(
        crate::machine_config::get_machine_descriptor(machine_type).unwrap(),
        &config,
        #[cfg(feature = "sound")]
        &crate::sound::SoundOutputConfig::default(),
        None,
        false,
    )
    .unwrap();
    let id = VideoCardId {
        idx: 0,
        vtype: VideoType::CGA,
    };
    let card = CGACard::new(TraceLogger::None, ClockingMode::Cycle, false);
    add_io_device!(bus, card, IoDeviceType::Video(id));
    add_mmio_device!(bus, card, MmioDeviceType::Video(id));
    bus.videocards.insert(id, VideoCardDispatch::Cga(Box::new(card)));
    bus.videocard_ids.push(id);
    bus.keyboard = Some(Keyboard::new(bus.keyboard_type, false));
    let serial = SerialPortController::new(false);
    add_io_device!(bus, serial, IoDeviceType::Serial);
    bus.serial = Some(serial);
    bus.mouse = Some(Mouse::new_serial(0, None));
    let game_port = GamePort::new(None, None);
    add_io_device!(bus, game_port, IoDeviceType::GamePort);
    bus.game_port = Some(game_port);
    // Independent native VHD creation, not the snapshot restorer under test.
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target"));
    std::fs::create_dir_all(&target).unwrap();
    let path = target.join(format!(
        "bus-owner-{}-{}.vhd",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut controller = XtIdeController::new(Some(0x340), drive_count);
    let geometry = controller.get_supported_formats()[0].geometry;
    drop(create_vhd(path.clone().into_os_string(), geometry.c, geometry.h, geometry.s).unwrap());
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let mut disk = VirtualHardDisk::parse(Box::new(Cursor::new(bytes)), false).unwrap();
    disk.write_sector(&(0..512).map(|i| (i * 13 + 7) as u8).collect::<Vec<_>>(), 0, 0, 0)
        .unwrap();
    controller.set_vhd(0, disk).unwrap();
    controller.mask_register_write(0);
    add_io_device!(bus, controller, IoDeviceType::HardDiskController);
    bus.xtide = Some(Box::new(controller));
    bus
}

fn capture(bus: &mut BusInterface) -> (BusState, [Option<Vec<u8>>; 2]) {
    let (state, payloads) = bus.snapshot_bus_state(DiskCaptureMode::Embed, 0).unwrap();
    (
        serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap(),
        payloads,
    )
}
fn providers(data: [Option<Vec<u8>>; 2]) -> [Option<Box<dyn VhdIO>>; 2] {
    data.map(|bytes| bytes.map(|bytes| Box::new(Cursor::new(bytes)) as Box<dyn VhdIO>))
}

fn advance(bus: &mut BusInterface, step: usize) -> (Vec<u8>, String, Vec<u8>) {
    let mut fifo = VecDeque::new();
    bus.write_u8(0x500 + step, (step * 17) as u8, 0).unwrap();
    bus.write_u8(0xB8000 + step, (step * 23) as u8, 0).unwrap();
    if step % 3 == 0 {
        bus.adjust_pit(3);
    }
    let ticks = [1, 2, 5, 8, 13, 34000][step % 6];
    let event = bus.run_devices(ticks as f64 / 14.31818, ticks, None, &mut fifo, None);
    let reads = [
        0x40,
        0x61,
        0xA0,
        0x3DA,
        0x3FD,
        0x201,
        0x340 + HDC_DATA_REGISTER0,
        0x340 + HDC_DATA_REGISTER1,
        0x340 + HDC_STATUS_REGISTER,
    ]
    .into_iter()
    .map(|p| bus.io_read_u8(p, 0))
    .collect();
    let previous = step.saturating_sub(1);
    let memory = [0x500 + step, 0xB8000 + step, 0x500 + previous, 0xB8000 + previous]
        .into_iter()
        .map(|p| bus.read_u8(p, 0).unwrap().0)
        .collect();
    (reads, format!("{event:?}"), memory)
}

#[test]
fn whole_bus_restores_fresh_owners_and_continues_native_io_memory_and_disks() {
    let mut reference = fixture();
    for (port, byte) in [
        (0x43, 0x34),
        (0x40, 19),
        (0x40, 0),
        (0x43, 0x74),
        (0x41, 11),
        (0x41, 0),
        (0x61, 3),
        (0x3FB, 3),
        (0x3FC, 0x1F),
        (0x3F9, 3),
        (0x3F8, 0xA5),
        (0x201, 0),
        (0x340 + HDC_DRIVE_HEAD_REGISTER, 0xA0),
        (0x340 + HDC_SECTOR_COUNT_REGISTER, 1),
        (0x340 + HDC_SECTOR_NUMBER_REGISTER, 1),
        (0x340 + HDC_STATUS_REGISTER, 0x20),
    ] {
        reference.io_write_u8(port, byte, 0, None);
    }
    // Exercise native refresh, not a Pyro startup claim. Clock programmed timers
    // before the first checkpoint, so losing PIT state changes a native port
    // read before the broader storage comparison runs.
    reference.refresh_enabled = true;
    reference.run_devices(512.0 / 14.31818, 512, None, &mut VecDeque::new(), None);
    let mut disk_activity = false;
    for step in 0..16 {
        let (saved, payloads) = capture(&mut reference);
        // No retained source RAM, timers, CGA, disk, UART or mouse.
        let mut restored = fixture().prepare_bus_restore(&saved, providers(payloads)).unwrap();
        let expected = advance(&mut reference, step);
        let actual = advance(&mut restored, step);
        assert_eq!(expected, actual, "native composed bus outputs at step {step}");
        disk_activity |= expected.0[6] != 0;
        let (expected_state, expected_disks) = capture(&mut reference);
        let (actual_state, actual_disks) = capture(&mut restored);
        // Native outputs above are checked first. This additionally catches
        // diagnostic counters, inactive storage and entire writable disk bytes.
        assert!(expected_state == actual_state, "composed storage at step {step}");
        assert_eq!(expected_disks, actual_disks, "whole disk bytes at step {step}");
    }
    assert!(disk_activity);
    println!("BUS_OWNER_NATIVE: 16 fresh-owner JSON restores; native ports/events/RAM/VRAM/disk continuation, no Machine/process proof");
}

#[test]
fn whole_bus_rejects_nested_invalid_state_before_mutation() {
    let mut target = fixture();
    let (saved, payloads) = capture(&mut target);
    let wire = serde_json::to_value(&saved).unwrap();
    for name in wire.as_object().unwrap().keys() {
        let mut invalid = wire.clone();
        invalid.as_object_mut().unwrap().remove(name);
        assert!(serde_json::from_value::<BusState>(invalid).is_err(), "required {name}");
    }
    let mut unknown = wire.clone();
    unknown.as_object_mut().unwrap().insert("unknown".into(), true.into());
    assert!(serde_json::from_value::<BusState>(unknown).is_err());
    for case in 0..11 {
        let mut invalid = wire.clone();
        match case {
            0 => invalid["version"] = 2.into(),
            1 => invalid["memory"]["memory"] = serde_json::json!([]),
            2 => invalid["pit"]["version"] = 2.into(),
            3 => invalid["video"][0][1]["version"] = 2.into(),
            4 => invalid["serial"]["version"] = 2.into(),
            5 => invalid["xtide"]["version"] = 2.into(),
            6 => invalid["mouse"] = serde_json::Value::Null,
            7 => invalid["terminal_port"] = 123.into(),
            8 => invalid["serial"]["port"][0]["irq"] = 7.into(),
            9 => invalid["mouse"]["body"]["state"]["port"] = 1.into(),
            10 => invalid["xtide"]["supported_formats"] = serde_json::json!([]),
            _ => unreachable!(),
        }
        let invalid: BusState = serde_json::from_value(invalid).unwrap();
        assert!(
            fixture()
                .prepare_bus_restore(&invalid, providers(payloads.clone()))
                .is_err(),
            "invalid case {case}"
        );
        assert_eq!(
            capture(&mut target),
            (saved.clone(), payloads.clone()),
            "atomic refusal {case}"
        );
    }
    let mut wrong_bytes = payloads.clone();
    wrong_bytes[0].as_mut().unwrap()[0] ^= 1;
    assert!(fixture().prepare_bus_restore(&saved, providers(wrong_bytes)).is_err());
    assert_eq!(capture(&mut target), (saved, payloads));
    target.speaker_src = Some(0);
    assert!(target.snapshot_bus_state(DiskCaptureMode::Embed, 0).is_err());
    println!("BUS_OWNER_REFUSAL: required/unknown schema, eleven failed candidates; live reference unchanged; disk checksum and external audio owner");
}

#[test]
fn bus_owner_preserves_native_mutable_metadata_and_refuses_orphan_routes_and_audio() {
    let mut reference = fixture();
    // These are real public native setters, not fabricated malformed records.
    reference.copy_from(&[0xB8, 0x34, 0x12], 0x900, 0, false).unwrap();
    reference.set_descriptor(0x920, 8, 3, false);
    let id = reference.videocard_ids[0];
    let Some(VideoCardDispatch::Cga(card)) = reference.videocards.get_mut(&id) else {
        unreachable!()
    };
    card.set_clocking_mode(ClockingMode::Dynamic);
    let (saved, data) = capture(&mut reference);
    let mut restored = fixture().prepare_bus_restore(&saved, providers(data)).unwrap();
    assert_eq!(advance(&mut reference, 0), advance(&mut restored, 0));
    assert!(capture(&mut reference) == capture(&mut restored));
    for route in [
        IoDeviceType::FloppyController,
        IoDeviceType::Parallel,
        IoDeviceType::Video(VideoCardId {
            idx: 99,
            vtype: VideoType::CGA,
        }),
    ] {
        let mut invalid = BusInterface::default();
        invalid.io_map.insert(0x777, route);
        assert!(invalid.snapshot_bus_state(DiskCaptureMode::Embed, 0).is_err());
    }
    for route in [
        MmioDeviceType::Cga,
        MmioDeviceType::Ems,
        MmioDeviceType::Video(VideoCardId {
            idx: 99,
            vtype: VideoType::CGA,
        }),
    ] {
        let mut invalid = BusInterface::default();
        invalid.register_map(route, MemRangeDescriptor::new(0xB8000, MMIO_MAP_SIZE, false));
        assert!(invalid.snapshot_bus_state(DiskCaptureMode::Embed, 0).is_err());
    }
    // Installed owners still cannot service an arbitrary stale routed port.
    let mut wrong_port = fixture();
    let routes: Vec<_> = wrong_port.io_map.values().cloned().collect();
    for route in routes.into_iter().chain([IoDeviceType::Mouse]) {
        wrong_port.io_map.insert(0x777, route);
        assert!(wrong_port.snapshot_bus_state(DiskCaptureMode::Embed, 0).is_err());
    }
    wrong_port.io_map.remove(&0x777);
    assert!(wrong_port.snapshot_bus_state(DiskCaptureMode::Embed, 0).is_ok());
    // A CGA dispatch value must not disguise a different card identity. Exercise
    // I/O, MMIO and traversal guards independently with real CGA owners.
    for case in 0..3 {
        let mut invalid = BusInterface::default();
        let id = VideoCardId {
            idx: 0,
            vtype: VideoType::MDA,
        };
        let card = CGACard::new(TraceLogger::None, ClockingMode::Cycle, false);
        if case == 0 {
            add_io_device!(invalid, card, IoDeviceType::Video(id));
        }
        if case == 1 {
            add_mmio_device!(invalid, card, MmioDeviceType::Video(id));
        }
        invalid.videocards.insert(id, VideoCardDispatch::Cga(Box::new(card)));
        invalid.videocard_ids.push(id);
        let error = invalid
            .snapshot_bus_state(DiskCaptureMode::Embed, 0)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(["I/O route", "MMIO route", "video traversal"][case]),
            "{error}"
        );
    }
    let mut orphan = fixture();
    let (sender, _receiver) = crossbeam_channel::unbounded();
    orphan.pit = Some(Pit::new(
        crate::devices::pit::PitType::Model8253,
        14.31818,
        12,
        Some(sender),
    ));
    assert!(orphan.speaker_src.is_none());
    assert!(orphan.snapshot_bus_state(DiskCaptureMode::Embed, 0).is_err());
    println!("BUS_OWNER_REVIEW: native mutable descriptors/CGA clock continue; six orphan routes, actual owner port lists, three mismatched video identities and orphan PIT sender refused");
}

#[test]
fn composed_disk_requirements_match_native_bytes_modes_read_only_and_unload() {
    use sha2::{Digest, Sha256};
    // Native unload validates the configured drive count; use two real drives.
    let mut bus = fixture_with_drives(2);
    let (saved, data) = capture(&mut bus);
    let req = saved.disk_requirements();
    assert_eq!(req[1], None);
    let raw = data[0].as_ref().unwrap();
    assert_eq!(req[0].as_ref().unwrap().bytes, raw.len() as u64);
    assert_eq!(req[0].as_ref().unwrap().sha256, <[u8; 32]>::from(Sha256::digest(raw)));
    assert!(req[0].as_ref().unwrap().embedded);
    assert!(!req[0].as_ref().unwrap().read_only);
    let readonly = VirtualHardDisk::parse(Box::new(Cursor::new(raw.clone())), true).unwrap();
    bus.xtide.as_mut().unwrap().set_vhd(1, readonly).unwrap();
    let (saved, data) = bus.snapshot_bus_state(DiskCaptureMode::Reference, 0).unwrap();
    assert!(data.iter().all(Option::is_none));
    let req = saved.disk_requirements();
    assert!(!req[0].as_ref().unwrap().embedded);
    assert!(!req[0].as_ref().unwrap().read_only);
    assert!(req[1].as_ref().unwrap().read_only);
    assert_eq!(req[0].as_ref().unwrap().sha256, req[1].as_ref().unwrap().sha256);
    bus.xtide.as_mut().unwrap().unload_vhd(0).unwrap();
    bus.xtide.as_mut().unwrap().unload_vhd(1).unwrap();
    let (saved, data) = bus.snapshot_bus_state(DiskCaptureMode::Embed, 0).unwrap();
    assert_eq!(saved.disk_requirements(), [None, None]);
    assert!(data.iter().all(Option::is_none));
    println!("DISK_REQUIREMENTS:actual mounted bytes/hash, both controller slots, embed/reference policy, readonly and native unload; no archive/process proof");
}
