//! PIT-owned state, including count latches, partial I/O, gates, clock phase
//! and not-yet-emitted speaker samples. The existing audio sender is retained.
//! Samples already delivered to the external receiver belong to machine/audio
//! state and are NOT restored here. PIC/DMA/PPI and full restart remain separate.

use super::*;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
enum TimeWarp {
    SystemTicks(u32),
    MicrosecondsBits(u64),
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PitState {
    version: u32,
    ptype: PitType,
    crystal_bits: u64,
    clock_divisor: u32,
    pit_cycles: u64,
    sys_tick_accumulator: u32,
    sys_ticks_advance: u32,
    cycle_accumulator_bits: u64,
    channels: Vec<Channel>,
    timewarp: TimeWarp,
    speaker_buf_bits: VecDeque<u32>,
    defer_reload_flag: bool,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    chan1_source: Option<usize>,
    last_output_state: [bool; 3],
    speaker_enabled: bool,
    speaker_sample_accum_bits: u32,
    speaker_sample_ct: u32,
    speaker_connected: bool,
}

impl ProgrammableIntervalTimer {
    pub(crate) fn snapshot_state(&self) -> Result<PitState, &'static str> {
        let saved = PitState {
            version: 1,
            ptype: self.ptype,
            crystal_bits: self._crystal.to_bits(),
            clock_divisor: self.clock_divisor,
            pit_cycles: self.pit_cycles,
            sys_tick_accumulator: self.sys_tick_accumulator,
            sys_ticks_advance: self.sys_ticks_advance,
            cycle_accumulator_bits: self.cycle_accumulator.to_bits(),
            channels: self.channels.clone(),
            timewarp: match self.timewarp {
                DeviceRunTimeUnit::SystemTicks(ticks) => TimeWarp::SystemTicks(ticks),
                DeviceRunTimeUnit::Microseconds(us) => TimeWarp::MicrosecondsBits(us.to_bits()),
            },
            speaker_buf_bits: self.speaker_buf.iter().map(|s| s.to_bits()).collect(),
            defer_reload_flag: self.defer_reload_flag,
            chan1_source: self.chan1_source,
            last_output_state: self.last_output_state,
            speaker_enabled: self.speaker.enabled,
            speaker_sample_accum_bits: self.speaker.sample_accum.to_bits(),
            speaker_sample_ct: self.speaker.sample_ct,
            speaker_connected: self.speaker.sender.is_some(),
        };
        self.preflight_state(&saved)?;
        Ok(saved)
    }

