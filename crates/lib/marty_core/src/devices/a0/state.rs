//! NMI/system-register owned state. Preserve the raw byte and cached flags
//! independently: native setters can intentionally make them differ. The bus's
//! observed NMI edge and PPI keyboard latch belong to their separate owners.
use super::*;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
enum Model {
    PCXT,
    PCJr,
    Tandy1000,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct A0State {
    version: u32,
    a0type: Model,
    a0_byte: u8,
    kbd_latch: bool,
    nmi_enabled: bool,
    ir_test_ena: bool,
    clock_1_select: bool,
    hrq_disable: bool,
    clear_kbd_latch: bool,
    tandy_aperture_sel: u8,
    tandy_enable_256k: bool,
}

impl A0Register {
    fn snapshot_model(&self) -> Model {
        match self.a0type {
            A0Type::PCXT => Model::PCXT,
            A0Type::PCJr => Model::PCJr,
            A0Type::Tandy1000 => Model::Tandy1000,
        }
    }

    pub(crate) fn snapshot_state(&self) -> Result<A0State, &'static str> {
        let saved = A0State {
            version: 1,
            a0type: self.snapshot_model(),
            a0_byte: self.a0_byte,
            kbd_latch: self.kbd_latch,
            nmi_enabled: self.nmi_enabled,
            ir_test_ena: self.ir_test_ena,
            clock_1_select: self.clock_1_select,
            hrq_disable: self.hrq_disable,
            clear_kbd_latch: self.clear_kbd_latch,
            tandy_aperture_sel: self.tandy_aperture_sel,
            tandy_enable_256k: self.tandy_enable_256k,
        };
        self.preflight_state(&saved)?;
        Ok(saved)
    }

    pub(crate) fn preflight_state(&self, saved: &A0State) -> Result<(), &'static str> {
        if saved.version != 1 || saved.a0type != self.snapshot_model() || saved.tandy_aperture_sel > 7 {
            return Err("incompatible A0 version/model/aperture");
        }
        Ok(())
    }

    pub(crate) fn restore_state(&mut self, saved: &A0State) -> Result<(), &'static str> {
        self.preflight_state(saved)?;
        self.a0_byte = saved.a0_byte;
        self.kbd_latch = saved.kbd_latch;
        self.nmi_enabled = saved.nmi_enabled;
        self.ir_test_ena = saved.ir_test_ena;
        self.clock_1_select = saved.clock_1_select;
        self.hrq_disable = saved.hrq_disable;
        self.clear_kbd_latch = saved.clear_kbd_latch;
        self.tandy_aperture_sel = saved.tandy_aperture_sel;
        self.tandy_enable_256k = saved.tandy_enable_256k;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::pit::PitType;

    #[test]
    fn a0_json_restore_preserves_native_ports_flags_and_deferred_latch_clear() {
        let mut pit = Pit::new(PitType::Model8253, 14.31818, 12, None);
        let mut saw_deferred_clear = false;
        for model in [A0Type::PCXT, A0Type::PCJr, A0Type::Tandy1000] {
            let mut a = A0Register::new(model);
            for byte in 0..=255u8 {
                a.write_u8(0xA0, byte, None, DeviceRunTimeUnit::SystemTicks(0), None);
                a.set_kbd_latch(byte % 2 == 0);
                a.enable_nmi(byte % 3 != 0); // native setter can differ from raw byte
                if byte % 4 == 0 {
                    assert_eq!(a.read_u8(0xA0, DeviceRunTimeUnit::SystemTicks(0)), 0xFF);
                }
                saw_deferred_clear |= a.clear_kbd_latch;
                let saved: A0State =
                    serde_json::from_slice(&serde_json::to_vec(&a.snapshot_state().unwrap()).unwrap()).unwrap();
                let mut b = A0Register::new(model); // destroys every old device field
                b.restore_state(&saved).unwrap();
                assert_eq!(a.read(), b.read(), "native raw byte {model:?}/{byte}");
                assert_eq!(a.is_nmi_enabled(), b.is_nmi_enabled());
                assert_eq!(a.ir_test_ena(), b.ir_test_ena());
                assert_eq!(a.clock_1_select(), b.clock_1_select());
                assert_eq!(a.hrq_disable(), b.hrq_disable());
                assert_eq!(a.tandy_aperture_sel(), b.tandy_aperture_sel());
                assert_eq!(a.tandy_256k_enabled(), b.tandy_256k_enabled());
                assert_eq!(
                    a.run(&mut pit, 0.0),
                    b.run(&mut pit, 0.0),
                    "native latch/NMI continuation {model:?}/{byte}"
                );
                assert_eq!(a.snapshot_state().unwrap(), b.snapshot_state().unwrap());
            }
        }
        assert!(saw_deferred_clear);
        println!("A0_NATIVE: 768 native-port JSON restores, including PCJr deferred latch clears; no Machine/hardware timing claim");
    }

    #[test]
    fn a0_inventory_schema_and_invalid_restores_are_atomic() {
        let mut device = A0Register::new(A0Type::PCXT);
        let saved = device.snapshot_state().unwrap();
        let value = serde_json::to_value(&saved).unwrap();
        let body = include_str!("../a0.rs")
            .split("pub struct A0Register {")
            .nth(1)
            .unwrap()
            .split("\n}")
            .next()
            .unwrap();
        let fields = regex::Regex::new(r"(?m)^\s+([a-z_][a-z0-9_]*):").unwrap();
        let mut native: std::collections::HashSet<String> = fields.captures_iter(body).map(|c| c[1].into()).collect();
        native.insert("version".into());
        assert_eq!(native, value.as_object().unwrap().keys().cloned().collect());
        for key in value.as_object().unwrap().keys() {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(serde_json::from_value::<A0State>(missing).is_err(), "required {key}");
        }
        let mut unknown = value.clone();
        unknown["unexpected"] = serde_json::json!(1);
        assert!(serde_json::from_value::<A0State>(unknown).is_err());
        for n in 0..3 {
            let mut bad = saved.clone();
            match n {
                0 => bad.version += 1,
                1 => bad.a0type = Model::PCJr,
                _ => bad.tandy_aperture_sel = 8,
            }
            assert!(device.restore_state(&bad).is_err());
            assert_eq!(device.snapshot_state().unwrap(), saved);
        }
        println!("A0_REFUSALS: all ten native fields represented; required schema and three atomic refusals");
    }
}
