# Native VGA text crop regression

Original Pyro mode1 loads 8-dot glyphs at a half pixel clock: 40 text cells
occupy 640 native raster pixels. In the observed scene the first image pixel
is at x128, while the old Cropped aperture starts at112 and retains the
Clock28 preset width720. Thus it includes exactly16 left and64 right surplus
pixels: one and four character cells. Independent plane2 font/VRAM/palette
rendering matches five complete top-row scanlines at x128 with zero mismatches.
This display path became visible when the fork enabled VGA for Pyro; the
fixed-width aperture and insufficient 8-dot text latency were inherited VGA
code. Snapshot support itself is not a claim of correct screen geometry.

The crop now uses the nonempty measured text extent and includes the native
character-fetch/attribute-controller latency. A local copy preserves raw CRTC
boundaries; empty or reversed boundaries cannot replace a usable crop. Graphics
crop sizing and Accurate/Full preset widths retain their existing policy.
The existing vertical sync height clamp is retained. No CPU/IRQ timing or guest
RAM behavior is changed intentionally.

`text_geometry_registers.json` retains the original programmed register values,
without live counters or captured raster products. Native tests execute fresh
frames with distinct generated glyphs per column at all four8/9-dot native/half
clock settings. They compare every pixel in a full row and both outside edges,
check320/360/640/720 widths, and check raw-boundary ownership. A second test
exercises empty/reversed startup bounds. These are native rendering regressions,
not physical VGA timing or all-mode/panning proof.

```powershell
cargo +1.98.0 test -p marty_core --no-default-features --features vga,ega --lib --locked
```

The old source fails the native width test (720 instead of320 in its first
8-dot case). The initial review found an empty-boundary bug in the correction;
the guard and independent column/edge checks were added. Corrected review's
sync-clamp finding is fixed with a separate350-line control and a test that
invalid bounds preserve a previously measured dynamic crop. All339 tests pass.
The measured original Pyro scene has pel panning0; captures with nonzero panning
are outside this witness. Fresh GUI/original-game validation, publication of
the original raster packet and final-head Windows/Linux CI are required before
integration.
