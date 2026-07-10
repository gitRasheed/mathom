//! Subtree statistics for the detail panel.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use crate::category::{Category, ExtKey, categorize_ext, extension_key};
use crate::entry::EntryFlags;
use crate::tree::{NodeId, Tree};

#[derive(Clone, Copy, Debug)]
pub struct TypeStat {
    pub ext: Option<ExtKey>,
    pub category: Category,
    pub bytes: u64,
    pub files: u64,
}

#[derive(Clone, Debug, Default)]
pub struct TypeBreakdown {
    /// Every extension in the subtree, largest first.
    pub types: Vec<TypeStat>,
    pub total_bytes: u64,
    pub total_files: u64,
}

pub fn type_breakdown(tree: &Tree, root: NodeId, hide_system: bool) -> TypeBreakdown {
    let mut groups: HashMap<Option<ExtKey>, (u64, u64)> = HashMap::new();
    for_each_file(tree, root, hide_system, |id, size| {
        let g = groups.entry(extension_key(tree.name(id))).or_default();
        g.0 += size;
        g.1 += 1;
    });

    let mut all: Vec<TypeStat> = groups
        .into_iter()
        .map(|(ext, (bytes, files))| TypeStat {
            ext,
            category: match &ext {
                Some(key) => categorize_ext(key),
                None => Category::Other,
            },
            bytes,
            files,
        })
        .collect();
    all.sort_by(|a, b| {
        b.bytes
            .cmp(&a.bytes)
            .then_with(|| ext_str(a).cmp(ext_str(b)))
    });

    let total_bytes = all.iter().map(|t| t.bytes).sum();
    let total_files = all.iter().map(|t| t.files).sum();
    TypeBreakdown {
        types: all,
        total_bytes,
        total_files,
    }
}

pub fn largest_files(tree: &Tree, root: NodeId, n: usize, hide_system: bool) -> Vec<NodeId> {
    if n == 0 {
        return Vec::new();
    }
    let mut heap: BinaryHeap<Reverse<(u64, NodeId)>> = BinaryHeap::with_capacity(n + 1);
    for_each_file(tree, root, hide_system, |id, size| {
        heap.push(Reverse((size, id)));
        if heap.len() > n {
            heap.pop();
        }
    });
    let mut out: Vec<(u64, NodeId)> = heap.into_iter().map(|r| r.0).collect();
    out.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    out.into_iter().map(|(_, id)| id).collect()
}

fn ext_str(t: &TypeStat) -> &str {
    t.ext.as_ref().map_or("", |k| k.as_str())
}

