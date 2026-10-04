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

    cpu_808x::queue.rs

    Implements the data structure for the processor instruction queue.

*/

use crate::cpu_808x::*;

/// Complete queue component of a future machine snapshot. Ordered visible bytes
/// alone lose ring position, stale storage, preload and fetch policy. This is not
/// a CPU/machine snapshot: bus phase, devices, RAM and disks must be saved too.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstructionQueueState {
    version: u32,
    size: usize,
    fetch_size: usize,
    policy_len0: usize,
    policy_len1: usize,
    len: usize,
    back: usize,
    front: usize,
    q: [u8; QUEUE_MAX],
    #[serde(deserialize_with = "required_preload")]
    preload: Option<u8>,
    discard: bool,
}

fn required_preload<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u8>, D::Error> {
    serde::Deserialize::deserialize(d)
}

pub struct InstructionQueue {
    size: usize,
    fetch_size: usize,
    policy_len0: usize,
    policy_len1: usize,
    len: usize,
    back: usize,
    front: usize,
    q: [u8; QUEUE_MAX],
    preload: Option<u8>,
    // Whether to discard the low order byte of the next fetch
    discard: bool,
}

impl Default for InstructionQueue {
    fn default() -> Self {
        Self {
            size: QUEUE_MAX,
            fetch_size: 2,
            policy_len0: QUEUE_MAX - 1,
            policy_len1: QUEUE_MAX - 2,
            len: 0,
            back: 0,
            front: 0,
            q: [0; QUEUE_MAX],
            preload: None,
            discard: false,
        }
    }
}

impl Display for InstructionQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut base_str = "".to_string();

        if let Some(preload) = self.preload {
            base_str.push_str(&format!("{:02X}", preload));
        }

        for i in 0..self.len {
            base_str.push_str(&format!("{:02X}", self.q[(self.back + i) % self.size]));
        }
        write!(f, "{}", base_str)
    }
}

impl InstructionQueue {
    pub(crate) fn snapshot_state(&self) -> InstructionQueueState {
        InstructionQueueState {
            version: 1,
            size: self.size,
            fetch_size: self.fetch_size,
            policy_len0: self.policy_len0,
            policy_len1: self.policy_len1,
            len: self.len,
            back: self.back,
            front: self.front,
            q: self.q,
            preload: self.preload,
            discard: self.discard,
        }
    }

