use super::*;
use crate::device_types::disk::Disk;

#[test]
fn sector_id_bounds_follow_declared_offset_without_byte_overflow() {
    let mut count = 0;
    for offset in [0, 1, 2, 17, 254, 255] {
        for sectors in [0, 1, 2, 4, 127, 255] {
            let geometry = DriveGeometry::new(2, 2, sectors, offset, 512);
            for cylinder in 0..=2 {
                for head in 0..=2 {
                    for sector in 0..=255 {
                        // Independently derive the half-open interval using
                        // wider integers, so offset + count cannot wrap.
                        let expected = cylinder < 2
                            && head < 2
                            && (sector as u16) >= offset as u16
                            && (sector as u16) < offset as u16 + sectors as u16;
                        assert_eq!(
                            geometry.contains(DiskChs::new(cylinder, head, sector)),
                            expected,
                            "c={cylinder} h={head} sector={sector} offset={offset} count={sectors}"
                        );
                        count += 1;
                    }
                }
            }
        }
    }
    assert_eq!(count, 82944);
    println!("Geometry:82944 native contains checks against independent wide-integer bounds");
}

#[test]
fn native_disk_seek_refuses_sector_below_its_declared_base() {
    let mut disk = Disk::new(DriveGeometry::new(2, 2, 4, 1, 512));
    disk.seek(DiskChs::new(1, 1, 4));
    let previous = disk.position();
    disk.seek(DiskChs::new(1, 1, 0));
    assert_eq!(
        disk.position(),
        previous,
        "sector0 must not replace valid one-based CHS"
    );
    disk.seek(DiskChs::new(1, 1, 5));
    assert_eq!(disk.position(), previous, "upper sector bound must also be refused");
}

#[test]
fn native_vhd_position_uses_the_actual_geometry_sector_offset() {
    for offset in [0u8, 1, 17, 255] {
        let mut disk = Disk::new(DriveGeometry::new(2, 2, 4, offset, 512));
        for sector in offset..=offset.saturating_add(3) {
            disk.seek(DiskChs::new(1, 1, sector));
            assert_eq!(disk.position().get(), (1, 1, sector));
            assert_eq!(disk.position_vhd().get(), (1, 1, sector - offset), "offset={offset}");
        }
    }
}

#[test]
fn native_next_sector_does_not_overflow_offset_plus_count() {
    let geometry = DriveGeometry::new(2, 2, 255, 17, 512);
    assert!(geometry.contains(DiskChs::new(0, 0, 17)));
    assert_eq!(
        DiskChs::new(0, 0, 17).next_sector(&geometry),
        Some(DiskChs::new(0, 0, 18))
    );
    // Sector 256 is declared by this geometry but cannot fit in DiskChs.
    // Return None instead of wrapping the address or skipping to another head.
    assert_eq!(DiskChs::new(0, 0, 255).next_sector(&geometry), None);
}

#[test]
fn native_next_sector_matches_independent_wide_lba_successor() {
    let mut count = 0;
    for offset in [0u8, 1, 2, 17, 254, 255] {
        for sectors in [0u8, 1, 2, 4, 127, 255] {
            let geometry = DriveGeometry::new(2, 2, sectors, offset, 512);
            for cylinder in 0..=2 {
                for head in 0..=2 {
                    for sector in 0..=255 {
                        let in_range = cylinder < 2
                            && head < 2
                            && (sector as u16) >= offset as u16
                            && (sector as u16) < offset as u16 + sectors as u16;
                        let expected = if in_range {
                            let next = (cylinder as u32 * 2 + head as u32) * sectors as u32 + sector as u32
                                - offset as u32
                                + 1;
                            let track = next / sectors as u32;
                            let next_sector = next % sectors as u32 + offset as u32;
                            if next < 4 * sectors as u32 && next_sector <= 255 {
                                Some(DiskChs::new((track / 2) as u16, (track % 2) as u8, next_sector as u8))
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        assert_eq!(
                            DiskChs::new(cylinder, head, sector).next_sector(&geometry),
                            expected,
                            "c={cylinder} h={head} s={sector} offset={offset} count={sectors}"
                        );
                        count += 1;
                    }
                }
            }
        }
    }
    assert_eq!(count, 82944);
    println!("CHS:82944 native successors against independent wide LBA calculation");
}
