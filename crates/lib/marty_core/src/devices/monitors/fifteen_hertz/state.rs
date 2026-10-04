//! Native RGBI monitor/PLL state only. CGA raster, VRAM, buffers, bus clocks
//! and frontend rendering are separate owners. This preserves MartyPC's
//! current synchronization algorithm; it is not physical monitor validation.

use super::*;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MonitorState {
    version: u32,
    monitor: FifteenHertzMonitor,
}

impl FifteenHertzMonitor {
    pub(crate) fn snapshot_state(&self) -> Result<MonitorState, &'static str> {
        let saved = MonitorState {
            version: 1,
            monitor: self.clone(),
        };
        self.preflight_state(&saved)?;
        Ok(saved)
    }

    pub(crate) fn preflight_state(&self, saved: &MonitorState) -> Result<(), &'static str> {
        if saved.version != 1 {
            return Err("incompatible monitor snapshot version");
        }
        saved.monitor.horizontal_pll.validate_snapshot()?;
        saved.monitor.vertical_pll.validate_snapshot()
    }

    pub(crate) fn restore_state(&mut self, saved: &MonitorState) -> Result<(), &'static str> {
        self.preflight_state(saved)?;
        *self = saved.monitor.clone();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(monitor: &FifteenHertzMonitor) -> serde_json::Value {
        serde_json::to_value(monitor.snapshot_state().unwrap()).unwrap()
    }

    fn restore(monitor: &FifteenHertzMonitor) -> FifteenHertzMonitor {
        let bytes = serde_json::to_vec(&monitor.snapshot_state().unwrap()).unwrap();
        let mut cold = FifteenHertzMonitor::default(); // independent destruction, no state loader
        cold.restore_state(&serde_json::from_slice(&bytes).unwrap()).unwrap();
        cold
    }

    fn run(monitor: &mut FifteenHertzMonitor, ticks: u32, h: bool, v: bool) -> (u32, u32) {
        let (mut horizontal, mut vertical) = (0, 0);
        monitor.run(ticks, h, v, &mut || horizontal += 1, &mut || vertical += 1);
        (horizontal, vertical)
    }

    // Observe native APIs separately from the snapshot encoder.
    fn observed(pll: &VideoHoldPll) -> (bool, SyncPolarity, Option<u64>, u64, u64, u64, u64, bool) {
        (
            pll.is_locked(),
            pll.polarity(),
            pll.observed_freq().map(f64::to_bits),
            pll.current_freq().to_bits(),
            pll.phase().to_bits(),
            pll.sync_phase().to_bits(),
            pll.error().to_bits(),
            pll.is_in_window(),
        )
    }

    #[test]
    fn monitor_json_restore_continues_native_sync_and_hold() {
        let mut total_callbacks = (0, 0);
        for enabled in [false, true] {
            for polarity in [SyncPolarity::Positive, SyncPolarity::Negative] {
                let mut reference = FifteenHertzMonitor::default();
                reference.set_enabled(enabled);
                reference.set_sync_polarities(polarity, polarity);
                reference.horizontal_pll.adjust_hold(0.000001);
                reference.vertical_pll.adjust_hold(-0.00000001);
                let mut restored = restore(&reference);
                for n in 0..128 {
                    restored = restore(&restored);
                    let active_h = n % 4 == 0 || n % 4 == 1;
                    let active_v = n % 32 == 0 || n % 32 == 1;
                    let invert = polarity == SyncPolarity::Negative;
                    let ticks = [0, 32, 880, 0][n % 4];
                    let actual = run(&mut reference, ticks, active_h ^ invert, active_v ^ invert);
                    total_callbacks.0 += actual.0;
                    total_callbacks.1 += actual.1;
                    assert_eq!(
                        run(&mut restored, ticks, active_h ^ invert, active_v ^ invert),
                        actual,
                        "native sync callbacks enabled={enabled} polarity={polarity:?} n={n}"
                    );
                    assert_eq!(
                        observed(&reference.horizontal_pll),
                        observed(&restored.horizontal_pll),
                        "native horizontal API enabled={enabled} polarity={polarity:?} n={n}"
                    );
                    assert_eq!(
                        observed(&reference.vertical_pll),
                        observed(&restored.vertical_pll),
                        "native vertical API enabled={enabled} polarity={polarity:?} n={n}"
                    );
                    assert_eq!(json(&reference), json(&restored));
                }
            }
        } //512 destructive continuation restores, plus four initial restores
        assert!(total_callbacks.0 > 0 && total_callbacks.1 > 0);
    }

    #[test]
    fn monitor_restore_preserves_held_sync_edge_and_enabled_output() {
        for enabled in [true, false] {
            let mut reference = FifteenHertzMonitor::default();
            reference.set_enabled(enabled);
            assert_eq!(run(&mut reference, 0, true, true), (1, 1));
            let mut restored = restore(&reference);
            let actual = run(&mut reference, 912, true, true);
            assert_eq!(actual, if enabled { (0, 0) } else { (1, 1) });
            assert_eq!(
                run(&mut restored, 912, true, true),
                actual,
                "restored held sync/enable output"
            );
            assert_eq!(observed(&reference.horizontal_pll), observed(&restored.horizontal_pll));
        }
    }

    #[test]
    fn monitor_restore_preserves_actual_observed_period_and_drift() {
        let mut reference = FifteenHertzMonitor::default();
        assert_eq!(run(&mut reference, 0, true, true), (1, 1));
        run(&mut reference, 500, false, false);
        reference.horizontal_pll.adjust_hold(0.0001); // native API, allowed beyond nominal range
        let mut restored = restore(&reference);
        let actual = run(&mut reference, 500, true, false);
        assert_eq!(
            reference.horizontal_pll.observed_freq().unwrap().to_bits(),
            (NTSC_CLOCK * 1_000_000.0 / 1000.0).to_bits()
        );
        assert_eq!(run(&mut restored, 500, true, false), actual);
        assert_eq!(
            observed(&reference.horizontal_pll),
            observed(&restored.horizontal_pll),
            "restored actual period/drift API"
        );
        assert_eq!(json(&reference), json(&restored));
    }

    #[test]
    fn monitor_schema_covers_every_native_field_and_refuses_atomically() {
        let mut monitor = FifteenHertzMonitor::default();
        run(&mut monitor, 128, false, false);
        let value = json(&monitor);
        let fields = regex::Regex::new(r"(?m)^\s+([a-z_][a-z0-9_]*):").unwrap();
        for (name, source, pointer) in [
            ("FifteenHertzMonitor", include_str!("../fifteen_hertz.rs"), "/monitor"),
            (
                "VideoHoldPll",
                include_str!("../../../video_pll.rs"),
                "/monitor/horizontal_pll",
            ),
        ] {
            let body = source
                .split(&format!("pub struct {name} {{"))
                .nth(1)
                .unwrap()
                .split("\n}")
                .next()
                .unwrap();
            let native: std::collections::HashSet<_> = fields.captures_iter(body).map(|c| c[1].to_owned()).collect();
            assert_eq!(
                native,
                value
                    .pointer(pointer)
                    .unwrap()
                    .as_object()
                    .unwrap()
                    .keys()
                    .cloned()
                    .collect()
            );
        }
        for pointer in ["", "/monitor", "/monitor/horizontal_pll", "/monitor/vertical_pll"] {
            for key in value.pointer(pointer).unwrap().as_object().unwrap().keys() {
                let mut missing = value.clone();
                missing
                    .pointer_mut(pointer)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
                assert!(
                    serde_json::from_value::<MonitorState>(missing).is_err(),
                    "missing {pointer}/{key}"
                );
            }
            let mut extra = value.clone();
            extra
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("unknown".into(), true.into());
            assert!(serde_json::from_value::<MonitorState>(extra).is_err());
        }
        for n in 0..7 {
            let mut invalid = value.clone();
            match n {
                0 => invalid["version"] = 2.into(),
                1 => invalid["monitor"]["horizontal_pll"]["ticks_per_second"] = 0u64.into(),
                2 => invalid["monitor"]["vertical_pll"]["target_period_ticks"] = 0u64.into(),
                3 => invalid["monitor"]["horizontal_pll"]["min_drift"] = 1.0f64.to_bits().into(),
                4 => invalid["monitor"]["vertical_pll"]["max_drift"] = (-1.0f64).to_bits().into(),
                5 => invalid["monitor"]["horizontal_pll"]["vco_phase"] = f64::NAN.to_bits().into(),
                _ => invalid["monitor"]["vertical_pll"]["drift_offset"] = f64::INFINITY.to_bits().into(),
            }
            invalid["monitor"]["emulate_hsync"] = false.into(); // expose premature mutation
            match serde_json::from_value::<MonitorState>(invalid) {
                Ok(decoded) => assert!(monitor.restore_state(&decoded).is_err()),
                Err(_) => assert!(n >= 5),
            }
            assert_eq!(json(&monitor), value);
        }
        // Direct invalid typed state is also refused before replacing live fields.
        let mut bad = monitor.snapshot_state().unwrap();
        bad.monitor.vertical_pll.adjust_hold(f64::NAN);
        assert!(monitor.restore_state(&bad).is_err());
        assert_eq!(json(&monitor), value);
        monitor.vertical_pll.adjust_hold(f64::INFINITY);
        assert!(monitor.snapshot_state().is_err());
    }
}
