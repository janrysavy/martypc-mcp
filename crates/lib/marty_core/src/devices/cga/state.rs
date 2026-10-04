//! Complete native CGA-owned state, including both in-progress raster buffers.
//! External trace writers are refused. Bus/frontend/disk state is separate.
//! Restore constructs a fully validated replacement before changing live state.

use super::*;
use crate::devices::{mc6845::CrtcState, monitors::fifteen_hertz::MonitorState};

macro_rules! cga_state {
    ($( $(#[$attr:meta])* $field:ident: $ty:ty ),* $(,)?) => {
        #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        pub(crate) struct CgaState {
            version: u32,
            crtc: CrtcState,
            monitor: MonitorState,
            mem: Vec<u8>,
            buf: [Vec<u8>; 2],
            $( $(#[$attr])* $field: $ty,)*
        }

        impl CGACard {
            pub(crate) fn snapshot_state(&self) -> Result<CgaState, &'static str> {
                if self.trace_logger.is_some() {
                    return Err("active CGA trace snapshots are unsupported");
                }
                let saved = CgaState {
                    version: 1,
                    crtc: self.crtc.snapshot_state()?,
                    monitor: self.monitor.snapshot_state()?,
                    mem: self.mem.to_vec(),
                    buf: [self.buf[0].to_vec(), self.buf[1].to_vec()],
                    $($field: self.$field.clone(),)*
                };
                self.preflight_state(&saved)?;
                Ok(saved)
            }

            pub(crate) fn preflight_state(&self, saved: &CgaState) -> Result<(), &'static str> {
                if self.trace_logger.is_some() || saved.version != 1 {
                    return Err("incompatible CGA version/active trace");
                }
                self.crtc.preflight_state(&saved.crtc)?;
                self.monitor.preflight_state(&saved.monitor)?;
                if saved.mem.len() != CGA_MEM_SIZE || saved.buf.iter().any(|b| b.len() != CGA_MAX_CLOCK) {
                    return Err("incompatible CGA VRAM/raster length");
                }
                if saved.front_buf > 1 || saved.back_buf > 1 || saved.front_buf == saved.back_buf
                    || saved.slot_idx > 4 || saved.cc_palette >= CGA_PALETTES.len()
                    || saved.char_col >= CGA_HCHAR_CLOCK
                    || saved.hsync_phase > 1
                    || saved.front_buf_interlaced_frame_parity.is_some_and(|p| p > 1)
                    || saved.cur_fg > 15 || saved.cur_bg > 15
                    || saved.cc_altcolor > 15 || saved.cc_overscan_color > 15 {
                    return Err("invalid CGA native index/color/phase");
                }
                if !matches!((saved.clock_divisor, saved.char_clock, saved.char_clock_mask, saved.char_clock_odd_mask),
                    (1, 8, 7, 15) | (2, 16, 15, 31)) {
                    return Err("invalid CGA native character clock");
                }
                if saved.extents.apertures.len() != CGA_APERTURES.len()
                    || saved.extents.field_w != CGA_XRES_MAX
                    || saved.extents.field_h != CGA_YRES_MAX_INTERLACED
                    || saved.extents.row_stride != CGA_XRES_MAX as usize
                    || saved.aperture >= CGA_APERTURES.len() {
                    return Err("incompatible CGA display extents");
                }
                let mut apertures = CGA_APERTURES;
                let crop = saved.extents.apertures[0].x;
                if ![CGA_APERTURE_CROPPED_X, CGA_APERTURE_CROPPED_X - 8].contains(&crop)
                    || !saved.extents.double_scan {
                    return Err("invalid CGA aperture crop/scan mode");
                }
                // Native reset preserves extents but resets hsync_phase.
                // Either measured native crop remains valid independently.
                apertures[0].x = crop;
                if saved.extents.apertures != apertures {
                    return Err("incompatible CGA aperture geometry");
                }
                if ![saved.scanline_us, saved.frame_us, saved.accumulated_us].iter().all(|v| v.is_finite()) {
                    return Err("nonfinite CGA clock");
                }
                // Pending mode/clock changes can make register bytes disagree
                // with effective flags. Preserve them; never regenerate state.
                // Raster positions can exceed the drawable field in native
                // out-of-sync operation; drawing guards that case itself.
                Ok(())
            }

            pub(crate) fn restore_state(&mut self, saved: &CgaState) -> Result<(), &'static str> {
                self.preflight_state(saved)?;
                let mut replacement = CGACard::new(TraceLogger::None, saved.clock_mode, false);
                replacement.crtc.restore_state(&saved.crtc)?;
                replacement.monitor.restore_state(&saved.monitor)?;
                replacement.mem = saved.mem.clone().into_boxed_slice().try_into()
                    .map_err(|_| "incompatible CGA VRAM length")?;
                replacement.buf = [
                    saved.buf[0].clone().into_boxed_slice().try_into().map_err(|_| "incompatible CGA raster length")?,
                    saved.buf[1].clone().into_boxed_slice().try_into().map_err(|_| "incompatible CGA raster length")?,
                ];
                $(replacement.$field = saved.$field.clone();)*
                *self = replacement;
                Ok(())
            }
        }
    };
}

