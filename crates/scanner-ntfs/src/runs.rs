//! Data-run decoding for non-resident attributes.
//!
//! Only used to locate the $MFT itself (record 0's $DATA runs) — file sizes
//! come straight from attribute headers, never from runs.

use crate::ParseError;

/// One contiguous stretch of an attribute's data on disk, in clusters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extent {
    pub lcn: u64,
    pub clusters: u64,
}

/// Decodes a run list into absolute extents.
///
/// Each run is `[header][length bytes][offset bytes]` where the header's low
/// nibble is the length field width and the high nibble the offset field
/// width; offsets are signed deltas from the previous run's LCN; a zero
/// header terminates. Sparse runs (offset width 0) are rejected — the $MFT
/// is never sparse.
pub fn decode_runs(data: &[u8]) -> Result<Vec<Extent>, ParseError> {
    let mut extents = Vec::new();
    let mut pos = 0usize;
    let mut lcn: i64 = 0;

    while pos < data.len() {
        let header = data[pos];
        if header == 0 {
            return Ok(extents);
        }
        pos += 1;
        let len_width = (header & 0x0F) as usize;
        let off_width = (header >> 4) as usize;
        if len_width == 0 || len_width > 8 || off_width > 8 {
            return Err(ParseError("run header has implausible field widths"));
        }
        if off_width == 0 {
            return Err(ParseError("sparse run in a run list that must be dense"));
        }
        let end = pos + len_width + off_width;
        if end > data.len() {
            return Err(ParseError("run list truncated"));
        }

        let clusters = uint_le(&data[pos..pos + len_width]);
        let delta = int_le(&data[pos + len_width..end]);
        pos = end;

        lcn = lcn
            .checked_add(delta)
            .ok_or(ParseError("run LCN overflow"))?;
        if lcn < 0 {
            return Err(ParseError("run points before the volume start"));
        }
        if clusters == 0 {
            return Err(ParseError("zero-length run"));
        }
        extents.push(Extent {
            lcn: lcn as u64,
            clusters,
        });
    }
    Err(ParseError("run list missing terminator"))
}

/// Little-endian unsigned int of 1..=8 bytes.
fn uint_le(bytes: &[u8]) -> u64 {
    let mut v = 0u64;
    for (i, &b) in bytes.iter().enumerate() {
        v |= (b as u64) << (8 * i);
    }
    v
}

/// Little-endian signed int of 1..=8 bytes (sign-extended).
fn int_le(bytes: &[u8]) -> i64 {
    let mut v = uint_le(bytes);
    let bits = 8 * bytes.len();
    if bits < 64 && v & (1 << (bits - 1)) != 0 {
        v |= u64::MAX << bits; // sign-extend
    }
    v as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_single_run() {
        // The canonical documented example: header 0x21 = 1-byte length,
        // 2-byte offset → 0x18 clusters at LCN 0x5634.
        let runs = decode_runs(&[0x21, 0x18, 0x34, 0x56, 0x00]).unwrap();
        assert_eq!(
            runs,
            vec![Extent {
                lcn: 0x5634,
                clusters: 0x18
            }]
        );
    }

    #[test]
    fn decodes_multiple_runs_with_negative_delta() {
        // Run 1: 0x30 clusters at LCN 0x120. Run 2: 0x10 clusters at delta -1.
        let data = [0x21, 0x30, 0x20, 0x01, 0x11, 0x10, 0xFF, 0x00];
        let runs = decode_runs(&data).unwrap();
        assert_eq!(
            runs,
            vec![
                Extent {
                    lcn: 0x120,
                    clusters: 0x30
                },
                Extent {
                    lcn: 0x11F,
                    clusters: 0x10
                },
            ]
        );
    }

    #[test]
    fn decodes_wide_fields() {
        // 4-byte length, 3-byte offset.
        let mut data = vec![0x34];
        data.extend_from_slice(&0x0012_3456u32.to_le_bytes());
        data.extend_from_slice(&0x654321u32.to_le_bytes()[..3]);
        data.push(0x00);
        let runs = decode_runs(&data).unwrap();
        assert_eq!(
            runs,
            vec![Extent {
                lcn: 0x654321,
                clusters: 0x0012_3456
            }]
        );
    }

    #[test]
    fn rejects_sparse_run() {
        // Header 0x01: 1-byte length, no offset field = sparse.
        assert_eq!(
            decode_runs(&[0x01, 0x10, 0x00]),
            Err(ParseError("sparse run in a run list that must be dense"))
        );
    }

    #[test]
    fn rejects_truncated_and_unterminated_input() {
        assert!(decode_runs(&[0x21, 0x56]).is_err()); // fields cut off
        assert!(decode_runs(&[0x11, 0x10, 0x34]).is_err()); // no terminator
    }

    #[test]
    fn rejects_run_before_volume_start() {
        // First delta is negative: LCN would be -2.
        assert!(decode_runs(&[0x11, 0x10, 0xFE, 0x00]).is_err());
    }

    #[test]
    fn empty_input_is_an_error() {
        assert!(decode_runs(&[]).is_err());
    }
}
