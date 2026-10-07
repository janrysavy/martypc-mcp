use super::*;

vga_state!(VGACard, VgaState, {trace_logger: TraceLogger::None,}, {
    debug: bool => copy,
    debug_draw: bool => copy,
    dip_sw: DipSwitch => copy,
    #[serde(with = "crate::snapshot_codec::f64_bits")]
    ticks_accum: f64 => copy,
    clock_mode: ClockingMode => copy,
    cycles: u64 => copy,
    io_adjust: u16 => copy,
    mode_byte: u8 => copy,
    display_mode: DisplayMode => copy,
    mode_enable: bool => copy,
    mode_graphics: bool => copy,
    mode_bw: bool => copy,
    mode_line_gfx: bool => copy,
    mode_hires_gfx: bool => copy,
    mode_hires_txt: bool => copy,
    mode_blinking: bool => copy,
    scanline: u32 => copy,
    frame: u64 => copy,
    #[serde(with = "crate::snapshot_codec::f32_bits")]
    scanline_cycles: f32 => copy,
    #[serde(with = "crate::snapshot_codec::f32_bits")]
    frame_cycles: f32 => copy,
    cursor_frames: u32 => copy,
    raster_x: u32 => copy,
    raster_y: u32 => copy,
    cur_char: u8 => copy,
    next_char: u8 => copy,
    cur_attr: u8 => copy,
    next_attr: u8 => copy,
    cur_fg: u8 => copy,
    cur_bg: u8 => copy,
    cur_blink: bool => copy,
    blink_state: bool => copy,
    cursor_status: bool => copy,
    cursor_slowblink: bool => copy,
    cursor_blink_rate: u32 => copy,
    cursor_attr: u8 => copy,
    crtc: crtc::VgaCrtcState => (|c: &VGACard| c.crtc.snapshot_state(), |s: &VgaState| -> Result<VgaCrtc, &'static str> { Ok(VgaCrtc::prepare_state(&s.crtc)?) }),
    vma: usize => copy,
    sequencer: sequencer::SequencerState => (|c: &VGACard| c.sequencer.snapshot_state(), |s: &VgaState| -> Result<Sequencer, &'static str> { Ok(Sequencer::prepare_state(&s.sequencer)?) }),
    gc: graphics_controller::GraphicsState => (|c: &VGACard| c.gc.snapshot_state(), |s: &VgaState| -> Result<GraphicsController, &'static str> { Ok(GraphicsController::prepare_state(&s.gc)?) }),
    ac: attribute_controller::AttributeState => (|c: &VGACard| c.ac.snapshot_state(), |s: &VgaState| -> Result<AttributeController, &'static str> { Ok(AttributeController::prepare_state(&s.ac)?) }),
    pel_pan_latch: u8 => copy,
    current_font: u8 => copy,
    misc_output_register: u8 => (|c: &VGACard| c.misc_output_register.into_bytes()[0], |s: &VgaState| -> Result<EMiscellaneousOutputRegister, &'static str> { Ok(EMiscellaneousOutputRegister::from_bytes([s.misc_output_register])) }),
    back_buf: usize => copy,
    front_buf: usize => copy,
    extents: DisplayExtents => copy,
    aperture: usize => copy,
    buf: [Vec<u32>; 2] => (|c: &VGACard| [c.buf[0].to_vec(), c.buf[1].to_vec()], |s: &VgaState| -> Result<[Box<[u32; VGA_MAX_CLOCK28]>; 2], &'static str> { Ok([s.buf[0].clone().into_boxed_slice().try_into().map_err(|_| "VGA raster length")?, s.buf[1].clone().into_boxed_slice().try_into().map_err(|_| "VGA raster length")?]) }),
    rba: usize => copy,
    hblank_color: u8 => copy,
    vblank_color: u8 => copy,
    disable_color: u8 => copy,
    hsync_ct: u64 => copy,
    vsync_ct: u64 => copy,
    intr: bool => copy,
    last_intr: bool => copy,
    feature_bits: u8 => copy,
    gc_debug: [u8; 8] => copy,
});

impl VGACard {
    fn validate_state(s: &VgaState) -> Result<(), &'static str> {
        if s.front_buf > 1 || s.back_buf > 1 || s.front_buf==s.back_buf
            || s.buf.iter().any(|b| b.len()!=VGA_MAX_CLOCK28)
            || s.aperture>=s.extents.apertures.len()
            || s.extents.apertures.len()!=VGA_APERTURES[0].len()
            || s.extents.field_w==0 || s.extents.field_h==0
            || s.extents.row_stride != s.extents.field_w as usize
            || (s.extents.field_w as usize).checked_mul(s.extents.field_h as usize)
                .map_or(true, |area| area > VGA_MAX_CLOCK28)
            || s.rba > VGA_MAX_CLOCK28
            || s.raster_x > s.extents.field_w || s.raster_y > s.extents.field_h
            || s.extents.apertures.iter().any(|a|
                a.w>s.extents.field_w || a.h>s.extents.field_h
                || a.x.checked_add(a.w).is_none() || a.y.checked_add(a.h).is_none())
            || !s.ticks_accum.is_finite() || !s.scanline_cycles.is_finite() || !s.frame_cycles.is_finite() {
            return Err("VGA raster/configuration/clock");
        }
        VgaCrtc::prepare_state(&s.crtc)?;
        Sequencer::prepare_state(&s.sequencer)?;
        GraphicsController::prepare_state(&s.gc)?;
        AttributeController::prepare_state(&s.ac)?;
        Ok(())
    }
    pub(crate) fn capture_state(&self) -> Result<VgaState, &'static str> {
        if self.trace_logger.is_some() { return Err("active VGA trace snapshots unsupported"); }
        let saved=self.snapshot_state();
        Self::validate_state(&saved)?;
        Ok(saved)
    }
    pub(crate) fn preflight_state(&self, saved: &VgaState) -> Result<(), &'static str> {
        if self.trace_logger.is_some() { return Err("active VGA trace snapshots unsupported"); }
        Self::validate_state(saved)
    }
    pub(crate) fn restore_state(&mut self, saved: &VgaState) -> Result<(), &'static str> {
        self.preflight_state(saved)?;
        *self=Self::prepare_state(saved)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_vga_owner_roundtrip_and_mutations() {
        let mut card=VGACard::default();
        card.ticks_accum=f64::from_bits(0x3fa5555555555555);
        card.scanline_cycles=f32::from_bits(0x3eaaaaab);
        card.buf[0][42]=0x12345678;
        card.sequencer.vram.write_u8(2,123,0xa5);
        card.sequencer.write_address(2); card.sequencer.write_data(4);
        card.ac.write_attribute_register(0x14); card.ac.write_attribute_register(0x0f);
        let saved=card.capture_state().unwrap();
        let wire=serde_json::to_value(&saved).unwrap();
        let loaded: VgaState=serde_json::from_value(wire.clone()).unwrap();
        let restored=VGACard::prepare_state(&loaded).unwrap();
        assert_eq!(saved, restored.capture_state().unwrap());
        assert_eq!(restored.sequencer.vram.read_glyph(123),0xa5);
        assert_eq!(restored.buf[0][42],0x12345678);
        for name in wire.as_object().unwrap().keys() {
            let mut bad=wire.clone();bad.as_object_mut().unwrap().remove(name);
            assert!(serde_json::from_value::<VgaState>(bad).is_err(),"required {name}");
        }
        for mutant in 0..10 {
            let mut bad=saved.clone();
            match mutant {
                0=>bad.buf[0].pop().map(|_| ()).unwrap(),
                1=>bad.front_buf=bad.back_buf,
                2=>bad.sequencer.char_clock=7,
                3=>bad.ac.color_registers.pop().map(|_| ()).unwrap(),
                4=>bad.rba=usize::MAX,
                5=>bad.raster_y=u32::MAX,
                6=>bad.extents.apertures[0].x=u32::MAX,
                7=>bad.ac.overscan_color.six=64,
                8=>bad.sequencer.font_offset_a=0x2000,
                _=>{bad.extents.field_w=65536;bad.extents.field_h=65536;bad.extents.row_stride=65536;},
            }
            assert!(card.restore_state(&bad).is_err());
            assert_eq!(saved,card.capture_state().unwrap(),"mutation was atomic");
        }
    }
    #[test]
    fn incomplete_aperture_is_refused_without_subtraction() {
        let mut a=crtc::CrtcAperture {left:100,right:50,top:10,bottom:5};
        assert!(!a.is_compatible_with((900,600)));
        a.right=200;a.bottom=20;assert!(a.is_compatible_with((900,600)));
        a.right=901;assert!(!a.is_compatible_with((900,600)));
    }
    #[test]
    fn attribute_pas_bit_keeps_palette_address_and_color_select_works() {
        let mut ac=AttributeController::default();
        ac.write_attribute_register(0x25); ac.write_attribute_register(0x12);
        assert_eq!(ac.palette_registers[5].six,0x12);
        assert_eq!(ac.palette_registers[0].six,0);
        ac.write_attribute_register(0x14); ac.write_attribute_register(0x0f);
        ac.write_attribute_register(0x14);assert_eq!(ac.read_attribute_register(),0x0f);
    }
}

#[cfg(test)]
mod read_tests {
    use super::*;
    use crate::bus::MemoryMappedDevice;
    #[test]
    fn peeks_match_native_reads_without_changing_latches_or_pipelines() {
        let mut card=VGACard::default();
        card.misc_output_register.set_enable_ram(true);
        for (reg,value) in [(6,4),(4,2)] { card.gc.write_address(reg);card.gc.write_data(value); }
        card.sequencer.vram.write_u8(2,123,0xa5);
        let before=card.capture_state().unwrap();
        assert_eq!(card.mmio_peek_u8(0xa0000+123,None),0xa5);
        assert_eq!(card.capture_state().unwrap(),before);
        assert_eq!(card.mmio_read_u8(0xa0000+123,0,None).0,0xa5);
        for (reg,value) in [(5,8),(2,4),(7,4)] {card.gc.write_address(reg);card.gc.write_data(value);}
        let before=card.capture_state().unwrap();
        assert_eq!(card.mmio_peek_u8(0xa0000+123,None),0xa5);
        assert_eq!(card.capture_state().unwrap(),before);
        assert_eq!(card.mmio_read_u8(0xa0000+123,0,None).0,0xa5);
    }
    #[test]
    fn extended_font_map_bit_enables_attribute_selection() {
        let mut seq=Sequencer::default();seq.write_address(3);seq.write_data(0x20);
        assert!(seq.font_select_enabled());
        seq.vram.write_u8(2,0x2000,0x80);seq.vram.write_u8(2,0,0x01);
        assert_eq!(seq.get_glyph_span(0,1,0),tablegen::BIT_EXTEND_TABLE64[0x80]);
        assert_eq!(seq.get_glyph_span(0,0,0),tablegen::BIT_EXTEND_TABLE64[0x01]);
    }
    #[test]
    fn native_character_ticks_select_font_and_remove_selection_intensity() {
        for halfclock in [false,true] {
            for (attribute,glyph) in [(1,0x01),(9,0x80)] {
                let mut card=VGACard::default();
                card.sequencer.write_address(3);card.sequencer.write_data(0x20);
                card.sequencer.vram.write_u8(2,0,0x01);
                card.sequencer.vram.write_u8(2,0x2000,0x80);
                card.ac.palette_registers[1].set(1);
                card.cur_attr=attribute;card.crtc.status.den=true;
                let mut expected=AttributeController::default();
                expected.palette_registers[1].set(1);
                expected.load(AttributeInput::Parallel64(tablegen::BIT_EXTEND_TABLE64[glyph],0,1,false),ClockSelect::Clock25,true);
                if halfclock {card.tick_lchar(ClockSelect::Clock25);} else {card.tick_hchar(ClockSelect::Clock25);}
                assert_eq!(card.ac.snapshot_state().shift_reg,expected.snapshot_state().shift_reg);
                assert_eq!(card.ac.snapshot_state().shift_reg9,expected.snapshot_state().shift_reg9);
            }
        }
    }
}

#[cfg(test)]
mod geometry_tests {
    use super::*;

    // Registers captured from original Pyro mode1 after its custom 8-dot font
    // load. Device counters, font pixels and raster products are freshly made.
    fn text_card(clocking: u8) -> VGACard {
        let params: serde_json::Value = serde_json::from_str(include_str!("text_geometry_registers.json")).unwrap();
        let mut card = VGACard::default();
        let mut crtc = serde_json::to_value(card.crtc.snapshot_state()).unwrap();
        for (key, value) in params["crtc"].as_object().unwrap() { crtc[key] = value.clone(); }
        card.crtc = VgaCrtc::prepare_state(&serde_json::from_value(crtc).unwrap()).unwrap();
        card.misc_output_register = EMiscellaneousOutputRegister::from_bytes([params["misc_output_register"].as_u64().unwrap() as u8]);
        card.sequencer.write_address(1); card.sequencer.write_data(clocking);
        card.sequencer.write_address(2); card.sequencer.write_data(params["sequencer"]["map_mask"].as_u64().unwrap() as u8);
        card.sequencer.write_address(3); card.sequencer.write_data(params["sequencer"]["character_map_select"].as_u64().unwrap() as u8);
        card.sequencer.write_address(4); card.sequencer.write_data(params["sequencer"]["memory_mode"].as_u64().unwrap() as u8);
        card.gc.write_address(5); card.gc.write_data(params["gc"]["graphics_mode"].as_u64().unwrap() as u8);
        card.gc.write_address(6); card.gc.write_data(params["gc"]["graphics_micellaneous"].as_u64().unwrap() as u8);
        card.ac.palette_registers[15].set(63);
        // Distinct glyphs per column, with outside pixels set. A whole-cell
        // shift cannot accidentally agree with another identical text cell.
        for at in 0..65536 {
            card.sequencer.vram.write_u8(0, at, ((at / 2) % 40 + 1) as u8);
            card.sequencer.vram.write_u8(1, at, 15);
        }
        for glyph in 1..=40 {
            for y in 0..16 { card.sequencer.vram.write_u8(2, glyph*32+y, ((glyph*37) as u8)|0x81); }
        }
        card
    }

    #[test]
    fn cropped_text_matches_native_glyphs_without_extra_columns() {
        for clocking in [1, 9, 0, 8] {
            let mut card = text_card(clocking);
            let mut pic = None;
            for _ in 0..150000 {
                card.tick(card.sequencer.char_clock as f64 + 0.001, &mut pic);
                if card.frame >= 3 { break; }
            }
            assert!(card.frame >= 3, "native VGA must complete frames");
            let dots = card.sequencer.char_clock;
            let aperture = &card.extents.apertures[DisplayApertureType::Cropped as usize];
            assert_eq!(aperture.w, 40 * dots, "clocking {clocking}");
            assert_eq!(aperture.h, 400);
            let raw = card.crtc.status.dynamic_aperture;
            let expected_origin = raw.left + 3*dots;
            assert_eq!(aperture.x, expected_origin, "native origin, clocking {clocking}");
            let white = card.ac.color_registers_u32[63];
            let black = card.ac.color_registers_u32[0];
            let repeat = card.sequencer.clock_divisor;
            let expected: Vec<_> = (1..=40).flat_map(|glyph| {
                let bitmap = ((glyph*37) as u8)|0x81;
                (0..dots/repeat).flat_map(move |bit| {
                    std::iter::repeat(if bit<8 && bitmap & (0x80>>bit)!=0 {white} else {black}).take(repeat as usize)
                })
            }).collect();
            let row = ((raw.top+7) as usize)*card.extents.row_stride + expected_origin as usize;
            assert_eq!(&card.buf[card.front_buf][row..row+expected.len()], expected, "native crop origin, clocking {clocking}");
            assert_eq!(card.buf[card.front_buf][row-1], black);
            assert_eq!(card.buf[card.front_buf][row+expected.len()], black);
            card.update_clock();
            assert_eq!(card.crtc.status.dynamic_aperture, raw, "crop must not rewrite raw CRTC bounds");
        }
    }

    #[test]
    fn incomplete_text_boundaries_keep_the_last_nonempty_crop() {
        for raw in [crtc::CrtcAperture::default(),
            crtc::CrtcAperture {left:100,right:50,top:10,bottom:5}] {
            let mut card = text_card(9);
            card.update_clock();
            let before = card.extents.apertures[DisplayApertureType::Cropped as usize].clone();
            card.crtc.status.dynamic_aperture = raw;
            card.update_clock();
            let after = &card.extents.apertures[DisplayApertureType::Cropped as usize];
            assert_eq!((after.x,after.y,after.w,after.h), (before.x,before.y,before.w,before.h));
            assert_eq!(card.crtc.status.dynamic_aperture,raw);
        }
    }
}
