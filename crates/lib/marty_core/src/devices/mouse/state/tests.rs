use super::*;
use crate::{
    bus::{DeviceRunTimeUnit, IoDevice},
    devices::{pic::Pic, serial::SerialPortController},
};
use std::collections::BTreeSet;

fn restored(mouse: &Mouse) -> Mouse {
    let saved = serde_json::from_slice(&serde_json::to_vec(&mouse.snapshot_state().unwrap()).unwrap()).unwrap();
    Mouse::prepare_restore(&saved).unwrap()
}
fn native_storage(a: &Mouse, b: &Mouse) {
    macro_rules! same {($a:ident,$b:ident;$($name:ident),+ $(,)?)=>{$(assert_eq!($a.$name,$b.$name,stringify!($name));)+};}
    macro_rules! bits {($a:ident,$b:ident;$($name:ident),+ $(,)?)=>{$(assert_eq!($a.$name.to_bits(),$b.$name.to_bits(),stringify!($name));)+};}
    match (a, b) {
        (Mouse::Serial(a), Mouse::Serial(b)) => {
            same!(a,b;left_button,right_button,reported_left_button,reported_right_button,left_press_pending,right_press_pending,rts,port);
            bits!(a,b;speed,pending_x,pending_y,rts_low_timer);
        }
        (Mouse::Virtual(a), Mouse::Virtual(b)) => {
            same!(a,b;irq,input_mode,left_button,right_button,left_press_pending,right_press_pending,event_pending,interrupt_asserted,lower_interrupt,change_counter,consumer_driver_loaded,consumer_range);
            bits!(a,b;speed,x,y,pending_relative_x,pending_relative_y);
        }
        _ => panic!("native mouse variant"),
    }
}
fn uart_write(serial: &mut SerialPortController, port: usize, offset: u16, byte: u8) {
    serial.write_u8(
        [0x3f8, 0x2f8][port] + offset,
        byte,
        None,
        DeviceRunTimeUnit::SystemTicks(0),
        None,
    );
}
fn uart_read(serial: &mut SerialPortController, port: usize, offset: u16) -> u8 {
    serial.read_u8([0x3f8, 0x2f8][port] + offset, DeviceRunTimeUnit::SystemTicks(0))
}
fn uart() -> SerialPortController {
    let mut serial = SerialPortController::new(true);
    for port in 0..2 {
        for (offset, byte) in [(3, 0x82), (0, 96), (1, 0), (3, 2), (4, 10), (1, 5)] {
            uart_write(&mut serial, port, offset, byte);
        }
    }
    serial
}
fn restore_uart(serial: &SerialPortController) -> SerialPortController {
    let saved = serde_json::from_slice(&serde_json::to_vec(&serial.snapshot_state().unwrap()).unwrap()).unwrap();
    SerialPortController::prepare_restore(&saved).unwrap()
}
fn run_serial(mouse: &mut Mouse, serial: &mut SerialPortController, us: f64) {
    if let Mouse::Serial(mouse) = mouse {
        mouse.run(serial, us);
    } else {
        panic!("serial variant");
    }
}
fn run_virtual(mouse: &mut Mouse, pic: &mut Pic) {
    if let Mouse::Virtual(mouse) = mouse {
        mouse.run(pic);
    } else {
        panic!("virtual variant");
    }
}
fn init_pic() -> Pic {
    let mut pic = Pic::new();
    for (port, byte) in [(0x20, 0x13), (0x21, 0x20), (0x21, 1), (0x21, 0)] {
        pic.write_u8(port, byte, None, DeviceRunTimeUnit::SystemTicks(0), None);
    }
    pic
}
fn receive_available(a: &mut SerialPortController, b: &mut SerialPortController, port: usize, out: &mut Vec<u8>) {
    let status = uart_read(a, port, 5);
    assert_eq!(status, uart_read(b, port, 5), "native UART LSR");
    if status & 1 != 0 {
        let byte = uart_read(a, port, 0);
        assert_eq!(byte, uart_read(b, port, 0));
        out.push(byte);
    }
}
fn drain(
    a: &mut Mouse,
    b: &mut Mouse,
    sa: &mut SerialPortController,
    sb: &mut SerialPortController,
    pa: &mut Pic,
    pb: &mut Pic,
    port: usize,
) -> Vec<u8> {
    let mut bytes = vec![];
    receive_available(sa, sb, port, &mut bytes);
    for _ in 0..72 {
        run_serial(a, sa, 0.0);
        run_serial(b, sb, 0.0);
        native_storage(a, b);
        sa.run(pa, 1875.0);
        sb.run(pb, 1875.0);
        pa.run(3);
        pb.run(3);
        assert_eq!(pa, pb, "separate native PIC consumer");
        receive_available(sa, sb, port, &mut bytes);
    }
    assert_eq!(sa.snapshot_state().unwrap(), sb.snapshot_state().unwrap());
    bytes
}

