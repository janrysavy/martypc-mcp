use super::*;
use marty_core::{
    machine::{MachineBuilder, MachineRomManifest},
    machine_config::{MachineConfiguration, VideoCardConfig},
    machine_types::MachineType,
    vhd::DiskCaptureMode,
};
fn fixture() -> Machine {
    let mut config =
        marty_config::read_config(include_str!("../../../../install/martypc.toml"), Default::default()).unwrap();
    config.machine.no_roms = true;
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
    let mut machine = MachineBuilder::new()
        .with_core_config(Box::new(&config))
        .with_machine_config(&description)
        .with_roms(MachineRomManifest::new())
        .build()
        .unwrap();
    machine.change_state(MachineState::On);
    machine.load_program(&[0x90, 0xeb, 0xfd], 0, 0x100, 0, 0x100).unwrap();
    machine.bus_mut().io_write_u8(0x3d8, 0x29, 0, None);
    machine
}
fn saved(m: &mut Machine) -> marty_core::machine::MachineSnapshot {
    m.snapshot_state_quiesced(DiskCaptureMode::Embed, 128 * 1024 * 1024)
        .unwrap()
        .0
}
#[test]
fn coherent_observation_preserves_complete_machine_and_wraps_native_text() {
    let mut m = fixture();
    let mut a = Agent::new(1);
    a.handle(&mut m, "execution.continue", &json!({})).unwrap();
    for _ in 0..5 {
        a.advance(&mut m);
    }
    a.handle(&mut m, "execution.pause", &json!({})).unwrap();
    for (port, data) in [(0x3d4, 12), (0x3d5, 0x1f), (0x3d4, 13), (0x3d5, 0xff)] {
        m.bus_mut().io_write_u8(port, data, 0, None);
    }
    m.bus_mut().write_u8(0xbbffe, 65, 0).unwrap();
    m.bus_mut().write_u8(0xbbfff, 0x9e, 0).unwrap();
    m.bus_mut().write_u8(0xb8000, 0xe1, 0).unwrap();
    m.bus_mut().write_u8(0xb8001, 0x21, 0).unwrap();
    let before = saved(&mut m);
    let q=observe(&a,&mut m,&json!({"expected_state_revision":a.revision,
        "memory":[{"address":0x100,"length":32},{"address":{"space":"segmented","segment":0xb800,"offset":0},"length":8}],
        "video_text":{},"video_memory":true})).unwrap();
    assert_eq!(q["state_revision"], a.revision);
    assert_eq!(q["registers"]["state_revision"], a.revision);
    assert_eq!(q["memory"][1]["data_hex"], "e121000000000000");
    assert_eq!(q["video_memory"]["byte_count"], 16384);
    assert_eq!(q["video_text"]["display_address"], 16382);
    assert_eq!(q["video_text"]["cells"][0][0]["code"], 65);
    assert_eq!(q["video_text"]["cells"][0][0]["foreground"], 14);
    assert_eq!(q["video_text"]["cells"][0][0]["background"], 1);
    assert_eq!(q["video_text"]["cells"][0][0]["blink"], true);
    assert_eq!(q["video_text"]["cells"][0][1]["char"], "ß");
    assert!(q["video_text"]["text"][0].as_str().unwrap().starts_with("Aß"));
    assert!(before == saved(&mut m), "observation changed complete machine");
    let page = text(&m, &json!({"page":0}), a.revision).unwrap();
    assert_eq!(page["cells"][0][0]["code"], 225);
    assert_eq!(page["is_active_page"], false);
    assert!(before == saved(&mut m));
}
#[test]
fn refused_observations_preserve_machine_and_revision() {
    let mut m = fixture();
    let mut a = Agent::new(1);
    let before = saved(&mut m);
    let bad = [
        json!({}),
        json!({"expected_state_revision":true}),
        json!({"expected_state_revision":1}),
        json!({"expected_state_revision":0,"extra":0}),
        json!({"expected_state_revision":0,"memory":[{"address":0,"length":32},{"address":1048575,"length":2}]}),
        json!({"expected_state_revision":0,"memory":[{"address":0,"length":65536},{"address":65536}]}),
        json!({"expected_state_revision":0,"memory":vec![json!({"address":0});17]}),
        json!({"expected_state_revision":0,"video_memory":1}),
        json!({"expected_state_revision":0,"video_text":{"page":9}}),
        json!({"expected_state_revision":0,"video_text":null}),
    ];
    for p in bad {
        assert_eq!(observe(&a, &mut m, &p).unwrap_err().1, -32602);
        assert!(before == saved(&mut m));
        assert_eq!(a.revision, 0);
    }
    a.running = true;
    assert!(observe(&a, &mut m, &json!({"expected_state_revision":0})).is_err());
    assert!(before == saved(&mut m));
    a.running = false;
    m.bus_mut().io_write_u8(0x3d8, 0x0a, 0, None);
    let graphics = saved(&mut m);
    assert!(observe(&a, &mut m, &json!({"expected_state_revision":0,"video_text":{}})).is_err());
    assert!(graphics == saved(&mut m));
}

