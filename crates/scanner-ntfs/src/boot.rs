//! NTFS boot sector → volume geometry and the $MFT's first cluster, plus
//! the read-plan validation that makes those disk-supplied sizes safe to
//! multiply and allocate with.

use crate::ParseError;
use crate::runs::Extent;

/// Everything the reader needs to interpret the volume, straight from the
/// boot sector. All sizes in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub bytes_per_sector: u32,
    pub cluster_size: u32,
    /// FILE record size (1 KiB on almost every volume; 4 KiB on 4Kn disks).
    pub record_size: u32,
    /// First cluster of the $MFT (where FILE record 0 lives).
    pub mft_lcn: u64,
    pub total_clusters: u64,
}

impl Geometry {
    /// Callers must have run [`geometry_fits_device`] first — it proves
    /// every in-volume cluster offset (this one included) fits u64.
    pub fn mft_byte_offset(&self) -> u64 {
        self.mft_lcn * self.cluster_size as u64
    }
}

/// The validated $MFT read plan. Produced only by [`plan_mft_read`] — the
/// trust boundary for every disk-supplied size downstream code multiplies,
/// reads, or allocates with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MftPlan {
    /// Bytes of FILE records to read ($DATA real size, extent-capped).
    pub mft_bytes: u64,
    /// `mft_bytes` in whole records; fits NTFS's 32-bit record-number space.
    pub total_records: u32,
}

/// Rejects geometry describing more volume than the device physically has.
/// The boot sector's totals are disk bytes, not facts — until this passes,
/// nothing may be sized, offset, or allocated from them.
pub fn geometry_fits_device(geometry: &Geometry, device_bytes: u64) -> Result<(), ParseError> {
    match geometry
        .total_clusters
        .checked_mul(geometry.cluster_size as u64)
    {
        Some(bytes) if bytes <= device_bytes => Ok(()),
        _ => Err(ParseError("boot sector claims more space than the device")),
    }
}

/// Validates the $MFT's self-description (record 0 or FSCTL) against the
/// volume's own geometry and the device's real size. Afterwards every
/// extent read offset and the slot-table size are bounded by `device_bytes`
/// and overflow-free. `data_size` of `u64::MAX` means "unknown, read all
/// covered clusters" (the FSCTL fallback has no real size).
pub fn plan_mft_read(
    geometry: &Geometry,
    extents: &[Extent],
    data_size: u64,
    device_bytes: u64,
) -> Result<MftPlan, ParseError> {
    geometry_fits_device(geometry, device_bytes)?;

    let mut covered = 0u64;
    for e in extents {
        match e.lcn.checked_add(e.clusters) {
            Some(end) if end <= geometry.total_clusters => {}
            _ => return Err(ParseError("$MFT extent beyond the volume end")),
        }
        covered = match covered.checked_add(e.clusters) {
            Some(c) if c <= geometry.total_clusters => c,
            _ => return Err(ParseError("$MFT extents cover more than the volume")),
        };
    }

    // covered ≤ total_clusters and total_clusters × cluster fits (checked
    // above), so this multiplication cannot overflow.
    let mft_bytes = data_size.min(covered * geometry.cluster_size as u64);
    let total_records = mft_bytes / geometry.record_size as u64;
    if total_records == 0 {
        return Err(ParseError("$MFT extent map is empty"));
    }
    match u32::try_from(total_records) {
        Ok(total_records) => Ok(MftPlan {
            mft_bytes,
            total_records,
        }),
        Err(_) => Err(ParseError("implausible $MFT record count")),
    }
}