#[test]
fn mouse_serial_json_continues_busy_uart_fractional_motion_and_click_packets() {
    for port in 0..2 {
        for speed in [1.0, 2.0] {
            for phase in [0.0, 0.25, 1.0, 1.0001] {
                for click in [false, true] {
                    let mut a = Mouse::Serial(SerialMouse::new(port, Some(speed)));
                    a.submit_input(MouseInput {
                        delta_x: 600.0,
                        delta_y: -400.0,
                        left_pressed_since: click,
                        ..MouseInput::default()
                    });
                    let mut sa = uart();
                    let mut pa = init_pic();
                    run_serial(&mut a, &mut sa, 0.0); // queues first 3-byte packet
                    sa.run(&mut pa, 7500.0 * phase);
                    let mut b = restored(&a);
                    let mut sb = restore_uart(&sa);
                    let mut pb = pa.clone();
                    native_storage(&a, &b);
                    let bytes = drain(&mut a, &mut b, &mut sa, &mut sb, &mut pa, &mut pb, port);
                    // At both speeds first X saturates127, Y saturates-128.
                    // Binary64 subtraction preserves native fractional carry.
                    // At speed2 the final scaled X is 68.99999999999994:
                    // emit68 and retain its remainder, rather than rounding69.
                    let mut expected = vec![if click { 0x69 } else { 0x49 }, 63, 0];
                    if speed == 1.0 {
                        expected.extend([0x4d, 34, 42]);
                    } else {
                        expected.extend([0x49, 63, 0, 0x4d, 63, 20, 0x41, 4, 0]);
                    }
                    assert_eq!(
                        bytes, expected,
                        "independent packet stream port{port}/speed{speed}/phase{phase}/click{click}"
                    );
                    if let Mouse::Serial(mouse) = &a {
                        assert!(mouse.pending_packet().is_none());
                        // Independently computed IEEE754 residuals, including
                        // the sub-count motion that cannot yet form a packet.
                        assert_eq!(
                            mouse.pending_x.to_bits(),
                            if speed == 1.0 { 0 } else { 4608683618675807168 }
                        );
                        assert_eq!(mouse.pending_y.to_bits(), 13625640672609435648);
                    }
                }
            }
        }
    }
    println!(
        "MOUSE:32 joint serial mouse/UART JSON continuations; known saturated motion and quick-click packet bytes"
    );
}

