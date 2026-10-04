//! Internal Intel CPU component, including the BIU, decoded instruction and
//! pending IRQ/DMA/REP/HLT state. The owned BusInterface is a separate component;
//! saving this alone is NOT a machine snapshot or process-restart proof.
//! Host files/listings/analyzers, active tracing, fuzzer RNG and validator builds
//! are explicitly refused rather than silently discarding their pending state.

use super::biu_state::BiuState;
use super::*;

macro_rules! cpu_state {
    ($( $(#[$attr:meta])* $field:ident: $ty:ty ),* $(,)?) => {
        #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        pub(crate) struct Intel808xState {
            version: u32,
            cpu_type: String,
            cpu_subtype: CpuSubType,
            biu: BiuState,
            ax: u16,
            bx: u16,
            cx: u16,
            dx: u16,
            $( $(#[$attr])* $field: $ty,)*
        }

        impl Intel808x {
            fn snapshot_supported(&self) -> Result<(), &'static str> {
                if cfg!(any(feature = "cpu_validator", feature = "cpu_collect_cycle_states")) {
                    return Err("validator/cycle-collector snapshots are unsupported");
                }
                if self.rng.is_some() || self.trace_enabled || self.trace_logger.is_some() || self.trace_mode != TraceMode::None
                    || !self.trace_comment.is_empty() || !self.trace_str_vec.is_empty()
                    || !self.trace_token_vec.is_empty() || !self.analyzer.entries.is_empty()
                    || self.analyzer.need_flush || self.services.listing_filename.is_some()
                    || !self.services.listing.is_empty() {
                    return Err("active RNG/trace/analyzer/listing snapshots are unsupported");
                }
                Ok(())
            }

            pub(crate) fn snapshot_cpu_state(&self) -> Result<Intel808xState, &'static str> {
                self.snapshot_supported()?;
                let state = Intel808xState {
                    version: 1,
                    cpu_type: format!("{:?}", self.cpu_type),
                    cpu_subtype: self.cpu_subtype,
                    biu: self.snapshot_biu_state(),
                    ax: self.a.x(), bx: self.b.x(), cx: self.c.x(), dx: self.d.x(),
                    $($field: self.$field.clone(),)*
                };
                state.validate(self)?;
                Ok(state)
            }

            pub(crate) fn restore_cpu_state(&mut self, saved: &Intel808xState) -> Result<(), &'static str> {
                self.snapshot_supported()?;
                saved.validate(self)?;
                // BIU/queue performs its own complete preflight before mutation.
                // No fallible operation follows it. The machine-level restorer
                // must preflight all other components/dependencies BEFORE this.
                self.restore_biu_state(&saved.biu)?;
                self.a.set_x(saved.ax); self.b.set_x(saved.bx);
                self.c.set_x(saved.cx); self.d.set_x(saved.dx);
                $(self.$field = saved.$field.clone();)*
                Ok(())
            }
        }
    };
}

cpu_state! {
    state: CpuState,
    sp: u16, bp: u16, si: u16, di: u16,
    cs: u16, ds: u16, ss: u16, es: u16,
    flags: u16,
    last_ea: u16,
    mc_pc: u16,
    nx: bool, rni: bool, ea_opr: u16,
    intr: bool, intr_pending: bool, in_int: bool,
    int_count: u64, iret_count: u64, interrupt_inhibit: bool,
    halted: bool, reported_halt: bool, halt_not_hold: bool, wake_timer: u32,
    is_running: bool, is_error: bool,
    in_rep: bool, rep_init: bool, rep_mnemonic: Mnemonic, rep_type: RepType,
    cycle_num: u64, halt_cycles: u64,
    #[serde(with = "crate::snapshot_codec::f64_bits")]
    t_stamp: f64,
    #[serde(with = "crate::snapshot_codec::f64_bits")]
    t_step: f64,
    #[serde(with = "crate::snapshot_codec::f64_bits")]
    t_step_h: f64,
    instr_cycle: u32, device_cycles: u32, int_elapsed: u32, instr_elapsed: u32,
    instruction_count: u64,
    i: Instruction,
    instruction_ip: u16, instruction_reentrant: bool,
    last_cs: u16, last_ip: u16, last_intr: bool, jumped: bool,
    exception: CpuException,
    instruction_address: u32,
    instruction_history_on: bool,
    instruction_history: VecDeque<HistoryEntry>,
    call_stack: VecDeque<CallStackEntry>,
    exec_result: ExecutionResult,
    breakpoints: Vec<BreakPointType>,
    stopwatches: Vec<Option<CycleStopWatch>>,
    stopwatch_running: bool,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    step_over_target: Option<CpuAddress>,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    step_over_breakpoint: Option<u32>,
    reset_vector: CpuAddress,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    reset_queue: Option<Vec<u8>>,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    service_interrupt_vector: Option<u8>,
    service_interrupt_enabled: bool,
    trace_instr: u16,
    enable_wait_states: bool, off_rails_detection: bool, opcode0_counter: u32,
    end_addr: usize,
    service_events: VecDeque<ServiceEvent>,
    interrupt_scheduling: bool, interrupt_cycle_period: u32,
    interrupt_cycle_num: u32, interrupt_retrigger: bool,
    clk0: bool,
    dma_state: DmaState,
    dram_refresh_simulation: bool, dram_refresh_cycle_period: u32,
    dram_refresh_cycle_num: u32, dram_refresh_adjust: u32,
    dram_refresh_tc: bool, dram_refresh_retrigger: bool,
    dma_aen: bool, dma_holda: bool, dma_req: bool, dma_ack: bool,
    dma_wait_states: u32, dma_wait: bool,
    trap_enable_delay: u32, trap_disable_delay: u32, trap_suppressed: bool,
    nmi: bool, nmi_triggered: bool, halt_resume_delay: u32,
    int_flags: Vec<u8>, io_flags: Vec<u8>,
}