    pub(crate) fn preflight_state(&self, saved: &PitState) -> Result<(), &'static str> {
        if saved.version != 1
            || saved.ptype != self.ptype
            || saved.clock_divisor != self.clock_divisor
            || saved.crystal_bits != self._crystal.to_bits()
            || saved.speaker_connected != self.speaker.sender.is_some()
        {
            return Err("incompatible PIT version/configuration/audio connection");
        }
        if saved.channels.len() != 3
            || saved
                .channels
                .iter()
                .enumerate()
                .any(|(i, c)| c.c != i || c.ptype != saved.ptype)
            || saved.chan1_source.is_some_and(|c| c >= 3)
        {
            return Err("invalid PIT channel configuration");
        }
        if !f64::from_bits(saved.crystal_bits).is_finite()
            || !f64::from_bits(saved.cycle_accumulator_bits).is_finite()
            || matches!(saved.timewarp, TimeWarp::MicrosecondsBits(bits) if !f64::from_bits(bits).is_finite())
            || !f32::from_bits(saved.speaker_sample_accum_bits).is_finite()
            || saved
                .speaker_buf_bits
                .iter()
                .any(|bits| !f32::from_bits(*bits).is_finite())
            || saved.speaker_sample_ct >= SPEAKER_SAMPLE_RATIO
        {
            return Err("invalid PIT clock/sample state");
        }
        Ok(())
    }

    pub(crate) fn restore_state(&mut self, saved: &PitState) -> Result<(), &'static str> {
        self.preflight_state(saved)?;
        // Machine restore must preflight PIC/DMA/PPI/audio queues and all other
        // dependencies before applying components; this performs no bus I/O.
        self.pit_cycles = saved.pit_cycles;
        self.sys_tick_accumulator = saved.sys_tick_accumulator;
        self.sys_ticks_advance = saved.sys_ticks_advance;
        self.cycle_accumulator = f64::from_bits(saved.cycle_accumulator_bits);
        self.channels.clone_from(&saved.channels);
        self.timewarp = match saved.timewarp {
            TimeWarp::SystemTicks(ticks) => DeviceRunTimeUnit::SystemTicks(ticks),
            TimeWarp::MicrosecondsBits(bits) => DeviceRunTimeUnit::Microseconds(f64::from_bits(bits)),
        };
        self.speaker_buf = saved.speaker_buf_bits.iter().map(|s| f32::from_bits(*s)).collect();
        self.defer_reload_flag = saved.defer_reload_flag;
        self.chan1_source = saved.chan1_source;
        self.last_output_state = saved.last_output_state;
        self.speaker.enabled = saved.speaker_enabled;
        self.speaker.sample_accum = f32::from_bits(saved.speaker_sample_accum_bits);
        self.speaker.sample_ct = saved.speaker_sample_ct;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pit_storage_inventory_has_no_unrepresented_owned_fields() {
        let (pit, _bus, _audio) = setup(PitType::Model8253);
        let value = serde_json::to_value(pit.snapshot_state().unwrap()).unwrap();
        let mut covered: std::collections::HashSet<String> = value.as_object().unwrap().keys().cloned().collect();
        covered.extend(["_crystal", "cycle_accumulator", "speaker_buf", "speaker"].map(str::to_owned));
        let source = include_str!("../pit.rs")
            .split("pub struct ProgrammableIntervalTimer {")
            .nth(1)
            .unwrap()
            .split("\n}")
            .next()
            .unwrap();
        let fields = regex::Regex::new(r"(?m)^\s+([a-z_][a-z0-9_]*):").unwrap();
        for field in fields.captures_iter(source) {
            assert!(
                covered.contains(&field[1]),
                "new PIT storage field needs snapshot coverage: {}",
                &field[1]
            );
        }
        // PitSpeaker's sender is the external route, whose presence is checked.
        let source = include_str!("../pit.rs")
            .split("pub struct PitSpeaker {")
            .nth(1)
            .unwrap()
            .split("\n}")
            .next()
            .unwrap();
        let fields = regex::Regex::new(r"(?m)^\s+pub ([a-z_][a-z0-9_]*):").unwrap();
        for field in fields.captures_iter(source) {
            let key = match &field[1] {
                "sender" => "speaker_connected".to_owned(),
                "sample_accum" => "speaker_sample_accum_bits".to_owned(),
                other => format!("speaker_{other}"),
            };
            assert!(
                covered.contains(&key),
                "new speaker field needs state or route handling: {}",
                &field[1]
            );
        }
    }

    fn setup(model: PitType) -> (Pit, BusInterface, crossbeam_channel::Receiver<f32>) {
        let mut bus = BusInterface::default();
        let machine_type = crate::machine_types::MachineType::Ibm5160;
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
        bus.io_write_u8(0x61, 3, 0, None); // actual PPI gate2 and speaker-data bits
        let (sender, receiver) = crossbeam_channel::unbounded();
        (Pit::new(model, 14.31818, 12, Some(sender)), bus, receiver)
    }

    fn destroy(pit: &mut Pit) {
        // Fresh independent device, retaining only the external audio route.
        let sender = pit.speaker.sender.take();
        *pit = Pit::new(pit.ptype, pit._crystal, pit.clock_divisor, sender);
        pit.pit_cycles = u64::MAX - 1; // differ even for first reset sample
    }

    fn program(pit: &mut Pit, bus: &mut BusInterface, mode: u8) {
        for c in 0..3 {
            pit.control_register_write((c << 6) | 0x30 | (mode << 1), bus);
            pit.channels[c as usize].set_gate(true, bus);
            pit.data_write(c as usize, 19 + c, bus); // incomplete LSB/MSB reload
        }
    }

    #[test]
    fn pit_restore_continues_all_modes_partial_io_clock_phase_and_audio() {
        for model in [PitType::Model8253, PitType::Model8254] {
            for mode in 0..6 {
                let (mut reference, mut bus_a, audio_a) = setup(model);
                let (mut restored, mut bus_b, audio_b) = setup(model);
                program(&mut reference, &mut bus_a, mode);
                program(&mut restored, &mut bus_b, mode);
                let mut saw_samples = false;
                for n in 0..160 {
                    if n % 17 == 0 {
                        // Native I/O catch-up precedes the next bus run. This
                        // leaves a nonzero timewarp and deferred-load flag.
                        reference.write_u8(
                            PIT_COMMAND_REGISTER,
                            0,
                            Some(&mut bus_a),
                            DeviceRunTimeUnit::SystemTicks(5),
                            None,
                        );
                        restored.write_u8(
                            PIT_COMMAND_REGISTER,
                            0,
                            Some(&mut bus_b),
                            DeviceRunTimeUnit::SystemTicks(5),
                            None,
                        );
                    }
                    let saved = reference.snapshot_state().unwrap();
                    let decoded: PitState = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
                    destroy(&mut restored);
                    assert_ne!(restored.pit_cycles, saved.pit_cycles);
                    restored.restore_state(&decoded).unwrap();
                    assert!(restored.snapshot_state().unwrap() == saved);
                    if n == 0 {
                        for c in 0..3 {
                            reference.data_write(c, 0, &mut bus_a);
                            restored.data_write(c, 0, &mut bus_b);
                        }
                    }
                    if n % 11 == 0 {
                        for c in 0..3 {
                            // Exercise actual PIT port dispatch and monotonic
                            // I/O catch-up deltas before the outer bus run.
                            let delta = DeviceRunTimeUnit::SystemTicks(5 + c as u32);
                            reference.write_u8(PIT_COMMAND_REGISTER, (c as u8) << 6, Some(&mut bus_a), delta, None);
                            restored.write_u8(PIT_COMMAND_REGISTER, (c as u8) << 6, Some(&mut bus_b), delta, None);
                            assert_eq!(
                                reference.read_u8(0x40 + c as u16, delta),
                                restored.read_u8(0x40 + c as u16, delta)
                            );
                        } // preserve each channel's pending read MSB
                    }
                    if n % 11 == 1 {
                        for c in 0..3 {
                            let delta = DeviceRunTimeUnit::SystemTicks(0);
                            assert_eq!(
                                reference.read_u8(0x40 + c as u16, delta),
                                restored.read_u8(0x40 + c as u16, delta)
                            );
                        }
                    }
                    if n % 23 == 0 {
                        let enabled = if n % 46 == 0 { 0 } else { 3 };
                        bus_a.io_write_u8(0x61, enabled, 0, None);
                        bus_b.io_write_u8(0x61, enabled, 0, None);
                        for c in 0..2 {
                            reference.channels[c].set_gate(enabled != 0, &mut bus_a);
                            restored.channels[c].set_gate(enabled != 0, &mut bus_b);
                        }
                    }
                    reference.run(&mut bus_a, DeviceRunTimeUnit::SystemTicks(13), None);
                    restored.run(&mut bus_b, DeviceRunTimeUnit::SystemTicks(13), None);
                    let a: Vec<_> = audio_a.try_iter().map(f32::to_bits).collect();
                    let b: Vec<_> = audio_b.try_iter().map(f32::to_bits).collect();
                    saw_samples |= !a.is_empty();
                    assert_eq!(a, b);
                    assert_eq!(
                        bus_a.pic().as_ref().unwrap().query_interrupt_line(),
                        bus_b.pic().as_ref().unwrap().query_interrupt_line()
                    );
                    assert_eq!(
                        bus_a.pic_mut().as_mut().unwrap().handle_command_register_read(),
                        bus_b.pic_mut().as_mut().unwrap().handle_command_register_read()
                    );
                    assert!(reference.snapshot_state().unwrap() == restored.snapshot_state().unwrap());
                }
                assert!(saw_samples, "native speaker output must be observed");
            }
        }
    }

    #[test]
    fn pit_pending_buffer_restores_fifo_order_and_emitted_sample_bits() {
        for model in [PitType::Model8253, PitType::Model8254] {
            let (mut reference, mut bus_a, audio_a) = setup(model);
            let (mut restored, mut bus_b, audio_b) = setup(model);
            for (pit, bus) in [(&mut reference, &mut bus_a), (&mut restored, &mut bus_b)] {
                pit.control_register_write(0xB6, bus); // channel2, LSB/MSB, square wave
                pit.data_write(2, 5, bus);
                pit.data_write(2, 0, bus);
                pit.channels[2].set_gate(true, bus);
            }
            // The current native producer leaves speaker_buf empty. Seed its
            // storage to test the existing native consumer, without claiming
            // this queue is filled on the normal Pyro sound path.
            reference.speaker_buf.extend([0.25, 0.75]);
            reference.speaker.sample_ct = 23;
            reference.speaker.sample_accum = 4.0;
            for n in 0..64 {
                let saved = reference.snapshot_state().unwrap();
                let decoded = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
                destroy(&mut restored);
                restored.restore_state(&decoded).unwrap();
                // Deliberately compare emitted output BEFORE state equality:
                // reversing saved queue order must fail native audio itself.
                reference.run(&mut bus_a, DeviceRunTimeUnit::SystemTicks(12), None);
                restored.run(&mut bus_b, DeviceRunTimeUnit::SystemTicks(12), None);
                let a: Vec<_> = audio_a.try_iter().map(f32::to_bits).collect();
                let b: Vec<_> = audio_b.try_iter().map(f32::to_bits).collect();
                assert_eq!(a, b, "restored pending queue changes emitted PCM");
                if n == 0 {
                    let live = if reference.get_output_state(2) { 1.0 } else { 0.0 };
                    assert_eq!(a, vec![((4.0f32 + 0.25 + live) / 25.0).to_bits()]);
                    assert_eq!(reference.speaker_buf.iter().copied().collect::<Vec<_>>(), vec![0.75]);
                }
                assert!(reference.snapshot_state().unwrap() == restored.snapshot_state().unwrap());
            }
        }
    }

    #[test]
    fn pit_invalid_state_is_refused_atomically_and_json_is_explicit() {
        let (mut target, _bus, _audio) = setup(PitType::Model8253);
        let before = target.snapshot_state().unwrap();
        let value = serde_json::to_value(&before).unwrap();
        for (field, val) in [
            ("version", serde_json::json!(2)),
            ("clock_divisor", serde_json::json!(4)),
            ("channels", serde_json::json!([])),
            ("chan1_source", serde_json::json!(3)),
            ("speaker_connected", serde_json::json!(false)),
            ("speaker_sample_ct", serde_json::json!(25)),
            ("cycle_accumulator_bits", serde_json::json!(f64::NAN.to_bits())),
            ("speaker_sample_accum_bits", serde_json::json!(f32::INFINITY.to_bits())),
        ] {
            let mut invalid = value.clone();
            invalid[field] = val;
            invalid["pit_cycles"] = serde_json::json!(999);
            assert!(
                target.restore_state(&serde_json::from_value(invalid).unwrap()).is_err(),
                "{field}"
            );
            assert!(
                target.snapshot_state().unwrap() == before,
                "refusal mutated PIT: {field}"
            );
        }
        for field in value.as_object().unwrap().keys() {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<PitState>(missing).is_err(), "{field}");
        }
        let mut future = value;
        future["future_field"] = serde_json::json!(0);
        assert!(serde_json::from_value::<PitState>(future).is_err());
    }

    #[test]
    fn pit_component_preserves_exact_float_bits_and_nested_channel_schema() {
        let (mut target, _bus, _audio) = setup(PitType::Model8253);
        // Nontrivial bit patterns exercise the storage codec; this is not a
        // claim that every seeded value occurs during normal hardware timing.
        target.cycle_accumulator = f64::from_bits(0x3FD0_0000_0000_0001);
        target.timewarp = DeviceRunTimeUnit::Microseconds(f64::from_bits(0x3FE0_0000_0000_0001));
        target.speaker.sample_accum = f32::from_bits(0x3F80_0001);
        target.speaker.sample_ct = 2;
        target.speaker_buf.push_back(f32::from_bits(0x3F00_0001));
        target.set_clock_source(1, Some(0));
        let saved = target.snapshot_state().unwrap();
        let value = serde_json::to_value(&saved).unwrap();
        let decoded: PitState = serde_json::from_value(value.clone()).unwrap();
        destroy(&mut target);
        target.restore_state(&decoded).unwrap();
        assert!(target.snapshot_state().unwrap() == saved);
        for field in value["channels"][0].as_object().unwrap().keys() {
            let mut missing = value.clone();
            missing["channels"][0].as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<PitState>(missing).is_err(), "channel {field}");
        }
        for field in ["val", "dirty"] {
            let mut missing = value.clone();
            missing["channels"][0]["mode"].as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<PitState>(missing).is_err(),
                "Updatable {field}"
            );
        }
        let mut unknown = value;
        unknown["channels"][0]["future_field"] = serde_json::json!(0);
        assert!(serde_json::from_value::<PitState>(unknown).is_err());
    }
}
