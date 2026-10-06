# Shared snapshot contract slice, 2026-10-06

FINISHED source and local37 RPC tests: reference-files for File VHDs, expected_sha256
with legacy sha256 (both aliases refused), preserve_breakpoints defaulttrue.
Preserve permanent definitions and host hit counters; always clear transient
predicates and operations. Whole Machine and two RW VHD/partial ATA tests pass.
WIP: review, publication, rebase/live common contract, final CI and integration.

# Coherent observation slice, 2026-10-06

FINISHED source: shared paused `state.observe` and native CGA `video.text`.
Whole-request preflight, revision guards and immutable peeks avoid mixed captures.
After the keyboard rebase, local42 RPC +10 config +5 headless and319 serial/sound
core tests pass. A fresh headless executable binds604 build-source inputs.
Original-Pyro86064 relocated code bytes match at startup. The shared CP437 and
observation contract preserves complete native Machine state and both disks.
The first live checker exposed an unbacked F0000 ROM-hole fixture: refusal also
preserved complete state. Its shared reset-vector correction passes both backends.
Initial review mapping defects/off-aperture panic are corrected and tested;
CP437127 preserves the Python byte decoder's control character through cells.
Evidence: https://github.com/janrysavy/pyro221_next/tree/master/docs/evidence/emulator_rpc_portability_20261006
Owned headless guest is closed. Actual-main rebase onto c82809d4 is complete.
WIP: final-head CI and PR36 integration; do not assume these gates passed.
See JSON_RPC_API.md.

# Raw XT keyboard RPC, 2026-10-06

FINISHED local source and native regression proof: paused `input.keyboard` and
`keyboard.scancode` accept bounded explicit make/break batches, preserving raw
zero bytes and pending FIFO order through mandatory keyboard snapshot version 2.
IBM PC/XT and supported Compaq controller policies are explicit in JSON_RPC_API.
RPC does not write the BIOS ring or invoke host layout/macros/typematic.
Native and RPC bytes now wait for the occupied XT PPI latch. Native reset AA
also occupies it until guest acknowledgement. Paired failure tests reproduced
both prior byte-loss paths; positive controls pass after correction.

Fresh 319 serial+sound core and 51 RPC/config/headless tests pass; a new normal
headless product binds 602 Rust/TOML/lock/build inputs. Native NASM and PyPy-pynasm
emit the same 169-byte IRQ1 probe. Native zero/make/break IRQ1 delivery and cold
serialized complete Machine continuation match. Its first cold IBM5160 byte is
bracketed at 4997142..5000914ns; event-triggered service may occur earlier.
Two scoped read-only reviews found native-priority loss, unsupported-controller
acceptance and overbroad scheduling/Deskpro wording; source fixes are tested and
the final documentation states native scheduling and Deskpro inhibition limits.

FINISHED original-Pyro hardware-input/restart proof: fresh DOS/ROM/game disks,
natural raw scan-code boot and launch, all 86064 relocated original code bytes
verified. Tutorial End make 4Fh stops at native BIOS IRQ1 F000:E987 with occupied
PPI latch and CFh break still queued. A new process restores every captured
Machine field and both VHDs; the exact 200ms continuation matches and consumes
the break. Wrong archive digest leaves cold Machine/disks unchanged. Further
bounded execution reaches World Terrorism/Secret Agent. All owned guests are
closed. The earlier wrong slave-disk boot selection remains a failed startup.
Source-bound gates, paired failures, reviews, live/restart receipts and producers
are retained in the parent repository's
`docs/evidence/martypc_keyboard_20261006/` (witness.json and inventory.json).

FINISHED PR38: exact9059a86f Windows/Linux CI37400543552 passed; rebase
merged main c82809d4 has the identical tested tree. Keyboard proof remains
source-bound; observation/snapshot follow-up integration is still pending.

The prior GUI/snapshot work is integrated at 25cb6066 (PR35). Its final CI/tree
and bounded minimized restart proofs live in the parent repository's
`docs/evidence/martypc_rpc_20261004/`; old PR34/35 WIP text is superseded.
Supported snapshots remain the no-floppy CGA/RW-VHD, disabled-host-sink profile.
Whole-floor gameplay, rendered pixels, physical timing/audio, hidden/occluded
GUI and live manual pointer parity remain OPEN.
