//! Record-for-record parity between our hand-rolled reader and the `mft`
//! crate — the north-star correctness oracle (plan.md). The oracle parses
//! the *same bytes*; our documented policy (name ranking, backed-run
//! allocation, VCN-0 logical, extension merge) is recomputed on top of the
//! oracle's parsed fields, so every byte-offset interpretation is genuinely
//! cross-checked rather than asserted against our own output.
//!
//! Run: `cargo test -p mathom-scanner-ntfs --features oracle-tests`
//! Real-volume parity: set `MATHOM_MFT_DUMP` to a raw $MFT dump (see
//! `examples/dump_mft.rs`, needs elevation) before running.

use std::collections::BTreeMap;

use mathom_scanner_ntfs::fixture::{RecordBuilder, image, root_dir};
use mathom_scanner_ntfs::pipeline::{NO_NAME, Sweep, Table};

use mft::MftParser;
use mft::attribute::data_run::RunType;
use mft::attribute::header::ResidentialHeader;
use mft::attribute::x30::FileNamespace;
use mft::attribute::{MftAttributeContent, MftAttributeType};
use mft::entry::MftEntry;

/// Fixtures are built for 4 KiB clusters; the real C: dump (no boot sector
/// travels with it) is also a 4 KiB-cluster volume.
const CLUSTER: u64 = mathom_scanner_ntfs::fixture::CLUSTER;

/// One merged base record as the oracle sees it (crate-parsed fields, our
/// policy arithmetic).
#[derive(Debug, Default)]
struct Expect {
    is_dir: bool,
    /// (name, parent record, parent sequence)
    name: Option<(String, u64, u16)>,
    rank: u8,
    link_names: u32,
    logical: u64,
    has_logical: bool,
    alloc: u64,
    mtime: i64,
    system: bool,
    sparse: bool,
    compressed_data: bool,
}

fn namespace_byte(ns: &FileNamespace) -> u8 {
    match ns {
        FileNamespace::POSIX => 0,
        FileNamespace::Win32 => 1,
        FileNamespace::DOS => 2,
        FileNamespace::Win32AndDos => 3,
    }
}

fn name_rank(ns: u8) -> u8 {
    match ns {
        1 | 3 => 0,
        0 => 1,
        2 => 2,
        _ => 3,
    }
}

/// Folds one oracle-parsed entry's attributes into an [`Expect`], mirroring
/// the documented policy exactly (first-wins per rank, Σ allocated, VCN-0).
fn fold_entry(e: &MftEntry, x: &mut Expect) {
    for attr in e.iter_attributes() {
        let attr = attr.expect("oracle should parse fixture attributes");
        let unnamed = attr.header.name_size == 0;

        // $DATA logical size comes from the attribute *header*; allocation
        // comes from the run list (the crate wraps non-resident content as
        // DataRun, resident as AttrX80).
        if attr.header.type_code == MftAttributeType::DATA {
            match &attr.header.residential_header {
                ResidentialHeader::Resident(r) => {
                    if unnamed && !x.has_logical {
                        x.logical = r.data_size as u64;
                        x.has_logical = true;
                    }
                }
                ResidentialHeader::NonResident(nr) => {
                    // Mirror the backed-run policy: Σ Standard runs × cluster
                    // (holes back nothing), with the parser's header fallback
                    // when no run list is available.
                    let backed = match &attr.data {
                        MftAttributeContent::DataRun(rl) => Some(
                            rl.data_runs
                                .iter()
                                .filter(|r| r.run_type == RunType::Standard)
                                .map(|r| r.lcn_length)
                                .sum::<u64>()
                                * CLUSTER,
                        ),
                        _ => None,
                    };
                    if nr.vnc_first != 0 {
                        x.alloc += backed.unwrap_or(0);
                        continue;
                    }
                    x.alloc += backed.unwrap_or_else(|| {
                        if nr.unit_compression_size != 0 {
                            nr.total_allocated.unwrap_or(nr.allocated_length)
                        } else {
                            nr.allocated_length
                        }
                    });
                    if unnamed && !x.has_logical {
                        x.logical = nr.file_size;
                        x.has_logical = true;
                        let bits = attr.header.data_flags.bits();
                        x.sparse = bits & 0x8000 != 0;
                        x.compressed_data = bits & 0x00FF != 0;
                    }
                }
            }
            continue;
        }

        match &attr.data {
            MftAttributeContent::AttrX10(si) => {
                if x.mtime == 0 {
                    // Mirror the documented policy: FILETIME 0 means
                    // "unknown" and stays 0 (the oracle converts literally
                    // to 1601-01-01).
                    let secs = si.modified.as_second();
                    x.mtime = if secs == -11_644_473_600 && si.modified.subsec_nanosecond() == 0 {
                        0
                    } else {
                        secs
                    };
                }
                const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
                if si.file_flags.bits() & FILE_ATTRIBUTE_SYSTEM != 0 {
                    x.system = true;
                }
            }
            MftAttributeContent::AttrX30(fname) => {
                let ns = namespace_byte(&fname.namespace);
                if ns != 2 {
                    x.link_names += 1;
                }
                let rank = name_rank(ns);
                if rank < x.rank {
                    x.rank = rank;
                    x.name = Some((
                        fname.name.clone(),
                        fname.parent.entry,
                        fname.parent.sequence,
                    ));
                }
            }
            _ => {}
        }
    }
}

