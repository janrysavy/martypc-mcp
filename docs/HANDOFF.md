# Fork handoff

NEXT: complete restartable snapshots before further Pyro timing research.
No complete CPU/machine snapshot or process-restart continuation proof exists.
Contract and launch/input rules: [JSON_RPC_API.md](JSON_RPC_API.md).

PR6 is locally tested and independently reviewed: native Rust/wgpu UI and
headless share one Machine debugger. RPC replaces the normal GUI runner.
Local keyboard/hotkeys/joystick mappings, mouse/light pen, gamepads and
focus-loss clearing default off; --local-input explicitly permits manual input.
Frontend worker mutations and GUI execution controls cannot take ownership.
All38 debugger/disk/config tests and fresh Windows builds pass. A live GUI/peer
receipt agrees over paused reads,64 BIOS boundaries,RAM/CGA/ROM and5ms execution.
The bounded64-request queue/soft8ms pump has a flood regression rejecting the
old full-batch drain. Live host-key injection/audio/pixel equality remain OPEN.
Final-head CI is pending; do not assume PR6 is integrated yet.

PR7 is a tested internal queue component, not a whole snapshot: version1
ring/stale storage/preload/discard/fetch policies, with complete preflight.
Four tests include4000 JSON restores/continuations and invalid-field refusals.
Reachable-policy review fixes pass all184 core tests. Follow-up found no
production defect; existing tests disprove its Default/wrap objections.
Discard is currently never read; only storage preservation is proven.
Final-head CI/integration is pending.

PR8 adds the internal BIU component: queue/address/data/8288 pins/fetch PC,
T/TA/pipeline/READY/wait/transfer state. All187 core tests pass;1000 JSON restores
per8088/8086 execute two subsequent native cycles against an untouched reference.
The first negative control exposed destruction via the restorer masking an
omitted assignment. Independent corruption now rejects lost T-state, physical
queue storage, either policy field and discard restorations in five actual
mutation runs. Exact source restoration returns187 tests green. Follow-up found
no production defect; its queue-coverage concern is fixed and tested, without
another review. transfer0 is native idle/reset; the old field comment is fixed.
EU/registers, interrupt/DMA/clocks, owned bus/devices/disks and restart proof
remain WIP. Wait-state/device scenarios are not yet dynamically proven.
PR8 final-head CI is pending. Rebase/integrate stacked PRs in6,7,8 order.

Integrated baseline48cd3cdb provides bounded headless RPC, configured disks,
guarded mailbox RAM, joystick control and actual DOSCTRL/TP6 compilation.
The parent retains receipts under docs/evidence/martypc_*_20261004 and
joystick_rpc_20261004, including all original startup/compiler limitations.
Hardware trace,VNC,keyboard/serial RPC and physical XT/gameplay parity remain
OPEN. Controlled BIOS-ring input is not hardware IRQ input.

CI: Windows/Linux debugger/core tests and native builds; no artifacts/caches.
macOS/WASM are manual. Native Pyro work uses cargo +1.98.0 with CARGO_HOME,
CARGO_TARGET_DIR and TEMP/TMP under the parent's ignored re/_build tree.
