//! Disk-owned CHS/geometry cache, independently of the mounted VHD cache.
//! Native unload leaves the previous CHS behind; set_geometry need not agree
//! with VHD geometry. Preserve both without normalizing or resetting them.
//! ATA transfer buffers/latches and controller state remain separate owners.

use super::*;
use crate::vhd::{DiskCaptureMode, VhdIO, VhdState};
use anyhow::{bail, Result};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiskState {
    version: u32,
    position: (u16, u8, u8),
    geometry: (u16, u8, u8, u8, usize),
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    vhd: Option<VhdState>,
}

impl DiskState {
    pub(crate) fn disk_requirement(&self) -> Option<crate::vhd::DiskSnapshotRequirement> {
        self.vhd.as_ref().map(VhdState::disk_requirement)
    }
}

impl Disk {
    pub(crate) fn snapshot_state(&mut self, mode: DiskCaptureMode, limit: u64) -> Result<(DiskState, Option<Vec<u8>>)> {
        let (vhd, payload) = match self.vhd.as_mut() {
            Some(vhd) => {
                let (saved, payload) = vhd.snapshot_state(mode, limit)?;
                (Some(saved), payload)
            }
            None => (None, None),
        };
        Ok((
            DiskState {
                version: 1,
                position: self.position.get(),
                geometry: self.geometry.get(),
                vhd,
            },
            payload,
        ))
    }

