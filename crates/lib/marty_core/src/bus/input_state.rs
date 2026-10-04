//! Keyboard-owned state and its bus update clock only. PPI/PIC/A0, CPU,
//! other bus clocks/devices and hardware delivery peers remain separate owners.
//! Preserve native strict >5000us polling, not an invented physical model.

use super::*;
use crate::devices::keyboard_common::KeyboardSnapshot;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KeyboardBusState {
    version: u32,
    keyboard_type: KeyboardType,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    keyboard: Option<KeyboardSnapshot>,
    #[serde(with = "crate::snapshot_codec::f64_bits")]
    kb_us_accum: f64,
}

impl BusInterface {
    pub(crate) fn snapshot_keyboard_bus_state(&self) -> Result<KeyboardBusState, &'static str> {
        let saved = KeyboardBusState {
            version: 1,
            keyboard_type: self.keyboard_type,
            keyboard: self.keyboard.as_ref().map(Keyboard::snapshot_state).transpose()?,
            kb_us_accum: self.kb_us_accum,
        };
        self.preflight_keyboard_bus_state(&saved)?;
        Ok(saved)
    }

    pub(crate) fn preflight_keyboard_bus_state(&self, saved: &KeyboardBusState) -> Result<(), &'static str> {
        if saved.version != 1 || saved.keyboard_type != self.keyboard_type || !saved.kb_us_accum.is_finite() {
            return Err("incompatible keyboard bus version/type/clock");
        }
        match (&self.keyboard, &saved.keyboard) {
            (Some(live), Some(saved)) => live.preflight_state(saved),
            (None, None) => Ok(()),
            _ => Err("keyboard presence/configuration mismatch"),
        }
    }

    pub(crate) fn restore_keyboard_bus_state(&mut self, saved: &KeyboardBusState) -> Result<(), &'static str> {
        self.preflight_keyboard_bus_state(saved)?;
        if let (Some(live), Some(saved)) = (&mut self.keyboard, &saved.keyboard) {
            live.restore_state(saved)?;
        }
        self.kb_us_accum = saved.kb_us_accum;
        Ok(())
    }

    // Independent destruction for native continuation tests: never call a
    // snapshot restore to reset the very fields its omissions must detect.
    #[cfg(test)]
    pub(crate) fn destroy_keyboard_component_for_test(&mut self) {
        self.kb_us_accum = 0.0;
        if let Some(live) = &mut self.keyboard {
            *live = Keyboard::new(live.get_type(), false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_bus_preserves_no_device_exact_clock_and_atomic_refusals() {
        let mut bus = BusInterface::default();
        for bits in [
            0,
            1,
            0x8000000000000000,
            0x8000000000000001,
            0x3ff0000000000001,
            5000.0f64.to_bits(),
            5000.0f64.to_bits() + 1,
            0x7fefffffffffffff,
        ] {
            bus.kb_us_accum = f64::from_bits(bits);
            let saved = bus.snapshot_keyboard_bus_state().unwrap();
            let wire = serde_json::to_vec(&saved).unwrap();
            let mut target = BusInterface::default();
            target
                .restore_keyboard_bus_state(&serde_json::from_slice(&wire).unwrap())
                .unwrap();
            assert_eq!(target.kb_us_accum.to_bits(), bits);
            assert!(target.keyboard.is_none());
        } //eight seeded storage-only restores, no scheduler/hardware claim
        let value = serde_json::to_value(bus.snapshot_keyboard_bus_state().unwrap()).unwrap();
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove("keyboard");
        assert!(serde_json::from_value::<KeyboardBusState>(missing).is_err());
        for n in 0..5 {
            let mut invalid = bus.snapshot_keyboard_bus_state().unwrap();
            match n {
                0 => invalid.version += 1,
                1 => invalid.keyboard_type = KeyboardType::ModelM,
                2 => invalid.kb_us_accum = f64::NAN,
                3 => invalid.kb_us_accum = f64::INFINITY,
                _ => invalid.keyboard = Some(Keyboard::default().snapshot_state().unwrap()),
            }
            assert!(bus.restore_keyboard_bus_state(&invalid).is_err());
            assert_eq!(
                serde_json::to_value(bus.snapshot_keyboard_bus_state().unwrap()).unwrap(),
                value
            );
        }
    }
}