#[test]
fn mouse_serial_json_preserves_rts_reset_threshold_with_busy_queue() {
    for port in 0..2 {
        for elapsed in [9999.0, 10000.0, 10000.25, 10001.0] {
            let mut a = Mouse::Serial(SerialMouse::new(port, None));
            let mut sa = uart();
            let mut pa = init_pic();
            sa.queue_rx_bytes(port, &[0xa7]); // busy line prevents consuming pending mouse packet
            uart_write(&mut sa, port, 4, 8); // RTS low
            a.submit_input(MouseInput {
                delta_x: 3.0,
                left_pressed_since: true,
                ..MouseInput::default()
            });
            run_serial(&mut a, &mut sa, elapsed);
            let mut b = restored(&a);
            let mut sb = restore_uart(&sa);
            let mut pb = pa.clone();
            native_storage(&a, &b);
            uart_write(&mut sa, port, 4, 10);
            uart_write(&mut sb, port, 4, 10);
            run_serial(&mut a, &mut sa, 0.0);
            run_serial(&mut b, &mut sb, 0.0);
            native_storage(&a, &b);
            let bytes = drain(&mut a, &mut b, &mut sa, &mut sb, &mut pa, &mut pb, port);
            let expected = if elapsed > 10000.0 {
                vec![0xa7, 0x4d]
            } else {
                vec![0xa7, 0x60, 1, 0, 0x40, 0, 0]
            };
            assert_eq!(bytes, expected, "strict RTS reset threshold {elapsed}");
        }
    }
    println!("MOUSE:8 joint serial/UART RTS threshold continuations with independent reset/packet bytes");
}

#[test]
fn mouse_virtual_json_continues_motion_quick_click_and_pending_irq_ack() {
    for speed in [0.5, 1.0] {
        for mode in [VirtualMouseInputMode::Absolute, VirtualMouseInputMode::Relative] {
            for stage in 0..3 {
                let mut a = Mouse::Virtual(VirtualMouse::new(5, Some(speed)));
                let mut pa = init_pic();
                a.set_virtual_input_mode(mode);
                if let Mouse::Virtual(mouse) = &mut a {
                    mouse.set_consumer_status(true);
                    mouse.set_consumer_range(VirtualMouseConsumerRange {
                        min_x: 0,
                        max_x: 639,
                        min_y: 0,
                        max_y: 199,
                    });
                }
                a.submit_input(MouseInput {
                    delta_x: 200000.75,
                    delta_y: -200000.25,
                    left_pressed_since: true,
                    ..MouseInput::default()
                });
                if stage >= 1 {
                    run_virtual(&mut a, &mut pa);
                    pa.run(3);
                }
                if stage == 2 {
                    a.take_virtual_state().unwrap();
                }
                let mut b = restored(&a);
                let mut pb = pa.clone();
                native_storage(&a, &b);
                run_virtual(&mut a, &mut pa);
                run_virtual(&mut b, &mut pb);
                pa.run(3);
                pb.run(3);
                assert_eq!(pa, pb);
                let ir = u8::from_str_radix(&pa.get_string_state().ir, 2).unwrap();
                assert_eq!(ir, if stage == 2 { 0 } else { 1 << 5 }, "native IRQ line");
                let x = a.take_virtual_state().unwrap();
                let y = b.take_virtual_state().unwrap();
                assert_eq!(x, y);
                assert_eq!((x.x, x.y), (65535, 0));
                assert_eq!(x.input_mode, mode);
                assert_eq!(x.buttons, if stage == 2 { 0 } else { VMOUSE_BUTTON_LEFT });
                let expected = if mode == VirtualMouseInputMode::Absolute {
                    (0, 0)
                } else if stage == 2 && speed == 0.5 {
                    (4733, -4732)
                } else {
                    (32767, -32768)
                };
                assert_eq!((x.relative_x, x.relative_y), expected, "independent scaled report");
                native_storage(&a, &b);
                for _ in 0..4 {
                    run_virtual(&mut a, &mut pa);
                    run_virtual(&mut b, &mut pb);
                    assert_eq!(pa, pb);
                    native_storage(&a, &b);
                }
            }
        }
    }
    println!("MOUSE:12 virtual native IRQ/report continuations, clamped positions/scaled counts/quick-click halves");
}

