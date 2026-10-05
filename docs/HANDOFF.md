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

INTEGRATED DMA PR13 atca691f98: every native controller/channel field, partial register
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
and fresh Windows UI/headless builds pass. Final Windows/Linux CI passed;
integrated linearly, source branch deleted. PPI PR14 now targets main.
Not full bus/device/restart.

INTEGRATED PPI PR14 at54f9230e: raw control byte, modes/latches/dirty flags and four exact
keyboard clock values plus reset/PCjr serial state are preserved. Four tests
and210 core tests pass.2304 destructive JSON restores on nine native machine
models continue keyboard/port/IRQ behavior against untouched peers;256 seeded
raw-byte/clock cases prove storage only. Seven actual lost-state controls fail;
exact source restoration returns210 green. Invalid model/clock/serializer
state is refused before mutation. Review found active serializer/data mismatch
was accepted: the new regression fails on the prior code. Preflight now requires
data in every active phase and none in Idle; all13 invalid cases are atomic.
Seven controls and fresh210 core tests pass on the exact repaired source.
Follow-up review confirms both verified repairs are resolved. All38 RPC/config
tests and fresh Windows UI/headless builds pass. Final Windows/Linux CI passed;
integrated linearly and source branch deleted.
FINISHED parser fix: ibm5150v256k no longer selects the64K board, and
compaq_portable is accepted. Both new regressions fail on the original parser;
all212 core tests pass after the two-line repair. Config serde already used enum
names correctly; the earlier review overstated that configuration-path impact.
Separate native PCjr gap: set_data bypasses the Idle-arm bit_ct reset, so the
next byte's parity calculation retains the prior byte's bit count. Snapshots
preserve this existing behavior; per-byte protocol/hardware correction is OPEN.
External PIC/PIT/cassette/keyboard queues and complete restart remain OPEN;
these tests do not establish physical keyboard or8255 timing.

INTEGRATED game-port PR15 atff279738: positions/buttons and all four in-flight charge timers
are serialized without retriggering or host sampling. Five tests and217 core
tests pass.1024 destructive JSON native continuations cover both layouts and
default/alternate ports;64 seeded IEEE-bit cases prove storage only. Four actual
lost-state controls fail, then exact source restoration returns217 green.
Ten additional restores cross native charge deadlines on actual port reads;
lost position/time/active fields fail those I/O checks, buttons fail native I/O.
Initial review's missing-time-control claim is false: the exact prompt contained
that actual failure. Native runtime tail/constants are byte-identical to base.
Strict nested keys/array lengths and atomic version/port/layout/axis preflight
are tested. Focused follow-up found no unresolved scoped restore defect or
meaningful coverage gap. All38 RPC/config tests and fresh Windows UI/headless
builds pass. Final Windows/Linux CI passed; integrated linearly, branch deleted.
Not physical analog timing,
full input queues, complete machine state or restart proof.

WIP keyboard component: native scan/reset buffers, held-key order, cached
translations, mapping/macros and exact finite typematic clocks are captured.
Native reset appends break bytes in hash order; receiver pops them in reverse.
The constructor iteration profile is preserved; incompatible
constructor profiles are refused. Existing TOML mapping extensions still work.
Native algorithms are unchanged. Seven tests cover768 destructive continuations,
three repeat-deadline restores,51 queued scan/reset restores and three cached/new
mapping restores on ModelF/Tandy1000/Pcjr, plus27 seeded legacy buffered restores;
33 default/unique-seeded cases prove
storage only. Nine actual lost-state controls fail native scan/repeat/mapping
observations; exact source restoration passes224 core tests. Strict nested
schema, missing/extra fields and invalid restore atomicity are tested. ModelM
and unsupported multi-code key-up states are explicitly refused. Typed macro
entries roundtrip, but Machine macro FIFO is NOT yet restored: peers retained
and compared. Bus delivery clocks, PPI/PIC/hardware keyboard, physical timing
and process restart remain OPEN. Initial review reset-order wording is corrected
and exact reverse delivery asserted. Buffered overflow/size omissions fail actual
FF output, with reset priority and one-shot flag consumption. Public constructors
use capacity1; seeded capacities2/4/8 test the legacy arm. Its sub-capacity
producer drops bytes; that unchanged native gap is observed, not repaired here.
Review follow-up confused deliberate lost-capacity/overflow mutants with
production failures; fresh hash-bound unmodified recheck passes. Corrected audit
confirms both findings resolved and no remaining scoped defect. All38 RPC/config
tests and fresh final Windows UI/headless builds pass. Final CI/integration pending.

