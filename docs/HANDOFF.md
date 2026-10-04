# Fork handoff

FINISHED: limited native headless JSON-lines debugger; see
[JSON_RPC_API.md](JSON_RPC_API.md) for the supported PyPC-compatible methods and
explicit gaps. Fifteen native tests pass: direct/RPC CPU+RAM+PIT equality, guarded
writes, breakpoint resumes, private predicates, turbo-independent deadlines,
operation retention and persistent TCP framing. Windows local build passes.
The parent probe uses the unchanged PyPC client against the real executable.
Initial review completed; fixed stale breakpoint stop after stepping and
clarified capability/session metadata. Focused follow-up and final-head CI pending.
Initial Linux CI exposed missing libudev development files used by existing
host serial enumeration; the workflow now installs that dependency.
Live alias probes exposed rejection of default execution kind and a false
segmented alias hit. Both are fixed with native and parent client regressions.
Flag text matches PyPC's trap-bit order. PPI software-turbo configurations are
refused until frame housekeeping is supported, rather than running incorrectly.
Review's missing-config-key and halt-hang claims are disproved by fresh config
parsing and 100 CLI/HLT steps under each Continue/Warn/Stop policy. See tests.

WIP: DOS/Pyro boot, hardware/instruction trace, VNC, input/serial, snapshots and
DOSCTRL are not implemented or validated. Do not infer them from transport
compatibility. No game timing claim yet. Next: establish a scratch DOS boot with
the required disks/ROMs, then observe original CRT calibration with native
device timing; extend only the control/observation gaps that this exposes.

CI is Windows/Linux headless tests+build, no artifacts/caches. macOS/WASM are
manual. Local Pyro workspace uses installed Rust1.98.0 via `cargo +1.98.0`, with
CARGO_HOME/TARGET_DIR and TEMP/TMP inside its ignored `re/_build/`.
