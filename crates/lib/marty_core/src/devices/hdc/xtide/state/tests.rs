use super::*;
use crate::device_types::geometry::DriveGeometry;
use std::{
    io::Cursor,
    sync::atomic::{AtomicUsize, Ordering},
};

fn fixture(slave: bool) -> XtIdeController {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target"));
    std::fs::create_dir_all(&target).unwrap();
    let mut controller = XtIdeController::new(Some(0x340), if slave { 2 } else { 1 });
    controller.supported_formats = vec![HardDiskFormat {
        geometry: DriveGeometry {
            c: 2,
            h: 2,
            s: 4,
            s_off: 1,
            size: 512,
        },
        wpc: Some(17),
        desc: "independent two-drive fixture".into(),
    }];
    for drive in 0..if slave { 2 } else { 1 } {
        let path = target.join(format!(
            "xtide-fixture-{}-{}.vhd",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        drop(crate::vhd::create_vhd(path.clone().into_os_string(), 2, 2, 4).unwrap());
        let data = std::fs::read(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let mut vhd = VirtualHardDisk::parse(Box::new(Cursor::new(data)), false).unwrap();
        for c in 0..2 {
            for h in 0..2 {
                for s in 0..4 {
                    let bytes: Vec<_> = (0..512)
                        .map(|i| (drive * 73 + i * 13 + c as usize * 8 + h as usize * 4 + s as usize) as u8)
                        .collect();
                    vhd.write_sector(&bytes, c, h, s).unwrap();
                }
            }
        }
        controller.set_vhd(drive, vhd).unwrap();
    }
    controller
}

fn put(c: &mut XtIdeController, reg: u16, byte: u8) {
    c.write_u8(c.io_base + reg, byte, None, DeviceRunTimeUnit::Microseconds(0.0), None);
}
fn get(c: &mut XtIdeController, reg: u16) -> u8 {
    c.read_u8(c.io_base + reg, DeviceRunTimeUnit::Microseconds(0.0))
}
fn select(c: &mut XtIdeController, drive: usize) {
    put(c, HDC_DRIVE_HEAD_REGISTER, 0xA0 | ((drive as u8) << 4));
}
fn run(c: &mut XtIdeController) {
    c.run(&mut dma::DMAController::new(), &mut BusInterface::default(), 0.25);
}
fn capture(c: &mut XtIdeController) -> (XtIdeState, [Option<Vec<u8>>; 2]) {
    c.snapshot_state(DiskCaptureMode::Embed, 0).unwrap()
}
fn providers(payloads: [Option<Vec<u8>>; 2]) -> [Option<Box<dyn VhdIO>>; 2] {
    payloads.map(|data| data.map(|bytes| Box::new(Cursor::new(bytes)) as Box<dyn VhdIO>))
}
fn restore(c: &mut XtIdeController) -> XtIdeController {
    let (state, payload) = capture(c);
    let state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    XtIdeController::prepare_restore(&state, providers(payload)).unwrap()
}
fn native_controller(a: &XtIdeController, b: &XtIdeController) {
    // Direct native controller fields, independent of snapshot serialization.
    assert_eq!(a.io_base, b.io_base);
    assert_eq!(a.drive_ct, b.drive_ct);
    assert_eq!(a.drive_select, b.drive_select);
    assert_eq!(a.supported_formats, b.supported_formats);
    assert_eq!(a.drive_type_dip, b.drive_type_dip);
    assert_eq!(a.drive_head_register, b.drive_head_register);
    assert_eq!(format!("{:?}", a.last_error), format!("{:?}", b.last_error));
    assert_eq!(a.last_error_drive, b.last_error_drive);
    assert_eq!(a.error_flag, b.error_flag);
    for (a, b) in a.drives.iter().zip(b.drives.iter()) {
        assert_eq!(
            a.disk().map(|d| (d.geometry(), d.position())),
            b.disk().map(|d| (d.geometry(), d.position()))
        );
        assert_eq!(a.sector_buffer(), b.sector_buffer());
    }
}

#[test]
fn two_drive_partial_reads_continue_through_native_ports() {
    let mut checkpoints = 0;
    for consumed in [0, 1, 3, 511, 512, 513] {
        for selected in 0..2 {
            let mut a = fixture(true);
            for drive in 0..2 {
                select(&mut a, drive);
                a.mask_register_write(0);
                put(&mut a, HDC_SECTOR_COUNT_REGISTER, 2);
                put(&mut a, HDC_STATUS_REGISTER, 0x21);
                for i in 0..consumed {
                    get(&mut a, HDC_DATA_REGISTER0);
                    if i % 512 == 511 {
                        run(&mut a);
                    }
                }
            }
            select(&mut a, selected);
            let mut b = restore(&mut a);
            native_controller(&a, &b);
            // Read the selected drive before writing any new selection. Otherwise
            // a lost selected-drive restore could be concealed by the test itself.
            assert_eq!(
                get(&mut a, HDC_DATA_REGISTER0),
                get(&mut b, HDC_DATA_REGISTER0),
                "pending selected drive byte"
            );
            for drive in 0..2 {
                select(&mut a, drive);
                select(&mut b, drive);
                for _ in 0..1024 {
                    assert_eq!(
                        get(&mut a, HDC_DATA_REGISTER0),
                        get(&mut b, HDC_DATA_REGISTER0),
                        "native drive{drive} read"
                    );
                    run(&mut a);
                    run(&mut b);
                }
                assert_eq!(get(&mut a, HDC_STATUS_REGISTER), get(&mut b, HDC_STATUS_REGISTER));
            }
            native_controller(&a, &b);
            assert_eq!(capture(&mut a), capture(&mut b));
            checkpoints += 1;
        }
    }
    assert_eq!(checkpoints, 12);
    println!("XTIDE:12 dual-drive partial-read native port JSON checkpoints");
}

#[test]
fn two_drive_partial_writes_reach_independently_parsed_backings() {
    let mut checkpoints = 0;
    for consumed in [1, 255, 511] {
        for high_first in [false, true] {
            let mut a = fixture(true);
            let data_port = |i: usize| {
                if (i % 2 == 0) != high_first {
                    HDC_DATA_REGISTER0
                } else {
                    HDC_DATA_REGISTER1
                }
            };
            for drive in 0..2 {
                select(&mut a, drive);
                a.mask_register_write(0);
                put(&mut a, HDC_SECTOR_COUNT_REGISTER, 2);
                put(&mut a, HDC_STATUS_REGISTER, 0x30);
                for i in 0..consumed {
                    put(&mut a, data_port(i), (drive * 73 + i * 7 + 9) as u8);
                }
            }
            // The native selection is slave here; retain it through restoration.
            let mut b = restore(&mut a);
            native_controller(&a, &b);
            for drive in 0..2 {
                select(&mut a, drive);
                select(&mut b, drive);
                for i in consumed..1024 {
                    let byte = (drive * 73 + i * 7 + 9) as u8;
                    put(&mut a, data_port(i), byte);
                    put(&mut b, data_port(i), byte);
                    if i % 512 == 511 {
                        run(&mut a);
                        run(&mut b);
                    }
                }
            }
            native_controller(&a, &b);
            assert_eq!(capture(&mut a), capture(&mut b));
            for (drive, payload) in capture(&mut b).1.into_iter().enumerate() {
                let mut vhd = VirtualHardDisk::parse(Box::new(Cursor::new(payload.unwrap())), false).unwrap();
                for sector in 0..2 {
                    let mut bytes = [0; 512];
                    vhd.read_sector(&mut bytes, 0, 0, sector).unwrap();
                    for (i, byte) in bytes.into_iter().enumerate() {
                        let input = sector as usize * 512 + if high_first { i ^ 1 } else { i };
                        assert_eq!(
                            byte,
                            (drive * 73 + input * 7 + 9) as u8,
                            "native drive{drive} written sector{sector} byte{i}"
                        );
                    }
                }
            }
            checkpoints += 1;
        }
    }
    assert_eq!(checkpoints, 6);
    println!("XTIDE:6 dual-drive partial-latch writes with independently parsed sectors");
}

#[test]
fn empty_slave_probe_and_unloaded_native_disk_survive_json_restore() {
    let mut a = fixture(false);
    select(&mut a, 1);
    assert_eq!(a.drive_ct, 1);
    assert_eq!(a.drive_select, 1);
    let mut b = restore(&mut a);
    native_controller(&a, &b);
    assert_eq!(get(&mut a, HDC_STATUS_REGISTER), get(&mut b, HDC_STATUS_REGISTER));
    assert_eq!(get(&mut a, HDC_DATA_REGISTER0), get(&mut b, HDC_DATA_REGISTER0));
    select(&mut a, 0);
    a.unload_vhd(0).unwrap();
    let mut b = restore(&mut a);
    native_controller(&a, &b);
    assert!(b.drives[0].disk().is_some()); // Native unload retains the Disk wrapper.
    assert!(b.drives[0].disk().unwrap().vhd().is_none()); // Independent native backing presence.
    let original = capture(&mut a);
    let restored = capture(&mut b);
    assert!(original.1.iter().all(Option::is_none));
    assert!(restored.1.iter().all(Option::is_none));
    assert_eq!(original, restored);
    println!("XTIDE:2 native empty-slave/unloaded-disk JSON checkpoints");
}

#[test]
fn native_mount_accepts_slave_with_count_one_and_restore_preserves_it() {
    // The low-level native API accepts slot1 even when drive_ct is1. Snapshot
    // restoration preserves that existing state; outer configuration policy is
    // a separate concern and must not silently rewrite native disk presence.
    let mut a = fixture(false);
    let mut other = fixture(true);
    let payload = capture(&mut other).1[1].take().unwrap();
    let vhd = VirtualHardDisk::parse(Box::new(Cursor::new(payload)), false).unwrap();
    assert_eq!(a.drive_ct(), 1);
    a.set_vhd(1, vhd).unwrap(); // Actual native setter, not a seeded drive pointer.
    select(&mut a, 1);
    a.mask_register_write(0);
    put(&mut a, HDC_SECTOR_COUNT_REGISTER, 1);
    put(&mut a, HDC_STATUS_REGISTER, 0x21);
    let mut b = restore(&mut a);
    assert_eq!(b.drive_ct(), 1);
    assert!(b.drives[1].disk().unwrap().vhd().is_some());
    for i in 0..512 {
        let original = get(&mut a, HDC_DATA_REGISTER0);
        let restored = get(&mut b, HDC_DATA_REGISTER0);
        assert_eq!(original, (73 + i * 13) as u8, "independent slave sector byte{i}");
        assert_eq!(restored, original, "native mounted-slave restore byte{i}");
    }
    native_controller(&a, &b);
    println!("XTIDE:1 native count1 mounted-slave JSON checkpoint and independently expected sector bytes");
}

#[test]
fn controller_metadata_and_all_error_variants_are_storage_only() {
    for error in [
        ControllerError::NoError,
        ControllerError::InvalidDevice,
        ControllerError::UnsupportedVHD,
        ControllerError::AtaError(AtaError::NoError),
        ControllerError::AtaError(AtaError::InvalidDevice),
        ControllerError::AtaError(AtaError::UnsupportedVHD),
    ] {
        let mut a = fixture(true);
        a.drive_type_dip = 37;
        a.set_error(error, 7);
        a.supported_formats.push(HardDiskFormat {
            geometry: DriveGeometry {
                c: 3,
                h: 1,
                s: 5,
                s_off: 0,
                size: 1024,
            },
            wpc: None,
            desc: "stored capability metadata".into(),
        });
        let b = restore(&mut a);
        native_controller(&a, &b);
    }
    println!("XTIDE:6 explicitly seeded controller-error/capability storage-only JSON checkpoints");
}

#[test]
fn second_disk_refusal_and_strict_schema_leave_live_owner_unchanged() {
    let mut live = fixture(true);
    select(&mut live, 1);
    put(&mut live, HDC_STATUS_REGISTER, 0x21);
    get(&mut live, HDC_DATA_REGISTER0);
    let before = capture(&mut live);
    let json = serde_json::to_value(&before.0).unwrap();
    for key in json.as_object().unwrap().keys() {
        let mut bad = json.clone();
        bad.as_object_mut().unwrap().remove(key);
        assert!(serde_json::from_value::<XtIdeState>(bad).is_err(), "required {key}");
    }
    let mut extra = json.clone();
    extra["unexpected"] = true.into();
    assert!(serde_json::from_value::<XtIdeState>(extra).is_err());
    let mut missing = json.clone();
    missing["supported_formats"][0].as_object_mut().unwrap().remove("wpc");
    assert!(serde_json::from_value::<XtIdeState>(missing).is_err());
    let mut extra = json.clone();
    extra["supported_formats"][0]["unexpected"] = true.into();
    assert!(serde_json::from_value::<XtIdeState>(extra).is_err());
    let mut short = json.clone();
    short["drives"].as_array_mut().unwrap().pop();
    assert!(serde_json::from_value::<XtIdeState>(short).is_err());
    for kind in 0..5 {
        let mut state = before.0.clone();
        let mut payload = before.1.clone();
        match kind {
            0 => state.version = 0,
            1 => state.drive_ct = 3,
            2 => state.drive_select = 2,
            3 => payload[1] = None,
            _ => payload[1].as_mut().unwrap()[0] ^= 1,
        }
        assert!(XtIdeController::prepare_restore(&state, providers(payload)).is_err());
        assert_eq!(live.drive_select, 1);
        assert_eq!(live.drive_head_register, 0xB0);
        assert_eq!(capture(&mut live), before, "live owner unchanged on refusal{kind}");
    }
    let native: std::collections::BTreeSet<_> = include_str!("../../xtide.rs")
        .split("pub struct XtIdeController {")
        .nth(1)
        .unwrap()
        .lines()
        .skip(1)
        .take_while(|line| line.trim() != "}")
        .filter_map(|line| line.trim().split_once(':').map(|(name, _)| name.to_owned()))
        .collect();
    let keys: std::collections::BTreeSet<_> = json
        .as_object()
        .unwrap()
        .keys()
        .filter(|k| k.as_str() != "version")
        .cloned()
        .collect();
    assert_eq!(keys, native);
}