fn native_fields(name: &str) -> BTreeSet<String> {
    let source = include_str!("../../mouse.rs");
    let body = source.split(&format!("pub struct {name} {{")).nth(1).unwrap();
    let regex = regex::Regex::new(r"^\s+(?:pub\s+)?([a-z_][a-z0-9_]*):").unwrap();
    body.lines()
        .take_while(|line| line.trim() != "}")
        .filter_map(|line| regex.captures(line).map(|c| c[1].to_owned()))
        .collect()
}
fn wire_oracle(mouse: &Mouse) {
    let wire = serde_json::to_value(mouse.snapshot_state().unwrap()).unwrap();
    let value = &wire["body"]["state"];
    macro_rules! key {($m:ident;$($name:ident),+ $(,)?)=>{$(assert_eq!(value[stringify!($name)],serde_json::to_value(&$m.$name).unwrap(),concat!("wire meaning ",stringify!($name)));)+};}
    macro_rules! bits {($m:ident;$($name:ident),+ $(,)?)=>{$(assert_eq!(value[stringify!($name)],$m.$name.to_bits(),concat!("wire bits ",stringify!($name)));)+};}
    match mouse {
        Mouse::Serial(m) => {
            key!(m;left_button,right_button,reported_left_button,reported_right_button,left_press_pending,right_press_pending,rts,port);
            bits!(m;speed,pending_x,pending_y,rts_low_timer);
            assert_eq!(wire["body"]["kind"], "Serial");
            assert_eq!(
                value.as_object().unwrap().keys().cloned().collect::<BTreeSet<_>>(),
                native_fields("SerialMouse")
            );
            assert_eq!(native_fields("SerialMouse").len(), 12);
        }
        Mouse::Virtual(m) => {
            key!(m;irq,left_button,right_button,left_press_pending,right_press_pending,event_pending,interrupt_asserted,lower_interrupt,change_counter,consumer_driver_loaded);
            bits!(m;speed,x,y,pending_relative_x,pending_relative_y);
            assert_eq!(
                value["input_mode"],
                match m.input_mode {
                    VirtualMouseInputMode::Absolute => "Absolute",
                    VirtualMouseInputMode::Relative => "Relative",
                }
            );
            assert_eq!(wire["body"]["kind"], "Virtual");
            assert_eq!(
                value.as_object().unwrap().keys().cloned().collect::<BTreeSet<_>>(),
                native_fields("VirtualMouse")
            );
            assert_eq!(native_fields("VirtualMouse").len(), 17);
            if let Some(range) = m.consumer_range {
                assert_eq!(
                    value["consumer_range"]
                        .as_object()
                        .unwrap()
                        .keys()
                        .cloned()
                        .collect::<BTreeSet<_>>(),
                    native_fields("VirtualMouseConsumerRange")
                );
                assert_eq!(value["consumer_range"]["min_x"], range.min_x);
                assert_eq!(value["consumer_range"]["max_x"], range.max_x);
                assert_eq!(value["consumer_range"]["min_y"], range.min_y);
                assert_eq!(value["consumer_range"]["max_y"], range.max_y);
            } else {
                assert!(value["consumer_range"].is_null());
            }
        }
    }
}

#[test]
fn mouse_inventory_and_onehot_metadata_have_independent_wire_meanings() {
    for selected in 0..7 {
        let mut m = SerialMouse::new(1, Some(-0.0));
        m.pending_x = 13.25;
        m.pending_y = -27.5;
        m.rts_low_timer = 31.75;
        match selected {
            0 => m.left_button = true,
            1 => m.right_button = true,
            2 => m.reported_left_button = true,
            3 => m.reported_right_button = true,
            4 => m.left_press_pending = true,
            5 => m.right_press_pending = true,
            _ => m.rts = true,
        }
        let a = Mouse::Serial(m);
        let b = restored(&a);
        native_storage(&a, &b);
        wire_oracle(&a);
    }
    for selected in 0..8 {
        let mut m = VirtualMouse::new(3, Some(-0.0));
        m.input_mode = VirtualMouseInputMode::Relative;
        m.x = 11.25;
        m.y = 19.5;
        m.pending_relative_x = -31.75;
        m.pending_relative_y = 47.125;
        m.change_counter = 54321;
        // Setter accepts any diagnostic range; do not invent a normalization.
        m.consumer_range = Some(VirtualMouseConsumerRange {
            min_x: 7,
            max_x: 3,
            min_y: 11,
            max_y: 5,
        });
        match selected {
            0 => m.left_button = true,
            1 => m.right_button = true,
            2 => m.left_press_pending = true,
            3 => m.right_press_pending = true,
            4 => m.event_pending = true,
            5 => m.interrupt_asserted = true,
            6 => m.lower_interrupt = true,
            _ => m.consumer_driver_loaded = true,
        }
        let a = Mouse::Virtual(m);
        let b = restored(&a);
        native_storage(&a, &b);
        wire_oracle(&a);
    }
    println!("MOUSE:15 deliberately seeded one-hot/signed-zero/range storage-only JSON checkpoints;29 native fields and wire meanings");
}