cga_state! {
    debug: bool,
    debug_draw: bool,
    cycles: u64,
    last_vsync_cycles: u64,
    cur_screen_cycles: u64,
    cycles_per_vsync: u64,
    sink_cycles: u32,
    catching_up: bool,
    last_rw_tick: u32,
    rw_slots: [RwSlot; 4],
    slot_idx: usize,
    enable_snow: bool,
    dirty_snow: bool,
    snow_char: u8,
    last_bus_value: u8,
    last_bus_addr: usize,
    snow_count: u64,
    mode_pending: bool,
    clock_pending: bool,
    mode_byte: u8,
    display_mode: DisplayMode,
    mode_enable: bool,
    mode_graphics: bool,
    mode_bw: bool,
    mode_hires_gfx: bool,
    mode_hires_txt: bool,
    mode_blinking: bool,
    cc_palette: usize,
    cc_altcolor: u8,
    cc_overscan_color: u8,
    #[serde(with = "crate::snapshot_codec::f64_bits")]
    scanline_us: f64,
    #[serde(with = "crate::snapshot_codec::f64_bits")]
    frame_us: f64,
    cursor_frames: u32,
    v_flyback_count: u64,
    frame_count: u64,
    status_reads: u64,
    in_crtc_hblank: bool,
    in_crtc_vblank: bool,
    in_card_vblank: bool,
    in_card_hblank: bool,
    border: bool,
    border_override: bool,
    cc_register: u8,
    clock_divisor: u8,
    clock_mode: ClockingMode,
    char_clock: u32,
    char_clock_mask: u64,
    char_clock_odd_mask: u64,
    hsync_phase: u8,
    beam_x: u32,
    beam_y: u32,
    in_monitor_hsync: bool,
    in_monitor_vblank: bool,
    in_monitor_vsync: bool,
    monitor_hsc: u32,
    scanline: u32,
    missed_hsyncs: u32,
    overscan_left: u32,
    overscan_right_start: u32,
    overscan_right: u32,
    vsync_len: u32,
    in_display_area: bool,
    cur_char: u8,
    cur_attr: u8,
    cur_fg: u8,
    cur_bg: u8,
    cur_blink: bool,
    char_col: u8,
    hcc_c0: u8,
    vcc_c4: u8,
    last_row: bool,
    last_line: bool,
    vsc_c3h: u8,
    hsc_c3l: u8,
    vtac_c5: u8,
    in_vta: bool,
    vma: usize,
    vma_t: usize,
    rba: usize,
    blink_ticks: usize,
    internal_cursor_blink_state: bool,
    text_blink_state: bool,
    #[serde(with = "crate::snapshot_codec::f64_bits")]
    accumulated_us: f64,
    ticks_advanced: u32,
    pixel_clocks_owed: u32,
    ticks_accum: u32,
    clocks_accum: u32,
    back_buf: usize,
    front_buf: usize,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    front_buf_interlaced_frame_parity: Option<u8>,
    extents: DisplayExtents,
    aperture: usize,
    debug_color: u8,
    debug_counter: u64,
    lightpen_pos: (u32, u32),
    lightpen_tick: u32,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    lightpen_trigger_tick: Option<u32>,
    lightpen_latch: bool,
    lightpen_addr: usize,
    lightpen_switch: bool,
    emulate_sync: bool,
    monitor_emulation: bool,
    last_card_hblank: bool,
    last_card_vblank: bool,
    out_of_sync: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{IoDevice, MemoryMappedDevice};
    use crate::devices::cga::io::*;

    fn write(card: &mut CGACard, port: u16, byte: u8, ticks: u32) {
        IoDevice::write_u8(card, port, byte, None, DeviceRunTimeUnit::SystemTicks(ticks), None);
    }

    fn configured(clock: ClockingMode, mode: u8, monitor: bool) -> CGACard {
        let mut card = CGACard::new(TraceLogger::None, clock, false);
        card.enable_snow = true;
        card.set_debug_draw_state(false);
        card.set_monitor_emulation(monitor);
        // Establish native BIOS-style 80-column clock selection first. Native
        // mode changes detect edges relative to that effective text mode.
        write(&mut card, CGA_MODE_CONTROL_REGISTER, 0x29, 0);
        card.run(DeviceRunTimeUnit::SystemTicks(32), &mut None, None);
        let high_text = mode & 1 != 0;
        let graphics = mode & 2 != 0;
        let registers = [
            if high_text { 113 } else { 56 },
            if high_text { 80 } else { 40 },
            if high_text { 90 } else { 45 },
            10,
            if graphics { 127 } else { 31 },
            6,
            if graphics { 100 } else { 25 },
            if graphics { 112 } else { 28 },
            0,
            if graphics { 1 } else { 7 },
            6,
            7,
            0,
            0,
            0,
            0,
        ];
        for (reg, byte) in registers.into_iter().enumerate() {
            write(&mut card, CRTC_ADDRESS0, reg as u8, 0);
            write(&mut card, CRTC_DATA0, byte, 0);
        }
        write(&mut card, CGA_MODE_CONTROL_REGISTER, mode, 0);
        write(&mut card, CGA_COLOR_CONTROL_REGISTER, 0x30, 0);
        for n in 0..CGA_MEM_SIZE {
            let byte = (n as u8).wrapping_mul(13).wrapping_add(33);
            MemoryMappedDevice::mmio_write_u8(&mut card, CGA_MEM_ADDRESS + n, byte, 0, None);
        }
        card.run(DeviceRunTimeUnit::SystemTicks(32003), &mut None, None);
        assert_eq!(card.clock_divisor, if high_text { 1 } else { 2 });
        assert_eq!(card.mode_graphics, graphics);
        card
    }

    fn restore(card: &CGACard) -> CGACard {
        let bytes = serde_json::to_vec(&card.snapshot_state().unwrap()).unwrap();
        let mut cold = CGACard::new(TraceLogger::None, ClockingMode::Cycle, false);
        cold.restore_state(&serde_json::from_slice(&bytes).unwrap()).unwrap();
        cold
    }

    // These observations use native card interfaces, not snapshot fields.
    fn compare(reference: &mut CGACard, restored: &mut CGACard) {
        assert_eq!(reference.beam_pos(), restored.beam_pos(), "native beam");
        assert_eq!(reference.sync(), restored.sync(), "native sync");
        assert_eq!(reference.crtc.hcc(), restored.crtc.hcc(), "native CRTC character count");
        assert_eq!(reference.crtc.ma(), restored.crtc.ma(), "native CRTC memory address");
        assert_eq!(reference.frame_count(), restored.frame_count(), "native frame");
        assert_eq!(reference.interlaced_frame_parity(), restored.interlaced_frame_parity());
        assert_eq!(reference.display_mode(), restored.display_mode(), "native display mode");
        assert_eq!(
            reference.display_extents(),
            restored.display_extents(),
            "native display extents"
        );
        assert!(
            reference.buf(BufferSelect::Back) == restored.buf(BufferSelect::Back),
            "native back raster"
        );
        assert!(reference.display_buf() == restored.display_buf(), "native front raster");
        for offset in 0..CGA_MEM_SIZE {
            assert_eq!(
                MemoryMappedDevice::mmio_peek_u8(reference, CGA_MEM_ADDRESS + offset, None),
                MemoryMappedDevice::mmio_peek_u8(restored, CGA_MEM_ADDRESS + offset, None),
                "native VRAM read"
            );
        }
        let read = |card: &mut CGACard| IoDevice::read_u8(card, CGA_STATUS_REGISTER, DeviceRunTimeUnit::SystemTicks(0));
        assert_eq!(read(reference), read(restored), "native status port");
        assert!(
            reference.snapshot_state().unwrap() == restored.snapshot_state().unwrap(),
            "complete owned CGA state"
        );
    }

    #[test]
    fn cga_json_restore_continues_native_modes_raster_vram_and_monitor() {
        let (mut swaps, mut snow, mut pending) = (0, 0, 0);
        for clock in [ClockingMode::Cycle, ClockingMode::Character, ClockingMode::Dynamic] {
            for mode in [0x29, 0x28, 0x0a, 0x1a] {
                for monitor in [false, true] {
                    let mut reference = configured(clock, mode, monitor);
                    reference.run(DeviceRunTimeUnit::SystemTicks(320003), &mut None, None);
                    let mut restored = restore(&reference);
                    for n in 0..8 {
                        // Writes and a pending light-pen trigger precede each checkpoint.
                        // The next run consumes precisely the retained native state.
                        for card in [&mut reference, &mut restored] {
                            if n == 2 {
                                write(card, CGA_MODE_CONTROL_REGISTER, 0x0a, 3);
                            }
                            if n == 5 {
                                write(card, CGA_MODE_CONTROL_REGISTER, 0x29, 5);
                            }
                            write(card, CGA_COLOR_CONTROL_REGISTER, ((n * 7) as u8) & 0x3f, 0);
                            MemoryMappedDevice::mmio_write_u8(
                                card,
                                CGA_MEM_ADDRESS + (n * 83),
                                0x41 + n as u8,
                                0,
                                None,
                            );
                            card.light_pen_trigger((n * 3) as u32, (n * 2) as u32);
                        }
                        pending += u32::from(reference.mode_pending || reference.clock_pending);
                        restored = restore(&restored);
                        let frames = reference.frame_count();
                        for card in [&mut reference, &mut restored] {
                            card.run(
                                DeviceRunTimeUnit::SystemTicks([32003, 19013, 65539, 17011][n % 4]),
                                &mut None,
                                None,
                            );
                        }
                        swaps += u32::from(reference.frame_count() > frames);
                        snow += reference.snow_count;
                        compare(&mut reference, &mut restored);
                    }
                }
            }
        } //192 destructive continuation restores across24 configurations
        assert!(swaps > 0, "must see native buffer swaps");
        assert!(snow > 0, "must see actual native snow");
        assert!(pending > 0, "must checkpoint actual pending mode/clock changes");
    }

    #[test]
    fn cga_restore_continues_pending_native_snow_pixels() {
        let mut reference = configured(ClockingMode::Cycle, 0x29, false);
        for _ in 0..320003 {
            if reference.cycles & 15 == 7 && reference.char_col == 7 && reference.in_display_area {
                break;
            }
            reference.tick();
        }
        assert_eq!(reference.cycles & 15, 7, "native snow sampling phase");
        assert_eq!(reference.char_col, 7);
        assert!(reference.in_display_area && reference.mode_hires_txt);
        let count = reference.snow_count;
        MemoryMappedDevice::mmio_write_u8(&mut reference, CGA_MEM_ADDRESS, 0x4f, 0, None);
        assert!(reference.dirty_snow);
        let mut restored = restore(&reference);
        reference.run(DeviceRunTimeUnit::SystemTicks(9), &mut None, None);
        restored.run(DeviceRunTimeUnit::SystemTicks(9), &mut None, None);
        assert!(reference.snow_count > count, "must consume native snow latch");
        compare(&mut reference, &mut restored);
    }

    #[test]
    fn cga_restore_continues_native_pending_character_clock() {
        let mut reference = configured(ClockingMode::Cycle, 0x29, false);
        assert_ne!(reference.cycles & 15, 0);
        write(&mut reference, CGA_MODE_CONTROL_REGISTER, 0x28, 0);
        assert!(reference.clock_pending);
        assert_eq!(reference.clock_divisor, 1);
        let mut restored = restore(&reference);
        reference.run(DeviceRunTimeUnit::SystemTicks(32), &mut None, None);
        restored.run(DeviceRunTimeUnit::SystemTicks(32), &mut None, None);
        assert_eq!(reference.clock_divisor, 2, "native deferred LCLOCK transition");
        compare(&mut reference, &mut restored);
    }

    #[test]
    fn cga_restore_preserves_native_reset_aperture_phase() {
        let mut reference = configured(ClockingMode::Cycle, 0x29, false);
        write(&mut reference, CRTC_ADDRESS0, 2, 0);
        write(&mut reference, CRTC_DATA0, 91, 0);
        for _ in 0..4096 {
            if reference.hsync_phase == 1 {
                break;
            }
            reference.tick();
        }
        assert_eq!(reference.hsync_phase, 1, "must observe native odd hsync phase");
        assert_eq!(reference.display_extents().apertures[0].x, CGA_APERTURE_CROPPED_X - 8);
        reference.reset_private();
        assert_eq!(reference.hsync_phase, 0);
        assert_eq!(reference.display_extents().apertures[0].x, CGA_APERTURE_CROPPED_X - 8);
        let mut restored = restore(&reference);
        reference.run(DeviceRunTimeUnit::SystemTicks(32), &mut None, None);
        restored.run(DeviceRunTimeUnit::SystemTicks(32), &mut None, None);
        compare(&mut reference, &mut restored);
    }

    #[test]
    fn cga_schema_inventory_and_invalid_restores_are_atomic() {
        let mut card = configured(ClockingMode::Dynamic, 0x29, true);
        let saved = card.snapshot_state().unwrap();
        let mut shape = serde_json::to_value(&saved).unwrap();
        let source = include_str!("mod.rs");
        let body = source
            .split("pub struct CGACard {")
            .nth(1)
            .unwrap()
            .split("\n}")
            .next()
            .unwrap();
        let fields = regex::Regex::new(r"(?m)^\s+([a-z_][a-z0-9_]*):").unwrap();
        let mut native: std::collections::HashSet<_> = fields.captures_iter(body).map(|c| c[1].to_owned()).collect();
        assert!(native.remove("trace_logger"));
        native.insert("version".into());
        assert_eq!(native, shape.as_object().unwrap().keys().cloned().collect());
        // Lengths are a preflight obligation; small schema-only payloads avoid
        // copying half a megabyte for each required-key mutation.
        shape["mem"] = serde_json::json!([]);
        shape["buf"] = serde_json::json!([[], []]);
        for pointer in ["", "/extents", "/extents/apertures/0", "/rw_slots/0"] {
            for key in shape.pointer(pointer).unwrap().as_object().unwrap().keys() {
                let mut missing = shape.clone();
                missing
                    .pointer_mut(pointer)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
                assert!(
                    serde_json::from_value::<CgaState>(missing).is_err(),
                    "missing {pointer}/{key}"
                );
            }
            let mut extra = shape.clone();
            extra
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("unknown".into(), true.into());
            assert!(serde_json::from_value::<CgaState>(extra).is_err());
        }
        for field in ["rw_slots", "buf"] {
            for longer in [false, true] {
                let mut bad = shape.clone();
                let array = bad[field].as_array_mut().unwrap();
                if longer {
                    array.push(array[0].clone());
                } else {
                    array.pop();
                }
                assert!(serde_json::from_value::<CgaState>(bad).is_err());
            }
        }
        for (field, bad_value) in [
            ("version", 2),
            ("front_buf", 2),
            ("back_buf", 2),
            ("slot_idx", 5),
            ("cc_palette", 6),
            ("char_col", 8),
            ("hsync_phase", 2),
            ("clock_divisor", 0),
            ("cur_fg", 16),
            ("cur_bg", 16),
            ("cc_altcolor", 16),
            ("cc_overscan_color", 16),
            ("char_clock", 0),
            ("char_clock_mask", 0),
            ("char_clock_odd_mask", 0),
            ("front_buf_interlaced_frame_parity", 2),
            ("aperture", 4),
        ] {
            let mut value = serde_json::to_value(&saved).unwrap();
            value[field] = bad_value.into();
            value["cycles"] = 0.into();
            let bad = serde_json::from_value(value).unwrap();
            assert!(card.restore_state(&bad).is_err(), "invalid {field}");
            assert!(card.snapshot_state().unwrap() == saved, "atomic {field}");
        }
        for which in 0..10 {
            let mut bad = saved.clone();
            bad.cycles = 0;
            match which {
                0 => {
                    bad.mem.pop();
                }
                1 => {
                    bad.buf[0].pop();
                }
                2 => {
                    bad.buf[1].push(0);
                }
                3 => {
                    bad.extents.apertures.clear();
                }
                4 => {
                    bad.extents.row_stride = 0;
                }
                5 => {
                    bad.back_buf = bad.front_buf;
                }
                6 => {
                    bad.extents.apertures[1].w += 1;
                }
                7 => {
                    bad.extents.apertures[0].x = u32::MAX;
                }
                8 => {
                    bad.extents.double_scan = false;
                }
                _ => {
                    bad.extents.apertures[2].debug = true;
                }
            }
            assert!(card.restore_state(&bad).is_err(), "invalid storage {which}");
            assert!(card.snapshot_state().unwrap() == saved, "atomic storage {which}");
        }
        for pointer in [
            "/crtc/version",
            "/monitor/version",
            "/monitor/monitor/horizontal_pll/last_period_ticks",
        ] {
            let mut value = serde_json::to_value(&saved).unwrap();
            value["cycles"] = 0.into();
            *value.pointer_mut(pointer).unwrap() = if pointer.ends_with("last_period_ticks") {
                // PLL clocks are exact IEEE bits on the wire, not JSON floats.
                (-1.0f64).to_bits().into()
            } else {
                2.into()
            };
            assert!(
                card.restore_state(&serde_json::from_value(value).unwrap()).is_err(),
                "invalid nested {pointer}"
            );
            assert!(card.snapshot_state().unwrap() == saved, "atomic nested {pointer}");
        }
        for field in ["scanline_us", "frame_us", "accumulated_us"] {
            let mut value = shape.clone();
            value[field] = u64::MAX.into();
            assert!(serde_json::from_value::<CgaState>(value).is_err(), "nonfinite {field}");
        }
    }

    #[test]
    fn cga_exact_clock_and_legacy_slot_storage_roundtrip() {
        let mut card = CGACard::default();
        for (n, slot) in card.rw_slots.iter_mut().enumerate() {
            *slot = RwSlot {
                t: if n % 2 == 0 { RwSlotType::Io } else { RwSlotType::Mem },
                data: 23 + n as u8,
                addr: 0x3d0 + n as u32,
                tick: 17 + n as u32,
            };
        }
        card.slot_idx = 4;
        card.last_rw_tick = 123;
        card.scanline_us = f64::from_bits(0x3ff123456789abcd);
        card.frame_us = -0.0;
        card.accumulated_us = f64::from_bits(0x400123456789abcd);
        let restored = restore(&card);
        assert!(card.snapshot_state().unwrap() == restored.snapshot_state().unwrap());
        assert_eq!(card.frame_us.to_bits(), restored.frame_us.to_bits());
        assert_eq!(card.scanline_us.to_bits(), restored.scanline_us.to_bits());
        assert_eq!(card.accumulated_us.to_bits(), restored.accumulated_us.to_bits());
        // Legacy slots and unused microsecond fields are storage only, not
        // claimed native producers or a timing continuation.
    }

    #[test]
    fn cga_active_external_trace_refusals_preserve_live_card() {
        let mut card = configured(ClockingMode::Cycle, 0x29, true);
        let saved = card.snapshot_state().unwrap();
        let mut replacement = saved.clone();
        replacement.cycles = 0;
        replacement.mem[0] = 0;
        let directory = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "target".into());
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("cga-snapshot-trace-{}.log", std::process::id()));
        for logger in [TraceLogger::Console, TraceLogger::from_filename(&path)] {
            assert!(logger.is_some());
            card.trace_logger = logger;
            assert!(card.snapshot_state().is_err());
            assert!(card.restore_state(&replacement).is_err());
            card.trace_logger = TraceLogger::None;
            assert!(card.snapshot_state().unwrap() == saved);
        }
        for logger in [TraceLogger::Console, TraceLogger::from_filename(&path)] {
            assert!(logger.is_some());
            let original = std::mem::replace(&mut card.crtc, Crtc6845::new(logger));
            let observed = (card.crtc.ma(), card.crtc.ra(), card.crtc.hcc(), card.crtc.vcc());
            assert!(card.snapshot_state().is_err());
            assert!(card.restore_state(&replacement).is_err());
            assert_eq!(
                (card.crtc.ma(), card.crtc.ra(), card.crtc.hcc(), card.crtc.vcc()),
                observed
            );
            card.crtc = original;
            assert!(card.snapshot_state().unwrap() == saved);
        }
        std::fs::remove_file(path).unwrap();
    }
}
