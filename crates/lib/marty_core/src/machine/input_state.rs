//! Native machine keyboard/macro FIFO plus keyboard device/update clock.
//! Other machine, CPU, bus, PPI/PIC/A0 and disk state is not captured here.
//! Call only between completed native calls: the caller's per-frame
//! kb_event_processed flag belongs to execution orchestration, not this FIFO.
//! translate metadata is retained; native bus key-down ignores that flag.

use super::*;
use crate::bus::KeyboardBusState;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KeyboardInputState {
    version: u32,
    machine_type: MachineType,
    queue: VecDeque<KeybufferEntry>,
    bus: KeyboardBusState,
}

impl Machine {
    pub(crate) fn snapshot_keyboard_input_state(&self) -> Result<KeyboardInputState, &'static str> {
        Ok(KeyboardInputState {
            version: 1,
            machine_type: self.machine_type,
            queue: self.kb_buf.clone(),
            bus: self.cpu.bus().snapshot_keyboard_bus_state()?,
        })
    }

    pub(crate) fn preflight_keyboard_input_state(&self, saved: &KeyboardInputState) -> Result<(), &'static str> {
        if saved.version != 1 || saved.machine_type != self.machine_type {
            return Err("incompatible keyboard input version/machine");
        }
        self.cpu.bus().preflight_keyboard_bus_state(&saved.bus)
    }

    pub(crate) fn restore_keyboard_input_state(&mut self, saved: &KeyboardInputState) -> Result<(), &'static str> {
        self.preflight_keyboard_input_state(saved)?;
        self.cpu.bus_mut().restore_keyboard_bus_state(&saved.bus)?;
        self.kb_buf = saved.queue.clone();
        Ok(())
    }
}

#[cfg(all(test, not(feature = "cpu_validator")))]
mod tests {
    use super::*;
    use crate::{
        bus::{DeviceRunTimeUnit, IoDevice, KB_UPDATE_RATE},
        cpu_validator::ValidatorType,
        device_types::keyboard::KeyboardType,
        machine_config::KeyboardConfig,
    };
    use marty_common::types::joystick::ControllerLayout;