#[test]
fn mouse_schema_requires_every_key_and_rejects_invalid_state_atomically() {
    for mouse in [
        Mouse::Serial(SerialMouse::new(0, None)),
        Mouse::Virtual(VirtualMouse::new(5, None)),
    ] {
        let saved = mouse.snapshot_state().unwrap();
        let value = serde_json::to_value(&saved).unwrap();
        for path in [vec![], vec!["body"], vec!["body", "state"]] {
            let mut node = &value;
            for key in &path {
                node = &node[*key];
            }
            for field in node.as_object().unwrap().keys() {
                let mut missing = value.clone();
                let mut target = &mut missing;
                for key in &path {
                    target = &mut target[*key];
                }
                target.as_object_mut().unwrap().remove(field);
                assert!(
                    serde_json::from_value::<MouseState>(missing).is_err(),
                    "missing {path:?}/{field}"
                );
            }
            let mut extra = value.clone();
            let mut target = &mut extra;
            for key in &path {
                target = &mut target[*key];
            }
            target
                .as_object_mut()
                .unwrap()
                .insert("unknown".into(), serde_json::json!(true));
            assert!(serde_json::from_value::<MouseState>(extra).is_err());
        }
        let mut version = saved.clone();
        version.version += 1;
        assert!(Mouse::prepare_restore(&version).is_err());
        for kind in 0..3 {
            let mut invalid = saved.clone();
            match &mut invalid.body {
                BodyState::Serial(s) => match kind {
                    0 => s.port = 2,
                    1 => s.speed = f32::NAN.to_bits(),
                    _ => s.pending_x = f64::INFINITY.to_bits(),
                },
                BodyState::Virtual(s) => match kind {
                    0 => s.irq = 8,
                    1 => s.speed = f32::INFINITY.to_bits(),
                    _ => s.pending_relative_y = f64::NAN.to_bits(),
                },
            }
            assert!(Mouse::prepare_restore(&invalid).is_err());
            assert_eq!(mouse.snapshot_state().unwrap(), saved, "live owner unchanged");
        }
    }
    let mut m = VirtualMouse::new(5, None);
    m.set_consumer_range(VirtualMouseConsumerRange::default());
    let value = serde_json::to_value(Mouse::Virtual(m).snapshot_state().unwrap()).unwrap();
    for key in ["min_x", "max_x", "min_y", "max_y"] {
        let mut missing = value.clone();
        missing["body"]["state"]["consumer_range"]
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(serde_json::from_value::<MouseState>(missing).is_err());
    }
    let mut extra = value.clone();
    extra["body"]["state"]["consumer_range"]["extra"] = serde_json::json!(1);
    assert!(serde_json::from_value::<MouseState>(extra).is_err());
    let mut unknown = value;
    unknown["body"]["state"]["input_mode"] = serde_json::json!("invented");
    assert!(serde_json::from_value::<MouseState>(unknown).is_err());
}

#[test]
fn mouse_json_preserves_fractional_and_finite_constructor_values_without_normalizing() {
    for speed in [-1.0, -0.0, 0.0, 0.01, 1.234567] {
        for mouse in [
            Mouse::Serial(SerialMouse::new(1, Some(speed))),
            Mouse::Virtual(VirtualMouse::new(7, Some(speed))),
        ] {
            let next = restored(&mouse);
            native_storage(&mouse, &next);
            wire_oracle(&mouse);
        }
    }
    println!(
        "MOUSE:10 finite constructor/signed-zero storage-only checkpoints; no invalid-speed hardware behavior claim"
    );
}
