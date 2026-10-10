/*
    MartyPC
    https://github.com/dbalsom/martypc

    Copyright 2022-2026 Daniel Balsom

    Permission is hereby granted, free of charge, to any person obtaining a
    copy of this software and associated documentation files (the “Software”),
    to deal in the Software without restriction, including without limitation
    the rights to use, copy, modify, merge, publish, distribute, sublicense,
    and/or sell copies of the Software, and to permit persons to whom the
    Software is furnished to do so, subject to the following conditions:

    The above copyright notice and this permission notice shall be included in
    all copies or substantial portions of the Software.

    THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
    IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
    FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
    AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
    LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
    FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
    DEALINGS IN THE SOFTWARE.

    ---------------------------------------------------------------------------

    cpu_vx0::cpu.rs

    Implements the Cpu trait for the Intel 808x CPUs

*/

#[cfg(feature = "cpu_validator")]
use crate::cpu_validator::CpuValidator;
#[cfg(feature = "cpu_validator")]
use crate::cpu_vx0::CpuValidatorState;
use crate::{
    breakpoints::{BreakPointType, StopWatchData},
    bus::BusInterface,
    cpu_common::{
        Cpu,
        CpuAddress,
        CpuError,
        CpuOption,
        CpuStringState,
        CpuType,
        Disassembly,
        LogicAnalyzer,
        QueueOp,
        Register8,
        ServiceEvent,
        StepResult,
        TraceMode,
    },
    cpu_vx0::{
        trace_print,
        BusStatus,
        CpuState,
        FetchState,
        NecVx0,
        Register16,
        TCycle,
        TaCycle,
        CPU_FLAGS_RESERVED_ON,
    },
};
use marty_common::syntax_token::SyntaxTokenStream;

#[cfg(any(feature = "cpu_validator", feature = "cpu_collect_cycle_states"))]
use crate::cpu_validator::{CycleState, VRegisters};
use crate::cpu_vx0::CPU_FLAG_MODE;

