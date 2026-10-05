# Native GUI RPC, 2026-10-05

FINISHED local corrective source: native logic applies the original input path
before the existing timestep runner. Read/control requests are serviced before
its tick gate; execution keeps configured cycle quotas. RPC alone clamps slow
host deltas to two frames instead of discarding every minimized update.
Local58 frontend timing,31 RPC and17 GUI tests plus a source-bound UI build pass.
The clamped GUI preserves complete paused Machine/disks and replays a minimized
one-millisecond continuation after import with complete equality.

The first scoped review rejected the unpaced u32::MAX candidate and changed
input order; neither is accepted. A subsequent paced native trial reproduced
read timeouts at the old timestep gate. Its failure remains in the parent.
WIP: original-Pyro runtime, follow-up review, final-head Windows/Linux CI and
parent pin integration. Do not assume those gates passed. Snapshot executable
identity stays strict: another executable must boot its own fresh test session.

## Prior snapshot handoff (historical status)

# Fork handoff

FINISHED local native GUI restart for the no-floppy CGA/RW-VHD Pyro profile.
Fresh normal EXE f7b6b9ea (601 source inputs) preserves every captured Machine
field and both VHD hashes across real process restart at23->24 and41->42 guest
seconds; the latter preserves pending BIOS Enter and level-generation RNG.
Wrong digest refusal preserves cold Machine/disks. Paused repaints/import also
leave full state unchanged. Scoped follow-up review finds no concrete profile
defect; prior VHD-path finding was disproved by actual manager code.
Evidence: https://github.com/janrysavy/pyro221_next/tree/master/docs/evidence/martypc_rpc_20261004
CI now includes serial+sound core regression on Windows/Linux and17 native GUI
library checks on Windows. NEXT: final-head CI then linear PR34 integration.
Whole-floor survival, pixels and physical timing/audio remain OPEN.

FINISHED explicit native GUI snapshot sink guard: factory availability and cold
construction reject configured CPU trace/on/file or disassembly recording/file.
Fresh17 GUI library checks pass, including five independent refusal settings.
The older positive fixture inherited stock trace settings; first refusal is
retained and the fixture now uses the actual no-sink Pyro profile. Earlier
pump-only notes below are historical.

FINISHED configured-speaker repair: real native GUI export refused an output
queue despite disabled SoundOutputConfig. A speaker=true positive factory
fixture reproduces the refusal; PC-speaker sender creation now honors the host
output flag. Fresh318 serial+sound core and62 frontend checks pass (601 inputs).
Enabled/disabled host output match CPU cycles/register, full PPI and all native
PIT channel/clock fields after10000 cycles. Five host output fields intentionally
differ, including PitSpeaker.enabled, which has no native reads. The first
comparison omitted that captured enable flag; its failure is retained. Host PCM
sampling is disabled; physical audible parity is not claimed.
PR34 remains draft pending final-head CI; bounded live proof is recorded above.
FINISHED local GUI wiring:62 fresh checks (10 config,31 RPC,5 headless,16 native
GUI library tests),601 source inputs stable before/after. Loaded ROM/config/
keyboard dependencies build a cold candidate; imported state yields before
queued execution. Independent next100 native cycles match the restored factory
Machine. This test has no GPU/render/process coverage.
No-playback GUI explicitly disables host output queues, retaining native guest
PIT/PPI/speaker clocks. Snapshot methods require RPC, no host sound player and
local input off. RW disk wrappers cover absolute/relative/resource-name mounts.
After import, clear frontend input/counters/old VHD selections and refresh native
power/machine info; renderers reacquire device buffers by stable IDs. Never call
apply_config on restored guest state. RPC repaint preserves native event queues,
light-pen and debug state (manual local input remains explicit opt-in).
Headless PR33 is integrated at exact2c6082c9 after Windows/Linux CI37309652341.
FINISHED local transport slice:45 fresh RPC/config/headless tests pass;600 inputs
bound. Snapshot-aware nonblocking pump yields immediately after import, before
queued Continue or native execution; next boundary matches an independent
reference. Wrong-digest refusal preserves complete Machine and pending inspection.
Scoped review confirms the in-process pump; its TCP coverage gap is closed by
a real persistent connection exporting/importing/inspecting/refusing/inspecting.
Initial fixture incorrectly expected state.get.paused; corrected to the actual
session.status.state contract, without changing production. New TCP test was
not re-reviewed in that transport-only slice; later scoped GUI review and
live bounded continuation are now complete. Storage PR32 is integrated
at exact8bc61587 after final Windows/Linux CI37301899013 passes.
FINISHED local headless RPC: fresh42 frontend/RPC tests, fresh317 serial+sound
core tests,600 bound source inputs and fresh normal Windows consumer build
b3e0b8e6. Actual executable digest, mandatory independent expected archive SHA,
paused revision guards, isolated new RW disk copies and final Machine swap.
Runtime provider guard refuses Cursor/raw RO/append Files; constrained typed File
constructors preserve actual RW/non-append access, not cached read_only flags.
40 native CPU/PIT/CGA steps with nondefault CPU options/PIT phase, partial ATA
byte17 and native writes to both restored Files pass; original refs unchanged.
Cleanup failures log retained paths. Both scoped reviews are complete; their
remaining real factory/process gap is now covered by bounded native runs.
Real stock frontend refuses trace/listing sinks; no-sink profile exports at reset.
FINISHED bounded headless process restart: two unmodified Pyro startup windows
(36->37 and37->38 seconds) compare every captured Machine field and both VHDs.
A third40->41-second replay preserves pending BIOS Enter, consumes it identically
and generates the first level with RandSeed24130000->CE19534E on both branches.
Wrong independent archive digest leaves each cold Machine/disks unchanged.
Parent evidence: machine-restart.json.gz and machine-restart-queued.json.gz in
https://github.com/janrysavy/pyro221_next/tree/master/docs/evidence/martypc_rpc_20261004
Production/product9adf487c/b3e0b8e6 are unchanged for this documentation update.
GUI restore, physical timing/input/audio and whole-floor gameplay remain OPEN.
The selected profile is no-floppy IBM XT/CGA with DOS/Pyro VHDs. Generic FDC,
weak-media entropy and floppy ownership are deferred. Never silently omit a
configured unsupported owner. Local UI input stays off during deterministic RPC.

