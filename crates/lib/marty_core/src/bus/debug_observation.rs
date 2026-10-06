//! Transient host observations. Never part of machine continuation state.
//! The RPC owner enables this only around one native Machine::run boundary.
use super::BusInterface;
pub const MEMORY: u8 = 1;
pub const IO: u8 = 2;
pub const INTERRUPT: u8 = 4;
pub const OPCODES: u8 = 8;
pub const OLD_VALUES: u8 = 16;
pub const PIC: u8 = 32;
use std::sync::{Arc, Mutex};
#[derive(Clone, Debug, Default)]
pub struct Journal(Arc<Mutex<Observation>>);
impl PartialEq for Journal {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Journal {
    fn lock(&self) -> std::sync::LockResult<std::sync::MutexGuard<'_, Observation>> {
        self.0.lock()
    }
}

#[derive(Clone, Debug)]
pub struct Event {
    pub kind: &'static str,
    pub clock: u64,
    pub cs: u16,
    pub ip: u16,
    pub address: u32,
    pub width: usize,
    pub value: u16,
    pub old: Option<Vec<u8>>,
    pub interrupt_kind: Option<String>,
    pub ah: u8,
    pub al: u8,
    pub irq: Option<u8>,
    pub handled: Option<bool>,
    pub registers: Option<[u16; 14]>,
}

#[derive(Default, Debug)]
pub struct Observation {
    pub(crate) events: Vec<Event>,
    pub(crate) dropped: usize,
    pub(crate) dropped_effects: usize,
}

impl BusInterface {
    pub fn debug_port_handled(&self, port: u16, width: usize) -> bool {
        (0..width).any(|offset| self.io_map.contains_key(&port.wrapping_add(offset as u16)))
    }
    pub fn debug_class(&self, class: u8) -> bool {
        self.debug_classes & class != 0
    }
    pub fn debug_observing(&self) -> bool {
        self.debug_observation.is_some()
    }
    pub fn debug_begin(&mut self, classes: u8) {
        self.debug_classes = classes;
        let journal = Journal::default();
        if let Some(pic) = self.pic1.as_mut() {
            if classes & PIC != 0 {
                pic.debug_journal = Some(journal.clone());
            }
        }
        self.debug_observation = Some(journal);
    }
    pub fn debug_record(&mut self, event: Event) {
        if let Some(journal) = &self.debug_observation {
            record(journal, event);
        }
    }
    pub fn debug_end(&mut self) -> (Vec<Event>, usize, usize) {
        self.debug_classes = 0;
        if let Some(pic) = self.pic1.as_mut() {
            pic.debug_journal = None;
        }
        let journal = self.debug_observation.take().unwrap();
        let mut observation = journal.lock().unwrap();
        (
            std::mem::take(&mut observation.events),
            observation.dropped,
            observation.dropped_effects,
        )
    }
}

pub(crate) fn record(journal: &Journal, event: Event) {
    let mut observation = journal.lock().unwrap();
    if observation.events.len() < 65536 {
        observation.events.push(event);
    } else {
        observation.dropped += 1;
        if event.kind.starts_with("memory_") || event.kind.starts_with("io_") {
            observation.dropped_effects += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_journal_counts_memory_io_losses_separately_from_opcode_and_pic_losses() {
        let journal = Journal::default();
        let mut event = Event {
            kind: "instruction_byte",
            clock: 0,
            cs: 0,
            ip: 0,
            address: 0,
            width: 1,
            value: 0,
            old: None,
            interrupt_kind: None,
            ah: 0,
            al: 0,
            irq: None,
            handled: None,
            registers: None,
        };
        for _ in 0..65536 {
            record(&journal, event.clone());
        }
        for kind in ["instruction_byte", "irq_raise", "memory_write", "io_read"] {
            event.kind = kind;
            record(&journal, event.clone());
        }
        let state = journal.lock().unwrap();
        assert_eq!(state.events.len(), 65536);
        assert_eq!(state.dropped, 4);
        assert_eq!(state.dropped_effects, 2);
    }
    #[test]
    fn native_word_io_wraps_second_port_without_overflow_or_device_invention() {
        let mut bus = BusInterface::default();
        assert_eq!(bus.io_read_u16(0xffff, 0), 0xffff);
        bus.io_write_u16(0xffff, 0x1234, 0, None);
        assert!(!bus.debug_port_handled(0xffff, 2));
    }
    #[test]
    fn word_io_handled_reports_either_half_including_port_wrap() {
        let mut bus = BusInterface::default();
        bus.io_map.insert(0, super::super::IoDeviceType::PicPrimary);
        assert!(!bus.debug_port_handled(0xffff, 1));
        assert!(bus.debug_port_handled(0xffff, 2));
        assert!(bus.debug_port_handled(0, 2));
        assert!(!bus.debug_port_handled(1, 2));
    }
    #[test]
    fn bounded_shared_pic_journal_preserves_native_lines_and_refuses_active_snapshots() {
        let mut pic = crate::devices::pic::Pic::new();
        let journal = Journal::default();
        pic.debug_journal = Some(journal.clone());
        pic.pulse_interrupt(1);
        pic.request_interrupt(2);
        pic.request_interrupt(2);
        pic.clear_interrupt(2);
        assert!(pic.snapshot_state().is_err());
        let observed = journal.lock().unwrap();
        assert_eq!(
            observed.events.iter().map(|e| (e.kind, e.irq)).collect::<Vec<_>>(),
            vec![
                ("irq_raise", Some(1)),
                ("irq_lower", Some(1)),
                ("irq_raise", Some(2)),
                ("irq_lower", Some(2))
            ]
        );
        drop(observed);
        pic.debug_journal = None;
        assert!(pic.snapshot_state().is_ok());
    }
}
