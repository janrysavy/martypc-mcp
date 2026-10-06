//! Host-side persistence for frontends with known read/write File VHD providers.
//! A factory constructs a cold Machine from the frontend's loaded configuration,
//! ROM bytes and keyboard mapping. It must not borrow/move the live Machine or
//! attach its disks. GUI consumers must be rebound before using this API there.
use super::*;
use marty_core::{
    machine::storage::{decode_snapshot_archive, encode_snapshot_archive, SnapshotArchiveLimits, SnapshotDependencyMismatch},
    vhd::{DiskCaptureMode, SnapshotRwFile, VhdIO},
};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

type Factory<'a> = dyn FnMut() -> std::result::Result<Machine, String> + 'a;

/// Actual running product identity, cached once by a frame-driven frontend.
/// The digest is private: callers cannot substitute a snapshot's build label.
pub struct SnapshotExecutable([u8; 32]);

impl SnapshotExecutable {
    pub fn current() -> std::io::Result<Self> {
        let mut executable = File::open(std::env::current_exe()?)?;
        let mut hash = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let n = executable.read(&mut buffer)?;
            if n == 0 { break; }
            hash.update(&buffer[..n]);
        }
        Ok(Self(hash.finalize().into()))
    }
}

#[derive(Copy, Clone)]
pub enum SnapshotFrontend { Headless, NativeGui }

/// Enable only for a frontend whose mounted VHD providers are known RW Files.
/// Cached VHD read_only metadata is not an OS access policy. Other providers
/// must remain unsupported until their access/backend contract is implemented.
pub struct SnapshotHost<'a> {
    factory: &'a mut Factory<'a>,
    build: [u8; 32],
    frontend: SnapshotFrontend,
    limits: SnapshotArchiveLimits,
}

impl<'a> SnapshotHost<'a> {
    pub fn for_rw_files(factory: &'a mut Factory<'a>) -> std::io::Result<Self> {
        let executable = SnapshotExecutable::current()?;
        Ok(Self::for_rw_files_with_executable(factory, &executable, SnapshotFrontend::Headless))
    }

    /// Reuse an identity made from the actual EXE; avoid hashing it each repaint.
    pub fn for_rw_files_with_executable(
        factory: &'a mut Factory<'a>,
        executable: &SnapshotExecutable,
        frontend: SnapshotFrontend,
    ) -> Self {
        Self { factory, build: executable.0, frontend, limits: SnapshotArchiveLimits::default() }
    }

    fn export(&mut self, machine: &mut Machine, p: &Value) -> Result<Value> {
        if !machine.snapshot_rw_files() {
            return invalid("mounted snapshot providers must be constructor-enforced RW Files");
        }
        let path = path(p, "path")?;
        let mode_name = match p.get("disk_mode") {
            None => "embed",
            Some(value) => value.as_str().ok_or(("disk_mode string required", -32602))?,
        };
        let mode = match mode_name {
            "embed" | "auto" => DiskCaptureMode::Embed,
            "reference" | "reference-files" => DiskCaptureMode::Reference,
            _ => return invalid("disk_mode must be embed, auto, reference or reference-files"),
        };
        let (saved, payloads) = machine
            .snapshot_state_quiesced(mode, self.limits.total_bytes)
            .map_err(host_error)?;
        let (archive, sha) = encode_snapshot_archive(&saved, &payloads, self.build, self.limits).map_err(host_error)?;
        // create_new refuses existing products. Never replace a caller's file.
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(host_error)?;
        let result = file.write_all(&archive).and_then(|_| file.sync_all());
        drop(file);
        if let Err(error) = result {
            if let Err(cleanup) = fs::remove_file(&path) {
                eprintln!("Snapshot cleanup failed; retained {}: {cleanup}", path.display());
            } // only the file just created by us
            return Err(host_error(error));
        }
        Ok(json!({"path":path,"sha256":hex(&sha),"bytes":archive.len(),
            "format":"martypc-machine","version":1,"build_sha256":hex(&self.build),
            "disk_mode":if mode_name == "reference-files" {"reference-files"} else if matches!(mode, DiskCaptureMode::Reference) {"reference"} else {"embed"},
            "disk_access":"rw-file"}))
    }