impl Cpu for NecVx0 {
    fn reset(&mut self) {
        log::debug!("CPU Resetting...");
        /*
        let trace_logger = std::mem::replace(&mut self.trace_logger, TraceLogger::None);

        // Save non-default values
        *self = Self {
            // Save parameters to new()
            cpu_type: self.cpu_type,
            reset_vector: self.reset_vector,
            trace_mode: self.trace_mode,
            trace_logger,
            // Save options
            instruction_history_on: self.instruction_history_on,
            dram_refresh_simulation: self.dram_refresh_simulation,
            halt_resume_delay: self.halt_resume_delay,
            off_rails_detection: self.off_rails_detection,
            enable_wait_states: self.enable_wait_states,
            trace_enabled: self.trace_enabled,

            // Copy bus
            bus: self.bus,

            #[cfg(feature = "cpu_validator")]
            validator_type: ValidatorType,
            #[cfg(feature = "cpu_validator")]
            validator_trace: TraceLogger,
            ..Self::default()
        };
        */

        self.state = CpuState::Normal;

        self.set_register16(Register16::AX, 0);
        self.set_register16(Register16::BX, 0);
        self.set_register16(Register16::CX, 0);
        self.set_register16(Register16::DX, 0);
        self.set_register16(Register16::SP, 0);
        self.set_register16(Register16::BP, 0);
        self.set_register16(Register16::SI, 0);
        self.set_register16(Register16::DI, 0);
        self.set_register16(Register16::ES, 0);

        self.set_register16(Register16::SS, 0);
        self.set_register16(Register16::DS, 0);

        self.flags = CPU_FLAGS_RESERVED_ON;
        self.mode_flag_write_enabled = false;
        // Reset into native mode.
        // Set the mode flag to native (set)
        self.flags |= CPU_FLAG_MODE;
        // Set the cpu type for native disassembly.
        self.exit_emulation_mode();

        self.queue.flush();

        if let CpuAddress::Segmented(segment, offset) = self.reset_vector {
            self.set_register16(Register16::CS, segment);
            self.set_register16(Register16::PC, offset);
        }
        else {
            panic!("Invalid CpuAddress for reset vector.");
        }

        self.address_latch = 0;
        self.bus_status = BusStatus::Passive;
        self.bus_status_latch = BusStatus::Passive;
        self.t_cycle = TCycle::Ti;
        self.ta_cycle = TaCycle::Td;
        self.pl_status = BusStatus::Passive;
        self.pl_slot = false;

        self.fetch_state = FetchState::Normal;

        self.instruction_count = 0;
        self.int_count = 0;
        self.iret_count = 0;
        self.instr_cycle = 0;
        self.cycle_num = 1;
        self.halt_cycles = 0;
        self.t_stamp = 0.0;
        self.t_step = 0.00000021;
        self.t_step_h = 0.000000105;
        self.ready = true;
        self.in_rep = false;
        self.halted = false;
        self.reported_halt = false;
        self.halt_not_hold = false;
        self.opcode0_counter = 0;
        self.interrupt_inhibit = false;
        self.intr_pending = false;
        self.in_int = false;
        self.is_error = false;
        self.instruction_history.clear();
        self.call_stack.clear();
        self.int_flags = vec![0; 256];

        self.instruction_reentrant = false;
        self.last_ip = 0;
        self.last_cs = 0;
        self.last_intr = false;
        self.jumped = false;

        self.queue_op = QueueOp::Idle;
        self.last_queue_op = QueueOp::Idle;

        self.i8288.ale = false;
        self.i8288.mrdc = false;
        self.i8288.amwc = false;
        self.i8288.mwtc = false;
        self.i8288.iorc = false;
        self.i8288.aiowc = false;
        self.i8288.iowc = false;

        self.dram_refresh_tc = false;
        self.dram_refresh_retrigger = false;

        self.step_over_target = None;
        self.step_over_breakpoint = None;
        self.end_addr = 0xFFFFF;
        self.stopwatch_running = false;

        self.nx = false;
        self.rni = false;

        self.halt_resume_delay = 4;

        trace_print!(self, "Resetting CPU!");

        // Reset takes 6 cycles before first fetch
        self.cycle();
        self.biu_fetch_suspend();
        self.cycles_i(2, &[0x1e4, 0x1e5]);

        // If reset queue contents are provided, set the queue contents instead of flushing.
        if let Some(reset_queue) = self.reset_queue.clone() {
            self.set_queue_contents(&reset_queue);
        }
        else {
            self.biu_queue_flush();
        }
        self.reset_queue = None;

        self.cycles_i(3, &[0x1e6, 0x1e7, 0x1e8]);

        #[cfg(feature = "cpu_validator")]
        {
            self.validator_state = CpuValidatorState::Uninitialized;
        }
        #[cfg(any(feature = "cpu_validator", feature = "cpu_collect_cycle_states"))]
        {
            self.cycle_states.clear();
        }

        trace_print!(self, "Reset CPU! CS: {:04X} IP: {:04X}", self.cs, self.ip());
    }

    #[inline]
    fn set_reset_vector(&mut self, address: CpuAddress) {
        self.set_reset_vector(address);
    }

    #[inline]
    fn set_queue_contents(&mut self, contents: &[u8], on_reset: bool) {
        if on_reset {
            self.set_reset_queue_contents(contents);
        }
        else {
            self.set_queue_contents(contents);
        }
    }

    #[inline]
    fn set_end_address(&mut self, address: CpuAddress) {
        let end_addr;
        match address {
            CpuAddress::Segmented(segment, offset) => {
                end_addr = NecVx0::calc_linear_address(segment, offset);
            }
            CpuAddress::Flat(addr) => {
                end_addr = addr;
            }
            _ => {
                panic!("Invalid CpuAddress for end address.");
            }
        }
        self.set_end_address(end_addr as usize);
    }

