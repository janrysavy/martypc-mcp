//! Shared bus memory only. Video/EMS/expansion memory belongs to its owning
//! device; this component must never be presented as a complete RAM or machine
//! snapshot. The fixed ROM/MMIO layout is checked before any live mutation.

use super::*;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MemoryState {
    version: u32,
    memory: Vec<u8>,
    memory_mask: Vec<u8>,
    cursor: usize,
    descriptors: Vec<MemRangeDescriptor>,
    conventional_size: usize,
    open_bus_byte: u8,
    mmio_map: Vec<(MemRangeDescriptor, MmioDeviceType)>,
    mmio_map_fast: Vec<MmioMapEntry>,
    first_map: usize,
    last_map: usize,
}

impl BusInterface {
    pub(crate) fn snapshot_memory_state(&self) -> MemoryState {
        MemoryState {
            version: 1,
            memory: self.memory.clone(),
            memory_mask: self.memory_mask.clone(),
            cursor: self.cursor,
            descriptors: self.desc_vec.clone(),
            conventional_size: self.conventional_size,
            open_bus_byte: self.open_bus_byte,
            mmio_map: self.mmio_map.clone(),
            mmio_map_fast: self.mmio_map_fast.to_vec(),
            first_map: self.mmio_data.first_map,
            last_map: self.mmio_data.last_map,
        }
    }

