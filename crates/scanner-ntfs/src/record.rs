//! FILE-record parsing: fixup application and the attribute walk. This is
//! the scan's hot loop — zero heap allocation per record (names append to a
//! caller-owned arena) and every offset/length read from disk bytes is
//! bounds-checked before use.

use crate::ParseError;
use mathom_core::EntryFlags;

/// The volume root directory is always FILE record 5.
pub const ROOT_RECORD: u64 = 5;

/// A packed MFT reference: 48-bit record number + 16-bit sequence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecordRef(pub u64);

impl RecordRef {
    pub fn number(self) -> u64 {
        self.0 & 0x0000_FFFF_FFFF_FFFF
    }

    pub fn sequence(self) -> u16 {
        (self.0 >> 48) as u16
    }
}

/// One chosen file name: where it lives in the name arena and which parent
/// it links to. `rank` orders namespaces (Win32 best, DOS 8.3 last) so a
/// better name replaces a worse one during the walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NameFact {
    pub parent: RecordRef,
    pub rank: u8,
    pub off: u32,
    pub len: u16,
}

/// Everything one FILE record contributes to the tree. For extension
/// records (`base != 0`) the caller routes these facts to the base record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecordFacts {
    /// Base record number; 0 means this *is* a base record.
    pub base: u64,
    pub seq: u16,
    pub is_dir: bool,
    pub mtime: i64,
    /// Unnamed $DATA real size.
    pub logical: u64,
    pub has_logical: bool,
    /// Σ allocated over all streams (unnamed + ADS); resident data counts 0.
    pub alloc: u64,
    pub reparse_tag: u32,
    /// Non-DOS $FILE_NAME count — >1 on a base record means hardlinks.
    pub link_names: u32,
    pub name: Option<NameFact>,
    pub flags: EntryFlags,
}

const ATTR_STANDARD_INFORMATION: u32 = 0x10;
const ATTR_FILE_NAME: u32 = 0x30;
const ATTR_DATA: u32 = 0x80;
const ATTR_REPARSE_POINT: u32 = 0xC0;
const ATTR_END: u32 = 0xFFFF_FFFF;

const NS_DOS: u8 = 2;

/// Parses one FILE record in place (fixups are applied to the buffer).
///
/// `Ok(None)`: not a record or not in use (free/beyond-initialized space) —
/// skipped silently. `Err`: a torn or corrupt record — the caller counts it.
pub fn parse_record(rec: &mut [u8], arena: &mut String) -> Result<Option<RecordFacts>, ParseError> {
    debug_assert!(rec.len() >= 512 && rec.len().is_power_of_two());
    if &rec[0..4] != b"FILE" {
        return Ok(None);
    }
    let header_flags = u16_at(rec, 0x16);
    if header_flags & 0x1 == 0 {
        return Ok(None); // deleted / never used
    }
    apply_fixups(rec)?;

    let seq = u16_at(rec, 0x10);
    let first_attr = u16_at(rec, 0x14) as usize;
    let used = u32_at(rec, 0x18) as usize;
    let base_ref = RecordRef(u64_at(rec, 0x20));
    let limit = used.min(rec.len());
    // The attribute area may legitimately be just the 4-byte end marker.
    if first_attr < 0x30 || first_attr + 4 > limit {
        return Err(ParseError("attribute offset out of bounds"));
    }

    let mut facts = RecordFacts {
        base: base_ref.number(),
        seq,
        is_dir: header_flags & 0x2 != 0,
        ..RecordFacts::default()
    };

    let mut pos = first_attr;
    loop {
        if pos + 4 > limit {
            return Err(ParseError("attribute walk past record end"));
        }
        if u32_at(rec, pos) == ATTR_END {
            break;
        }
        if pos + 8 > limit {
            return Err(ParseError("attribute header truncated"));
        }
        let alen = u32_at(rec, pos + 4) as usize;
        if alen < 24 || !alen.is_multiple_of(8) || pos + alen > limit {
            return Err(ParseError("attribute length out of bounds"));
        }
        let attr = &rec[pos..pos + alen];
        match u32_at(attr, 0) {
            ATTR_STANDARD_INFORMATION => std_info(attr, &mut facts)?,
            ATTR_FILE_NAME => file_name(attr, arena, &mut facts)?,
            ATTR_DATA => data(attr, &mut facts)?,
            ATTR_REPARSE_POINT => reparse_point(attr, &mut facts),
            _ => {} // $ATTRIBUTE_LIST included: a full sweep sees extension
                    // records anyway, so x20 is skipped by design (plan.md)
        }
        pos += alen;
    }
    Ok(Some(facts))
}

