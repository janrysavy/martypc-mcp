use super::*;
use crate::devices::pic::Pic;
use std::collections::BTreeSet;

fn write(serial: &mut SerialPortController, port: usize, offset: u16, byte: u8) {
    serial.write_u8(
        [0x3f8, 0x2f8][port] + offset,
        byte,
        None,
        DeviceRunTimeUnit::SystemTicks(0),
        None,
    );
}
fn read(serial: &mut SerialPortController, port: usize, offset: u16) -> u8 {
    serial.read_u8([0x3f8, 0x2f8][port] + offset, DeviceRunTimeUnit::SystemTicks(0))
}
fn init_pic() -> Pic {
    let mut pic = Pic::new();
    for (port, byte) in [(0x20, 0x13), (0x21, 0x20), (0x21, 1), (0x21, 0)] {
        pic.write_u8(port, byte, None, DeviceRunTimeUnit::SystemTicks(0), None);
    }
    pic
}
fn restored(reference: &SerialPortController) -> SerialPortController {
    let bytes = serde_json::to_vec(&reference.snapshot_state().unwrap()).unwrap();
    let saved = serde_json::from_slice(&bytes).unwrap();
    SerialPortController::prepare_restore(&saved).unwrap()
}
fn native_storage(a: &SerialPortController, b: &SerialPortController) {
    // Direct native storage, independent of the codec on either side.
    for (a, b) in a.port.iter().zip(&b.port) {
        macro_rules! same { ($($name:ident),+ $(,)?) => {$(assert_eq!(a.$name,b.$name,stringify!($name));)+}; }
        same!(
            name,
            irq,
            line_control_reg,
            word_length,
            parity_enable,
            divisor_latch_access,
            divisor,
            line_status_reg,
            interrupts_active,
            interrupt_enable_reg,
            modem_control_reg,
            last_dtr,
            last_rts,
            out2_suppresses_int,
            loopback,
            modem_status_reg,
            rx_byte,
            rx_count,
            rx_overrun_count,
            rx_was_read,
            tx_holding_reg,
            tx_last_byte,
            tx_holding_empty,
            rx_queue,
            tx_count,
            tx_queue,
            bridge_port_id
        );
        assert_eq!(a.rx_timer.to_bits(), b.rx_timer.to_bits());
        assert_eq!(a.tx_timer.to_bits(), b.tx_timer.to_bits());
        assert_eq!(a.us_per_byte.to_bits(), b.us_per_byte.to_bits());
        assert_eq!(
            std::mem::discriminant(&a.stop_bits),
            std::mem::discriminant(&b.stop_bits)
        );
        assert_eq!(
            std::mem::discriminant(&a.intr_action),
            std::mem::discriminant(&b.intr_action)
        );
        #[cfg(feature = "serial")]
        {
            assert!(a.bridge_port.is_none() && b.bridge_port.is_none());
            assert_eq!(a.bridge_buf, b.bridge_buf);
            match (&a.bridge_cfg, &b.bridge_cfg) {
                (Some(a), Some(b)) => config_storage(a, b),
                (None, None) => {}
                _ => panic!("bridge config presence"),
            }
        }
    }
    #[cfg(feature = "serial")]
    {
        assert_eq!(a.bridge_configs.len(), b.bridge_configs.len());
        for (name, cfg) in &a.bridge_configs {
            config_storage(cfg, b.bridge_configs.get(name).unwrap());
        }
    }
}
#[cfg(feature = "serial")]
fn config_storage(a: &SerialBridgePortConfiguration, b: &SerialBridgePortConfiguration) {
    assert_eq!(a.host_port_name, b.host_port_name);
    assert_eq!(a.host_port_id, b.host_port_id);
    assert_eq!(a.baud_rate, b.baud_rate);
    assert_eq!(a.stop_bits, b.stop_bits);
    assert_eq!(a.data_bits, b.data_bits);
    assert_eq!(std::mem::discriminant(&a.parity), std::mem::discriminant(&b.parity));
    assert_eq!(
        std::mem::discriminant(&a.flow_control),
        std::mem::discriminant(&b.flow_control)
    );
}
fn tick(a: &mut SerialPortController, b: &mut SerialPortController, pa: &mut Pic, pb: &mut Pic, us: f64) {
    a.run(pa, us);
    b.run(pb, us);
    pa.run(3);
    pb.run(3);
    assert_eq!(pa, pb, "native PIC consumer");
    native_storage(a, b);
}
fn configured(line: u8) -> SerialPortController {
    let mut serial = SerialPortController::new(true);
    for p in 0..2 {
        write(&mut serial, p, 3, line | 0x80);
        write(&mut serial, p, 0, 96);
        write(&mut serial, p, 1, 0);
        write(&mut serial, p, 3, line);
        write(&mut serial, p, 4, 8);
        write(&mut serial, p, 1, 5); // RX data + RX line errors, not TX empty.
    }
    serial
}