    // Use the real MachineBuilder/new/install_devices paths without a circular
    // test dependency on the frontend config crate or any ROM/filesystem input.
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
            true
        }
        fn get_machine_turbo(&self) -> bool {
            false
        }
        fn get_service_interrupt(&self) -> Option<u8> {
            None
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
            false
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

    const BOARDS: [(MachineType, KeyboardType); 3] = [
        (MachineType::Ibm5160, KeyboardType::ModelF),
        (MachineType::Tandy1000, KeyboardType::Tandy1000),
        (MachineType::IbmPCJr, KeyboardType::Pcjr),
    ];

    fn mapping() -> String {
        let mut text = String::new();
        for section in ["modelf", "tandy1000", "pcjr"] {
            for (key, modifiers, macro_keys, code) in [
                // Native mapping loop uses the LAST matching entry.
                ("KeyA", "\"any\"", "", "51"),
                ("KeyA", "\"control\",\"alt\",\"shift\",\"meta\"", "", "119"),
                ("KeyC", "\"any\"", "", "68"),
                ("F10", "\"any\"", "\"+KeyC\",\"-KeyC\"", ""),
            ] {
                text.push_str(&format!("\n[[keyboard.{section}.keycode_mappings]]\nkeycode=\"{key}\"\nmodifiers=[{modifiers}]\nkey_macro=[{macro_keys}]\nmacro_translate=false\nscancodes=[{code}]\n"));
            }
        }
        text
    }

    fn machine(board: (MachineType, KeyboardType)) -> Machine {
        let config = NoRoms(board.0);
        let description = MachineConfiguration {
            machine_type: board.0,
            keyboard: Some(KeyboardConfig {
                kb_type: board.1,
                layout: String::new(),
                typematic: true,
                typematic_delay: Some(5.0),
                typematic_rate: Some(2.0),
            }),
            ..Default::default()
        };
        let mut machine = MachineBuilder::new()
            .with_core_config(Box::new(&config))
            .with_machine_config(&description)
            .with_roms(MachineRomManifest::new())
            .with_keyboard_layout(Some(mapping()))
            .build()
            .unwrap();
        write_ppi(&mut machine, 0x48);
        let pic = machine.cpu.bus_mut().pic_mut().as_mut().unwrap();
        pic.handle_command_register_write(0x13);
        pic.handle_data_register_write(8);
        pic.handle_data_register_write(1);
        pic.handle_data_register_write(0);
        machine
    }

    fn write_ppi(machine: &mut Machine, byte: u8) {
        machine.cpu.bus_mut().ppi_mut().as_mut().unwrap().write_u8(
            0x61,
            byte,
            None,
            DeviceRunTimeUnit::SystemTicks(0),
            None,
        );
    }

    fn input_json(machine: &Machine) -> serde_json::Value {
        serde_json::to_value(machine.snapshot_keyboard_input_state().unwrap()).unwrap()
    }

    fn restore_destructively(machine: &mut Machine) {
        let wire = serde_json::to_vec(&machine.snapshot_keyboard_input_state().unwrap()).unwrap();
        machine.kb_buf.clear();
        machine.cpu.bus_mut().destroy_keyboard_component_for_test();
        machine
            .restore_keyboard_input_state(&serde_json::from_slice(&wire).unwrap())
            .unwrap();
    }

    fn observe(machine: &mut Machine) -> serde_json::Value {
        let bus = machine.cpu.bus_mut();
        let ppi = bus.ppi_mut().as_mut().unwrap();
        let ports: Vec<_> = [0x60, 0x61, 0x62]
            .into_iter()
            .map(|port| ppi.read_u8(port, DeviceRunTimeUnit::SystemTicks(0)))
            .collect();
        let ppi = serde_json::to_value(ppi.snapshot_state().unwrap()).unwrap();
        let pic = serde_json::to_value(bus.pic_mut().as_ref().unwrap().snapshot_state().unwrap()).unwrap();
        serde_json::json!({"ports": ports, "ppi":ppi, "pic":pic,
            "cpu": format!("{:?}", machine.cpu.get_string_state()), "ticks": machine.system_ticks,
            "input": input_json(machine)})
    }

    #[test]
    fn input_json_restore_continues_native_macro_fifo_and_hardware_delivery() {
        for board in BOARDS {
            let mut reference = machine(board);
            let mut restored = machine(board);
            let (mut frame_a, mut frame_b) = (false, false);
            let mut macro_seen = false;
            for n in 0..128 {
                if n % 16 == 0 {
                    for machine in [&mut reference, &mut restored] {
                        machine.emit_key_sequence(&[MartyKey::F10, MartyKey::KeyA]);
                        machine.key_press(
                            MartyKey::KeyA,
                            KeyboardModifiers {
                                control: true,
                                alt: true,
                                shift: true,
                                meta: true,
                            },
                        );
                        machine.key_release(MartyKey::KeyA);
                    }
                }
                restore_destructively(&mut restored); // input only; native peers retained
                if n % 4 == 0 {
                    frame_a = false;
                    frame_b = false;
                }
                if n % 8 == 2 {
                    for machine in [&mut reference, &mut restored] {
                        write_ppi(machine, 0xc8);
                    }
                } else if n % 8 == 3 {
                    for machine in [&mut reference, &mut restored] {
                        write_ppi(machine, 0x48);
                    }
                }
                let cycles = [1, 64, 32, 100, 25000, 64, 8, 3][n % 8];
                assert_eq!(
                    reference.run_devices(cycles, &mut frame_a),
                    restored.run_devices(cycles, &mut frame_b)
                );
                assert_eq!(frame_a, frame_b);
                macro_seen |= reference.kb_buf.iter().any(|event| event.keycode == MartyKey::KeyC);
                assert_eq!(observe(&mut reference), observe(&mut restored), "board={board:?} n={n}");
            }
            assert!(macro_seen, "native keyboard must actually append macro entries");
        } //384 destructive JSON restores; CPU/PPI/PIC/A0/other peers are NOT restored
    }

    fn port_a(machine: &mut Machine) -> u8 {
        machine
            .cpu
            .bus_mut()
            .ppi_mut()
            .as_mut()
            .unwrap()
            .read_u8(0x60, DeviceRunTimeUnit::SystemTicks(0))
    }

    #[test]
    fn input_restore_preserves_fifo_key_flags_modifiers_and_frame_gate() {
        let mut reference = machine(BOARDS[0]);
        let mut restored = machine(BOARDS[0]);
        for machine in [&mut reference, &mut restored] {
            machine.key_press(
                MartyKey::KeyA,
                KeyboardModifiers {
                    control: true,
                    alt: true,
                    shift: true,
                    meta: true,
                },
            );
            machine.key_release(MartyKey::KeyA);
        }
        restore_destructively(&mut restored);
        let (mut frame_a, mut frame_b) = (false, false);
        reference.run_devices(1, &mut frame_a);
        restored.run_devices(1, &mut frame_b);
        assert_eq!(port_a(&mut reference), 0x77);
        assert_eq!(port_a(&mut restored), 0x77, "restored first FIFO key/modifiers");
        assert_eq!(reference.kb_buf.len(), 1);
        assert_eq!(restored.kb_buf.len(), 1);
        restore_destructively(&mut restored);
        reference.run_devices(1, &mut frame_a);
        restored.run_devices(1, &mut frame_b);
        assert_eq!(reference.kb_buf.len(), 1); // same frame: no second event
        assert_eq!(input_json(&reference), input_json(&restored));
        for machine in [&mut reference, &mut restored] {
            write_ppi(machine, 0xc8);
            machine.run_devices(1, &mut true);
            write_ppi(machine, 0x48);
        }
        frame_a = false;
        frame_b = false;
        reference.run_devices(1, &mut frame_a);
        restored.run_devices(1, &mut frame_b);
        assert_eq!(port_a(&mut reference), 0xf7);
        assert_eq!(port_a(&mut restored), 0xf7, "restored FIFO release");
        assert!(reference.kb_buf.is_empty());
        assert_eq!(observe(&mut reference), observe(&mut restored));
    }

    #[test]
    fn input_restore_preserves_native_keyboard_poll_deadline() {
        let mut reference = machine(BOARDS[0]);
        let mut restored = machine(BOARDS[0]);
        for machine in [&mut reference, &mut restored] {
            let keyboard = machine.cpu.bus_mut().keyboard_mut().unwrap();
            keyboard.key_down(MartyKey::KeyA, &KeyboardModifiers::default(), None);
            assert_eq!(keyboard.recv_scancode(), Some(0x33));
            machine
                .cpu
                .bus_mut()
                .run_devices(KB_UPDATE_RATE - 0.25, 0, None, &mut machine.kb_buf, None);
            assert_eq!(port_a(machine), 0);
        }
        restore_destructively(&mut restored);
        reference.run_devices(2, &mut false); // native CPU clock crosses bus poll threshold
        restored.run_devices(2, &mut false);
        assert_eq!(port_a(&mut reference), 0x33);
        assert_eq!(port_a(&mut restored), 0x33, "restored polling phase");
        assert_eq!(observe(&mut reference), observe(&mut restored));
    }

    #[test]
    fn input_schema_and_invalid_nested_restores_are_atomic() {
        let mut machine = machine(BOARDS[0]);
        machine.emit_key_sequence(&[MartyKey::KeyA, MartyKey::KeyB]);
        let saved = input_json(&machine);
        for pointer in ["", "/bus", "/queue/0", "/queue/0/modifiers"] {
            let object = saved.pointer(pointer).unwrap().as_object().unwrap();
            for key in object.keys() {
                let mut missing = saved.clone();
                missing
                    .pointer_mut(pointer)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
                assert!(
                    serde_json::from_value::<KeyboardInputState>(missing).is_err(),
                    "missing {pointer}/{key}"
                );
            }
            let mut extra = saved.clone();
            extra
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("unknown".into(), true.into());
            assert!(serde_json::from_value::<KeyboardInputState>(extra).is_err());
        }
        for n in 0..8 {
            let mut invalid = saved.clone();
            match n {
                0 => invalid["version"] = 2.into(),
                1 => invalid["machine_type"] = "IbmPCJr".into(),
                2 => invalid["bus"]["version"] = 2.into(),
                3 => invalid["bus"]["keyboard_type"] = "Pcjr".into(),
                4 => invalid["bus"]["keyboard"] = serde_json::Value::Null,
                5 => invalid["bus"]["keyboard"]["version"] = 2.into(),
                6 => invalid["bus"]["keyboard"]["keyboard"]["kb_type"] = "Pcjr".into(),
                _ => invalid["bus"]["keyboard"]["keyboard"]["kb_buffer_size"] = 0.into(),
            }
            invalid["queue"] = serde_json::json!([]); // would visibly destroy live queue if preflight were late
            let decoded: KeyboardInputState = serde_json::from_value(invalid).unwrap();
            assert!(machine.restore_keyboard_input_state(&decoded).is_err());
            assert_eq!(input_json(&machine), saved);
        }
        let mut nan = saved;
        nan["bus"]["kb_us_accum"] = f64::NAN.to_bits().into();
        assert!(serde_json::from_value::<KeyboardInputState>(nan).is_err());
    }
}
