# Fork handoff

FINISHED locally: guarded RPC writes now recognize installed writable RAM
expansions, using active bus mapping and exact device bounds. Three new tests
bring the native suite to 25 passing tests: actual CPU/RPC mailbox word exchange,
absent/boundary/hash/ROM/running refusals, and read-only/card extent checks.
Fresh Windows headless build passes. No implicit upper-memory allocation and
no video/EMS/ROM write permission. Parent's unchanged PyPC DOSCTRL client has
uploaded/read/renamed/deleted files, captured stdout/stderr and child exit7,
and freshly compiled a TP6 unit on a writable VHD. Deleting that TPU before a
syntax-error compile leaves no stale product. These are bounded parent receipts,
not full snapshot/gameplay proof. Focused source review found no defects;
final-head Windows/Linux CI remains pending before integration.

FINISHED: limited native headless JSON-lines debugger; see
[JSON_RPC_API.md](JSON_RPC_API.md) for the supported PyPC-compatible methods and
explicit gaps. Twenty-two native tests cover direct/RPC CPU+RAM+PIT equality, guarded
writes, breakpoint resumes, private predicates, turbo-independent deadlines,
operation retention and persistent TCP framing. Windows local build passes.
The parent probe uses the unchanged PyPC client against the real executable.
Initial review completed; fixed stale breakpoint stop after stepping and
clarified capability/session metadata. PR1 is integrated after focused review
and passing final-head Windows/Linux checks.
Follow-up confirms step-stop/PPI fixes; unsupported metadata is expanded.
Execution length remains exact-address metadata as in PyPC, tested explicitly;
the review's range-matching objection does not describe the shared contract.
Initial Linux CI exposed missing libudev development files used by existing
host serial enumeration; the workflow now installs that dependency.
Live alias probes exposed rejection of default execution kind and a false
segmented alias hit. Both are fixed with native and parent client regressions.
Flag text matches PyPC's trap-bit order. PPI software-turbo configurations are
refused until frame housekeeping is supported, rather than running incorrectly.
Review's missing-config-key and halt-hang claims are disproved by fresh config
parsing and 100 CLI/HLT steps under each Continue/Warn/Stop policy. See tests.

FINISHED locally: headless configured disks dispatch to Xebec/XT-IDE/Jr-IDE,
preserve drive-slot holes and fail startup on load/parse/controller errors.
Native slave IDENTIFY confirms attachment; missing/unsupported disks and excess
slots fail. Headless frontend now enables matching EGA/VGA ROM requirements.
Paused parent startup reproduced missing XT-IDE attachment and VGA BIOS before
these fixes. PR2 is integrated after review and final-head Windows/Linux CI.
The review found main-config drive numbers were ignored; explicit indexed
overrides now preserve sparse/out-of-order entries and reject duplicate/excess
slots. The parent is exercising real startup failures and bounded DOS boot.

FINISHED locally: RPC continue versus normal batched native Run, including
PIT IRQ0, REP copies and inspection at every boundary. Twelve comparisons agree
on CPU/prefetch, clocks, PIC, PIT and low32KiB RAM; 366 copies/293 IRQ entries
execute in300144 CPU cycles. All21 tests pass. PR3 is integrated at5026107d
after focused review and Windows/Linux CI; parent retains the probe receipt.
This closes a test gap, not every possible scheduling defect.

The parent workspace boots MS-DOS6.22/original Pyro tutorial and menu with its
pinned patched GLaBIOS and CGA; loaded code matches, CRT calibration103 observed.
Automatic disk selection still fails. Hardware/instruction trace, VNC,
keyboard/serial, complete snapshots and DOSCTRL remain unsupported. Controlled
BIOS-ring input bypasses hardware IRQs. Next: observation/input for comparable
original-game timing runs. No physical XT or full gameplay parity claim.

CI is Windows/Linux headless tests+build, no artifacts/caches. macOS/WASM are
manual. Local Pyro workspace uses installed Rust1.98.0 via `cargo +1.98.0`, with
CARGO_HOME/TARGET_DIR and TEMP/TMP inside its ignored `re/_build/`.

FINISHED locally: `input.joystick`/`input.joystick.state` expose the configured
native game port. Complete normalized axes/buttons are atomically validated;
tests check absent card, invalid late fields, paused-only writes, active-low
buttons and both charge durations. Clock unchanged by injection. Keyboard is
still unsupported. Shared PyPC implementation and actual guest OUT/IN201h TCP
probes now pass; the parent retains the receipts. PyPC live ZIP restore during
charge passes too. Focused review's Y-sign objection is false: RPC passes -y
to the native frontend setter, which negates it into potentiometer position y.
The actual guest at200us reads EEh for x=-1/y=+1 (X expired, Y charging),
then E0h after1.5ms. This is not merely an echoed input value.
Physical timing and Pyro calibration/gameplay remain unproven. Final-head CI
and linear integration are tracked by PR4; parent records the integrated pins.
