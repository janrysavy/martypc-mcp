//! Native MC6845 register selection, latches, raster/sync/cursor counters.
//! Active external trace writers are refused before capture or mutation.
//! Card VRAM, raster buffers, monitor PLL and bus/frontend clocks are separate.

use super::*;

macro_rules! crtc_state {
    ($( $(#[$attr:meta])* $field:ident: $ty:ty ),* $(,)?) => {
        #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        pub(crate) struct CrtcState {
            version: u32,
            $( $(#[$attr])* $field: $ty,)*
        }

        impl Crtc6845 {
            pub(crate) fn snapshot_state(&self) -> Result<CrtcState, &'static str> {
                if self.trace_logger.is_some() {
                    return Err("active CRTC trace snapshots are unsupported");
                }
                let saved = CrtcState { version: 1, $($field: self.$field.clone(),)* };
                self.preflight_state(&saved)?;
                Ok(saved)
            }

            pub(crate) fn preflight_state(&self, saved: &CrtcState) -> Result<(), &'static str> {
                if self.trace_logger.is_some() || saved.version != 1 {
                    return Err("incompatible CRTC version/active trace");
                }
                if saved.cursor_blink_rate.is_some_and(|v| !matches!(v, BLINK_FAST_RATE | BLINK_SLOW_RATE))
                    || saved.cursor_start_line > CURSOR_LINE_MASK
                    || saved.vlc_c9 > 0x1f || saved.vlc_c9i > 0x0f
                    || saved.vsc_c3h > 0x0f || saved.hsc_c3l > 0x0f || saved.vtac_c5 > 0x20 {
                    return Err("invalid native CRTC counter/cursor divider");
                }
                // Do not rebuild latches/counters from registers: those can
                // differ across pending port writes and frame transitions.
                // Native VTA reaches32 before entering an interlaced half-line;
                // the counter resets only at the subsequent frame start.
                // Native C4 wraps at256, despite its seven-bit field comment:
                // lowering R4 behind the current row reaches128 through255.
                Ok(())
            }

            pub(crate) fn restore_state(&mut self, saved: &CrtcState) -> Result<(), &'static str> {
                self.preflight_state(saved)?;
                $(self.$field = saved.$field.clone();)*
                Ok(())
            }
        }
    };
}

