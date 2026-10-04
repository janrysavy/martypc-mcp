//! Fetch/electrical component of a future complete CPU snapshot.
//!
//! A queue alone cannot resume a pipelined transfer: address/data latches,
//! T/TA states, READY/waits and the 8288 pins also persist across boundaries.
//! This component deliberately does NOT claim a CPU or machine snapshot.
//! Architectural/EU/interrupt/DMA/clock/debug state and the owned BusInterface
//! must be captured separately before a whole-machine restore can be exposed.

use super::biu::BusWidth;
use super::queue::InstructionQueueState;
use super::*;
use crate::cpu_common::operands::OperandSize;

// One explicit field list keeps export and restore paired. It describes actual
// stored values, rather than reconstructing phase from visible instruction bytes.
macro_rules! biu_state {
    ($($field:ident: $ty:ty),* $(,)?) => {
        #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        pub(crate) struct BiuState {
            version: u32,
            queue: InstructionQueueState,
            $($field: $ty,)*
        }

        impl Intel808x {
            pub(crate) fn snapshot_biu_state(&self) -> BiuState {
                BiuState {
                    version: 1,
                    queue: self.queue.snapshot_state(),
                    $($field: self.$field,)*
                }
            }

            pub(crate) fn restore_biu_state(&mut self, saved: &BiuState) -> Result<(), &'static str> {
                if saved.version != 1 {
                    return Err("unsupported BIU state version");
                }
                if saved.bus_width != self.bus_width || saved.fetch_size != self.fetch_size {
                    return Err("incompatible BIU configuration");
                }
                if saved.address_bus > 0xFFFFF || saved.address_latch > 0xFFFFF
                    || saved.last_queue_len > self.queue.size() || saved.transfer_n > 2 {
                    return Err("invalid BIU address, queue length or transfer number");
                }
                // Queue restore preflights every field before mutation. Once it
                // succeeds the remaining assignments are infallible; a refused
                // component cannot partially change pins, queue or bus phase.
                self.queue.restore_state(&saved.queue)?;
                $(self.$field = saved.$field;)*
                Ok(())
            }
        }
    };
}

biu_state! {
    address_bus: u32,
    address_latch: u32,
    data_bus: u16,
    bhe: bool,
    i8288: I8288,
    pc: u16,
    bus_width: BusWidth,
    ready: bool,
    ready_next: bool,
    fetch_size: TransferSize,
    fetch_state: FetchState,
    bus_pending: BusPendingType,
    queue_op: QueueOp,
    last_queue_op: QueueOp,
    queue_byte: u8,
    last_queue_byte: u8,
    last_queue_len: usize,
    t_cycle: TCycle,
    ta_cycle: TaCycle,
    bus_status: BusStatus,
    bus_status_latch: BusStatus,
    pl_status: BusStatus,
    pl_slot: bool,
    bus_segment: Segment,
    transfer_size: TransferSize,
    operand_size: OperandSize,
    transfer_n: u32,
    final_transfer: bool,
    bus_wait_states: u32,
    io_wait_states: u32,
    lock: bool,
}

#[cfg(all(test, not(feature = "cpu_validator")))]
mod tests {
    use super::*;

    fn cpu(word: bool) -> Intel808x {
        let (typ, subtype) = if word {
            (CpuType::Intel8086, CpuSubType::Intel8086)
        } else {
            (CpuType::Intel8088, CpuSubType::Intel8088)
        };
        let mut cpu = Intel808x::new(typ, subtype, None, TraceMode::None, TraceLogger::None);
        cpu.bus.copy_from(&[0x90; 16], 0xFFFF0, 0, false).unwrap();
        cpu.bus.copy_from(&[0x90; 256], 0, 0, false).unwrap();
        cpu
    }

    // Do not destroy state through the restorer being tested: an omitted
    // assignment would then leave the old value in place on BOTH calls and
    // falsely pass. Mutate fields independently, keeping only configuration.
    fn destroy_component(cpu: &mut Intel808x) {
        cpu.queue.flush();
        cpu.address_bus ^= 0xFFFFF;
        cpu.address_latch ^= 0xFFFFF;
        cpu.data_bus ^= 0xFFFF;
        cpu.bhe = !cpu.bhe;
        cpu.i8288 = I8288::default();
        cpu.pc = cpu.pc.wrapping_add(123);
        cpu.ready = !cpu.ready;
        cpu.ready_next = !cpu.ready_next;
        cpu.fetch_state = FetchState::Suspended;
        cpu.bus_pending = BusPendingType::EuEarly;
        cpu.queue_op = QueueOp::Flush;
        cpu.last_queue_op = QueueOp::Flush;
        cpu.queue_byte ^= 0xFF;
        cpu.last_queue_byte ^= 0xFF;
        cpu.last_queue_len = 0;
        cpu.t_cycle = TCycle::Ti;
        cpu.ta_cycle = TaCycle::Td;
        cpu.bus_status = BusStatus::Passive;
        cpu.bus_status_latch = BusStatus::Passive;
        cpu.pl_status = BusStatus::Passive;
        cpu.pl_slot = !cpu.pl_slot;
        cpu.bus_segment = Segment::None;
        cpu.transfer_size = TransferSize::Byte;
        cpu.operand_size = OperandSize::NoOperand;
        cpu.transfer_n = 0;
        cpu.final_transfer = !cpu.final_transfer;
        cpu.bus_wait_states = 123;
        cpu.io_wait_states = 456;
        cpu.lock = !cpu.lock;
    }

