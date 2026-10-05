# MartyPC snapshot archive

This is MartyPC storage only. PyPC/DOSBox snapshots are different formats.

`marty_core::machine::storage` encodes a quiesced `MachineSnapshot` and its
embedded disk payloads into a single ZIP32/Deflate archive. Fixed members are
`manifest.json`, `machine.json`, and optional `disks/0.vhd` / `disks/1.vhd`.
The strict version 1 manifest binds machine metadata and embedded disk bytes
by raw length and SHA-256, plus an independently expected emulator build ID.
The returned whole-file digest must be retained separately and supplied on
import. Checksums inside a changed archive cannot authenticate themselves.

Reference slots require explicit matching raw bytes on import. Both disk slots
are checked before decoding returns; extra/missing references and mode mismatches
are refused. Filenames are never extracted or treated as identities. The
Machine's native preflight still validates configuration/ROMs and nested state.
The caller must choose fresh disk providers and preserve the native cached read_only flag;
native write_sector does not enforce that flag. Actual provider permissions,
host access/backend/alias policy are not proved by content equality.

Default limits are 128 MiB compressed archive, 32 MiB machine JSON and 512 MiB total
uncompressed metadata/disks including supplied references. These are adjustable
host storage budgets. The existing capture policy `Auto` uses the caller's
explicit embed limit; the parent selected 64 MiB, refusing an implicit large-disk
reference. Writable experiment disks should normally be embedded.

The container uses ordinary single-volume ZIP32 without archive comments.
Unknown paths, duplicate names, unsupported compression and byte-budget failures
are refused. The ZIP reader collapses duplicate names; a raw footer count check
is needed before trusting the reader's name map. No general ZIP extraction API
or ZIP64 snapshot support is claimed.

File publication and snapshot provenance are caller obligations: save to a new
experiment path, close/flush it, retain its checksum/build identity and reopen it
for verification. Never overwrite current experiment disks during import.
Execution and all frontend consumers/producers must be quiesced. Construct and
validate a fresh Machine, reconnect external resources, then perform one live
swap. The decoder itself never mutates a running Machine.

Fresh Windows serial+sound core run: 317 tests pass, 597 compiler inputs bound
before/after. A synced archive File is closed and reopened; a fresh Machine with
two VHDs preserves the cached read_only flag (metadata only) and resumes a partial ATA read at byte 17,
matching an independently patterned sector and subsequent native CPU/PIT/CGA
cycles and complete captured storage. Reference validation refuses changed bytes
in either slot, then original matching bytes restore. Metadata/build/digest,
missing/duplicate/unknown members and byte-budget controls are refused. The first
reader accepted duplicate filenames; the recorded before failure passes after
the raw count guard. A fresh normal Windows core/headless build also passes.
Exact sources/products/failures are retained in the parent evidence directory.

This is same-process native candidate and actual File persistence proof.
Frontend/RPC save/load, host provider policy, fresh-process continuation and
original Pyro replay remain unimplemented. Test build IDs are fixture constants;
actual executable build binding and external checksum retention must be proven
by the frontend. The scoped review found the read-only-policy wording overclaimed:
corrected to cached metadata, preserving native writer semantics. Its other
provenance/process/provider limits remain open; no concrete container codec
defect was identified. Crafted decompression-bomb coverage is not claimed.
Final CI is pending. The wording/comment correction was not re-reviewed.
