//! Converts the swept record table into parent-before-child `EntryBatch`es.

use std::collections::VecDeque;

use mathom_core::{EntryBatch, EntryFlags, FileEntry};

use crate::ParseError;
use crate::pipeline::{NO_NAME, Slot, Table};
use crate::record::{ROOT_RECORD, RecordRef};

#[derive(Clone, Copy, Debug, Default)]
pub struct AssembleStats {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub allocated: u64,
    /// Live records whose parent link failed validation (missing, freed,
    /// reused sequence, not a directory, or self-referential).
    pub orphans: u64,
    pub cancelled: bool,
}

/// Emits a subtree as batches; `send` returning false cancels.
pub fn assemble(
    table: &Table,
    subtree: &[&str],
    root_path: &str,
    batch_size: usize,
    mut send: impl FnMut(EntryBatch) -> bool,
) -> Result<AssembleStats, ParseError> {
    let n = table.records.len();
    if ROOT_RECORD as usize >= n || !is_live_dir(&table.records[ROOT_RECORD as usize]) {
        return Err(ParseError("volume root record missing from $MFT"));
    }

    let mut child_count = vec![0u32; n];
    let mut linked = vec![false; n];
    let mut orphans = 0u64;
    for (i, slot) in table.records.iter().enumerate() {
        if !slot.is_base() || i == ROOT_RECORD as usize {
            continue;
        }
        if slot.rank == NO_NAME {
            orphans += 1;
            continue;
        }
        let parent = RecordRef(slot.parent_ref);
        let p = parent.number() as usize;
        let valid = p != i
            && table
                .records
                .get(p)
                .is_some_and(|ps| is_live_dir(ps) && ps.seq == parent.sequence());
        if !valid {
            orphans += 1;
            continue;
        }
        child_count[p] += 1;
        linked[i] = true;
    }

    let mut starts = vec![0u32; n + 1];
    for i in 0..n {
        starts[i + 1] = starts[i] + child_count[i];
    }
    let mut cursor: Vec<u32> = starts[..n].to_vec();
    let mut children = vec![0u32; starts[n] as usize];
    for (i, &is_linked) in linked.iter().enumerate() {
        if is_linked {
            let p = RecordRef(table.records[i].parent_ref).number() as usize;
            children[cursor[p] as usize] = i as u32;
            cursor[p] += 1;
        }
    }
    let kids = |rec: usize| &children[starts[rec] as usize..starts[rec + 1] as usize];

    let mut start = ROOT_RECORD as usize;
    for comp in subtree {
        start = kids(start)
            .iter()
            .map(|&c| c as usize)
            .find(|&c| {
                let s = &table.records[c];
                s.flags.is_dir()
                    && !s.flags.contains(EntryFlags::REPARSE)
                    && names_equal_ci(table.name(s), comp)
            })
            .ok_or(ParseError("scan root not found in $MFT"))?;
    }

    let mut stats = AssembleStats::default();
    let mut root_batch = EntryBatch::with_capacity(1, root_path.len());
    root_batch.push(
        root_path,
        FileEntry {
            path_id: 0,
            parent_id: 0,
            name_off: 0,
            name_len: 0,
            flags: EntryFlags::DIR,
            size: 0,
            allocated_size: 0,
            mtime: table.records[start].mtime,
        },
    );
    stats.dirs = 1;
    if !send(root_batch) {
        stats.cancelled = true;
        return Ok(stats);
    }

    let mut queue: VecDeque<(u32, u32)> = VecDeque::new();
    queue.push_back((start as u32, 0));
    let mut next_id = 1u32;
    let mut batch = EntryBatch::with_capacity(batch_size, batch_size * 16);

    while let Some((rec, parent_id)) = queue.pop_front() {
        for &c in kids(rec as usize) {
            let slot = &table.records[c as usize];
            let mut flags = slot.flags;
            if slot.link_names > 1 {
                flags.insert(EntryFlags::HARDLINK);
            }
            let is_reparse = flags.contains(EntryFlags::REPARSE);
            if is_reparse {
                flags.remove(EntryFlags::DIR);
            }
            let is_dir = flags.is_dir();
            let (size, alloc) = if is_dir || is_reparse {
                (0, 0)
            } else {
                (slot.logical, slot.alloc)
            };

            let id = next_id;
            next_id += 1;
            batch.push(
                table.name(slot),
                FileEntry {
                    path_id: id,
                    parent_id,
                    name_off: 0,
                    name_len: 0,
                    flags,
                    size,
                    allocated_size: alloc,
                    mtime: slot.mtime,
                },
            );
            if is_dir {
                stats.dirs += 1;
                queue.push_back((c, id));
            } else {
                stats.files += 1;
                stats.bytes += size;
                stats.allocated += alloc;
            }

            if batch.len() >= batch_size {
                let full = std::mem::replace(
                    &mut batch,
                    EntryBatch::with_capacity(batch_size, batch_size * 16),
                );
                if !send(full) {
                    stats.cancelled = true;
                    return Ok(stats);
                }
            }
        }
    }
    if !batch.is_empty() && !send(batch) {
        stats.cancelled = true;
    }
    stats.orphans = orphans;
    Ok(stats)
}