    /// Prepare all owned state using a fresh matching dependency. No live Disk
    /// is mutated; the outer loader still owns access policy and atomic swap.
    pub(crate) fn prepare_restore(saved: &DiskState, provider: Option<Box<dyn VhdIO>>) -> Result<Self> {
        if saved.version != 1 {
            bail!("incompatible Disk snapshot version");
        }
        let vhd = match (&saved.vhd, provider) {
            (Some(saved), Some(provider)) => Some(VirtualHardDisk::prepare_restore(saved, provider)?),
            (None, None) => None,
            _ => bail!("Disk snapshot/provider presence mismatch"),
        };
        let (c, h, s, s_off, size) = saved.geometry;
        Ok(Self {
            position: DiskChs::from(saved.position),
            geometry: DriveGeometry::new(c, h, s, s_off, size),
            vhd,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Cursor,
        sync::atomic::{AtomicUsize, Ordering},
    };

    fn native_vhd_bytes() -> Vec<u8> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target"));
        std::fs::create_dir_all(&target).unwrap();
        let path = target.join(format!(
            "disk-fixture-{}-{}.vhd",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        drop(crate::vhd::create_vhd(path.clone().into_os_string(), 2, 2, 4).unwrap());
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        bytes
    }

    fn vhd(bytes: Vec<u8>) -> VirtualHardDisk {
        VirtualHardDisk::parse(Box::new(Cursor::new(bytes)), false).unwrap()
    }

    fn capture(disk: &mut Disk) -> (DiskState, Option<Vec<u8>>) {
        disk.snapshot_state(DiskCaptureMode::Embed, 0).unwrap()
    }

    fn restore(saved: &DiskState, payload: Option<Vec<u8>>) -> Disk {
        let provider = payload.map(|bytes| Box::new(Cursor::new(bytes)) as Box<dyn VhdIO>);
        Disk::prepare_restore(saved, provider).unwrap()
    }

    fn observations(reference: &mut Disk, restored: &mut Disk) {
        assert_eq!(
            reference.vhd().is_some(),
            restored.vhd().is_some(),
            "native mounted VHD presence"
        );
        assert_eq!(reference.position(), restored.position(), "native CHS");
        assert_eq!(
            reference.position_vhd(),
            restored.position_vhd(),
            "native zero-indexed CHS"
        );
        assert_eq!(reference.geometry(), restored.geometry(), "native disk geometry cache");
        assert_eq!(reference.next_sector(), restored.next_sector(), "native next sector");
        if reference.vhd().is_some() {
            let position = reference.position_vhd();
            let (c, h, s) = position.get();
            let mut a = [0; 512];
            let mut b = [0; 512];
            let read_a = reference.vhd_mut().unwrap().read_sector(&mut a, c, h, s).is_ok();
            let read_b = restored.vhd_mut().unwrap().read_sector(&mut b, c, h, s).is_ok();
            assert_eq!(read_a, read_b, "native disk read outcome");
            assert_eq!(a, b, "native disk sector bytes");
            a[23] ^= 0xA7;
            assert_eq!(
                reference.vhd_mut().unwrap().write_sector(&a, c, h, s).is_ok(),
                restored.vhd_mut().unwrap().write_sector(&a, c, h, s).is_ok(),
                "native disk write outcome"
            );
        }
        assert_eq!(capture(reference), capture(restored));
    }

    #[test]
    fn native_disk_json_restore_continues_seek_and_different_geometry_cache() {
        let bytes = native_vhd_bytes();
        let mut reference = Disk::from_vhd(vhd(bytes));
        for alternate in [false, true] {
            // Native setter can change the cache without changing VHD geometry.
            reference.set_geometry(DriveGeometry::new(
                2,
                2,
                if alternate { 3 } else { 4 },
                1,
                if alternate { 1024 } else { 512 },
            ));
            for c in 0..2 {
                for h in 0..2 {
                    for s in 1..=3 {
                        reference.seek(DiskChs::new(c, h, s));
                        let (saved, payload) = capture(&mut reference);
                        let encoded = serde_json::to_vec(&saved).unwrap();
                        let saved = serde_json::from_slice(&encoded).unwrap();
                        let mut restored = restore(&saved, payload);
                        observations(&mut reference, &mut restored);
                        reference.seek(DiskChs::new(9, 8, 7)); // native refused seek
                        restored.seek(DiskChs::new(9, 8, 7));
                        observations(&mut reference, &mut restored);
                    }
                }
            }
        }
        println!("Disk:24 native JSON continuations include changed geometry cache, sector I/O and refused seeks");
    }

    #[test]
    fn native_unload_and_rebind_preserve_stale_chs() {
        let bytes = native_vhd_bytes();
        let mut reference = Disk::from_vhd(vhd(bytes.clone()));
        reference.seek(DiskChs::new(1, 1, 3));
        reference.unload_vhd();
        assert_eq!(reference.position().get(), (1, 1, 3));
        assert_eq!(reference.geometry(), DriveGeometry::default());
        let (saved, none) = capture(&mut reference);
        let saved: DiskState = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        let mut restored = restore(&saved, none);
        assert_eq!(capture(&mut reference), capture(&mut restored));
        reference.set_vhd(vhd(bytes.clone()));
        restored.set_vhd(vhd(bytes));
        observations(&mut reference, &mut restored);
        println!("Disk: native unload/rebind JSON continuation keeps stale CHS instead of normalizing");
    }

    #[test]
    fn dependencies_and_strict_schema_refuse_without_live_mutation() {
        let bytes = native_vhd_bytes();
        let mut live = Disk::from_vhd(vhd(bytes));
        let before = capture(&mut live);
        assert!(Disk::prepare_restore(&before.0, None).is_err());
        let mut wrong = before.1.clone().unwrap();
        wrong[13] ^= 1;
        assert!(Disk::prepare_restore(&before.0, Some(Box::new(Cursor::new(wrong)))).is_err());
        let mut empty = Disk::new(DriveGeometry::new(3, 2, 4, 1, 512));
        let (saved_empty, none) = capture(&mut empty);
        assert!(none.is_none());
        assert!(Disk::prepare_restore(&saved_empty, Some(Box::new(Cursor::new(before.1.clone().unwrap())))).is_err());
        assert_eq!(capture(&mut live), before);
        for saved in [before.0, saved_empty] {
            let encoded = serde_json::to_value(&saved).unwrap();
            for key in ["version", "position", "geometry", "vhd"] {
                let mut bad = encoded.clone();
                bad.as_object_mut().unwrap().remove(key);
                assert!(serde_json::from_value::<DiskState>(bad).is_err(), "required {key}");
            }
            let mut bad = encoded.clone();
            bad["unexpected"] = true.into();
            assert!(serde_json::from_value::<DiskState>(bad).is_err());
            let mut bad = saved;
            bad.version = 0;
            assert!(Disk::prepare_restore(&bad, None).is_err());
        }
    }

    fn fields(source: &str, name: &str) -> std::collections::BTreeSet<String> {
        source
            .split(&format!("pub struct {name} {{"))
            .nth(1)
            .unwrap()
            .split('}')
            .next()
            .unwrap()
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                let line = line.strip_prefix("pub(crate) ").unwrap_or(line);
                if line.starts_with("//") {
                    return None;
                }
                line.split_once(':').map(|(name, _)| name.trim().to_owned())
            })
            .collect()
    }

    #[test]
    fn native_stale_below_base_position_is_preserved_without_normalization() {
        for replace_geometry in [false, true] {
            let geometry = DriveGeometry::new(2, 2, 4, 17, 512);
            let mut reference = if replace_geometry {
                let mut disk = Disk::new(DriveGeometry::new(2, 2, 4, 1, 512));
                disk.set_geometry(geometry);
                disk
            } else {
                Disk::new(geometry)
            };
            // Native constructor/geometry replacement leaves sector1 cached.
            // Its saturating VHD view is not a valid-address guarantee. This
            // is a storage-only observation, not a successful sector transfer.
            assert_eq!(reference.position(), DiskChs::new(0, 0, 1));
            assert!(!reference.geometry().contains(reference.position()));
            assert_eq!(reference.position_vhd(), DiskChs::new(0, 0, 0));
            let (saved, payload) = capture(&mut reference);
            let json = serde_json::to_string(&saved).unwrap();
            let mut restored = restore(&serde_json::from_str(&json).unwrap(), payload);
            assert_eq!(restored.position(), reference.position());
            assert_eq!(restored.geometry(), reference.geometry());
            assert_eq!(restored.position_vhd(), reference.position_vhd());
            assert_eq!(restored.next_sector(), None);
            assert_eq!(capture(&mut restored), capture(&mut reference));
        }
    }

    #[test]
    fn schema_inventory_covers_native_disk_chs_and_geometry_fields() {
        let mut disk = Disk::new(DriveGeometry::new(2, 2, 4, 0, 512));
        let saved = serde_json::to_value(capture(&mut disk).0).unwrap();
        let mut native = fields(include_str!("../disk.rs"), "Disk");
        native.insert("version".to_owned());
        assert_eq!(
            saved
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            native
        );
        assert_eq!(
            fields(include_str!("../chs.rs"), "DiskChs"),
            ["c", "h", "s"].map(String::from).into_iter().collect()
        );
        assert_eq!(
            fields(include_str!("../geometry.rs"), "DriveGeometry"),
            ["c", "h", "s", "s_off", "size"].map(String::from).into_iter().collect()
        );
    }
}
