//! Native keyboard latches, held-key order, typematic clocks and mappings.
//! Reconstruct the constructor's fixed key table, and refuse a different hash
//! iteration profile: clear(true) emits break bytes in that order. Exact clock
//! bits and stale clear(false) key lists are retained, not normalized.
//! Machine macro FIFO, PPI/PIC/bus clocks and complete restart are separate.

use super::*;

pub(crate) mod key_table {
    use super::*;

    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
        entries: Vec<(MartyKey, KeyState)>,
        order: Vec<MartyKey>,
    }

    pub fn serialize<S: serde::Serializer>(table: &MartyHashMap<MartyKey, KeyState>, s: S) -> Result<S::Ok, S::Error> {
        let wire = Wire {
            entries: MartyKey::iter()
                .filter_map(|key| table.get(&key).map(|value| (key, value.clone())))
                .collect(),
            order: table.keys().copied().collect(),
        };
        serde::Serialize::serialize(&wire, s)
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<MartyHashMap<MartyKey, KeyState>, D::Error> {
        let wire: Wire = serde::Deserialize::deserialize(d)?;
        if wire.entries.is_empty() && wire.order.is_empty() {
            return Ok(MartyHashMap::default()); // native Keyboard::default has no key table
        }
        let mut table = Keyboard::new(KeyboardType::ModelF, false).kb_hash;
        if wire.entries.len() != table.len() || wire.order != table.keys().copied().collect::<Vec<_>>() {
            return Err(serde::de::Error::custom(
                "incompatible keyboard key inventory/iteration profile",
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for (key, value) in wire.entries {
            if !seen.insert(key) {
                return Err(serde::de::Error::custom("duplicate keyboard key"));
            }
            let slot = table
                .get_mut(&key)
                .ok_or_else(|| <D::Error as serde::de::Error>::custom("unknown keyboard table key"))?;
            *slot = value;
        }
        Ok(table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TYPES: [KeyboardType; 3] = [KeyboardType::ModelF, KeyboardType::Tandy1000, KeyboardType::Pcjr];

    fn mapping(byte: u8) -> String {
        let mut text = String::new();
        for section in ["modelf", "tandy1000", "pcjr"] {
            text.push_str(&format!(
                r#"
[[keyboard.{section}.keycode_mappings]]
keycode = "KeyA"
modifiers = ["any"]
key_macro = []
macro_translate = false
scancodes = [{byte}]
existing_ignored_extension = true
[[keyboard.{section}.keycode_mappings]]
keycode = "F10"
modifiers = ["any"]
key_macro = ["+KeyC", "-KeyC"]
macro_translate = true
scancodes = []
"#
            ));
        }
        text
    }

    fn configured(kind: KeyboardType) -> Keyboard {
        let mut kb = Keyboard::new(kind, false);
        kb.load_mapping(&mapping(0x33)).unwrap(); // native TOML still accepts its extension
        kb.set_typematic_params(Some(true), Some(200.0), Some(50.0));
        kb
    }

    fn restore(kb: &Keyboard) -> Keyboard {
        let wire = serde_json::to_vec(&kb.snapshot_state().unwrap()).unwrap();
        let mut target = Keyboard::new(kb.kb_type, false); // independent destruction, no mappings
        target.restore_state(&serde_json::from_slice(&wire).unwrap()).unwrap();
        target
    }

    fn operation(kb: &mut Keyboard, macros: &mut VecDeque<KeybufferEntry>, n: usize) -> Option<u8> {
        let modifiers = KeyboardModifiers::default();
        match n % 32 {
            0 | 16 => kb.key_down(MartyKey::KeyA, &modifiers, Some(macros)),
            1 => kb.run(125.0),
            2 => kb.key_down(MartyKey::KeyB, &modifiers, Some(macros)),
            3 => kb.key_down(MartyKey::ShiftLeft, &modifiers, Some(macros)),
            4 => kb.key_down(MartyKey::KeyA, &modifiers, Some(macros)),
            5..=12 => kb.run(100_062.5),
            13 => kb.key_down(MartyKey::F10, &modifiers, Some(macros)),
            14 => kb.key_up(MartyKey::KeyA),
            15 => kb.clear(true),
            17 => kb.key_down(MartyKey::KeyC, &modifiers, Some(macros)),
            18 => kb.clear(false), // native leaves stale keys_pressed; do not normalize
            19..=26 => kb.run(50_062.5),
            27 => kb.key_up(MartyKey::KeyC),
            _ => {}
        }
        kb.recv_scancode()
    }

    #[test]
    fn keyboard_json_restore_continues_native_input_repeat_reset_and_macros() {
        for kind in TYPES {
            let mut reference = configured(kind);
            let mut restored = configured(kind);
            let mut macros_a = VecDeque::new();
            let mut macros_b = VecDeque::new();
            for n in 0..256 {
                restored = restore(&restored);
                assert_eq!(
                    reference.recv_scancode(),
                    restored.recv_scancode(),
                    "pending scan kind={kind:?} n={n}"
                );
                assert_eq!(
                    operation(&mut reference, &mut macros_a, n),
                    operation(&mut restored, &mut macros_b, n),
                    "native scan kind={kind:?} n={n}"
                );
                assert_eq!(macros_a, macros_b, "native macro output kind={kind:?} n={n}");
                assert_eq!(
                    serde_json::to_value(reference.snapshot_state().unwrap()).unwrap(),
                    serde_json::to_value(restored.snapshot_state().unwrap()).unwrap(),
                    "native continuation kind={kind:?} n={n}"
                );
            }
            assert!(!macros_a.is_empty(), "must actually produce macros");
        } // macro peer queues are retained/compared, not restored by KeyboardSnapshot
    }

    #[test]
    fn keyboard_restore_matches_typematic_deadline_scan_bytes() {
        for kind in TYPES {
            let mut reference = configured(kind);
            reference.key_down(MartyKey::KeyA, &KeyboardModifiers::default(), None);
            reference.recv_scancode();
            reference.run(140_000.0);
            reference.key_down(MartyKey::KeyB, &KeyboardModifiers::default(), None);
            reference.recv_scancode();
            reference.run(25_000.0); // A:165ms pressed/25ms repeat; B:25ms pressed
            assert_eq!(reference.recv_scancode(), None);
            let mut restored = restore(&reference);
            reference.run(26_000.0); // A crosses native repeat threshold; B does not
            restored.run(26_000.0);
            let byte = reference.recv_scancode();
            assert_eq!(byte, Some(0x33), "native mapped repeat kind={kind:?}");
            assert_eq!(restored.recv_scancode(), byte, "restored repeat deadline kind={kind:?}");
        }
    }

    #[test]
    fn keyboard_restore_preserves_scan_reset_bytes_and_hash_order() {
        for kind in TYPES {
            let mut reference = configured(kind);
            for key in [
                MartyKey::KeyA,
                MartyKey::KeyB,
                MartyKey::KeyC,
                MartyKey::KeyD,
                MartyKey::KeyE,
                MartyKey::KeyF,
                MartyKey::KeyG,
                MartyKey::KeyH,
                MartyKey::KeyI,
                MartyKey::KeyJ,
                MartyKey::ShiftLeft,
                MartyKey::ShiftRight,
            ] {
                reference.key_down(key, &KeyboardModifiers::default(), None);
            }
            let mut restored = restore(&reference);
            let scan = reference.recv_scancode();
            assert!(scan.is_some());
            assert_eq!(restored.recv_scancode(), scan, "pending scan kind={kind:?}");
            reference.clear(true);
            restored.clear(true);
            assert!(reference.reset_buffer.len() >= 10);
            assert_eq!(
                reference.reset_buffer, restored.reset_buffer,
                "native hash-order break sequence kind={kind:?}"
            );
            for _ in 0..16 {
                restored = restore(&reference); // destructive restores between queued reset bytes
                assert_eq!(
                    reference.recv_scancode(),
                    restored.recv_scancode(),
                    "pending reset byte kind={kind:?}"
                );
            }
        }
    }

    #[test]
    fn keyboard_restore_preserves_mapping_macros_and_cached_break_codes() {
        for kind in TYPES {
            let mut reference = configured(kind);
            reference.key_down(MartyKey::KeyA, &KeyboardModifiers::default(), None);
            assert_eq!(reference.recv_scancode(), Some(0x33));
            reference.load_mapping(&mapping(0x63)).unwrap(); // held key retains old translation
            let mut restored = restore(&reference);
            reference.key_up(MartyKey::KeyA);
            restored.key_up(MartyKey::KeyA);
            let byte = reference.recv_scancode();
            assert_eq!(byte, Some(0xb3));
            assert_eq!(restored.recv_scancode(), byte, "cached break byte kind={kind:?}");
            reference.key_down(MartyKey::KeyA, &KeyboardModifiers::default(), None);
            restored.key_down(MartyKey::KeyA, &KeyboardModifiers::default(), None);
            let byte = reference.recv_scancode();
            assert_eq!(byte, Some(0x63));
            assert_eq!(restored.recv_scancode(), byte, "current mapping kind={kind:?}");
            let mut macros_a = VecDeque::new();
            let mut macros_b = VecDeque::new();
            reference.key_down(MartyKey::F10, &KeyboardModifiers::default(), Some(&mut macros_a));
            restored.key_down(MartyKey::F10, &KeyboardModifiers::default(), Some(&mut macros_b));
            assert_eq!(macros_a.len(), 2);
            assert_eq!(macros_a, macros_b);
            let wire = serde_json::to_vec(&macros_a).unwrap();
            let decoded: VecDeque<KeybufferEntry> = serde_json::from_slice(&wire).unwrap();
            assert_eq!(decoded, macros_a); // typed queue codec only, not Machine FIFO restore
        }
    }

    #[test]
    fn keyboard_schema_requires_all_native_and_nested_fields() {
        let value = serde_json::to_value(configured(TYPES[0]).snapshot_state().unwrap()).unwrap();
        let source = include_str!("../keyboard_common.rs");
        let fields = regex::Regex::new(r"(?m)^\s+([a-z_][a-z0-9_]*):").unwrap();
        for (name, pointer) in [
            ("Keyboard", "/keyboard"),
            ("KeyState", "/keyboard/kb_hash/entries/0/1"),
            ("KeycodeMapping", "/keyboard/keycode_mappings/0"),
        ] {
            let object = value.pointer(pointer).unwrap().as_object().unwrap();
            let body = source
                .split(&format!("pub struct {name} {{"))
                .nth(1)
                .unwrap()
                .split("\n}")
                .next()
                .unwrap();
            let native: std::collections::HashSet<_> = fields.captures_iter(body).map(|c| c[1].to_owned()).collect();
            assert_eq!(native, object.keys().cloned().collect());
            for key in object.keys() {
                let mut missing = value.clone();
                missing
                    .pointer_mut(pointer)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
                assert!(
                    serde_json::from_value::<KeyboardSnapshot>(missing).is_err(),
                    "missing {name}.{key}"
                );
            }
        }
        for pointer in [
            "",
            "/keyboard",
            "/keyboard/kb_hash",
            "/keyboard/kb_hash/entries/0/1",
            "/keyboard/keycode_mappings/0",
        ] {
            let mut extra = value.clone();
            extra
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("unknown".to_owned(), true.into());
            assert!(serde_json::from_value::<KeyboardSnapshot>(extra).is_err());
        }
        for (pointer, key) in [
            ("", "version"),
            ("", "keyboard"),
            ("/keyboard/kb_hash", "entries"),
            ("/keyboard/kb_hash", "order"),
        ] {
            let mut missing = value.clone();
            missing
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(key);
            assert!(serde_json::from_value::<KeyboardSnapshot>(missing).is_err());
        }
        let entry = KeybufferEntry {
            keycode: MartyKey::KeyA,
            pressed: true,
            modifiers: KeyboardModifiers::default(),
            translate: true,
        };
        let entry_wire = serde_json::to_value(entry).unwrap();
        for pointer in ["", "/modifiers"] {
            let object = entry_wire.pointer(pointer).unwrap().as_object().unwrap();
            for key in object.keys() {
                let mut missing = entry_wire.clone();
                missing
                    .pointer_mut(pointer)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
                assert!(serde_json::from_value::<KeybufferEntry>(missing).is_err());
            }
            let mut extra = entry_wire.clone();
            extra
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("unknown".to_owned(), true.into());
            assert!(serde_json::from_value::<KeybufferEntry>(extra).is_err());
        }
        let mut duplicate = value.clone();
        duplicate["keyboard"]["kb_hash"]["entries"][1][0] = duplicate["keyboard"]["kb_hash"]["entries"][0][0].clone();
        assert!(serde_json::from_value::<KeyboardSnapshot>(duplicate).is_err());
        let mut profile = value.clone();
        profile["keyboard"]["kb_hash"]["order"]
            .as_array_mut()
            .unwrap()
            .reverse();
        assert!(serde_json::from_value::<KeyboardSnapshot>(profile).is_err());
        let mut truncated = value.clone();
        truncated["keyboard"]["kb_hash"]["entries"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(serde_json::from_value::<KeyboardSnapshot>(truncated).is_err());
    }

    #[test]
    fn keyboard_exact_clock_bits_default_and_atomic_refusals() {
        let default = Keyboard::default();
        assert_eq!(
            restore(&default).snapshot_state().unwrap(),
            default.snapshot_state().unwrap()
        );
        let mut unique = std::collections::HashSet::new();
        for n in 0..32 {
            let mut kb = configured(TYPES[0]);
            let seeds = [0, 1, 0x3ff0000000000001, 0x8000000000000000];
            let clocks: Vec<_> = (0..4)
                .map(|i| f64::from_bits(seeds[(n + i) % 4] + (n / 4) as u64))
                .collect();
            kb.typematic_delay = clocks[0];
            kb.typematic_rate = clocks[1];
            let key = kb.kb_hash.get_mut(&MartyKey::KeyA).unwrap();
            key.pressed_time = clocks[2];
            key.repeat_time = clocks[3];
            let value = serde_json::to_value(kb.snapshot_state().unwrap()).unwrap();
            assert!(unique.insert(value.to_string()));
            assert_eq!(
                serde_json::to_value(restore(&kb).snapshot_state().unwrap()).unwrap(),
                value
            );
        } //32 distinct seeded clock states, storage only
        let mut kb = configured(TYPES[0]);
        kb.key_down(MartyKey::KeyA, &KeyboardModifiers::default(), None);
        let saved = kb.snapshot_state().unwrap();
        for kind in 0..12 {
            let mut invalid = saved.clone();
            match kind {
                0 => invalid.version += 1,
                1 => invalid.keyboard.kb_type = KeyboardType::Pcjr,
                2 => invalid.keyboard.kb_type = KeyboardType::ModelM,
                3 => invalid.keyboard.kb_buffer_size = 0,
                4 => invalid.keyboard.typematic_delay = f64::NAN,
                5 => invalid.keyboard.typematic_rate = f64::INFINITY,
                6 => invalid.keyboard.kb_hash.get_mut(&MartyKey::KeyA).unwrap().pressed_time = f64::NAN,
                7 => invalid.keyboard.kb_hash.get_mut(&MartyKey::KeyA).unwrap().repeat_time = f64::INFINITY,
                8 => invalid.keyboard.keys_pressed.push(MartyKey::KeyA),
                9 => invalid.keyboard.kb_hash.get_mut(&MartyKey::KeyA).unwrap().translation = None,
                10 => invalid.keyboard.kb_hash.get_mut(&MartyKey::KeyA).unwrap().translation = Some(vec![1, 2]),
                _ => invalid.keyboard.keycode_mappings[0].modifiers.clear(),
            }
            assert!(kb.restore_state(&invalid).is_err());
            assert_eq!(kb.snapshot_state().unwrap(), saved);
        }
        let mut nan = serde_json::to_value(saved).unwrap();
        nan["keyboard"]["typematic_rate"] = f64::NAN.to_bits().into();
        assert!(serde_json::from_value::<KeyboardSnapshot>(nan).is_err());
    }
}

pub(crate) mod mapping_table {
    use super::*;

    // Snapshot records are strict without changing the existing TOML mapping
    // loader's acceptance of extension fields. Construction names every field.
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Record {
        keycode: String,
        modifiers: Vec<String>,
        key_macro: Vec<String>,
        macro_translate: bool,
        scancodes: Vec<u8>,
    }

    pub fn serialize<S: serde::Serializer>(mappings: &[KeycodeMapping], s: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(mappings, s)
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<KeycodeMapping>, D::Error> {
        let records: Vec<Record> = serde::Deserialize::deserialize(d)?;
        Ok(records
            .into_iter()
            .map(|r| KeycodeMapping {
                keycode: r.keycode,
                modifiers: r.modifiers,
                key_macro: r.key_macro,
                macro_translate: r.macro_translate,
                scancodes: r.scancodes,
            })
            .collect())
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KeyboardSnapshot {
    version: u32,
    keyboard: Keyboard,
}

impl Keyboard {
    pub(crate) fn snapshot_state(&self) -> Result<KeyboardSnapshot, &'static str> {
        let saved = KeyboardSnapshot {
            version: 1,
            keyboard: self.clone(),
        };
        self.preflight_state(&saved)?;
        Ok(saved)
    }

    pub(crate) fn preflight_state(&self, saved: &KeyboardSnapshot) -> Result<(), &'static str> {
        let kb = &saved.keyboard;
        if saved.version != 1
            || kb.kb_type != self.kb_type
            || kb.kb_type == KeyboardType::ModelM
            || kb.kb_buffer_size == 0
        {
            return Err("incompatible/unsupported keyboard version/type/buffer");
        }
        if !kb.typematic_delay.is_finite()
            || !kb.typematic_rate.is_finite()
            || kb
                .kb_hash
                .values()
                .any(|v| !v.pressed_time.is_finite() || !v.repeat_time.is_finite())
        {
            return Err("nonfinite keyboard typematic clock");
        }
        let template = Keyboard::new(kb.kb_type, false);
        if !kb.kb_hash.is_empty()
            && kb.kb_hash.keys().copied().collect::<Vec<_>>() != template.kb_hash.keys().copied().collect::<Vec<_>>()
        {
            return Err("incompatible keyboard table profile");
        }
        let mut seen = std::collections::HashSet::new();
        if kb
            .keys_pressed
            .iter()
            .any(|key| !kb.kb_hash.contains_key(key) || !seen.insert(*key))
        {
            return Err("invalid held-key inventory/order");
        }
        if kb.kb_hash.values().any(|v| {
            v.translation.as_ref().is_some_and(|codes| codes.len() != 1) || (v.pressed && v.translation.is_none())
        }) || kb.keycode_mappings.iter().any(|m| m.modifiers.is_empty())
        {
            return Err("unsupported keyboard translation/mapping");
        }
        // clear(false) leaves keys_pressed while resetting KeyState.pressed;
        // native set_typematic_params also accepts finite negative values.
        // Preserve both instead of inventing a tighter protocol invariant.
        Ok(())
    }

    pub(crate) fn restore_state(&mut self, saved: &KeyboardSnapshot) -> Result<(), &'static str> {
        self.preflight_state(saved)?;
        *self = saved.keyboard.clone();
        Ok(())
    }
}