    fn prepare(&mut self, p: &Value) -> Result<(Machine, Value)> {
        let archive_path = path(p, "path")?;
        let disk_root = path(p, "disk_root")?;
        if p.get("expected_sha256").is_some() && p.get("sha256").is_some() {
            return invalid("provide only expected_sha256 or legacy sha256");
        }
        let expected = checksum(p.get("expected_sha256").or_else(|| p.get("sha256")).unwrap_or(&Value::Null))?;
        let archive = read_bounded(&archive_path, self.limits.archive_bytes)?;
        let mut references: [Option<Vec<u8>>; 2] = [None, None];
        let mut reference_bytes = 0u64;
        if let Some(value) = p.get("references") {
            let object = value
                .as_object()
                .ok_or(("references must map slots 0/1 to files", -32602))?;
            for (slot, value) in object {
                let index = match slot.as_str() {
                    "0" => 0,
                    "1" => 1,
                    _ => return invalid("reference slot must be 0 or 1"),
                };
                let name = value
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or(("reference path required", -32602))?;
                let bytes = read_bounded(Path::new(name), self.limits.total_bytes - reference_bytes)?;
                reference_bytes += bytes.len() as u64;
                references[index] = Some(bytes);
            }
        }
        let decoded =
            decode_snapshot_archive(&archive, expected, self.build, references, self.limits).map_err(|error| {
                if error.is::<SnapshotDependencyMismatch>() {
                    eprintln!("Snapshot refused: {error}");
                    ("snapshot dependency size/checksum mismatch", -32602)
                } else {
                    host_error(error)
                }
            })?;
        let candidate = (self.factory)().map_err(host_error)?;
        // Both disk modes restore into separate writable copies, never alias a
        // reference or currently mounted file. Require a NEW directory, even
        // for an empty snapshot. Its parent must already exist.
        fs::create_dir(&disk_root).map_err(host_error)?;
        let mut owned = StagedDisks {
            root: disk_root,
            paths: Vec::new(),
            committed: false,
        };
        let mut providers: [Option<Box<dyn VhdIO>>; 2] = [None, None];
        for (slot, bytes) in decoded.disks.into_iter().enumerate() {
            if let Some(bytes) = bytes {
                let name = owned.root.join(format!("disk-{slot}.vhd"));
                let mut file = SnapshotRwFile::create_new(&name).map_err(host_error)?;
                owned.paths.push(name);
                file.write_all(&bytes)
                    .and_then(|_| file.sync_all())
                    .map_err(host_error)?;
                providers[slot] = Some(Box::new(file));
            }
        }
        // On refusal, providers/candidate are dropped before our owned paths.
        // The caller's independent live Machine has never been moved or touched.
        let restored = candidate
            .prepare_snapshot_restore(&decoded.machine, providers)
            .map_err(host_error)?;
        let receipt = json!({"path":archive_path,"sha256":hex(&expected),"build_sha256":hex(&self.build),
            "disk_root":owned.root,"disk_paths":owned.paths,"disk_access":"rw-file"});
        owned.committed = true;
        Ok((restored, receipt))
    }
}

