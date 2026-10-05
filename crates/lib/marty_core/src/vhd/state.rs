//! VHD-owned cached metadata and an independently authenticated disk payload.
//! Binary bytes stay outside JSON. The caller owns compression/storage and must
//! quiesce guest/host I/O. Access policy/path/backend and controller transfers
//! are separate resources; a matching hash alone does not prove their identity.
//! This authenticates disk bytes, not JSON metadata. The outer snapshot loader
//! must authenticate metadata; this component assumes trusted cached state.

use super::*;
use anyhow::Context;
use sha2::{Digest, Sha256};

/// Auto embeds up to the explicit limit and refuses a silent large reference.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum DiskCaptureMode {
    Auto,
    Embed,
    Reference,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VhdState {
    version: u32,
    read_only: bool,
    footer: VHDFileFooter,
    size: u64,
    checksum: u32,
    max_cylinders: u32,
    max_heads: u32,
    max_sectors: u32,
    cur_cylinder: u32,
    cur_head: u32,
    cur_sector: u32,
    io: DiskPayload,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DiskPayload {
    bytes: u64,
    sha256: [u8; 32],
    position: u64,
    embedded: bool,
}

fn read_payload(io: &mut dyn VhdIO, mode: DiskCaptureMode, limit: u64) -> Result<(DiskPayload, Option<Vec<u8>>)> {
    let position = io.stream_position()?;
    let result = (|| {
        let length = io.seek(SeekFrom::End(0))?;
        if length <= VHD_FOOTER_LEN as u64 || position > length {
            bail!("invalid VHD backing length/position");
        }
        let embedded = match mode {
            DiskCaptureMode::Embed => true,
            DiskCaptureMode::Reference => false,
            DiskCaptureMode::Auto if length <= limit => true,
            DiskCaptureMode::Auto => bail!("large disk requires explicit embed/reference choice"),
        };
        let mut data = if embedded {
            let mut data = Vec::new();
            data.try_reserve_exact(usize::try_from(length)?)?;
            Some(data)
        } else {
            None
        };
        io.seek(SeekFrom::Start(0))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 65536];
        let mut left = length;
        while left != 0 {
            let n = left.min(buffer.len() as u64) as usize;
            io.read_exact(&mut buffer[..n])?;
            hasher.update(&buffer[..n]);
            if let Some(data) = &mut data {
                data.extend_from_slice(&buffer[..n]);
            }
            left -= n as u64;
        }
        Ok((
            DiskPayload {
                bytes: length,
                sha256: hasher.finalize().into(),
                position,
                embedded,
            },
            data,
        ))
    })();
    // Attempt this even after a read/allocation/policy refusal. A seek failure
    // is reported explicitly; no successful snapshot may hide a lost cursor.
    io.seek(SeekFrom::Start(position))
        .context("restore VHD position after capture")?;
    result
}

/// File identity and native cached read_only flag for the outer snapshot loader.
/// Paths/backend access and metadata authentication remain caller obligations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskSnapshotRequirement {
    pub bytes: u64,
    pub sha256: [u8; 32],
    pub embedded: bool,
    /// Cached metadata only: native write_sector does not enforce this flag.
    /// The backing provider owns actual write permissions.
    pub read_only: bool,
}

impl VhdState {
    pub(crate) fn disk_requirement(&self) -> DiskSnapshotRequirement {
        DiskSnapshotRequirement { bytes: self.io.bytes, sha256: self.io.sha256,
            embedded: self.io.embedded, read_only: self.read_only }
    }

    fn preflight(&self) -> Result<()> {
        if self.version != 1
            || self.size <= VHD_FOOTER_LEN as u64
            || self.io.bytes <= VHD_FOOTER_LEN as u64
            || self.io.position > self.io.bytes
        {
            bail!("incompatible VHD snapshot version/size/position");
        }
        // Match the native parser's actual cached invariants, not a stricter
        // invented disk geometry/checksum policy. Its warnings remain valid.
        if self.footer.cookie != *b"conectix"
            || self.footer.version != VHD_VERSION
            || self.footer.offset != VHD_DATA_OFFSET
            || self.footer.disk_type != VHD_DISK_TYPE
        {
            bail!("incompatible cached VHD footer");
        }
        Ok(())
    }
}

