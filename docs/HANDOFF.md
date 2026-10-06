# Shared observation, snapshots and native tracing, 2026-10-06

FINISHED local timer-conversion fix: selected CPU factor now converts complete
PIT intervals for refresh/interrupt hints. Old-source native reload18 probe fails
turbo period72 versus216; fixed normal72/turbo216 and complete component replay
pass. All324 core tests pass, including divisor/multiplier/zero-reload cases.
Fresh original Pyro/BIOS boot, final review/CI and integration remain WIP.
Runtime turbo switches after programming a timer remain an unproven path;
alternate AT timer conversion remains explicitly unimplemented. No physical
timing claim follows from a scheduler period alone. This depends on PR42.

FINISHED local turbo initialization fix: Machine CPU clock changes now synchronize
the bus factor/CGA write lookup table, including construction. The original
configured XT turbo/CGA boot panicked (ticks_advanced30 > clocks15); all322 core
tests now pass, including normal/turbo PIT/VRAM and complete snapshot continuation.
XT turbo is14.32MHz; the old7.16MHz config comment was stale. Normal startup now
initializes the previously zero CGA write timing lookup; historical timing stays
bound to its recorded product. Snapshot preflight refuses mismatched Machine/bus
clock factors, preserving consistent historical tables verbatim. Fresh original
Pyro code identity passes normal/turbo, calibration99/297 observed at source0e816.
Final-head native continuation, CI and correction review remain WIP.

FINISHED local gates: snapshot archive/reference host-read I/O maps to -32603;
validation stays -32602 and other backend/export errors stay -32000. All 61 RPC
tests pass, including named missing-file controls and omitted-map failure/exact
restoration. Fresh clean 02c8cc8d/aa284a27 actual TCP missing-reference and
missing-archive requests preserve complete Machine/two disks; owned guest closed.
Review found no code defect. Metadata/read failure branches use the same typed
mapping but were not separately fault-injected. Broader export/output I/O remains
OPEN. Before integration, require final-head Windows/Linux CI and source binding.

FINISHED source: coherent paused state.observe/native CGA video.text; shared
snapshot hash aliases, preserve_breakpoints defaulttrue, reference-files and
atomic typed dependency refusals; Intel8088/8086 native data/interrupt watchpoints
and bounded CPU/I/O/primary-PIC traces. Async step accepts entry+operation_id;
read completed registers at execution.wait.stop_reason.registers. Permanent
breakpoints/hits survive default imports; transient operations, step and populated
CPU/hardware recorders are cleared. Any active native bus journal refuses capture.

Memory stops after the actual native machine boundary; software interrupts stop
after dispatch before the handler. Conditions use before-dispatch registers.
Native opcode bytes come from the consumed prefetch queue. IRQ edges report
boundary time intervals; primary lines0..7 only. No CPU timing/prefetch substitution.
NEC observation and unavailable phases are refused. See docs/JSON_RPC_API.md.
Snapshots retain the supported no-floppy CGA/RW-VHD profile with host sinks off.
Prior GUI/snapshot PR35 main25cb6066 proof: parent docs/evidence/martypc_rpc_20261004.
Whole-floor gameplay, rendered pixels, physical timing/audio and hidden/occluded
GUI/manual pointer parity remain OPEN.

Earlier independent source-bound proofs are retained in the parent Pyro repo:

- docs/evidence/martypc_keyboard_20261006: original raw keyboard/native IRQ1 and
  complete cold continuation; PR38 main c82809d4 equals its tested9059a86f tree.
- docs/evidence/emulator_rpc_portability_20261006: original coherent peeks and
  embedded/reference-files cold restore, full Machine/two-disk continuation,
  parameter/backend refusal controls. These precede combined tracing integration.
- docs/evidence/martypc_tracing_20261006: source9ac63fe6 original86064 relocated
  code identity; actual CellPlotter0FC6:7DDE byte7 ->6, opcode26881d and shared
  completed registers agree. Historical product, not proof of this combined head.

All those owned guests are closed. Their records are bounded tooling witnesses,
not new mechanics, protected-fire8 ->14 or whole-game/physical-timing parity.

FINISHED corrections: simultaneous text selectors refuse before inspection.
Native CPU/device clock progress establishes execution boundaries; zero/sentinel
returns cannot fabricate a revision or trace. Native breakpoint/step-over/end
stops retain their scope, other refusals report backend_no_progress; journals
close and recovery advances a real boundary. Named old-source counterexamples
and paired native whole-state controls pass. 61 RPC, 10 config, 60 frontend
and 5 headless gates pass, including populated import reset and traced/untraced
complete native continuation. Focused frozen correction review found no defect;
its omitted runtime gates are separately observed, not review-derived claims.

Earlier tracing product a3aa3ddc binds clean source 806905fa and 607
compiler/source inputs.
The known original action replay preserves every earlier RAM/BDA/VRAM read and
actual 0FC6:7DDE byte7 ->6; trace/wait completed registers agree. Shared PyPC-client
and current common selector/policy controls pass. Both reference-files and
default-embedded cold proofs preserve full Machine/two disks and 200ms
continuation, with typed digest/reference refusal controls. All owned guests
are closed. These are bounded tooling proofs, not additional game mechanics.

Combined tracing PR37 was rebase-integrated at 45ec; docs PR40 at 4421 is
runtime-identical. Historical tracing/restart packets remain in the parent
evidence directories above. Root owns the parent submodule pin.