WIP machine/bus keyboard input component: real FIFO entries, keyboard state
and exact bus polling accumulator are captured together; nested version/type/
presence/clock validation completes before live mutation. Five new tests and229
core tests pass.384 destructive JSON restores on IBM5160/Tandy1000/PCjr continue
native macros, frame-gated events and PPI/PIC observations against untouched
references. Two FIFO/frame restores and one poll-deadline restore verify actual
77/F7 press/release and33 repeat bytes. Eight no-device clock seeds prove storage
only. Six actual lost queue/order/keycode/pressed/modifiers/poll-clock controls
fail native port output; exact source restoration returns229 green.
Native emulation algorithms are unchanged. Last matching mapping wins; bus
key-down ignores translate metadata, retained as existing behavior. Caller-owned
per-frame flag and CPU/PPI/PIC/A0/video/other clocks/disks are retained peers,
not restored here. No complete machine restart/Pyro/physical timing proof.
All38 RPC/config tests and fresh Windows UI/headless builds pass.
The initial review lacked inherited context: mappings/typematic are captured
mutable owner state; strict nested schemas and both presence refusals already
exist. Audit with full inherited code disproves all three objections and finds
no remaining concrete scoped restore defect. Parent retains source/product-bound
proof and both review answers. Final publication/CI/integration remain pending.

INTEGRATED keyboard PR16/cb724cd0 after final Windows/Linux CI; source branch
deleted. Input PR17/df27e5f0 is integrated after final Windows/Linux CI; source branch
deleted and monitor PR18 retargeted to main.
WIP monitor component: both native synchronization PLLs and all monitor fields
are captured with exact finite IEEE clock bits and strict nested schemas.
233 core tests pass.515 destructive native continuation restores cover enabled/
disabled operation, both polarities, hold adjustment, callback edges and actual
observed periods. Four initial configuration roundtrips are storage only.
Seven actual lost phase/drift/edge/enable/timer/polarity controls fail native
callbacks or PLL APIs; exact source restoration returns233 green. Native monitor
and PLL runtime algorithms are unchanged. This preserves the current model;
no physical monitor or CGA/VRAM/full-machine/restart proof is claimed.
Initial review found a real negative last_period_ticks validation gap and two
coverage gaps. A new regression fails before repair. Negative observed periods
are now refused atomically; zero remains initial/unobserved. All four independent
h/v polarity pairs and a native hold/phase above2.0 case are covered. New swapped-
polarity, clipped-hold and normalized-phase controls fail actual APIs. All ten
loss controls fail, then235 core tests pass.1028 native continuation restores
and eight initial storage roundtrips are covered. The initial0.0001 hold example
was below the nominal maximum; it is corrected to0.001, with0.01 used to prove
out-of-range continuation. Native runtime algorithms remain unchanged.
Follow-up review confirms all three findings closed and no remaining concrete
scoped defect. All38 RPC/config tests and fresh Windows UI/headless builds pass.
The parent retains both source/product-bound receipts, actual mutation driver
and before-fix regression. Monitor PR18/a56c32fd is integrated unchanged after
final Windows/Linux CI; linear history, source branch deleted.

