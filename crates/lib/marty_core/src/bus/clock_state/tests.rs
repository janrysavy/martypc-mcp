use super::*;
use crate::devices::pit::PitType;

fn roundtrip(bus: &BusInterface) -> BusClockState {
    serde_json::from_slice(&serde_json::to_vec(&bus.snapshot_clock_state().unwrap()).unwrap()).unwrap()
}

fn destroy(bus: &mut BusInterface) {
    // Independent reset, not the restore under test. Other owners are retained
    // deliberately: this probe tests bus-owned continuation, not Machine restart.
    bus.cpu_factor = ClockFactor::Divisor(9);
    bus.timing_table.fill(TimingTableEntry { sys_ticks: 0, us: 0.0 });
    bus.cycles_to_ticks.fill(0);
    bus.pit_ticks_advance = 0;
    bus.intr_imminent = false;
    bus.a0_data = 0;
    bus.nmi_gate = false;
    bus.dma_counter = 0;
    bus.do_title_hacks = false;
    bus.timer_trigger1_armed = false;
    bus.timer_trigger2_armed = false;
    bus.cga_tick_accum = 0;
    bus.tga_tick_accum = 0;
    bus.refresh_enabled = false;
    bus.refresh_active = false;
}

#[test]
fn bus_clock_preserves_native_conversion_tables_and_seeded_storage() {
    let mut bus = BusInterface::default();
    let cold = roundtrip(&bus);
    bus.set_cpu_factor(ClockFactor::Multiplier(2));
    bus.restore_clock_state(&cold).unwrap();
    assert_eq!(bus.cycles_to_ticks, [0; 256], "native default table remains cold");
    for factor in [
        ClockFactor::Divisor(1),
        ClockFactor::Divisor(3),
        ClockFactor::Multiplier(2),
    ] {
        bus.set_cpu_factor(factor);
        BusInterface::update_timing_table(&mut *bus.timing_table, factor, 14.31818);
        bus.adjust_pit(7);
        bus.a0_data = 0xA5;
        bus.nmi_gate = true;
        bus.dma_counter = 0x1234;
        bus.do_title_hacks = true;
        bus.timer_trigger1_armed = true;
        bus.timer_trigger2_armed = true;
        bus.intr_imminent = true;
        bus.cga_tick_accum = 8;
        bus.tga_tick_accum = 19;
        bus.refresh_enabled = true;
        bus.refresh_active = true;
        // Seeded finite binary64 storage vectors are not physical timing proof.
        for (i, bits) in [0, 1, (-0.0f64).to_bits(), 0x3ff0000000000001].into_iter().enumerate() {
            bus.timing_table[i].us = f64::from_bits(bits);
        }
        let saved = roundtrip(&bus);
        let mut target = BusInterface::default();
        target.restore_clock_state(&saved).unwrap();
        assert_eq!(target.snapshot_clock_state().unwrap(), saved);
        for cycles in 0..256 {
            assert_eq!(
                bus.cpu_cycles_to_system_ticks(cycles),
                target.cpu_cycles_to_system_ticks(cycles)
            );
            assert_eq!(
                bus.system_ticks_to_cpu_cycles(cycles),
                target.system_ticks_to_cpu_cycles(cycles)
            );
            assert_eq!(
                target.cycles_to_ticks[cycles as usize],
                bus.cycles_to_ticks[cycles as usize]
            );
            let mut a = DeviceRunContext::default();
            let mut b = DeviceRunContext::default();
            bus.set_context_timings(&mut a, cycles);
            target.set_context_timings(&mut b, cycles);
            assert_eq!(a.delta_ticks, b.delta_ticks);
            assert_eq!(a.delta_us.to_bits(), b.delta_us.to_bits());
        }
    }
    println!("BUS_CLOCK_STORAGE: cold table and three seeded/native-conversion restores; 768 consumers");
}