    #[inline]
    fn set_nmi(&mut self, state: bool) {
        self.set_nmi(state);
    }

    #[inline]
    fn set_intr(&mut self, state: bool) {
        self.set_intr(state);
    }

    #[inline]
    fn step(&mut self, skip_breakpoint: bool) -> Result<(StepResult, u32), CpuError> {
        self.step(skip_breakpoint)
    }

    #[inline]
    fn step_finish(&mut self, disassembly: Option<&mut Disassembly>) -> Result<StepResult, CpuError> {
        self.step_finish(disassembly)
    }

    #[inline]
    fn in_rep(&self) -> bool {
        self.in_rep
    }

    #[inline]
    fn get_type(&self) -> CpuType {
        self.cpu_type
    }

    fn flush_piq(&mut self) {
        // Rewind PC to the start of the instruction before flushing, so we will re-fetch it
        self.pc = self.pc.wrapping_sub(self.queue.len_p() as u16);
        self.queue.flush();
        // Debugger flush: discard an in-flight fetch from the old address too.
        // Guest branch microcode keeps its own timing; no cycles run here.
        if self.bus_status_latch == BusStatus::CodeFetch {
            self.biu_bus_end();
            self.i8288.ale = false;
            self.bus_status = BusStatus::Passive;
            self.bus_status_latch = BusStatus::Passive;
            self.t_cycle = TCycle::Ti;
            self.bus_wait_states = 0;
            self.wait_states = 0;
            self.ready = self.dma_wait_states == 0;
        }
        if self.pl_status == BusStatus::CodeFetch {
            self.pl_status = BusStatus::Passive;
            self.ta_cycle = TaCycle::Td;
        }
        self.queue_op = QueueOp::Flush;
        self.last_queue_op = QueueOp::Idle;
        self.last_queue_len = 0;
        if !self.halted {
            self.fetch_state = FetchState::Normal;
            self.biu_fetch_start();
        }
    }

    #[inline]
    fn get_ip(&mut self) -> u16 {
        self.ip()
    }

    #[inline]
    fn get_register16(&self, reg: Register16) -> u16 {
        self.get_register16(reg)
    }

    #[inline]
    fn set_register16(&mut self, reg: Register16, value: u16) {
        self.set_register16(reg, value);
    }

    #[inline]
    fn get_register8(&self, reg: Register8) -> u8 {
        self.get_register8(reg)
    }

    #[inline]
    fn set_register8(&mut self, reg: Register8, value: u8) {
        self.set_register8(reg, value);
    }

    #[inline]
    fn get_flags(&self) -> u16 {
        self.get_flags()
    }

    #[inline]
    fn set_flags(&mut self, flags: u16) {
        self.set_flags(flags);
    }

    #[inline]
    fn get_cycle_ct(&self) -> (u64, u64) {
        self.get_cycle_ct()
    }

    #[inline]
    fn get_instruction_ct(&self) -> u64 {
        self.get_instruction_ct()
    }

    /// Return the resolved flat address of CS:CORR(PC)
    #[inline]
    fn flat_ip(&self) -> u32 {
        NecVx0::calc_linear_address(self.cs, self.ip())
    }

    /// Return the resolved flat address of CS:CORR(PC), adjusted for reentrant instructions
    #[inline]
    fn flat_ip_disassembly(&self) -> u32 {
        NecVx0::calc_linear_address(self.cs, self.disassembly_ip())
    }

    #[inline]
    fn flat_sp(&self) -> u32 {
        self.flat_sp()
    }

    #[inline]
    fn dump_instruction_history_string(&self) -> String {
        self.dump_instruction_history_string()
    }

    #[inline]
    fn dump_instruction_history_tokens(&self) -> Vec<SyntaxTokenStream> {
        self.dump_instruction_history_tokens()
    }

    fn dump_call_stack(&self) -> String {
        self.dump_call_stack()
    }

    #[inline]
    fn get_service_event(&mut self) -> Option<ServiceEvent> {
        self.service_events.pop_front()
    }

