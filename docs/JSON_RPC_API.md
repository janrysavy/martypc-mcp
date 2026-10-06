# Native debugger JSON-RPC

Build and test the native frontend without GUI dependencies:

```text
cargo test -p marty_debug_rpc -p marty_config -p martypc_headless --no-default-features --lib --locked
cargo build -p martypc_headless --no-default-features --locked
```

Run from a writable installation copy containing `martypc.toml`, configuration
definitions and the required ROMs:

```text
martypc_headless --rpc-port 2301
```

Alternatively set `rpc_port = 2301` in `[emulator]`. The command-line option
overrides configuration. The machine starts paused regardless of CPU autostart.
Power-on, ROM, CPU, memory, video and disk settings retain their normal meaning.
Configured VHDs are attached to the selected native Xebec, XT-IDE or Jr-IDE
controller before RPC starts, retaining empty drive slots. A configured disk
that cannot load, parse or attach aborts startup; an RPC-ready line is never
printed for that failed startup. EGA/VGA configurations require their video BIOS.
No-port headless execution still has no event loop.

For an optional visible screen, build `cargo build -p martypc_eframe --locked`
and run `martypc --rpc-port 2301` in the same writable installation. The existing
Rust/wgpu UI renders the exact machine controlled by RPC; it does not start a
second guest. Both frontends use `marty_debug_rpc`. The GUI replaces its normal
machine runner with a debugger pump. Its soft8ms budget is checked before every
request/native boundary; one atomic request/instruction may overrun it. The
transport channel holds at most64 queued requests. This is not a hard latency SLA.
Neither repaint nor inspection advances paused guest time. RPC requests and
execution in deterministic sessions (local input off) are serviced by the GUI
logic callback while its window is minimized. Hidden/occluded windows are not
covered by the retained live witness. The visible UI
timestep never runs that guest a second time. Explicit manual local-input
sessions use visible UI/input routing and require a visible window. Primary
and secondary display pointer samples are collected/mapped before that UI
timestep executes, including absolute mouse/light-pen input. Execution
keeps configured cycle quotas. RPC slow host deltas are capped to two frames
rather than discarded; normal UI pacing still discards them. Read/control
requests do not wait for an emulation/render tick. Logic requests another poll
after4ms; operating-system
scheduling and the soft pump budget still preclude a hard response-time promise.
RPC configurations skip wall-frame guest housekeeping in both frontends;
software PPI turbo remains refused and host serial bridging is not supported
in this mode.

**Local guest input is disabled by default whenever `rpc_port` is set.**
This includes keyboard/hotkeys, keyboard-to-joystick mappings, mouse/light pen,
gamepads and clearing guest keys on window focus loss. Resizing/closing the
window still works. Use `--local-input`, or explicit `local_input = true` under
`[emulator]`, only for manual response/play experiments. Keys reach the guest
only while RPC execution is running, including OSD keyboard events. Set
`local_input = false` for ordinary
view-only runs even without RPC. RPC mode hides GUI machine controls to prevent
a second execution/mutation owner; queued frontend media/host-file completions
are also ignored under RPC. Reboot/debug-step/menu hotkeys cannot take execution
ownership even with manual input enabled. Looping audio pause follows RPC's
actual running state. Live sound-output/input injection proof remains open.
Deliberate RPC input remains available; snapshot commands require a supported
profile. Raw XT keyboard RPC is described below.

The server binds only `127.0.0.1`. Send one JSON-RPC 2.0 object per newline over
TCP; connections are persistent. Replies preserve request IDs. Notifications
execute without replies. Batches are unsupported. Lines are limited to 1 MiB;
an oversized or unterminated line closes that connection. All machine work runs
on one thread; transport threads never inspect or mutate the machine. Host
polling does not advance a paused machine.

This is a **subset** of the PyPC debugger contract. The existing PyPC
`guest.dos_control.RPC` transport, `read` and `write` methods work unchanged for
supported memory ranges. Query `agent.capabilities` before using higher-level
controllers. DOSCTRL's `D800:0000` mailbox requires explicit installed RAM:
normal upper-memory holes remain unwritable. A working transport alone does
**not** establish DOSCTRL command execution or file-transfer support.

