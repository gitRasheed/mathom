//! Arena tree with streaming aggregation.

use crate::entry::{EntryBatch, EntryFlags};
use crate::interner::{NameInterner, NameRef};

pub type NodeId = u32;

const NONE: u32 = u32::MAX;

/// 48 bytes; directories carry aggregates, files carry their own size.
#[derive(Clone, Copy, Debug)]
pub struct Node {
    name_off: u32,
    name_len: u16,
    pub flags: EntryFlags,
    parent: u32,
    first_child: u32,
    next_sibling: u32,
    /// Directories: descendant count. Files: 0.
    pub items: u32,
    pub size: u64,
    pub allocated: u64,
    pub mtime: i64,
}

impl Node {
    const VACANT: Node = Node {
        name_off: 0,
        name_len: 0,
        flags: EntryFlags(0),
        parent: NONE,
        first_child: NONE,
        next_sibling: NONE,
        items: 0,
        size: 0,
        allocated: 0,
        mtime: 0,
    };

    pub fn is_dir(&self) -> bool {
        self.flags.is_dir()
    }

    pub fn parent(&self) -> Option<NodeId> {
        (self.parent != NONE).then_some(self.parent)
    }

    fn name_ref(&self) -> NameRef {
        NameRef {
            off: self.name_off,
            len: self.name_len,
        }
    }

    fn is_vacant(&self) -> bool {
        self.parent == NONE && self.first_child == NONE && self.next_sibling == NONE
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Removed {
    pub size: u64,
    pub allocated: u64,
    pub files: u64,
    pub dirs: u64,
}

impl Removed {
    pub fn nodes(&self) -> u64 {
        self.files + self.dirs
    }
}

#[derive(Default)]
pub struct Tree {
    nodes: Vec<Node>,
    names: NameInterner,
}

impl Tree {
    pub const ROOT: NodeId = 0;

    fn new() -> Self {
        Tree {
            nodes: Vec::new(),
            names: NameInterner::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    /// True for nodes currently linked into the tree; stale UI ids fail.
    /// (Not `is_vacant`: a childless root has the same all-NONE links.)
    pub fn is_live(&self, id: NodeId) -> bool {
        match self.nodes.get(id as usize) {
            Some(n) => id == Self::ROOT || n.parent != NONE,
            None => false,
        }
    }

    pub fn name(&self, id: NodeId) -> &str {
        self.names.get(self.nodes[id as usize].name_ref())
    }

    pub fn children(&self, id: NodeId) -> ChildIter<'_> {
        ChildIter {
            tree: self,
            next: self.nodes[id as usize].first_child,
        }
    }

    pub fn path(&self, mut id: NodeId) -> String {
        let mut parts = vec![self.name(id)];
        while let Some(p) = self.node(id).parent() {
            parts.push(self.name(p));
            id = p;
        }
        let mut out = String::new();
        for (i, part) in parts.iter().rev().enumerate() {
            if i > 0 && !out.ends_with(std::path::MAIN_SEPARATOR) {
                out.push(std::path::MAIN_SEPARATOR);
            }
            out.push_str(part);
        }
        out
    }

    pub fn name_bytes_used(&self) -> usize {
        self.names.bytes_used()
    }

    /// Removes a subtree after a filesystem delete and returns freed totals.
    pub fn remove_subtree(&mut self, id: NodeId) -> Option<Removed> {
        let idx = id as usize;
        if idx >= self.nodes.len() {
            return None;
        }
        let parent = self.nodes[idx].parent;
        if parent == NONE {
            return None;
        }

        let next = self.nodes[idx].next_sibling;
        if self.nodes[parent as usize].first_child == id {
            self.nodes[parent as usize].first_child = next;
        } else {
            let mut prev = self.nodes[parent as usize].first_child;
            while prev != NONE {
                let sib = self.nodes[prev as usize].next_sibling;
                if sib == id {
                    self.nodes[prev as usize].next_sibling = next;
                    break;
                }
                prev = sib;
            }
        }

        let removed = self.detach_subtree(id);

        let node_count = removed.nodes() as u32;
        let mut anc = parent;
        loop {
            let n = &mut self.nodes[anc as usize];
            n.size -= removed.size;
            n.allocated -= removed.allocated;
            n.items -= node_count;
            if n.parent == NONE {
                break;
            }
            anc = n.parent;
        }
        Some(removed)
    }

    /// Iteratively vacates the subtree and tallies leaf bytes.
    fn detach_subtree(&mut self, id: NodeId) -> Removed {
        let mut removed = Removed::default();
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            let n = self.nodes[cur as usize];
            let mut child = n.first_child;
            while child != NONE {
                stack.push(child);
                child = self.nodes[child as usize].next_sibling;
            }
            if n.is_dir() {
                removed.dirs += 1;
            } else {
                removed.files += 1;
                removed.size += n.size;
                removed.allocated += n.allocated;
            }
            self.nodes[cur as usize] = Node::VACANT;
        }
        removed
    }
}

pub struct ChildIter<'a> {
    tree: &'a Tree,
    next: u32,
}

impl Iterator for ChildIter<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        if self.next == NONE {
            return None;
        }
        let id = self.next;
        self.next = self.tree.nodes[id as usize].next_sibling;
        Some(id)
    }
}

