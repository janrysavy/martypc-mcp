use super::*;
use marty_core::bus::debug_observation::Event;

#[derive(Default)]
pub(super) struct CpuTrace {
    pub active: bool,
    started: bool,
    effect_count: usize,
    remaining: usize,
    detail: String,
    pub(super) events: Vec<Value>,
}
#[derive(Default)]
pub(super) struct HardwareTrace {
    pub active: bool,
    started: bool,
    capacity: usize,
    pub(super) include_io: bool,
    pub(super) include_irq: bool,
    ports: Vec<(u16, u16)>,
    irqs: Vec<u8>,
    events: VecDeque<Value>,
    next: u64,
    dropped: u64,
}
fn instruction(event: &Event) -> Value {
    json!({"space":"segmented","segment":event.cs,"offset":event.ip,
        "physical":(((event.cs as u32)<<4)+event.ip as u32)&0xfffff})
}
fn effect(event: &Event) -> Value {
    let mut out = json!({"kind":event.kind,"clock":if event.irq.is_none() {Some(event.clock)} else {None},"address":instruction(event)});
    if event.kind.starts_with("memory_") {
        let data = event.value.to_le_bytes()[..event.width].to_vec();
        out["address"] = json!({"space":"linear","offset":event.address});
        out["instruction_address"] = instruction(event);
        out["byte_count"] = json!(event.width);
        if event.kind == "memory_read" {
            out["data_base64"] = json!(STANDARD.encode(data));
        } else {
            out["before_base64"] = json!(event.old.as_ref().map(|old| STANDARD.encode(old)));
            out["after_base64"] = json!(STANDARD.encode(data));
        }
    } else if event.kind.starts_with("io_") {
        out["port"] = json!(event.address & 0xffff);
        out["byte_count"] = json!(event.width);
        out["value"] = json!(event.value);
        out["handled"] = json!(event.handled);
    } else {
        out["number"] = json!(event.address);
        out["interrupt_type"] = json!(event.interrupt_kind);
        out["ah"] = json!(event.ah);
        out["al"] = json!(event.al);
    }
    out
}
impl Breakpoint {
    fn matches_event(&mut self, machine: &mut Machine, event: &Event) -> bool {
        let matches = if self.kind == "interrupt" {
            event.kind == "interrupt"
                && event.interrupt_kind.as_deref() == Some("Software")
                && self.event["number"] == event.address
                && self.event.get("ah").is_none_or(|ah| ah == &json!(event.ah))
                && self.event.get("al").is_none_or(|al| al == &json!(event.al))
        } else {
            (self.kind == event.kind || (self.kind == "memory_access" && event.kind.starts_with("memory_")))
                && (event.address as u64) < self.address as u64 + self.length
                && self.address < event.address as usize + event.width
        };
        if !matches {
            return false;
        }
        if self.kind == "interrupt" {
            if let Some((name, op, value)) = &self.condition {
                let names = [
                    "ax", "bx", "cx", "dx", "sp", "bp", "si", "di", "cs", "ds", "es", "ss", "ip", "flags",
                ];
                let actual = event.registers.as_ref().unwrap()[names.iter().position(|n| n == name).unwrap()];
                if !compare(actual, op, *value) {
                    return false;
                }
            }
            self.selected_hit()
        } else {
            self.condition_matches(machine)
        }
    }
    fn event_reason(&self, event: &Event, private: bool) -> Value {
        let mut reason = json!({"kind":if private {"run_until"} else {"breakpoint"},
            "breakpoint_id":self.id,"address":self.address_value,"hit_count":self.hits,
            "phase":"after_native_boundary"});
        if private {
            reason["predicate_id"] = json!(self.id);
        }
        if self.kind == "interrupt" {
            reason["event"] = json!({"type":"software_interrupt","number":event.address,
                "ah":event.ah,"al":event.al,"phase":"after_dispatch_before_handler"});
        } else {
            reason["access"] = json!({"kind":event.kind,"address":{"space":"linear","offset":event.address},
                "byte_count":event.width,"new_value":event.value,"instruction_address":instruction(event),"clock":event.clock});
            let old = if event.kind == "memory_read" {
                Some(event.value.to_le_bytes()[..event.width].to_vec())
            } else {
                event.old.clone()
            };
            if let Some(old) = old {
                reason["access"]["old_value"] = json!(old
                    .iter()
                    .enumerate()
                    .fold(0u16, |v, (n, b)| v | ((*b as u16) << (n * 8))));
            }
        }
        reason
    }
}
impl HardwareTrace {
    fn meta(&self) -> Value {
        json!({"active":self.active,"capacity":self.capacity,"dropped_event_count":self.dropped,
            "first_available_sequence":self.next-self.events.len() as u64,"event_count":self.events.len(),
            "irq_scope":"native_pic_line_transitions_and_accepted_dispatch", "time_base":"native_cpu_cycles"})
    }
    fn push(&mut self, event: &Event, before: &Value, after: &Value) {
        let io = event.kind.starts_with("io_")
            && self.include_io
            && (self.ports.is_empty()
                || self
                    .ports
                    .iter()
                    .any(|(first, last)| (*first as u32..=*last as u32).contains(&event.address)));
        let irq = event.irq.is_some()
            && self.include_irq
            && (self.irqs.is_empty() || self.irqs.contains(&event.irq.unwrap()));
        if !io && !irq {
            return;
        }

        let mut value = effect(event);
        if irq {
            value["kind"] = json!(event.kind);
            value["irq"] = json!(event.irq);
            if event.kind == "irq_dispatch" {
                value["vector"] = json!(event.address);
            }
            value["address"] = json!({"space":"segmented","segment":before["segments"]["cs"],"offset":before["ip"]});
            value["time_scope"] = json!("native_boundary_interval");
            value["clock_start"] = before["clock"].clone();
            value["clock_end"] = after["clock"].clone();
        }
        value["sequence"] = json!(self.next);
        if !irq {
            value["emulated_time"] = json!(event.clock);
        }
        self.next += 1;
        if self.events.len() == self.capacity {
            self.events.pop_front();
            self.dropped += 1;
        }
        self.events.push_back(value);
    }
}
impl Agent {
    pub(super) fn require_observation_cpu(&self, machine: &Machine, bp: &Breakpoint) -> Result<()> {
        if bp.kind != "execution" && !format!("{:?}", machine.cpu().get_type()).starts_with("Intel") {
            return invalid("native observation currently supports Intel8088/8086 only");
        }
        Ok(())
    }
    pub(super) fn record_boundary(
        &mut self,
        machine: &mut Machine,
        before: Option<Value>,
        events: &[Event],
        dropped: usize,
        skip: Option<&str>,
    ) {
        let before = before.unwrap();
        let after = registers(machine, self.revision);
        if self.trace.active {
            let opcodes = events
                .iter()
                .filter(|e| e.kind == "instruction_byte")
                .map(|e| e.value as u8)
                .collect::<Vec<_>>();
            let effects = events
                .iter()
                .filter(|e| e.kind.starts_with("memory_") || e.kind.starts_with("io_"))
                .collect::<Vec<_>>();
            let available = 65536usize.saturating_sub(self.trace.effect_count);
            let retained = effects.len().min(available);
            let kind = if !opcodes.is_empty() {
                "instruction"
            } else if events.iter().any(|e| e.interrupt_kind.as_deref() == Some("Hardware")) {
                "interrupt_dispatch"
            } else if before["in_hlt"] == true {
                "hlt"
            } else {
                "native_boundary"
            };
            let mut event = json!({"kind":kind,"address":{"space":"segmented",
                "segment":before["segments"]["cs"],"offset":before["ip"]},
                "physical":(((before["segments"]["cs"].as_u64().unwrap())<<4)+before["ip"].as_u64().unwrap())&0xfffff,
                "sequence":self.trace.events.len(),"opcode_hex":opcodes.iter().map(|b|format!("{b:02x}")).collect::<String>(),
                "opcode_scope":"consumed_native_prefetch_bytes",
                "clock_before":before["clock"],"clock_after":after["clock"],
                "clock_delta":after["clock"].as_u64().unwrap()-before["clock"].as_u64().unwrap(),
                "effects":effects[..retained].iter().map(|e|effect(e)).collect::<Vec<_>>(),"dropped_effect_count":dropped+effects.len()-retained});
            if let Some(dispatch) = events.iter().find(|e| e.kind == "irq_dispatch") {
                event["interrupt"] = json!({"source":"pic","irq":dispatch.irq,"vector":dispatch.address});
            } else if let Some(dispatch) = events.iter().find(|e| e.interrupt_kind.as_deref() == Some("Hardware")) {
                // The vector is native; do not invent a PIC line when its source is unavailable.
                event["interrupt"] = json!({"source":"hardware","vector":dispatch.address});
            }
            self.trace.effect_count += retained;
            if self.trace.detail != "csip" {
                event["registers_before"] = before.clone();
            }
            if ["normal", "long"].contains(&self.trace.detail.as_str()) {
                event["registers_after"] = registers(machine, self.revision);
            }
            self.trace.events.push(event);
            self.trace.remaining -= 1;
            self.trace.active = self.trace.remaining > 0 && retained == effects.len();
        }
        if self.hardware.active {
            for event in events {
                self.hardware.push(event, &before, &after);
            }
        }
        if dropped > 0 {
            self.stop(
                machine,
                json!({"kind":"observation_overflow","dropped_effect_count":dropped}),
            );
            return;
        }
        for event in events {
            let mut found = None;
            for bp in self.ordered_breakpoints_mut() {
                if skip == Some(bp.id.as_str()) {
                    continue;
                }
                if bp.matches_event(machine, event) {
                    found = Some((bp.clone(), false));
                    break;
                }
            }
            if found.is_none() {
                if let Some(bp) = &mut self.predicate {
                    if bp.matches_event(machine, event) {
                        found = Some((bp.clone(), true));
                    }
                }
            }
            if let Some((bp, private)) = found {
                if bp.once && !private {
                    self.breakpoints.remove(&bp.id);
                }
                self.stop(machine, bp.event_reason(event, private));
                break;
            }
        }
    }
    pub(super) fn trace_request(&mut self, _machine: &mut Machine, method: &str, p: &Value) -> Result<Value> {
        match method {
            "trace.start" => {
                self.paused()?;
                if self.trace.active {
                    return invalid("a CPU trace is already active");
                }
                let count = p.get("instruction_count").map(number).transpose()?.unwrap_or(256);
                if !(1..=65536).contains(&count) {
                    return invalid("trace count outside 1..65536");
                }
                let detail = p
                    .get("detail")
                    .map(|v| v.as_str().ok_or(("trace detail string required", -32602)))
                    .transpose()?
                    .unwrap_or("normal");
                if !["csip", "short", "normal", "long"].contains(&detail) {
                    return invalid("unknown trace detail");
                }
                self.trace = CpuTrace {
                    active: true,
                    started: true,
                    effect_count: 0,
                    remaining: count as usize,
                    detail: detail.to_owned(),
                    events: Vec::new(),
                };
                Ok(json!({"active":true,"detail":detail,"instruction_count":count,"scope":"native_machine_boundaries"}))
            }
            "trace.stop" => {
                if !self.trace.started {
                    return invalid("no CPU trace has been started");
                }
                self.trace.active = false;
                Ok(json!({"active":false,"event_count":self.trace.events.len()}))
            }
            "trace.read" => {
                if !self.trace.started {
                    return invalid("no CPU trace has been started");
                }
                if limit(p)? > 256 {
                    return invalid("CPU trace limit outside 1..256");
                }
                let start = cursor(p, "trace", self.trace.events.len() as u64)? as usize;
                let limit = limit(p)?;
                let end = (start + limit).min(self.trace.events.len());
                Ok(
                    json!({"events":self.trace.events[start..end],"event_count":self.trace.events.len(),
                    "active":self.trace.active,"detail":self.trace.detail,
                    "next_cursor":if self.trace.active || end<self.trace.events.len() {Some(format!("trace-{end}"))} else {None}}),
                )
            }
            "hardware.trace.start" => {
                if self.hardware.active {
                    return invalid("a hardware trace is already active");
                }
                let capacity = p.get("capacity").map(number).transpose()?.unwrap_or(4096);
                if !(1..=65536).contains(&capacity) {
                    return invalid("hardware trace capacity outside 1..65536");
                }
                let boolean = |key| {
                    p.get(key)
                        .map(|v| v.as_bool().ok_or(("trace selector must be boolean", -32602)))
                        .transpose()
                };
                let include_io = boolean("include_io")?.unwrap_or(true);
                let include_irq = boolean("include_irq")?.unwrap_or(true);
                if !include_io && !include_irq {
                    return invalid("enable at least one hardware event class");
                }
                let mut ports = Vec::new();
                if let Some(value) = p.get("ports") {
                    for range in value.as_array().ok_or(("ports array required", -32602))? {
                        let first = number(&range["first"])?;
                        let last = number(&range["last"])?;
                        if first > last || last > 65535 {
                            return invalid("invalid port range");
                        }
                        ports.push((first as u16, last as u16));
                    }
                }
                let mut irqs = Vec::new();
                if let Some(value) = p.get("irqs") {
                    for irq in value.as_array().ok_or(("irqs array required", -32602))? {
                        let irq = number(irq)?;
                        if irq > 15 {
                            return invalid("IRQ outside 0..15");
                        }
                        irqs.push(irq as u8);
                    }
                }
                self.hardware = HardwareTrace {
                    active: true,
                    started: true,
                    next: 1,
                    capacity: capacity as usize,
                    include_io,
                    include_irq,
                    ports,
                    irqs,
                    ..Default::default()
                };
                Ok(self.hardware.meta())
            }
            "hardware.trace.stop" => {
                if !self.hardware.started {
                    return invalid("no hardware trace has been started");
                }
                self.hardware.active = false;
                Ok(self.hardware.meta())
            }
            "hardware.trace.read" => {
                if !self.hardware.started {
                    return invalid("no hardware trace has been started");
                }
                let start = cursor(p, "hardware", self.hardware.next - 1)?
                    .checked_add(1)
                    .ok_or(("cursor overflow", -32602))?;
                let first = self.hardware.next - self.hardware.events.len() as u64;
                let start = if p.get("cursor").is_none_or(Value::is_null) {
                    first
                } else {
                    start
                };
                if start < first {
                    return invalid("hardware cursor expired");
                }
                let end = (start + limit(p)? as u64).min(self.hardware.next);
                let mut out = self.hardware.meta();
                out["events"] = json!(self
                    .hardware
                    .events
                    .iter()
                    .skip((start - first) as usize)
                    .take((end - start) as usize)
                    .collect::<Vec<_>>());
                out["next_cursor"] = json!(if end < self.hardware.next {
                    Some(format!("hardware-{}", end - 1))
                } else {
                    None
                });
                Ok(out)
            }
            _ => unreachable!(),
        }
    }
}
fn cursor(p: &Value, prefix: &str, maximum: u64) -> Result<u64> {
    let Some(value) = p.get("cursor").filter(|v| !v.is_null()) else {
        return Ok(0);
    };
    let n = value
        .as_str()
        .and_then(|s| s.strip_prefix(&format!("{prefix}-")))
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or(("invalid trace cursor", -32602))?;
    if n > maximum {
        return invalid("trace cursor beyond retained events");
    }
    Ok(n)
}
fn limit(p: &Value) -> Result<usize> {
    let n = p.get("limit").map(number).transpose()?.unwrap_or(128);
    if !(1..=65536).contains(&n) {
        return invalid("trace limit outside 1..65536");
    }
    Ok(n as usize)
}

#[cfg(test)]
mod tests;

fn compare(actual: u16, op: &str, value: u16) -> bool {
    match op {
        "eq" => actual == value,
        "ne" => actual != value,
        "lt" => actual < value,
        "le" => actual <= value,
        "gt" => actual > value,
        _ => actual >= value,
    }
}
