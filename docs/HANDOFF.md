# Fork handoff

NEXT: complete restartable snapshots before further Pyro timing research.
No complete machine snapshot or process-restart continuation proof exists.
Contract and launch/input rules: [JSON_RPC_API.md](JSON_RPC_API.md).

INTEGRATED: PR6 native Rust/wgpu UI at e6c3fcc2 and PR7 prefetch queue at
8baf250a after final Windows/Linux CI. RPC and UI share one Machine;
local keyboard, hotkeys, joystick mappings, mouse/light pen, gamepads and
focus-loss clearing default off. --local-input explicitly permits manual input.
All38 debugger/disk/config tests and fresh Windows builds pass. Queue tests
cover4000 JSON restores; policy review fixes pass all184 core tests.
The parent retains bounded native UI and queue evidence. Live host-key
injection/audio/pixel equality remain OPEN.

PR8 BIU component at65cac7f6: all187 core tests pass, with2000 JSON restores
executing subsequent native cycles against an untouched reference. Independent
corruption and five actual omitted-restore controls catch lost T-state, queue
storage, either policy and discard. Follow-up found no production defect;
final Windows/Linux CI is pending. Device/wait-state scenarios remain OPEN.

WIP CPU component: versioned CPU/BIU state includes registers, decoded
instruction, REP/HLT/interrupt/DMA scheduling, clock bits, native debugger
state and pending service events. Four tests and all191 core tests pass.
384 JSON restores at EU/RNI boundaries continue native8088/8086 probes with
identical CPU state and low64KiB RAM. Five actual omissions (AX, cycle count,
REP, NMI and decoded instruction) fail; exact source restoration passes191.
The test destroys the CPU independently and retains its owned bus, so it does
NOT prove bus/device/disk restoration or process restart. Active RNG, traces,
listings/analyzers and validator/cycle-collector builds are explicitly refused.
CPU review and final-head CI/integration remain pending.

NEXT: owned bus/RAM, timers/devices/video/audio/input, disks and pending host
I/O, with atomic dependency preflight, then real process-restart continuation.
Hardware trace,VNC,keyboard/serial RPC and physical XT/gameplay parity remain
OPEN. Controlled BIOS-ring input is not hardware IRQ input.

CI: Windows/Linux debugger/core tests and native builds; no artifacts/caches.
macOS/WASM are manual. Native work uses cargo +1.98.0 with CARGO_HOME,
CARGO_TARGET_DIR and TEMP/TMP under the parent's ignored re/_build tree.
