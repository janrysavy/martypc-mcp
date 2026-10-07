# VGA text palette lookup

Pyro's original attribute06h stair stripes program palette entry6 to14h.
The old text path mapped6->14h in apply_attribute, then the rasterizer
looked up14h's low nibble4 again, producing dark red instead of brown.
An independent original CHA/VRAM/DAC rendering found10752 wrong pixels
on42 gameplay cells; the640x400 crop itself was correct.
The frozen witness is in the parent repository:
https://github.com/janrysavy/pyro221_next/tree/6ed5a165/docs/evidence/gameplay_vga_raster_20261007

Text glyph masks now carry logical foreground/background attribute indices
until palette_lookup. That function applies the programmed six-bit palette,
P54S override and Color Select DAC-bank bits once. The oscillator controls
pixel timing, not an EGA palette conversion. Color Select bits3:2 supply
DAC address bits7:6 with either value of P54S.

The native regression completes fresh VGA frames with distinct glyphs,
all16 foreground/background indices, a nonidentity palette and distinct
DAC markers. It covers8/9-dot normal/half clocks at both oscillators and
four P54S/bank configurations:32 complete cases. Expected pixels are derived
directly from register bit fields, not palette_lookup. The old code fails.
Use32 visible cells to fit both native raster buffers: the initial40-cell
9-dot half-clock25MHz fixture exceeded the800-pixel buffer and fell back
to an old crop. The corrected fixture writes horizontal timing through
the CRTC API:44 character clocks keep even18-dot lines within800 pixels;
oscillator selection uses the external-register write path. That failed
fixture was corrected rather than calling it a second palette defect.
Existing40-cell crop tests remain unchanged. The diagnostic attribute
palette also uses the renderer's actual palette/Color Select/DAC lookup.

```powershell
cargo +1.98.0 test -p marty_core --no-default-features --features vga,ega --lib --locked
```

Fresh native GUI/original gameplay raster and cold replay are still WIP.
The fix does not establish general VGA timing, pel panning, blinking,
overscan or all graphics paths. Snapshot files still require their exact
native executable; preserve older checkpoints and never bypass that guard.

Review identified two preexisting gaps outside this text correction:
Border input emits a six-bit overscan value into a pipeline whose rasterizer
interprets logical palette indices; nonidentity palette0/overscan/banks need
an explicit border-path correction and independent frame tests. PEL Mask
is stored but not applied to the DAC address. Pyro's measured mask isFFh,
overscan0 and palette0=0; broader settings remain open, not silently passed.