/// Parses an image with the oracle: merged base-record expectations plus
/// the records the oracle itself flags as torn.
fn oracle_expectations(img: &[u8]) -> (BTreeMap<u64, Expect>, Vec<u64>) {
    let mut parser = MftParser::from_buffer(img.to_vec()).expect("oracle accepts the image");
    let mut bases: BTreeMap<u64, Expect> = BTreeMap::new();
    let mut extensions: Vec<MftEntry> = Vec::new();
    let mut torn = Vec::new();

    for entry in parser.iter_entries() {
        let Ok(e) = entry else { continue }; // zeroed / non-record slots
        if !e.is_allocated() {
            continue;
        }
        if e.valid_fixup == Some(false) {
            torn.push(e.header.record_number);
            continue;
        }
        if e.header.base_reference.entry != 0 {
            extensions.push(e);
            continue;
        }
        let mut x = Expect {
            is_dir: e.is_dir(),
            rank: NO_NAME,
            ..Expect::default()
        };
        fold_entry(&e, &mut x);
        bases.insert(e.header.record_number, x);
    }
    // Extensions merge after all bases, in record order — exactly like the
    // pipeline's patch phase. Patches to unseen bases drop on both sides.
    for e in extensions {
        if let Some(x) = bases.get_mut(&e.header.base_reference.entry) {
            fold_entry(&e, x);
        }
    }
    (bases, torn)
}

fn sweep(img: &[u8]) -> Table {
    let record_size = 1024;
    let total = img.len() / record_size;
    let mut owned = img.to_vec();
    let mut sweep = Sweep::new(total as u32, record_size, CLUSTER as u32).unwrap();
    // Feed in two chunks to keep the chunked path honest.
    let split = (total / 2) * record_size;
    let (a, b) = owned.split_at_mut(split);
    sweep.consume(0, a);
    sweep.consume(split / record_size, b);
    sweep.finish()
}

fn without_replacement_chars(s: &str) -> impl Iterator<Item = char> + '_ {
    s.chars().filter(|&c| c != char::REPLACEMENT_CHARACTER)
}

/// Chosen-name comparison, modulo one decode-policy difference: NTFS names
/// are arbitrary u16 sequences, and we replace invalid UTF-16 (unpaired
/// surrogates) with U+FFFD — matching the generic walker's
/// `to_string_lossy` and Unicode TR36 (replace, never delete). The oracle
/// decodes with `DecoderTrap::Ignore` (mft x30.rs), silently *dropping*
/// bad units. Comparing with U+FFFD stripped from both sides keeps every
/// real divergence fatal while accepting the policy gap (found by real-C:
/// parity: a cache file whose name embeds four lone surrogates).
fn names_match(ours: &str, oracle: &str) -> bool {
    ours == oracle || without_replacement_chars(ours).eq(without_replacement_chars(oracle))
}

/// The parity gate: every merged fact equal, record for record, both ways.
fn assert_parity(img: &[u8]) {
    let (expects, torn) = oracle_expectations(img);
    let table = sweep(img);

    for (&no, exp) in &expects {
        let slot = &table.records[no as usize];
        assert!(slot.is_base(), "record {no}: live in oracle, empty in ours");
        assert_eq!(
            slot.flags.is_dir(),
            exp.is_dir,
            "record {no}: directory bit"
        );
        match &exp.name {
            Some((name, parent, pseq)) => {
                assert_ne!(slot.rank, NO_NAME, "record {no}: we lost the name");
                let ours_name = table.name(slot);
                assert!(
                    names_match(ours_name, name),
                    "record {no}: chosen name\n  ours:   {ours_name:?}\n  oracle: {name:?}"
                );
                let ours = mathom_scanner_ntfs::record::RecordRef(slot.parent_ref);
                assert_eq!(ours.number(), *parent, "record {no}: parent");
                assert_eq!(ours.sequence(), *pseq, "record {no}: parent sequence");
            }
            None => assert_eq!(slot.rank, NO_NAME, "record {no}: phantom name"),
        }
        assert_eq!(
            u32::from(slot.link_names),
            exp.link_names,
            "record {no}: link count"
        );
        assert_eq!(slot.logical, exp.logical, "record {no}: logical size");
        assert_eq!(slot.alloc, exp.alloc, "record {no}: allocated size");
        assert_eq!(slot.mtime, exp.mtime, "record {no}: mtime");
        use mathom_core::EntryFlags;
        assert_eq!(
            slot.flags.contains(EntryFlags::SYSTEM),
            exp.system,
            "record {no}: system flag"
        );
        assert_eq!(
            slot.flags.contains(EntryFlags::SPARSE),
            exp.sparse,
            "record {no}: sparse flag"
        );
        if exp.compressed_data {
            assert!(
                slot.flags.contains(EntryFlags::COMPRESSED),
                "record {no}: compressed flag lost"
            );
        }
    }

    for (i, slot) in table.records.iter().enumerate() {
        if slot.is_base() {
            assert!(
                expects.contains_key(&(i as u64)),
                "record {i}: live in ours, absent in oracle"
            );
        }
    }

    assert_eq!(table.torn, torn.len() as u64, "torn-record count");
    for no in torn {
        assert!(
            !table.records[no as usize].is_base(),
            "record {no}: oracle says torn, we placed it"
        );
    }
}