    #[test]
    fn native_prefetch_cycles_survive_json_restore_without_queue_flush() {
        for word in [false, true] {
            let mut original = cpu(word);
            let mut restored = cpu(word);
            let mut seen_t = std::collections::HashSet::new();
            let mut seen_ta = std::collections::HashSet::new();
            let mut saw_latched_fetch = false;
            let mut saw_preload = false;
            for n in 0..1000 {
                // Consume real fetched bytes to keep the native BIU active.
                if n % 3 == 0 && original.queue.len() > 0 && !original.queue.has_preload() {
                    original.queue.set_preload();
                    restored.queue.set_preload();
                }
                if n % 5 == 0 {
                    assert_eq!(original.queue.get_preload(), restored.queue.get_preload());
                }
                if n % 7 == 0 && original.queue.len() > 0 {
                    assert_eq!(original.queue.pop(), restored.queue.pop());
                }
                original.cycle();
                restored.cycle();
                let expected = original.snapshot_biu_state();
                assert_eq!(expected, restored.snapshot_biu_state());
                let saved = restored.snapshot_biu_state();
                let json = serde_json::to_vec(&saved).unwrap();
                let decoded = serde_json::from_slice(&json).unwrap();
                assert_eq!(saved, restored.snapshot_biu_state()); // export is inert
                destroy_component(&mut restored);
                assert_ne!(saved, restored.snapshot_biu_state());
                restored.restore_biu_state(&decoded).unwrap();
                assert_eq!(expected, restored.snapshot_biu_state());
                // Explicit continuation, not merely equality of assigned DTO
                // fields. The reference is never restored or perturbed.
                for _ in 0..2 {
                    original.cycle();
                    restored.cycle();
                    assert_eq!(original.snapshot_biu_state(), restored.snapshot_biu_state());
                    assert_eq!(
                        original.bus.get_slice_at(0xFFFF0, 16),
                        restored.bus.get_slice_at(0xFFFF0, 16)
                    );
                }
                assert_eq!(original.cycle_num, restored.cycle_num);
                assert_eq!(original.instr_elapsed, restored.instr_elapsed);
                seen_t.insert(format!("{:?}", saved.t_cycle));
                seen_ta.insert(format!("{:?}", saved.ta_cycle));
                saw_latched_fetch |= saved.bus_status_latch == BusStatus::CodeFetch;
                saw_preload |= original.queue.has_preload();
            }
            for phase in ["T1", "T2", "T3", "T4"] {
                assert!(seen_t.contains(phase), "missing native bus phase {phase}: {seen_t:?}");
            }
            assert!(seen_ta.len() >= 3, "address pipeline was not exercised: {seen_ta:?}");
            assert!(saw_latched_fetch && saw_preload);
        }
    }

    #[test]
    fn biu_refusals_precede_any_component_mutation() {
        let mut cpu = cpu(false);
        let before = cpu.snapshot_biu_state();
        for (field, value) in [
            ("version", serde_json::json!(2)),
            ("address_bus", serde_json::json!(0x100000)),
            ("address_latch", serde_json::json!(0x100000)),
            ("last_queue_len", serde_json::json!(5)),
            ("transfer_n", serde_json::json!(3)),
            ("bus_width", serde_json::json!("Word")),
            ("fetch_size", serde_json::json!("Word")),
        ] {
            let mut invalid = serde_json::to_value(&before).unwrap();
            invalid[field] = value;
            invalid["pc"] = serde_json::json!(1234); // detect early partial assignments
            let state = serde_json::from_value(invalid).unwrap();
            assert!(cpu.restore_biu_state(&state).is_err(), "{field}");
            assert_eq!(before, cpu.snapshot_biu_state());
        }
        let mut invalid = serde_json::to_value(&before).unwrap();
        invalid["pc"] = serde_json::json!(1234);
        invalid["queue"]["len"] = serde_json::json!(5);
        let state = serde_json::from_value(invalid).unwrap();
        assert!(cpu.restore_biu_state(&state).is_err());
        assert_eq!(before, cpu.snapshot_biu_state());
    }

    #[test]
    fn biu_json_requires_every_field_and_rejects_unknown_pins_or_phases() {
        let valid = serde_json::to_value(cpu(false).snapshot_biu_state()).unwrap();
        for field in valid.as_object().unwrap().keys() {
            let mut missing = valid.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<BiuState>(missing).is_err(), "{field}");
        }
        for field in valid["i8288"].as_object().unwrap().keys() {
            let mut missing = valid.clone();
            missing["i8288"].as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<BiuState>(missing).is_err(), "8288 pin {field}");
        }
        for field in valid["queue"].as_object().unwrap().keys() {
            let mut missing = valid.clone();
            missing["queue"].as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<BiuState>(missing).is_err(),
                "queue field {field}"
            );
        }
        for field in ["t_cycle", "ta_cycle", "bus_status", "fetch_state", "bus_segment"] {
            let mut invalid = valid.clone();
            invalid[field] = serde_json::json!("future_phase");
            assert!(serde_json::from_value::<BiuState>(invalid).is_err(), "{field}");
        }
        let mut future = valid.clone();
        future["future_field"] = serde_json::json!(true);
        assert!(serde_json::from_value::<BiuState>(future).is_err());
        let mut future_pin = valid;
        future_pin["i8288"]["future_pin"] = serde_json::json!(true);
        assert!(serde_json::from_value::<BiuState>(future_pin).is_err());
    }
}