/// Consumes scanner batches and maintains the tree incrementally.
/// Scanner contract: a parent entry is emitted before its children (root is
/// entry 0 with `parent_id == 0`); sibling batches may arrive in any order.
#[derive(Default)]
pub struct TreeBuilder {
    tree: Tree,
}

impl TreeBuilder {
    pub fn new() -> Self {
        TreeBuilder { tree: Tree::new() }
    }

    pub fn add_batch(&mut self, batch: &EntryBatch) {
        for entry in &batch.entries {
            let id = entry.path_id as usize;
            let name = self.tree.names.intern(batch.name_of(entry));

            if id >= self.tree.nodes.len() {
                self.tree.nodes.resize(id + 1, Node::VACANT);
            }
            debug_assert!(
                self.tree.nodes[id].is_vacant() || entry.path_id == 0,
                "entry id {id} emitted twice"
            );

            let is_root = entry.path_id == 0;
            let parent = if is_root { NONE } else { entry.parent_id };
            if !is_root {
                let p = &mut self.tree.nodes[entry.parent_id as usize];
                assert!(
                    p.flags.is_dir(),
                    "parent {} of entry {id} not yet emitted or not a dir",
                    entry.parent_id
                );
                let prev_first = p.first_child;
                p.first_child = entry.path_id;
                self.tree.nodes[id].next_sibling = prev_first;
            }

            let n = &mut self.tree.nodes[id];
            n.name_off = name.off;
            n.name_len = name.len;
            n.flags = entry.flags;
            n.parent = parent;
            n.size = entry.size;
            n.allocated = entry.allocated_size;
            n.mtime = entry.mtime;

            if !is_root {
                self.propagate(entry.parent_id, entry.size, entry.allocated_size);
            }
        }
    }

    pub fn mark_error(&mut self, id: NodeId) {
        if let Some(n) = self.tree.nodes.get_mut(id as usize) {
            n.flags.insert(EntryFlags::ERROR);
        }
    }

    /// Detaches the insertion-only lookup while keeping names readable.
    pub fn release_name_index(&mut self) -> impl Send + use<> {
        self.tree.names.release_index()
    }

    pub fn remove(&mut self, id: NodeId) -> Option<Removed> {
        self.tree.remove_subtree(id)
    }

    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    pub fn finish(self) -> Tree {
        self.tree
    }