/// Record 0 stand-in so the oracle can size entries from the first record.
fn mft_metafile() -> RecordBuilder {
    RecordBuilder::file()
        .std_info(0x6, 130_000_000_000_000_000)
        .name(5, 5, 3, "$MFT")
        .data_nonresident(1 << 21, 1 << 21)
}

#[test]
fn parity_on_curated_volume() {
    let img = image(
        1024,
        vec![
            (0, mft_metafile()),
            (5, root_dir()),
            (
                16,
                RecordBuilder::dir()
                    .std_info(0x6, 131_000_000_000_000_000)
                    .name(5, 5, 1, "Windows"),
            ),
            (
                17,
                RecordBuilder::file()
                    .std_info(0, 132_000_000_000_000_000)
                    .name(16, 1, 1, "notepad.exe")
                    .data_nonresident(360_448, 364_544),
            ),
            (
                18,
                RecordBuilder::file()
                    .name(16, 1, 2, "COMPRE~1.LOG")
                    .name(16, 1, 1, "compressed.log")
                    .data_nonresident_compressed(1_000_000, 1_048_576, 65_536),
            ),
            (
                19,
                RecordBuilder::file()
                    .name(16, 1, 1, "sparse.vhd")
                    .data_nonresident_sparse(1 << 30, 1 << 20),
            ),
            (
                20,
                RecordBuilder::file()
                    .name(16, 1, 1, "wof-system.dll")
                    .data_nonresident_sparse(500_000, 0)
                    .named_data_nonresident("WofCompressedData", 180_000, 184_320)
                    .reparse(0x8000_0017),
            ),
            (
                21,
                RecordBuilder::file()
                    .name(5, 5, 1, "tiny.ini")
                    .data_resident_bytes(&[0x55; 129]),
            ),
            (
                22,
                RecordBuilder::file()
                    .name(5, 5, 1, "with-ads.doc")
                    .data_nonresident(8192, 8192)
                    .named_data_nonresident("Zone.Identifier", 26, 4096),
            ),
            (
                23,
                RecordBuilder::file()
                    .seq(9)
                    .attribute_list_stub()
                    .name(5, 5, 1, "fragmented.iso"),
            ),
            (
                24,
                RecordBuilder::file()
                    .extension_of(23, 9)
                    .data_nonresident(4 << 30, 4 << 30),
            ),
            (
                25,
                RecordBuilder::file()
                    .name(16, 1, 1, "hardlink-a.bin")
                    .name(5, 5, 1, "hardlink-b.bin")
                    .data_nonresident(512, 4096),
            ),
            (
                26,
                RecordBuilder::dir()
                    .name(5, 5, 1, "Documents and Settings")
                    .reparse(0xA000_0003),
            ),
            (
                27,
                RecordBuilder::file()
                    .name(5, 5, 1, "OneDrive dehydrated.png")
                    .data_nonresident_sparse(5_000_000, 4096)
                    .reparse(0x9000_601A),
            ),
            (28, RecordBuilder::file().name(5, 5, 1, "日本語 🗾.txt")),
            (29, RecordBuilder::free()),
            (31, RecordBuilder::file().name(5, 5, 0, "posix-only")),
            // Invalid UTF-16 (lone low surrogate mid-name): we emit U+FFFD,
            // the oracle drops the unit — exercises names_match's mirror.
            (
                32,
                RecordBuilder::file().name_utf16(
                    5,
                    5,
                    1,
                    &[
                        0x62, 0x61, 0x64, 0xDC00, 0x6E, 0x61, 0x6D, 0x65, 0x2E, 0x64, 0x61, 0x74,
                    ],
                ),
            ),
        ],
    );
    assert_parity(&img);
}