#[test]
fn uart_json_continues_partial_rx_tx_on_both_ports() {
    let mut checkpoints = 0;
    for port in 0..2 {
        // Independently known framed durations at 1200 baud: 7N1, 5N1.5, 8P2.
        for (line, period) in [(2, 7500.0), (4, 6250.0), (15, 10000.0)] {
            for phase in [0.0, 0.25, 0.75, 1.0, 1.0001] {
                let mut a = configured(line);
                let mut pa = init_pic();
                assert!((a.port[port].us_per_byte - period).abs() < 1e-9);
                a.queue_rx_bytes(port, &[0x31, 0x72, 0xe3]);
                a.queue_rx_bytes(1 - port, &[0xa4, 0xb5]);
                write(&mut a, port, 0, 0x9c);
                write(&mut a, 1 - port, 0, 0x27);
                a.run(&mut pa, period * phase);
                assert_eq!(a.port[port].rx_count, usize::from(phase > 1.0));
                // Native tx_count counts both the holding write and completion.
                assert_eq!(a.port[port].tx_count, 1 + usize::from(phase > 1.0));
                let mut b = restored(&a);
                let mut pb = pa.clone(); // independent PIC component, not a Machine restore.
                native_storage(&a, &b);
                for step in 0..14 {
                    tick(&mut a, &mut b, &mut pa, &mut pb, period / 4.0);
                    for p in 0..2 {
                        if a.port[p].line_status_reg & STATUS_DATA_READY != 0 {
                            let count = a.port[p].rx_count;
                            let expected = if p == port {
                                [0x31, 0x72, 0xe3][count - 1]
                            } else {
                                [0xa4, 0xb5][count - 1]
                            };
                            assert_eq!(read(&mut a, p, 0), expected, "known RX byte step{step}");
                            assert_eq!(read(&mut b, p, 0), expected);
                        }
                        assert_eq!(read(&mut a, p, 5), read(&mut b, p, 5));
                    }
                    native_storage(&a, &b);
                }
                assert_eq!(a.port[port].rx_count, 3);
                assert_eq!(a.port[1 - port].rx_count, 2);
                assert_eq!(a.port[port].tx_last_byte, 0x9c);
                assert_eq!(a.port[1 - port].tx_last_byte, 0x27);
                assert_eq!(a.port[port].tx_count, 2);
                assert!(a.port[port].rx_queue.is_empty());
                checkpoints += 1;
            }
        }
    }
    println!("UART:30 dual-port partial RX/TX JSON continuations with known bytes and durations");
    assert_eq!(checkpoints, 30);
}

#[test]
fn uart_json_preserves_pending_raise_lower_and_out2_gate() {
    for port in 0..2 {
        for suppressed in [false, true] {
            for out2 in [false, true] {
                let mut a = SerialPortController::new(suppressed);
                let mut pa = init_pic();
                write(&mut a, port, 4, if out2 { 8 } else { 0 });
                write(&mut a, port, 1, 2); // native pending TX-empty raise, if gated on
                let raises = !suppressed || out2;
                let mut b = restored(&a);
                let mut pb = pa.clone();
                native_storage(&a, &b);
                tick(&mut a, &mut b, &mut pa, &mut pb, 0.0);
                let expected = if raises { 1u8 << [4, 3][port] } else { 0 };
                assert_eq!(pa.handle_command_register_read(), expected, "known PIC IRR");
                assert_eq!(pb.handle_command_register_read(), expected);
                assert_eq!(read(&mut a, port, 2), 2); // acknowledges TX-empty -> pending Lower
                assert_eq!(read(&mut b, port, 2), 2);
                b = restored(&a);
                native_storage(&a, &b);
                tick(&mut a, &mut b, &mut pa, &mut pb, 0.0);
                assert!(matches!(a.port[port].intr_action, IntrAction::None));
                assert_eq!(a.port[port].interrupts_active, 0);
            }
        }
    }
    println!("UART:16 pending Raise/Lower JSON checkpoints with native PIC consumers and OUT2 gates");
}

