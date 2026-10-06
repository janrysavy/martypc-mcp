use super::tests::{machine, machine_with_keyboard, machine_with_keyboard_on};
use super::*;

fn call(a: &mut Agent, m: &mut Machine, method: &str, params: Value) -> Value {
    a.handle(m, method, &params).unwrap()
}

#[test]
fn raw_xt_fifo_waits_for_guest_ack_including_zero_byte() {
    let mut m = machine_with_keyboard();
    m.bus_mut().ppi_mut().as_mut().unwrap().handle_portb_write(0x40);
    m.bus_mut()
        .keyboard_mut()
        .unwrap()
        .queue_rpc_scancodes(&[0, 0x80, 0x1e, 0x9e])
        .unwrap();
    m.bus_mut().process_keyboard_input();
    assert_eq!(m.bus_mut().keyboard_mut().unwrap().pending_rpc_scancodes(), 3);
    for _ in 0..8 {
        m.bus_mut().process_keyboard_input();
    }
    assert_eq!(m.bus_mut().keyboard_mut().unwrap().pending_rpc_scancodes(), 3);
    assert!(!m.bus_mut().ppi_mut().as_ref().unwrap().keyboard_latch_ready());
    // Holding PB7 inhibits delivery even after the scheduled acknowledgement.
    m.bus_mut().ppi_mut().as_mut().unwrap().handle_portb_write(0xc0);
    m.bus_mut().run_devices(1.0, 1, None, &mut VecDeque::new(), None);
    m.bus_mut().process_keyboard_input();
    assert_eq!(m.bus_mut().keyboard_mut().unwrap().pending_rpc_scancodes(), 3);
    m.bus_mut().ppi_mut().as_mut().unwrap().handle_portb_write(0x40);
    m.bus_mut().process_keyboard_input();
    assert_eq!(m.bus_mut().keyboard_mut().unwrap().pending_rpc_scancodes(), 2);
}

#[test]
fn pending_native_make_is_preserved_ahead_of_raw_xt_fifo() {
    use marty_core::{devices::keyboard_common::KeyboardModifiers, keys::MartyKey};
    let mut m = machine_with_keyboard();
    m.bus_mut().ppi_mut().as_mut().unwrap().handle_portb_write(0x40);
    m.bus_mut()
        .keyboard_mut()
        .unwrap()
        .queue_rpc_scancodes(&[0, 0x80])
        .unwrap();
    m.bus_mut().process_keyboard_input();
    m.bus_mut()
        .keyboard_mut()
        .unwrap()
        .key_down(MartyKey::KeyA, &KeyboardModifiers::default(), None);
    m.bus_mut().process_keyboard_input(); // Busy: native make must remain pending.
    m.bus_mut().ppi_mut().as_mut().unwrap().handle_portb_write(0xc0);
    m.bus_mut().run_devices(1.0, 1, None, &mut VecDeque::new(), None);
    m.bus_mut().ppi_mut().as_mut().unwrap().handle_portb_write(0x40);
    m.bus_mut().process_keyboard_input();
    assert_eq!(m.bus_mut().ppi_mut().as_ref().unwrap().handle_porta_read(), 0x1e);
    assert_eq!(m.bus_mut().keyboard_mut().unwrap().pending_rpc_scancodes(), 1);
}

