use super::*;
use crate::tests::{call, machine};

fn finish(a: &mut Agent, m: &mut Machine) {
    for _ in 0..1000 {
        a.advance(m);
        if !a.running {
            return;
        }
    }
    panic!("probe did not stop");
}
#[test]
fn native_write_stop_reports_actual_byte_transition_and_owner() {
    let mut m = machine();
    let mut a = Agent::new(2301);
    let bp = call(
        &mut a,
        &mut m,
        "breakpoints.create",
        json!({"kind":"memory_write","address":{"space":"linear","offset":0x200},"once":true}),
    );
    call(&mut a, &mut m, "execution.continue", json!({}));
    finish(&mut a, &mut m);
    assert_eq!(a.last_stop["breakpoint_id"], bp["breakpoint_id"]);
    let access = &a.last_stop["access"];
    assert_eq!(access["address"], json!({"space":"linear","offset":0x200}));
    assert_eq!(access["old_value"], 0);
    assert_eq!(access["new_value"], 0x35);
    assert_eq!(access["byte_count"], 1);
    assert_eq!(access["instruction_address"]["offset"], 0x104);
    assert_eq!(a.last_stop["registers"]["ip"], 0x107);
    assert_eq!(peek(&m, 0x200, 2).unwrap(), vec![0x35, 0x12]);
    assert!(a.breakpoints.is_empty());
    assert!(!m.bus().debug_observing());
}
#[test]
fn read_and_access_watchpoints_observe_data_not_prefetch_or_host_peeks() {
    for kind in ["memory_read", "memory_access"] {
        let mut m = machine();
        let mut a = Agent::new(2301);
        m.load_program(&[0xa1, 0, 2, 0xeb, 0xfe], 0, 0x100, 0, 0x100).unwrap();
        m.bus_mut().write_u8(0x200, 0x72, 0).unwrap();
        call(
            &mut a,
            &mut m,
            "execution.run_until",
            json!({"predicate":{"kind":kind,"address":{"space":"linear","offset":0x200}}}),
        );
        call(&mut a, &mut m, "memory.read", json!({"address":0x200}));
        assert_eq!(a.predicate.as_ref().unwrap().hits, 0);
        finish(&mut a, &mut m);
        assert_eq!(a.last_stop["kind"], "run_until");
        assert_eq!(a.last_stop["access"]["kind"], "memory_read");
        assert_eq!(a.last_stop["access"]["old_value"], 0x72);
        assert_eq!(a.last_stop["access"]["new_value"], 0x72);
    }
    let mut m = machine();
    let mut a = Agent::new(2301);
    call(
        &mut a,
        &mut m,
        "execution.run_until",
        json!({"predicate":{"kind":"memory_read","address":{"space":"linear","offset":0x100}},"max_emulated_ns":10000}),
    );
    finish(&mut a, &mut m);
    assert_eq!(a.last_stop["kind"], "emulated_time_limit");
}
#[test]
fn software_interrupt_stops_after_native_dispatch_before_first_handler_opcode() {
    let mut m = machine();
    let mut a = Agent::new(2301);
    m.load_program(&[0xb8, 0x11, 0x4c, 0xcd, 0x21, 0xeb, 0xfe], 0, 0x100, 0, 0x100)
        .unwrap();
    for (addr, value) in [
        (0x84, 0x80),
        (0x85, 1),
        (0x86, 0),
        (0x87, 0),
        (0x180, 0x40),
        (0x181, 0xcf),
    ] {
        m.bus_mut().write_u8(addr, value, 0).unwrap();
    }
    call(
        &mut a,
        &mut m,
        "execution.run_until",
        json!({"predicate":{"kind":"interrupt","event":{"type":"software_interrupt","number":0x21,"ah":0x4c,"al":0x11}}}),
    );
    finish(&mut a, &mut m);
    assert_eq!(a.last_stop["event"]["number"], 0x21);
    assert_eq!(a.last_stop["event"]["phase"], "after_dispatch_before_handler");
    assert_eq!(a.last_stop["registers"]["ip"], 0x180);
    assert_eq!(a.last_stop["registers"]["general"]["ax"], 0x4c11); // Handler INC AX has not run.
}
#[test]
fn async_step_returns_entry_then_wait_returns_actual_completed_state() {
    let mut m = machine();
    let mut a = Agent::new(2301);
    let before = registers(&mut m, 0);
    let accepted = a.handle(&mut m, "execution.step", &json!({})).unwrap();
    assert_eq!(accepted["ip"], before["ip"]);
    assert_eq!(accepted["clock"], before["clock"]);
    assert_eq!(accepted["stepping"], true);
    assert!(a.running);
    assert_eq!(
        call(
            &mut a,
            &mut m,
            "execution.wait",
            json!({"operation_id":accepted["operation_id"]})
        )["running"],
        true
    );
    a.advance(&mut m);
    let done = call(
        &mut a,
        &mut m,
        "execution.wait",
        json!({"operation_id":accepted["operation_id"]}),
    );
    assert_eq!(done["stop_reason"]["kind"], "step");
    assert_eq!(done["ip"], 0x103);
    assert_eq!(done["general"]["ax"], 0x1234);
    assert!(done["clock"].as_u64().unwrap() > before["clock"].as_u64().unwrap());
}
#[test]
fn cpu_trace_has_ordered_data_effects_and_does_not_change_native_continuation() {
    let mut native = machine();
    let mut observed = machine();
    let mut a = Agent::new(2301);
    call(
        &mut a,
        &mut observed,
        "trace.start",
        json!({"instruction_count":40,"detail":"normal"}),
    );
    call(&mut a, &mut observed, "hardware.trace.start", json!({"capacity":16}));
    let mut control = ExecutionControl::new();
    for _ in 0..40 {
        call(&mut a, &mut observed, "execution.step", json!({}));
        control.set_op(ExecutionOperation::Step);
        native.run(1, &mut control);
        assert_eq!(registers(&mut native, 0), registers(&mut observed, 0));
        assert_eq!(
            format!("{:?}", native.cpu().get_string_state()),
            format!("{:?}", observed.cpu().get_string_state())
        );
        assert_eq!(native.system_ticks(), observed.system_ticks());
        assert_eq!(peek(&native, 0, 65536).unwrap(), peek(&observed, 0, 65536).unwrap());
        assert_eq!(
            native.bus().pit().as_ref().unwrap().get_cycles(),
            observed.bus().pit().as_ref().unwrap().get_cycles()
        );
    }
    let data = call(&mut a, &mut observed, "trace.read", json!({"limit":40}));
    assert_eq!(data["event_count"], 40);
    assert_eq!(data["active"], false);
    let effects = data["events"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|e| e["effects"].as_array().unwrap());
    let writes = effects.filter(|e| e["kind"] == "memory_write").collect::<Vec<_>>();
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0]["after_base64"], "NQ==");
    assert_eq!(writes[1]["after_base64"], "Eg==");
    assert_eq!(writes[0]["before_base64"], "AA==");
    assert_eq!(writes[1]["before_base64"], "AA==");
    assert_eq!(writes[0]["instruction_address"]["offset"], 0x104);
    assert!(data["events"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|e| e["effects"].as_array().unwrap())
        .all(|e| matches!(
            e["kind"].as_str(),
            Some("memory_write" | "memory_read" | "io_write" | "io_read")
        )));
    assert!(a
        .handle(&mut observed, "trace.read", &json!({"cursor":"trace-41"}))
        .is_err());
}
#[test]
fn native_io_filters_overflow_and_expired_cursor_are_explicit() {
    let mut m = machine();
    let mut a = Agent::new(2301);
    m.load_program(&[0xb0, 0x36, 0xe6, 0x43, 0xe4, 0x40, 0xeb, 0xfa], 0, 0x100, 0, 0x100)
        .unwrap();
    call(
        &mut a,
        &mut m,
        "hardware.trace.start",
        json!({"capacity":2,"ports":[{"first":0x40,"last":0x43}],"include_irq":false}),
    );
    for _ in 0..15 {
        call(&mut a, &mut m, "execution.step", json!({}));
    }
    let data = call(&mut a, &mut m, "hardware.trace.read", json!({}));
    assert_eq!(data["events"].as_array().unwrap().len(), 2);
    assert!(data["dropped_event_count"].as_u64().unwrap() > 0);
    assert!(data["events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["port"] == 0x40 || e["port"] == 0x43));
    assert!(a
        .handle(&mut m, "hardware.trace.read", &json!({"cursor":"hardware-0"}))
        .is_err());
    call(&mut a, &mut m, "hardware.trace.stop", json!({}));
    let frozen = a.hardware.next;
    call(&mut a, &mut m, "execution.step", json!({}));
    assert_eq!(a.hardware.next, frozen);
}

#[test]
fn resume_data_watchpoint_observes_immediately_following_matching_access() {
    let mut m = machine();
    let mut a = Agent::new(2301);
    m.load_program(&[0xa2, 0, 2, 0xa2, 0, 2, 0xeb, 0xfe], 0, 0x100, 0, 0x100)
        .unwrap();
    let bp = call(
        &mut a,
        &mut m,
        "breakpoints.create",
        json!({"kind":"memory_write","address":0x200}),
    );
    for (hit, owner) in [(1, 0x100), (2, 0x103)] {
        call(&mut a, &mut m, "execution.continue", json!({}));
        finish(&mut a, &mut m);
        assert_eq!(a.last_stop["breakpoint_id"], bp["breakpoint_id"]);
        assert_eq!(a.last_stop["hit_count"], hit);
        assert_eq!(a.last_stop["access"]["instruction_address"]["offset"], owner);
    }
}

#[test]
fn resumed_native_interrupt_stop_observes_nested_first_handler_interrupt() {
    let mut m = machine();
    let mut a = Agent::new(2301);
    m.load_program(&[0xcd, 0x21], 0, 0x100, 0, 0x100).unwrap();
    for (addr, value) in [
        (0x84, 0x80),
        (0x85, 1),
        (0x86, 0),
        (0x87, 0),
        (0x180, 0xcd),
        (0x181, 0x21),
    ] {
        m.bus_mut().write_u8(addr, value, 0).unwrap();
    }
    call(
        &mut a,
        &mut m,
        "breakpoints.create",
        json!({"kind":"interrupt","event":{"type":"software_interrupt","number":0x21}}),
    );
    for hit in 1..=2 {
        call(&mut a, &mut m, "execution.continue", json!({}));
        finish(&mut a, &mut m);
        assert_eq!(a.last_stop["hit_count"], hit);
        assert_eq!(a.last_stop["registers"]["ip"], 0x180);
    }
}

#[test]
fn overlapping_execution_and_data_breakpoints_keep_creation_order_after_nine_ids() {
    for kind in ["execution", "memory_write"] {
        let mut m = machine();
        let mut a = Agent::new(2301);
        m.load_program(&[0xa2, 0, 2, 0xeb, 0xfe], 0, 0x100, 0, 0x100).unwrap();
        let target = if kind == "execution" { 0x100 } else { 0x200 };
        for n in 1..=10 {
            call(
                &mut a,
                &mut m,
                "breakpoints.create",
                json!({"kind":kind,"address":if n==2 || n==10 {target} else {0x500+n}}),
            );
        }
        let listed = call(&mut a, &mut m, "breakpoints.list", json!({}));
        assert_eq!(listed["breakpoints"][1]["breakpoint_id"], "bp-2");
        assert_eq!(listed["breakpoints"][9]["breakpoint_id"], "bp-10");
        call(&mut a, &mut m, "execution.continue", json!({}));
        finish(&mut a, &mut m);
        assert_eq!(a.last_stop["breakpoint_id"], "bp-2");
    }
}

#[test]
fn unsupported_secondary_irq_and_unavailable_memory_phase_are_refused() {
    let mut m = machine();
    let mut a = Agent::new(2301);
    assert!(a.handle(&mut m, "hardware.trace.start", &json!({"irqs":[8]})).is_err());
    assert!(!a.hardware.active);
    assert!(a
        .handle(
            &mut m,
            "breakpoints.create",
            &json!({"kind":"memory_write","address":0x200,"phase":"before_access"})
        )
        .is_err());
    assert!(a.breakpoints.is_empty());
    let bp = call(
        &mut a,
        &mut m,
        "breakpoints.create",
        json!({"kind":"memory_write","address":0x200,"phase":"after_native_boundary"}),
    );
    assert_eq!(bp["phase"], "after_native_boundary");
    let caps = call(&mut a, &mut m, "agent.capabilities", json!({}));
    assert_eq!(caps["observation"]["irq_lines"], json!([0, 1, 2, 3, 4, 5, 6, 7]));
    for kind in ["memory_read", "memory_write", "memory_access", "interrupt"] {
        assert!(caps["breakpoint_kinds"].as_array().unwrap().contains(&json!(kind)));
    }
}
#[test]
fn synthetic_overflow_distinguishes_lost_effects_and_stops_incomplete_cpu_trace() {
    // Loss accounting control, not evidence of an executed instruction or guest timing.
    let mut m = machine();
    let mut a = Agent::new(2301);
    call(&mut a, &mut m, "trace.start", json!({"instruction_count":2}));
    a.start(&mut m);
    let before = registers(&mut m, a.revision);
    a.record_boundary(&mut m, Some(before), &[], 4, 2, None);
    assert_eq!(a.last_stop["kind"], "observation_overflow");
    assert_eq!(a.last_stop["dropped_event_count"], 4);
    assert_eq!(a.last_stop["dropped_effect_count"], 2);
    let data = call(&mut a, &mut m, "trace.read", json!({}));
    assert_eq!(data["active"], false);
    assert_eq!(data["events"][0]["dropped_event_count"], 4);
    assert_eq!(data["events"][0]["dropped_effect_count"], 2);
}

#[test]
fn exact_effect_capacity_stops_recording_without_a_later_boundary() {
    // Capacity edge control. The native instruction supplies its actual two-byte store.
    let mut m = machine();
    let mut a = Agent::new(2301);
    call(&mut a, &mut m, "trace.start", json!({"instruction_count":8}));
    a.trace.effect_count = 65534;
    call(&mut a, &mut m, "execution.continue", json!({}));
    while a.trace.active {
        a.advance(&mut m);
    }
    assert_eq!(a.trace.effect_count, 65536);
    assert!(a.running);
    let before = a.trace.events.len();
    a.advance(&mut m);
    assert_eq!(a.trace.events.len(), before);
    let data = call(&mut a, &mut m, "trace.read", json!({}));
    assert_eq!(data["active"], false);
    assert_eq!(data["events"].as_array().unwrap().last().unwrap()["dropped_effect_count"], 0);
}
