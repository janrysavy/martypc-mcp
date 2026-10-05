//! Local JSON-lines debugger compatible with the PyPC control contract.
//! All handlers and execution use one machine thread. Read-only inspection
//! never flushes the 8088 prefetch queue or substitutes instruction timings.
use base64::{engine::general_purpose::STANDARD, Engine};
use marty_core::{
    cpu_common::{Cpu, Register16},
    machine::{ExecutionControl, ExecutionOperation, ExecutionState, Machine, MachineState},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub mod snapshot;

const METHODS: &[&str] = &[
    "agent.capabilities",
    "emulator.info",
    "session.status",
    "state.get",
    "state.get_registers",
    "state.set_registers",
    "memory.read",
    "memory.write",
    "input.joystick",
    "input.joystick.state",
    "breakpoints.create",
    "breakpoints.list",
    "breakpoints.delete",
    "execution.pause",
    "execution.continue",
    "execution.go",
    "execution.run_until",
    "execution.wait",
    "execution.step",
];
const MAX_LINE: usize = 1024 * 1024;
type Result<T> = std::result::Result<T, (&'static str, i32)>;

fn invalid<T>(message: &'static str) -> Result<T> {
    Err((message, -32602))
}
fn number(value: &Value) -> Result<u64> {
    if let Some(n) = value.as_u64() {
        return Ok(n);
    }
    if let Some(s) = value.as_str() {
        let s = s.trim();
        let (digits, radix) = if let Some(v) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            (v, 16)
        } else if let Some(v) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
            (v, 2)
        } else if let Some(v) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
            (v, 8)
        } else {
            (s, 10)
        };
        return u64::from_str_radix(digits, radix).map_err(|_| ("unsigned integer required", -32602));
    }
    invalid("unsigned integer required")
}
fn address(v: &Value) -> Result<usize> {
    let n = if let Some(object) = v.as_object() {
        match object.get("space").map(Value::as_str).unwrap_or(Some("physical")) {
            Some("physical" | "linear") => number(&v["offset"])?,
            Some("segmented") => {
                let s = number(&v["segment"])?;
                let o = number(&v["offset"])?;
                if s > 65535 || o > 65535 {
                    return invalid("16-bit segment/offset required");
                }
                (s * 16 + o) & 0xfffff
            }
            _ => return invalid("unsupported address space"),
        }
    } else {
        number(v)?
    };
    if n >= 0x100000 {
        return invalid("address outside 1MiB");
    }
    Ok(n as usize)
}
fn digest(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn reg(name: &str) -> Option<Register16> {
    Some(match name {
        "ax" => Register16::AX,
        "bx" => Register16::BX,
        "cx" => Register16::CX,
        "dx" => Register16::DX,
        "sp" => Register16::SP,
        "bp" => Register16::BP,
        "si" => Register16::SI,
        "di" => Register16::DI,
        "cs" => Register16::CS,
        "ds" => Register16::DS,
        "es" => Register16::ES,
        "ss" => Register16::SS,
        _ => return None,
    })
}
fn register_value(machine: &mut Machine, name: &str) -> Result<u16> {
    match name {
        "ip" => Ok(machine.cpu_mut().get_ip()),
        "flags" => Ok(machine.cpu().get_flags()),
        _ => reg(name)
            .map(|r| machine.cpu().get_register16(r))
            .ok_or(("unknown register", -32602)),
    }
}
fn registers(machine: &mut Machine, revision: u64) -> Value {
    let mut general = json!({});
    let mut segments = json!({});
    for n in ["ax", "bx", "cx", "dx", "sp", "bp", "si", "di"] {
        general[n] = json!(register_value(machine, n).unwrap());
    }
    for n in ["cs", "ds", "es", "ss"] {
        segments[n] = json!(register_value(machine, n).unwrap());
    }
    let flags = machine.cpu().get_flags();
    let flags_text: String = [
        (0x800, 'o'),
        (0x200, 'I'),
        (0x100, 'T'),
        (0x80, 's'),
        (0x40, 'z'),
        (0x10, 'a'),
        (4, 'p'),
        (1, 'c'),
    ]
    .iter()
    .map(|(mask, ch)| if flags & mask != 0 { *ch } else { '-' })
    .collect();
    json!({"general":general,"segments":segments,"ip":machine.cpu_mut().get_ip(),"flags":flags,
        "flags_text":flags_text,"clock":machine.cpu().get_cycle_ct().0,"in_hlt":machine.cpu().get_string_state().halted,
        "state_revision":revision,"emulated_time_ns":emulated_ns(machine,machine.system_ticks())})
}
fn emulated_ns(machine: &Machine, ticks: u64) -> u64 {
    (ticks as f64 * 1000.0 / machine.system_clock_mhz()).floor() as u64
}
fn peek(machine: &Machine, start: usize, length: usize) -> Result<Vec<u8>> {
    if !(1..=65536).contains(&length) || start.checked_add(length).is_none_or(|end| end > 0x100000) {
        return invalid("memory range outside limits");
    }
    (start..start + length)
        .map(|a| machine.bus().peek_u8(a).map_err(|_| ("bus memory read failed", -32603)))
        .collect()
}

fn joystick_state(machine: &Machine, revision: u64) -> Value {
    let Some(port) = machine.bus().game_port().as_ref() else {
        return json!({"joysticks":[], "state_revision":revision});
    };
    let state = port.get_state();
    let count = port.get_controller_count();
    let button_count = if count == 1 { 4 } else { 2 };
    let sticks: Vec<_> = (0..count)
        .map(|i| {
            json!({
                "joystick":i, "x":state.sticks[i].0, "y":state.sticks[i].1,
                "buttons":state.buttons[i * button_count..(i + 1) * button_count]
            })
        })
        .collect();
    json!({"joysticks":sticks, "state_revision":revision})
}

#[derive(Clone)]
struct Breakpoint {
    id: String,
    address: usize,
    address_value: Value,
    segment_offset: Option<(u16, u16)>,
    length: u64,
    once: bool,
    condition: Option<(String, String, u16)>,
    hits: u64,
    skip: u64,
    every: u64,
}
impl Breakpoint {
    fn parse(id: String, p: &Value) -> Result<Self> {
        if p.get("kind").is_some_and(|v| v != "execution") {
            return invalid("only execution breakpoints supported");
        }
        let condition = if p.get("condition").is_some_and(|v| !v.is_null()) {
            let c = &p["condition"];
            let name = c["register"].as_str().ok_or(("condition register required", -32602))?;
            if reg(name).is_none() && name != "ip" && name != "flags" {
                return invalid("unknown condition register");
            }
            let op = c["operator"].as_str().ok_or(("condition operator required", -32602))?;
            if !["eq", "ne", "lt", "le", "gt", "ge"].contains(&op) {
                return invalid("unsupported condition operator");
            }
            let value = number(&c["value"])?;
            if value > 65535 {
                return invalid("condition value outside Word");
            }
            Some((name.to_owned(), op.to_owned(), value as u16))
        } else {
            None
        };
        let filter = p.get("hit_filter");
        if filter.is_some_and(|v| !v.is_object()) {
            return invalid("hit_filter must be an object");
        }
        let skip = filter.and_then(|v| v.get("skip")).map(number).transpose()?.unwrap_or(0);
        let every = filter
            .and_then(|v| v.get("every"))
            .map(number)
            .transpose()?
            .unwrap_or(1);
        if every == 0 {
            return invalid("hit-filter every must be positive");
        }
        let once = p
            .get("once")
            .map(|v| v.as_bool().ok_or(("once must be boolean", -32602)))
            .transpose()?
            .unwrap_or(false);
        let physical = address(&p["address"])?;
        let segment_offset = if p["address"]["space"] == "segmented" {
            Some((
                number(&p["address"]["segment"])? as u16,
                number(&p["address"]["offset"])? as u16,
            ))
        } else {
            None
        };
        let space = if p["address"]["space"] == "linear" {
            "linear"
        } else {
            "physical"
        };
        let address_value = match segment_offset {
            Some((segment, offset)) => json!({"space":"segmented","segment":segment,"offset":offset}),
            None => json!({"space":space,"offset":physical}),
        };
        let length = p.get("length").map(number).transpose()?.unwrap_or(1);
        if !(1..=65536).contains(&length) || physical as u64 + length > 0x100000 {
            return invalid("breakpoint range outside limits");
        }
        Ok(Self {
            id,
            address: physical,
            address_value,
            segment_offset,
            length,
            once,
            condition,
            hits: 0,
            skip,
            every,
        })
    }
    fn value(&self) -> Value {
        json!({"breakpoint_id":self.id,"kind":"execution",
        "address":self.address_value,"length":self.length,"once":self.once,"hit_count":self.hits,
        "condition":self.condition.as_ref().map(|(name,op,value)|json!({"register":name,"operator":op,"value":value})),
        "hit_filter":{"skip":self.skip,"every":self.every}})
    }
    fn matches(&mut self, machine: &mut Machine) -> bool {
        let flat = machine.cpu().flat_ip_disassembly();
        if let Some((segment, offset)) = self.segment_offset {
            let cs = machine.cpu().get_register16(Register16::CS);
            let ip = flat.wrapping_sub((cs as u32) << 4) as u16;
            if cs != segment || ip != offset {
                return false;
            }
        } else if (flat & 0xfffff) as usize != self.address {
            return false;
        }
        if let Some((name, op, value)) = &self.condition {
            let actual = register_value(machine, name).unwrap();
            if !match op.as_str() {
                "eq" => actual == *value,
                "ne" => actual != *value,
                "lt" => actual < *value,
                "le" => actual <= *value,
                "gt" => actual > *value,
                _ => actual >= *value,
            } {
                return false;
            }
        }
        self.hits += 1;
        self.hits > self.skip && (self.hits - self.skip - 1) % self.every == 0
    }
}

struct Agent {
    revision: u64,
    running: bool,
    control: ExecutionControl,
    next: u64,
    breakpoints: BTreeMap<String, Breakpoint>,
    predicate: Option<Breakpoint>,
    skip_once: Option<String>,
    operation: Option<String>,
    completed: BTreeMap<String, Value>,
    completed_order: VecDeque<String>,
    last_stop: Value,
    deadline: Option<(u64, u64, u64)>,
    port: u16,
}
impl Agent {
    fn new(port: u16) -> Self {
        Self {
            revision: 0,
            running: false,
            control: ExecutionControl::new(),
            next: 0,
            breakpoints: BTreeMap::new(),
            predicate: None,
            skip_once: None,
            operation: None,
            completed: BTreeMap::new(),
            completed_order: VecDeque::new(),
            last_stop: Value::Null,
            deadline: None,
            port,
        }
    }
    fn id(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}-{}", self.next)
    }
    fn paused(&self) -> Result<()> {
        if self.running {
            invalid("emulator must be paused")
        } else {
            Ok(())
        }
    }
    fn stop(&mut self, machine: &mut Machine, mut reason: Value) {
        reason["registers"] = registers(machine, self.revision);
        self.running = false;
        self.control.set_state(ExecutionState::Paused);
        self.predicate = None;
        self.deadline = None;
        self.last_stop = reason.clone();
        if let Some(id) = self.operation.take() {
            let mut result = registers(machine, self.revision);
            result["state"] = json!("stopped");
            result["stop_reason"] = reason;
            self.completed_order.push_back(id.clone());
            self.completed.insert(id, result);
            while self.completed.len() > 64 {
                let first = self.completed_order.pop_front().unwrap();
                self.completed.remove(&first);
            }
        }
    }
    fn start(&mut self, machine: &mut Machine) -> Value {
        self.skip_once = self.last_stop["breakpoint_id"].as_str().map(str::to_owned);
        let id = self.id("op");
        self.operation = Some(id.clone());
        self.running = true;
        self.last_stop = Value::Null;
        {
            let mut result = registers(machine, self.revision);
            result["operation_id"] = json!(id);
            result["state"] = json!("running");
            result["paused"] = json!(false);
            result
        }
    }
    fn step(&mut self, machine: &mut Machine) {
        self.control.set_state(ExecutionState::Paused);
        self.control.set_op(ExecutionOperation::Step);
        machine.run(1, &mut self.control);
        self.revision += 1;
    }
    fn advance(&mut self, machine: &mut Machine) {
        if !self.running {
            return;
        }
        let skip = self.skip_once.take();
        let mut hit = None;
        for (id, bp) in &mut self.breakpoints {
            if skip.as_ref() != Some(id) && bp.matches(machine) {
                hit = Some(bp.clone());
                break;
            }
        }
        if let Some(bp) = hit {
            if bp.once {
                self.breakpoints.remove(&bp.id);
            }
            self.stop(
                machine,
                json!({"kind":"breakpoint","breakpoint_id":bp.id,"address":bp.value()["address"],"hit_count":bp.hits}),
            );
            return;
        }
        if let Some(bp) = &mut self.predicate {
            if bp.matches(machine) {
                let reason = json!({"kind":"run_until","predicate_id":bp.id,
                "breakpoint_id":bp.id,"address":bp.value()["address"],"hit_count":bp.hits});
                self.stop(machine, reason);
                return;
            }
        }
        self.step(machine);
        if matches!(self.control.state, ExecutionState::Halted) {
            self.stop(machine, json!({"kind":"cpu_halt"}));
            return;
        }
        if let Some((start, requested, limit)) = self.deadline {
            let ticks = machine.system_ticks();
            if ticks >= limit {
                let actual = emulated_ns(machine, ticks);
                let deadline = emulated_ns(machine, limit);
                self.stop(machine,json!({"kind":"emulated_time_limit","emulated_time_ns":actual,
                    "emulated_time_limit":{"requested_duration_ns":requested,"start_emulated_time_ns":emulated_ns(machine,start),
                        "deadline_emulated_time_ns":deadline,"actual_stop_emulated_time_ns":actual,"reached":true,
                        "overshoot_ns":actual.saturating_sub(deadline)}}));
            }
        }
    }
    fn handle(&mut self, machine: &mut Machine, method: &str, p: &Value) -> Result<Value> {
        match method {
            "agent.capabilities" | "emulator.info" => {
                let cpu = format!("{:?}", machine.cpu().get_type());
                Ok(
                    json!({"emulator":"MartyPC","protocol":"JSON-RPC 2.0 over localhost JSON-lines",
                "endpoint":format!("127.0.0.1:{}",self.port),"methods":METHODS,
                "cpu":cpu.trim_start_matches("Intel"),"memory_bytes":0x100000,
                "address_spaces":["physical","linear","segmented"],
                "limits":{"max_memory_bytes":65536,"memory_write":"installed writable RAM only","completed_operations":64},
                "clock":{"unit":"cpu_cycle","frequency_hz":machine.get_cpu_mhz()*1_000_000.0},
                "execution_step_unit":"native machine boundary (including device/interrupt work)",
                "breakpoint_kinds":["execution"],"step_modes":["into"],
                "time_base":"system crystal ticks (independent of turbo)",
                "unsupported":["trace","hardware.trace","video","vnc","serial","input.keyboard","io","machine.snapshot",
                    "memory_read_breakpoints","memory_write_breakpoints","memory_access_breakpoints",
                    "interrupt_breakpoints","step_over","frontend_file_transfer",
                    "frontend_speed_control","frontend_cursor_control","ppi_software_turbo"]}),
                )
            }
            "session.status" => {
                if p.get("session_id").is_some_and(|v| v != "martypc") {
                    return invalid("unknown session id");
                }
                let cpu = format!("{:?}", machine.cpu().get_type());
                let video = machine.primary_videocard().map(|v| format!("{:?}", v.video_type()));
                Ok(
                    json!({"session_id":"martypc","state":if self.running {"running"} else {"stopped"},
                    "state_revision":self.revision,"clock":machine.cpu().get_cycle_ct().0,"last_stop":self.last_stop,
                    "target":{"cpu":cpu.trim_start_matches("Intel"),"cpu_model":cpu,"video":video,"memory_bytes":0x100000}}),
                )
            }
            "state.get" | "state.get_registers" => Ok(registers(machine, self.revision)),
            "input.joystick.state" => Ok(joystick_state(machine, self.revision)),
            "input.joystick" => {
                self.paused()?;
                let port = machine
                    .bus()
                    .game_port()
                    .as_ref()
                    .ok_or(("no game port configured", -32602))?;
                let index = number(&p["joystick"])?;
                if index >= port.get_controller_count() as u64 {
                    return invalid("joystick index outside configured layout");
                }
                let axis = |name: &str| -> Result<f64> {
                    let value = p[name].as_f64().ok_or(("numeric joystick axis required", -32602))?;
                    if !value.is_finite() || !(-1.0..=1.0).contains(&value) {
                        return invalid("joystick axes must be finite in -1..1");
                    }
                    Ok(value)
                };
                let x = axis("x")?;
                let y = axis("y")?;
                let count = if port.get_controller_count() == 1 { 4 } else { 2 };
                let buttons = p["buttons"].as_array().ok_or(("joystick buttons required", -32602))?;
                if buttons.len() != count || buttons.iter().any(|b| !b.is_boolean()) {
                    return invalid("buttons must match configured layout and be booleans");
                }
                // Preflight everything before mutation. RPC Y increases towards down;
                // native frontend setter negates Y to obtain potentiometer position.
                let port = machine.bus_mut().game_port_mut().as_mut().unwrap();
                port.set_stick_pos(index as usize, 0, Some(x), Some(-y));
                for (button, pressed) in buttons.iter().enumerate() {
                    port.set_button(index as usize, button, pressed.as_bool().unwrap());
                }
                self.revision += 1;
                Ok(joystick_state(machine, self.revision))
            }
            "state.set_registers" => {
                self.paused()?;
                if number(&p["expected_state_revision"])? != self.revision {
                    return invalid("state revision mismatch");
                }
                let changes = p["set"].as_object().ok_or(("set object required", -32602))?;
                if changes.is_empty() {
                    return invalid("at least one register required");
                }
                let mut values = Vec::new();
                for (name, v) in changes {
                    let old = register_value(machine, name)?;
                    let next = number(v)?;
                    if next > 65535 || number(&p["expected"][name])? != old as u64 {
                        return invalid("register guard/range mismatch");
                    }
                    values.push((name.clone(), next as u16));
                }
                let before = registers(machine, self.revision);
                let old_ip = machine.cpu_mut().get_ip();
                let reposition = changes.contains_key("ip") || changes.contains_key("cs");
                if reposition {
                    machine.cpu_mut().flush_piq();
                }
                for (name, value) in values {
                    match name.as_str() {
                        "flags" => machine.cpu_mut().set_flags(value),
                        "ip" => {}
                        _ => machine.cpu_mut().set_register16(reg(&name).unwrap(), value),
                    }
                }
                if reposition {
                    machine.cpu_mut().set_register16(
                        Register16::PC,
                        changes
                            .get("ip")
                            .map(number)
                            .transpose()?
                            .map(|v| v as u16)
                            .unwrap_or(old_ip),
                    );
                }
                self.revision += 1;
                Ok(json!({"before":before,"after":registers(machine,self.revision)}))
            }
            "memory.read" => {
                let start = address(&p["address"])?;
                let length = p.get("length").map(number).transpose()?.unwrap_or(1);
                let data = peek(
                    machine,
                    start,
                    usize::try_from(length).map_err(|_| ("length too large", -32602))?,
                )?;
                Ok(
                    json!({"address":start,"byte_count":data.len(),"data_hex":data.iter().map(|b|format!("{b:02x}")).collect::<String>(),
                    "data_base64":STANDARD.encode(&data),"sha256":digest(&data),"state_revision":self.revision}),
                )
            }
            "memory.write" => {
                self.paused()?;
                let start = address(&p["address"])?;
                let encoded = p["data_base64"].as_str().ok_or(("base64 data required", -32602))?;
                if encoded.len() > 87384 {
                    return invalid("memory write exceeds limit");
                }
                let data = STANDARD.decode(encoded).map_err(|_| ("invalid base64", -32602))?;
                let before = peek(machine, start, data.len())?;
                if let Some(hash) = p.get("expected_sha256") {
                    let hash = hash.as_str().ok_or(("SHA-256 string required", -32602))?;
                    if hash.len() != 64
                        || !hash.bytes().all(|c| c.is_ascii_hexdigit())
                        || hash.to_ascii_lowercase() != digest(&before)
                    {
                        return invalid("memory hash guard mismatch");
                    }
                }
                if (start..start + data.len()).any(|a| !machine.bus().is_writable_ram(a)) {
                    return invalid("only installed writable RAM writes supported");
                }
                for (i, value) in data.iter().enumerate() {
                    machine
                        .bus_mut()
                        .write_u8(start + i, *value, 0)
                        .map_err(|_| ("bus write failed", -32603))?;
                }
                self.revision += 1;
                Ok(
                    json!({"address":start,"byte_count":data.len(),"before_sha256":digest(&before),
                    "after_sha256":digest(&data),"state_revision":self.revision}),
                )
            }
            "breakpoints.create" => {
                if self.breakpoints.len() >= 256 {
                    return invalid("breakpoint limit reached");
                }
                let id = self.id("bp");
                let bp = Breakpoint::parse(id.clone(), p)?;
                let result = bp.value();
                self.breakpoints.insert(id, bp);
                Ok(result)
            }
            "breakpoints.list" => {
                Ok(json!({"breakpoints":self.breakpoints.values().map(Breakpoint::value).collect::<Vec<_>>()}))
            }
            "breakpoints.delete" => {
                let id = p["breakpoint_id"].as_str().ok_or(("breakpoint id required", -32602))?;
                if self.breakpoints.remove(id).is_none() {
                    return invalid("unknown breakpoint");
                }
                Ok(json!({"breakpoint_id":id,"deleted":true}))
            }
            "execution.pause" => {
                if self.running {
                    self.stop(machine, json!({"kind":"pause"}));
                }
                let mut r = registers(machine, self.revision);
                r["paused"] = json!(true);
                Ok(r)
            }
            "execution.continue" | "execution.go" => {
                self.paused()?;
                if !matches!(machine.get_state(), MachineState::On) {
                    return invalid("machine must be powered on");
                }
                self.predicate = None;
                self.deadline = None;
                Ok(self.start(machine))
            }
            "execution.run_until" => {
                self.paused()?;
                if !matches!(machine.get_state(), MachineState::On) {
                    return invalid("machine must be powered on");
                }
                let id = self.id("predicate");
                let mut bp = Breakpoint::parse(id.clone(), &p["predicate"])?;
                bp.once = true;
                let deadline = if let Some(value) = p.get("max_emulated_ns") {
                    let ns = number(value)?;
                    if ns == 0 || ns > 60_000_000_000 {
                        return invalid("guest-time limit must be 1ns..60s");
                    }
                    let start = machine.system_ticks();
                    let ticks = (ns as f64 * machine.system_clock_mhz() / 1000.0).ceil() as u64;
                    Some((
                        start,
                        ns,
                        start.checked_add(ticks).ok_or(("deadline overflow", -32602))?,
                    ))
                } else {
                    None
                };
                self.predicate = Some(bp);
                self.deadline = deadline;
                let mut r = self.start(machine);
                r["predicate_id"] = json!(id);
                Ok(r)
            }
            "execution.wait" => {
                let id = p["operation_id"].as_str().ok_or(("operation id required", -32602))?;
                if p.get("timeout_ms").map(number).transpose()?.unwrap_or(0) > 60000 {
                    return invalid("timeout outside 0..60000ms");
                }
                if self.operation.as_deref() == Some(id) {
                    Ok(json!({"running":true}))
                } else {
                    self.completed
                        .get(id)
                        .cloned()
                        .ok_or(("unknown/expired operation", -32602))
                }
            }
            "execution.step" => {
                self.paused()?;
                if !matches!(machine.get_state(), MachineState::On) {
                    return invalid("machine must be powered on");
                }
                if p.get("mode").is_some_and(|v| v != "into") {
                    return invalid("only step-into supported");
                }
                self.step(machine);
                let kind = if matches!(self.control.state, ExecutionState::Halted) {
                    "cpu_halt"
                } else {
                    "step"
                };
                self.stop(machine, json!({"kind":kind}));
                let mut r = registers(machine, self.revision);
                r["stepping"] = json!(true);
                Ok(r)
            }
            _ => Err(("method not supported", -32601)),
        }
    }
    fn request(&mut self, machine: &mut Machine, line: &[u8]) -> Option<Value> {
        self.request_with_snapshots(machine, line, None)
    }
    fn request_with_snapshots(&mut self, machine: &mut Machine, line: &[u8],
        host: Option<&mut snapshot::SnapshotHost<'_>>) -> Option<Value> {
        let error = |id, code, message| json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}});
        let request: Value = match serde_json::from_slice(line) {
            Ok(v) => v,
            Err(_) => return Some(error(Value::Null, -32700, "invalid JSON")),
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        if !request.is_object()
            || request["jsonrpc"] != "2.0"
            || !request["method"].is_string()
            || request
                .get("id")
                .is_some_and(|v| !v.is_null() && !v.is_string() && !v.is_number())
            || request.get("params").is_some_and(|v| !v.is_object())
        {
            return Some(error(id, -32600, "invalid request"));
        }
        let result = self.handle_with_snapshots(
            machine,
            request["method"].as_str().unwrap(),
            request.get("params").unwrap_or(&json!({})),
            host,
        );
        if request.get("id").is_none() {
            return None;
        }
        Some(match result {
            Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
            Err((message, code)) => error(id, code, message),
        })
    }
}

