//! Versioned MartyPC storage, not interchangeable with PyPC/DOSBox snapshots.
//! Capture must already be quiesced. Import requires an independently retained
//! archive digest and exact build identity; a checksum inside the archive cannot
//! authenticate its own metadata. No paths are extracted or treated as identity.
//! Native candidate preflight, provider access policy, frontend reconnection and
//! the final live swap remain necessary after decoding.

use super::MachineSnapshot;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const FORMAT: &str = "martypc-machine";
const MANIFEST: &str = "manifest.json";
const MACHINE: &str = "machine.json";
const DISKS: [&str; 2] = ["disks/0.vhd", "disks/1.vhd"];

/// Invalid authenticated input, distinguishable from actual host I/O failures.
/// RPC callers map this category to invalid parameters without parsing text.
#[derive(Debug)]
pub struct SnapshotDependencyMismatch(pub &'static str);

impl std::fmt::Display for SnapshotDependencyMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for SnapshotDependencyMismatch {}

/// Host resource budgets, not guest emulation constants. Reference bytes count
/// towards the total as well. The caller may explicitly increase these limits.
#[derive(Clone, Copy, Debug)]
pub struct SnapshotArchiveLimits {
    pub archive_bytes: u64,
    pub metadata_bytes: u64,
    pub total_bytes: u64,
}

impl Default for SnapshotArchiveLimits {
    fn default() -> Self {
        Self {
            archive_bytes: 128 << 20,
            metadata_bytes: 32 << 20,
            total_bytes: 512 << 20,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Blob {
    bytes: u64,
    sha256: [u8; 32],
}

impl Blob {
    fn of(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.len() as u64,
            sha256: digest(bytes),
        }
    }
    fn verify(&self, bytes: &[u8]) -> Result<()> {
        if self.bytes != bytes.len() as u64 || self.sha256 != digest(bytes) {
            return Err(SnapshotDependencyMismatch("snapshot member length/checksum mismatch").into());
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    version: u32,
    build: [u8; 32],
    machine: Blob,
    disks: [Option<Blob>; 2],
}

/// Authenticated decoded storage only. Both embedded and supplied reference
/// disks are checked. A fresh Machine must still validate its native state and
/// dependencies; this value is never permission to mutate a live Machine.
pub struct DecodedSnapshot {
    pub machine: MachineSnapshot,
    pub disks: [Option<Vec<u8>>; 2],
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

// This bounded format writes ordinary single-volume ZIP32, without comments.
// zip2 collapses duplicate central-directory names into an IndexMap, so len()
// alone hides duplicates. Check the raw declared count before trusting that map.
fn archive_member_count(bytes: &[u8]) -> Result<usize> {
    let end = bytes.len().checked_sub(22).context("truncated snapshot ZIP footer")?;
    let footer = &bytes[end..];
    let word = |i| u16::from_le_bytes([footer[i], footer[i + 1]]);
    let dword = |i| u32::from_le_bytes(footer[i..i + 4].try_into().unwrap()) as u64;
    let count = usize::from(word(10));
    if &footer[..4] != b"PK\x05\x06"
        || word(4) != 0
        || word(6) != 0
        || word(8) != word(10)
        || word(20) != 0
        || !(2..=4).contains(&count)
        || dword(16).checked_add(dword(12)) != Some(end as u64)
    {
        bail!("unsupported snapshot ZIP footer/layout");
    }
    Ok(count)
}

fn charge(total: &mut u64, bytes: u64, limit: u64) -> Result<()> {
    *total = total.checked_add(bytes).context("snapshot byte budget overflow")?;
    if *total > limit {
        bail!("snapshot total byte budget exceeded");
    }
    Ok(())
}

/// Returns compressed bytes and their digest for independent retention. Only
/// the actual embedded slots may have payloads; references remain external.
pub fn encode_snapshot_archive(
    machine: &MachineSnapshot,
    disks: &[Option<Vec<u8>>; 2],
    build: [u8; 32],
    limits: SnapshotArchiveLimits,
) -> Result<(Vec<u8>, [u8; 32])> {
    let machine_bytes = serde_json::to_vec(machine)?;
    if machine_bytes.len() as u64 > limits.metadata_bytes {
        bail!("snapshot metadata byte budget exceeded");
    }
    let mut total = 0;
    charge(&mut total, machine_bytes.len() as u64, limits.total_bytes)?;
    let requirements = machine.disk_requirements();
    for slot in 0..2 {
        match (&requirements[slot], &disks[slot]) {
            (Some(req), Some(bytes)) if req.embedded => {
                Blob {
                    bytes: req.bytes,
                    sha256: req.sha256,
                }
                .verify(bytes)?;
                charge(&mut total, req.bytes, limits.total_bytes)?;
            }
            (Some(req), None) if !req.embedded => {
                charge(&mut total, req.bytes, limits.total_bytes)?;
            }
            (None, None) => {}
            _ => bail!("snapshot disk payload mode mismatch at slot {slot}"),
        }
    }
    let manifest = serde_json::to_vec(&Manifest {
        format: FORMAT.into(),
        version: 1,
        build,
        machine: Blob::of(&machine_bytes),
        disks: std::array::from_fn(|slot| disks[slot].as_deref().map(Blob::of)),
    })?;
    if manifest.len() > 65536 {
        bail!("snapshot manifest too large");
    }
    charge(&mut total, manifest.len() as u64, limits.total_bytes)?;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in [(MANIFEST, manifest.as_slice()), (MACHINE, machine_bytes.as_slice())]
        .into_iter()
        .chain((0..2).filter_map(|slot| disks[slot].as_deref().map(|bytes| (DISKS[slot], bytes))))
    {
        zip.start_file(name, options)?;
        zip.write_all(bytes)?;
    }
    let bytes = zip.finish()?.into_inner();
    if bytes.len() as u64 > limits.archive_bytes {
        bail!("snapshot archive byte budget exceeded");
    }
    archive_member_count(&bytes)?;
    let checksum = digest(&bytes);
    Ok((bytes, checksum))
}

/// Decode without writing files or changing guest state. Expected build/digest
/// must come from outside this archive. Reference slots require matching bytes;
/// extra references, duplicate/unknown members and oversized data are refused.
pub fn decode_snapshot_archive(
    bytes: &[u8],
    expected_sha256: [u8; 32],
    expected_build: [u8; 32],
    references: [Option<Vec<u8>>; 2],
    limits: SnapshotArchiveLimits,
) -> Result<DecodedSnapshot> {
    if bytes.len() as u64 > limits.archive_bytes || digest(bytes) != expected_sha256 {
        return Err(SnapshotDependencyMismatch("snapshot archive size/checksum mismatch").into());
    }
    let count = archive_member_count(bytes)?;
    let mut zip = ZipArchive::new(Cursor::new(bytes))?;
    if zip.len() != count || zip.offset() != 0 {
        bail!("snapshot duplicate member/offset mismatch");
    }
    let mut members = BTreeMap::new();
    let mut total = 0;
    // Inspect every header and charge the declared unpacked lengths before any
    // decompression. Then bound the actual decoder output independently.
    for index in 0..zip.len() {
        let file = zip.by_index(index)?;
        let name = file.name().to_owned();
        let ceiling = match name.as_str() {
            MANIFEST => 65536,
            MACHINE => limits.metadata_bytes,
            "disks/0.vhd" | "disks/1.vhd" => limits.total_bytes,
            _ => bail!("unknown snapshot member {name}"),
        };
        if file.compression() != CompressionMethod::Deflated
            || file.size() > ceiling
            || members.insert(name, (index, file.size())).is_some()
        {
            bail!("snapshot member compression/size/duplicate mismatch");
        }
        charge(&mut total, file.size(), limits.total_bytes)?;
    }
    fn read(
        zip: &mut ZipArchive<Cursor<&[u8]>>,
        members: &BTreeMap<String, (usize, u64)>,
        name: &str,
    ) -> Result<Vec<u8>> {
        let &(index, size) = members.get(name).context("missing snapshot member")?;
        let mut out = Vec::new();
        out.try_reserve_exact(usize::try_from(size)?)?;
        zip.by_index(index)?
            .take(size.checked_add(1).context("snapshot size overflow")?)
            .read_to_end(&mut out)?;
        if out.len() as u64 != size {
            bail!("snapshot unpacked size mismatch");
        }
        Ok(out)
    }
    let manifest: Manifest = serde_json::from_slice(&read(&mut zip, &members, MANIFEST)?)?;
    if manifest.format != FORMAT || manifest.version != 1 || manifest.build != expected_build {
        bail!("incompatible snapshot format/version/build");
    }
    let raw_machine = read(&mut zip, &members, MACHINE)?;
    manifest.machine.verify(&raw_machine)?;
    let machine: MachineSnapshot = serde_json::from_slice(&raw_machine)?;
    let requirements = machine.disk_requirements();
    let mut disks: [Option<Vec<u8>>; 2] = [None, None];
    for (slot, reference) in references.into_iter().enumerate() {
        let present = members.contains_key(DISKS[slot]);
        let bytes = match &requirements[slot] {
            Some(req) if req.embedded => {
                if reference.is_some() || !present {
                    bail!("embedded snapshot slot mismatch");
                }
                let raw = read(&mut zip, &members, DISKS[slot])?;
                manifest.disks[slot]
                    .as_ref()
                    .context("missing embedded disk descriptor")?
                    .verify(&raw)?;
                raw
            }
            Some(req) => {
                if present || manifest.disks[slot].is_some() {
                    bail!("referenced snapshot slot mismatch");
                }
                charge(&mut total, req.bytes, limits.total_bytes)?;
                reference.context("missing snapshot reference disk")?
            }
            None => {
                if present || manifest.disks[slot].is_some() || reference.is_some() {
                    bail!("orphan snapshot disk");
                }
                continue;
            }
        };
        let req = requirements[slot].as_ref().unwrap();
        Blob {
            bytes: req.bytes,
            sha256: req.sha256,
        }
        .verify(&bytes)?;
        disks[slot] = Some(bytes);
    }
    Ok(DecodedSnapshot { machine, disks })
}