#[test]
fn parity_on_torn_record() {
    let mut img = image(
        1024,
        vec![
            (0, mft_metafile()),
            (5, root_dir()),
            (
                16,
                RecordBuilder::file()
                    .name(5, 5, 1, "torn.bin")
                    .data_nonresident(4096, 4096),
            ),
        ],
    );
    img[16 * 1024 + 510] ^= 0x5A; // tear the first sector's protected word
    assert_parity(&img);
}

/// ~400 records of deterministic pseudo-random shapes: the broad net that
/// catches byte-offset mistakes no hand-picked case would.
#[test]
fn parity_on_generated_volume() {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut rnd = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    let mut records: Vec<(u64, RecordBuilder)> = vec![(0, mft_metafile()), (5, root_dir())];
    let mut dirs: Vec<u64> = vec![5];
    let mut no = 16u64;
    while no < 416 {
        let parent = dirs[(rnd() % dirs.len() as u64) as usize];
        let pseq = if parent == 5 { 5 } else { 1 };
        let pick = rnd() % 100;
        let rec = match pick {
            0..=9 => {
                no += 1;
                continue; // gap: zeroed slot
            }
            10..=14 => RecordBuilder::free(),
            15..=29 => {
                dirs.push(no);
                RecordBuilder::dir()
                    .std_info(0, 130_000_000_000_000_000 + rnd() % (1 << 50))
                    .name(parent, pseq, 1, &format!("dir-{no}"))
            }
            30..=39 => RecordBuilder::file()
                .name(parent, pseq, 1, &format!("res-{no}.txt"))
                .data_resident_bytes(&vec![0xA5u8; (rnd() % 600) as usize]),
            40..=49 => RecordBuilder::file()
                .name(parent, pseq, 1, &format!("comp-{no}.log"))
                .data_nonresident_compressed(
                    rnd() % (1 << 30),
                    rnd() % (1 << 30),
                    rnd() % (1 << 24),
                ),
            50..=54 => RecordBuilder::file()
                .name(parent, pseq, 1, &format!("sparse-{no}.dat"))
                .data_nonresident_sparse(rnd() % (1 << 40), rnd() % (1 << 20)),
            55..=59 => RecordBuilder::file()
                .name(parent, pseq, 1, &format!("ads-{no}.doc"))
                .data_nonresident(rnd() % (1 << 20), rnd() % (1 << 20))
                .named_data_nonresident("Zone.Identifier", 26, rnd() % 8192),
            60..=64 => {
                let other = dirs[(rnd() % dirs.len() as u64) as usize];
                let oseq = if other == 5 { 5 } else { 1 };
                RecordBuilder::file()
                    .name(parent, pseq, 1, &format!("link-{no}-a"))
                    .name(other, oseq, 1, &format!("link-{no}-b"))
                    .data_nonresident(rnd() % (1 << 16), 4096)
            }
            65..=69 => RecordBuilder::file()
                .name(parent, pseq, 2, &format!("EIGHT~{}", no % 10))
                .name(parent, pseq, 1, &format!("long name {no} with spaces.dat"))
                .data_nonresident(rnd() % (1 << 22), rnd() % (1 << 22)),
            70..=74 => {
                let base_no = no;
                let base = RecordBuilder::file().seq(3).attribute_list_stub().name(
                    parent,
                    pseq,
                    1,
                    &format!("frag-{base_no}.iso"),
                );
                records.push((base_no, base));
                no += 1;
                records.push((
                    no,
                    RecordBuilder::file()
                        .extension_of(base_no, 3)
                        .data_nonresident(rnd() % (1 << 35), rnd() % (1 << 35)),
                ));
                no += 1;
                continue;
            }
            75..=79 => RecordBuilder::file().name(parent, pseq, 1, &format!("ünïcødé-{no}-✓")),
            _ => RecordBuilder::file()
                .std_info(if pick % 2 == 0 { 0x6 } else { 0 }, rnd() % (1 << 57))
                .name(parent, pseq, 1, &format!("file-{no}.bin"))
                .data_nonresident(rnd() % (1 << 32), rnd() % (1 << 32)),
        };
        records.push((no, rec));
        no += 1;
    }
    let img = image(1024, records);
    assert_parity(&img);
}

/// Parity over a real recorded $MFT (set `MATHOM_MFT_DUMP`); skipped when
/// no dump is present so the suite stays runnable anywhere.
#[test]
fn parity_on_real_dump_if_present() {
    let Ok(path) = std::env::var("MATHOM_MFT_DUMP") else {
        eprintln!("MATHOM_MFT_DUMP not set — skipping real-volume parity");
        return;
    };
    let img = std::fs::read(&path).expect("readable $MFT dump");
    let record_size = 1024usize;
    let whole = (img.len() / record_size) * record_size;
    eprintln!("real-dump parity over {} records…", whole / record_size);
    assert_parity(&img[..whole]);
}
