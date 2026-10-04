//! Native PPI-owned latches, dirty flags, keyboard reset clocks and PCjr serial
//! phase. Exact clock bits/raw control byte survive JSON. External PIC/PIT,
//! cassette, keyboard queue and other machine state must be restored separately.
//! This does not establish physical keyboard/8255 timing or complete restart.

use super::*;

pub(crate) mod control_word_bits {
    use super::PpiControlWord;
    pub fn serialize<S: serde::Serializer>(word: &PpiControlWord, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(word.into_bytes()[0])
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<PpiControlWord, D::Error> {
        let byte = serde::Deserialize::deserialize(d)?;
        Ok(PpiControlWord::from_bytes([byte]))
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PpiState {
    version: u32,
    ppi: Ppi,
}

impl Ppi {
    pub(crate) fn snapshot_state(&self) -> Result<PpiState, &'static str> {
        let saved = PpiState {
            version: 1,
            ppi: self.clone(),
        };
        self.preflight_state(&saved)?;
        Ok(saved)
    }

    pub(crate) fn preflight_state(&self, saved: &PpiState) -> Result<(), &'static str> {
        let ppi = &saved.ppi;
        let serial = &ppi.kb_serializer;
        if saved.version != 1 || ppi.machine_type != self.machine_type {
            return Err("incompatible PPI version/machine type");
        }
        if [
            ppi.kb_low_count,
            ppi.kb_count_until_reset_byte,
            serial.us_accum,
            serial.rate,
        ]
        .iter()
        .any(|v| !v.is_finite())
        {
            return Err("nonfinite PPI keyboard clock");
        }
        if matches!(serial.state, KbSerializeState::DataBit(bit) if !bit.is_power_of_two())
            || (matches!(serial.state, KbSerializeState::Idle) != serial.data.is_none())
        {
            return Err("invalid PPI serializer bit/data");
        }
        // Preserve native clocks/counters and cached modes as stored, rather
        // than deriving them from the control byte or protocol assumptions.
        Ok(())
    }

    pub(crate) fn restore_state(&mut self, saved: &PpiState) -> Result<(), &'static str> {
        self.preflight_state(saved)?;
        // No external handle is owned here. Do not regenerate interrupts or
        // cassette/PIT side effects: machine restore must cover those peers.
        *self = saved.ppi.clone();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODELS: [MachineType; 9] = [
        MachineType::Ibm5150v64K,
        MachineType::Ibm5150v256K,
        MachineType::Ibm5160,
        MachineType::IbmPCJr,
        MachineType::Tandy1000,
        MachineType::Tandy1000SL,
        MachineType::Tandy1000HX,
        MachineType::CompaqPortable,
        MachineType::CompaqDeskpro,
    ];

    fn ppi(model: MachineType) -> Ppi {
        let memory = if model == MachineType::Ibm5150v64K {
            0x10000
        } else {
            0xa0000
        };
        Ppi::new(model, memory, false, vec![VideoType::CGA], 2)
    }

    fn pic() -> pic::Pic {
        let mut pic = pic::Pic::new();
        pic.handle_command_register_write(0x13);
        pic.handle_data_register_write(8);
        pic.handle_data_register_write(1);
        pic.handle_data_register_write(0);
        pic.request_interrupt(1); // controlled peer input; PPI can withdraw it
        pic
    }

    fn write(ppi: &mut Ppi, port: u16, byte: u8) {
        ppi.write_u8(port, byte, None, DeviceRunTimeUnit::SystemTicks(0), None);
    }

    fn operation(ppi: &mut Ppi, pic: &mut pic::Pic, n: usize) -> (u8, u8, u8, bool, bool) {
        if n % 17 == 0 {
            ppi.set_pit_output_bit(true);
            ppi.set_speaker_bit(true);
            ppi.set_cassette_input_bit(true);
            ppi.set_nmi_latch_bit(true);
        } else if n % 17 == 7 {
            ppi.set_pit_output_bit(false);
            ppi.set_speaker_bit(false);
            ppi.set_cassette_input_bit(false);
            ppi.set_nmi_latch_bit(false);
        }
        match n % 64 {
            0 => write(ppi, PPI_COMMAND_PORT, 0x99),
            1 | 16 => write(ppi, PPI_PORT_B, 0x48),
            2 => ppi.send_keyboard(0x1c + (n / 64) as u8),
            3 => write(ppi, PPI_PORT_B, 0xc8), // leave keyboard clear pending
            4 => ppi.run(pic, 0.25),
            5 => write(ppi, PPI_PORT_B, 0), // pull clock low
            6..=15 => ppi.run(pic, 1001.125),
            17 | 18 => ppi.run(pic, 500.125), // restore inside reset-byte delay
            _ => ppi.run(pic, PCJR_US_PER_HALFBIT * 0.75),
        }
        if n % 27 == 0 {
            let _ = ppi.get_display_state(true); // native dirty-marker cleaning
        }
        (
            ppi.read_u8(PPI_PORT_A, DeviceRunTimeUnit::SystemTicks(0)),
            ppi.read_u8(PPI_PORT_B, DeviceRunTimeUnit::SystemTicks(0)),
            ppi.read_u8(PPI_PORT_C, DeviceRunTimeUnit::SystemTicks(0)),
            ppi.kb_enabled(),
            ppi.get_pit_channel2_gate(),
        )
    }

    #[test]
    fn ppi_json_restore_continues_native_keyboard_reset_and_serial_phase() {
        for model in MODELS {
            let mut reference = ppi(model);
            let mut restored = ppi(model);
            let mut pic_a = pic();
            let mut pic_b = pic();
            for n in 0..256 {
                let wire = serde_json::to_vec(&restored.snapshot_state().unwrap()).unwrap();
                restored = ppi(model); // independent destruction, no restore helper
                let saved: PpiState = serde_json::from_slice(&wire).unwrap();
                restored.restore_state(&saved).unwrap();
                // Read the restored keyboard latch before a later PB7 write
                // can suppress/clear it and conceal a lost pending byte.
                assert_eq!(
                    reference.read_u8(PPI_PORT_A, DeviceRunTimeUnit::SystemTicks(0)),
                    restored.read_u8(PPI_PORT_A, DeviceRunTimeUnit::SystemTicks(0)),
                    "restored keyboard latch n={n} model={model:?}"
                );
                assert_eq!(
                    operation(&mut reference, &mut pic_a, n),
                    operation(&mut restored, &mut pic_b, n),
                    "native PPI ports n={n} model={model:?}"
                );
                assert_eq!(pic_a, pic_b, "native PPI IRQ effects n={n} model={model:?}");
                // Encoded IEEE bits distinguish +0/-0 too; float equality alone
                // cannot demonstrate lossless clock storage.
                assert_eq!(
                    serde_json::to_value(reference.snapshot_state().unwrap()).unwrap(),
                    serde_json::to_value(restored.snapshot_state().unwrap()).unwrap(),
                    "native PPI continuation n={n} model={model:?}"
                );
            }
        }
    }

    #[test]
    fn ppi_raw_control_bytes_and_exact_clock_bits_roundtrip() {
        let mut reference = ppi(MachineType::Ibm5160);
        for byte in 0..=255 {
            write(&mut reference, PPI_COMMAND_PORT, byte);
            reference.kb_low_count = -0.0;
            reference.kb_count_until_reset_byte = f64::from_bits(0x3ff0000000000001);
            reference.kb_serializer.us_accum = f64::from_bits(1);
            reference.kb_serializer.rate = f64::from_bits(0x4092c00000000001);
            let value = serde_json::to_value(reference.snapshot_state().unwrap()).unwrap();
            assert_eq!(value["ppi"]["control_word"], serde_json::json!(byte));
            let decoded: PpiState = serde_json::from_value(value.clone()).unwrap();
            let mut restored = ppi(MachineType::Ibm5160);
            restored.restore_state(&decoded).unwrap();
            assert_eq!(serde_json::to_value(restored.snapshot_state().unwrap()).unwrap(), value);
        } // storage-only seeded clock bits, not natural runtime clock evidence
    }

    #[test]
    fn ppi_schema_covers_native_storage_and_requires_nested_keys() {
        let ppi = ppi(MachineType::IbmPCJr);
        let value = serde_json::to_value(ppi.snapshot_state().unwrap()).unwrap();
        let source = include_str!("../ppi.rs");
        let fields = regex::Regex::new(r"(?m)^\s+([a-z_][a-z0-9_]*):").unwrap();
        for (name, object) in [
            ("Ppi", value["ppi"].as_object().unwrap()),
            ("KbSerializer", value["ppi"]["kb_serializer"].as_object().unwrap()),
        ] {
            let body = source
                .split(&format!("pub struct {name} {{"))
                .nth(1)
                .unwrap()
                .split("\n}")
                .next()
                .unwrap();
            let native: std::collections::HashSet<_> = fields.captures_iter(body).map(|c| c[1].to_owned()).collect();
            assert_eq!(native, object.keys().cloned().collect());
            for field in object.keys() {
                let mut missing = value.clone();
                let object = if name == "Ppi" {
                    &mut missing["ppi"]
                } else {
                    &mut missing["ppi"]["kb_serializer"]
                };
                object.as_object_mut().unwrap().remove(field);
                assert!(
                    serde_json::from_value::<PpiState>(missing).is_err(),
                    "missing {name}.{field}"
                );
            }
        }
        for field in ["version", "ppi"] {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<PpiState>(missing).is_err());
        }
        for depth in ["top", "ppi", "serial", "dirty"] {
            let mut extra = value.clone();
            let object = match depth {
                "ppi" => &mut extra["ppi"],
                "serial" => &mut extra["ppi"]["kb_serializer"],
                "dirty" => &mut extra["ppi"]["kb_byte"],
                _ => &mut extra,
            };
            object
                .as_object_mut()
                .unwrap()
                .insert("unknown".to_owned(), true.into());
            assert!(serde_json::from_value::<PpiState>(extra).is_err());
        }
        for field in ["val", "dirty"] {
            let mut missing = value.clone();
            missing["ppi"]["kb_byte"].as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<PpiState>(missing).is_err());
        }
    }

    #[test]
    fn ppi_preflight_refuses_invalid_clock_serializer_and_model_atomically() {
        let mut ppi = ppi(MachineType::Ibm5160);
        ppi.send_keyboard(0x57);
        let saved = ppi.snapshot_state().unwrap();
        for kind in 0..13 {
            let mut invalid = saved.clone();
            match kind {
                0 => invalid.version += 1,
                1 => invalid.ppi.machine_type = MachineType::IbmPCJr,
                2 => invalid.ppi.kb_low_count = f64::NAN,
                3 => invalid.ppi.kb_count_until_reset_byte = f64::INFINITY,
                4 => invalid.ppi.kb_serializer.us_accum = f64::NAN,
                5 => invalid.ppi.kb_serializer.rate = f64::INFINITY,
                6 => invalid.ppi.kb_serializer.state = KbSerializeState::DataBit(0),
                7 => invalid.ppi.kb_serializer.state = KbSerializeState::DataBit(3),
                8 => {
                    invalid.ppi.kb_serializer.state = KbSerializeState::ParityBit;
                    invalid.ppi.kb_serializer.data = None;
                }
                9 | 10 | 12 => {
                    invalid.ppi.kb_serializer.state = match kind {
                        9 => KbSerializeState::StartBit,
                        10 => KbSerializeState::StopBit,
                        _ => KbSerializeState::DataBit(1),
                    };
                    invalid.ppi.kb_serializer.data = None;
                }
                _ => {
                    invalid.ppi.kb_serializer.state = KbSerializeState::Idle;
                    invalid.ppi.kb_serializer.data = Some(0x57);
                }
            }
            assert!(ppi.restore_state(&invalid).is_err());
            assert_eq!(ppi.snapshot_state().unwrap(), saved);
        }
        let mut invalid = serde_json::to_value(&saved).unwrap();
        invalid["ppi"]["kb_low_count"] = f64::NAN.to_bits().into();
        assert!(serde_json::from_value::<PpiState>(invalid).is_err());
    }
}