#[test]
fn observation_refuses_unimplemented_mmio_without_guest_reads() {
    use marty_core::bus::{MemRangeDescriptor, MmioDeviceType};
    let mut m = fixture();
    let a = Agent::new(1);
    m.bus_mut().register_map(
        MmioDeviceType::JrIde,
        MemRangeDescriptor {
            address: 0xd8000,
            size: 8192,
            cycle_cost: 0,
            read_only: false,
            priority: 0,
        },
    );
    let before = registers(&mut m, 0);
    let ticks = m.system_ticks();
    assert!(!m.bus().is_observable_memory(0xd8000));
    let refused = observe(
        &a,
        &mut m,
        &json!({"expected_state_revision":0,
        "memory":[{"address":0,"length":32},{"address":0xd8000}]}),
    );
    assert_eq!(
        refused.unwrap_err(),
        ("observation cannot peek this memory-mapped device", -32602)
    );
    assert_eq!(registers(&mut m, 0), before);
    assert_eq!(m.system_ticks(), ticks);
}

#[test]
fn observation_dispatch_and_unbacked_ranges_have_explicit_refusals() {
    let mut m = fixture();
    let mut a = Agent::new(1);
    let before = saved(&mut m);
    let q = a
        .request(&mut m, br#"{"jsonrpc":"2.0","id":1,"method":"agent.capabilities"}"#)
        .unwrap();
    assert!(q["result"]["methods"]
        .as_array()
        .unwrap()
        .contains(&json!("state.observe")));
    let q=a.request(&mut m,br#"{"jsonrpc":"2.0","id":2,"method":"state.observe","params":{"expected_state_revision":0,"memory":[{"address":1024,"length":2}],"video_text":{}}}"#).unwrap();
    assert_eq!(q["id"], 2);
    assert_eq!(q["result"]["memory"][0]["address"], 1024);
    assert_eq!(q["result"]["registers"]["state_revision"], 0);
    let q=a.request(&mut m,br#"{"jsonrpc":"2.0","id":3,"method":"state.observe","params":{"expected_state_revision":0,"memory":[{"address":655360}]}}"#).unwrap();
    assert_eq!(q["error"]["code"], -32602);
    assert!(before == saved(&mut m));
    assert_eq!(character(127), '\u{7f}'); // Python bytes([127]).decode('cp437') is U+007F.
}
#[test]
fn overridden_cga_mapping_is_refused_before_copying_video() {
    use marty_core::bus::{MemRangeDescriptor, MmioDeviceType};
    let mut m = fixture();
    let a = Agent::new(1);
    m.bus_mut().register_map(
        MmioDeviceType::JrIde,
        MemRangeDescriptor {
            address: 0xb8000,
            size: 8192,
            cycle_cost: 0,
            read_only: false,
            priority: 0,
        },
    );
    let before = registers(&mut m, 0);
    let ticks = m.system_ticks();
    for options in [json!({"video_text":{}}), json!({"video_memory":true})] {
        let mut p = options;
        p["expected_state_revision"] = json!(0);
        assert_eq!(observe(&a, &mut m, &p).unwrap_err().1, -32602);
    }
    assert_eq!(registers(&mut m, 0), before);
    assert_eq!(m.system_ticks(), ticks);
}

#[test]
fn remapped_cga_outside_native_aperture_is_refused_without_underflow() {
    use marty_core::bus::{MemRangeDescriptor, MmioDeviceType};
    let mut m = fixture();
    let a = Agent::new(1);
    let id = m.bus().enumerate_videocards()[0];
    m.bus_mut().register_map(
        MmioDeviceType::Video(id),
        MemRangeDescriptor {
            address: 0xa0000,
            size: 8192,
            cycle_cost: 0,
            read_only: false,
            priority: 0,
        },
    );
    let before = registers(&mut m, 0);
    let ticks = m.system_ticks();
    assert_eq!(
        observe(
            &a,
            &mut m,
            &json!({"expected_state_revision":0,"memory":[{"address":0xa0000}]})
        )
        .unwrap_err()
        .1,
        -32602
    );
    assert_eq!(registers(&mut m, 0), before);
    assert_eq!(m.system_ticks(), ticks);
}