#[test]
fn uart_json_continues_loopback_overrun_and_modem_delta_acknowledgements() {
    for port in 0..2 {
        let mut a = configured(2);
        let mut pa = init_pic();
        write(&mut a, port, 1, 15);
        write(&mut a, port, 4, 0x10);
        write(&mut a, port, 4, 0x1f); // in loopback: DTR/RTS/OUT2 deltas
        write(&mut a, port, 0, 0x42);
        write(&mut a, port, 0, 0x93); // unread RX -> overrun; native first RX also overruns
        assert_eq!(a.port[port].rx_overrun_count, 2);
        let mut b = restored(&a);
        let mut pb = pa.clone();
        native_storage(&a, &b);
        assert_eq!(read(&mut a, port, 2), 6);
        assert_eq!(read(&mut b, port, 2), 6);
        assert_eq!(read(&mut a, port, 5) & 3, 3);
        assert_eq!(read(&mut b, port, 5) & 3, 3);
        assert_eq!(read(&mut a, port, 0), 0x93);
        assert_eq!(read(&mut b, port, 0), 0x93);
        assert_eq!(read(&mut a, port, 6), 0xfb);
        assert_eq!(read(&mut b, port, 6), 0xfb);
        assert_eq!(read(&mut a, port, 6), 0xf0);
        assert_eq!(read(&mut b, port, 6), 0xf0);
        tick(&mut a, &mut b, &mut pa, &mut pb, 7501.0);
        assert_eq!(a.port[port].tx_last_byte, 0x93);
    }
    println!("UART:2 native loopback/overrun/modem acknowledgement JSON continuations");
}

#[test]
fn uart_json_preserves_partial_divisor_and_native_cold_timing_cache() {
    for port in 0..2 {
        let mut a = SerialPortController::new(true);
        let cold = a.port[port].us_per_byte.to_bits();
        let b = restored(&a);
        native_storage(&a, &b);
        assert_eq!(b.port[port].us_per_byte.to_bits(), 1041.66f64.to_bits());
        assert_eq!(cold, b.port[port].us_per_byte.to_bits());
        write(&mut a, port, 3, 0x82);
        write(&mut a, port, 0, 0x34); // checkpoint between low/high divisor writes
        let mut b = restored(&a);
        native_storage(&a, &b);
        assert_eq!(read(&mut a, port, 0), 0x34);
        assert_eq!(read(&mut b, port, 0), 0x34);
        assert_eq!(read(&mut a, port, 1), 0);
        assert_eq!(read(&mut b, port, 1), 0);
        write(&mut a, port, 1, 0x12);
        write(&mut b, port, 1, 0x12);
        assert_eq!(a.port[port].divisor, 0x1234);
        native_storage(&a, &b);
        write(&mut a, port, 3, 2);
        write(&mut b, port, 3, 2);
        a.queue_rx_bytes(port, &[0xc7]);
        b.queue_rx_bytes(port, &[0xc7]);
        let mut pa = init_pic();
        let mut pb = pa.clone();
        tick(&mut a, &mut b, &mut pa, &mut pb, a_period(0x1234) + 1.0);
        assert_eq!(read(&mut a, port, 0), 0xc7);
        assert_eq!(read(&mut b, port, 0), 0xc7);
    }
    println!("UART:2 partial-divisor native continuations and2 cold-cache storage JSON checkpoints");
}
fn a_period(divisor: u16) -> f64 {
    9.0 / (115200.0 / divisor as f64) * 1e6
}

