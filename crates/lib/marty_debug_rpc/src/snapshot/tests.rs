use super::*;
use marty_core::{
    device_traits::videocard::VideoType,
    devices::hdc::xtide::*,
    machine::{MachineBuilder, MachineRomManifest},
    machine_config::{HardDriveControllerConfig, MachineConfiguration, VideoCardConfig},
    machine_types::{HardDiskControllerType, MachineType},
    vhd::{create_vhd, VirtualHardDisk},
};
use std::sync::atomic::AtomicU64;

fn cold(disks: bool) -> Machine {
    let mut config =
        marty_config::read_config(include_str!("../../../../../install/martypc.toml"), Default::default()).unwrap();
    config.machine.no_roms = true;
    let description = MachineConfiguration {
        machine_type: MachineType::Ibm5160,
        hdc: disks.then_some(HardDriveControllerConfig {
            hdc_type: HardDiskControllerType::XtIde,
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
    MachineBuilder::new()
        .with_core_config(Box::new(&config))
        .with_machine_config(&description)
        .with_roms(MachineRomManifest::new())
        .build()
        .unwrap()
}
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target"));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let path = root.join(format!(
            "snapshot-rpc-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        assert!(path.starts_with(root));
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn saved(machine: &mut Machine) -> (marty_core::machine::MachineSnapshot, [Option<Vec<u8>>; 2]) {
    machine.snapshot_state_quiesced(DiskCaptureMode::Embed, 0).unwrap()
}
fn invoke(a: &mut Agent, m: &mut Machine, host: &mut SnapshotHost<'_>, method: &str, p: Value) -> Result<Value> {
    a.handle_with_snapshots(m, method, &p, Some(host))
}

#[test]
fn rpc_file_snapshot_matches_native_continuation_and_clears_old_debugger_ids() {
    let scratch = Scratch::new();
    let mut factory = || Ok(cold(false));
    let mut host = SnapshotHost::for_rw_files(&mut factory).unwrap();
    let actual_exe = fs::read(std::env::current_exe().unwrap()).unwrap();
    assert_eq!(host.build, <[u8; 32]>::from(Sha256::digest(&actual_exe)));
    let mut m = cold(false);
    m.load_program(&[0xff, 0x06, 0x00, 0x02, 0xeb, 0xfa], 0, 0x100, 0, 0x100)
        .unwrap();
    let mut a = Agent::new(2301);
    m.pit_adjust(2);
    m.set_cpu_option(marty_core::cpu_common::CpuOption::EnableWaitStates(false));
    m.set_cpu_option(marty_core::cpu_common::CpuOption::OffRailsDetection(true));
    a.step(&mut m);
    a.next = 99;
    a.completed.insert("op-3".into(), json!({"old":true}));
    a.completed_order.push_back("op-3".into());
    a.handle(
        &mut m,
        "breakpoints.create",
        &json!({"kind":"execution","address":0x105}),
    )
    .unwrap();
    let baseline = saved(&mut m);
    let path = scratch.0.join("saved.zip");
    let revision = a.revision;
    let export = invoke(
        &mut a,
        &mut m,
        &mut host,
        "machine.snapshot.export",
        json!({"path":path,"expected_state_revision":revision}),
    )
    .unwrap();
    let digest = export["sha256"].clone();
    assert_eq!(digest, json!(super::super::digest(&fs::read(&path).unwrap())));
    assert_eq!(export["build_sha256"], json!(super::super::digest(&actual_exe)));
    a.step(&mut m);
    let revision = a.revision;
    let imported = invoke(&mut a,&mut m,&mut host,"machine.snapshot.import",
        json!({"path":path,"disk_root":scratch.0.join("restored"),"expected_sha256":digest,"expected_state_revision":revision})).unwrap();
    assert_eq!(a.revision, revision + 1);
    assert_eq!(saved(&mut m), baseline);
    assert_eq!(imported["paused"], true);
    assert_eq!(a.last_stop["kind"], "snapshot_restored");
    assert!(a.breakpoints.is_empty() && a.completed.is_empty() && a.completed_order.is_empty());
    assert!(a
        .handle(&mut m, "execution.wait", &json!({"operation_id":"op-3"}))
        .is_err());
    assert!(a.next >= 100);
    let mut reference = cold(false).prepare_snapshot_restore(&baseline.0, [None, None]).unwrap();
    let mut control = ExecutionControl::new();
    for _ in 0..40 {
        a.step(&mut m);
        control.set_op(ExecutionOperation::Step);
        reference.run(1, &mut control);
        assert_eq!(saved(&mut m), saved(&mut reference));
    }
    assert!(u16::from_le_bytes(peek(&m, 0x200, 2).unwrap().try_into().unwrap()) > 1);
    println!("RPC_SNAPSHOT_NATIVE: actual test executable SHA, File export/import, guarded live swap, stale IDs refused,40 native CPU/PIT/CGA steps/full Machine equality; same process, not Pyro");
}

#[test]
fn refused_import_export_leave_machine_and_caller_files_unchanged() {
    let scratch = Scratch::new();
    let mut factory = || Ok(cold(false));
    let mut host = SnapshotHost::for_rw_files(&mut factory).unwrap();
    let mut m = cold(false);
    let mut a = Agent::new(2301);
    let before = saved(&mut m);
    let path = scratch.0.join("saved.zip");
    let export = invoke(
        &mut a,
        &mut m,
        &mut host,
        "machine.snapshot.export",
        json!({"path":path,"expected_state_revision":0}),
    )
    .unwrap();
    let original = fs::read(&path).unwrap();
    fs::write(&path, [&original[..], b"DELIBERATE ARCHIVE MUTANT"].concat()).unwrap();
    let corrupt = json!({"path":path,"disk_root":scratch.0.join("corrupt"),"expected_sha256":export["sha256"],"expected_state_revision":0});
    assert!(invoke(&mut a, &mut m, &mut host, "machine.snapshot.import", corrupt).is_err());
    assert!(!scratch.0.join("corrupt").exists());
    fs::write(&path, &original).unwrap();
    assert!(invoke(
        &mut a,
        &mut m,
        &mut host,
        "machine.snapshot.export",
        json!({"path":path,"expected_state_revision":0})
    )
    .is_err());
    let root = scratch.0.join("new-disks");
    let good = json!({"path":path,"disk_root":root,"expected_sha256":export["sha256"],"expected_state_revision":0});
    for (field, value) in [
        ("expected_state_revision", json!(1)),
        ("expected_sha256", Value::Null),
        ("expected_sha256", json!("00".repeat(32))),
        ("expected_sha256", json!("é".repeat(32))),
        ("references", json!({"2":"unused"})),
        ("references", json!(false)),
    ] {
        let mut bad = good.clone();
        bad[field] = value;
        assert!(invoke(&mut a, &mut m, &mut host, "machine.snapshot.import", bad).is_err());
        assert!(!root.exists());
        assert_eq!(saved(&mut m), before);
        assert_eq!(a.revision, 0);
    }
    a.running = true;
    assert!(invoke(&mut a, &mut m, &mut host, "machine.snapshot.import", good.clone()).is_err());
    a.running = false;
    fs::create_dir(&root).unwrap();
    let sentinel = root.join("keep");
    fs::write(&sentinel, b"caller-owned").unwrap();
    assert!(invoke(&mut a, &mut m, &mut host, "machine.snapshot.import", good.clone()).is_err());
    assert_eq!(fs::read(&sentinel).unwrap(), b"caller-owned");
    fs::remove_file(sentinel).unwrap();
    fs::remove_dir(&root).unwrap();
    host.build[0] ^= 1;
    assert!(invoke(&mut a, &mut m, &mut host, "machine.snapshot.import", good.clone()).is_err());
    host.build[0] ^= 1;
    assert!(!root.exists());
    drop(host);
    let mut factory = || {
        let mut m = cold(false);
        m.reinstall_roms(MachineRomManifest {
            roms: vec![marty_core::machine::MachineRomEntry {
                data: vec![0x90],
                addr: 0xf0000,
                repeat: 1,
                ..Default::default()
            }],
            ..Default::default()
        })
        .unwrap();
        Ok(m)
    };
    let mut host = SnapshotHost::for_rw_files(&mut factory).unwrap();
    assert!(invoke(&mut a, &mut m, &mut host, "machine.snapshot.import", good).is_err());
    assert!(!root.exists(), "refused candidate left staged disks behind");
    assert_eq!(saved(&mut m), before);
    assert_eq!(a.revision, 0);
    assert_eq!(fs::read(path).unwrap(), original);
    println!("RPC_SNAPSHOT_REFUSAL: revision/running/checksum/build/reference schema/existing output/existing directory/changed ROM refused; full live state and caller files unchanged");
}

#[test]
fn rw_file_disk_restore_preserves_partial_ata_and_writes_only_new_disk_copies() {
    let scratch = Scratch::new();
    let mut m = cold(true);
    let mut source_paths = Vec::new();
    let controller = m.bus_mut().xtide_mut().as_mut().unwrap();
    let g = controller.get_supported_formats()[0].geometry;
    for slot in 0..2 {
        let path = scratch.0.join(format!("source-{slot}.vhd"));
        let file = create_vhd(path.clone().into_os_string(), g.c(), g.h(), g.s()).unwrap();
        file.sync_all().unwrap();
        drop(file);
        let file = SnapshotRwFile::open(&path).unwrap();
        let mut disk = VirtualHardDisk::parse(Box::new(file), slot == 1).unwrap();
        disk.write_sector(&(0..512).map(|i| (i * 13 + 7) as u8).collect::<Vec<_>>(), 0, 0, 0)
            .unwrap();
        controller.set_vhd(slot, disk).unwrap();
        source_paths.push(path);
    }
    controller.mask_register_write(0);
    for (reg, byte) in [
        (HDC_DRIVE_HEAD_REGISTER, 0xa0),
        (HDC_SECTOR_COUNT_REGISTER, 1),
        (HDC_SECTOR_NUMBER_REGISTER, 1),
        (HDC_STATUS_REGISTER, 0x20),
    ] {
        m.bus_mut().io_write_u8(DEFAULT_IO_BASE + reg, byte, 0, None);
    }
    for i in 0..17 {
        assert_eq!(
            m.bus_mut().io_read_u8(
                DEFAULT_IO_BASE
                    + if i % 2 == 0 {
                        HDC_DATA_REGISTER0
                    } else {
                        HDC_DATA_REGISTER1
                    },
                0
            ),
            (i * 13 + 7) as u8
        );
    }
    let baseline = saved(&mut m);
    let source_bytes: Vec<_> = source_paths.iter().map(|p| fs::read(p).unwrap()).collect();
    let mut factory = || Ok(cold(true));
    let mut host = SnapshotHost::for_rw_files(&mut factory).unwrap();
    let mut a = Agent::new(2301);
    for mode in ["embed", "reference"] {
        let path = scratch.0.join(format!("{mode}.zip"));
        let revision = a.revision;
        let export = invoke(
            &mut a,
            &mut m,
            &mut host,
            "machine.snapshot.export",
            json!({"path":path,"disk_mode":mode,"expected_state_revision":revision}),
        )
        .unwrap();
        let root = scratch.0.join(format!("restored-{mode}"));
        let mut p = json!({"path":path,"disk_root":root,"expected_sha256":export["sha256"],"expected_state_revision":a.revision});
        if mode == "reference" {
            assert!(invoke(&mut a, &mut m, &mut host, "machine.snapshot.import", p.clone()).is_err());
            assert!(!root.exists());
            p["references"] = json!({"0":source_paths[0],"1":source_paths[1]});
        }
        invoke(&mut a, &mut m, &mut host, "machine.snapshot.import", p).unwrap();
        assert_eq!(saved(&mut m), baseline);
    }
    for i in 17..512 {
        assert_eq!(
            m.bus_mut().io_read_u8(
                DEFAULT_IO_BASE
                    + if i % 2 == 0 {
                        HDC_DATA_REGISTER0
                    } else {
                        HDC_DATA_REGISTER1
                    },
                0
            ),
            (i * 13 + 7) as u8
        );
    }
    // Complete the prior read and the cold slave's native reset delay before
    // issuing write commands; otherwise ATA correctly refuses a busy/reset drive.
    let mut controller = m.bus_mut().xtide_mut().take().unwrap();
    controller.run(
        &mut marty_core::devices::dma::DMAController::new(),
        m.bus_mut(),
        200_000.0,
    );
    *m.bus_mut().xtide_mut() = Some(controller);
    // Native ATA port writes, including the slot with cached read_only=true, exercise
    // actual OS RW providers. Verify raw bytes independently through File reads.
    for slot in 0..2 {
        for (reg, byte) in [
            (HDC_DRIVE_HEAD_REGISTER, 0xa0 | (slot << 4)),
            (HDC_SECTOR_COUNT_REGISTER, 1),
            (HDC_SECTOR_NUMBER_REGISTER, 1),
            (HDC_STATUS_REGISTER, 0x30),
        ] {
            m.bus_mut().io_write_u8(DEFAULT_IO_BASE + reg, byte, 0, None);
        }
        for i in 0..512 {
            m.bus_mut().io_write_u8(
                DEFAULT_IO_BASE
                    + if i % 2 == 0 {
                        HDC_DATA_REGISTER0
                    } else {
                        HDC_DATA_REGISTER1
                    },
                0xa7,
                0,
                None,
            );
        }
        // Filling the ATA buffer queues a write; native device run commits it.
        let mut controller = m.bus_mut().xtide_mut().take().unwrap();
        controller.run(&mut marty_core::devices::dma::DMAController::new(), m.bus_mut(), 0.25);
        *m.bus_mut().xtide_mut() = Some(controller);
        let raw = fs::read(scratch.0.join(format!("restored-reference/disk-{slot}.vhd"))).unwrap();
        // The native creator writes FIXED VHD data at zero and its footer at
        // the end. Independently check its type/offset before using raw[..512].
        let footer = &raw[raw.len() - 512..];
        assert_eq!(&footer[..8], b"conectix");
        assert_eq!(u32::from_be_bytes(footer[60..64].try_into().unwrap()), 2);
        assert_eq!(u64::from_be_bytes(footer[16..24].try_into().unwrap()), u64::MAX);
        assert_eq!(&raw[..512], &[0xa7; 512], "restored slot{slot} raw sector");
        assert_eq!(
            fs::read(&source_paths[slot as usize]).unwrap(),
            source_bytes[slot as usize]
        );
    }
    drop(m); // close restored Windows File handles before Scratch cleanup
    println!("RPC_SNAPSHOT_FILE_DISKS: two real RW Files, both embed/reference modes, native ATA byte17 continuation and512-byte native ATA port writes to both restored copies; original reference files unchanged");
}

#[test]
fn snapshot_rpc_refuses_unverified_providers_and_never_overwrites_existing_files() {
    let scratch = Scratch::new();
    let mut m = cold(true);
    let g = m.bus_mut().xtide_mut().as_ref().unwrap().get_supported_formats()[0].geometry;
    let path = scratch.0.join("raw.vhd");
    drop(create_vhd(path.clone().into_os_string(), g.c(), g.h(), g.s()).unwrap());
    let original = fs::read(&path).unwrap();
    let mut factory = || Ok(cold(true));
    let mut host = SnapshotHost::for_rw_files(&mut factory).unwrap();
    let mut a = Agent::new(2301);
    let output = scratch.0.join("must-not-exist.zip");
    for provider in [
        Box::new(std::io::Cursor::new(original.clone())) as Box<dyn VhdIO>,
        Box::new(File::open(&path).unwrap()) as Box<dyn VhdIO>,
        Box::new(
            OpenOptions::new()
                .read(true)
                .write(true)
                .append(true)
                .open(&path)
                .unwrap(),
        ) as Box<dyn VhdIO>,
    ] {
        m.bus_mut()
            .xtide_mut()
            .as_mut()
            .unwrap()
            .set_vhd(0, VirtualHardDisk::parse(provider, false).unwrap())
            .unwrap();
        let before = saved(&mut m);
        assert!(!m.snapshot_rw_files());
        assert!(invoke(
            &mut a,
            &mut m,
            &mut host,
            "machine.snapshot.export",
            json!({"path":output,"expected_state_revision":0})
        )
        .is_err());
        assert!(!output.exists());
        assert_eq!(saved(&mut m), before);
    }
    assert!(SnapshotRwFile::create_new(&path).is_err());
    assert!(SnapshotRwFile::open(&scratch.0).is_err());
    assert_eq!(fs::read(path).unwrap(), original);
    drop(m);
    println!("RPC_PROVIDER_REFUSAL: Cursor, raw read-only File and append File rejected before export; existing disk unchanged; typed providers enforce RW/non-append constructors");
}