impl Intel808xState {
    fn validate(&self, target: &Intel808x) -> Result<(), &'static str> {
        if self.version != 1
            || self.cpu_type != format!("{:?}", target.cpu_type)
            || self.cpu_subtype != target.cpu_subtype
        {
            return Err("incompatible CPU component version/type/subtype");
        }
        if self.int_flags.len() != 256
            || self.io_flags.len() != 65536
            || self.instruction_history.len() > CPU_HISTORY_LEN
            || self.call_stack.len() > CPU_CALL_STACK_LEN
        {
            return Err("invalid CPU table/history length");
        }
        if !self.t_stamp.is_finite()
            || !self.t_step.is_finite()
            || !self.t_step_h.is_finite()
            || self.t_step <= 0.0
            || self.t_step_h <= 0.0
        {
            return Err("invalid CPU clock state");
        }
        if self.instruction_address > 0xFFFFF
            || self.end_addr > 0xFFFFF
            || self.i.address > 0xFFFFF
            || self.i.decode_idx >= super::decode::DECODE.len()
            || self.i.size == 0
        {
            return Err("invalid CPU instruction address/decode state");
        }
        if !matches!(self.reset_vector, CpuAddress::Segmented(_, _))
            || self.step_over_target.as_ref().is_some_and(|a| !valid_address(a))
            || self.step_over_breakpoint.is_some_and(|a| a > 0xFFFFF)
            || self.reset_queue.as_ref().is_some_and(|q| q.len() > target.queue.size())
        {
            return Err("invalid CPU reset/step-over state");
        }
        Ok(())
    }
}

fn valid_address(address: &CpuAddress) -> bool {
    !matches!(address, CpuAddress::Flat(value) if *value > 0xFFFFF)
}

#[cfg(all(test, not(any(feature = "cpu_validator", feature = "cpu_collect_cycle_states"))))]
mod tests {
    use super::*;

    #[test]
    fn every_cpu_storage_field_has_snapshot_coverage_or_an_explicit_refusal() {
        let value = serde_json::to_value(cpu(false).snapshot_cpu_state().unwrap()).unwrap();
        let mut covered: std::collections::HashSet<String> = value.as_object().unwrap().keys().cloned().collect();
        covered.extend(value["biu"].as_object().unwrap().keys().cloned());
        covered.extend(["a", "b", "c", "d"].map(str::to_owned)); // explicit union word codec
                                                                 // Bus is the next separate component. These host-only/configuration
                                                                 // facilities are refused when active; they must not disappear silently.
        covered.extend(
            [
                "bus",
                "services",
                "analyzer",
                "rng",
                "trace_enabled",
                "trace_mode",
                "trace_logger",
                "trace_comment",
                "trace_str_vec",
                "trace_token_vec",
                "validator",
                "vregs",
                "cycle_states",
                "validator_state",
                "validator_mode",
                "validator_end",
                "peek_fetch",
                "instr_slice",
            ]
            .map(str::to_owned),
        );
        let source = include_str!("mod.rs")
            .split("pub struct Intel808x {")
            .nth(1)
            .unwrap()
            .split("\n}")
            .next()
            .unwrap();
        let fields = Regex::new(r"(?m)^\s+([a-z][a-z0-9_]*):").unwrap();
        for field in fields.captures_iter(source) {
            assert!(
                covered.contains(&field[1]),
                "new CPU storage field needs state or explicit refusal: {}",
                &field[1]
            );
        }
    }