fn native_fields(source: &str, name: &str) -> BTreeSet<String> {
    let body = source.split(&format!("pub struct {name} {{")).nth(1).unwrap();
    let regex = regex::Regex::new(r"^\s+(?:pub\s+)?([a-z_][a-z0-9_]*):").unwrap();
    body.lines()
        .take_while(|line| line.trim() != "}")
        .filter_map(|line| regex.captures(line).map(|c| c[1].to_string()))
        .collect()
}

#[test]
fn uart_schema_and_wire_meanings_cover_every_native_field() {
    let mut a = SerialPortController::new(true);
    // Deliberately seeded inactive metadata/diagnostic storage, not hardware evidence.
    for (p, port) in a.port.iter_mut().enumerate() {
        port.last_dtr = p == 0;
        port.last_rts = p == 1;
        port.rx_was_read = p == 1;
        port.rx_count = 13 + p;
        port.tx_count = 37 + p;
        port.rx_overrun_count = 59 + p;
        port.rx_timer = 123.25 + p as f64;
        port.tx_timer = 287.5 + p as f64;
        port.tx_queue = VecDeque::from([0x51 + p as u8, 0x62 + p as u8]);
        port.bridge_port_id = Some(71 + p);
    }
    let b = restored(&a);
    native_storage(&a, &b);
    let wire = serde_json::to_value(a.snapshot_state().unwrap()).unwrap();
    for (p, port) in a.port.iter().enumerate() {
        let value = &wire["port"][p];
        macro_rules! key { ($($field:ident),+ $(,)?) => {$(assert_eq!(value[stringify!($field)],serde_json::to_value(&port.$field).unwrap(),concat!("wire meaning ",stringify!($field)));)+}; }
        key!(
            name,
            irq,
            line_control_reg,
            word_length,
            parity_enable,
            divisor_latch_access,
            divisor,
            line_status_reg,
            interrupts_active,
            interrupt_enable_reg,
            modem_control_reg,
            last_dtr,
            last_rts,
            out2_suppresses_int,
            loopback,
            modem_status_reg,
            rx_byte,
            rx_count,
            rx_overrun_count,
            rx_was_read,
            tx_holding_reg,
            tx_last_byte,
            tx_holding_empty,
            rx_queue,
            tx_count,
            tx_queue,
            bridge_port_id
        );
        assert_eq!(value["rx_timer"], port.rx_timer.to_bits());
        assert_eq!(value["tx_timer"], port.tx_timer.to_bits());
        assert_eq!(value["us_per_byte"], port.us_per_byte.to_bits());
        assert_eq!(value["intr_action"], "None");
        assert_eq!(value["stop_bits"], "One");
    }
    for source in [
        include_str!("../../serial.rs").replace("\r\n", "\n"),
        include_str!("../../serial.rs")
            .replace("\r\n", "\n")
            .replace('\n', "\r\n"),
    ] {
        let mut expected = native_fields(&source, "SerialPort");
        assert_eq!(expected.len(), 35);
        assert!(expected.remove("bridge_port")); // external handle is explicitly refused
        assert_eq!(expected, wire["port"][0].as_object().unwrap().keys().cloned().collect());
        assert_eq!(
            native_fields(&source, "SerialPortController"),
            ["port".into(), "bridge_configs".into()].into_iter().collect()
        );
        assert_eq!(native_fields(&source, "SerialBridgePortConfiguration").len(), 7);
    }
    println!("UART:1 seeded dual-port storage-only checkpoint, direct fields and wire-key meanings");
}