    /// Preflight the entire component before mutation. The enclosing CPU restore
    /// must first establish a compatible CPU/queue configuration. Keep actual
    /// policy values: Default and new(6, 2) currently initialize them differently.
    pub(crate) fn restore_state(&mut self, state: &InstructionQueueState) -> Result<(), &'static str> {
        if state.version != 1 {
            return Err("unsupported instruction queue state version");
        }
        if state.size == 0 || state.size > QUEUE_MAX || state.size != self.size
            || state.fetch_size != self.fetch_size || !matches!(state.fetch_size, 1 | 2)
            || state.fetch_size > state.size {
            return Err("incompatible instruction queue configuration");
        }
        if state.len > state.size || state.back >= state.size || state.front >= state.size
            || state.front != (state.back + state.len) % state.size
            || state.policy_len0 >= state.size || state.policy_len1 >= state.size {
            return Err("invalid instruction queue ring or policy");
        }
        self.policy_len0 = state.policy_len0;
        self.policy_len1 = state.policy_len1;
        self.len = state.len;
        self.back = state.back;
        self.front = state.front;
        self.q = state.q;
        self.preload = state.preload;
        self.discard = state.discard;
        Ok(())
    }
    pub fn new(size: usize, fetch_size: usize) -> Self {
        Self {
            size,
            fetch_size,
            policy_len0: if fetch_size == 1 { size - 1 } else { size - 2 },
            policy_len1: if fetch_size == 1 { size - 1 } else { size - 3 },
            ..Self::default()
        }
    }

    pub fn set_size(&mut self, size: usize, fetch_size: usize) {
        assert!(size <= QUEUE_MAX);
        self.size = size;
        self.fetch_size = fetch_size;
        self.policy_len0 = if fetch_size == 1 { size - 1 } else { size - 2 };
        self.policy_len1 = if fetch_size == 1 { size - 1 } else { size - 3 };
    }

    pub fn size(&self) -> usize {
        self.size
    }

    #[inline]
    pub fn at_policy_len(&self) -> bool {
        self.len == self.policy_len0 || self.len == self.policy_len1
    }

    #[inline]
    pub fn at_policy_threshold(&self) -> bool {
        self.len == self.policy_len1
    }

    #[inline]
    pub fn has_room_for_fetch(&self) -> bool {
        self.len <= (self.size - self.fetch_size)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn len_p(&self) -> usize {
        self.len + if self.preload.is_some() { 1 } else { 0 }
    }

    #[allow(dead_code)]
    #[inline]
    pub fn is_full(&self) -> bool {
        self.len == self.size
    }

    #[inline]
    pub fn get_preload(&mut self) -> Option<u8> {
        let preload = self.preload;
        self.preload = None;
        preload
    }

    #[inline]
    pub fn has_preload(&self) -> bool {
        self.preload.is_some()
    }

    #[inline]
    pub fn set_preload(&mut self) {
        if self.len > 0 {
            let byte = self.pop();
            self.preload = Some(byte);
        }
        else {
            panic!("Tried to preload with empty queue.")
        }
    }

    #[inline]
    pub fn set_discard(&mut self) {
        self.discard = true;
    }

    #[inline]
    pub fn push8(&mut self, byte: u8) -> u16 {
        if self.len < self.size {
            self.q[self.front] = byte;
            self.front = (self.front + 1) % self.size;
            self.len += 1;
            1
        }
        else {
            panic!("Queue overrun!");
        }
    }

    #[inline]
    pub fn push16(&mut self, word: u16, a0: bool) -> u16 {
        assert_eq!(self.fetch_size, 2);

        if a0 {
            self.push8((word >> 8) as u8);
            1
        }
        else {
            self.push8((word & 0xFF) as u8);
            self.push8((word >> 8) as u8);
            2
        }
    }

    #[inline]
    pub fn pop(&mut self) -> u8 {
        if self.len > 0 {
            let byte = self.q[self.back];
            self.back = (self.back + 1) % self.size;
            self.len -= 1;

            return byte;
        }
        panic!("Queue underrun!");
    }

    /// Flush the processor queue. This resets the queue to an empty state
    pub fn flush(&mut self) {
        log::trace!("flushing queue!");
        self.len = 0;
        self.back = 0;
        self.front = 0;
        self.preload = None;
        self.discard = false;
    }

    /// Write the contents of the processor instruction queue in order to the
    /// provided slice of u8. The slice must be the same size as the current piq
    /// length for the given cpu type.
    #[allow(dead_code)]
    pub fn to_slice(&self, slice: &mut [u8]) {
        for i in 0..self.len {
            slice[i] = self.q[(self.back + i) % self.size];
        }
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    fn exercise(q: &mut InstructionQueue, seed: u32) {
        match seed % 5 {
            0 if q.len() > 0 => { q.pop(); }
            1 if q.len() > 0 && !q.has_preload() => q.set_preload(),
            2 => { q.get_preload(); }
            3 => q.set_discard(),
            _ if q.has_room_for_fetch() => {
                if q.fetch_size == 2 { q.push16(seed as u16, seed & 0x80 != 0); }
                else { q.push8(seed as u8); }
            }
            _ => {}
        }
    }

    #[test]
    fn snapshot_json_preserves_wrapping_queue_and_continuation() {
        for (size, fetch) in [(4, 1), (6, 2)] {
            let mut original = InstructionQueue::new(size, fetch);
            let mut seed = 0x12345678_u32;
            for n in 0..2000 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                exercise(&mut original, seed);
                let saved = original.snapshot_state();
                let json = serde_json::to_vec(&saved).unwrap();
                let decoded = serde_json::from_slice(&json).unwrap();
                let mut restored = InstructionQueue::new(size, fetch);
                restored.restore_state(&decoded).unwrap();
                assert_eq!(saved, original.snapshot_state()); // export must not flush/mutate
                assert_eq!(saved, restored.snapshot_state());
                exercise(&mut original, seed.rotate_left(13));
                exercise(&mut restored, seed.rotate_left(13));
                assert_eq!(original.snapshot_state(), restored.snapshot_state());
                assert_eq!(original.to_string(), restored.to_string());
                if n % 37 == 0 {
                    original.flush(); restored.flush();
                    assert_eq!(original.snapshot_state(), restored.snapshot_state());
                }
            }
        }
    }

    #[test]
    fn snapshot_retains_preload_stale_bytes_and_actual_default_policy() {
        let mut original = InstructionQueue::default();
        original.push16(0x3412, false);
        original.push16(0x7856, false);
        original.push16(0xab90, true); // odd fetch contributes only the high byte
        original.set_preload();
        original.set_discard();
        let saved = original.snapshot_state();
        assert_eq!(saved.preload, Some(0x12));
        assert!(saved.discard);
        assert_eq!(saved.q, [0x12, 0x34, 0x56, 0x78, 0xab, 0]);
        let mut restored = InstructionQueue::new(6, 2);
        assert_ne!(restored.policy_len0, saved.policy_len0);
        restored.restore_state(&saved).unwrap();
        assert_eq!(saved, restored.snapshot_state());
        assert_eq!(original.get_preload(), restored.get_preload());
        while original.len() > 0 { assert_eq!(original.pop(), restored.pop()); }
        original.flush(); restored.flush();
        assert_eq!(original.snapshot_state(), restored.snapshot_state());
        // Flush leaves physical storage intact; a bytes-only export loses it.
        assert_ne!(restored.snapshot_state().q, [0; QUEUE_MAX]);
    }

    #[test]
    fn snapshot_rejects_invalid_state_before_mutation() {
        let mut target = InstructionQueue::new(4, 1);
        target.push8(0x90);
        let before = target.snapshot_state();
        for field in ["version", "size", "fetch_size", "len", "back", "front", "policy_len0", "policy_len1"] {
            let mut value = serde_json::to_value(&before).unwrap();
            value[field] = serde_json::json!(if field == "version" { 2 } else { usize::MAX });
            let invalid = serde_json::from_value(value).unwrap();
            assert!(target.restore_state(&invalid).is_err(), "{field}");
            assert_eq!(before, target.snapshot_state());
        }
        let incompatible = InstructionQueue::new(6, 2).snapshot_state();
        assert!(target.restore_state(&incompatible).is_err());
        assert_eq!(before, target.snapshot_state());
    }

    #[test]
    fn snapshot_refuses_unknown_and_missing_fields() {
        let valid = serde_json::to_value(InstructionQueue::default().snapshot_state()).unwrap();
        for field in valid.as_object().unwrap().keys() {
            let mut missing = valid.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<InstructionQueueState>(missing).is_err(), "{field}");
        }
        let mut future = valid;
        future["future_field"] = serde_json::json!(true);
        assert!(serde_json::from_value::<InstructionQueueState>(future).is_err());
    }
}