FINISHED local container: fresh317 serial+sound core tests,597 bound inputs,
plus fresh normal Windows core/headless build. Synced File close/reopen restores
two VHDs/cached read_only flag (metadata only), partial ATA byte17 with independent sector pattern,
native CPU/PIT/CGA continuation and full captured storage. Missing/altered refs,
metadata/build/digest/member/budget controls refused. Actual duplicate filename
acceptance repaired by raw ZIP32 count validation; failure retained.
[Storage API and proof limits](SNAPSHOT_STORAGE.md). Review corrected an
overclaim: cached read_only is not write protection; native writer unchanged.
Actual build provenance/host provider/decompression-bomb coverage stay open.
Final CI pending; no
frontend/RPC/host provider policy/process/Pyro proof.

FINISHED native flag probe: fresh317 serial+sound tests verify read_only=true
still writes exact sector bytes5E/A7 through writable Cursor before/after restore.
The review overstated the older test: it preserved the flag but never wrote.
Actual write permission belongs to the provider. Guest algorithms unchanged;
normal f741 core/headless product explicitly reused after this test-only change.

FINISHED local snapshot disk requirements: fresh314 serial+sound core tests,596
source inputs bound before/after. Typed Machine accessors expose each actual
mounted VHD size/SHA, embed/reference and cached read_only metadata; no schema or
emulation change. Both native slots and unload are tested. Initial fixture
failed because unload slot1 requires two configured drives; fixed the fixture,
not native semantics. Container integration/RPC/process/Pyro remain WIP.

INTEGRATED main9df4b73a after final Windows/Linux CI: native UI/input policy,
queue/BIU/CPU, bus RAM, motherboard devices, keyboard/FIFO, game port, CGA,
VHD/Disk/ATA/XT-IDE, UART/mouse, bus clocks, A0, bus and service owners (PR6..31). Component evidence
and exact CI/product/source receipts live in the parent repository:
https://github.com/janrysavy/pyro221_next/tree/master/docs/evidence/martypc_rpc_20261004