    #[inline]
    #[cfg(any(feature = "cpu_validator", feature = "cpu_collect_cycle_states"))]
    fn get_cycle_states(&self) -> &Vec<CycleState> {
        self.get_cycle_states_internal()
    }

    fn get_cycle_trace(&self) -> &Vec<String> {
        self.get_cycle_trace()
    }

    fn get_cycle_trace_tokens(&self) -> &[SyntaxTokenStream] {
        self.get_cycle_trace_tokens()
    }

    #[inline]
    fn get_string_state(&self) -> CpuStringState {
        self.get_string_state()
    }

    fn eval_address(&self, expr: &str) -> Option<CpuAddress> {
        self.eval_address(expr)
    }

    #[inline]
    fn clear_breakpoint_flag(&mut self) {
        self.clear_breakpoint_flag();
    }

    #[inline]
    fn set_breakpoints(&mut self, bp_list: Vec<BreakPointType>) {
        self.set_breakpoints(bp_list)
    }

    #[inline]
    fn get_step_over_breakpoint(&self) -> Option<CpuAddress> {
        self.get_step_over_breakpoint()
    }

    #[inline]
    fn set_step_over_breakpoint(&mut self, address: CpuAddress) {
        self.set_step_over_breakpoint(address)
    }

    #[inline]
    fn get_sw_data(&self) -> Vec<StopWatchData> {
        self.get_sw_data()
    }

    #[inline]
    fn set_stopwatch(&mut self, sw_idx: usize, start: u32, stop: u32) {
        self.set_stopwatch(sw_idx, start, stop)
    }

    fn set_option(&mut self, opt: CpuOption) {
        match opt {
            CpuOption::InstructionHistory(state) => {
                log::debug!("Setting InstructionHistory to: {:?}", state);
                self.instruction_history.clear();
                self.instruction_history_on = state;
            }
            CpuOption::ScheduleInterrupt(_state, cycle_target, cycles, retrigger) => {
                log::debug!("Setting InterruptHint to: ({},{})", cycle_target, cycles);
                self.interrupt_scheduling = true;
                self.interrupt_cycle_period = cycle_target;
                self.interrupt_cycle_num = cycles;
                self.interrupt_retrigger = retrigger;
            }
            CpuOption::ScheduleDramRefresh(state, cycle_target, cycles, retrigger) => {
                log::trace!(
                    "Setting SimulateDramRefresh to: {:?} ({},{})",
                    state,
                    cycle_target,
                    cycles
                );
                self.dram_refresh_simulation = state;
                self.dram_refresh_cycle_period = cycle_target;
                self.dram_refresh_cycle_num = cycles;
                self.dram_refresh_retrigger = retrigger;
                self.dram_refresh_tc = false;
            }
            CpuOption::DramRefreshAdjust(adj) => {
                log::debug!("Setting DramRefreshAdjust to: {}", adj);
                self.dram_refresh_adjust = adj;
            }
            CpuOption::HaltResumeDelay(delay) => {
                log::debug!("Setting HaltResumeDelay to: {}", delay);
                self.halt_resume_delay = delay;
            }
            CpuOption::OffRailsDetection(state) => {
                log::debug!("Setting OffRailsDetection to: {:?}", state);
                self.off_rails_detection = state;
            }
            CpuOption::EnableWaitStates(state) => {
                log::debug!("Setting EnableWaitStates to: {:?}", state);
                self.enable_wait_states = state;
            }
            CpuOption::TraceLoggingEnabled(state) => {
                log::debug!("Setting TraceLoggingEnabled to: {:?}", state);
                self.trace_enabled = state;

                // Flush the trace log file on stopping trace so that we can immediately
                // see results otherwise buffered
                if !state {
                    self.trace_flush();
                }
            }
            CpuOption::ServiceInterruptVector(vector) => {
                log::debug!("Setting service interrupt vector to: {:?}", vector);
                self.service_interrupt_vector = vector;
            }
            CpuOption::ServiceInterruptEnabled(state) => {
                log::debug!("Setting service interrupt enabled state to: {}", state);
                self.service_interrupt_enabled = state;
            }
        }
    }