The parent Pyro repository separately tested the unchanged DOSCTRL client on
MS-DOS 6.22 and writable XT-IDE VHDs: upload/readback, rename/delete, stdout and
stderr capture, exit status, fresh TP6 compilation and stale-product deletion.
Configure an 8 KiB conventional expansion at `0xD8000`, stage the DOS worker on
the disk and start it through DOS. This is guest software using the listed
memory/execution RPC methods, not a new frontend file-transfer endpoint. The
parent launcher is `scripts/martypc_dos.py --dosctrl --bios pypc`; boot/input
limitations and full-machine snapshot gaps still apply.

| Method | Parameters and result |
| --- | --- |
| `agent.capabilities`, `emulator.info` | Exact aliases; methods, limits, unsupported features, CPU cycle frequency and time-base description. |
| `session.status` | Session state, revision, CPU clock, CPU/video metadata and retained `last_stop`. Optional `session_id` must be `martypc` (discover it; do not hardcode PyPC's backend identity). |
| `state.get`, `state.get_registers` | Exact aliases; `general`, `segments`, architectural `ip`, `flags`, `flags_text`, CPU `clock`, `emulated_time_ns`, `in_hlt`, `state_revision`. |
| `state.observe` | Paused, required exact revision; one coherent registers/RAM/optional CGA text/VRAM capture. See coherent observation below. |
| `video.text` | Native CGA Text40/Text80 cells and CP437 text; optional page or byte display_address. Other adapters/graphics refused. |
| `state.set_registers` | Paused only. Requires `expected_state_revision`, `expected` values and nonempty `set`. AX/BX/CX/DX/SP/BP/SI/DI/CS/DS/ES/SS/IP/FLAGS, lowercase. All guards and Word ranges checked before any write. Returns `before`/`after`. |
| `memory.read` | `address`, optional `length` (default 1, maximum 65536). Returns physical address, byte count, hex, base64, SHA-256 and revision. Uses native bus peeks. |
| `memory.write` | Paused only. `address`, `data_base64`, optional `expected_sha256` (case insensitive). Preflights the entire range; only installed writable RAM is writable (base RAM or configured conventional RAM expansion). ROM, video, EMS and other MMIO devices remain refused. Returns before/after hashes and revision. |
| `input.keyboard` | Paused only. `events`:1..32 objects with `scan_code`0..127 and boolean `pressed`. Queues raw XT make/break bytes through a Model-F keyboard with IBM5150/5160 or Compaq Portable/Deskpro PPI, and native IRQ1; no BIOS-ring writes, host mappings or automatic typematic. Entire batch validates before mutation. Returns `accepted` and `state_revision`. Queue capacity4096; resume execution to deliver. |
| `keyboard.scancode` | Single-event alias: `scan_code` and `pressed` at the parameter root. Same hardware queue and guards. |
| `input.joystick.state` | Configured `joysticks` with index `joystick`, normalized `x`/`y`, boolean `buttons`, and revision. An absent game port reports an empty array. |
| `input.joystick` | Paused only. Complete `joystick`, finite `x`/`y` in -1..1 and `buttons` matching the configured layout (two buttons per stick, or four on a single stick). Entire request preflighted before native potentiometer/button mutation; guest clock does not advance. Returns the same state schema. |
| `breakpoints.create` | Optional `kind:"execution"` (default), `address`, optional boolean `once`, `condition`, `hit_filter`, bounded `length` (default 1). Returns `breakpoint_id` and descriptor. Maximum 256 persistent breakpoints. |
| `breakpoints.list` | Returns `breakpoints` array. |
| `breakpoints.delete` | `breakpoint_id`; unknown IDs are errors. |
| `execution.continue`, `execution.go` | Paused, powered-on machine required. Exact aliases; registers, `operation_id`, `state:"running"`, `paused:false`. |
| `execution.run_until` | Same precondition; private one-shot execution `predicate`, optional positive `max_emulated_ns` (at most 60 seconds). Returns operation and predicate IDs. |
| `execution.wait` | `operation_id`, optional `timeout_ms` in 0..60000. Nonblocking poll, as in PyPC. Returns `running:true` or stopped registers and `stop_reason`. Last 64 completed operations retained in completion order. |
| `execution.pause` | Stops current operation. An already stopped machine retains its previous stop reason. Returns paused registers. |
| `execution.step` | Paused, powered-on machine; optional `mode:"into"`. Returns registers with `stepping:true`. Step-over is unsupported. |

Addresses accept an unsigned integer or numeric string (decimal, `0x`, `0b`,
`0o`), or `{space:"physical"|"linear",offset:...}` or
`{space:"segmented",segment:...,offset:...}`. Segment and offset are Words.
Segmented addresses wrap at 20 bits; ranges crossing the end of 1 MiB are
refused. A segmented execution breakpoint requires the actual CS:IP pair;
physical/linear breakpoints match its physical address including aliases.
For execution breakpoints, `length` is validated descriptor metadata and does
not widen exact-address matching, matching PyPC's `matches_address` behavior.
String separators such as
underscores are unsupported. Conditions use `register`, `operator` (`eq`, `ne`,
`lt`, `le`, `gt`, `ge`) and a Word `value`; comparisons are unsigned. Hit filters
use `skip` and positive `every`, counted after condition matches. A persistent
breakpoint is skipped for exactly one boundary when resuming from its stop.

Execution uses native `Machine::run(1, Step)` including CPU prefetch, bus, device
advancement and interrupt completion. This is a machine boundary, not a promise
that every step executes exactly one guest instruction. Reading IP does not
flush prefetch. Explicit CS/IP changes flush and reposition it; writing code
bytes alone deliberately does not flush prefetched bytes.

`clock` is the native CPU cycle counter. Guest time and deadlines use the native
system crystal tick counter, so turbo changes do not change the time base. Tick
to nanosecond conversion uses the machine's configured crystal frequency;
reported nanoseconds are floored, deadline ticks rounded up. Execution stops at
the first completed native boundary at or beyond the deadline, with PyPC's
`emulated_time_limit` fields: requested duration, start, deadline, actual stop,
reached and overshoot nanoseconds. This is modeled emulated time, not a new claim
of measured physical XT accuracy. The initial CPU reset cycles and accumulated
device ticks need not have identical epochs.

Stop kinds are `breakpoint`, `run_until`, `pause`, `step`, `cpu_halt`, and
`emulated_time_limit`. Every completed operation retains its reason and stop
registers. JSON errors use -32700 (parse), -32600 (envelope), -32601 (unsupported
method), -32602 (invalid parameters/guards), -32603 (bus failure).

Joystick axes use -1 for left/up, +1 for right/down, zero for center. Positions
set the native game-port resistance; port201h one-shot timing and active-low
buttons remain native device behavior. This does not inject keys or alter Pyro
memory. Configure `[machine.game_port]` with `io_base = 0x201` to attach a card.
Host input is a paused boundary operation; explicitly resume after setting it.

Raw keyboard bytes wait behind existing native keyboard output. IBM PC/XT and
Compaq Portable controllers require the enabled PPI, high keyboard clock and
acknowledged previous latch. Zero is a valid wire byte: readiness uses the latch
state, not its value. Delivery uses existing bus keyboard service: periodic
updates when its accumulator exceeds5000 microseconds, and native host-key event
boundaries. A host event producing no native byte can therefore service an RPC
byte earlier. This is native emulator scheduling, not measured physical wire
timing. The cold IBM5160 probe brackets its first delivery at4997142..5000914ns.
The existing Deskpro native `kb_enabled()` policy is always true: queued bytes
still wait for latch acknowledgement, but PB7/clock inhibition is not modeled
there. The IBM5160 proofs do not establish Deskpro hardware behavior.
Press/release are explicit bytes; holding a raw make event does not trigger host typematic.
Use the ordinary scan codes for modifiers and their explicit break events.
The FIFO is mandatory in keyboard snapshot version2; earlier component snapshots
are refused rather than silently losing pending input. Complete machine imports
still require the exact executable/configuration/ROM/disk identities.

**Unsupported:** instruction/hardware tracing, memory/interrupt watchpoints,
step-over, video snapshots/history/VNC, serial channels,
frontend file-transfer services and frontend speed/cursor controls. Unsupported
PPI software-turbo configurations are refused before the listener starts:
their native `frame_update` housekeeping is not scheduled by this frontend.
Host serial polling likewise requires that housekeeping and remains unsupported.
Other unsupported methods return errors; this is not a full PyPC replacement. Native CPU/device
tests and a real executable probe establish the listed subset. A separate
native regression compares normal batched Run with RPC continue/inspection:
PIT IRQ0 and REP copies agree on registers, prefetch state, CPU/crystal/PIT
clocks, PIC state and low32KiB RAM at twelve boundaries. This bounded probe
does not prove full DOS/game timing or hardware input equivalence. The parent
workspace's original Pyro startup does not close those gaps.

In the parent Pyro repository, `python scripts/test_martypc_rpc_live.py` makes a
writable scratch installation, loads a nine-byte MOV/INC/store/loop probe,
uses the pinned unchanged PyPC Python client, and closes its owned process.
It does not boot DOS or execute Pyro. Automatic fork CI tests and builds this
frontend on Windows/Linux without uploading artifacts or caching build trees.
The inherited macOS and WASM workflows are available only by manual dispatch.

## Persistent snapshots

Headless and supported native GUI profiles enable `machine.snapshot.export/import`
with their loaded ROM/config/keyboard factories. Query capabilities: unsupported
frontend settings leave the methods unavailable or fail core preflight explicitly.

Both methods require pause and `expected_state_revision`. Export accepts a NEW
`path` and `disk_mode` (`embed`, `auto`=embed, or `reference`), flushes the archive,
and returns its SHA-256 and the actual running executable SHA-256. Retain the
archive digest independently. Import requires `path`, that `expected_sha256`,
and a NEW `disk_root` whose parent already exists. Optional `references` maps
string slots `0`/`1` to exact matching reference disk files. All checks precede
the live swap. Both modes create separate RW/non-append File copies; references
and currently mounted disks are never reused as mutable restore providers.

Export refuses providers other than constructor-enforced `SnapshotRwFile`,
including Cursor, arbitrary/read-only/append Files. Cached VHD read_only remains
metadata; native emulation behavior is unchanged. Existing output paths/roots
are refused. Cleanup failures name retained paths in stderr; successful cleanup
cannot be guaranteed against host I/O failures. Import leaves the guest paused,
increments the revision, resets breakpoints/predicates/completed operations and
retains monotonic ID allocation. Old operation IDs cannot identify new work.

The separate strict `martypc-machine` ZIP format requires an exact executable
build and an independently supplied archive digest. Default limits are128MiB
compressed,32MiB metadata and512MiB total including references. See
[storage contract](SNAPSHOT_STORAGE.md). Core codecs refuse uncomposed configured
owners, active logging/listing/audio queues and unsupported profiles; they do
not silently omit them. Default install trace/listing sinks are not snapshot
ready even when the UI has not enabled trace playback. The parent preparer's
`--snapshots --no-floppy --video CGA` selects a no-sink test profile while keeping
guest PIT/PPI/speaker simulation. Optional host audio output is disabled there.

Local transport proof:45 fresh RPC/config/headless tests pass, including real
persistent TCP snapshot export/import, post-restore inspection and wrong-digest
refusal with the full paused Machine unchanged. The previously
tested headless product also replays three original Pyro startup/first-level
windows across real process restart, including pending BIOS Enter, all captured
Machine fields and both disk hashes. Prior317 serial+sound core tests cover
native component continuation and original reference-file isolation. Broader
gameplay and physical timing/audio remain OPEN. Bounded GUI proof is below.

`DebugRpc::pump_with_snapshots` accepts a supported frontend's SnapshotHost and
returns true immediately after a successful import. No later queued request or
native instruction executes in that pump; the caller must refresh derived
frontend consumers before pumping again. Refusal returns no restore signal,
preserves the live Machine and allows queued inspection. Paused pumps consume
no guest cycles. The GUI wires loaded factory/providers and refreshes derived
renderer/input/event consumers before the next pump.

Native GUI observation under RPC preserves native Machine/presentable-event
queues and video debug flags. Light-pen mutations require explicit local input;
ordinary RPC observation leaves captured light-pen state untouched. Rendering
still reads native video buffers. Actual paused repaint/import measurements
preserve every captured Machine field and both VHD hashes.

Native GUI snapshots are integrated. They are offered
only with RPC, local input off and no host sound player (`--nosound`). The core's
host output configuration is explicitly disabled; guest PIT/PPI/speaker clocks
continue. Loaded configuration/preferences/ROMs/keyboard construct a cold
candidate. All native VHD mount routes use constrained RW File providers.
The cached build identity is read from the actual running EXE once, not from the
archive or a caller label. Capabilities identify this host as `native-gui`.

Successful import pauses RPC, yields before queued execution, resets derived
frontend input/counters/power metadata and removes old media selections. The
new Machine owns its separate restored disk copies (paths in the import receipt).
Renderers reacquire native buffers/extents/mode/palette by stable card IDs. No
machine options, PIT phase or pending native event queues are rewritten by this
refresh. Host performance timing/pixels are derived, not captured guest state.
Actual visible GUI process restart preserves complete captured Machine and both
VHD hashes at23->24 Pyro startup seconds and41->42 level initialization seconds,
including pending BIOS Enter, consumed input and matching seed changes.
Wrong archive digest preserves cold state. Scoped follow-up review finds no
concrete defect within this profile; the GUI snapshot work is integrated through
PR35. Rendered pixel equality, whole-floor survival and physical audio/timing
are not established by these bounded restart measurements. Raw keyboard proof
is separate: the headless IBM5160 original-Pyro run reaches native BIOS IRQ1
F000:E987 for End make 4Fh while break CFh remains queued. Complete machine/disk
continuation over a fresh-process restart matches and consumes that break;
bounded native execution then reaches the game mode menu. This proves that
hardware path and restart state, not physical keyboard serial timing or GUI
manual-input parity.

The source/product binding, native failure controls and live/restart receipts
are retained in the parent pyro221_next repository under
`docs/evidence/martypc_keyboard_20261006/` (witness.json and inventory.json).

The configured PC speaker honors disabled host output when creating its sample
channel. Native PIT counters/gates/phase and PPI state still run. Host PCM sample
accumulation/connection/enablement intentionally differ from enabled playback.
The actual speaker=true factory regression and10000-cycle native comparison
pass; this does not establish physical audible parity. Other configured audio
owners remain explicitly subject to core snapshot preflight/refusal.

Native GUI snapshot methods also require CPU trace/on/file and disassembly
recording/file settings disabled; these external sinks are not persistent state.
The cold candidate factory independently rejects such settings.

## Coherent paused observation

`state.observe` shares the PyPC schema. Pass required `expected_state_revision`,
optional `memory` (up to16 `{address,length}` windows, at most65536 bytes total),
optional `video_text` (`{}`, or `page`/byte `display_address`) and boolean
`video_memory`. Unknown fields, stale revisions, running targets and malformed
later windows/options are refused with `-32602` before copying. Memory addresses
use the existing physical/linear/segmented conventions, and `length` defaults to1.

The result has one `state_revision`, `registers` in the existing register shape,
and ordered `memory` descriptors identical to `memory.read` (address, byte count,
hex/base64, SHA256 and revision). Requested video components carry that revision.
`video_memory` is the full16384-byte CGA backing VRAM with address `0xb8000` and
adapter `CGA`; it is not a rendered frame/font/graphics-plane snapshot.
RAM/ROM and native CGA VRAM are inspected through immutable native peeks.
Other MMIO is explicitly refused. Inspection does not execute CPU/device work,
flush prefetch, consume events or modify Machine state.

`video.text` returns the shared PyPC text schema: adapter, columns/rows,
page size/count, selected/active page, byte display address, mode/control byte,
CP437 text rows and raw cells with code/character/attribute/foreground/background/
blink. CRTC start words are converted to byte addresses, and cell reads wrap in
CGA's16KiB backing. Control characters are replaced by spaces only in text rows;
raw cells preserve codes. Rows are25, columns40 or80 from actual CGA mode control.
Only CGA text is supported; graphics and other adapters are refused rather than
being decoded as text. These are memory interpretations, not raster/font evidence.
The optional VGA cursor object that PyPC provides is absent for CGA on both backends.

video.text and state.observe.video_text refuse both page and display_address
keys together with -32602, even for equal values. Supply at most one selector.