/// The $MFT's self-description, from FILE record 0: how many bytes of
/// records exist and where they live on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MftSelf {
    /// $DATA real size — the read limit (records beyond it don't exist).
    pub data_size: u64,
    pub extents: Vec<crate::runs::Extent>,
}

/// Parses FILE record 0 for the $MFT's own unnamed $DATA runs. Errors if
/// the runs in record 0 don't cover the whole attribute (a heavily
/// fragmented $MFT spills runs into extension records — the caller falls
/// back to FSCTL retrieval pointers rather than parsing $ATTRIBUTE_LIST).
pub fn parse_record0(rec: &mut [u8]) -> Result<MftSelf, ParseError> {
    if &rec[0..4] != b"FILE" || u16_at(rec, 0x16) & 0x1 == 0 {
        return Err(ParseError("record 0 is not a live FILE record"));
    }
    apply_fixups(rec)?;

    let first_attr = u16_at(rec, 0x14) as usize;
    let limit = (u32_at(rec, 0x18) as usize).min(rec.len());
    if first_attr < 0x30 || first_attr + 4 > limit {
        return Err(ParseError("record 0 attribute offset out of bounds"));
    }
    let mut pos = first_attr;
    loop {
        if pos + 4 > limit {
            return Err(ParseError("record 0 walk past record end"));
        }
        if u32_at(rec, pos) == ATTR_END {
            return Err(ParseError("record 0 has no non-resident $DATA"));
        }
        if pos + 8 > limit {
            return Err(ParseError("record 0 attribute truncated"));
        }
        let alen = u32_at(rec, pos + 4) as usize;
        if alen < 24 || !alen.is_multiple_of(8) || pos + alen > limit {
            return Err(ParseError("record 0 attribute length out of bounds"));
        }
        let attr = &rec[pos..pos + alen];
        pos += alen;
        if u32_at(attr, 0) != ATTR_DATA || attr[8] == 0 || attr[9] != 0 {
            continue; // not the unnamed non-resident $DATA
        }
        if attr.len() < 64 || u64_at(attr, 16) != 0 {
            return Err(ParseError("record 0 $DATA shape unexpected"));
        }
        let highest_vcn = u64_at(attr, 24);
        let data_size = u64_at(attr, 48);
        let run_off = u16_at(attr, 32) as usize;
        if run_off < 64 || run_off >= attr.len() {
            return Err(ParseError("record 0 run list out of bounds"));
        }
        let extents = crate::runs::decode_runs(&attr[run_off..])?;
        let covered: u64 = extents.iter().map(|e| e.clusters).sum();
        if covered != highest_vcn + 1 {
            return Err(ParseError("$MFT runs incomplete in record 0"));
        }
        return Ok(MftSelf { data_size, extents });
    }
}

/// Maps a reparse tag to entry flags: name surrogates (junctions, symlinks)
/// are marked and never descended; WOF-backed files are compressed; cloud
/// placeholders (OneDrive & co) are flagged so dehydrated sizes read right.
pub fn reparse_entry_flags(tag: u32) -> EntryFlags {
    const NAME_SURROGATE: u32 = 0x2000_0000;
    const WOF: u32 = 0x8000_0017;
    const CLOUD_FAMILY_MASK: u32 = 0xFFFF_0FFF;
    const CLOUD: u32 = 0x9000_001A;
    let mut f = EntryFlags(0);
    if tag & NAME_SURROGATE != 0 {
        f.insert(EntryFlags::REPARSE);
    }
    if tag == WOF {
        f.insert(EntryFlags::COMPRESSED);
    }
    if tag & CLOUD_FAMILY_MASK == CLOUD {
        f.insert(EntryFlags::PLACEHOLDER);
    }
    f
}