fn is_live_dir(slot: &Slot) -> bool {
    slot.is_base() && slot.flags.is_dir() && !slot.flags.contains(EntryFlags::REPARSE)
}

/// NTFS-style case-insensitive comparison.
fn names_equal_ci(a: &str, b: &str) -> bool {
    if a.is_ascii() && b.is_ascii() {
        return a.eq_ignore_ascii_case(b);
    }
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::{RecordBuilder, image, root_dir};
    use crate::pipeline::Sweep;
    use mathom_core::TreeBuilder;

    const FT_2020: u64 = 132_223_104_000_000_000;

    /// A little volume exercising the tree-shaping rules end to end:
    ///
    /// ```text
    /// C:\                       (record 5)
    /// ├── docs\                 (16)
    /// │   ├── a.txt   1000 B    (17)
    /// │   └── link.bin 512 B    (20, hardlinked here first + into root)
    /// ├── big.mkv    2 GiB      (18, $DATA via extension record 19)
    /// ├── junction\             (21, mount point — leaf, never descended)
    /// ├── orphan.txt            (22, parent is a *file* → invalid)
    /// ├── stale.txt             (23, parent seq mismatch → invalid)
    /// └── free slot             (24, deleted record)
    /// ```
    fn little_volume() -> Vec<u8> {
        image(
            1024,
            vec![
                (5, root_dir()),
                (
                    16,
                    RecordBuilder::dir()
                        .std_info(0, FT_2020)
                        .name(5, 5, 1, "docs"),
                ),
                (
                    17,
                    RecordBuilder::file()
                        .std_info(0, FT_2020)
                        .name(16, 1, 1, "a.txt")
                        .data_nonresident(1000, 4096),
                ),
                (
                    18,
                    RecordBuilder::file()
                        .seq(2)
                        .attribute_list_stub()
                        .name(5, 5, 1, "big.mkv"),
                ),
                (
                    19,
                    RecordBuilder::file()
                        .extension_of(18, 2)
                        .data_nonresident(2 << 30, 2 << 30),
                ),
                (
                    20,
                    RecordBuilder::file()
                        .name(16, 1, 1, "link.bin")
                        .name(5, 5, 1, "link-alias.bin")
                        .data_nonresident(512, 4096),
                ),
                (
                    21,
                    RecordBuilder::dir()
                        .name(5, 5, 1, "junction")
                        .reparse(0xA000_0003),
                ),
                (
                    22,
                    RecordBuilder::file()
                        .name(17, 1, 1, "orphan.txt") // parent is a file
                        .data_nonresident(64, 4096),
                ),
                (
                    23,
                    RecordBuilder::file()
                        .name(16, 9, 1, "stale.txt") // seq 9 ≠ actual 1
                        .data_nonresident(64, 4096),
                ),
                (24, RecordBuilder::free()),
            ],
        )
    }

    fn sweep(mut img: Vec<u8>) -> Table {
        let total = (img.len() / 1024) as u32;
        let mut s = Sweep::new(total, 1024, crate::fixture::CLUSTER as u32).unwrap();
        s.consume(0, &mut img);
        s.finish()
    }

    fn build_tree(
        table: &Table,
        subtree: &[&str],
        root_path: &str,
    ) -> (TreeBuilder, AssembleStats) {
        let mut builder = TreeBuilder::new();
        let stats = assemble(table, subtree, root_path, 3, |b| {
            builder.add_batch(&b);
            true
        })
        .expect("assemble should succeed");
        (builder, stats)
    }

    fn child_named(builder: &TreeBuilder, dir: u32, name: &str) -> Option<u32> {
        let tree = builder.tree();
        tree.children(dir).find(|&c| tree.name(c) == name)
    }

    #[test]
    fn full_volume_tree_shape_and_sizes() {
        let table = sweep(little_volume());
        let (builder, stats) = build_tree(&table, &[], "C:\\");
        let tree = builder.tree();

        assert_eq!(stats.files, 4);
        assert_eq!(stats.dirs, 2); // root, docs
        assert_eq!(stats.bytes, 1000 + (2u64 << 30) + 512);
        assert_eq!(stats.orphans, 2, "bad-parent + stale-seq records");
        assert!(!stats.cancelled);

        assert_eq!(tree.name(0), "C:\\");
        assert_eq!(tree.node(0).size, 1000 + (2u64 << 30) + 512);

        let docs = child_named(&builder, 0, "docs").unwrap();
        assert_eq!(tree.node(docs).size, 1000 + 512);
        assert!(child_named(&builder, docs, "a.txt").is_some());

        let link = child_named(&builder, docs, "link.bin").unwrap();
        assert!(tree.node(link).flags.contains(EntryFlags::HARDLINK));
        assert!(child_named(&builder, 0, "link-alias.bin").is_none());

        let big = child_named(&builder, 0, "big.mkv").unwrap();
        assert_eq!(tree.node(big).size, 2 << 30);
        let junction = child_named(&builder, 0, "junction").unwrap();
        assert!(tree.node(junction).flags.contains(EntryFlags::REPARSE));
        assert!(!tree.node(junction).is_dir());
        assert_eq!(tree.node(junction).size, 0);

        assert!(child_named(&builder, 0, "orphan.txt").is_none());
        assert!(child_named(&builder, docs, "stale.txt").is_none());
    }

    #[test]
    fn subtree_scan_resolves_case_insensitively() {
        let table = sweep(little_volume());
        let (builder, stats) = build_tree(&table, &["DOCS"], "C:\\docs");
        let tree = builder.tree();

        assert_eq!(tree.name(0), "C:\\docs");
        assert_eq!(stats.files, 2); // a.txt + link.bin
        assert_eq!(stats.dirs, 1);
        assert_eq!(tree.node(0).size, 1000 + 512);
        assert!(child_named(&builder, 0, "a.txt").is_some());
        assert!(child_named(&builder, 0, "big.mkv").is_none());
    }

    #[test]
    fn unknown_subtree_is_an_error() {
        let table = sweep(little_volume());
        let err = assemble(&table, &["nope"], "C:\\nope", 64, |_| true).unwrap_err();
        assert_eq!(err, ParseError("scan root not found in $MFT"));
    }

    #[test]
    fn junction_path_component_does_not_resolve() {
        let table = sweep(little_volume());
        assert!(assemble(&table, &["junction"], "C:\\junction", 64, |_| true).is_err());
    }

    #[test]
    fn cancellation_stops_mid_emit() {
        let table = sweep(little_volume());
        let mut sent = 0;
        let stats = assemble(&table, &[], "C:\\", 1, |_| {
            sent += 1;
            sent < 2
        })
        .unwrap();
        assert!(stats.cancelled);
        assert!(sent <= 2);
    }

    #[test]
    fn mtime_lands_on_entries() {
        let table = sweep(little_volume());
        let (builder, _) = build_tree(&table, &[], "C:\\");
        let tree = builder.tree();
        let docs = child_named(&builder, 0, "docs").unwrap();
        assert_eq!(tree.node(docs).mtime, 1_577_836_800); // 2020-01-01
    }

    #[test]
    fn missing_root_record_is_an_error() {
        let img = image(1024, vec![(6, RecordBuilder::dir().name(5, 5, 1, "x"))]);
        let table = sweep(img);
        assert!(assemble(&table, &[], "C:\\", 64, |_| true).is_err());
    }
}
