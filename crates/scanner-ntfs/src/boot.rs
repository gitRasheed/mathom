//! NTFS boot sector → volume geometry and the $MFT's first cluster.

use crate::ParseError;

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
    pub fn mft_byte_offset(&self) -> u64 {
        self.mft_lcn * self.cluster_size as u64
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
}
