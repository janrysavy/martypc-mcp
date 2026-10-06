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

WIP: original-Pyro hardware-input/restart proof, PR final-head Windows/Linux CI
and linear integration. Do not assume those gates passed. NEXT: finish the live
fresh-disk witness using observed XT-IDE menu selection, then publish/CI/rebase.
The earlier wrong slave-disk boot selection is retained as a failed startup.

The prior GUI/snapshot work is integrated at 25cb6066 (PR35). Its final CI/tree
and bounded minimized restart proofs live in the parent repository's
`docs/evidence/martypc_rpc_20261004/`; old PR34/35 WIP text is superseded.
Supported snapshots remain the no-floppy CGA/RW-VHD, disabled-host-sink profile.
Whole-floor gameplay, rendered pixels, physical timing/audio, hidden/occluded
GUI and live manual pointer parity remain OPEN.
