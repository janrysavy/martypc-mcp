# Small authenticated Windows release

The public `Debugger` workflow keeps its ordinary Windows/Linux checks.
Manual dispatch with Boolean `release_only=true` instead builds the GUI with
Rust1.98.0, `--release --locked` and the default features. The two modes have
separate concurrency groups, so a release request does not cancel normal checks.

`util/windows_release.py` requires a clean source checkout, captures every tracked
file hash/head/tree before and after, removes the owned primary/dependency EXEs,
and requires Cargo `fresh=false`, optimization3, no debug assertions/info or test
profile, fresh identical executable copies and a bounded x64 PE32+ payload.
Unspecified Rust/profile overrides refuse. `util/test_windows_release.py` exercises
source, profile, stale-product, hash, path and coherently re-signed PE refusals.

The artifact contains only `martypc.exe`, `receipt.json` and `LICENSE`, with
one-day retention; no ROMs, DOS/game inputs, target cache or debug products.
Downloaders must authenticate the actual Actions run/commit, compare its tree
and source file hashes to the expected checkout, then call `verify` with that
independent source snapshot and the downloaded executable bytes. Record the
actual build commit even after a rebase merge; tree equality does not relabel
the embedded Git version. Bind new snapshots to the actual executable hash.

After durable local verification, delete the remote artifact when no longer
needed. Existing snapshots from other binaries are not made compatible by this
workflow. The receipt is build provenance, not emulator or gameplay acceptance.