    fn cpu(word: bool) -> Intel808x {
        let (typ, subtype) = if word {
            (CpuType::Intel8086, CpuSubType::Intel8086)
        } else {
            (CpuType::Intel8088, CpuSubType::Intel8088)
        };
        let mut cpu = Intel808x::new(typ, subtype, None, TraceMode::None, TraceLogger::None);
        let desc = crate::machine_config::MachineDescriptor::default();
        cpu.bus
            .install_devices(
                &desc,
                &crate::machine_config::MachineConfiguration::default(),
                #[cfg(feature = "sound")]
                &crate::sound::SoundOutputConfig::default(),
                None,
                false,
            )
            .unwrap();
        cpu.set_reset_vector(CpuAddress::Segmented(0, 0x100));
        // 100: init registers; REP MOVSB; near CALL; INT30; HLT; loop to100.
        // 11F: INC AX; RET. Software interrupt and NMI handlers INC BX; IRET.
        let code = [
            0xB8, 0x34, 0x12, 0xBB, 0, 0, 0xBC, 0, 0x90, 0xBE, 0, 4, 0xBF, 0, 5, 0xB9, 8, 0, 0xFC, 0xF3, 0xA4, 0xE8, 7,
            0, 0xCD, 0x30, 0xF4, 0xE9, 0xE2, 0xFF, 0x90, 0x40, 0xC3,
        ];
        cpu.bus.copy_from(&code, 0x100, 0, false).unwrap();
        cpu.bus.copy_from(&[0x43, 0xCF], 0x700, 0, false).unwrap();
        cpu.bus.copy_from(&[0, 7, 0, 0], 2 * 4, 0, false).unwrap();
        cpu.bus.copy_from(&[0, 7, 0, 0], 0x30 * 4, 0, false).unwrap();
        cpu.bus.copy_from(&[1, 2, 3, 4, 5, 6, 7, 8], 0x400, 0, false).unwrap();
        cpu.instruction_history_on = true;
        cpu.reset();
        cpu
    }

    fn restore_destroyed(target: &mut Intel808x, expected: &Intel808xState) {
        let encoded = serde_json::to_vec(expected).unwrap();
        let decoded = serde_json::from_slice(&encoded).unwrap();
        // Replace the entire CPU independently, retaining its current owned
        // bus only. reset() leaves some fields untouched and could mask an
        // omitted assignment. This is CPU continuation, not bus/disk restore.
        let mut fresh = cpu(target.cpu_type == CpuType::Intel8086);
        std::mem::swap(&mut fresh.bus, &mut target.bus);
        *target = fresh;
        target.a.set_x(expected.ax ^ 0xFFFF); // even the initial reset sample differs
        assert!(serde_json::to_vec(&target.snapshot_cpu_state().unwrap()).unwrap() != encoded);
        target.restore_cpu_state(&decoded).unwrap();
        // JSON compares exact float bits and CpuAddress variants too; PartialEq
        // on CpuAddress intentionally treats distinct segmented aliases alike.
        assert!(
            serde_json::to_vec(&target.snapshot_cpu_state().unwrap()).unwrap() == encoded,
            "restored CPU component differs from its exact JSON state"
        );
    }