FINISHED local bus composition: fresh305 core tests,16 fresh-owner JSON
continuations with native ports/events/RAM/VRAM and disk-read/backing-byte
comparisons. Eleven failed candidates leave the independent live reference
unchanged. prepare_bus_restore consumes a candidate; it never returns a partly
restored bus on error. Fixed XTIDE format wiring, six orphan route refusals and
actual orphan PIT sender refusal. Native descriptor writers and CGA clock-mode
setter are measured as mutable state, not forced to equal constructor values.
Guest device/timing algorithms are unchanged. Two earlier owner omissions are
caught in native outputs. Fresh305 tests pass after requiring actual owner
port lists and CGA identity in I/O, MMIO and traversal. Regression refuses
stale routes even with installed owners, plus Mouse I/O (no native dispatch).
Refreshed controls pass and scoped review reports no concrete bus restore
defect. Final9039724f Windows/Linux CI passed; integrated unchanged.
Converse wiring/general profiles
and outer authentication stay open. No new disk-write/flush proof.

FINISHED local service owner: fresh307 core/309 serial+sound tests, nine
fresh service/CPU/RAM
checkpoints with native pending host completion, partial transfer data/CRC,
events/registers/memory, speed bounds and LIFO handle reuse. Strict required
schema and six invalid restore cases leave the manager unchanged. The first
fixture incorrectly used an uninitialized Default CPU; existing CPU preflight
refused it. Fixed by using the native initialized Intel8088 constructor.
Two actual omitted stores fail native host completion; exact restoration
returns307 green. Scoped review finds field-complete codec/tested refusal
atomicity, no concrete defect. New comparisons cover visited transfer paths,
AX/BX/CX/DX/SI/flags, RAM100..3ff and CRC continuity. Other services, independent
CRC correctness and exhaustive field/error-path coverage remain unproven.
Finalec95459b Windows/Linux CI passed; integrated unchanged. No complete
Machine/process/Pyro proof.

FINISHED first local Machine owner: fresh309 core tests;24 fresh configured
CPU/bus/service/core restores continue native 8088 instructions, PIT/CGA writes,
ROM patch handling and matching captured storage. Required/unknown schema, eight
invalid candidates and changed ROM/config are refused. Initial fixture compilation
used a nonexistent Register16::IP; fixed with native get_ip. No disk, frontend,
process or Pyro proof. Fresh310 adds12 pending-turbo PIT continuations and
seeded native presentation channel consumers. Actual omitted pending speed and
presentation queue stores fail those consumers; exact restoration passes310.
Public DiskCaptureMode now permits frontend callers; fresh312 serial+sound
tests pass after the visibility fix. Review reproduced enabled-listing and
zero/inconsistent period acceptance; fixed preflight now refuses all three.
Fresh313 serial+sound tests pass. Follow-up confirms those two repairs. Queued
checkpoint index99 accepted before, refused after the bounds repair. Fresh313
passes with native reads of overwritten patch RAM and execution through restored
historical maps from a candidate constructed with different maps. Native queued
level2 survives manifest level7 and is preserved alongside new level7 hits.
Normal CI build failed because serde_json was only a dev-dependency. Moved
the existing1.0 dependency to production (lock unchanged); fresh Windows normal
core/headless consumer build passes,596 inputs bound. Final CI37296505851 passed.
Native reinstall_roms retains historical maps;
later RAM writes can alter installed patch bytes. Both legitimate states remain
restorable. Final scoped review finds no concrete candidate defect; the historical
patch-map store is checked but its fresh installation side effect is not separately
proven. Keep that narrow evidence limit. Exact9df4b73a integrated; no exhaustive
metadata reachability
or device-origin channel/frontend/VHD-bearing Machine/process/Pyro proof.

NOT DONE: frontend/RPC persistent composition, portable configuration/ROM
rebinding and host disk access/path/alias policy, pending
speaker output queues, persistent whole-Machine/process/Pyro restart. Do not
infer those from component tests. Attached audio queues and unsupported installed
bus owners are explicitly refused until they have composed snapshot support.

Build with cargo +1.98.0; CARGO_HOME, CARGO_TARGET_DIR and TEMP/TMP stay under
the parent's ignored re/_build tree. CI covers Windows/Linux; no artifacts or
caches are uploaded. Finished detail moved out of this handoff remains in the
parent evidence and Git history.

CI: run37286467609 hit the30-minute Windows job limit during cold UI build;
Linux passed. The workflow now allows60 minutes without dropping any checks.
Final exact-head CI still must pass before integration. Native code unchanged.