#[test]
fn raw_xt_fifo_preserves_pending_native_reset_reply() {
    let mut m = machine_with_keyboard();
    m.bus_mut().ppi_mut().as_mut().unwrap().handle_portb_write(0);
    m.bus_mut().run_devices(2000.0, 1, None, &mut VecDeque::new(), None);
    m.bus_mut().ppi_mut().as_mut().unwrap().handle_portb_write(0x40);
    m.bus_mut()
        .keyboard_mut()
        .unwrap()
        .queue_rpc_scancodes(&[0x1e, 0x9e])
        .unwrap();
    m.bus_mut().process_keyboard_input();
    assert_eq!(m.bus_mut().keyboard_mut().unwrap().pending_rpc_scancodes(), 2);
    m.bus_mut().run_devices(1001.0, 1, None, &mut VecDeque::new(), None);
    assert!(
        !m.bus_mut().ppi_mut().as_ref().unwrap().keyboard_latch_ready(),
        "reset reply occupies PPI until acknowledgement"
    );
    m.bus_mut().process_keyboard_input();
    assert_eq!(m.bus_mut().keyboard_mut().unwrap().pending_rpc_scancodes(), 2);
    m.bus_mut().ppi_mut().as_mut().unwrap().handle_portb_write(0xc0);
    m.bus_mut().run_devices(1.0, 1, None, &mut VecDeque::new(), None);
    m.bus_mut().ppi_mut().as_mut().unwrap().handle_portb_write(0x40);
    m.bus_mut().process_keyboard_input();
    assert_eq!(m.bus_mut().keyboard_mut().unwrap().pending_rpc_scancodes(), 1);
}

#[test]
fn raw_xt_bytes_reach_native_irq1_and_complete_cold_restart() {
    // Independently assembled 8086 probe installs IRQ1, reads port60h,
    // acknowledges PB7, records raw bytes and EOIs the ordinary PIC.
    let encoded="fa31c08ed88ec08ed0bc0080c70624008001c70626000000b013e620b008e621b001e621b0fde621b040e661fbebfe90909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909050531e31c08ed88b1e8002e46088870003ff068002e46188c40c80e66188e0e661b020e6201f5b58cf";
    let code: Vec<u8> = encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    let mut m = machine_with_keyboard();
    m.load_program(&code, 0, 0x100, 0, 0x100).unwrap();
    let mut a = Agent::new(1);
    call(
        &mut a,
        &mut m,
        "input.keyboard",
        json!({"events":[
        {"scan_code":0,"pressed":true},{"scan_code":0,"pressed":false},
        {"scan_code":30,"pressed":true},{"scan_code":30,"pressed":false}]}),
    );
    // Stop at a real native boundary with a byte in the PPI and IRQ1
    // pending. No probe-side register, BIOS-ring or port mutations.
    let mut reached = false;
    let mut prior_ns = emulated_ns(&m, m.system_ticks());
    let mut first_delivery = None;
    for _ in 0..10000 {
        a.step(&mut m, None);
        if m.bus_mut().keyboard_mut().unwrap().pending_rpc_scancodes() == 3 {
            reached = true;
            first_delivery = Some((prior_ns, emulated_ns(&m, m.system_ticks())));
            break;
        }
        prior_ns = emulated_ns(&m, m.system_ticks());
    }
    let (before_ns, after_ns) = first_delivery.unwrap();
    println!("RAW_XT_FIRST_DELIVERY: before={before_ns} after={after_ns}");
    assert!(before_ns <= 5_000_000 && after_ns >= 5_000_000 && after_ns - before_ns < 100_000);
    assert!(
        reached,
        "first native keyboard update must deliver exactly one wire byte"
    );
    let saved = m
        .snapshot_state_quiesced(marty_core::vhd::DiskCaptureMode::Embed, 0)
        .unwrap();
    let encoded = serde_json::to_vec(&saved.0).unwrap();
    let decoded = serde_json::from_slice(&encoded).unwrap();
    let mut restored = machine_with_keyboard()
        .prepare_snapshot_restore(&decoded, [None, None])
        .unwrap();
    assert_eq!(
        restored
            .snapshot_state_quiesced(marty_core::vhd::DiskCaptureMode::Embed, 0)
            .unwrap(),
        saved
    );
    let mut native = ExecutionControl::new();
    let mut consumed = false;
    for _ in 0..40000 {
        a.step(&mut m, None);
        native.set_op(ExecutionOperation::Step);
        restored.run(1, &mut native);
        if peek(&m, 0x280, 2).unwrap() == [4, 0] {
            consumed = true;
            break;
        }
    }
    assert!(consumed, "actual guest IRQ1 must acknowledge and record all four bytes");
    assert_eq!(peek(&m, 0x300, 4).unwrap(), [0, 0x80, 0x1e, 0x9e]);
    assert_eq!(m.bus_mut().keyboard_mut().unwrap().pending_rpc_scancodes(), 0);
    assert_eq!(
        m.snapshot_state_quiesced(marty_core::vhd::DiskCaptureMode::Embed, 0)
            .unwrap(),
        restored
            .snapshot_state_quiesced(marty_core::vhd::DiskCaptureMode::Embed, 0)
            .unwrap()
    );
    println!("RAW_XT_IRQ1: native5000us boundaries deliver zero/make/break through PPI/PIC; cold serialized Machine at pending IRQ continues to four matching raw bytes/full state");
}