    #[test]
    fn cpu_json_restore_continues_native_rep_call_interrupt_and_halt() {
        for word in [false, true] {
            let mut reference = cpu(word);
            let mut restored = cpu(word);
            let mut saw_rep = false;
            let mut saw_call = false;
            let mut saw_halt = false;
            let mut saw_nmi = false;
            let mut saw_interrupt = false;
            for _ in 0..96 {
                if reference.halted {
                    saw_halt = true;
                    reference.set_nmi(true);
                    restored.set_nmi(true);
                }
                let saved = reference.snapshot_cpu_state().unwrap();
                restore_destroyed(&mut restored, &saved);
                // CLI/HLT intentionally reports CpuHaltedError on its first
                // boundary. Compare that native result too; the next NMI wakes
                // the halted CPU and must preserve reported_halt/is_error.
                let a = reference.step(true);
                let b = restored.step(true);
                assert_eq!(format!("{a:?}"), format!("{b:?}"));
                assert!(
                    serde_json::to_vec(&reference.snapshot_cpu_state().unwrap()).unwrap()
                        == serde_json::to_vec(&restored.snapshot_cpu_state().unwrap()).unwrap(),
                    "CPU state after EU step differs"
                );
                saw_rep |= reference.in_rep;
                saw_call |= !reference.call_stack.is_empty();
                // Capture between EU execution and RNI/interrupt completion.
                let saved = reference.snapshot_cpu_state().unwrap();
                restore_destroyed(&mut restored, &saved);
                let a = reference.step_finish(None).unwrap();
                let b = restored.step_finish(None).unwrap();
                assert_eq!(format!("{a:?}"), format!("{b:?}"));
                saw_nmi |= reference.nmi_triggered;
                saw_interrupt |= reference.int_count > 0;
                reference.set_nmi(false);
                restored.set_nmi(false);
                assert!(
                    serde_json::to_vec(&reference.snapshot_cpu_state().unwrap()).unwrap()
                        == serde_json::to_vec(&restored.snapshot_cpu_state().unwrap()).unwrap(),
                    "CPU state after RNI differs"
                );
                assert_eq!(
                    reference.bus.get_slice_at(0, 65536),
                    restored.bus.get_slice_at(0, 65536)
                );
            }
            assert!(
                saw_rep && saw_call && saw_halt && saw_nmi && saw_interrupt,
                "native probe must cover REP/CALL/HLT/NMI/software INT"
            );
            assert_eq!(reference.bus.get_slice_at(0x500, 8), &[1, 2, 3, 4, 5, 6, 7, 8]);
        }
    }

    #[test]
    fn native_long_prefix_instruction_remains_snapshot_compatible() {
        // MAX_INSTRUCTION_SIZE is not a bound in the native 808x decoder:
        // decode() consumes prefixes until a non-prefix byte. Do not reject
        // valid native state using the later-x86 15-byte instruction limit.
        for word in [false, true] {
            let mut reference = cpu(word);
            let mut restored = cpu(word);
            let mut code = vec![0x26; 16];
            code.push(0x90);
            reference.bus.copy_from(&code, 0x100, 0, false).unwrap();
            restored.bus.copy_from(&code, 0x100, 0, false).unwrap();
            assert_eq!(
                format!("{:?}", reference.step(true)),
                format!("{:?}", restored.step(true))
            );
            assert_eq!(reference.i.size, 17);
            let saved = reference.snapshot_cpu_state().unwrap();
            restore_destroyed(&mut restored, &saved);
            assert_eq!(
                format!("{:?}", reference.step_finish(None)),
                format!("{:?}", restored.step_finish(None))
            );
            assert!(
                serde_json::to_vec(&reference.snapshot_cpu_state().unwrap()).unwrap()
                    == serde_json::to_vec(&restored.snapshot_cpu_state().unwrap()).unwrap()
            );
        }
    }

    #[test]
    fn every_excluded_host_facility_is_refused_when_active() {
        for facility in [
            "rng",
            "trace_enabled",
            "trace_mode",
            "trace_logger",
            "trace_comment",
            "trace_str_vec",
            "trace_token_vec",
            "analyzer_entries",
            "analyzer_flush",
            "listing_filename",
            "listing",
        ] {
            let mut target = cpu(false);
            let saved = target.snapshot_cpu_state().unwrap();
            match facility {
                "rng" => target.rng = Some(rand::SeedableRng::seed_from_u64(0)),
                "trace_enabled" => target.trace_enabled = true,
                "trace_mode" => target.trace_mode = TraceMode::Instruction,
                "trace_logger" => target.trace_logger = TraceLogger::Console,
                "trace_comment" => target.trace_comment.push("snapshot refusal probe"),
                "trace_str_vec" => target.trace_str_vec.push(String::new()),
                "trace_token_vec" => target.trace_token_vec.push(SyntaxTokenStream::new()),
                "analyzer_entries" => target.analyzer.entries.push_back(Default::default()),
                "analyzer_flush" => target.analyzer.need_flush = true,
                "listing_filename" => target.services.listing_filename = Some("not-opened".into()),
                "listing" => target
                    .services
                    .add_instruction(0, 0x100, false, false, Vec::new(), target.i.clone()),
                _ => unreachable!(),
            }
            target.a.set_x(0xBEEF);
            assert!(target.snapshot_cpu_state().is_err(), "{facility}");
            assert!(target.restore_cpu_state(&saved).is_err(), "{facility}");
            assert_eq!(target.a.x(), 0xBEEF, "refusal mutated CPU: {facility}");
        }
    }