    fn get_option(&self, opt: CpuOption) -> bool {
        match opt {
            CpuOption::InstructionHistory(_) => self.instruction_history_on,
            CpuOption::ScheduleInterrupt(..) => self.interrupt_cycle_period > 0,
            CpuOption::ScheduleDramRefresh(..) => self.dram_refresh_simulation,
            CpuOption::DramRefreshAdjust(..) => true,
            CpuOption::HaltResumeDelay(..) => true,
            CpuOption::OffRailsDetection(_) => self.off_rails_detection,
            CpuOption::EnableWaitStates(_) => self.enable_wait_states,
            CpuOption::TraceLoggingEnabled(_) => self.trace_enabled,
            CpuOption::ServiceInterruptVector(_) => self.service_interrupt_vector.is_some(),
            CpuOption::ServiceInterruptEnabled(_) => self.service_interrupt_enabled,
        }
    }

    fn bus(&self) -> &BusInterface {
        &self.bus
    }

    fn bus_mut(&mut self) -> &mut BusInterface {
        &mut self.bus
    }

    // Logging methods
    fn cycle_table_header(&self) -> Vec<String> {
        self.cycle_table_header()
    }

    fn emit_header(&mut self) {
        self.emit_header();
    }

    fn trace_flush(&mut self) {
        self.trace_flush();
    }

    #[inline]
    #[cfg(any(feature = "cpu_validator", feature = "cpu_collect_cycle_states"))]
    fn get_vregisters(&self) -> VRegisters {
        self.get_vregisters_internal()
    }

    #[cfg(feature = "cpu_validator")]
    fn get_validator(&self) -> &Option<Box<dyn CpuValidator>> {
        self.get_validator()
    }

    #[cfg(feature = "cpu_validator")]
    fn get_validator_mut(&mut self) -> &mut Option<Box<dyn CpuValidator>> {
        self.get_validator_mut()
    }

    fn randomize_seed(&mut self, seed: u64) {
        self.randomize_seed(seed);
    }

    fn randomize_mem(&mut self, weight: bool) {
        self.randomize_mem(weight);
    }

    fn randomize_regs(&mut self, cs: Option<u16>, pc: Option<u16>) {
        self.randomize_regs(cs, pc);
    }

    fn random_grp_instruction(&mut self, opcode: u8, extension_list: &[u8], addr: u32) {
        self.random_grp_instruction(opcode, extension_list, addr)
    }

    fn random_inst_from_opcodes(&mut self, opcode_list: &[u8], prefix: Option<u8>, addr: u32) {
        self.random_inst_from_opcodes(opcode_list, prefix, addr);
    }

    fn patch_instruction(&mut self, opcode: u8) {
        self.patch_instruction(opcode);
    }

    fn logic_analyzer(&mut self) -> Option<&mut LogicAnalyzer> {
        None
    }
    fn bus_and_analyzer_mut(&mut self) -> (&mut BusInterface, Option<&mut LogicAnalyzer>) {
        (&mut self.bus, None)
    }
}

#[cfg(all(test, not(feature = "cpu_validator")))]
mod debugger_tests {
    use super::*;
    use crate::{cpu_common::CpuArch, cpu_vx0::{QueueReader, QueueType}, tracelogger::TraceLogger};