#[test]
fn keyboard_rpc_preflights_entire_request_without_advancing_cpu() {
    let mut m = machine_with_keyboard();
    let mut a = Agent::new(1);
    let event = json!({"scan_code":30,"pressed":true});
    let before = m
        .snapshot_state_quiesced(marty_core::vhd::DiskCaptureMode::Embed, 0)
        .unwrap();
    for bad in [
        json!({"events":[]}),
        json!({"events":vec![event.clone();33]}),
        json!({"events":[event.clone(),{"scan_code":128,"pressed":false}]}),
        json!({"events":[event.clone(),{"scan_code":-1,"pressed":false}]}),
        json!({"events":[event.clone(),{"scan_code":30,"pressed":1}]}),
        json!({"events":[event.clone(),{"scan_code":true,"pressed":true}]}),
        json!({"events":[event.clone(),{"pressed":true}]}),
    ] {
        assert!(a.handle(&mut m, "input.keyboard", &bad).is_err());
        assert_eq!(a.revision, 0);
        assert!(
            m.snapshot_state_quiesced(marty_core::vhd::DiskCaptureMode::Embed, 0)
                .unwrap()
                == before
        );
    }
    let clock = m.cpu().get_cycle_ct();
    let result = call(
        &mut a,
        &mut m,
        "input.keyboard",
        json!({"events":[event.clone(),{"scan_code":30,"pressed":false}]}),
    );
    assert_eq!(result["accepted"], 2);
    assert_eq!(result["state_revision"], 1);
    assert_eq!(m.cpu().get_cycle_ct(), clock);
    assert_eq!(m.bus_mut().keyboard_mut().unwrap().pending_rpc_scancodes(), 2);
    call(
        &mut a,
        &mut m,
        "keyboard.scancode",
        json!({"scan_code":0,"pressed":true}),
    );
    assert_eq!(m.bus_mut().keyboard_mut().unwrap().pending_rpc_scancodes(), 3);
    m.bus_mut()
        .keyboard_mut()
        .unwrap()
        .queue_rpc_scancodes(&vec![0; 4093])
        .unwrap();
    assert!(a
        .handle(&mut m, "input.keyboard", &json!({"events":[event.clone()]}))
        .is_err());
    assert_eq!(m.bus_mut().keyboard_mut().unwrap().pending_rpc_scancodes(), 4096);
    assert_eq!(a.revision, 2);
    a.running = true;
    assert!(a.handle(&mut m, "keyboard.scancode", &event).is_err());
    assert_eq!(a.revision, 2);
    let mut absent = machine();
    a.running = false;
    assert!(a.handle(&mut absent, "keyboard.scancode", &event).is_err());
    let mut incompatible = machine_with_keyboard_on(marty_core::machine_types::MachineType::IbmPCJr);
    let before = serde_json::to_value(incompatible.bus_mut().keyboard_mut().unwrap()).unwrap();
    let clock = incompatible.cpu().get_cycle_ct();
    let revision = a.revision;
    assert!(a.handle(&mut incompatible, "keyboard.scancode", &event).is_err());
    assert_eq!(
        serde_json::to_value(incompatible.bus_mut().keyboard_mut().unwrap()).unwrap(),
        before
    );
    assert_eq!(incompatible.cpu().get_cycle_ct(), clock);
    assert_eq!(a.revision, revision);
}