/// Parses the first sector of an NTFS volume. Accepts at least 512 bytes.
pub fn parse_boot_sector(sector: &[u8]) -> Result<Geometry, ParseError> {
    if sector.len() < 512 {
        return Err(ParseError("boot sector shorter than 512 bytes"));
    }
    if &sector[3..11] != b"NTFS    " {
        return Err(ParseError("not an NTFS volume (OEM id)"));
    }
    if sector[510] != 0x55 || sector[511] != 0xAA {
        return Err(ParseError("boot sector signature missing"));
    }

    let bytes_per_sector = u16::from_le_bytes([sector[11], sector[12]]) as u32;
    if !(512..=4096).contains(&bytes_per_sector) || !bytes_per_sector.is_power_of_two() {
        return Err(ParseError("implausible bytes per sector"));
    }

    // Sectors per cluster: plain count, or (for clusters > 64 KiB) a negative
    // power-of-two exponent. Same signed encoding as clusters-per-record.
    let sectors_per_cluster = match sector[13] as i8 {
        n if n > 0 => n as u32,
        n if (-25..0).contains(&n) => 1u32 << (-n as u32),
        _ => return Err(ParseError("implausible sectors per cluster")),
    };
    let cluster_size = bytes_per_sector
        .checked_mul(sectors_per_cluster)
        .ok_or(ParseError("cluster size overflow"))?;
    if cluster_size > 2 * 1024 * 1024 || !cluster_size.is_power_of_two() {
        return Err(ParseError("implausible cluster size"));
    }

    let total_sectors = u64_at(sector, 0x28);
    let mft_lcn = u64_at(sector, 0x30);
    let total_clusters = total_sectors / sectors_per_cluster as u64;
    if mft_lcn == 0 || mft_lcn >= total_clusters {
        return Err(ParseError("$MFT cluster out of range"));
    }

    // Clusters per FILE record: positive = cluster count, negative = the
    // record is 2^|x| bytes (the common case: 0xF6 → 1024).
    let record_size = match sector[0x40] as i8 {
        n if n > 0 => (n as u32)
            .checked_mul(cluster_size)
            .ok_or(ParseError("record size overflow"))?,
        n if (-31..0).contains(&n) => 1u32 << (-n as u32),
        _ => return Err(ParseError("implausible record size encoding")),
    };
    if !(512..=65536).contains(&record_size) || !record_size.is_power_of_two() {
        return Err(ParseError("implausible FILE record size"));
    }

    Ok(Geometry {
        bytes_per_sector,
        cluster_size,
        record_size,
        mft_lcn,
        total_clusters,
    })
}