    #[test]
    fn debugger_redirect_discards_nec_fetch_from_every_native_bus_phase() {
        let mut phases = std::collections::HashSet::new();
        let mut address_phases = std::collections::HashSet::new();
        let mut saw_preload = false;
        let mut saw_pending_address = false;
        for elapsed in 0..80 {
            let mut cpu = NecVx0::new(CpuType::NecV20(CpuArch::I86), TraceMode::None, TraceLogger::None);
            cpu.bus.copy_from(&[0x90; 16], 0xFFFF0, 0, false).unwrap();
            cpu.bus.copy_from(&[0x90; 256], 0, 0, false).unwrap();
            cpu.bus.copy_from(&[0xB8, 0x56, 0x34, 0x90], 0x201, 0, false).unwrap();
            for n in 0..elapsed {
                if n % 3 == 0 && cpu.queue.len() > 0 {
                    cpu.queue.pop();
                }
                cpu.cycle();
            }
            if elapsed % 2 == 0 && cpu.queue.len() > 0 {
                cpu.queue.set_preload();
            }
            phases.insert(format!("{:?}", cpu.t_cycle));
            address_phases.insert(format!("{:?}", cpu.ta_cycle));
            saw_preload |= cpu.queue.has_preload();
            saw_pending_address |= cpu.pl_status == BusStatus::CodeFetch;
            let clock = cpu.cycle_num;
            let continuation = (cpu.nx, cpu.rni, cpu.in_rep, cpu.rep_init,
                std::mem::discriminant(&cpu.rep_type), cpu.instruction_reentrant);
            cpu.flush_piq();
            assert!(!cpu.queue.has_preload());
            assert_eq!(continuation, (cpu.nx, cpu.rni, cpu.in_rep, cpu.rep_init,
                std::mem::discriminant(&cpu.rep_type), cpu.instruction_reentrant));
            cpu.set_register16(Register16::CS, 0);
            cpu.set_register16(Register16::PC, 0x201);
            assert_eq!(cpu.cycle_num, clock);
            for expected in [0xB8, 0x56, 0x34] {
                assert_eq!(cpu.biu_queue_read(QueueType::First, QueueReader::Biu), expected,
                    "stale NEC fetch after {elapsed} cycles");
            }
        }
        for phase in ["T1", "T2", "T3", "T4"] {
            assert!(phases.contains(phase), "untested NEC bus phase {phase}");
        }
        for phase in ["Ts", "T0", "Td"] {
            assert!(address_phases.contains(phase), "untested NEC address phase {phase}");
        }
        assert!(saw_preload && saw_pending_address);
    }

    #[test]
    fn debugger_flush_cancels_nec_fetch_waits_but_keeps_refresh_waits() {
        for waiting in [false, true] {
            let mut cpu = NecVx0::new(CpuType::NecV20(CpuArch::I86), TraceMode::None, TraceLogger::None);
            cpu.bus.copy_from(&[0x90; 16], 0xFFFF0, 0, false).unwrap();
            cpu.bus.copy_from(&[0xB8, 0x56, 0x34], 0x201, 0, false).unwrap();
            cpu.enable_wait_states = true;
            while cpu.t_cycle != TCycle::T3 || cpu.bus_status_latch != BusStatus::CodeFetch {
                cpu.cycle();
            }
            // Controlled slow-memory response on an actual native fetch.
            cpu.bus_wait_states = 4;
            cpu.wait_states = 4;
            cpu.ready = false;
            if waiting {
                cpu.cycle();
                assert_eq!(cpu.t_cycle, TCycle::Tw);
            }
            cpu.dma_wait_states = 7;
            let clock = cpu.cycle_num;
            cpu.flush_piq();
            assert_eq!(cpu.wait_states, 0, "cancelled fetch must not delay the target");
            assert_eq!(cpu.bus_wait_states, 0);
            assert_eq!(cpu.dma_wait_states, 7, "refresh waits are independent of the fetch");
            assert!(!cpu.ready, "pending refresh must keep READY low");
            assert_eq!(cpu.cycle_num, clock);
            cpu.set_register16(Register16::CS, 0);
            cpu.set_register16(Register16::PC, 0x201);
            for expected in [0xB8, 0x56, 0x34] {
                assert_eq!(cpu.biu_queue_read(QueueType::First, QueueReader::Biu), expected);
            }
        }
    }
}
