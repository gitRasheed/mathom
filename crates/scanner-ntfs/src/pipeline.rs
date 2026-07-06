//! Chunk-parallel sweep of raw $MFT bytes into a flat record table.
//!
//! The reader hands over buffers of whole FILE records; rayon parses them in
//! ~1024-record tasks, each writing its slice of the shared slot table and
//! filling a private name arena — zero locks and zero per-record heap in the
//! hot path. Extension-record contributions become patches applied once at
//! the end (`$ATTRIBUTE_LIST` is never parsed; a full sweep sees every
//! extension record anyway — see plan.md).

use mathom_core::EntryFlags;
use rayon::prelude::*;

use crate::record::{self, RecordFacts};

/// `rank` value meaning "no $FILE_NAME seen".
pub const NO_NAME: u8 = u8::MAX;

const STATE_BASE: u8 = 1;
const STATE_HAS_LOGICAL: u8 = 2;

/// Records per rayon task: big enough to amortize scheduling, small enough
/// to load-balance a 16-core sweep of a 32 MiB buffer.
const TASK_RECORDS: usize = 1024;

/// One MFT record's distilled facts, indexed by record number. 64 bytes.
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    pub parent_ref: u64,
    pub logical: u64,
    pub alloc: u64,
    pub mtime: i64,
    pub name_arena: u32,
    pub name_off: u32,
    pub name_len: u16,
    pub flags: EntryFlags,
    pub seq: u16,
    pub link_names: u16,
    pub rank: u8,
    state: u8,
}

impl Slot {
    const EMPTY: Slot = Slot {
        parent_ref: 0,
        logical: 0,
        alloc: 0,
        mtime: 0,
        name_arena: 0,
        name_off: 0,
        name_len: 0,
        flags: EntryFlags(0),
        seq: 0,
        link_names: 0,
        rank: NO_NAME,
        state: 0,
    };

    /// An in-use base record (extension records only ever patch these).
    pub fn is_base(&self) -> bool {
        self.state & STATE_BASE != 0
    }

    fn has_logical(&self) -> bool {
        self.state & STATE_HAS_LOGICAL != 0
    }
}

/// The sweep's final output: the slot table plus the name arenas the slots
/// point into.
pub struct Table {
    pub records: Vec<Slot>,
    pub arenas: Vec<String>,
    /// Torn/corrupt records skipped during parsing.
    pub torn: u64,
    /// Extension-record patches whose base record wasn't live.
    pub dropped_patches: u64,
}

impl Table {
    pub fn name(&self, slot: &Slot) -> &str {
        let arena = &self.arenas[slot.name_arena as usize];
        &arena[slot.name_off as usize..slot.name_off as usize + slot.name_len as usize]
    }
}

/// Live-progress deltas from one consumed buffer (approximate: extension
/// patches land later; the final stats come from the emit phase).
#[derive(Clone, Copy, Debug, Default)]
pub struct ChunkCounts {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
}

/// Streaming sweep state. Feed record-aligned buffers in any order via
/// [`Sweep::consume`], then [`Sweep::finish`] to apply extension patches.
pub struct Sweep {
    records: Vec<Slot>,
    arenas: Vec<String>,
    patches: Vec<(u64, Slot)>,
    record_size: usize,
    torn: u64,
}

struct TaskOut {
    arena: String,
    patches: Vec<(u64, Slot)>,
    counts: ChunkCounts,
    torn: u64,
}

impl Sweep {
    pub fn new(total_records: usize, record_size: usize) -> Self {
        assert!(record_size >= 512 && record_size.is_power_of_two());
        assert!(
            total_records <= u32::MAX as usize,
            "MFT record count fits u32"
        );
        Sweep {
            records: vec![Slot::EMPTY; total_records],
            arenas: Vec::new(),
            patches: Vec::new(),
            record_size,
            torn: 0,
        }
    }