fn u64_at(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(b[off..off + 8].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A boot sector for a volume shaped like a typical desktop C: drive.
    fn typical_boot() -> [u8; 512] {
        let mut b = [0u8; 512];
        b[3..11].copy_from_slice(b"NTFS    ");
        b[11..13].copy_from_slice(&512u16.to_le_bytes()); // bytes/sector
        b[13] = 8; // sectors/cluster → 4 KiB clusters
        b[0x28..0x30].copy_from_slice(&500_000_000u64.to_le_bytes()); // total sectors
        b[0x30..0x38].copy_from_slice(&786_432u64.to_le_bytes()); // $MFT LCN
        b[0x40] = 0xF6; // -10 → 2^10 = 1 KiB records
        b[510] = 0x55;
        b[511] = 0xAA;
        b
    }

    #[test]
    fn parses_typical_volume() {
        let g = parse_boot_sector(&typical_boot()).unwrap();
        assert_eq!(g.bytes_per_sector, 512);
        assert_eq!(g.cluster_size, 4096);
        assert_eq!(g.record_size, 1024);
        assert_eq!(g.mft_lcn, 786_432);
        assert_eq!(g.mft_byte_offset(), 786_432 * 4096);
    }

    #[test]
    fn parses_4kn_volume() {
        let mut b = typical_boot();
        b[11..13].copy_from_slice(&4096u16.to_le_bytes());
        b[13] = 1; // 4 KiB clusters
        b[0x40] = 0xF4; // -12 → 4 KiB records
        let g = parse_boot_sector(&b).unwrap();
        assert_eq!(g.cluster_size, 4096);
        assert_eq!(g.record_size, 4096);
    }

    #[test]
    fn parses_large_cluster_exponent_encoding() {
        let mut b = typical_boot();
        b[13] = 0xF9; // -7 → 2^7 = 128 sectors → 64 KiB clusters
        let g = parse_boot_sector(&b).unwrap();
        assert_eq!(g.cluster_size, 64 * 1024);
    }

    #[test]
    fn record_size_as_positive_cluster_count() {
        let mut b = typical_boot();
        b[0x40] = 1; // one 4 KiB cluster per record
        let g = parse_boot_sector(&b).unwrap();
        assert_eq!(g.record_size, 4096);
    }

    #[test]
    fn rejects_non_ntfs_oem_id() {
        let mut b = typical_boot();
        b[3..11].copy_from_slice(b"MSDOS5.0");
        assert_eq!(
            parse_boot_sector(&b),
            Err(ParseError("not an NTFS volume (OEM id)"))
        );
    }

    #[test]
    fn rejects_missing_signature() {
        let mut b = typical_boot();
        b[511] = 0;
        assert!(parse_boot_sector(&b).is_err());
    }

    #[test]
    fn rejects_short_input_and_zero_mft_lcn() {
        assert!(parse_boot_sector(&[0u8; 100]).is_err());
        let mut b = typical_boot();
        b[0x30..0x38].copy_from_slice(&0u64.to_le_bytes());
        assert_eq!(
            parse_boot_sector(&b),
            Err(ParseError("$MFT cluster out of range"))
        );
    }

    #[test]
    fn rejects_non_power_of_two_geometry() {
        let mut b = typical_boot();
        b[13] = 3; // 1536-byte clusters
        assert!(parse_boot_sector(&b).is_err());
    }

    /// The volume `typical_boot` describes: 62.5M clusters × 4 KiB.
    fn typical_geometry() -> (Geometry, u64) {
        let g = parse_boot_sector(&typical_boot()).unwrap();
        let device_bytes = g.total_clusters * g.cluster_size as u64;
        (g, device_bytes)
    }

    #[test]
    fn plan_accepts_a_typical_mft() {
        let (g, dev) = typical_geometry();
        // One 2.5 GiB extent; $DATA real size a bit under the allocation.
        let extents = [Extent {
            lcn: 786_432,
            clusters: 655_360,
        }];
        let plan = plan_mft_read(&g, &extents, 2_600_000_000, dev).unwrap();
        assert_eq!(plan.mft_bytes, 2_600_000_000);
        assert_eq!(plan.total_records, 2_539_062); // 2.6e9 / 1024, floored
    }

    #[test]
    fn plan_with_unknown_data_size_reads_all_covered_clusters() {
        let (g, dev) = typical_geometry();
        let extents = [Extent {
            lcn: 786_432,
            clusters: 1_000,
        }];
        let plan = plan_mft_read(&g, &extents, u64::MAX, dev).unwrap();
        assert_eq!(plan.mft_bytes, 1_000 * 4096);
        assert_eq!(plan.total_records, 4_000);
    }

    #[test]
    fn plan_rejects_boot_sector_bigger_than_device() {
        let (g, dev) = typical_geometry();
        let extents = [Extent {
            lcn: 786_432,
            clusters: 1_000,
        }];
        assert_eq!(
            plan_mft_read(&g, &extents, u64::MAX, dev - 1),
            Err(ParseError("boot sector claims more space than the device"))
        );
    }

    #[test]
    fn plan_rejects_extent_beyond_the_volume() {
        let (g, dev) = typical_geometry();
        let extents = [Extent {
            lcn: g.total_clusters - 10,
            clusters: 100,
        }];
        assert_eq!(
            plan_mft_read(&g, &extents, u64::MAX, dev),
            Err(ParseError("$MFT extent beyond the volume end"))
        );
    }

    #[test]
    fn plan_rejects_overlapping_extents_covering_more_than_the_volume() {
        let (g, dev) = typical_geometry();
        // Each extent is individually in-bounds; together they claim more
        // clusters than the volume has.
        let half = Extent {
            lcn: 0,
            clusters: g.total_clusters / 2 + 1,
        };
        assert_eq!(
            plan_mft_read(&g, &[half, half], u64::MAX, dev),
            Err(ParseError("$MFT extents cover more than the volume"))
        );
    }

    #[test]
    fn plan_rejects_an_empty_extent_map() {
        let (g, dev) = typical_geometry();
        assert_eq!(
            plan_mft_read(&g, &[], u64::MAX, dev),
            Err(ParseError("$MFT extent map is empty"))
        );
    }

    #[test]
    fn plan_rejects_record_count_beyond_u32() {
        // A (real, huge) device whose forged record 0 claims a 4 TiB $MFT:
        // 2^32 × 1 KiB records is one past the u32 record-number space.
        let g = Geometry {
            bytes_per_sector: 512,
            cluster_size: 4096,
            record_size: 1024,
            mft_lcn: 16,
            total_clusters: (1 << 30) + 16,
        };
        let extents = [Extent {
            lcn: 16,
            clusters: 1 << 30, // × 4 KiB = 2^42 bytes = 2^32 records
        }];
        assert_eq!(
            plan_mft_read(&g, &extents, u64::MAX, u64::MAX / 2),
            Err(ParseError("implausible $MFT record count"))
        );
    }
}
