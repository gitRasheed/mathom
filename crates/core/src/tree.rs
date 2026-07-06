//! Arena tree with streaming aggregation.
//!
//! Nodes live in one `Vec<Node>` indexed by the scanner-assigned `path_id`,
//! names are interned, children form intrusive singly-linked lists. Directory
//! aggregates (size/allocated/items) update incrementally as batches arrive,
//! so the tree is queryable mid-scan for live UI snapshots.

use crate::entry::{EntryBatch, EntryFlags};
use crate::interner::{NameInterner, NameRef};

pub type NodeId = u32;

const NONE: u32 = u32::MAX;

/// 48 bytes. Directories carry aggregates in `size`/`allocated`/`items`;
/// files carry their own size and `items == 0`.
/// (`name_off`/`name_len` are inlined rather than a `NameRef` field so they
/// pack with `flags` into one word.)
#[derive(Clone, Copy, Debug)]
pub struct Node {
    name_off: u32,
    name_len: u16,
    pub flags: EntryFlags,
    parent: u32,
    first_child: u32,
    next_sibling: u32,
    /// Directories: total descendant count (files + dirs). Files: 0.
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

/// Totals freed by `Tree::remove_subtree`, for updating running counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Removed {
    pub size: u64,
    pub allocated: u64,
    pub files: u64,
    pub dirs: u64,
}

impl Removed {
    /// Arena nodes detached (files + dirs).
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

    pub fn name(&self, id: NodeId) -> &str {
        self.names.get(self.nodes[id as usize].name_ref())
    }

    pub fn children(&self, id: NodeId) -> ChildIter<'_> {
        ChildIter {
            tree: self,
            next: self.nodes[id as usize].first_child,
        }
    }

    /// Rebuilds the full path by walking parents up to the root. The root's
    /// own name is the scan root path as given to the scanner.
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

    /// Removes the subtree rooted at `id`: unlinks it from its parent, subtracts
    /// its aggregates from every ancestor, and vacates its arena slots. Mirrors
    /// a filesystem delete so the tree stays consistent without a rescan.
    ///
    /// Returns the freed totals, or `None` if `id` is out of range, the root,
    /// or already detached. Interned names are not reclaimed (append-only
    /// buffer); a handful of leaked names per delete is negligible.
    pub fn remove_subtree(&mut self, id: NodeId) -> Option<Removed> {
        let idx = id as usize;
        if idx >= self.nodes.len() {
            return None;
        }
        let parent = self.nodes[idx].parent;
        // Root and already-detached nodes both have parent == NONE.
        if parent == NONE {
            return None;
        }

        // Unlink `id` from the parent's singly-linked child list. Capture the
        // successor before detach_subtree vacates the node.
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

        // Subtract the freed aggregates from the parent up to the root.
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

    /// Walks the subtree rooted at `id`, tallying files/dirs/bytes and marking
    /// every node vacant. Iterative (explicit stack) to bound stack depth on
    /// deep trees. `size`/`allocated` sum leaf values only, since a directory's
    /// aggregate is exactly the sum of its leaves.
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
///
/// Contract with the scanner: an entry's parent was emitted in an earlier
/// batch (or earlier in the same batch); the root is entry 0 with
/// `parent_id == 0`. Sibling batches may arrive in any order.
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

    /// Marks a directory as unreadable (its children never arrive).
    pub fn mark_error(&mut self, id: NodeId) {
        if let Some(n) = self.tree.nodes.get_mut(id as usize) {
            n.flags.insert(EntryFlags::ERROR);
        }
    }

    /// Removes a node's subtree from the live tree after a filesystem delete.
    /// Delegates to [`Tree::remove_subtree`]; returns the freed totals.
    pub fn remove(&mut self, id: NodeId) -> Option<Removed> {
        self.tree.remove_subtree(id)
    }

    /// Live view for mid-scan snapshots.
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// Cancelled scans may leave vacant slots (ids allocated by the scanner
    /// whose batches never arrived); those are unreachable from the root and
    /// harmless.
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

    /// root(0) / a(1) / sub(3), files: f1(2, 100B) in a, f2(4, 7B) in sub, f3(5, 1B) in root
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
        // Two sibling dirs emitted together, their child batches arrive
        // in reverse id order — allowed.
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
        // Freed total equals the dir's own aggregate before removal.
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

        // Root drops the whole subtree: 108-107 bytes, 5-4 items.
        assert_eq!(tree.node(0).size, 1);
        assert_eq!(tree.node(0).items, 1);
        assert_eq!(child_ids(&tree, 0), vec![5]);

        // The detached node is vacated.
        assert!(tree.node(1).parent().is_none());
        assert!(!tree.node(1).is_dir());
    }

    #[test]
    fn remove_leaf_updates_ancestors_and_relinks_head() {
        let mut tree = sample_tree();
        // f3 is the head of root's child list; removing it relinks first_child.
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

        // Both a and root shrink; a's child list loses "sub".
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
        // Nothing changed.
        assert_eq!(tree.node(0).size, 108);
        assert_eq!(tree.node(0).items, 5);
    }

    #[test]
    fn remove_twice_does_not_double_subtract() {
        let mut tree = sample_tree();
        assert!(tree.remove_subtree(3).is_some());
        let root_size = tree.node(0).size;
        // The slot is now detached; a second remove is a no-op.
        assert_eq!(tree.remove_subtree(3), None);
        assert_eq!(tree.node(0).size, root_size);
    }
}
