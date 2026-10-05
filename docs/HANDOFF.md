# Fork handoff

NEXT: finish bus composition controls/review/CI, then combine CPU and Machine
metadata/service/events/input with the composed bus. Implement authenticated
save/load and a final live Machine swap, then prove fresh-process Pyro replay.
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
Refreshed controls/follow-up review and final CI are pending. Disk-write/flush continuation is not newly proven by this bus probe.

NOT DONE: CPU/Machine/service/audio/frontend composition, outer configuration/
ROM/metadata authentication and host disk access/path/alias policy, pending
speaker output queues, persistent whole-Machine/process/Pyro restart. Do not
infer those from component tests. Attached audio queues and unsupported installed
bus owners are explicitly refused until they have composed snapshot support.

Build with cargo +1.98.0; CARGO_HOME, CARGO_TARGET_DIR and TEMP/TMP stay under
the parent's ignored re/_build tree. CI covers Windows/Linux; no artifacts or
caches are uploaded. Finished detail moved out of this handoff remains in the
parent evidence and Git history.