    pub(crate) fn preflight_memory_state(&self, saved: &MemoryState) -> Result<(), &'static str> {
        if saved.version != 1 || saved.memory.len() != ADDRESS_SPACE || saved.memory_mask.len() != ADDRESS_SPACE {
            return Err("incompatible shared memory version/extent");
        }
        if self.memory.len() != ADDRESS_SPACE
            || self.memory_mask.len() != ADDRESS_SPACE
            || saved.conventional_size != self.conventional_size
            || saved.open_bus_byte != self.open_bus_byte
            || saved.mmio_map != self.mmio_map
            || saved.mmio_map_fast != self.mmio_map_fast
            || saved.first_map != self.mmio_data.first_map
            || saved.last_map != self.mmio_data.last_map
        {
            return Err("shared memory configuration/layout mismatch");
        }
        // Native seek() accepts any usize, including the one-past-end sentinel.
        // Such cursors return FF without advancing, so preserve them exactly.
        if saved
            .memory_mask
            .iter()
            .zip(&self.memory_mask)
            .any(|(saved, live)| saved & (MEM_ROM_BIT | MEM_MMIO_BIT) != live & (MEM_ROM_BIT | MEM_MMIO_BIT))
        {
            return Err("shared memory ROM/MMIO protection mismatch");
        }
        if saved
            .descriptors
            .iter()
            .any(|d| d.address.checked_add(d.size).is_none_or(|end| end > ADDRESS_SPACE))
        {
            return Err("shared memory descriptor outside address space");
        }
        Ok(())
    }

    pub(crate) fn restore_memory_state(&mut self, saved: &MemoryState) -> Result<(), &'static str> {
        self.preflight_memory_state(saved)?;
        // No fallible operation follows preflight. Machine restoration must
        // preflight all other device/disk dependencies before calling this.
        self.memory.clone_from(&saved.memory);
        self.memory_mask.clone_from(&saved.memory_mask);
        self.cursor = saved.cursor;
        self.desc_vec.clone_from(&saved.descriptors);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bus() -> BusInterface {
        let mut bus = BusInterface::default();
        bus.copy_from(&[0xB8, 0x34, 0x12, 0x90], 0x100, 0, false).unwrap();
        bus.copy_from(&[0xEA, 0xFF, 0, 0xF0], 0xF0000, 0, true).unwrap();
        bus.register_map(
            MmioDeviceType::Cga,
            MemRangeDescriptor::new(0xB8000, MMIO_MAP_SIZE, false),
        );
        bus
    }

    fn destroy_memory(bus: &mut BusInterface) {
        bus.memory.fill(0xCC);
        for mask in &mut bus.memory_mask {
            *mask &= MEM_ROM_BIT | MEM_MMIO_BIT;
        }
        bus.cursor = 0xFFFFF;
        bus.desc_vec.clear();
    }

    #[test]
    fn shared_memory_restore_continues_native_reads_writes_and_decode_cursor() {
        let mut reference = bus();
        let mut restored = bus();
        for i in 0..8 {
            let address = 0x400 + i;
            reference.write_u8(address, (i * 19) as u8, 0).unwrap();
            restored.write_u8(address, (i * 19) as u8, 0).unwrap();
            reference.set_flags(address, MEM_RET_BIT | MEM_BPE_BIT | MEM_SW_BIT);
            restored.set_flags(address, MEM_RET_BIT | MEM_BPE_BIT | MEM_SW_BIT);
            reference.seek(0x100 + i % 4);
            restored.seek(0x100 + i % 4);
            let saved = reference.snapshot_memory_state();
            let encoded = serde_json::to_vec(&saved).unwrap();
            let decoded: MemoryState = serde_json::from_slice(&encoded).unwrap();
            destroy_memory(&mut restored);
            assert_ne!(restored.memory[address], saved.memory[address]);
            restored.restore_memory_state(&decoded).unwrap();
            assert!(restored.snapshot_memory_state() == saved);
            assert_eq!(
                reference.read_u8(address, 0).unwrap(),
                restored.read_u8(address, 0).unwrap()
            );
            assert_eq!(
                reference.q_read_u8(QueueType::First, QueueReader::Eu),
                restored.q_read_u8(QueueType::First, QueueReader::Eu)
            );
            reference.write_u8(0xF0000, 0, 0).unwrap();
            restored.write_u8(0xF0000, 0, 0).unwrap();
            assert_eq!(restored.read_u8(0xF0000, 0).unwrap().0, 0xEA);
            assert!(reference.snapshot_memory_state() == restored.snapshot_memory_state());
        }
        // EOF/out-of-range native cursor is safe and is state too.
        reference.seek(ADDRESS_SPACE + 1);
        restored
            .restore_memory_state(&reference.snapshot_memory_state())
            .unwrap();
        assert_eq!(restored.q_read_u8(QueueType::First, QueueReader::Eu), 0xFF);
        assert_eq!(restored.tell(), ADDRESS_SPACE + 1);
    }

    #[test]
    fn shared_memory_preflight_refuses_layout_and_truncated_state_atomically() {
        let mut live = bus();
        let before = live.snapshot_memory_state();
        for field in [
            "version",
            "memory",
            "memory_mask",
            "mask_length",
            "conventional_size",
            "open_bus_byte",
            "mmio_map",
            "mmio_map_fast",
            "first_map",
            "last_map",
            "descriptors",
        ] {
            let mut invalid = before.clone();
            match field {
                "version" => invalid.version += 1,
                "memory" => {
                    invalid.memory.pop();
                }
                "memory_mask" => invalid.memory_mask[0] ^= MEM_ROM_BIT,
                "mask_length" => {
                    invalid.memory_mask.pop();
                }
                "conventional_size" => invalid.conventional_size += 1,
                "open_bus_byte" => invalid.open_bus_byte ^= 1,
                "mmio_map" => invalid.mmio_map.clear(),
                "mmio_map_fast" => invalid.mmio_map_fast[0].priority ^= 1,
                "first_map" => invalid.first_map ^= 1,
                "last_map" => invalid.last_map ^= 1,
                "descriptors" => invalid.descriptors[0].size = usize::MAX,
                _ => unreachable!(),
            }
            invalid.memory[0x100] ^= 0xFF;
            assert!(live.restore_memory_state(&invalid).is_err(), "{field}");
            assert!(
                live.snapshot_memory_state() == before,
                "preflight changed live state: {field}"
            );
        }
        let value = serde_json::to_value(before).unwrap();
        for field in value.as_object().unwrap().keys() {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<MemoryState>(missing).is_err(), "{field}");
        }
        let mut unknown = value;
        unknown["future_field"] = serde_json::json!(0);
        assert!(serde_json::from_value::<MemoryState>(unknown).is_err());
    }
}
