# Fork handoff

FINISHED: limited native headless JSON-lines debugger; see
[JSON_RPC_API.md](JSON_RPC_API.md) for the supported PyPC-compatible methods and
explicit gaps. Ten native tests pass: direct/RPC CPU+RAM+PIT equality, guarded
writes, breakpoint resumes, private predicates, turbo-independent deadlines,
operation retention and persistent TCP framing. Windows local build passes.
The parent probe uses the unchanged PyPC client against the real executable.
Independent review and remote CI are pending at this checkpoint.

WIP: DOS/Pyro boot, hardware/instruction trace, VNC, input/serial, snapshots and
DOSCTRL are not implemented or validated. Do not infer them from transport
compatibility. No game timing claim yet. Next: establish a scratch DOS boot with
the required disks/ROMs, then observe original CRT calibration with native
device timing; extend only the control/observation gaps that this exposes.

CI is Windows/Linux headless tests+build, no artifacts/caches. macOS/WASM are
manual. Local Pyro workspace uses installed Rust1.98.0 via `cargo +1.98.0`, with
CARGO_HOME/TARGET_DIR and TEMP/TMP inside its ignored `re/_build/`.