#[test]
fn uart_strict_schema_and_preflight_reject_without_live_mutation() {
    let a = configured(2);
    let original = a.snapshot_state().unwrap();
    let value = serde_json::to_value(&original).unwrap();
    for path in [vec![], vec!["port", "0"], vec!["port", "1"]] {
        let node = if path.is_empty() {
            &value
        } else {
            &value["port"][path[1].parse::<usize>().unwrap()]
        };
        for field in node.as_object().unwrap().keys() {
            let mut missing = value.clone();
            let obj = if path.is_empty() {
                missing.as_object_mut().unwrap()
            } else {
                missing["port"][path[1].parse::<usize>().unwrap()]
                    .as_object_mut()
                    .unwrap()
            };
            obj.remove(field);
            assert!(
                serde_json::from_value::<SerialControllerState>(missing).is_err(),
                "missing {path:?}/{field}"
            );
        }
        let mut extra = value.clone();
        let obj = if path.is_empty() {
            extra.as_object_mut().unwrap()
        } else {
            extra["port"][path[1].parse::<usize>().unwrap()]
                .as_object_mut()
                .unwrap()
        };
        obj.insert("unknown".into(), serde_json::json!(true));
        assert!(serde_json::from_value::<SerialControllerState>(extra).is_err());
    }
    let mut short = value.clone();
    short["port"].as_array_mut().unwrap().pop();
    assert!(serde_json::from_value::<SerialControllerState>(short).is_err());
    for kind in 0..6 {
        let mut invalid = original.clone();
        match kind {
            0 => invalid.version += 1,
            1 => invalid.serial_feature = !invalid.serial_feature,
            2 => invalid.port[1].rx_timer = f64::NAN.to_bits(),
            3 => invalid.port[1].tx_timer = (-1.0f64).to_bits(),
            4 => invalid.port[1].us_per_byte = 0,
            _ => invalid.port[1].us_per_byte = f64::INFINITY.to_bits(),
        }
        assert!(SerialPortController::prepare_restore(&invalid).is_err());
        assert_eq!(a.snapshot_state().unwrap(), original, "live owner unchanged");
    }
}

#[cfg(feature = "serial")]
#[test]
fn uart_inactive_bridge_configuration_and_buffers_are_preserved_strictly() {
    for (parity, flow) in [
        (ParityType::None, FlowControlType::None),
        (ParityType::Even, FlowControlType::Hardware),
        (ParityType::Odd, FlowControlType::Software),
    ] {
        let mut a = configured(2);
        let cfg = SerialBridgePortConfiguration {
            host_port_name: "saved-host-name".into(),
            host_port_id: Some(23),
            baud_rate: 19200,
            stop_bits: 2,
            data_bits: 7,
            parity,
            flow_control: flow,
        };
        a.set_bridge_port_cfg(&[cfg.clone()]);
        a.port[1].set_bridge_port_cfg(&cfg);
        a.port[1].bridge_buf = vec![0x31, 0x72, 0xa4];
        let b = restored(&a);
        native_storage(&a, &b);
        assert_eq!(b.port[1].bridge_buf, [0x31, 0x72, 0xa4]);
        let value = serde_json::to_value(a.snapshot_state().unwrap()).unwrap();
        assert_eq!(value["port"][1]["bridge_cfg"]["host_port_id"], 23);
        for path in ["port", "map"] {
            let node = if path == "port" {
                &value["port"][1]["bridge_cfg"]
            } else {
                &value["bridge_configs"]["saved-host-name"]
            };
            assert_eq!(
                node.as_object().unwrap().keys().cloned().collect::<BTreeSet<_>>(),
                native_fields(include_str!("../../serial.rs"), "SerialBridgePortConfiguration")
            );
            for field in node.as_object().unwrap().keys() {
                let mut missing = value.clone();
                let obj = if path == "port" {
                    missing["port"][1]["bridge_cfg"].as_object_mut().unwrap()
                } else {
                    missing["bridge_configs"]["saved-host-name"].as_object_mut().unwrap()
                };
                obj.remove(field);
                assert!(
                    serde_json::from_value::<SerialControllerState>(missing).is_err(),
                    "missing bridge {field}"
                );
            }
        }
    }
    println!("UART:3 inactive configured-bridge storage-only JSON checkpoints; no host port opened");
}

#[cfg(feature = "serial")]
#[path = "host_probe.rs"]
mod host_probe;

#[cfg(feature = "serial")]
#[test]
fn uart_open_host_bridge_refused_before_external_io() {
    for port in 0..2 {
        let mut serial = configured(2);
        serial.port[port].bridge_port = Some(Box::new(host_probe::HostProbe));
        assert!(serial
            .snapshot_state()
            .unwrap_err()
            .to_string()
            .contains("open host serial bridge"));
        assert!(serial.port[port].bridge_port.is_some());
        assert_eq!(serial.port[port].divisor, 96);
        serial.port[port].bridge_port = None;
        assert!(serial.snapshot_state().is_ok());
    }
    println!("UART:2 open-handle refusal probes, no real host serial connection");
}