    fn propagate(&mut self, mut id: u32, size: u64, allocated: u64) {
        loop {
            let n = &mut self.tree.nodes[id as usize];
            n.size += size;
            n.allocated += allocated;
            n.items += 1;
            if n.parent == NONE {
                break;
            }
            id = n.parent;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{EntryBatch, EntryFlags, FileEntry};

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

    fn batch(items: &[(&str, FileEntry)]) -> EntryBatch {
        let mut b = EntryBatch::default();
        for (name, e) in items {
            b.push(name, *e);
        }
        b
    }

    const DIR: EntryFlags = EntryFlags::DIR;
    const FILE: EntryFlags = EntryFlags(0);

    /// root(0) / a(1) / sub(3); files: f1(2)=100 in a, f2(4)=7 in sub, f3(5)=1 in root
    fn build_sample(batch_order: &[&[(&str, FileEntry)]]) -> Tree {
        let mut builder = TreeBuilder::new();
        for items in batch_order {
            builder.add_batch(&batch(items));
        }
        builder.finish()
    }

    fn sample_batches() -> Vec<Vec<(&'static str, FileEntry)>> {
        vec![
            vec![("root", entry(0, 0, DIR, 0))],
            vec![("a", entry(1, 0, DIR, 0)), ("f3", entry(5, 0, FILE, 1))],
            vec![("f1", entry(2, 1, FILE, 100)), ("sub", entry(3, 1, DIR, 0))],
            vec![("f2", entry(4, 3, FILE, 7))],
        ]
    }

    #[test]
    fn aggregates_propagate_to_all_ancestors() {
        let batches = sample_batches();
        let refs: Vec<&[(&str, FileEntry)]> = batches.iter().map(|b| b.as_slice()).collect();
        let tree = build_sample(&refs);

        assert_eq!(tree.len(), 6);
        assert_eq!(tree.node(0).size, 108);
        assert_eq!(tree.node(0).items, 5);
        assert_eq!(tree.node(1).size, 107);
        assert_eq!(tree.node(1).items, 3);
        assert_eq!(tree.node(3).size, 7);
        assert_eq!(tree.node(3).items, 1);
        assert_eq!(tree.node(2).size, 100);
        assert_eq!(tree.node(2).items, 0);
    }

    #[test]
    fn sibling_batches_may_arrive_in_any_order() {
        let mut builder = TreeBuilder::new();
        builder.add_batch(&batch(&[("root", entry(0, 0, DIR, 0))]));
        builder.add_batch(&batch(&[
            ("a", entry(1, 0, DIR, 0)),
            ("b", entry(2, 0, DIR, 0)),
        ]));
        builder.add_batch(&batch(&[("fb", entry(4, 2, FILE, 5))]));
        builder.add_batch(&batch(&[("fa", entry(3, 1, FILE, 9))]));
        let tree = builder.finish();

        assert_eq!(tree.node(0).size, 14);
        assert_eq!(tree.node(0).items, 4);
        assert_eq!(tree.node(1).size, 9);
        assert_eq!(tree.node(2).size, 5);
    }

    #[test]
    fn children_iterates_all_direct_children() {
        let batches = sample_batches();
        let refs: Vec<&[(&str, FileEntry)]> = batches.iter().map(|b| b.as_slice()).collect();
        let tree = build_sample(&refs);

        let mut kids: Vec<&str> = tree.children(0).map(|c| tree.name(c)).collect();
        kids.sort();
        assert_eq!(kids, ["a", "f3"]);

        let sub_kids: Vec<&str> = tree.children(3).map(|c| tree.name(c)).collect();
        assert_eq!(sub_kids, ["f2"]);
    }

    #[test]
    fn path_walks_up_to_root() {
        let batches = sample_batches();
        let refs: Vec<&[(&str, FileEntry)]> = batches.iter().map(|b| b.as_slice()).collect();
        let tree = build_sample(&refs);

        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(tree.path(4), format!("root{sep}a{sep}sub{sep}f2"));
        assert_eq!(tree.path(0), "root");
    }

    #[test]
    fn mark_error_sets_flag() {
        let mut builder = TreeBuilder::new();
        builder.add_batch(&batch(&[("root", entry(0, 0, DIR, 0))]));
        builder.add_batch(&batch(&[("locked", entry(1, 0, DIR, 0))]));
        builder.mark_error(1);
        let tree = builder.finish();
        assert!(tree.node(1).flags.contains(EntryFlags::ERROR));
        assert!(tree.node(1).is_dir());
    }

    #[test]
    fn node_struct_stays_48_bytes() {
        assert_eq!(std::mem::size_of::<Node>(), 48);
    }

    fn sample_tree() -> Tree {
        let batches = sample_batches();
        let refs: Vec<&[(&str, FileEntry)]> = batches.iter().map(|b| b.as_slice()).collect();
        build_sample(&refs)
    }

    fn child_ids(tree: &Tree, id: NodeId) -> Vec<NodeId> {
        tree.children(id).collect()
    }

    #[test]
    fn remove_subtree_frees_dir_and_updates_ancestors() {
        let mut tree = sample_tree();
        assert_eq!(tree.node(1).size, 107);

        let removed = tree.remove_subtree(1).unwrap();
        assert_eq!(
            removed,
            Removed {
                size: 107,
                allocated: 107,
                files: 2,
                dirs: 2,
            }
        );
        assert_eq!(removed.nodes(), 4);

        assert_eq!(tree.node(0).size, 1);
        assert_eq!(tree.node(0).items, 1);
        assert_eq!(child_ids(&tree, 0), vec![5]);

        assert!(tree.node(1).parent().is_none());
        assert!(!tree.node(1).is_dir());
    }

    #[test]
    fn remove_leaf_updates_ancestors_and_relinks_head() {
        let mut tree = sample_tree();
        let removed = tree.remove_subtree(5).unwrap();
        assert_eq!(
            removed,
            Removed {
                size: 1,
                allocated: 1,
                files: 1,
                dirs: 0,
            }
        );
        assert_eq!(tree.node(0).size, 107);
        assert_eq!(tree.node(0).items, 4);
        assert_eq!(child_ids(&tree, 0), vec![1]);
    }

    #[test]
    fn remove_middle_dir_updates_every_ancestor() {
        let mut tree = sample_tree();
        let removed = tree.remove_subtree(3).unwrap();
        assert_eq!(removed.nodes(), 2);
        assert_eq!(removed.size, 7);

        assert_eq!(tree.node(1).size, 100);
        assert_eq!(tree.node(1).items, 1);
        assert_eq!(tree.node(0).size, 101);
        assert_eq!(tree.node(0).items, 3);
        assert_eq!(child_ids(&tree, 1), vec![2]);
    }

    #[test]
    fn remove_rejects_root_and_unknown_ids() {
        let mut tree = sample_tree();
        assert_eq!(tree.remove_subtree(0), None);
        assert_eq!(tree.remove_subtree(99), None);
        assert_eq!(tree.node(0).size, 108);
        assert_eq!(tree.node(0).items, 5);
    }

    #[test]
    fn remove_twice_does_not_double_subtract() {
        let mut tree = sample_tree();
        assert!(tree.remove_subtree(3).is_some());
        let root_size = tree.node(0).size;
        assert_eq!(tree.remove_subtree(3), None);
        assert_eq!(tree.node(0).size, root_size);
    }

    #[test]
    fn is_live_rejects_removed_subtree_and_out_of_range() {
        let mut tree = sample_tree();
        tree.remove_subtree(1).unwrap();

        assert!(!tree.is_live(1));
        assert!(!tree.is_live(2));
        assert!(!tree.is_live(3));
        assert!(!tree.is_live(99));
        assert!(tree.is_live(0));
        assert!(tree.is_live(5));
    }

    #[test]
    fn is_live_accepts_childless_root() {
        let mut builder = TreeBuilder::new();
        builder.add_batch(&batch(&[("root", entry(0, 0, DIR, 0))]));
        let tree = builder.finish();
        assert!(tree.is_live(0));
        assert!(!tree.is_live(1));
    }

    // The delete-during-scan hazard: remove() vacates slots that in-flight
    // batches may still name as parents, so the app refuses deletes while a
    // scan is running. This pins the panic the gate prevents.
    #[test]
    #[should_panic(expected = "not yet emitted or not a dir")]
    fn add_batch_panics_when_parent_was_removed_mid_stream() {
        let mut builder = TreeBuilder::new();
        builder.add_batch(&batch(&[("root", entry(0, 0, DIR, 0))]));
        builder.add_batch(&batch(&[("a", entry(1, 0, DIR, 0))]));
        builder.remove(1);
        builder.add_batch(&batch(&[("late", entry(2, 1, FILE, 5))]));
    }

    #[test]
    fn is_live_rejects_never_filled_gap_slots() {
        let mut builder = TreeBuilder::new();
        builder.add_batch(&batch(&[("root", entry(0, 0, DIR, 0))]));
        builder.add_batch(&batch(&[("f", entry(2, 0, FILE, 5))]));
        let tree = builder.finish();
        assert!(!tree.is_live(1));
        assert!(tree.is_live(2));
    }
}