    #[test]
    fn cpu_component_refuses_invalid_or_unsupported_state_before_mutation() {
        let mut target = cpu(false);
        let before = target.snapshot_cpu_state().unwrap();
        let encoded = serde_json::to_vec(&before).unwrap();
        for (field, value) in [
            ("version", serde_json::json!(2)),
            ("cpu_type", serde_json::json!("Intel8086")),
            ("cpu_subtype", serde_json::json!("Intel8086")),
            ("int_flags", serde_json::json!([])),
            ("io_flags", serde_json::json!([])),
            ("instruction_address", serde_json::json!(0x100000)),
            ("end_addr", serde_json::json!(0x100000)),
            ("reset_vector", serde_json::json!({"Flat":256})),
            ("t_step", serde_json::json!(0)),
        ] {
            let mut invalid = serde_json::to_value(&before).unwrap();
            invalid[field] = value;
            invalid["ax"] = serde_json::json!(0xDEAD);
            let saved = serde_json::from_value(invalid).unwrap();
            assert!(target.restore_cpu_state(&saved).is_err(), "{field}");
            assert_eq!(
                encoded,
                serde_json::to_vec(&target.snapshot_cpu_state().unwrap()).unwrap()
            );
        }
        for (field, value) in [("decode_idx", 352), ("size", 0), ("address", 0x100000)] {
            let mut invalid = serde_json::to_value(&before).unwrap();
            invalid["i"][field] = serde_json::json!(value);
            invalid["ax"] = serde_json::json!(0xDEAD);
            assert!(target
                .restore_cpu_state(&serde_json::from_value(invalid).unwrap())
                .is_err());
            assert_eq!(
                encoded,
                serde_json::to_vec(&target.snapshot_cpu_state().unwrap()).unwrap()
            );
        }
        let mut invalid = serde_json::to_value(&before).unwrap();
        invalid["biu"]["queue"]["len"] = serde_json::json!(5);
        invalid["ax"] = serde_json::json!(0xDEAD);
        assert!(target
            .restore_cpu_state(&serde_json::from_value(invalid).unwrap())
            .is_err());
        assert_eq!(
            encoded,
            serde_json::to_vec(&target.snapshot_cpu_state().unwrap()).unwrap()
        );
        target.trace_enabled = true;
        assert!(target.snapshot_cpu_state().is_err());
        assert!(target.restore_cpu_state(&before).is_err());
        assert_eq!(target.a.x(), before.ax);
        target.trace_enabled = false;
        target.rng = Some(rand::SeedableRng::seed_from_u64(0));
        assert!(target.snapshot_cpu_state().is_err());
        assert!(target.restore_cpu_state(&before).is_err());
        assert_eq!(target.a.x(), before.ax);
    }

    #[test]
    fn cpu_component_json_requires_options_and_exact_clock_bits() {
        let mut cpu = cpu(false);
        cpu.t_stamp = f64::from_bits(0x3ff0000000000001);
        cpu.t_step = f64::from_bits(0x3E8C2F8BFFFFABCD);
        let saved = cpu.snapshot_cpu_state().unwrap();
        let value = serde_json::to_value(&saved).unwrap();
        let decoded: Intel808xState = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(saved.t_stamp.to_bits(), decoded.t_stamp.to_bits());
        assert_eq!(saved.t_step.to_bits(), decoded.t_step.to_bits());
        for field in value.as_object().unwrap().keys() {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<Intel808xState>(missing).is_err(), "{field}");
        }
        for field in value["i"].as_object().unwrap().keys() {
            let mut missing = value.clone();
            missing["i"].as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<Intel808xState>(missing).is_err(),
                "instruction {field}"
            );
        }
        let mut invalid = value.clone();
        invalid["future_field"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Intel808xState>(invalid).is_err());
        let mut invalid = value;
        invalid["t_stamp"] = serde_json::json!(f64::NAN.to_bits());
        assert!(serde_json::from_value::<Intel808xState>(invalid).is_err());
    }
}

#[cfg(all(test, any(feature = "cpu_validator", feature = "cpu_collect_cycle_states")))]
#[test]
fn collector_build_explicitly_refuses_cpu_snapshot_capture() {
    let target = Intel808x::new(
        CpuType::Intel8088,
        CpuSubType::Intel8088,
        None,
        TraceMode::None,
        TraceLogger::None,
    );
    assert_eq!(
        target.snapshot_cpu_state().unwrap_err(),
        "validator/cycle-collector snapshots are unsupported"
    );
}