struct StagedDisks {
    root: PathBuf,
    paths: Vec<PathBuf>,
    committed: bool,
}
impl Drop for StagedDisks {
    fn drop(&mut self) {
        if !self.committed {
            for path in &self.paths {
                if let Err(error) = fs::remove_file(path) {
                    eprintln!("Snapshot cleanup failed; retained {}: {error}", path.display());
                }
            }
            if let Err(error) = fs::remove_dir(&self.root) {
                eprintln!("Snapshot cleanup failed; retained {}: {error}", self.root.display());
            }
        }
    }
}
fn path(p: &Value, name: &str) -> Result<PathBuf> {
    p[name]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .ok_or(("nonempty snapshot path/disk_root required", -32602))
}
fn checksum(value: &Value) -> Result<[u8; 32]> {
    let text = value
        .as_str()
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or(("independently retained expected_sha256 required", -32602))?;
    let mut sha = [0; 32];
    for (i, byte) in sha.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16)
            .map_err(|_| ("expected_sha256 must be 64 hexadecimal characters", -32602))?;
    }
    Ok(sha)
}
fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn host_error(error: impl std::fmt::Display) -> (&'static str, i32) {
    eprintln!("Snapshot refused: {error}");
    ("snapshot host/dependency validation failed; see emulator log", -32000)
}
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = File::open(path).map_err(host_error)?;
    if !file.metadata().map_err(host_error)?.is_file() {
        return invalid("snapshot input must be a regular file");
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).map_err(host_error)?;
    if bytes.len() as u64 > limit {
        return invalid("snapshot input byte budget exceeded");
    }
    Ok(bytes)
}

impl Agent {
    pub(super) fn handle_with_snapshots(
        &mut self,
        machine: &mut Machine,
        method: &str,
        p: &Value,
        host: Option<&mut SnapshotHost<'_>>,
    ) -> Result<Value> {
        match (method, host) {
            ("machine.snapshot.export" | "machine.snapshot.import", Some(host)) => {
                self.paused()?;
                if number(&p["expected_state_revision"])? != self.revision {
                    return invalid("state revision mismatch");
                }
                if method == "machine.snapshot.export" {
                    let mut result = host.export(machine, p)?;
                    result["state_revision"] = json!(self.revision);
                    return Ok(result);
                }
                let preserve_breakpoints = match p.get("preserve_breakpoints") {
                    None => true,
                    Some(value) => value.as_bool().ok_or(("preserve_breakpoints must be boolean", -32602))?,
                };
                let (candidate, mut result) = host.prepare(p)?;
                // All fallible dependency checks and provider preparation are
                // finished. A GUI caller must handle the pump restore signal before
                // processing another request/instruction or using derived consumers.
                *machine = candidate;
                self.revision += 1;
                self.restore_generation += 1;
                self.running = false;
                self.step_pending = false;
                self.control = ExecutionControl::new();
                if !preserve_breakpoints { self.breakpoints.clear(); }
                self.trace = Default::default();
                self.hardware = Default::default();
                self.predicate = None;
                self.skip_once = None;
                self.operation = None;
                self.completed.clear();
                self.completed_order.clear();
                self.deadline = None;
                // Keep next monotonic: pre-restore IDs must never identify new operations.
                self.last_stop = json!({"kind":"snapshot_restored","registers":registers(machine,self.revision)});
                result["state_revision"] = json!(self.revision);
                result["registers"] = registers(machine, self.revision);
                result["paused"] = json!(true);
                Ok(result)
            }
            ("agent.capabilities" | "emulator.info", Some(host)) => {
                let mut result = self.handle(machine, method, p)?;
                let methods = result["methods"].as_array_mut().unwrap();
                methods.extend([json!("machine.snapshot.export"), json!("machine.snapshot.import")]);
                result["unsupported"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|v| v != "machine.snapshot");
                result["snapshot"] = json!({"format":"martypc-machine","version":1,
                    "build_sha256":hex(&host.build),"disk_access":"rw-file",
                    "expected_sha256":"required independently retained digest",
                    "frontend":match host.frontend { SnapshotFrontend::Headless => "headless", SnapshotFrontend::NativeGui => "native-gui" },"profile":"supported core owners only; active logging/audio queues refused"});
                Ok(result)
            }
            _ => self.handle(machine, method, p),
        }
    }
}

#[cfg(test)]
mod tests;
