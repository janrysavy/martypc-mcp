# Fork handoff

NEXT: finish bus/service final CI and integration; validate/review the new
whole-Machine candidate owner, then implement authenticated persistent save/load,
frontend/RPC rebind and a final live Machine swap. Prove fresh-process Pyro replay.
The selected profile is no-floppy IBM XT/CGA with DOS/Pyro VHDs. Generic FDC,
weak-media entropy and floppy ownership are deferred. Never silently omit a
configured unsupported owner. Local UI input stays off during deterministic RPC.

INTEGRATED main23c6641e after final Windows/Linux CI: native UI/input policy,
queue/BIU/CPU, bus RAM, motherboard devices, keyboard/FIFO, game port, CGA,
VHD/Disk/ATA/XT-IDE, UART/mouse, bus clocks and A0 (PR6..28). Component evidence
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
defect. Final CI/integration is pending. Converse wiring/general profiles
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
Final CI/integration pending; no whole-Machine/process/Pyro proof.

FINISHED first local Machine owner: fresh309 core tests;24 fresh configured
CPU/bus/service/core restores continue native 8088 instructions, PIT/CGA writes,
ROM patch handling and matching captured storage. Required/unknown schema, eight
invalid candidates and changed ROM/config are refused. Initial fixture compilation
used a nonexistent Register16::IP; fixed with native get_ip. No disk, frontend,
process or Pyro proof. Fresh310 adds12 pending-turbo PIT continuations and
seeded native presentation channel consumers. Actual omitted pending speed and
presentation queue stores fail those consumers; exact restoration passes310.
Public DiskCaptureMode now permits frontend callers; fresh312 serial+sound
tests pass after the visibility fix. Scoped review and final CI remain pending.

NOT DONE: Machine/audio/frontend persistent composition, outer configuration/
ROM/metadata authentication and host disk access/path/alias policy, pending
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