    /// Parses one buffer of records starting at record number
    /// `first_record`, in parallel. Buffers may arrive in any order but
    /// must not overlap.
    pub fn consume(&mut self, first_record: usize, buf: &mut [u8]) -> ChunkCounts {
        let rs = self.record_size;
        assert!(
            buf.len().is_multiple_of(rs),
            "buffer must hold whole records"
        );
        let n = buf.len() / rs;
        assert!(first_record + n <= self.records.len());

        let arena_base = self.arenas.len() as u32;
        let outs: Vec<TaskOut> = self.records[first_record..first_record + n]
            .par_chunks_mut(TASK_RECORDS)
            .zip(buf.par_chunks_mut(TASK_RECORDS * rs))
            .enumerate()
            .map(|(task, (slots, bytes))| {
                let arena_idx = arena_base + task as u32;
                let mut out = TaskOut {
                    arena: String::with_capacity(slots.len() * 16),
                    patches: Vec::new(),
                    counts: ChunkCounts::default(),
                    torn: 0,
                };
                for (slot, rec) in slots.iter_mut().zip(bytes.chunks_mut(rs)) {
                    match record::parse_record(rec, &mut out.arena) {
                        Ok(Some(facts)) => place(facts, arena_idx, slot, &mut out),
                        Ok(None) => {}
                        Err(_) => out.torn += 1,
                    }
                }
                out
            })
            .collect();

        let mut counts = ChunkCounts::default();
        for out in outs {
            self.arenas.push(out.arena);
            self.patches.extend(out.patches);
            self.torn += out.torn;
            counts.files += out.counts.files;
            counts.dirs += out.counts.dirs;
            counts.bytes += out.counts.bytes;
        }
        counts
    }

    /// Applies extension-record patches to their base records.
    pub fn finish(mut self) -> Table {
        let mut dropped = 0u64;
        for (base, patch) in std::mem::take(&mut self.patches) {
            match self.records.get_mut(base as usize) {
                Some(target) if target.is_base() => merge(target, &patch),
                _ => dropped += 1,
            }
        }
        Table {
            records: self.records,
            arenas: self.arenas,
            torn: self.torn,
            dropped_patches: dropped,
        }
    }
}

/// Distills parsed facts into a slot (base records) or a patch (extensions).
fn place(facts: RecordFacts, arena_idx: u32, slot: &mut Slot, out: &mut TaskOut) {
    let mut flags = facts.flags;
    if facts.is_dir {
        flags.insert(EntryFlags::DIR);
    }
    flags = flags.union(record::reparse_entry_flags(facts.reparse_tag));

    let mut s = Slot {
        logical: facts.logical,
        alloc: facts.alloc,
        mtime: facts.mtime,
        flags,
        seq: facts.seq,
        link_names: facts.link_names.min(u16::MAX as u32) as u16,
        state: u8::from(facts.has_logical) * STATE_HAS_LOGICAL,
        ..Slot::EMPTY
    };
    if let Some(n) = facts.name {
        s.parent_ref = n.parent.0;
        s.rank = n.rank;
        s.name_arena = arena_idx;
        s.name_off = n.off;
        s.name_len = n.len;
    }

    if facts.base != 0 {
        out.patches.push((facts.base, s));
        return;
    }
    s.state |= STATE_BASE;
    if facts.is_dir {
        out.counts.dirs += 1;
    } else {
        out.counts.files += 1;
        out.counts.bytes += facts.logical;
    }
    *slot = s;
}