impl VirtualHardDisk {
    /// Capture cached metadata and exact live bytes without consuming I/O.
    /// Embedded data is a separate binary payload, never a JSON byte array.
    pub(crate) fn snapshot_state(&mut self, mode: DiskCaptureMode, limit: u64) -> Result<(VhdState, Option<Vec<u8>>)> {
        let (io, payload) = read_payload(&mut *self.vhd_file, mode, limit)?;
        let saved = VhdState {
            version: 1,
            read_only: self.read_only,
            footer: self.footer.clone(),
            size: self.size,
            checksum: self.checksum,
            max_cylinders: self.max_cylinders,
            max_heads: self.max_heads,
            max_sectors: self.max_sectors,
            cur_cylinder: self.cur_cylinder,
            cur_head: self.cur_head,
            cur_sector: self.cur_sector,
            io,
        };
        saved.preflight()?;
        Ok((saved, payload))
    }

    /// Preflight a new provider, then construct a replacement. The live disk
    /// is untouched. The outer loader must supply the original I/O access and
    /// backend behavior in a fresh experiment location, never overwrite a user
    /// image. Missing/changed reference bytes refuse before device mutation.
    pub(crate) fn prepare_restore(saved: &VhdState, mut provider: Box<dyn VhdIO>) -> Result<Self> {
        saved.preflight()?;
        let (actual, _) = read_payload(&mut *provider, DiskCaptureMode::Reference, 0)?;
        if actual.bytes != saved.io.bytes || actual.sha256 != saved.io.sha256 {
            bail!("VHD dependency size/SHA-256 mismatch");
        }
        provider.seek(SeekFrom::Start(saved.io.position))?;
        // The live bytes can differ from the cached original footer after a
        // native failed write. Do not reparse/regenerate the cache from them.
        Ok(Self {
            vhd_file: provider,
            read_only: saved.read_only,
            footer: saved.footer.clone(),
            size: saved.size,
            checksum: saved.checksum,
            max_cylinders: saved.max_cylinders,
            max_heads: saved.max_heads,
            max_sectors: saved.max_sectors,
            cur_cylinder: saved.cur_cylinder,
            cur_head: saved.cur_head,
            cur_sector: saved.cur_sector,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, ErrorKind};

    fn disk_bytes() -> Vec<u8> {
        let mut bytes: Vec<u8> = (0..16 * SECTOR_SIZE).map(|i| (i * 37 + i / 512) as u8).collect();
        let mut footer = [0; VHD_FOOTER_LEN];
        VHDFileFooter::make_vhd_footer_bytes(&mut footer, VHDFileFooter::new(2, 2, 4, Uuid::from_bytes([17; 16])));
        bytes.extend_from_slice(&footer);
        bytes
    }

    fn disk(bytes: Vec<u8>) -> VirtualHardDisk {
        VirtualHardDisk::parse(Box::new(Cursor::new(bytes)), false).unwrap()
    }

    fn capture(vhd: &mut VirtualHardDisk) -> (VhdState, Vec<u8>) {
        let (state, bytes) = vhd.snapshot_state(DiskCaptureMode::Embed, 0).unwrap();
        (state, bytes.unwrap())
    }

    fn chs(lba: u8) -> (u16, u8, u8) {
        ((lba / 8) as u16, (lba / 4) % 2, lba % 4)
    }

    fn continuation(reference: &mut VirtualHardDisk, restored: &mut VirtualHardDisk, phase: u8) {
        assert_eq!(reference.geometry(), restored.geometry());
        assert_eq!(reference.size().unwrap(), restored.size().unwrap());
        for lba in 0..16 {
            let (c, h, s) = chs(lba);
            assert_eq!(reference.get_chs_offset(c, h, s), restored.get_chs_offset(c, h, s));
            let mut a = [0; SECTOR_SIZE];
            let mut b = [0; SECTOR_SIZE];
            reference.read_sector(&mut a, c, h, s).unwrap();
            restored.read_sector(&mut b, c, h, s).unwrap();
            assert_eq!(a, b, "native read lba {lba}");
            if lba % 3 == phase % 3 {
                a[3] ^= phase;
                reference.write_sector(&a, c, h, s).unwrap();
                restored.write_sector(&a, c, h, s).unwrap();
            }
        }
        let mut a = [0; SECTOR_SIZE];
        let mut b = [0; SECTOR_SIZE];
        assert!(reference.read_sector(&mut a, 2, 0, 0).is_err());
        assert!(restored.read_sector(&mut b, 2, 0, 0).is_err());
        assert_eq!(capture(reference), capture(restored));
    }

    #[test]
    fn native_sector_continuations_after_destructive_json_restore() {
        let mut reference = disk(disk_bytes());
        for phase in 0..64 {
            let (c, h, s) = chs(phase % 16);
            reference.write_sector(&[phase; SECTOR_SIZE], c, h, s).unwrap();
            let (saved, payload) = capture(&mut reference);
            let encoded = serde_json::to_vec(&saved).unwrap();
            let decoded: VhdState = serde_json::from_slice(&encoded).unwrap();
            let mut restored = VirtualHardDisk::prepare_restore(&decoded, Box::new(Cursor::new(payload))).unwrap();
            continuation(&mut reference, &mut restored, phase);
        }
        println!("VHD: 64 native JSON continuations; each checks 16 sectors, writes, bounds and full backing bytes");
    }

    #[test]
    fn native_failed_write_preserves_distinct_cached_footer() {
        let mut reference = disk(disk_bytes());
        let cached = reference.footer.clone();
        // Native write commits 514 bytes before refusing the unexpected count.
        // The last two bytes overwrite the footer cookie, but not its cache.
        assert!(reference.write_sector(&[0xA5; SECTOR_SIZE + 2], 1, 1, 3).is_err());
        let (saved, payload) = capture(&mut reference);
        assert_eq!(saved.footer, cached);
        assert_ne!(&payload[16 * SECTOR_SIZE..16 * SECTOR_SIZE + 2], b"co");
        assert!(VirtualHardDisk::parse(Box::new(Cursor::new(payload.clone())), false).is_err());
        let mut restored = VirtualHardDisk::prepare_restore(&saved, Box::new(Cursor::new(payload))).unwrap();
        continuation(&mut reference, &mut restored, 7);
        println!("VHD: native failed write corrupts live footer; cached-footer restore continues identically");
    }

    #[test]
    fn real_file_reopen_continuation_uses_fresh_backing_copy() {
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target"));
        let directory = target.join(format!("vhd-reopen-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let original = directory.join("original.vhd");
        let replacement = directory.join("replacement.vhd");
        fs::write(&original, disk_bytes()).unwrap();
        let open = |p: &std::path::Path| fs::OpenOptions::new().read(true).write(true).open(p).unwrap();
        let mut before = VirtualHardDisk::parse(Box::new(open(&original)), false).unwrap();
        before.write_sector(&[0x51; SECTOR_SIZE], 1, 0, 2).unwrap();
        let (saved, payload) = capture(&mut before);
        drop(before);
        fs::write(&replacement, payload).unwrap();
        // Reopen the reference through the independent native parser, not the
        // restore implementation under test. Native sector I/O did not alter
        // its cached metadata; recover only the observed backend position.
        let mut reference = VirtualHardDisk::parse(Box::new(open(&original)), false).unwrap();
        reference.vhd_file.seek(SeekFrom::Start(saved.io.position)).unwrap();
        assert_eq!(capture(&mut reference).0, saved);
        let mut restored = VirtualHardDisk::prepare_restore(&saved, Box::new(open(&replacement))).unwrap();
        continuation(&mut reference, &mut restored, 19);
        drop(reference);
        drop(restored);
        assert_eq!(fs::read(&original).unwrap(), fs::read(&replacement).unwrap());
        fs::remove_file(original).unwrap();
        fs::remove_file(replacement).unwrap();
        fs::remove_dir(directory).unwrap();
        println!("VHD: RW File resource closed/reopened into a fresh copy; NOT a process/machine restart proof");
    }

    #[test]
    fn authenticated_dependencies_refuse_without_mutating_live_disk() {
        let mut live = disk(disk_bytes());
        let before = capture(&mut live);
        let (saved, payload) = live.snapshot_state(DiskCaptureMode::Reference, 0).unwrap();
        assert!(payload.is_none());
        for mode in [false, true] {
            let mut state = saved.clone();
            state.io.embedded = mode;
            for changed in 0..3 {
                let mut bytes = before.1.clone();
                match changed {
                    0 => bytes[123] ^= 1,
                    1 => {
                        bytes.pop();
                    }
                    _ => bytes.clear(),
                }
                assert!(VirtualHardDisk::prepare_restore(&state, Box::new(Cursor::new(bytes))).is_err());
                assert_eq!(capture(&mut live), before);
            }
        }
        let mut restored = VirtualHardDisk::prepare_restore(&saved, Box::new(Cursor::new(before.1.clone()))).unwrap();
        continuation(&mut live, &mut restored, 2);
    }

    #[test]
    fn explicit_large_disk_policy_and_capture_cursor_preservation() {
        let bytes = disk_bytes();
        let mut live = disk(bytes.clone());
        live.vhd_file.seek(SeekFrom::Start(77)).unwrap();
        let length = bytes.len() as u64;
        assert!(live.snapshot_state(DiskCaptureMode::Auto, length - 1).is_err());
        assert_eq!(live.vhd_file.stream_position().unwrap(), 77);
        let (auto, payload) = live.snapshot_state(DiskCaptureMode::Auto, length).unwrap();
        assert!(auto.io.embedded);
        assert_eq!(payload.unwrap(), bytes);
        let (embedded, data) = capture(&mut live);
        assert_eq!(auto, embedded);
        assert_eq!(data, bytes);
        let (reference, none) = live.snapshot_state(DiskCaptureMode::Reference, 0).unwrap();
        assert!(!reference.io.embedded);
        assert!(none.is_none());
        assert_eq!(reference.io.sha256, embedded.io.sha256);
        assert_eq!(live.vhd_file.stream_position().unwrap(), 77);
    }

    fn native_fields(name: &str) -> std::collections::BTreeSet<String> {
        let source = include_str!("../vhd.rs");
        let body = source
            .split(&format!("pub struct {name} {{"))
            .nth(1)
            .unwrap()
            .split('}')
            .next()
            .unwrap();
        body.lines()
            .filter_map(|line| {
                let line = line.trim().strip_prefix("pub ").unwrap_or(line.trim());
                if line.starts_with("//") {
                    return None;
                }
                line.split_once(':').map(|(name, _)| name.trim().to_owned())
            })
            .collect()
    }

    fn keys(value: &serde_json::Value) -> std::collections::BTreeSet<String> {
        value.as_object().unwrap().keys().cloned().collect()
    }

    #[test]
    fn strict_schema_inventory_and_cached_storage_are_exact() {
        let mut live = disk(disk_bytes());
        // Storage-only sentinels for legacy fields without native producers.
        live.read_only = true; // Native write_sector does not enforce this flag.
        live.checksum = 0xA7329814;
        live.cur_cylinder = 7;
        live.cur_head = 3;
        live.cur_sector = 5;
        let (saved, payload) = capture(&mut live);
        let mut restored = VirtualHardDisk::prepare_restore(&saved, Box::new(Cursor::new(payload))).unwrap();
        assert_eq!(capture(&mut restored), capture(&mut live));
        let encoded = serde_json::to_value(saved).unwrap();
        let mut native = native_fields("VirtualHardDisk");
        native.remove("vhd_file");
        native.extend(["version".to_owned(), "io".to_owned()]);
        assert_eq!(keys(&encoded), native);
        assert_eq!(keys(&encoded["footer"]), native_fields("VHDFileFooter"));
        assert_eq!(keys(&encoded["footer"]["geometry"]), native_fields("VHDGeometry"));
        for path in ["", "/footer", "/footer/geometry", "/io"] {
            for key in keys(encoded.pointer(path).unwrap()) {
                let mut missing = encoded.clone();
                missing.pointer_mut(path).unwrap().as_object_mut().unwrap().remove(&key);
                assert!(
                    serde_json::from_value::<VhdState>(missing).is_err(),
                    "required {path}/{key}"
                );
            }
            let mut extra = encoded.clone();
            extra
                .pointer_mut(path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("unrecognized".into(), true.into());
            assert!(serde_json::from_value::<VhdState>(extra).is_err(), "strict {path}");
        }
        for path in [
            "/version",
            "/size",
            "/io/bytes",
            "/footer/version",
            "/footer/disk_type",
            "/footer/offset",
        ] {
            let mut bad = encoded.clone();
            *bad.pointer_mut(path).unwrap() = 0.into();
            let bad: VhdState = serde_json::from_value(bad).unwrap();
            assert!(bad.preflight().is_err(), "invalid {path}");
        }
        let mut bad = encoded;
        bad["io"]["position"] = u64::MAX.into();
        assert!(serde_json::from_value::<VhdState>(bad).unwrap().preflight().is_err());
    }

    #[test]
    fn native_warning_footer_is_preserved_without_canonicalization() {
        let mut bytes = disk_bytes();
        bytes[16 * SECTOR_SIZE + 11] ^= 4; // features warning
        bytes[16 * SECTOR_SIZE + VHD_CHECKSUM_OFFSET] ^= 1; // checksum warning
        let mut live = disk(bytes.clone());
        let (saved, payload) = capture(&mut live);
        let mut restored = VirtualHardDisk::prepare_restore(&saved, Box::new(Cursor::new(payload))).unwrap();
        assert_eq!(capture(&mut restored), (saved, bytes));
        continuation(&mut live, &mut restored, 11);
    }

    struct FailingRead {
        inner: Cursor<Vec<u8>>,
        fail_read: bool,
        fail_restore: bool,
        position: u64,
    }

    impl Read for FailingRead {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.fail_read {
                self.fail_read = false;
                return Err(std::io::Error::new(ErrorKind::Other, "injected read failure"));
            }
            self.inner.read(buf)
        }
    }
    impl Write for FailingRead {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.inner.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.inner.flush()
        }
    }
    impl Seek for FailingRead {
        fn seek(&mut self, seek: SeekFrom) -> std::io::Result<u64> {
            if self.fail_restore && seek == SeekFrom::Start(self.position) {
                return Err(std::io::Error::new(ErrorKind::Other, "injected restore seek failure"));
            }
            self.inner.seek(seek)
        }
    }

    #[test]
    fn failed_capture_restores_cursor_or_reports_seek_failure() {
        for fail_restore in [false, true] {
            let bytes = disk_bytes();
            let mut io = FailingRead {
                inner: Cursor::new(bytes.clone()),
                fail_read: true,
                fail_restore,
                position: 43,
            };
            io.inner.set_position(43);
            let error = read_payload(&mut io, DiskCaptureMode::Embed, 0).unwrap_err();
            if fail_restore {
                assert!(error.to_string().contains("restore VHD position"));
            } else {
                assert_eq!(io.inner.position(), 43);
                assert!(error.to_string().contains("injected read failure"));
            }
            assert_eq!(io.inner.into_inner(), bytes);
        }
    }

    #[test]
    fn restore_provider_read_and_final_seek_fail_without_live_mutation() {
        let mut live = disk(disk_bytes());
        live.vhd_file.seek(SeekFrom::Start(77)).unwrap();
        let before = capture(&mut live);
        for fail_read in [true, false] {
            let provider = FailingRead {
                inner: Cursor::new(before.1.clone()),
                fail_read,
                fail_restore: !fail_read,
                position: 77,
            };
            let result = VirtualHardDisk::prepare_restore(&before.0, Box::new(provider));
            let error = match result {
                Err(error) => error,
                Ok(_) => panic!("injected restore failure accepted"),
            };
            let expected = if fail_read {
                "injected read failure"
            } else {
                "injected restore seek failure"
            };
            assert!(error.to_string().contains(expected), "{error}");
            assert_eq!(capture(&mut live), before, "live disk unchanged after provider failure");
        }
    }
}