/// FILETIME (100ns ticks since 1601) → Unix seconds; 0 stays 0 ("unknown").
/// Division truncates toward zero, matching the generic walker's pre-1970
/// handling (and jiff's, which the oracle tests lean on).
pub fn filetime_to_unix(ft: u64) -> i64 {
    const EPOCH_DELTA_TICKS: i128 = 116_444_736_000_000_000;
    if ft == 0 {
        return 0;
    }
    ((ft as i128 - EPOCH_DELTA_TICKS) / 10_000_000) as i64
}

/// Verifies and undoes the update-sequence protection: the last word of
/// every sector must equal the USN, and gets its saved value back. A
/// mismatch means the record was torn mid-write.
fn apply_fixups(rec: &mut [u8]) -> Result<(), ParseError> {
    let usa_off = u16_at(rec, 4) as usize;
    let count = u16_at(rec, 6) as usize;
    if count < 2 {
        return Err(ParseError("fixup array too small"));
    }
    let sectors = count - 1;
    if !rec.len().is_multiple_of(sectors) {
        return Err(ParseError("fixup count does not divide record size"));
    }
    let stride = rec.len() / sectors;
    if stride < 512 || !stride.is_multiple_of(512) || usa_off + 2 * count > stride - 2 {
        return Err(ParseError("implausible fixup layout"));
    }

    let usn = u16_at(rec, usa_off);
    for i in 1..count {
        let end = i * stride - 2;
        if u16_at(rec, end) != usn {
            return Err(ParseError("torn record (fixup mismatch)"));
        }
        let saved = [rec[usa_off + 2 * i], rec[usa_off + 2 * i + 1]];
        rec[end..end + 2].copy_from_slice(&saved);
    }
    Ok(())
}

fn std_info(attr: &[u8], facts: &mut RecordFacts) -> Result<(), ParseError> {
    let Some(v) = resident_value(attr)? else {
        return Ok(()); // non-resident $STANDARD_INFORMATION: ignore
    };
    if v.len() < 0x24 {
        return Err(ParseError("standard-information value truncated"));
    }
    facts.mtime = filetime_to_unix(u64_at(v, 0x08));
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    if u32_at(v, 0x20) & FILE_ATTRIBUTE_SYSTEM != 0 {
        facts.flags.insert(EntryFlags::SYSTEM);
    }
    Ok(())
}

fn file_name(attr: &[u8], arena: &mut String, facts: &mut RecordFacts) -> Result<(), ParseError> {
    let Some(v) = resident_value(attr)? else {
        return Ok(());
    };
    if v.len() < 0x42 {
        return Err(ParseError("file-name value truncated"));
    }
    let chars = v[0x40] as usize;
    let namespace = v[0x41];
    if 0x42 + 2 * chars > v.len() {
        return Err(ParseError("file-name string out of bounds"));
    }
    if namespace != NS_DOS {
        facts.link_names += 1;
    }
    // NOTE: $FILE_NAME also carries sizes — they are notoriously stale and
    // never trusted; sizes come from $DATA only.
    let rank = name_rank(namespace);
    if facts.name.is_none_or(|n| rank < n.rank) {
        let off = arena.len() as u32;
        let units = v[0x42..0x42 + 2 * chars]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]));
        for r in char::decode_utf16(units) {
            arena.push(r.unwrap_or(char::REPLACEMENT_CHARACTER));
        }
        let len = (arena.len() - off as usize) as u16;
        facts.name = Some(NameFact {
            parent: RecordRef(u64_at(v, 0)),
            rank,
            off,
            len,
        });
    }
    Ok(())
}

/// Win32 (and Win32&DOS) beat POSIX beat DOS 8.3. First name of the best
/// rank wins, so hardlink placement is deterministic (disk order).
fn name_rank(namespace: u8) -> u8 {
    match namespace {
        1 | 3 => 0, // Win32, Win32&DOS
        0 => 1,     // POSIX
        NS_DOS => 2,
        _ => 3,
    }
}

