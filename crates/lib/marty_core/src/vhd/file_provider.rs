//! File providers with a constructor-enforced read/write, non-append contract.
//! Snapshot metadata flags do not replace these OS handle access rights.
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
};

pub struct SnapshotRwFile {
    file: File,
}
impl SnapshotRwFile {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        Self::check(OpenOptions::new().read(true).write(true).open(path)?)
    }
    pub fn create_new(path: impl AsRef<Path>) -> io::Result<Self> {
        Self::check(OpenOptions::new().read(true).write(true).create_new(true).open(path)?)
    }
    fn check(file: File) -> io::Result<Self> {
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "snapshot disk must be a regular RW File",
            ));
        }
        Ok(Self { file })
    }
    pub fn sync_all(&self) -> io::Result<()> {
        self.file.sync_all()
    }
}
impl Read for SnapshotRwFile {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.file.read(bytes)
    }
}
impl Write for SnapshotRwFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.file.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
impl Seek for SnapshotRwFile {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}
