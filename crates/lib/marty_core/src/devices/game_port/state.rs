//! Native game-port positions, buttons and in-flight capacitor timers. This is
//! an internal component, not a machine snapshot or proof of physical timing.
//! Restore preserves IEEE bits; it must not retrigger one-shots or sample host
//! input. Port/layout compatibility is preflighted before any live mutation.

use super::*;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GamePortSnapshot {
    version: u32,
    port: GamePort,
}

impl GamePort {
    pub(crate) fn snapshot_state(&self) -> Result<GamePortSnapshot, &'static str> {
        let saved = GamePortSnapshot {
            version: 1,
            port: self.clone(),
        };
        self.preflight_state(&saved)?;
        Ok(saved)
    }

    pub(crate) fn preflight_state(&self, saved: &GamePortSnapshot) -> Result<(), &'static str> {
        if saved.version != 1 || saved.port.port_base != self.port_base || saved.port.layout != self.layout {
            return Err("incompatible game-port version/port/layout");
        }
        for stick in &saved.port.sticks {
            for axis in [&stick.x, &stick.y] {
                if !axis.pos.is_finite() || !(-1.0..=1.0).contains(&axis.pos) || !axis.time.is_finite() {
                    return Err("invalid game-port axis position/time");
                }
            }
        }
        // Preserve stored elapsed clocks and timing flags. Do not infer them
        // from the current position: position can change while timing is active.
        Ok(())
    }

    pub(crate) fn restore_state(&mut self, saved: &GamePortSnapshot) -> Result<(), &'static str> {
        self.preflight_state(saved)?;
        *self = saved.port.clone();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAYOUTS: [ControllerLayout; 2] = [
        ControllerLayout::TwoJoysticksTwoButtons,
        ControllerLayout::OneJoystickFourButtons,
    ];

    fn operation(port: &mut GamePort, n: usize) -> (u8, u8) {
        if n % 16 == 0 || n % 16 == 3 {
            let pos = ((n % 17) as f64 - 8.0) / 8.0;
            port.set_stick_pos(0, 0, Some(pos), Some(-pos));
            port.set_stick_pos(1, 0, Some(-pos), Some(pos * 0.5));
            for button in 0..4 {
                let (controller, index) = match port.layout {
                    ControllerLayout::TwoJoysticksTwoButtons => (button / 2, button % 2),
                    ControllerLayout::OneJoystickFourButtons => (0, button),
                };
                port.set_button(controller, index, (n / 16 + button) % 2 == 0);
            }
        }
        if n % 16 == 1 {
            port.write_u8(port.port_base, 0x5a, None, DeviceRunTimeUnit::SystemTicks(0), None);
        } else {
            port.run((n % 5 + 1) as f64 * 90.0625);
        }
        (
            port.read_u8(port.port_base, DeviceRunTimeUnit::SystemTicks(0)),
            port.read_u8(port.port_base ^ 1, DeviceRunTimeUnit::SystemTicks(0)),
        )
    }

    #[test]
    fn game_port_json_restore_continues_native_timers_buttons_and_io() {
        for layout in LAYOUTS {
            for base in [GAMEPORT_DEFAULT_PORT, 0x207] {
                let mut reference = GamePort::new(Some(base), Some(layout));
                let mut restored = GamePort::new(Some(base), Some(layout));
                let mut active = false;
                let mut completed = false;
                for n in 0..256 {
                    let wire = serde_json::to_vec(&restored.snapshot_state().unwrap()).unwrap();
                    restored = GamePort::new(Some(base), Some(layout)); // independent destruction
                    restored.restore_state(&serde_json::from_slice(&wire).unwrap()).unwrap();
                    assert_eq!(
                        reference.port_read(),
                        restored.port_read(),
                        "restored I/O layout={layout:?} n={n}"
                    );
                    assert_eq!(
                        operation(&mut reference, n),
                        operation(&mut restored, n),
                        "native I/O layout={layout:?} n={n}"
                    );
                    assert_eq!(
                        serde_json::to_value(reference.snapshot_state().unwrap()).unwrap(),
                        serde_json::to_value(restored.snapshot_state().unwrap()).unwrap(),
                        "native continuation layout={layout:?} n={n}"
                    );
                    active |= reference.sticks.iter().any(|s| s.x.timing || s.y.timing);
                    completed |= n > 1 && reference.sticks.iter().any(|s| !s.x.timing || !s.y.timing);
                }
                assert!(active && completed, "must visit in-flight and completed charge timers");
            }
        }
    }

    #[test]
    fn game_port_restored_charge_deadline_matches_port_reads() {
        for layout in LAYOUTS {
            // These prefixes stop just before the native model's charge
            // deadlines. They do not establish physical analog calibration.
            for (pos, prefix) in [
                (-1.0, 25.0625),
                (-0.5, 300.0625),
                (0.0, 575.0625),
                (0.5, 850.0625),
                (1.0, 1125.0625),
            ] {
                let mut reference = GamePort::new(None, Some(layout));
                reference.set_stick_pos(0, 0, Some(pos), Some(-pos));
                reference.set_button(0, 0, true);
                reference.write_u8(GAMEPORT_DEFAULT_PORT, 0, None, DeviceRunTimeUnit::SystemTicks(0), None);
                reference.run(prefix);
                let before = reference.read_u8(GAMEPORT_DEFAULT_PORT, DeviceRunTimeUnit::SystemTicks(0));
                assert_ne!(before & STICK1_X, 0, "native charge still active pos={pos}");
                let wire = serde_json::to_vec(&reference.snapshot_state().unwrap()).unwrap();
                let mut restored = GamePort::new(None, Some(layout));
                restored.restore_state(&serde_json::from_slice(&wire).unwrap()).unwrap();
                assert_eq!(
                    restored.read_u8(GAMEPORT_DEFAULT_PORT, DeviceRunTimeUnit::SystemTicks(0)),
                    before
                );
                reference.run(0.25);
                restored.run(0.25);
                let after = reference.read_u8(GAMEPORT_DEFAULT_PORT, DeviceRunTimeUnit::SystemTicks(0));
                assert_eq!(after & STICK1_X, 0, "native charge completed pos={pos}");
                // Test the guest-visible deadline before any serialized-state
                // comparison can reject a lost elapsed clock merely as storage.
                assert_eq!(
                    restored.read_u8(GAMEPORT_DEFAULT_PORT, DeviceRunTimeUnit::SystemTicks(0)),
                    after,
                    "restored charge deadline pos={pos}"
                );
            }
        }
    }

    #[test]
    fn game_port_exact_axis_bits_roundtrip_without_retrigger() {
        for n in 0..64 {
            let mut port = GamePort::new(None, Some(LAYOUTS[n % 2]));
            for (i, stick) in port.sticks.iter_mut().enumerate() {
                for (j, axis) in [&mut stick.x, &mut stick.y].into_iter().enumerate() {
                    let bits = [0, 1, 0x3fe0000000000001, 0x8000000000000000][(n + i + j) % 4];
                    axis.pos = f64::from_bits(bits);
                    axis.time = f64::from_bits([0, 1, 0x3ff0000000000001, 0x8000000000000000][(n + i + j + 1) % 4]);
                    axis.timing = n % 2 == 0;
                }
            }
            let value = serde_json::to_value(port.snapshot_state().unwrap()).unwrap();
            let saved = serde_json::from_value(value.clone()).unwrap();
            let mut target = GamePort::new(None, Some(LAYOUTS[n % 2]));
            target.restore_state(&saved).unwrap();
            assert_eq!(serde_json::to_value(target.snapshot_state().unwrap()).unwrap(), value);
        } // seeded storage-only clocks, not natural runtime/physical timing evidence
    }

    #[test]
    fn game_port_schema_covers_native_fields_and_requires_nested_keys() {
        let port = GamePort::new(None, None);
        let value = serde_json::to_value(port.snapshot_state().unwrap()).unwrap();
        let source = include_str!("../game_port.rs");
        let fields = regex::Regex::new(r"(?m)^\s+([a-z_][a-z0-9_]*):").unwrap();
        for (name, path) in [
            ("GamePort", vec!["port"]),
            ("Stick", vec!["port", "sticks", "0"]),
            ("Axis", vec!["port", "sticks", "0", "x"]),
        ] {
            let pointer = format!("/{}", path.join("/"));
            let object = value.pointer(&pointer).unwrap().as_object().unwrap();
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
                missing
                    .pointer_mut(&pointer)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(field);
                assert!(
                    serde_json::from_value::<GamePortSnapshot>(missing).is_err(),
                    "missing {name}.{field}"
                );
            }
            let mut extra = value.clone();
            extra
                .pointer_mut(&pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("unknown".to_owned(), true.into());
            assert!(serde_json::from_value::<GamePortSnapshot>(extra).is_err());
        }
        for field in ["version", "port"] {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<GamePortSnapshot>(missing).is_err());
        }
        let mut extra = value.clone();
        extra.as_object_mut().unwrap().insert("unknown".to_owned(), true.into());
        assert!(serde_json::from_value::<GamePortSnapshot>(extra).is_err());
        for key in ["sticks", "buttons"] {
            let mut truncated = value.clone();
            truncated["port"][key].as_array_mut().unwrap().pop();
            assert!(serde_json::from_value::<GamePortSnapshot>(truncated).is_err());
        }
    }

    #[test]
    fn game_port_invalid_config_and_each_axis_refused_atomically() {
        let mut port = GamePort::new(None, None);
        port.set_button(0, 1, true);
        port.reset_oneshots();
        port.run(123.0625);
        let saved = port.snapshot_state().unwrap();
        for kind in 0..3 {
            let mut invalid = saved.clone();
            match kind {
                0 => invalid.version += 1,
                1 => invalid.port.port_base ^= 1,
                _ => invalid.port.layout = ControllerLayout::OneJoystickFourButtons,
            }
            assert!(port.restore_state(&invalid).is_err());
            assert_eq!(port.snapshot_state().unwrap(), saved);
        }
        for i in 0..2 {
            for j in 0..2 {
                for kind in 0..3 {
                    let mut invalid = saved.clone();
                    let stick = &mut invalid.port.sticks[i];
                    let axis = if j == 0 { &mut stick.x } else { &mut stick.y };
                    match kind {
                        0 => axis.pos = 1.125,
                        1 => axis.pos = f64::NAN,
                        _ => axis.time = f64::INFINITY,
                    }
                    assert!(port.restore_state(&invalid).is_err());
                    assert_eq!(port.snapshot_state().unwrap(), saved);
                }
            }
        }
        let mut nan = serde_json::to_value(saved).unwrap();
        nan["port"]["sticks"][1]["y"]["time"] = f64::NAN.to_bits().into();
        assert!(serde_json::from_value::<GamePortSnapshot>(nan).is_err());
    }
}