fn data(attr: &[u8], facts: &mut RecordFacts) -> Result<(), ParseError> {
    let named = attr[9] != 0;
    if attr[8] == 0 {
        // Resident: the bytes live inside this MFT record, so on-disk
        // allocation outside the MFT is 0 (decided policy, plan.md).
        let v = resident_value(attr)?.expect("checked resident");
        if !named && !facts.has_logical {
            facts.logical = v.len() as u64;
            facts.has_logical = true;
        }
        return Ok(());
    }

    if attr.len() < 64 {
        return Err(ParseError("non-resident header truncated"));
    }
    if u64_at(attr, 16) != 0 {
        return Ok(()); // continuation fragment (lowest VCN > 0): sizes live
        // in the VCN-0 fragment
    }
    let compression_unit = u16_at(attr, 34);
    let alloc = if compression_unit != 0 {
        if attr.len() < 72 {
            return Err(ParseError("compressed data header truncated"));
        }
        u64_at(attr, 64) // total_allocated: the truly backed bytes
    } else {
        u64_at(attr, 40)
    };
    facts.alloc = facts.alloc.saturating_add(alloc);

    if !named && !facts.has_logical {
        facts.logical = u64_at(attr, 48);
        facts.has_logical = true;
        let attr_flags = u16_at(attr, 12);
        if attr_flags & 0x00FF != 0 {
            facts.flags.insert(EntryFlags::COMPRESSED);
        }
        if attr_flags & 0x8000 != 0 {
            facts.flags.insert(EntryFlags::SPARSE);
        }
    }
    Ok(())
}

fn reparse_point(attr: &[u8], facts: &mut RecordFacts) {
    if let Ok(Some(v)) = resident_value(attr)
        && v.len() >= 4
    {
        facts.reparse_tag = u32_at(v, 0);
    }
}

/// The value slice of a resident attribute; `Ok(None)` if non-resident.
fn resident_value(attr: &[u8]) -> Result<Option<&[u8]>, ParseError> {
    if attr[8] != 0 {
        return Ok(None);
    }
    let vlen = u32_at(attr, 16) as usize;
    let voff = u16_at(attr, 20) as usize;
    if voff < 24 || voff + vlen > attr.len() {
        return Err(ParseError("resident value out of bounds"));
    }
    Ok(Some(&attr[voff..voff + vlen]))
}