type Message = (Vec<u8>, Sender<Option<Value>>);
fn connection(mut stream: TcpStream, sender: SyncSender<Message>) -> std::io::Result<()> {
    // Windows accepted sockets can inherit the listener's nonblocking mode.
    // Transport readers are dedicated threads and must wait for complete lines.
    stream.set_nonblocking(false)?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    loop {
        let mut line = Vec::new();
        // Take bounds allocation even when a client never sends a newline.
        let count = reader
            .by_ref()
            .take((MAX_LINE + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            return Ok(());
        }
        if count > MAX_LINE || line.last() != Some(&b'\n') {
            return Ok(());
        }
        let (reply, receive) = mpsc::channel();
        if sender.send((line, reply)).is_err() {
            return Ok(());
        }
        if let Ok(Some(value)) = receive.recv() {
            serde_json::to_writer(&mut stream, &value)?;
            stream.write_all(b"\n")?;
            stream.flush()?;
        }
    }
}
/// The machine stays on its frontend thread. Network threads only queue requests.
/// The native GUI calls `pump` instead of its normal machine runner; both frontends
/// therefore use the same boundary stepping and stop/operation bookkeeping.
pub struct DebugRpc {
    agent: Agent,
    receive: Receiver<Message>,
    stop: Arc<AtomicBool>,
    listener_thread: Option<thread::JoinHandle<()>>,
}
impl DebugRpc {
    pub fn bind(machine: &Machine, port: u16) -> std::io::Result<Self> {
        if port == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "RPC port must be 1..65535",
            ));
        }
        // RPC execution deliberately uses no wall-frame guest housekeeping. Keep
        // refusing software turbo rather than changing clocks according to UI repaint.
        if machine.config().ppi_turbo.is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "PPI software turbo requires unsupported frame housekeeping",
            ));
        }
        Self::from_listener(TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))?)
    }
    fn from_listener(listener: TcpListener) -> std::io::Result<Self> {
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let (sender, receive) = mpsc::sync_channel::<Message>(64);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let listener_thread = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                let stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(_) => break,
                };
                let sender = sender.clone();
                thread::spawn(move || {
                    let _ = connection(stream, sender);
                });
            }
        });
        eprintln!("JSON_RPC_READY=127.0.0.1:{port}");
        Ok(Self {
            agent: Agent::new(port),
            receive,
            stop,
            listener_thread: Some(listener_thread),
        })
    }
    pub fn is_running(&self) -> bool {
        self.agent.running
    }

    /// Nonblocking UI work with a soft host budget checked before every request
    /// and native boundary. An atomic request/instruction may overrun the budget;
    /// it cannot be interrupted halfway through mutation. Paused repaint costs no
    /// guest cycles. The request channel holds at most 64 queued messages.
    pub fn pump(&mut self, machine: &mut Machine, cycle_budget: u32) {
        let start = machine.cpu().get_cycle_ct().0;
        let wall_deadline = Instant::now() + Duration::from_millis(8);
        loop {
            for _ in 0..64 {
                if Instant::now() >= wall_deadline { return; }
                match self.receive.try_recv() {
                    Ok((line, reply)) => {
                        let _ = reply.send(self.agent.request(machine, &line));
                    }
                    Err(_) => break,
                }
            }
            if !self.agent.running
                || machine.cpu().get_cycle_ct().0.saturating_sub(start) >= cycle_budget as u64
                || Instant::now() >= wall_deadline
            {
                break;
            }
            self.agent.advance(machine);
        }
    }
}
impl Drop for DebugRpc {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.listener_thread.take() {
            let _ = thread.join();
        }
    }
}
pub fn serve(machine: &mut Machine, port: u16) -> std::io::Result<()> {
    serve_inner(machine, port, None)
}
/// Headless only: the factory uses loaded immutable dependencies and no live disks.
/// The frontend must own known RW File providers. GUI consumer rebind is not implemented.
pub fn serve_with_snapshots(machine: &mut Machine, port: u16, host: &mut snapshot::SnapshotHost<'_>) -> std::io::Result<()> {
    serve_inner(machine, port, Some(host))
}
fn serve_inner(machine: &mut Machine, port: u16, mut host: Option<&mut snapshot::SnapshotHost<'_>>) -> std::io::Result<()> {
    let mut rpc = DebugRpc::bind(machine, port)?;
    loop {
        if rpc.agent.running {
            for _ in 0..64 {
                match rpc.receive.try_recv() {
                    Ok((line, reply)) => {
                        let _ = reply.send(rpc.agent.request_with_snapshots(machine, &line, host.as_deref_mut()));
                    }
                    Err(_) => break,
                }
            }
            rpc.agent.advance(machine);
        } else if let Ok((line, reply)) = rpc.receive.recv() {
            let _ = reply.send(rpc.agent.request_with_snapshots(machine, &line, host.as_deref_mut()));
        } else {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marty_core::{
        bus::MEM_ROM_BIT,
        machine::{MachineBuilder, MachineRomManifest, MachineState},
        machine_config::MachineConfiguration,
        machine_types::{MachineType, OnHaltBehavior},
    };

    fn machine() -> Machine {
        machine_with_ppi_turbo(None)
    }
    fn machine_with_ppi_turbo(ppi_turbo: Option<bool>) -> Machine {
        machine_with_options(ppi_turbo, OnHaltBehavior::Warn)
    }
    fn machine_with_options(ppi_turbo: Option<bool>, halt: OnHaltBehavior) -> Machine {
        machine_with_game_port(ppi_turbo, halt, false)
    }
    fn machine_with_game_port(ppi_turbo: Option<bool>, halt: OnHaltBehavior, game_port: bool) -> Machine {
        machine_with_ram(ppi_turbo, halt, game_port, 0)
    }
    fn machine_with_ram(ppi_turbo: Option<bool>, halt: OnHaltBehavior, game_port: bool, ram_size: u32) -> Machine {
        let mut config =
            marty_config::read_config(include_str!("../../../../install/martypc.toml"), Default::default()).unwrap();
        assert!(config.emulator.rpc_port.is_none()); // Missing Option keys deserialize as None.
        config.machine.no_roms = true;
        config.machine.cpu.on_halt = Some(halt);
        let description = MachineConfiguration {
            machine_type: MachineType::Ibm5160,
            ppi_turbo,
            conventional_expansion: if ram_size > 0 {
                vec![marty_core::machine_config::ConventionalExpansionConfig {
                    bus_type: marty_core::machine_config::BusType::Isa8,
                    address: 0xD8000,
                    size: ram_size,
                    wait_states: 0,
                }]
            } else {
                vec![]
            },
            game_port: game_port.then_some(marty_core::machine_config::GamePortConfig {
                io_base: 0x201,
                controller_layout: None,
            }),
            ..Default::default()
        };
        let mut machine = MachineBuilder::new()
            .with_core_config(Box::new(&config))
            .with_machine_config(&description)
            .with_roms(MachineRomManifest::new())
            .build()
            .unwrap();
        machine.change_state(MachineState::On);
        machine
            .load_program(
                &[0xb8, 0x34, 0x12, 0x40, 0xa3, 0x00, 0x02, 0xeb, 0xfe],
                0,
                0x100,
                0,
                0x100,
            )
            .unwrap();
        machine
    }
    fn call(a: &mut Agent, m: &mut Machine, method: &str, params: Value) -> Value {
        a.handle(m, method, &params).unwrap()
    }
    #[test]
    fn gui_pump_preserves_paused_state_and_native_boundary_execution() {
        let mut m = machine();
        let mut reference = machine();
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let mut rpc = DebugRpc::from_listener(listener).unwrap();
        let mut headless = Agent::new(endpoint.port());
        let before = registers(&mut m, 0);
        for _ in 0..20 {
            rpc.pump(&mut m, 10000);
        }
        assert_eq!(before, registers(&mut m, 0));
        call(&mut rpc.agent, &mut m, "execution.continue", json!({}));
        call(&mut headless, &mut reference, "execution.continue", json!({}));
        // A tiny GUI budget still completes native boundaries. No second runner
        // may execute between frames; inspection and repaint consume no cycles.
        for _ in 0..30 {
            rpc.pump(&mut m, 1);
            headless.advance(&mut reference);
            assert_eq!(registers(&mut m, 0), registers(&mut reference, 0));
            assert_eq!(m.system_ticks(), reference.system_ticks());
            assert_eq!(peek(&m, 0, 1024).unwrap(), peek(&reference, 0, 1024).unwrap());
        }
        call(&mut rpc.agent, &mut m, "execution.pause", json!({}));
        let stopped = registers(&mut m, 0);
        rpc.pump(&mut m, 10000);
        assert_eq!(stopped, registers(&mut m, 0));
        drop(rpc);
        // Closing the window releases the port; the next slice can reopen it.
        assert!(TcpListener::bind(endpoint).is_ok());
    }
    #[test]
    fn frontend_listener_keeps_idle_connections_open() {
        let mut machine = machine();
        let initial_clock = machine.cpu().get_cycle_ct().0;
        let mut rpc = DebugRpc::from_listener(TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap()).unwrap();
        let port = rpc.agent.port;
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            for id in 0..3 {
                thread::sleep(Duration::from_millis(30));
                writeln!(stream, "{}", json!({"jsonrpc":"2.0","id":id,"method":"state.get"})).unwrap();
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["id"], id);
            }
        });
        let end = Instant::now() + Duration::from_secs(3);
        while !client.is_finished() && Instant::now() < end {
            rpc.pump(&mut machine, 10000);
            thread::sleep(Duration::from_millis(1));
        }
        assert!(client.is_finished());
        client.join().unwrap();
        assert_eq!(machine.cpu().get_cycle_ct().0, initial_clock);
    }
    #[test]
    fn gui_request_flood_is_bounded_and_yields_between_atomic_requests() {
        let mut m = machine();
        let initial_clock = m.cpu().get_cycle_ct().0;
        let (sender, receive) = mpsc::sync_channel(64);
        let mut replies = Vec::new();
        let line = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,
            "method":"memory.read","params":{"address":0,"length":65536}})).unwrap();
        for _ in 0..64 {
            let (reply, r) = mpsc::channel();
            sender.try_send((line.clone(), reply)).unwrap();
            replies.push(r);
        }
        let (reply, _) = mpsc::channel();
        assert!(matches!(sender.try_send((line, reply)), Err(mpsc::TrySendError::Full(_))));
        let mut rpc = DebugRpc { agent: Agent::new(1), receive,
            stop: Arc::new(AtomicBool::new(false)), listener_thread: None };
        rpc.pump(&mut m, 10000);
        let completed = replies.iter().filter(|r| r.try_recv().is_ok()).count();
        assert!(completed < 64, "the original pump drains all64 large requests before checking time");
        assert_eq!(initial_clock, m.cpu().get_cycle_ct().0);
    }
    #[test]
    fn joystick_rpc_drives_native_game_port_and_preflights_all_input() {
        let mut absent = machine();
        let mut a = Agent::new(2301);
        let event = json!({"joystick":0,"x":-1.0,"y":1.0,"buttons":[true,false]});
        assert!(a.handle(&mut absent, "input.joystick", &event).is_err());
        assert_eq!(
            call(&mut a, &mut absent, "input.joystick.state", json!({}))["joysticks"],
            json!([])
        );
        let mut m = machine_with_game_port(None, OnHaltBehavior::Warn, true);
        let clock = m.cpu().get_cycle_ct();
        let state = call(&mut a, &mut m, "input.joystick", event.clone());
        assert_eq!(state["joysticks"][0], event);
        assert_eq!(m.cpu().get_cycle_ct(), clock);
        for bad in [
            json!({"joystick":2,"x":0,"y":0,"buttons":[false,false]}),
            json!({"joystick":0,"x":0,"y":1.01,"buttons":[false,false]}),
            json!({"joystick":0,"x":0,"y":0,"buttons":[false,1]}),
        ] {
            assert!(a.handle(&mut m, "input.joystick", &bad).is_err());
            assert_eq!(call(&mut a, &mut m, "input.joystick.state", json!({})), state);
        }
        let port = m.bus_mut().game_port_mut().as_mut().unwrap();
        port.reset_oneshots();
        assert_eq!(port.port_read() & 0x13, 0x03); // Active-low button, both axes charging.
        port.run(26.0);
        assert_eq!(port.port_read() & 3, 2); // Left axis charged, down still charging.
        port.run(1100.0);
        assert_eq!(port.port_read() & 3, 0);
        a.running = true;
        assert!(a.handle(&mut m, "input.joystick", &event).is_err());
    }
    #[test]
    fn inspection_and_rpc_steps_preserve_native_cpu_device_cycles() {
        let mut native = machine();
        let mut controlled = machine();
        let mut a = Agent::new(2301);
        let mut control = ExecutionControl::new();
        for _ in 0..40 {
            let before = controlled.cpu().get_cycle_ct();
            for _ in 0..3 {
                call(&mut a, &mut controlled, "state.get_registers", json!({}));
                call(
                    &mut a,
                    &mut controlled,
                    "memory.read",
                    json!({"address":{"space":"segmented","segment":0,"offset":"0x100"},"length":9}),
                );
            }
            assert_eq!(before, controlled.cpu().get_cycle_ct());
            call(&mut a, &mut controlled, "execution.step", json!({"mode":"into"}));
            control.set_op(ExecutionOperation::Step);
            native.run(1, &mut control);
            assert_eq!(
                registers(&mut native, a.revision),
                registers(&mut controlled, a.revision)
            );
            assert_eq!(
                native.bus().pit().as_ref().unwrap().get_cycles(),
                controlled.bus().pit().as_ref().unwrap().get_cycles()
            );
            assert_eq!(peek(&native, 0x200, 2).unwrap(), peek(&controlled, 0x200, 2).unwrap());
        }
        assert_eq!(controlled.cpu().get_register16(Register16::AX), 0x1235);
        assert_eq!(peek(&controlled, 0x200, 2).unwrap(), vec![0x35, 0x12]);
    }

    #[test]
    fn rpc_continue_matches_batched_native_run_with_pit_irqs_and_rep() {
        // 0000:0100 installs IRQ0 at 0180, PIC vector 8, PIT divisor 0100h.
        // Repeated MOVSB copies 32 bytes while IRQ0 increments [0282] and EOIs.
        // NASM and pynasm emit the same 139-byte image including NOP padding.
        let mut code = vec![
            0xfa, 0x31, 0xc0, // CLI; XOR AX,AX
            0x8e, 0xd8, 0x8e, 0xc0, 0x8e, 0xd0, // DS/ES/SS := AX
            0xbc, 0x00, 0x80, // MOV SP,8000h
            0xc7, 0x06, 0x20, 0x00, 0x80, 0x01, // IRQ0 offset := 0180h
            0xc7, 0x06, 0x22, 0x00, 0x00, 0x00, // IRQ0 segment := 0
            0xb0, 0x13, 0xe6, 0x20, // PIC ICW1, single/ICW4
            0xb0, 0x08, 0xe6, 0x21, // PIC vector 8
            0xb0, 0x01, 0xe6, 0x21, // PIC 8086 mode
            0xb0, 0xfe, 0xe6, 0x21, // Unmask IRQ0 only
            0xb0, 0x36, 0xe6, 0x43, // PIT channel0, mode3, low/high
            0xb8, 0x00, 0x01, 0xe6, 0x40, 0x88, 0xe0, 0xe6, 0x40, // Divisor 0100h
            0xfb, // STI
            0xbe, 0x00, 0x03, 0xbf, 0x00, 0x05, 0xb9, 0x20, 0x00, // SI/DI/CX
            0xf3, 0xa4, // REP MOVSB
            0xff, 0x06, 0x80, 0x02, // INC word [0280h]
            0xeb, 0xef, // JMP back to MOV SI
        ];
        code.resize(0x80, 0x90);
        code.extend_from_slice(&[
            0x50, // PUSH AX
            0xff, 0x06, 0x82, 0x02, // INC word [0282h]
            0xb0, 0x20, 0xe6, 0x20, // MOV AL,20h; OUT 20h,AL (EOI)
            0x58, 0xcf, // POP AX; IRET
        ]);
        assert_eq!(
            digest(&code),
            "49a3c19b6146120dbb758398761f36a3db3bee6e3431025dce1ffb4fd54cc7ba"
        );
        let mut native = machine();
        let mut controlled = machine();
        for m in [&mut native, &mut controlled] {
            m.load_program(&code, 0, 0x100, 0, 0x100).unwrap();
            for i in 0..32 {
                m.bus_mut().write_u8(0x300 + i, (i + 1) as u8, 0).unwrap();
            }
        }
        let mut a = Agent::new(2301);
        let mut control = ExecutionControl::new();
        control.set_op(ExecutionOperation::Run);
        call(&mut a, &mut controlled, "execution.continue", json!({}));
        for _ in 0..12 {
            native.run(25_000, &mut control);
            let target = native.cpu().get_cycle_ct().0;
            while controlled.cpu().get_cycle_ct().0 < target {
                // Inspection between every boundary must not change guest time/prefetch.
                call(&mut a, &mut controlled, "state.get_registers", json!({}));
                a.advance(&mut controlled);
            }
            assert_eq!(registers(&mut native, 0), registers(&mut controlled, 0));
            assert_eq!(
                format!("{:?}", native.cpu().get_string_state()),
                format!("{:?}", controlled.cpu().get_string_state())
            );
            assert_eq!(native.system_ticks(), controlled.system_ticks());
            assert_eq!(
                native.bus().pit().as_ref().unwrap().get_cycles(),
                controlled.bus().pit().as_ref().unwrap().get_cycles()
            );
            let n = native.bus().pic().as_ref().unwrap().get_string_state();
            let c = controlled.bus().pic().as_ref().unwrap().get_string_state();
            assert_eq!(
                (n.imr, n.isr, n.irr, n.intr, n.interrupt_stats),
                (c.imr, c.isr, c.irr, c.intr, c.interrupt_stats)
            );
            assert_eq!(peek(&native, 0, 32768).unwrap(), peek(&controlled, 0, 32768).unwrap());
        }
        let memory = peek(&controlled, 0x280, 4).unwrap();
        assert!(
            u16::from_le_bytes([memory[0], memory[1]]) > 0,
            "REP loop never completed"
        );
        assert!(u16::from_le_bytes([memory[2], memory[3]]) > 0, "IRQ0 never executed");
        assert_eq!(
            peek(&controlled, 0x300, 32).unwrap(),
            peek(&controlled, 0x500, 32).unwrap()
        );
        println!(
            "Batched/RPC equality: cycles={}, crystal_ticks={}, completed_copies={}, IRQ0_entries={}",
            controlled.cpu().get_cycle_ct().0,
            controlled.system_ticks(),
            u16::from_le_bytes([memory[0], memory[1]]),
            u16::from_le_bytes([memory[2], memory[3]])
        );
    }
    #[test]
    fn register_guards_preflight_the_entire_request() {
        let mut m = machine();
        let mut a = Agent::new(2301);
        let before = registers(&mut m, 0);
        let wrong = json!({"expected_state_revision":0,"expected":{"ax":0,"cx":65535},"set":{"ax":9,"cx":8}});
        assert!(a.handle(&mut m, "state.set_registers", &wrong).is_err());
        assert_eq!(registers(&mut m, 0), before);
        let ax = before["general"]["ax"].clone();
        let done = call(
            &mut a,
            &mut m,
            "state.set_registers",
            json!({"expected_state_revision":0,"expected":{"ax":ax},"set":{"ax":9}}),
        );
        assert_eq!(done["after"]["general"]["ax"], 9);
        assert_eq!(a.revision, 1);
        assert!(a
            .handle(
                &mut m,
                "state.set_registers",
                &json!({"expected_state_revision":0,"expected":{"ax":9},"set":{"ax":10}})
            )
            .is_err());
        assert_eq!(m.cpu().get_register16(Register16::AX), 9);
    }
    #[test]
    fn writable_expansion_range_excludes_rom_and_both_boundaries() {
        use marty_core::devices::conventional_memory::ConventionalMemory;
        let ram = ConventionalMemory::new(0xD8000, 8192, 0, false);
        assert!(ram.is_writable(0xD8000));
        assert!(ram.is_writable(0xD9FFF));
        for address in [0, 0xD7FFF, 0xDA000, usize::MAX] {
            assert!(!ram.is_writable(address));
        }
        let rom = ConventionalMemory::new_rom(0xD8000, 8192, 0, &[0; 8192]);
        assert!(!rom.is_writable(0xD8000));
    }

    #[test]
    fn configured_mailbox_shares_guest_cpu_and_guarded_rpc_ram() {
        let mut m = machine_with_ram(None, OnHaltBehavior::Warn, false, 8192);
        let mut a = Agent::new(2301);
        // Real 8088 ES word store/load in the expansion; store loaded value in low RAM.
        m.load_program(
            &[
                0xB8, 0x00, 0xD8, 0x8E, 0xC0, 0x26, 0xC7, 0x06, 0, 0, 0x34, 0x12, 0x26, 0xA1, 0, 0, 0xA3, 0, 2, 0xF4,
            ],
            0,
            0x100,
            0,
            0x100,
        )
        .unwrap();
        for _ in 0..6 {
            call(&mut a, &mut m, "execution.step", json!({}));
        }
        assert_eq!(peek(&m, 0xD8000, 2).unwrap(), vec![0x34, 0x12]);
        assert_eq!(peek(&m, 0x200, 2).unwrap(), vec![0x34, 0x12]);
        let clock = m.cpu().get_cycle_ct();
        let before = peek(&m, 0xD8000, 2).unwrap();
        call(
            &mut a,
            &mut m,
            "memory.write",
            json!({"address":0xD8000,
            "data_base64":STANDARD.encode([0x78,0x56]),"expected_sha256":digest(&before)}),
        );
        assert_eq!(m.cpu().get_cycle_ct(), clock);
        m.load_program(
            &[0xB8, 0, 0xD8, 0x8E, 0xC0, 0x26, 0xA1, 0, 0, 0xA3, 2, 2, 0xF4],
            0,
            0x100,
            0,
            0x100,
        )
        .unwrap();
        for _ in 0..5 {
            call(&mut a, &mut m, "execution.step", json!({}));
        }
        assert_eq!(peek(&m, 0x202, 2).unwrap(), vec![0x78, 0x56]);
    }

    #[test]
    fn expansion_write_refuses_absent_ram_boundaries_rom_and_running_state() {
        let mut absent = machine();
        let mut a = Agent::new(2301);
        assert!(a
            .handle(
                &mut absent,
                "memory.write",
                &json!({"address":0xD8000,
            "data_base64":STANDARD.encode([1])})
            )
            .is_err());
        let mut m = machine_with_ram(None, OnHaltBehavior::Warn, false, 8192);
        for address in [0xD7FFF, 0xD9FFF] {
            let before = peek(&m, address, 2).unwrap();
            assert!(a
                .handle(
                    &mut m,
                    "memory.write",
                    &json!({"address":address,
                "data_base64":STANDARD.encode([7,8])})
                )
                .is_err());
            assert_eq!(peek(&m, address, 2).unwrap(), before);
        }
        let before = peek(&m, 0xD8000, 2).unwrap();
        assert!(a
            .handle(
                &mut m,
                "memory.write",
                &json!({"address":0xD8000,
            "data_base64":STANDARD.encode([7,8]),"expected_sha256":digest(&[1,2])})
            )
            .is_err());
        a.running = true;
        assert!(a
            .handle(
                &mut m,
                "memory.write",
                &json!({"address":0xD8000,
            "data_base64":STANDARD.encode([7,8])})
            )
            .is_err());
        a.running = false;
        m.bus_mut().set_flags(0xD8001, MEM_ROM_BIT);
        assert!(a
            .handle(
                &mut m,
                "memory.write",
                &json!({"address":0xD8000,
            "data_base64":STANDARD.encode([7,8])})
            )
            .is_err());
        assert_eq!(peek(&m, 0xD8000, 2).unwrap(), before);
        assert_eq!(a.revision, 0);
    }

    #[test]
    fn guarded_ram_write_and_refused_boundary_write_are_atomic() {
        let mut m = machine();
        let mut a = Agent::new(2301);
        let before = peek(&m, 0x200, 2).unwrap();
        let p = json!({"address":"0x200","data_base64":STANDARD.encode([1,2]),"expected_sha256":digest(&before)});
        let mut bad = p.clone();
        bad["expected_sha256"] = json!(digest(&[7, 7]));
        assert!(a.handle(&mut m, "memory.write", &bad).is_err());
        assert_eq!(peek(&m, 0x200, 2).unwrap(), before);
        assert_eq!(a.revision, 0);
        call(&mut a, &mut m, "memory.write", p);
        assert_eq!(peek(&m, 0x200, 2).unwrap(), vec![1, 2]);
        let address = m.bus().conventional_size() - 1;
        let old = peek(&m, address, 2).unwrap();
        let p = json!({"address":address,"data_base64":STANDARD.encode([7,8]),"expected_sha256":digest(&old)});
        assert!(a.handle(&mut m, "memory.write", &p).is_err());
        assert_eq!(peek(&m, address, 2).unwrap(), old);
        assert_eq!(a.revision, 1);
        assert!(a
            .handle(&mut m, "memory.read", &json!({"address":"0xFFFFF","length":2}))
            .is_err());
        assert!(a
            .handle(&mut m, "memory.read", &json!({"address":0,"length":65537}))
            .is_err());
    }
    #[test]
    fn one_shot_breakpoint_operation_and_pause_race() {
        let mut m = machine();
        let mut a = Agent::new(2301);
        let bp = call(
            &mut a,
            &mut m,
            "breakpoints.create",
            json!({"kind":"execution","address":0x103,"once":true}),
        );
        let op = call(&mut a, &mut m, "execution.continue", json!({}));
        assert!(a.handle(&mut m, "execution.step", &json!({})).is_err());
        for _ in 0..10 {
            a.advance(&mut m);
            if !a.running {
                break;
            }
        }
        let done = call(
            &mut a,
            &mut m,
            "execution.wait",
            json!({"operation_id":op["operation_id"],"timeout_ms":5}),
        );
        assert_eq!(done["state"], "stopped");
        assert_eq!(done["stop_reason"]["kind"], "breakpoint");
        assert_eq!(done["stop_reason"]["breakpoint_id"], bp["breakpoint_id"]);
        assert_eq!(done["ip"], 0x103);
        assert!(a.breakpoints.is_empty());
        let reason = a.last_stop.clone();
        call(&mut a, &mut m, "execution.pause", json!({}));
        assert_eq!(a.last_stop, reason);
        assert!(a
            .handle(&mut m, "execution.wait", &json!({"operation_id":"absent"}))
            .is_err());
    }
    #[test]
    fn persistent_breakpoint_skips_only_the_resume_boundary() {
        let mut m = machine();
        let mut a = Agent::new(2301);
        call(
            &mut a,
            &mut m,
            "breakpoints.create",
            json!({"kind":"execution","address":0x107}),
        );
        call(&mut a, &mut m, "execution.continue", json!({}));
        for _ in 0..10 {
            a.advance(&mut m);
            if !a.running {
                break;
            }
        }
        assert!(!a.running);
        let first = a.revision;
        call(&mut a, &mut m, "execution.continue", json!({}));
        a.advance(&mut m);
        assert!(a.running);
        assert_eq!(a.revision, first + 1);
        a.advance(&mut m);
        assert!(!a.running);
        assert_eq!(a.last_stop["hit_count"], 2);
    }
    #[test]
    fn run_until_is_private_and_guest_deadline_is_bounded() {
        let mut m = machine();
        let mut a = Agent::new(2301);
        let op = call(
            &mut a,
            &mut m,
            "execution.run_until",
            json!({"predicate":{"kind":"execution","address":0x104,"condition":{"register":"ax","operator":"eq","value":"0x1235"}}}),
        );
        for _ in 0..10 {
            a.advance(&mut m);
            if !a.running {
                break;
            }
        }
        let done = call(
            &mut a,
            &mut m,
            "execution.wait",
            json!({"operation_id":op["operation_id"]}),
        );
        assert_eq!(done["stop_reason"]["kind"], "run_until");
        assert_eq!(done["ip"], 0x104);
        assert!(a.breakpoints.is_empty());
        assert!(a.predicate.is_none());
        call(
            &mut a,
            &mut m,
            "execution.run_until",
            json!({"predicate":{"kind":"execution","address":0},"max_emulated_ns":1000}),
        );
        for _ in 0..100 {
            a.advance(&mut m);
            if !a.running {
                break;
            }
        }
        assert!(!a.running);
        assert_eq!(a.last_stop["kind"], "emulated_time_limit");
    }
    #[test]
    fn jsonrpc_envelopes_aliases_errors_and_notifications() {
        let mut m = machine();
        let mut a = Agent::new(2301);
        assert_eq!(a.request(&mut m, b"{").unwrap()["error"]["code"], -32700);
        assert_eq!(
            a.request(&mut m, br#"{"method":"state.get","id":1}"#).unwrap()["error"]["code"],
            -32600
        );
        assert_eq!(
            a.request(&mut m, br#"{"jsonrpc":"2.0","method":"unknown","id":1}"#)
                .unwrap()["error"]["code"],
            -32601
        );
        assert_eq!(
            a.request(
                &mut m,
                br#"{"jsonrpc":"2.0","method":"memory.read","params":{"address":true},"id":1}"#
            )
            .unwrap()["error"]["code"],
            -32602
        );
        assert!(a
            .request(&mut m, br#"{"jsonrpc":"2.0","method":"state.get"}"#)
            .is_none());
        assert_eq!(
            call(&mut a, &mut m, "state.get", json!({})),
            call(&mut a, &mut m, "state.get_registers", json!({}))
        );
        assert_eq!(
            call(&mut a, &mut m, "agent.capabilities", json!({})),
            call(&mut a, &mut m, "emulator.info", json!({}))
        );
    }
    #[test]
    fn step_clears_the_persistent_breakpoint_resume_exemption() {
        let mut m = machine();
        let mut a = Agent::new(2301);
        call(&mut a, &mut m, "breakpoints.create", json!({"address":0x107}));
        call(&mut a, &mut m, "execution.continue", json!({}));
        for _ in 0..10 {
            a.advance(&mut m);
            if !a.running {
                break;
            }
        }
        assert_eq!(a.last_stop["kind"], "breakpoint");
        call(&mut a, &mut m, "execution.step", json!({}));
        assert_eq!(a.last_stop["kind"], "step");
        let revision = a.revision;
        call(&mut a, &mut m, "execution.continue", json!({}));
        a.advance(&mut m);
        assert!(!a.running);
        assert_eq!(a.revision, revision);
        assert_eq!(a.last_stop["hit_count"], 2);
    }
    #[test]
    fn native_halt_policies_return_and_keep_rpc_responsive() {
        for halt in [OnHaltBehavior::Continue, OnHaltBehavior::Warn, OnHaltBehavior::Stop] {
            let mut m = machine_with_options(None, halt);
            let mut a = Agent::new(2301);
            m.load_program(&[0xfa, 0xf4], 0, 0x100, 0, 0x100).unwrap();
            for _ in 0..100 {
                call(&mut a, &mut m, "execution.step", json!({}));
            }
            assert!(registers(&mut m, a.revision)["in_hlt"].as_bool().unwrap());
            call(&mut a, &mut m, "execution.pause", json!({}));
        }
    }
    #[test]
    fn unsupported_soft_turbo_is_refused_before_starting_a_listener() {
        let mut m = machine_with_ppi_turbo(Some(true));
        assert!(serve(&mut m, 2301).unwrap_err().to_string().contains("software turbo"));
    }
    #[test]
    fn flags_text_matches_pypc_legacy_order_including_trap() {
        let mut m = machine();
        m.cpu_mut().set_flags(0x0402);
        assert_eq!(registers(&mut m, 0)["flags_text"], "--------"); // DF is in raw flags only.
        m.cpu_mut().set_flags(0x0bd7);
        assert_eq!(registers(&mut m, 0)["flags_text"], "oITszapc");
    }
    #[test]
    fn default_execution_and_segmented_alias_semantics_match_pypc() {
        let mut m = machine();
        let mut alias = Breakpoint::parse(
            "alias".into(),
            &json!({"address": {
            "space":"segmented","segment":0x10,"offset":0},"condition":null}),
        )
        .unwrap();
        assert!(!alias.matches(&mut m)); // 0010:0000 aliases 0000:0100, but CS differs.
        assert_eq!(alias.hits, 0);
        let mut exact = Breakpoint::parse(
            "exact".into(),
            &json!({"address": {
            "space":"segmented","segment":0,"offset":0x100}}),
        )
        .unwrap();
        assert!(exact.matches(&mut m));
        assert_eq!(
            exact.value()["address"],
            json!({"space":"segmented","segment":0,"offset":0x100})
        );
        assert_eq!(address(&json!({"offset":0x100})).unwrap(), 0x100);
        assert_eq!(
            address(&json!({"space":"segmented","segment":0xffff,"offset":0x10})).unwrap(),
            0
        );
        assert!(Breakpoint::parse("bad".into(), &json!({"address":0x100,"length":0})).is_err());
        // PyPC accepts length metadata on execution predicates but matches only
        // the starting instruction address (debugbreakpoints.matches_address).
        let mut metadata = Breakpoint::parse("metadata".into(), &json!({"address":0x100,"length":4})).unwrap();
        assert!(metadata.matches(&mut m));
        let mut control = ExecutionControl::new();
        control.set_op(ExecutionOperation::Step);
        m.run(1, &mut control);
        assert_eq!(m.cpu_mut().get_ip(), 0x103);
        assert!(!metadata.matches(&mut m));
    }
    #[test]
    fn optional_hash_guard_and_fifo_operation_retention() {
        let mut m = machine();
        let mut a = Agent::new(2301);
        call(
            &mut a,
            &mut m,
            "memory.write",
            json!({"address":0x200,"data_base64":STANDARD.encode([8,9])}),
        );
        assert_eq!(peek(&m, 0x200, 2).unwrap(), vec![8, 9]);
        let mut ids = Vec::new();
        for _ in 0..100 {
            ids.push(
                call(&mut a, &mut m, "execution.continue", json!({}))["operation_id"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            );
            call(&mut a, &mut m, "execution.pause", json!({}));
        }
        assert_eq!(a.completed.len(), 64);
        for (i, id) in ids.iter().enumerate() {
            assert_eq!(a.completed.contains_key(id), i >= 36);
        }
    }
    #[test]
    fn guest_deadline_uses_system_ticks_through_turbo_change() {
        let mut m = machine();
        let mut a = Agent::new(2301);
        let crystal = m.system_clock_mhz();
        call(
            &mut a,
            &mut m,
            "execution.run_until",
            json!({"predicate":{"kind":"execution","address":0},"max_emulated_ns":100000}),
        );
        m.set_turbo_mode(true);
        for _ in 0..1000 {
            a.advance(&mut m);
            if !a.running {
                break;
            }
        }
        assert!(!a.running);
        assert_eq!(m.system_clock_mhz(), crystal);
        let limit = &a.last_stop["emulated_time_limit"];
        assert_eq!(limit["requested_duration_ns"], 100000);
        assert!(
            limit["actual_stop_emulated_time_ns"].as_u64().unwrap()
                >= limit["deadline_emulated_time_ns"].as_u64().unwrap()
        );
        assert_eq!(
            a.last_stop["emulated_time_ns"],
            registers(&mut m, a.revision)["emulated_time_ns"]
        );
        m.change_state(MachineState::Off);
        assert!(a.handle(&mut m, "execution.continue", &json!({})).is_err());
        assert!(a.handle(&mut m, "execution.step", &json!({})).is_err());
    }
    #[test]
    fn tcp_persistent_json_lines_and_notifications() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, receive) = mpsc::sync_channel(64);
        let transport = thread::spawn(move || connection(listener.accept().unwrap().0, sender).unwrap());
        let worker = thread::spawn(move || {
            let mut m = machine();
            let mut a = Agent::new(port);
            while let Ok((line, reply)) = receive.recv() {
                reply.send(a.request(&mut m, &line)).unwrap();
            }
        });
        let mut client = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
        client.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        client.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"state.get\"}\n{\"jsonrpc\":\"2.0\",\"method\":\"state.get\",\"id\":7}\n").unwrap();
        let mut reader = BufReader::new(client);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], 7);
        assert_eq!(response["result"]["ip"], 0x100);
        reader.get_mut().write_all(b"{bad}\n").unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["error"]["code"], -32700);
        drop(reader);
        transport.join().unwrap();
        worker.join().unwrap();
    }
}