fn merge(target: &mut Slot, patch: &Slot) {
    if patch.has_logical() && !target.has_logical() {
        target.logical = patch.logical;
        target.state |= STATE_HAS_LOGICAL;
    }
    target.alloc = target.alloc.saturating_add(patch.alloc);
    target.link_names = target.link_names.saturating_add(patch.link_names);
    target.flags = target.flags.union(patch.flags);
    if patch.rank < target.rank {
        target.parent_ref = patch.parent_ref;
        target.rank = patch.rank;
        target.name_arena = patch.name_arena;
        target.name_off = patch.name_off;
        target.name_len = patch.name_len;
    }
    if target.mtime == 0 {
        target.mtime = patch.mtime;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::{RecordBuilder, image, root_dir};

    fn sweep_image(mut img: Vec<u8>, record_size: usize) -> (Table, ChunkCounts) {
        let total = img.len() / record_size;
        let mut sweep = Sweep::new(total, record_size);
        let counts = sweep.consume(0, &mut img);
        (sweep.finish(), counts)
    }

    #[test]
    fn sweep_places_base_records_and_counts() {
        let img = image(
            1024,
            vec![
                (5, root_dir()),
                (16, RecordBuilder::dir().name(5, 5, 1, "docs")),
                (
                    17,
                    RecordBuilder::file()
                        .name(16, 1, 1, "a.txt")
                        .data_nonresident(1000, 4096),
                ),
            ],
        );
        let (table, counts) = sweep_image(img, 1024);
        assert_eq!(counts.dirs, 2); // root + docs
        assert_eq!(counts.files, 1);
        assert_eq!(counts.bytes, 1000);
        let a = &table.records[17];
        assert_eq!(table.name(a), "a.txt");
        assert_eq!(a.logical, 1000);
        assert_eq!(a.alloc, 4096);
        assert_eq!(table.torn, 0);
    }

    #[test]
    fn extension_record_data_lands_on_base() {
        // A "big fragmented file": name in the base record, $DATA header in
        // an extension record (the $ATTRIBUTE_LIST shape, without parsing it).
        let img = image(
            1024,
            vec![
                (5, root_dir()),
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
            ],
        );
        let (table, _) = sweep_image(img, 1024);
        let big = &table.records[18];
        assert_eq!(table.name(big), "big.mkv");
        assert_eq!(big.logical, 2 << 30);
        assert_eq!(big.alloc, 2 << 30);
        assert_eq!(table.dropped_patches, 0);
    }

    #[test]
    fn chunked_consume_matches_single_shot() {
        let records = vec![
            (5, root_dir()),
            (16, RecordBuilder::dir().name(5, 5, 1, "sub")),
            (
                40,
                RecordBuilder::file()
                    .name(16, 1, 1, "tail.bin")
                    .data_nonresident(777, 4096),
            ),
        ];
        let img = image(1024, records);
        let total = img.len() / 1024;

        let (single, _) = sweep_image(img.clone(), 1024);

        let mut sweep = Sweep::new(total, 1024);
        let mut img2 = img;
        let split = 20 * 1024; // record boundary in the middle
        let (a, b) = img2.split_at_mut(split);
        sweep.consume(0, a);
        sweep.consume(20, b);
        let chunked = sweep.finish();

        for i in [5usize, 16, 40] {
            assert_eq!(chunked.records[i].logical, single.records[i].logical);
            assert_eq!(
                chunked.name(&chunked.records[i]),
                single.name(&single.records[i])
            );
        }
    }

    #[test]
    fn torn_records_are_counted_not_fatal() {
        let mut img = image(
            1024,
            vec![
                (5, root_dir()),
                (
                    16,
                    RecordBuilder::file()
                        .name(5, 5, 1, "fine.txt")
                        .data_nonresident(10, 4096),
                ),
                (
                    17,
                    RecordBuilder::file()
                        .name(5, 5, 1, "torn.txt")
                        .data_nonresident(10, 4096),
                ),
            ],
        );
        img[17 * 1024 + 1022] ^= 0xFF; // tear record 17's second sector
        let (table, counts) = sweep_image(img, 1024);
        assert_eq!(table.torn, 1);
        assert_eq!(counts.files, 1, "the healthy file still lands");
        assert!(!table.records[17].is_base());
    }

    #[test]
    fn patch_for_missing_base_is_dropped_and_counted() {
        let img = image(
            1024,
            vec![
                (5, root_dir()),
                (19, RecordBuilder::file().extension_of(42, 1)), // base 42 absent
            ],
        );
        let (table, _) = sweep_image(img, 1024);
        assert_eq!(table.dropped_patches, 1);
    }
}
