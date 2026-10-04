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
final Windows/Linux CI passed; PR8 is integrated on main. Device/wait-state
scenarios remain OPEN.

INTEGRATED CPU PR9 at14566051: versioned CPU/BIU state includes registers, decoded
instruction, REP/HLT/interrupt/DMA scheduling, clock bits, native debugger
state and pending service events. Six tests and all193 core tests pass.
384 JSON restores at EU/RNI boundaries continue native8088/8086 probes with
identical CPU state and low64KiB RAM. Five actual omissions (AX, cycle count,
REP, NMI and decoded instruction) fail; exact source restoration passes193.
The test destroys the CPU independently and retains its owned bus, so it does
NOT prove bus/device/disk restoration or process restart. Active RNG, traces,
listings/analyzers and validator/cycle-collector builds are explicitly refused.
Review test-gate defect is fixed for cycle collectors. Both native CPUs accept
a17-byte prefixed NOP and continue identically after restore, disproving the
proposed15-byte rejection. All11 active host facilities are refused. Removing
a BIU field from its macro makes the native storage inventory fail; that review
coverage objection is disproved too. Five restore omissions and this sixth
inventory control fail, then193 core tests pass. All188 collector-feature tests
pass, including explicit snapshot refusal. Focused follow-up found no defect;
final Windows/Linux CI passed; integrated linearly, branch deleted.

INTEGRATED shared bus-memory PR10 at177c5876: versioned1MiB backing bytes, protection/
debugger masks, descriptors and decode cursor. Fixed ROM/MMIO layout is
preflighted. Two tests and all195 rebased core tests pass; four actual
restore omissions (memory/mask/cursor/descriptors) fail. Independent review
found no defect within this scope. Device-owned memory, timers/disks and
restart remain excluded. Final Windows/Linux CI passed; integrated linearly,
source branch deleted.

INTEGRATED PIT PR11 at1a7fb6f1: all three channels, latches/partial I/O/gates, clock phase,
timewarp, dirty markers and exact pending speaker sample bits are serialized.
Five tests and200 core tests pass.2048 destructive JSON continuation restores
cover all6 modes on8253/8254, every channel's latched partial reads through actual PIT ports, native I/O/PPI gates,
PIC observations and newly emitted PCM bits. A seeded legacy FIFO consumer
probe checks emitted output before state equality; normal producer stays empty.
Five real restore omissions and reversed FIFO order fail; the latter changes
emitted PCM. Exact source restoration returns200 green. Review coverage gaps
are fixed; final follow-up found no PIT-owned restore defect. Native writes
exercise catch-up; reads currently ignore elapsed delta, so read-side catch-up
and physical hardware timing are NOT proven. That documentation limit is fixed. The first review accidentally
captured a deliberate mutation and is NOT a production review. All38 RPC/config
tests and fresh Windows UI/headless builds pass; later changes affect tests only.
External emitted audio queues, PIC/DMA/PPI/bus/device/disk/machine restart remain
separate WIP. Final Windows/Linux CI passed; integrated linearly, source branch deleted.

INTEGRATED PIC PR12 atd900ffde: all native fields, partial initialization, IRQ masks/lines,
ISR/IRR/read selection, deferred INTR and diagnostics are captured. Three tests
and203 core tests pass.2048 destructive JSON restores compare untouched native
references across edge/level and Auto-EOI modes, every IRQ, port reads/vector
acknowledgment and the three-tick mask-change delay. Five actual resets of
restored IRR/timer/initialization/read-selection/statistics fail; exact source
restoration returns203 green. Schema keys and atomic invalid-state refusal
are tested. Independent review found no defect within this bounded scope;
all38 RPC/config tests and fresh Windows UI/headless builds pass.
Final Windows/Linux CI passed; integrated linearly, source branch deleted.
This is PIC-owned state only, not physical 8259 timing or whole-machine restart.

WIP DMA component: every native controller/channel field, partial register
I/O, controller request_reg, transfer address/count/page and terminal status
are captured. Native channel.request status flags are not updated by service
calls; request-status behavior is not proven by this snapshot component.
Three tests and206 core tests pass.1536 destructive JSON continuation restores
compare untouched native controllers and256KiB RAM across all four channels,
read/write/verify and read auto-init;64 additional storage-only restores preserve
all mode combinations/full page bytes. Six actual lost-state controls fail;
exact source restoration returns206 green. Unsupported decrement transfers,
write auto-init and other native service gaps remain unchanged. Review found
no restore defect; request-status wording is clarified. All38 RPC/config tests
and fresh Windows UI/headless builds pass. Final CI/integration pending.
Not full bus/device/restart.

WIP PPI component: raw control byte, modes/latches/dirty flags and four exact
keyboard clock values plus reset/PCjr serial state are preserved. Four tests
and210 core tests pass.2304 destructive JSON restores on nine native machine
models continue keyboard/port/IRQ behavior against untouched peers;256 seeded
raw-byte/clock cases prove storage only. Seven actual lost-state controls fail;
exact source restoration returns210 green. Invalid model/clock/serializer
state is refused before mutation. Review found active serializer/data mismatch
was accepted: the new regression fails on the prior code. Preflight now requires
data in every active phase and none in Idle; all13 invalid cases are atomic.
Seven controls and fresh210 core tests pass on the exact repaired source.
Follow-up review, fresh frontend builds and final CI pending.
FINISHED parser fix: ibm5150v256k no longer selects the64K board, and
compaq_portable is accepted. Both new regressions fail on the original parser;
all212 core tests pass after the two-line repair. Config serde already used enum
names correctly; the earlier review overstated that configuration-path impact.
External PIC/PIT/cassette/keyboard queues and complete restart remain OPEN;
these tests do not establish physical keyboard or8255 timing.

NEXT: review/publish PPI, video/audio/input, disks and pending host
I/O, with atomic dependency preflight, then real process-restart continuation.
Hardware trace,VNC,keyboard/serial RPC and physical XT/gameplay parity remain
OPEN. Controlled BIOS-ring input is not hardware IRQ input.

CI: Windows/Linux debugger/core tests and native builds; no artifacts/caches.
macOS/WASM are manual. Native work uses cargo +1.98.0 with CARGO_HOME,
CARGO_TARGET_DIR and TEMP/TMP under the parent's ignored re/_build tree.
