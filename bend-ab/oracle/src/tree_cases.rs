//! A seeded directory tree, generated identically here and in tree.bend,
//! fed through mathom's real TreeBuilder, search, overlay and stats.

use std::fmt::Write as _;
use std::fs;

use mathom_core::entry::{EntryBatch, EntryFlags, FileEntry};
use mathom_core::search::{SearchQuery, build_overlay, search};
use mathom_core::stats::{largest_files, type_breakdown};
use mathom_core::tree::{Tree, TreeBuilder};

pub const BASES: &[&str] = &[
    "report",
    "IMG_",
    "photo",
    "Draft-REPORT",
    "notes",
    "data",
    "kernel32",
    "setup",
    "movie",
    "backup",
    "résumé",
    "日本",
    "a b",
    "x",
];
pub const EXTS: &[&str] = &[
    "pdf",
    "PDF",
    "mkv",
    "mp4",
    "jpg",
    "JPEG",
    "txt",
    "rs",
    "dll",
    "sys",
    "exe",
    "bin",
    "log",
    "",
    "gz",
    "weird1234",
    "Ω",
    "tar",
];
pub const QUERIES: &[&str] = &[
    "report",
    "ext:pdf",
    ">400000",
    "draft report",
    "ext:mkv,.MP4 >100000",
    "ext:sys",
    "IMG",
    "résumé",
    ">0",
    "日本 ext:txt",
    "dir1",
    "ext:gz,bin,log",
    ">=2mb",
    ">1kb img",
    ">abc",
    "  ext:..JPEG   photo  ",
    ">4tb",
];

pub fn h(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

struct Gen {
    batch: EntryBatch,
    next: u32,
    huge: bool,
}

impl Gen {
    fn node(&mut self, seed: u32, depth: u32, parent: u32, is_root: bool) {
        let id = self.next;
        self.next += 1;
        let r = h(seed);
        let thresh = if depth > 4 { 3 } else { 1 };
        let dir = is_root || (depth > 0 && r % 8 < thresh);
        let sys = !is_root && h(seed ^ 0x5bd1_e995).is_multiple_of(16);
        let mut flags = if dir { EntryFlags::DIR } else { EntryFlags(0) };
        if sys {
            flags = flags.union(EntryFlags::SYSTEM);
        }
        if dir {
            let name = format!("dir{}", r % 1000);
            self.batch.push(&name, entry(id, parent, flags, 0, 0));
            let nk = h(seed.wrapping_add(1)) % 13;
            for i in 0..nk {
                let kid = h(seed ^ h(i.wrapping_add(0x9e37_79b9)));
                self.node(kid, depth.saturating_sub(1), id, false);
            }
        } else {
            let s = h(seed.wrapping_add(2));
            let kind = h(seed.wrapping_add(3)) % 64;
            let size: u64 = if kind == 0 && self.huge {
                ((h(seed.wrapping_add(4)) as u64) << 32) | s as u64
            } else if kind < 4 {
                0
            } else {
                (s % 1_048_576) as u64
            };
            let alloc = size.saturating_add(4095) & !4095;
            let base = BASES[(h(seed.wrapping_add(5)) % BASES.len() as u32) as usize];
            let ext = EXTS[(h(seed.wrapping_add(6)) % EXTS.len() as u32) as usize];
            let mut name = format!("{base}{}", r % 1000);
            if !ext.is_empty() {
                name.push('.');
                name.push_str(ext);
            }
            self.batch
                .push(&name, entry(id, parent, flags, size, alloc));
        }
    }
}

fn entry(id: u32, parent: u32, flags: EntryFlags, size: u64, alloc: u64) -> FileEntry {
    FileEntry {
        path_id: id,
        parent_id: parent,
        name_off: 0,
        name_len: 0,
        flags,
        size,
        allocated_size: alloc,
        mtime: 0,
    }
}

pub fn build(seed: u32, depth: u32, huge: bool) -> Tree {
    let mut g = Gen {
        batch: EntryBatch::default(),
        next: 0,
        huge,
    };
    g.node(seed, depth, 0, true);
    let mut b = TreeBuilder::new();
    b.add_batch(&g.batch);
    b.finish()
}

fn mix64(acc: u32, v: u64) -> u32 {
    h(acc ^ h((v as u32) ^ h((v >> 32) as u32)))
}

pub fn report(tree: &Tree) -> String {
    let mut o = String::new();
    let n = tree.len() as u32;
    let root = tree.node(0);
    let _ = writeln!(o, "nodes {n}");
    let _ = writeln!(
        o,
        "root {:016x} {:016x} {}",
        root.size, root.allocated, root.items
    );
    let mut sum = 0u32;
    for id in 0..n {
        let nd = tree.node(id);
        sum = sum.wrapping_add(mix64(mix64(h(id ^ h(nd.items)), nd.size), nd.allocated));
    }
    let _ = writeln!(o, "aggsum {sum:08x}");
    for hide in [false, true] {
        let bd = type_breakdown(tree, 0, hide, None);
        let _ = writeln!(
            o,
            "bd{} {:016x} {}",
            hide as u8, bd.total_bytes, bd.total_files
        );
        for t in &bd.types {
            let ext = t.ext.as_ref().map_or("", |k| k.as_str());
            let _ = writeln!(
                o,
                " {ext}|{:016x} {} {}",
                t.bytes, t.files, t.category as u8
            );
        }
        let top = largest_files(tree, 0, 20, hide, None);
        let _ = writeln!(o, "top{} {:?}", hide as u8, top);
    }
    for (k, q) in QUERIES.iter().enumerate() {
        let query = SearchQuery::parse(q);
        for hide in [false, true] {
            let r = search(tree, &query, 50, hide);
            let _ = writeln!(o, "q{k}.{} {} {:?}", hide as u8, r.total_matches, r.ids);
            let ov = build_overlay(tree, &query, hide);
            let mut s = 0u32;
            for id in 0..n {
                let (b, v) = (ov.bytes[id as usize], ov.visible[id as usize]);
                if v || b != 0 {
                    s = s.wrapping_add(h(id ^ h((b as u32) ^ h(((b >> 32) as u32) ^ v as u32))));
                }
            }
            let _ = writeln!(o, "ov{k}.{} {s:08x}", hide as u8);
        }
    }
    o
}

pub fn emit(seed: usize, depth: usize, huge: bool, out: &str) {
    let tree = build(seed as u32, depth as u32, huge);
    fs::create_dir_all(out).unwrap();
    fs::write(format!("{out}/expected.txt"), report(&tree)).unwrap();
    let qs: Vec<String> = QUERIES
        .iter()
        .map(|q| crate::cat_cases::bend_str(q))
        .collect();
    let bases: Vec<String> = BASES
        .iter()
        .map(|q| crate::cat_cases::bend_str(q))
        .collect();
    let exts: Vec<String> = EXTS.iter().map(|q| crate::cat_cases::bend_str(q)).collect();
    let src = format!(
        "import Base\n\ndef seed() -> U32:\n  {seed}\n\ndef depth() -> Nat:\n  {depth}n\n\ndef huge() -> Bool:\n  {}\n\n\
         def queries() -> List<&2, String>:\n  [{}]\n\n\
         def bases() -> List<&2, String>:\n  [{}]\n\n\
         def exts() -> List<&2, String>:\n  [{}]\n",
        if huge { "True{}" } else { "False{}" },
        qs.join(", "),
        bases.join(", "),
        exts.join(", ")
    );
    fs::write(format!("{out}/cases.bend"), src).unwrap();
}