fn xt() -> BusInterface {
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
    bus.videocards.insert(
        id,
        VideoCardDispatch::Cga(Box::new(CGACard::new(TraceLogger::None, ClockingMode::Cycle, false))),
    );
    bus.videocard_ids.push(id);
    // Program real native PIT ports; keep each bus's own independent devices.
    bus.pit = Some(Pit::new(PitType::Model8253, 14.31818, 12, None));
    for (port, byte) in [(0x43, 0x34), (0x40, 19), (0x40, 0), (0x43, 0x74), (0x41, 11), (0x41, 0)] {
        bus.io_write_u8(port, byte, 0, None);
    }
    bus
}

#[test]
fn bus_clock_restore_continues_native_pit_refresh_and_cga() {
    let mut a = xt();
    let mut b = xt();
    let mut ka = VecDeque::new();
    let mut kb = VecDeque::new();
    let mut observed_pending = false;
    let mut saw_cga_remainder = false;
    for n in 0..64 {
        let ticks = [1, 2, 5, 8, 13, 34][n % 6];
        if n % 3 == 0 {
            a.adjust_pit(3);
            b.adjust_pit(3);
        }
        let saved = roundtrip(&a);
        observed_pending |= saved.pit_ticks_advance != 0;
        saw_cga_remainder |= saved.cga_tick_accum != 0;
        assert_eq!(saved, b.snapshot_clock_state().unwrap());
        destroy(&mut b);
        b.restore_clock_state(&saved).unwrap();
        // Compare a native consumer output before comparing saved storage.
        assert_eq!(a.is_intr_imminent(), b.is_intr_imminent());
        let ea = a.run_devices(ticks as f64 / 14.31818, ticks, None, &mut ka, None);
        let eb = b.run_devices(ticks as f64 / 14.31818, ticks, None, &mut kb, None);
        assert_eq!(
            format!("{ea:?}"),
            format!("{eb:?}"),
            "native returned refresh/NMI event n={n}"
        );
        assert_eq!(
            a.pit.as_ref().unwrap().snapshot_state().unwrap(),
            b.pit.as_ref().unwrap().snapshot_state().unwrap(),
            "native PIT n={n}"
        );
        assert_eq!(
            a.pic1.as_ref().unwrap().snapshot_state(),
            b.pic1.as_ref().unwrap().snapshot_state(),
            "native PIC n={n}"
        );
        for id in &a.videocard_ids {
            if let (VideoCardDispatch::Cga(a), VideoCardDispatch::Cga(b)) = (&a.videocards[id], &b.videocards[id]) {
                assert_eq!(
                    a.snapshot_state().unwrap(),
                    b.snapshot_state().unwrap(),
                    "native CGA n={n}"
                );
            } else {
                panic!("expected independent CGA devices");
            }
        }
        assert_eq!(
            a.snapshot_clock_state().unwrap(),
            b.snapshot_clock_state().unwrap(),
            "native bus n={n}"
        );
    }
    assert!(observed_pending && saw_cga_remainder);
    println!("BUS_CLOCK_NATIVE: 64 destructive JSON restores; independent native PIT/PIC/CGA retained");
}

#[test]
fn bus_clock_schema_invalid_restores_leave_live_state_unchanged() {
    let mut bus = xt();
    let saved = roundtrip(&bus);
    let value = serde_json::to_value(&saved).unwrap();
    for key in value.as_object().unwrap().keys() {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(
            serde_json::from_value::<BusClockState>(missing).is_err(),
            "required {key}"
        );
    }
    let mut unknown = value.clone();
    unknown["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<BusClockState>(unknown).is_err());
    for n in 0..7 {
        let mut bad = saved.clone();
        match n {
            0 => bad.version += 1,
            1 => bad.config = None,
            2 => bad.cpu_factor = Factor::Divisor(0),
            3 => {
                bad.timing_table.pop();
            }
            4 => {
                bad.cycles_to_ticks.pop();
            }
            5 => bad.timing_table[1].us_bits = f64::NAN.to_bits(),
            _ => bad.config.as_mut().unwrap().system_crystal_bits ^= 1,
        }
        assert!(bus.restore_clock_state(&bad).is_err());
        assert_eq!(bus.snapshot_clock_state().unwrap(), saved, "atomic refusal {n}");
    }
    println!("BUS_CLOCK_REFUSALS: required schema and seven preflight failures leave live state unchanged");
}