WIP CRTC component:37 native fields are captured, retaining selected register,
raw registers, pending frame/address latches, cursor dividers, interlaced parity
and sync/raster counters. Six new tests and241 core tests pass.4614 destructive
JSON native continuations cover three interlace modes, three sync widths, all
four cursor modes, partial port writes, lightpen reads, actual blink output and
native VTA32-to-half-line transition. Strict schemas and atomic counter/version/
trace refusals are tested. Seven actual lost-state controls fail native output;
omitting a native slot fails the independent field inventory, then exact source
restoration returns241 green. Console/file trace handles are refused; card VRAM,
monitor/bus/frontend state, physical timing and complete restart are not proven.
Native CRTC runtime is unchanged. Initial38 RPC/config tests and fresh Windows
UI/headless builds pass. Review found an impossible cursor-start value accepted;
the new regression fails before repair. The masked five-bit cursor latch is now
validated before mutation. Proposed seven-bit C4 rejection is disproved: lowering
R4 behind the current row reaches128 and255 through actual native ticks. Two
additional cold restores continue identically; masking or rejecting those native
values fails. All ten controls fail, then242 core tests pass, with4616 native
continuation restores. All38 RPC/config tests and fresh Windows UI/headless
builds pass on9d72968c. Follow-up review confirms original findings closed;
it identifies an untested valid nonzero cursor-start latch. Four new native
cursor-output continuations at starts1/2/15/31 close that gap; actual cursor-start
loss fails native tick output before JSON comparison.243 core tests pass;4620
native continuations and eleven controls are retained across frozen receipts.
Only test/HANDOFF changes after9d72968c; production restore is byte-identical.
Focused review confirms the nonzero cursor gap closed; no scoped witness flaw.
PR19/b16022d5 is integrated unchanged after final Windows/Linux CI passed;
linear history, finished source branch deleted. Parent retains final CI receipt.

FINISHED local CGA phase repair: cold clock zero made native !cycles + 1
overflow in debug builds before any first port write. Two regressions fail
before repair; explicit wrapping negation preserves release arithmetic.
All245 native core tests pass, including258 phase vectors and actual first
zero-clock CRTC port writes. No broader timing/card/machine proof is claimed.
Scoped phase review found no defect; PR20 integrates this after final CI.

FINISHED local CGA state:109 native non-trace fields,16KiB VRAM,both raster
buffers and nested CRTC/monitor are retained. Seven tests and252 core tests
pass.195 destructive JSON native continuations cover24 video/clock/monitor
configurations, pending character clock, native reset crop and pending snow
pixels.25 extra initial/legacy-slot restores are storage-only. Ten actual
restore omissions fail native raster/ports/MMIO/CRTC observations; omitting a
field fails independent native inventory. Exact source restoration returns252
green. Strict schemas, geometry/clock/version/trace refusals and unchanged
live state on invalid input are tested. Native runtime is unchanged after the
separate tested phase repair. The parent retains source/product-bound evidence.
Review finds no concrete production defect, requesting clearer nested-schema
and source-identity witnesses. CGA boundary now removes/adds keys inside nested
CRTC/monitor/both PLLs; their unchanged independent native inventories also pass
in the full252-test suite. Explicit hash/assertion recipes bind exact restored
source and native identity. Snapshot production remains byte-identical to577f6075.
Fresh38 RPC/config tests and Windows UI/headless builds passed on577f6075;
later test-only changes reuse those products explicitly. Scoped reviews find
no concrete CGA restore defect; nested-schema finding is closed. Artifact
review found complete receipt-byte hashes were not enforced: changed scope
passed before, fails after explicit fixed hashes; unmodified receipts pass.
This narrow checksum repair was not re-reviewed. Parent retains recipes,
review findings, actual controls and precise native implementation-suffix scope.
PR20/680341e9 is integrated unchanged after final Windows/Linux CI37243486466.
Finished source branch deleted after child VHD PR21 retargeted onto main.
Whole-machine/bus/disks/frontend/audio/process restart/Pyro/physical
timing remain OPEN.