crtc_state! {
    reg: CrtcRegisterFile,
    reg_select: CrtcRegister,
    ticks: usize,
    frames: usize,
    mode: CrtcMode,
    start_address: u16,
    start_address_latch: u16,
    lightpen_position: u16,
    cursor_address: u16,
    cursor_enabled: bool,
    cursor_start_line: u8,
    cursor_active: bool,
    blink_state: bool,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    cursor_blink_rate: Option<u16>,
    hcc_c0: u8,
    vlc_c9: u8,
    vlc_c9i: u8,
    vcc_c4: u8,
    vsc_c3h: u8,
    hsc_c3l: u8,
    vtac_c5: u8,
    last_row: bool,
    previous_last_line: bool,
    last_line: bool,
    last_line_mgmt: bool,
    vma: u16,
    vma_t: u16,
    interlaced_mode: CrtcInterlacedMode,
    scanline_parity: InterlacedParity,
    frame_parity: InterlacedParity,
    status: CrtcStatus,
    in_hsync: bool,
    in_vsync: bool,
    vertical_de: bool,
    horizontal_de: bool,
    in_display_rows: bool,
    in_last_vblank_line: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(crtc: &Crtc6845) -> serde_json::Value {
        serde_json::to_value(crtc.snapshot_state().unwrap()).unwrap()
    }

    fn restore(crtc: &Crtc6845) -> Crtc6845 {
        let bytes = serde_json::to_vec(&crtc.snapshot_state().unwrap()).unwrap();
        let mut cold = Crtc6845::new(TraceLogger::None); // independent destruction
        cold.restore_state(&serde_json::from_slice(&bytes).unwrap()).unwrap();
        cold
    }

    fn configured(interlace: u8, width: u8, cursor: u8, adjust: u8) -> Crtc6845 {
        let mut crtc = Crtc6845::new(TraceLogger::None);
        for (reg, byte) in [
            (HorizontalTotalR0, 31),
            (HorizontalDisplayedR1, 20),
            (HorizontalSyncPositionR2, 5),
            (SyncWidthR3, width),
            (VerticalTotalR4, 5),
            (VerticalTotalAdjustR5, adjust),
            (VerticalDisplayedR6, 4),
            (VerticalSyncR7, 4),
            (InterlaceModeR8, interlace),
            (MaximumScanlineAddressR9, 3),
            (CursorStartLine, cursor),
            (CursorEndLine, 2),
            (StartAddressH, 0x12),
            (StartAddressL, 0x30),
            (CursorAddressH, 0x12),
            (CursorAddressL, 0x30),
        ] {
            crtc.write_register_direct(reg, byte);
        }
        crtc.process_start_of_frame();
        crtc
    }

    // Independent native APIs, not values read back through snapshot encoding.
    fn observe(crtc: &Crtc6845) -> Vec<u64> {
        vec![
            crtc.start_address() as u64,
            crtc.start_address_latch() as u64,
            crtc.ma() as u64,
            crtc.ra() as u64,
            crtc.hcc() as u64,
            crtc.vcc() as u64,
            crtc.vlc() as u64,
            crtc.hsc() as u64,
            crtc.vsc() as u64,
            crtc.vtac() as u64,
            crtc.cursor_address() as u64,
            crtc.cursor() as u64,
            crtc.cursor_enabled() as u64,
            crtc.frame_parity_bit() as u64,
            crtc.hsync() as u64,
            crtc.vsync() as u64,
            crtc.in_vta() as u64,
            crtc.border() as u64,
            crtc.read_register() as u64,
        ]
    }

    fn operation(crtc: &mut Crtc6845, n: usize) {
        // Separate select/data writes deliberately span a restore boundary.
        match n % 32 {
            0 => crtc.port_write(0, 14),
            1 => crtc.port_write(1, 0x13),
            2 => crtc.port_write(0, 15),
            3 => crtc.port_write(1, (n as u8).wrapping_add(0x30)),
            4 => crtc.port_write(0, 12),
            5 => crtc.port_write(1, 0x12),
            6 => crtc.port_write(0, 13),
            7 => crtc.port_write(1, n as u8),
            8 => crtc.latch_lightpen(),
            9 => crtc.port_write(0, 16),
            10 => crtc.port_write(0, 17),
            11 => crtc.port_write(0, 31), // native invalid selection accepted, FF read
            _ => {}
        }
    }

    #[test]
    fn crtc_json_restore_continues_native_raster_cursor_interlace_and_io() {
        let (mut frames, mut syncs, mut vta, mut odd) = (0, 0, 0, 0);
        for interlace in [0, 1, 3] {
            for width in [0, 1, 0x1a] {
                for cursor in [0, 0x20, 0x40, 0x60] {
                    let mut reference = configured(interlace, width, cursor, 3);
                    let mut restored = configured(interlace, width, cursor, 3);
                    for n in 0..128 {
                        restored = restore(&restored);
                        operation(&mut reference, n);
                        operation(&mut restored, n);
                        for _ in 0..[1, 7, 16, 64][n % 4] {
                            let (status, address) = reference.tick();
                            let expected = (status.clone(), address);
                            let (status, address) = restored.tick();
                            assert_eq!(
                                (status.clone(), address),
                                expected,
                                "native tick interlace={interlace} width={width} cursor={cursor} n={n}"
                            );
                            syncs += u64::from(expected.0.hsync || expected.0.vsync);
                            vta += u64::from(reference.in_vta());
                            odd += u64::from(reference.frame_parity_bit());
                        }
                        assert_eq!(observe(&reference), observe(&restored), "native CRTC API n={n}");
                        assert_eq!(json(&reference), json(&restored));
                    }
                    frames += reference.frames - 1;
                }
            }
        } //4608 destructive JSON native continuations, no retained CRTC fields
        assert!(
            frames > 0 && syncs > 0 && vta > 0 && odd > 0,
            "must actually enter each timing path"
        );
    }

    #[test]
    fn crtc_restore_preserves_selected_port_and_pending_frame_latch() {
        let mut reference = configured(0, 1, 0, 0);
        reference.port_write(0, 14);
        let mut restored = restore(&reference);
        for crtc in [&mut reference, &mut restored] {
            crtc.port_write(1, 0x23);
        }
        assert_eq!(reference.port_read(1), 0x23);
        assert_eq!(restored.port_read(1), 0x23, "restored register selection");
        for crtc in [&mut reference, &mut restored] {
            crtc.write_register_direct(StartAddressL, 0x99);
        }
        assert_eq!(reference.start_address(), 0x1299);
        assert_eq!(reference.start_address_latch(), 0x1230);
        restored = restore(&restored);
        assert_eq!(restored.start_address_latch(), 0x1230, "pending original frame latch");
        for crtc in [&mut reference, &mut restored] {
            crtc.process_start_of_frame();
        }
        assert_eq!(reference.start_address_latch(), 0x1299);
        assert_eq!(restored.start_address_latch(), 0x1299);
        assert_eq!(observe(&reference), observe(&restored));
        reference.latch_lightpen();
        restored = restore(&reference);
        for register in [16, 17] {
            reference.port_write(0, register);
            restored.port_write(0, register);
            assert_eq!(
                reference.port_read(1),
                restored.port_read(1),
                "native lightpen register"
            );
        } //three native I/O/frame/lightpen continuations
    }

    #[test]
    fn crtc_restore_preserves_native_vta_32_until_half_line_frame_start() {
        let mut reference = configured(1, 1, 0, 31);
        for _ in 0..8192 {
            reference.tick();
            if reference.vtac_c5 == 32 {
                break;
            }
        }
        assert_eq!(reference.vtac_c5, 32, "must reach native transient32");
        assert!(matches!(reference.mode, CrtcMode::InterlacedHalfLine));
        let mut restored = restore(&reference);
        for _ in 0..32 {
            let (status, address) = reference.tick();
            let expected = (status.clone(), address);
            let (status, address) = restored.tick();
            assert_eq!((status.clone(), address), expected, "native half-line continuation");
            assert_eq!(observe(&reference), observe(&restored));
        }
        assert_eq!(reference.vtac_c5, 0);
        assert_eq!(json(&reference), json(&restored));
    }

    #[test]
    fn crtc_restore_preserves_native_cursor_blink_output() {
        for cursor in [0x40, 0x60] {
            let mut reference = configured(0, 1, cursor, 0);
            for (reg, value) in [
                (HorizontalTotalR0, 7),
                (HorizontalDisplayedR1, 4),
                (HorizontalSyncPositionR2, 2),
                (VerticalTotalR4, 3),
                (VerticalDisplayedR6, 3),
                (VerticalSyncR7, 3),
                (MaximumScanlineAddressR9, 1),
                (CursorEndLine, 1),
            ] {
                reference.write_register_direct(reg, value);
            }
            for _ in 0..4096 {
                reference.tick();
                if reference.blink_state {
                    break;
                }
            }
            assert!(reference.blink_state, "must reach native cursor divider {cursor}");
            let mut restored = restore(&reference);
            let mut visible = 0;
            for _ in 0..128 {
                let (status, address) = reference.tick();
                let expected = (status.clone(), address);
                visible += u32::from(expected.0.cursor);
                let (status, address) = restored.tick();
                assert_eq!((status.clone(), address), expected, "native restored blinking cursor");
            }
            assert!(visible > 0, "must observe an actual cursor output");
            assert_eq!(json(&reference), json(&restored));
        } //two native blink continuations, fast and slow dividers
    }

    #[test]
    fn crtc_restore_preserves_native_row_counter_above_seven_bits() {
        let mut reference = configured(0, 1, 0, 0);
        reference.write_register_direct(VerticalTotalR4, 127);
        while reference.vcc() == 0 {
            reference.tick();
        }
        // A port write moves the coincidence target behind the current row.
        // Native wrapping_add is eight bits despite the seven-bit field comment.
        reference.write_register_direct(VerticalTotalR4, 0);
        let mut witnessed = 0;
        for wanted in [128, 255] {
            for _ in 0..65536 {
                if reference.vcc() == wanted {
                    break;
                }
                reference.tick();
            }
            assert_eq!(reference.vcc(), wanted, "must reach native row {wanted}");
            let mut restored = restore(&reference);
            for _ in 0..32 {
                let (status, address) = reference.tick();
                let expected = (status.clone(), address);
                let (status, address) = restored.tick();
                assert_eq!((status.clone(), address), expected);
                assert_eq!(observe(&reference), observe(&restored));
            }
            assert_eq!(json(&reference), json(&restored));
            witnessed += 1;
        }
        assert_eq!(witnessed, 2);
    }

    #[test]
    fn crtc_schema_inventory_and_invalid_restores_are_atomic() {
        let mut crtc = configured(3, 0, 0x60, 3);
        for _ in 0..39 {
            crtc.tick();
        }
        let value = json(&crtc);
        let source = include_str!("../mc6845.rs");
        let body = source
            .split("pub struct Crtc6845 {")
            .nth(1)
            .unwrap()
            .split("\n}")
            .next()
            .unwrap();
        let fields = regex::Regex::new(r"(?m)^\s+(?:pub\s+)?([a-z_][a-z0-9_]*):").unwrap();
        let mut native: std::collections::HashSet<_> = fields.captures_iter(body).map(|c| c[1].to_owned()).collect();
        assert!(native.remove("trace_logger"));
        native.insert("version".into());
        assert_eq!(native, value.as_object().unwrap().keys().cloned().collect());
        for pointer in ["", "/status"] {
            for key in value.pointer(pointer).unwrap().as_object().unwrap().keys() {
                let mut missing = value.clone();
                missing
                    .pointer_mut(pointer)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
                assert!(
                    serde_json::from_value::<CrtcState>(missing).is_err(),
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
            assert!(serde_json::from_value::<CrtcState>(extra).is_err());
        }
        for delta in [-1, 1] {
            let mut bad = value.clone();
            let array = bad["reg"].as_array_mut().unwrap();
            if delta < 0 {
                array.pop();
            } else {
                array.push(0.into());
            }
            assert!(serde_json::from_value::<CrtcState>(bad).is_err());
        }
        for (field, bad) in [
            ("version", 2),
            ("cursor_blink_rate", 0),
            ("cursor_blink_rate", 1),
            ("cursor_start_line", 32),
            ("vlc_c9", 32),
            ("vlc_c9i", 16),
            ("vsc_c3h", 16),
            ("hsc_c3l", 16),
            ("vtac_c5", 33),
        ] {
            let mut invalid = value.clone();
            invalid[field] = bad.into();
            invalid["start_address_latch"] = 0.into(); // expose premature assignment
            let decoded = serde_json::from_value(invalid).unwrap();
            assert!(crtc.restore_state(&decoded).is_err(), "invalid {field}");
            assert_eq!(json(&crtc), value);
        }
    }

    #[test]
    fn crtc_active_external_trace_refusals_preserve_live_state() {
        let mut crtc = configured(1, 0, 0x40, 3);
        crtc.tick();
        let saved = crtc.snapshot_state().unwrap();
        let mut replacement = saved.clone();
        replacement.vma = 0;
        let directory = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "target".into());
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("crtc-snapshot-trace-{}.log", std::process::id()));
        for logger in [TraceLogger::Console, TraceLogger::from_filename(&path)] {
            assert!(logger.is_some());
            crtc.trace_logger = logger;
            assert!(crtc.snapshot_state().is_err());
            assert!(crtc.restore_state(&replacement).is_err());
            crtc.trace_logger = TraceLogger::None;
            assert_eq!(crtc.snapshot_state().unwrap(), saved);
        }
        std::fs::remove_file(path).unwrap();
    }
}
