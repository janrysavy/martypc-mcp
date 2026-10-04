# Headless debugger JSON-RPC

Build and test the native frontend without GUI dependencies:

```text
cargo test -p martypc_headless --no-default-features --lib --locked
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
No-port headless execution still has no event loop.

The server binds only `127.0.0.1`. Send one JSON-RPC 2.0 object per newline over
TCP; connections are persistent. Replies preserve request IDs. Notifications
execute without replies. Batches are unsupported. Lines are limited to 1 MiB;
an oversized or unterminated line closes that connection. All machine work runs
on one thread; transport threads never inspect or mutate the machine. Host
polling does not advance a paused machine.

This is a **subset** of the PyPC debugger contract. The existing PyPC
`guest.dos_control.RPC` transport, `read` and `write` methods work unchanged for
supported memory ranges. Query `agent.capabilities` before using higher-level
controllers. In particular DOSCTRL's `D800:0000` mailbox is outside conventional
RAM: a working transport does **not** establish DOSCTRL support.

| Method | Parameters and result |
| --- | --- |
| `agent.capabilities`, `emulator.info` | Exact aliases; methods, limits, unsupported features, CPU cycle frequency and time-base description. |
| `session.status` | Session state, revision, CPU clock, CPU/video metadata and retained `last_stop`. Optional `session_id` must be `martypc` (discover it; do not hardcode PyPC's backend identity). |
| `state.get`, `state.get_registers` | Exact aliases; `general`, `segments`, architectural `ip`, `flags`, `flags_text`, CPU `clock`, `emulated_time_ns`, `in_hlt`, `state_revision`. |
| `state.set_registers` | Paused only. Requires `expected_state_revision`, `expected` values and nonempty `set`. AX/BX/CX/DX/SP/BP/SI/DI/CS/DS/ES/SS/IP/FLAGS, lowercase. All guards and Word ranges checked before any write. Returns `before`/`after`. |
| `memory.read` | `address`, optional `length` (default 1, maximum 65536). Returns physical address, byte count, hex, base64, SHA-256 and revision. Uses native bus peeks. |
| `memory.write` | Paused only. `address`, `data_base64`, optional `expected_sha256` (case insensitive). Preflights the entire range; only unmapped conventional RAM is writable. Returns before/after hashes and revision. |
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

**Unsupported:** instruction/hardware tracing, memory/interrupt watchpoints,
step-over, video/VNC, keyboard injection, serial channels, DOSCTRL, snapshots,
frontend file-transfer services and frontend speed/cursor controls. Unsupported
PPI software-turbo configurations are refused before the listener starts:
their native `frame_update` housekeeping is not scheduled by this frontend.
Host serial polling likewise requires that housekeeping and remains unsupported.
Other unsupported methods return errors; this is not a full PyPC replacement. Native CPU/device
tests and a real executable probe establish the listed subset. Full DOS/Pyro
boot, original CRT calibration and runtime timing comparisons remain unproved.

In the parent Pyro repository, `python scripts/test_martypc_rpc_live.py` makes a
writable scratch installation, loads a nine-byte MOV/INC/store/loop probe,
uses the pinned unchanged PyPC Python client, and closes its owned process.
It does not boot DOS or execute Pyro. Automatic fork CI tests and builds this
frontend on Windows/Linux without uploading artifacts or caching build trees.
The inherited macOS and WASM workflows are available only by manual dispatch.