FINISHED local VHD component: cached metadata and exact backing size/SHA-256/
I/O position, with binary payload separate from JSON. Auto embeds to an explicit
limit and refuses larger disks without an embed/reference choice. Replacement
preflights a supplied fresh provider without mutating the live disk. All260 core
tests pass:64 native sector continuations, native failed-write/footer-cache
divergence, and actual RW File close/reopen into a separate copy. Four DELIBERATE
MUTANTS fail named tests; exact restored source returns260 green. Two controls
test storage-only cursor/checksum fields. Native sector algorithms are unchanged.
Initial fresh38 frontend tests and Windows UI/headless builds pass on321ead32.
Review identifies restore-provider read/final-seek witness and trusted-metadata
scope gaps. New direct tests cover both failure paths with unchanged live disk;
all261 core tests pass. Real File reference now reopens through independent
native parsing rather than prepare_restore. Metadata authentication belongs to
the future outer container; disk hash alone covers backing bytes only. No
production restore algorithm changed. Fresh38 frontend tests/UI builds pass
on4a6586c9; scoped follow-up closes both findings with no remaining bounded
defect. Final CI/integration is tracked by the parent's handoff/evidence.
Caller-owned access policy/backend/path, atomic multi-disk dependencies,
Disk/ATA/XT-IDE transfer state and actual process/machine restart remain OPEN.
The parent retains source/product-bound raw transcripts and control recipes.

FINISHED local Disk wrapper:265 rebased core tests pass;24 native JSON continuations
preserve seek/next-sector behavior, reads/writes and geometry differing from
VHD. A native unload/rebind keeps stale CHS. Strict required/unknown keys,
provider-presence/hash/version refusals and independent Disk/CHS/geometry field
inventories pass. Four DELIBERATE MUTANTS (CHS, geometry, mounted VHD and
required option) fail named native/schema tests; exact restored source returns
265 green. Initial snapshot runtime is unchanged in its frozen receipt.
Review independently exposed existing sector-base bugs: contains accepted sector0
below a one-based geometry, and VHD mapping always subtracted1. Three native
regressions fail before correction; all268 core tests pass after using declared
offsets, including82944 wider-integer boundary checks. Zero-based geometry stays
valid. Native Disk/geometry change only those proven rules; full-file comparison
against680341e9 plus explicit module/fix edits closes the earlier suffix-only
identity gap. A pre-anchor field-type copy passes the old scope but fails the
full-file verifier; this is source-only, never a live compiled mutation.
Fresh four snapshot omissions also fail; exact source restoration passes268.
Follow-up review finds a native next_sector byte-overflow. Two successor
regressions fail before correction; guarded wider bounds now refuse invalid
or unrepresentable successors instead of overflowing/skipping sectors.
82944 independent wider-LBA successors and all271 core tests pass. Constructor/
set_geometry stale below-base CHS is explicitly preserved by two storage-only
JSON restores; saturating position_vhd is not a valid-address guarantee.
Identity scope is exact native Disk/geometry/CHS files plus explicit intended
edits; snapshot/dependency files are separately pinned, not baseline-equal.
Snapshot production is unchanged. Fresh38 frontend tests and Windows native
UI/headless builds pass on d7dd3be0. A fresh source-bound271 core run deletes
previous test executables and records clean source hashes before/after compilation.
The continuation count means25 restored checkpoints:24 matrix positions plus
one unload/rebind; two observations per matrix checkpoint are not extra restores.
The same named whole-file comparator accepts originals and rejects a deliberate
pre-anchor field-type copy. Final bounded reviews/CI/integration are tracked
in the parent's handoff/evidence; these are not Machine restart claims.

