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
}

impl BusInterface {
    pub fn debug_port_handled(&self, port: u16) -> bool {
        self.io_map.contains_key(&port)
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
    pub fn debug_end(&mut self) -> (Vec<Event>, usize) {
        self.debug_classes = 0;
        if let Some(pic) = self.pic1.as_mut() {
            pic.debug_journal = None;
        }
        let journal = self.debug_observation.take().unwrap();
        let mut observation = journal.lock().unwrap();
        (std::mem::take(&mut observation.events), observation.dropped)
    }
}

pub(crate) fn record(journal: &Journal, event: Event) {
    let mut observation = journal.lock().unwrap();
    if observation.events.len() < 65536 {
        observation.events.push(event);
    } else {
        observation.dropped += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