// Callers validate lengths before these reads; slice indexing is the last
// line of defense (a panic here is a parser bug, not a corrupt-input path).
fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(b[off..off + 2].try_into().unwrap())
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(b[off..off + 8].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::RecordBuilder;

    const FT_2020: u64 = 132_223_104_000_000_000; // 2020-01-01T00:00:00Z
    const UNIX_2020: i64 = 1_577_836_800;

    fn parse_one(builder: RecordBuilder) -> (Option<RecordFacts>, String) {
        let mut arena = String::new();
        let mut bytes = builder.build(30, 1024);
        let facts = parse_record(&mut bytes, &mut arena).expect("record should parse");
        (facts, arena)
    }

    fn name_of<'a>(facts: &RecordFacts, arena: &'a str) -> &'a str {
        let n = facts.name.expect("record should have a name");
        &arena[n.off as usize..(n.off + n.len as u32) as usize]
    }

    #[test]
    fn parses_minimal_file_record() {
        let (facts, arena) = parse_one(
            RecordBuilder::file()
                .seq(7)
                .std_info(0, FT_2020)
                .name(5, 5, 1, "report.txt")
                .data_nonresident(120_000, 122_880),
        );
        let f = facts.unwrap();
        assert_eq!(name_of(&f, &arena), "report.txt");
        assert_eq!(f.name.unwrap().parent.number(), 5);
        assert_eq!(f.name.unwrap().parent.sequence(), 5);
        assert_eq!(f.seq, 7);
        assert_eq!(f.base, 0);
        assert!(!f.is_dir);
        assert_eq!(f.logical, 120_000);
        assert_eq!(f.alloc, 122_880);
        assert_eq!(f.mtime, UNIX_2020);
        assert_eq!(f.link_names, 1);
    }

    #[test]
    fn parses_directory_record() {
        let (facts, _) = parse_one(
            RecordBuilder::dir()
                .std_info(0, FT_2020)
                .name(5, 5, 3, "src"),
        );
        assert!(facts.unwrap().is_dir);
    }

    #[test]
    fn free_record_is_skipped_not_an_error() {
        let (facts, _) = parse_one(RecordBuilder::free());
        assert!(facts.is_none());
    }

    #[test]
    fn zeroed_and_garbage_buffers_are_skipped() {
        let mut arena = String::new();
        let mut zeroed = vec![0u8; 1024];
        assert!(parse_record(&mut zeroed, &mut arena).unwrap().is_none());
        let mut garbage = vec![0xABu8; 1024];
        assert!(parse_record(&mut garbage, &mut arena).unwrap().is_none());
    }

    #[test]
    fn torn_record_is_an_error() {
        let mut bytes = RecordBuilder::file()
            .std_info(0, FT_2020)
            .name(5, 5, 1, "torn.bin")
            .build(30, 1024);
        // Corrupt the protected last word of the second sector — as if the
        // second half of the record came from an older write.
        bytes[1022] ^= 0xFF;
        let mut arena = String::new();
        assert_eq!(
            parse_record(&mut bytes, &mut arena),
            Err(ParseError("torn record (fixup mismatch)"))
        );
    }

    #[test]
    fn fixup_restores_protected_words() {
        // A resident value that spans the first sector boundary: bytes at the
        // sector end travel through the fixup array and must come back intact.
        let payload: Vec<u8> = (0..=255).cycle().take(700).map(|b| b as u8).collect();
        let mut bytes = RecordBuilder::file()
            .name(5, 5, 1, "spans.bin")
            .data_resident_bytes(&payload)
            .build(30, 1024);
        let mut arena = String::new();
        let facts = parse_record(&mut bytes, &mut arena).unwrap().unwrap();
        assert_eq!(facts.logical, 700);
        // The parser saw the *restored* bytes; verify directly too.
        let start = bytes.windows(700).any(|w| w == payload);
        assert!(start, "payload should be restored verbatim after fixups");
    }

    #[test]
    fn resident_data_reports_zero_allocated() {
        let (facts, _) = parse_one(
            RecordBuilder::file()
                .name(5, 5, 1, "tiny.ini")
                .data_resident_bytes(&[1, 2, 3, 4, 5]),
        );
        let f = facts.unwrap();
        assert_eq!(f.logical, 5);
        assert_eq!(f.alloc, 0);
    }

    #[test]
    fn compressed_data_uses_total_allocated_and_flags() {
        let (facts, _) = parse_one(
            RecordBuilder::file()
                .name(5, 5, 1, "log.txt")
                .data_nonresident_compressed(1_000_000, 1_048_576, 65_536),
        );
        let f = facts.unwrap();
        assert_eq!(f.logical, 1_000_000);
        assert_eq!(f.alloc, 65_536, "allocated must be the backed bytes");
        assert!(f.flags.contains(EntryFlags::COMPRESSED));
    }

    #[test]
    fn sparse_data_sets_sparse_flag() {
        let (facts, _) = parse_one(
            RecordBuilder::file()
                .name(5, 5, 1, "sparse.dat")
                .data_nonresident_sparse(10_000_000, 4096),
        );
        let f = facts.unwrap();
        assert_eq!(f.logical, 10_000_000);
        assert_eq!(f.alloc, 4096);
        assert!(f.flags.contains(EntryFlags::SPARSE));
    }

    #[test]
    fn named_streams_add_allocated_but_not_logical() {
        // The WOF/CompactOS shape: sparse main stream ≈ 0 backed bytes, real
        // data in the WofCompressedData ADS. Σ-streams prices it correctly.
        let (facts, _) = parse_one(
            RecordBuilder::file()
                .name(5, 5, 1, "system.dll")
                .data_nonresident_sparse(500_000, 0)
                .named_data_nonresident("WofCompressedData", 180_000, 184_320)
                .reparse(0x8000_0017), // IO_REPARSE_TAG_WOF
        );
        let f = facts.unwrap();
        assert_eq!(f.logical, 500_000, "logical comes from the main stream");
        assert_eq!(f.alloc, 184_320, "allocated sums all streams");
        assert!(reparse_entry_flags(f.reparse_tag).contains(EntryFlags::COMPRESSED));
        assert!(!reparse_entry_flags(f.reparse_tag).contains(EntryFlags::REPARSE));
    }

    #[test]
    fn continuation_fragment_contributes_nothing() {
        let (facts, _) = parse_one(
            RecordBuilder::file()
                .name(5, 5, 1, "frag.bin")
                .data_nonresident(4096, 4096)
                .data_continuation(64),
        );
        let f = facts.unwrap();
        assert_eq!(f.logical, 4096);
        assert_eq!(f.alloc, 4096);
    }

    #[test]
    fn dos_name_loses_to_win32_name_regardless_of_order() {
        let (facts, arena) = parse_one(RecordBuilder::file().name(5, 5, NS_DOS, "PROGRA~1").name(
            5,
            5,
            1,
            "Program Files",
        ));
        let f = facts.unwrap();
        assert_eq!(name_of(&f, &arena), "Program Files");
        assert_eq!(f.link_names, 1, "the DOS alias is not a hardlink");
    }

    #[test]
    fn dos_only_name_is_still_used() {
        let (facts, arena) = parse_one(RecordBuilder::file().name(5, 5, NS_DOS, "LEGACY~1.TXT"));
        let f = facts.unwrap();
        assert_eq!(name_of(&f, &arena), "LEGACY~1.TXT");
        assert_eq!(f.link_names, 0);
    }

    #[test]
    fn hardlinks_keep_first_win32_name_and_count_links() {
        let (facts, arena) = parse_one(RecordBuilder::file().name(5, 5, 1, "first-link.dll").name(
            9,
            2,
            1,
            "second-link.dll",
        ));
        let f = facts.unwrap();
        assert_eq!(name_of(&f, &arena), "first-link.dll");
        assert_eq!(f.name.unwrap().parent.number(), 5);
        assert_eq!(f.link_names, 2);
    }

    #[test]
    fn unicode_names_survive_utf16_conversion() {
        let (facts, arena) = parse_one(RecordBuilder::file().name(5, 5, 1, "日本語 🗾.txt"));
        assert_eq!(name_of(&facts.unwrap(), &arena), "日本語 🗾.txt");
    }

    #[test]
    fn unpaired_surrogate_becomes_replacement_char() {
        let (facts, arena) = parse_one(RecordBuilder::file().name_utf16(
            5,
            5,
            1,
            &[0x0061, 0xD800, 0x0062], // "a", lone high surrogate, "b"
        ));
        assert_eq!(name_of(&facts.unwrap(), &arena), "a\u{FFFD}b");
    }

    #[test]
    fn junction_and_symlink_tags_map_to_reparse_flag() {
        let (facts, _) = parse_one(
            RecordBuilder::dir()
                .name(5, 5, 3, "Documents and Settings")
                .reparse(0xA000_0003), // IO_REPARSE_TAG_MOUNT_POINT
        );
        let f = facts.unwrap();
        assert_eq!(f.reparse_tag, 0xA000_0003);
        assert!(reparse_entry_flags(f.reparse_tag).contains(EntryFlags::REPARSE));
        assert!(reparse_entry_flags(0xA000_000C).contains(EntryFlags::REPARSE)); // symlink
    }

    #[test]
    fn cloud_placeholder_tags_map_to_placeholder_flag() {
        assert!(reparse_entry_flags(0x9000_001A).contains(EntryFlags::PLACEHOLDER));
        assert!(reparse_entry_flags(0x9000_601A).contains(EntryFlags::PLACEHOLDER));
        assert!(!reparse_entry_flags(0x9000_601A).contains(EntryFlags::REPARSE));
        assert!(!reparse_entry_flags(0xA000_0003).contains(EntryFlags::PLACEHOLDER));
    }

    #[test]
    fn system_attribute_sets_system_flag() {
        let (facts, _) = parse_one(
            RecordBuilder::file()
                .std_info(0x6, FT_2020) // HIDDEN | SYSTEM
                .name(5, 5, 1, "pagefile.sys"),
        );
        assert!(facts.unwrap().flags.contains(EntryFlags::SYSTEM));
    }

    #[test]
    fn extension_record_reports_its_base() {
        let (facts, _) = parse_one(
            RecordBuilder::file()
                .extension_of(42, 3)
                .data_nonresident(8192, 8192),
        );
        let f = facts.unwrap();
        assert_eq!(f.base, 42);
        assert_eq!(f.alloc, 8192);
    }

    #[test]
    fn attribute_list_is_skipped_without_effect() {
        let (facts, arena) = parse_one(
            RecordBuilder::file()
                .attribute_list_stub()
                .name(5, 5, 1, "big.mkv")
                .data_nonresident(1 << 30, 1 << 30),
        );
        let f = facts.unwrap();
        assert_eq!(name_of(&f, &arena), "big.mkv");
        assert_eq!(f.logical, 1 << 30);
    }

    #[test]
    fn corrupt_attribute_length_is_an_error_not_a_panic() {
        let mut bytes = RecordBuilder::file()
            .name(5, 5, 1, "x")
            .std_info(0, FT_2020)
            .build(30, 1024);
        let first_attr = u16_at(&bytes, 0x14) as usize;
        // Attribute length 0 would loop forever; unaligned/oversized walk off.
        for bad_len in [0u32, 7, 100_000] {
            let mut copy = bytes.clone();
            copy[first_attr + 4..first_attr + 8].copy_from_slice(&bad_len.to_le_bytes());
            let mut arena = String::new();
            assert!(
                parse_record(&mut copy, &mut arena).is_err(),
                "len {bad_len} must be rejected"
            );
        }
        // Also: first-attribute offset pointing past the record.
        bytes[0x14..0x16].copy_from_slice(&2000u16.to_le_bytes());
        let mut arena = String::new();
        assert!(parse_record(&mut bytes, &mut arena).is_err());
    }

    #[test]
    fn pseudo_random_garbage_never_panics() {
        // Deterministic xorshift junk with a valid "FILE" magic + in-use flag
        // forced in, so the parser gets past the early outs and must survive
        // the fixup/attribute machinery on hostile bytes.
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut arena = String::new();
        for _ in 0..2000 {
            let mut buf = vec![0u8; 1024];
            for b in buf.iter_mut() {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *b = state as u8;
            }
            buf[0..4].copy_from_slice(b"FILE");
            buf[0x16] |= 0x1;
            let _ = parse_record(&mut buf, &mut arena); // any Ok/Err, no panic
        }
    }

    #[test]
    fn record0_yields_mft_extents_and_size() {
        // Two runs: 0x100 clusters at LCN 0x400, then 0x80 at LCN 0x300.
        let runs = [0x22, 0x00, 0x01, 0x00, 0x04, 0x21, 0x80, 0x00, 0xFF, 0x00];
        let mut rec = RecordBuilder::file()
            .std_info(0x6, FT_2020)
            .name(5, 5, 3, "$MFT")
            .data_nonresident_with_runs(0x180 * 4096, 0x17F, &runs)
            .build(0, 1024);
        let m = parse_record0(&mut rec).unwrap();
        assert_eq!(m.data_size, 0x180 * 4096);
        assert_eq!(m.extents.len(), 2);
        assert_eq!(m.extents[0].lcn, 0x400);
        assert_eq!(m.extents[0].clusters, 0x100);
        assert_eq!(m.extents[1].lcn, 0x300);
        assert_eq!(m.extents[1].clusters, 0x80);
    }

    #[test]
    fn record0_with_incomplete_runs_is_an_error() {
        // Runs cover 0x100 clusters but the VCN range claims 0x180.
        let runs = [0x22, 0x00, 0x01, 0x00, 0x04, 0x00];
        let mut rec = RecordBuilder::file()
            .name(5, 5, 3, "$MFT")
            .data_nonresident_with_runs(0x180 * 4096, 0x17F, &runs)
            .build(0, 1024);
        assert_eq!(
            parse_record0(&mut rec),
            Err(ParseError("$MFT runs incomplete in record 0"))
        );
    }

    #[test]
    fn record0_rejects_free_or_dataless_records() {
        let mut free = RecordBuilder::free().build(0, 1024);
        assert!(parse_record0(&mut free).is_err());
        let mut no_data = RecordBuilder::file().name(5, 5, 3, "$MFT").build(0, 1024);
        assert!(parse_record0(&mut no_data).is_err());
    }

    #[test]
    fn filetime_conversion_matches_known_values() {
        assert_eq!(filetime_to_unix(0), 0);
        assert_eq!(filetime_to_unix(116_444_736_000_000_000), 0); // 1970-01-01
        assert_eq!(filetime_to_unix(FT_2020), UNIX_2020);
        // Pre-1970 truncates toward zero (walker semantics).
        assert_eq!(filetime_to_unix(116_444_735_985_000_000), -1);
        assert_eq!(filetime_to_unix(116_444_736_015_000_000), 1);
    }
}