FINISHED local ATA component:42 native fields have explicit versioned state;
partial low/high bytes, buffer contents/cursor (native len+1), operation counters,
command FIFO and known callback independent of opcode, pending bus flags and
exact clock bits are preserved. Unknown callbacks are refused before disk I/O.
Fresh279 core tests pass, including70 native API JSON checkpoints:32 read/identify,
28 writes with independently parsed backing bytes,8 pending callbacks and2
buffer/reset phases. Two IRQ/DREQ consumer checkpoints are explicitly seeded:
native PIO handlers do not currently produce those requests. Thirteen one-hot seeded
storage-only JSON cases compare all42 native fields directly, independently of
the serializer. LF/CRLF inventory regression covers Windows CI parser failure.
Strict missing/extra keys and dependency/version/wiring/buffer refusals pass;
no live ATA owner is mutated on restore refusal. Native ATA algorithms unchanged.
Six actual omissions and source-bound278 evidence remain frozen on8f1311c6;
restore production is byte-identical. Fresh38 frontend tests/UI builds passed
on8f1311c6. Review found witness/scope gaps, no definite production omission;
direct storage checks now address the shared serializer limitation.
Fresh source-bound279 and both repair controls pass. Follow-up review found
consistent wire-key permutations were not checked; explicit seeded wire values
now pass279 core tests while restore production remains unchanged.
Actual consistent lba/dma remapping passes the earlier witness and fails the
new wire oracle. Exact source restoration and source-bound279 pass on d8ae54d9.
WIP: final scoped review/Windows-Linux CI/integration, tracked in parent
docs/HANDOFF.md and docs/evidence/martypc_rpc_20261004/. This final update
changes HANDOFF only; no native product or production-code identity claim.
Initial Windows CI failed only CRLF inventory.
Controller selection/bus wiring, host access/path, complete Machine/process
restart and Pyro continuation remain unproven.

FINISHED local XT-IDE component: ten controller fields, both ATA owners and
complete capability descriptors have versioned state. Preparation authenticates
both fresh disk providers before any live controller replacement; count1 permits
native empty-slave selection1. Native algorithms unchanged apart from module
registration. Fresh285 core tests pass:12 dual-drive partial-read and6 partial
low/high-write native-port checkpoints, with independently parsed written sectors;
1 empty-slave and1 count1 mounted-slave native-port continuation, plus1
native-unload storage checkpoint with restored VHD/payload absence verified.
Six seeded error/capability vectors are storage-only. Missing/extra schema keys,
fixed drive-array length and second-disk missing/corrupt dependency refusals pass
without live-owner mutation. Controller restore is not a complete bus/Machine
or process-restart proof.
Draft PR24 and fresh source-bound284/frontend products are retained. Four
actual omitted-state/schema controls fail; exact source restoration passes284.
Review-requested rejection of a count1 mounted slave is incompatible with the
existing native set_vhd API: a fresh285 run mounts slot1 at count1 and checks
512 independent native sector bytes across JSON restore. Native mounting
policy is preserved; nested ATA/Disk/VHD proof and outer policy stay separate.
Fresh source-bound285 and native-policy refusal control pass on28721662.
The executed outer launcher restores exact source before its same-UUID fresh
positive run; chronology/source/product witness is retained in the parent.
Scoped follow-ups close mounting/unload/order findings. Nested ATA/Disk/VHD
proofs remain delegated; outer policy and complete Machine/process remain open.
WIP: final Windows/Linux CI/integration, tracked in parent docs/HANDOFF.md.
This final update changes HANDOFF only after tested28721662.
ATA final scoped wire review passed; final CI/integration tracked by parent.

NEXT: finish XT-IDE evidence/review/final gates; ATA final CI tracked by parent;
ATA/XT-IDE pending transfers, remaining devices/bus clocks/audio/host I/O,
atomic whole-machine dependency preflight and real process-restart continuation.
Hardware trace,VNC,keyboard/serial RPC and physical XT/gameplay parity remain
OPEN. Controlled BIOS-ring input is not hardware IRQ input.

CI: Windows/Linux debugger/core tests and native builds; no artifacts/caches.
macOS/WASM are manual. Native work uses cargo +1.98.0 with CARGO_HOME,
CARGO_TARGET_DIR and TEMP/TMP under the parent's ignored re/_build tree.
