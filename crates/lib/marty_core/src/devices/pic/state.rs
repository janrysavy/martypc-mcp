//! Complete PIC-owned state, including partial initialization and deferred INTR.
//! CPU interrupt bookkeeping and the other devices belong to machine state.
//! This captures existing native behavior, not physical 8259 timing accuracy.

use super::*;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PicState {
    version: u32,
    pic: Pic,
}

impl Pic {
    pub(crate) fn snapshot_state(&self) -> Result<PicState, &'static str> {
        let saved = PicState {
            version: 1,
            pic: self.clone(),
        };
        self.preflight_state(&saved)?;
        Ok(saved)
    }

    pub(crate) fn preflight_state(&self, saved: &PicState) -> Result<(), &'static str> {
        if saved.version != 1
            || saved.pic.interrupt_stats.len() != 8
            || saved.pic.irq >= 8
            || saved.pic.int_offset & !ICW2_MASK != 0
        {
            return Err("invalid PIC state version/IRQ/vector/statistics");
        }
        Ok(())
    }

    pub(crate) fn restore_state(&mut self, saved: &PicState) -> Result<(), &'static str> {
        self.preflight_state(saved)?;
        // No external handles or callbacks are stored by Pic. Keep every field,
        // including diagnostic counters and native partially supported modes.
        *self = saved.pic.clone();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(pic: &mut Pic, port: u16, byte: u8) {
        pic.write_u8(port, byte, None, DeviceRunTimeUnit::SystemTicks(0), None);
    }

    fn observe(pic: &mut Pic) -> (bool, u8, u8, Option<u8>) {
        let line = pic.query_interrupt_line();
        let command = pic.read_u8(PIC_COMMAND_PORT, DeviceRunTimeUnit::SystemTicks(0));
        let mask = pic.read_u8(PIC_DATA_PORT, DeviceRunTimeUnit::SystemTicks(0));
        let vector = if line { pic.get_interrupt_vector() } else { None };
        (line, command, mask, vector)
    }

    #[test]
    fn pic_json_restore_continues_native_initialization_masks_and_irqs() {
        for level in [false, true] {
            for auto_eoi in [false, true] {
                let mut reference = Pic::new();
                let mut restored = Pic::new();
                for n in 0..512 {
                    // Serialize, destroy independently, then restore before the
                    // next native operation. Reference is never restored.
                    let wire = serde_json::to_vec(&restored.snapshot_state().unwrap()).unwrap();
                    restored = Pic::new();
                    let saved: PicState = serde_json::from_slice(&wire).unwrap();
                    restored.restore_state(&saved).unwrap();
                    for pic in [&mut reference, &mut restored] {
                        match n % 32 {
                            0 => write(pic, PIC_COMMAND_PORT, 0x13 | if level { 8 } else { 0 }),
                            1 => write(pic, PIC_DATA_PORT, 0x28), // ICW2 after restore
                            2 => write(pic, PIC_DATA_PORT, 1 | if auto_eoi { 2 } else { 0 }),
                            3 => write(pic, PIC_DATA_PORT, 0xff),
                            4 => pic.request_interrupt(((n / 32) % 8) as u8),
                            5 => write(pic, PIC_DATA_PORT, 0), // schedules native three-tick INTR
                            6 | 7 => pic.run(1),               // captures deferred timer before it expires
                            8 => pic.run(1),
                            9 => write(pic, PIC_COMMAND_PORT, 0x0b), // read ISR
                            10 => pic.request_interrupt((((n / 32) + 1) % 8) as u8),
                            11 => write(pic, PIC_COMMAND_PORT, 0x20), // native EOI
                            12 => pic.clear_interrupt(((n / 32) % 8) as u8),
                            13 => pic.pulse_interrupt((((n / 32) + 2) % 8) as u8),
                            14 => write(pic, PIC_COMMAND_PORT, 0x0a), // read IRR
                            15 => write(pic, PIC_DATA_PORT, 0xff),
                            16 => pic.run(50),
                            17 => write(pic, PIC_DATA_PORT, 0),
                            18 => pic.run(3),
                            19 => write(pic, PIC_COMMAND_PORT, 0x20),
                            _ => pic.run(7),
                        }
                    }
                    assert_eq!(
                        observe(&mut reference),
                        observe(&mut restored),
                        "native ports/vector n={n}"
                    );
                    assert_eq!(reference, restored, "native PIC continuation n={n}");
                }
            }
        }
    }

    #[test]
    fn pic_saved_schema_covers_native_fields_and_requires_each_key() {
        let pic = Pic::new();
        let value = serde_json::to_value(pic.snapshot_state().unwrap()).unwrap();
        let source = include_str!("../pic.rs")
            .split("pub struct Pic {")
            .nth(1)
            .unwrap()
            .split("\n}")
            .next()
            .unwrap();
        let fields = regex::Regex::new(r"(?m)^\s+([a-z_][a-z0-9_]*):").unwrap();
        let native: std::collections::HashSet<_> = fields.captures_iter(source).map(|c| c[1].to_owned()).collect();
        let serialized: std::collections::HashSet<_> = value["pic"].as_object().unwrap().keys().cloned().collect();
        assert_eq!(native, serialized, "PIC native storage must all be serialized");
        for field in serialized {
            let mut missing = value.clone();
            missing["pic"].as_object_mut().unwrap().remove(&field);
            assert!(
                serde_json::from_value::<PicState>(missing).is_err(),
                "missing PIC field {field}"
            );
        }
        for field in ["version", "pic"] {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<PicState>(missing).is_err());
        }
        let stats = value["pic"]["interrupt_stats"][0].as_object().unwrap();
        for field in stats.keys() {
            let mut missing = value.clone();
            missing["pic"]["interrupt_stats"][0]
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(serde_json::from_value::<PicState>(missing).is_err());
        }
        for target in ["top", "pic", "stats"] {
            let mut extra = value.clone();
            let object = match target {
                "pic" => extra["pic"].as_object_mut().unwrap(),
                "stats" => extra["pic"]["interrupt_stats"][0].as_object_mut().unwrap(),
                _ => extra.as_object_mut().unwrap(),
            };
            object.insert("unknown".to_owned(), true.into());
            assert!(serde_json::from_value::<PicState>(extra).is_err());
        }
    }

    #[test]
    fn pic_preflight_refuses_invalid_state_without_mutation() {
        let mut pic = Pic::new();
        pic.request_interrupt(3);
        let saved = pic.snapshot_state().unwrap();
        for kind in 0..4 {
            let mut invalid = saved.clone();
            match kind {
                0 => invalid.version += 1,
                1 => invalid.pic.interrupt_stats.pop().map(|_| ()).unwrap(),
                2 => invalid.pic.irq = 8,
                _ => invalid.pic.int_offset = 9,
            }
            assert!(pic.restore_state(&invalid).is_err());
            assert_eq!(pic.snapshot_state().unwrap(), saved);
        }
    }
}
