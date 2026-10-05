use super::*;
use crate::devices::pic::Pic;
use std::sync::atomic::{AtomicUsize, Ordering};

fn device() -> AtaDevice {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target"));
    std::fs::create_dir_all(&target).unwrap();
    let path = target.join(format!(
        "ata-fixture-{}-{}.vhd",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    drop(crate::vhd::create_vhd(path.clone().into_os_string(), 2, 2, 4).unwrap());
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let mut vhd = VirtualHardDisk::parse(Box::new(Cursor::new(bytes)), false).unwrap();
    for c in 0..2 {
        for h in 0..2 {
            for s in 0..4 {
                let data: Vec<_> = (0..512)
                    .map(|i| ((i * 13 + c as usize * 8 + h as usize * 4 + s as usize) & 255) as u8)
                    .collect();
                vhd.write_sector(&data, c, h, s).unwrap();
            }
        }
    }
    let mut device = AtaDevice::new(0, Disk::from_vhd(vhd), None, false, None);
    device.mask_register_write(0);
    device
}

fn capture(device: &mut AtaDevice) -> (AtaDeviceState, Option<Vec<u8>>) {
    device.snapshot_state(DiskCaptureMode::Embed, 0).unwrap()
}

fn roundtrip(reference: &mut AtaDevice) -> AtaDevice {
    let (saved, payload) = capture(reference);
    let saved = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    AtaDevice::prepare_restore(
        &saved,
        payload.map(|bytes| Box::new(Cursor::new(bytes)) as Box<dyn VhdIO>),
    )
    .unwrap()
}

fn native_observations(reference: &mut AtaDevice, restored: &mut AtaDevice) {
    // Observe native register/buffer/disk outputs before comparing the codec.
    assert_eq!(
        reference.error_register_read(),
        restored.error_register_read(),
        "native ATA error"
    );
    assert_eq!(
        reference.status_register_read(),
        restored.status_register_read(),
        "native ATA status"
    );
    for reg in 2..=6 {
        assert_eq!(
            reference.register_read(reg),
            restored.register_read(reg),
            "native ATA register {reg}"
        );
    }
    assert_eq!(
        reference.sector_buffer(),
        restored.sector_buffer(),
        "native ATA buffered bytes"
    );
    assert_eq!(
        reference.sector_buffer_start(),
        restored.sector_buffer_start(),
        "native ATA buffer start"
    );
    assert_eq!(
        reference.sector_buffer_end(),
        restored.sector_buffer_end(),
        "native ATA buffer end"
    );
    assert_eq!(
        reference.disk().map(Disk::position),
        restored.disk().map(Disk::position),
        "native ATA disk CHS"
    );
    assert_eq!(capture(reference), capture(restored));
}

fn bus() -> BusInterface {
    let mut bus = BusInterface::default();
    *bus.pic_mut() = Some(Box::new(Pic::new()));
    bus
}

fn tick(reference: &mut AtaDevice, restored: &mut AtaDevice, us: f64) {
    let mut a = bus();
    let mut b = bus();
    let mut da = dma::DMAController::new();
    let mut db = dma::DMAController::new();
    reference.run(&mut da, &mut a, us);
    restored.run(&mut db, &mut b, us);
    assert_eq!(
        a.pic_mut().as_mut().unwrap().snapshot_state().unwrap(),
        b.pic_mut().as_mut().unwrap().snapshot_state().unwrap()
    );
    assert_eq!(da.snapshot_state().unwrap(), db.snapshot_state().unwrap());
    native_observations(reference, restored);
}

#[test]
fn native_pio_reads_continue_after_partial_buffer_json_restore() {
    let mut checkpoints = 0;
    for command in [0x20, 0x21, 0xC4, 0xEC] {
        for consumed in [0, 1, 2, 3, 255, 511, 512, 513] {
            let mut reference = device();
            reference.sector_count_register_write(3);
            reference.handle_command_register_write(command, None);
            for _ in 0..consumed {
                reference.data_register_read();
            }
            let mut restored = roundtrip(&mut reference);
            for i in consumed..1536 {
                assert_eq!(
                    reference.data_register_read(),
                    restored.data_register_read(),
                    "native PIO read byte {i}"
                );
                if i % 512 == 511 {
                    tick(&mut reference, &mut restored, 0.125);
                }
            }
            native_observations(&mut reference, &mut restored);
            checkpoints += 1;
        }
    }
    assert_eq!(checkpoints, 32);
    println!("ATA:32 native partial PIO-read/identify JSON checkpoints");
}

#[test]
fn native_pio_writes_continue_with_either_half_latched() {
    let mut checkpoints = 0;
    for command in [0x30, 0xC5] {
        for high_first in [false, true] {
            for consumed in [0, 1, 2, 3, 255, 511, 512] {
                let mut reference = device();
                reference.sector_count_register_write(2);
                reference.handle_command_register_write(command, None);
                for i in 0..consumed {
                    reference.data_register_write((i * 7 + 9) as u8, (i % 2 == 0) != high_first);
                    if i % 512 == 511 {
                        reference.run(&mut dma::DMAController::new(), &mut bus(), 0.25);
                    }
                }
                let mut restored = roundtrip(&mut reference);
                for i in consumed..1024 {
                    let byte = (i * 7 + 9) as u8;
                    let low = (i % 2 == 0) != high_first;
                    reference.data_register_write(byte, low);
                    restored.data_register_write(byte, low);
                    if i % 512 == 511 {
                        tick(&mut reference, &mut restored, 0.25);
                    }
                }
                native_observations(&mut reference, &mut restored);
                // Independently parse the written backing, rather than restoring
                // it through the codec under test, and inspect original sectors.
                let bytes = capture(&mut restored).1.unwrap();
                let mut native_disk = VirtualHardDisk::parse(Box::new(Cursor::new(bytes)), false).unwrap();
                for sector in 0..2 {
                    let mut bytes = [0; 512];
                    native_disk.read_sector(&mut bytes, 0, 0, sector).unwrap();
                    for (i, &byte) in bytes.iter().enumerate() {
                        let input_index = sector as usize * 512 + if high_first { i ^ 1 } else { i };
                        assert_eq!(byte, (input_index * 7 + 9) as u8,
                            "native write command={command:02X} consumed={consumed} high_first={high_first} sector={sector} byte={i}");
                    }
                }
                checkpoints += 1;
            }
        }
    }
    assert_eq!(checkpoints, 28);
    println!("ATA:28 native partial low/high-latch writes and independently parsed disk checks");
}

#[test]
fn native_pending_callback_is_independent_of_command_opcode() {
    let callbacks: [CommandDispatchFn; 8] = [
        AtaDevice::command_read_sectors_retry,
        AtaDevice::command_read_sectors,
        AtaDevice::command_write_sectors,
        AtaDevice::command_read_verify_sectors,
        AtaDevice::command_identify_drive,
        AtaDevice::command_read_multiple,
        AtaDevice::command_write_multiple,
        AtaDevice::command_set_multiple_mode,
    ];
    for callback in callbacks {
        let mut reference = device();
        reference.set_command(AtaCommand::Seek, 3, callback);
        reference.process_command_byte(0xA5, None);
        reference.process_command_byte(0x39, None);
        assert_eq!(reference.command_queue, VecDeque::from([0xA5, 0x39]));
        let mut restored = roundtrip(&mut reference);
        reference.process_command_byte(0xF1, None);
        restored.process_command_byte(0xF1, None);
        native_observations(&mut reference, &mut restored);
        for _ in 0..512 {
            assert_eq!(reference.data_register_read(), restored.data_register_read());
        }
        native_observations(&mut reference, &mut restored);
    }
    println!("ATA:8 native pending known callbacks retained independently of opcode");
}

#[test]
fn native_len_plus_one_buffer_cursor_and_fractional_reset_continue() {
    for written in [false, true] {
        let mut reference = AtaDevice::default();
        if written {
            reference.sector_buffer_mark_written();
        } else {
            reference.sector_buffer_mark_read();
        }
        assert_eq!(reference.sector_buffer.position(), 513);
        reference.run(&mut dma::DMAController::new(), &mut bus(), 199999.125);
        let mut restored = roundtrip(&mut reference);
        assert_eq!(restored.sector_buffer.position(), 513);
        tick(&mut reference, &mut restored, 0.5);
        assert!(matches!(restored.state, AtaState::Reset));
        tick(&mut reference, &mut restored, 0.375);
        assert!(matches!(restored.state, AtaState::WaitingForCommand));
    }
    println!("ATA:2 native len+1 cursor/fractional reset JSON checkpoints");
}

#[test]
fn seeded_pending_irq_dma_requests_continue_native_bus_effects() {
    for clear in [false, true] {
        let mut reference = device();
        reference.irq = Some(5);
        reference.dma_channel = Some(3);
        reference.mask_register_write(3);
        reference.handle_command_register_write(0x21, None);
        // Native PIO handlers do not currently produce IRQ or DREQ requests.
        // Explicitly seed those pending consumer inputs; this tests native bus
        // effects, not natural production of an ATA DMA/interrupt operation.
        reference.send_interrupt = true;
        if clear {
            reference.status_register_read();
        } // native clear_interrupt
        reference.send_dreq = true;
        reference.clear_dreq = clear;
        let mut restored = roundtrip(&mut reference);
        tick(&mut reference, &mut restored, 0.125);
        assert_eq!(restored.interrupt_active, !clear);
        assert_eq!(restored.dreq_active, !clear);
    }
    println!("ATA:2 explicitly seeded IRQ/DREQ pending native consumer JSON checkpoints");
}

fn unknown_callback(device: &mut AtaDevice, _: Option<&mut BusInterface>) -> Continuation {
    device.command_lba ^= 0x12345678;
    Continuation::ContinueAsOperation
}

#[test]
fn strict_schema_and_preflight_refuse_without_live_mutation() {
    let mut live = device();
    let before = capture(&mut live);
    let value = serde_json::to_value(&before.0).unwrap();
    for key in value.as_object().unwrap().keys() {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(
            serde_json::from_value::<AtaDeviceState>(missing).is_err(),
            "required {key}"
        );
    }
    let mut extra = value.clone();
    extra["unexpected"] = true.into();
    assert!(serde_json::from_value::<AtaDeviceState>(extra).is_err());
    let mut short = value.clone();
    short["data_register"] = serde_json::json!([null]);
    assert!(serde_json::from_value::<AtaDeviceState>(short).is_err());
    for kind in 0..8 {
        let mut bad = before.0.clone();
        match kind {
            0 => bad.version = 0,
            1 => bad.state = 255,
            2 => bad.command = 255,
            3 => bad.last_command = 255,
            4 => bad.last_error = 255,
            5 => bad.irq = Some(8),
            6 => bad.dma_channel = Some(4),
            _ => {
                bad.sector_buffer.0.pop();
            }
        }
        assert!(AtaDevice::prepare_restore(&bad, None).is_err());
        assert_eq!(capture(&mut live), before);
    }
    assert!(AtaDevice::prepare_restore(&before.0, None).is_err());
    let mut bad_payload = before.1.clone().unwrap();
    bad_payload[0] ^= 1;
    assert!(AtaDevice::prepare_restore(&before.0, Some(Box::new(Cursor::new(bad_payload)))).is_err());
    assert_eq!(capture(&mut live), before);
    let mut empty = AtaDevice::default();
    assert!(AtaDevice::prepare_restore(
        &capture(&mut empty).0,
        Some(Box::new(Cursor::new(before.1.clone().unwrap())))
    )
    .is_err());
    live.command_fn = Some(unknown_callback);
    let disk_before = live
        .disk_mut()
        .unwrap()
        .snapshot_state(DiskCaptureMode::Embed, 0)
        .unwrap();
    assert!(live.snapshot_state(DiskCaptureMode::Embed, 0).is_err());
    assert_eq!(
        live.disk_mut()
            .unwrap()
            .snapshot_state(DiskCaptureMode::Embed, 0)
            .unwrap(),
        disk_before
    );
}

#[test]
fn schema_inventory_and_seeded_storage_cover_native_fields() {
    let fields = |source: &str, name: &str| -> std::collections::BTreeSet<String> {
        source
            .split(&format!("pub struct {name} {{"))
            .nth(1)
            .unwrap()
            .split('\n')
            .skip(1)
            .take_while(|line| *line != "}")
            .filter_map(|line| {
                line.trim()
                    .split_once(':')
                    .map(|(name, _)| name.trim().strip_prefix("pub ").unwrap_or(name.trim()).to_owned())
            })
            .collect()
    };
    let mut reference = device();
    reference.set_error(AtaOperationError::IllegalAccess);
    reference.operation_status = OperationStatus {
        sectors_complete: 3,
        sectors_left: 4,
        block_ct: 5,
        block_n: 6,
        dma_bytes_left: 71,
        dma_byte_count: 83,
    };
    reference.state_accumulator = -0.0;
    reference.status_register = AtaStatusRegister::from_bytes([0xA7]);
    reference.error_register = AtaErrorRegister::from_bytes([0x53]);
    reference.status_reads = 93;
    reference.data_reads = 101;
    reference.data_writes = 109;
    reference.command_result_pending = true;
    reference.data_register.set_hi(0xD3);
    let mut restored = roundtrip(&mut reference);
    assert_eq!(capture(&mut restored), capture(&mut reference)); // explicitly seeded storage-only
    let saved = serde_json::to_value(capture(&mut reference).0).unwrap();
    let mut native = fields(include_str!("../../ata_device.rs"), "AtaDevice");
    native.insert("version".to_owned());
    assert_eq!(
        saved
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        native
    );
    assert_eq!(
        fields(include_str!("../../ata_device.rs"), "OperationStatus"),
        [
            "sectors_complete",
            "sectors_left",
            "block_ct",
            "block_n",
            "dma_bytes_left",
            "dma_byte_count"
        ]
        .map(String::from)
        .into_iter()
        .collect()
    );
    assert_eq!(
        fields(include_str!("../../ata_register16.rs"), "AtaRegister16"),
        ["bytes".to_owned()].into_iter().collect()
    );
}