fn for_each_file(tree: &Tree, root: NodeId, hide_system: bool, mut f: impl FnMut(NodeId, u64)) {
    if (root as usize) >= tree.len() {
        return; // ids come from the IPC boundary; unknown roots yield nothing
    }
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let node = tree.node(id);
        if hide_system && id != root && node.flags.contains(EntryFlags::SYSTEM) {
            continue;
        }
        if node.is_dir() {
            stack.extend(tree.children(id));
        } else {
            f(id, node.size);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{EntryBatch, FileEntry};
    use crate::tree::TreeBuilder;

    fn entry(id: u32, parent: u32, flags: EntryFlags, size: u64) -> FileEntry {
        FileEntry {
            path_id: id,
            parent_id: parent,
            name_off: 0,
            name_len: 0,
            flags,
            size,
            allocated_size: size,
            mtime: 1_700_000_000,
        }
    }

    const DIR: EntryFlags = EntryFlags::DIR;
    const FILE: EntryFlags = EntryFlags(0);

    /// root(0)/docs(1): a.pdf(2)=100, b.PDF(3)=50, notes(4)=8;
    /// media(5): movie.mkv(6)=500; sys(7)[SYSTEM]: pagefile.sys(8)=999; raw.bin(9)=30
    fn sample() -> Tree {
        let mut b = TreeBuilder::new();
        let mut batch = EntryBatch::default();
        batch.push("root", entry(0, 0, DIR, 0));
        b.add_batch(&batch);
        let mut batch = EntryBatch::default();
        batch.push("docs", entry(1, 0, DIR, 0));
        batch.push("media", entry(5, 0, DIR, 0));
        batch.push(
            "sys",
            entry(7, 0, EntryFlags::DIR.union(EntryFlags::SYSTEM), 0),
        );
        batch.push("raw.bin", entry(9, 0, FILE, 30));
        b.add_batch(&batch);
        let mut batch = EntryBatch::default();
        batch.push("a.pdf", entry(2, 1, FILE, 100));
        batch.push("b.PDF", entry(3, 1, FILE, 50));
        batch.push("notes", entry(4, 1, FILE, 8));
        batch.push("movie.mkv", entry(6, 5, FILE, 500));
        batch.push("pagefile.sys", entry(8, 7, FILE, 999));
        b.add_batch(&batch);
        b.finish()
    }

    fn ext_of(t: &TypeStat) -> &str {
        ext_str(t)
    }

    #[test]
    fn breakdown_groups_by_lowercased_extension_sorted_by_bytes() {
        let tree = sample();
        let bd = type_breakdown(&tree, 0, false);
        let got: Vec<(&str, u64, u64)> = bd
            .types
            .iter()
            .map(|t| (ext_of(t), t.bytes, t.files))
            .collect();
        assert_eq!(
            got,
            [
                ("sys", 999, 1),
                ("mkv", 500, 1),
                ("pdf", 150, 2), // a.pdf + b.PDF: case-insensitive group
                ("bin", 30, 1),
                ("", 8, 1), // "notes" — the no-extension bucket
            ]
        );
        assert_eq!(bd.total_bytes, 1687);
        assert_eq!(bd.total_files, 6);
    }

    #[test]
    fn every_extension_is_listed_and_sums_reconcile_with_totals() {
        let tree = sample();
        let bd = type_breakdown(&tree, 0, false);
        assert_eq!(bd.types.len(), 5); // sys, mkv, pdf, bin, no-extension
        assert_eq!(
            bd.types.iter().map(|t| t.bytes).sum::<u64>(),
            bd.total_bytes
        );
        assert_eq!(
            bd.types.iter().map(|t| t.files).sum::<u64>(),
            bd.total_files
        );
    }

    #[test]
    fn breakdown_categories_match_the_treemap_palette() {
        let tree = sample();
        let bd = type_breakdown(&tree, 0, false);
        let cat = |ext: &str| bd.types.iter().find(|t| ext_of(t) == ext).unwrap().category;
        assert_eq!(cat("mkv"), Category::Video);
        assert_eq!(cat("pdf"), Category::Document);
        assert_eq!(cat("sys"), Category::System);
        assert_eq!(cat(""), Category::Other);
    }

    #[test]
    fn breakdown_is_scoped_to_the_subtree() {
        let tree = sample();
        let bd = type_breakdown(&tree, 1, false); // docs only
        let got: Vec<(&str, u64)> = bd.types.iter().map(|t| (ext_of(t), t.bytes)).collect();
        assert_eq!(got, [("pdf", 150), ("", 8)]);
        assert_eq!(bd.total_bytes, 158);
    }

    #[test]
    fn hide_system_prunes_system_subtrees() {
        let tree = sample();
        let bd = type_breakdown(&tree, 0, true);
        assert!(bd.types.iter().all(|t| ext_of(t) != "sys"));
        assert_eq!(bd.total_bytes, 688); // 1687 - pagefile.sys(999)
        assert_eq!(bd.total_files, 5);
    }

    #[test]
    fn largest_files_are_size_descending_and_scoped() {
        let tree = sample();
        assert_eq!(largest_files(&tree, 0, 3, false), [8, 6, 2]);
        assert_eq!(largest_files(&tree, 0, 3, true), [6, 2, 3]);
        assert_eq!(largest_files(&tree, 1, 2, false), [2, 3]);
        assert_eq!(largest_files(&tree, 0, 0, false), [] as [NodeId; 0]);
    }

    #[test]
    fn unknown_root_yields_empty_results() {
        let tree = sample();
        let bd = type_breakdown(&tree, 999, false);
        assert!(bd.types.is_empty());
        assert_eq!(bd.total_files, 0);
        assert_eq!(largest_files(&tree, 999, 5, false), [] as [NodeId; 0]);
    }
}
