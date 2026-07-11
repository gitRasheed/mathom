//! Scan-wide search over the arena tree. A linear walk over ~2M nodes is
//! a few milliseconds — deliberately no index.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::category::extension_key;
use crate::entry::EntryFlags;
use crate::tree::{NodeId, Tree};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchQuery {
    pub name_terms: Vec<String>,
    /// Lowercased extension without the dot.
    pub ext: Option<String>,
    pub min_size: u64,
}

impl SearchQuery {
    /// Tokens: name terms, `ext:pdf`, and `>100mb`/`>=1.5gb` size filters.
    pub fn parse(text: &str) -> SearchQuery {
        let mut q = SearchQuery::default();
        for token in text.split_whitespace() {
            if let Some(ext) = token.strip_prefix("ext:") {
                q.ext = Some(ext.trim_start_matches('.').to_lowercase());
            } else if let Some(bytes) = parse_min_size(token) {
                q.min_size = bytes;
            } else {
                q.name_terms.push(token.to_lowercase());
            }
        }
        q
    }

    pub fn is_empty(&self) -> bool {
        self.name_terms.is_empty() && self.ext.is_none() && self.min_size == 0
    }
}

#[derive(Clone, Debug, Default)]
pub struct SearchResults {
    /// The largest matches, size descending, at most `cap`.
    pub ids: Vec<NodeId>,
    /// Every match in the tree, not just the returned ones.
    pub total_matches: u64,
}

/// Searches the whole tree; the scan root itself is never a hit.
pub fn search(tree: &Tree, query: &SearchQuery, cap: usize, hide_system: bool) -> SearchResults {
    if query.is_empty() || tree.is_empty() {
        return SearchResults::default();
    }
    let mut heap: BinaryHeap<Reverse<(u64, NodeId)>> = BinaryHeap::with_capacity(cap + 1);
    let mut total = 0u64;
    let mut stack = vec![Tree::ROOT];
    while let Some(id) = stack.pop() {
        let node = tree.node(id);
        if hide_system && id != Tree::ROOT && node.flags.contains(EntryFlags::SYSTEM) {
            continue;
        }
        if node.is_dir() {
            stack.extend(tree.children(id));
        }
        if id != Tree::ROOT && matches(tree, id, query) {
            total += 1;
            heap.push(Reverse((node.size, id)));
            if heap.len() > cap {
                heap.pop();
            }
        }
    }
    let mut out: Vec<(u64, NodeId)> = heap.into_iter().map(|r| r.0).collect();
    out.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    SearchResults {
        ids: out.into_iter().map(|(_, id)| id).collect(),
        total_matches: total,
    }
}

/// Per-node view-filter overlay: the tree view, treemap, and type panel
/// read it the way they read hide_system. A match is inherited downward —
/// a matching directory includes its whole subtree.
pub struct FilterOverlay {
    /// Node matches, inherits a match, or contains one.
    pub visible: Vec<bool>,
    /// Matching file bytes under each node.
    pub bytes: Vec<u64>,
}

impl FilterOverlay {
    /// Bounds-tolerant: ids past the built length (a tree that grew after
    /// the overlay snapshot) read as filtered-out, never a panic.
    pub fn is_visible(&self, id: NodeId) -> bool {
        self.visible.get(id as usize).copied().unwrap_or(false)
    }

    pub fn bytes_of(&self, id: NodeId) -> u64 {
        self.bytes.get(id as usize).copied().unwrap_or(0)
    }
}

/// One O(N) pass. hide_system composes: system subtrees neither match nor
/// contribute bytes, even inside a matching directory.
pub fn build_overlay(tree: &Tree, query: &SearchQuery, hide_system: bool) -> FilterOverlay {
    let mut visible = vec![false; tree.len()];
    let mut bytes = vec![0u64; tree.len()];
    if tree.is_empty() || query.is_empty() {
        return FilterOverlay { visible, bytes };
    }
    // (id, inherited, exiting): dirs get an exit marker so their subtree
    // totals fold into the parent post-order; files fold immediately.
    let mut stack: Vec<(NodeId, bool, bool)> = vec![(Tree::ROOT, false, false)];
    while let Some((id, inherited, exiting)) = stack.pop() {
        let node = tree.node(id);
        if exiting {
            if let Some(p) = node.parent() {
                bytes[p as usize] += bytes[id as usize];
                if visible[id as usize] {
                    visible[p as usize] = true;
                }
            }
            continue;
        }
        if hide_system && id != Tree::ROOT && node.flags.contains(EntryFlags::SYSTEM) {
            continue;
        }
        let matched = inherited || (id != Tree::ROOT && matches(tree, id, query));
        if node.is_dir() {
            visible[id as usize] = matched;
            stack.push((id, matched, true));
            for c in tree.children(id) {
                stack.push((c, matched, false));
            }
        } else if matched {
            visible[id as usize] = true;
            bytes[id as usize] = node.size;
            if let Some(p) = node.parent() {
                bytes[p as usize] += node.size;
                visible[p as usize] = true;
            }
        }
    }
    FilterOverlay { visible, bytes }
}

fn matches(tree: &Tree, id: NodeId, q: &SearchQuery) -> bool {
    let node = tree.node(id);
    if node.size < q.min_size {
        return false;
    }
    if let Some(ext) = &q.ext {
        if node.is_dir() {
            return false;
        }
        match extension_key(tree.name(id)) {
            Some(key) if key.as_str() == ext => {}
            _ => return false,
        }
    }
    let name = tree.name(id);
    q.name_terms.iter().all(|t| contains_ci(name, t))
}

/// ASCII-case-insensitive substring; non-ASCII bytes match exactly.
fn contains_ci(haystack: &str, needle: &str) -> bool {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    if n.is_empty() {
        return true;
    }
    n.len() <= h.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

/// `>100mb` / `>=1.5gb` / `>4096` → bytes; `None` if not a size filter.
fn parse_min_size(token: &str) -> Option<u64> {
    let rest = token
        .strip_prefix(">=")
        .or_else(|| token.strip_prefix('>'))?;
    let unit_at = rest
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(rest.len());
    let (num, unit) = rest.split_at(unit_at);
    let value: f64 = num.parse().ok()?;
    let mult: u64 = match unit.to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" => 1 << 10,
        "m" | "mb" => 1 << 20,
        "g" | "gb" => 1 << 30,
        "t" | "tb" => 1 << 40,
        _ => return None,
    };
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    Some((value * mult as f64) as u64)
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

    /// root(0)
    /// ├─ docs(1)/          report.pdf(2)=100, Draft-REPORT.txt(3)=50, notes(4)=8
    /// ├─ media(5)/         movie.mkv(6)=500, clip.mp4(7)=200
    /// ├─ sys(8)/ SYSTEM    pagefile.sys(9)=999
    /// └─ archive.pdf(10)/  big.bin(11)=300
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
            entry(8, 0, EntryFlags::DIR.union(EntryFlags::SYSTEM), 0),
        );
        batch.push("archive.pdf", entry(10, 0, DIR, 0));
        b.add_batch(&batch);
        let mut batch = EntryBatch::default();
        batch.push("report.pdf", entry(2, 1, FILE, 100));
        batch.push("Draft-REPORT.txt", entry(3, 1, FILE, 50));
        batch.push("notes", entry(4, 1, FILE, 8));
        batch.push("movie.mkv", entry(6, 5, FILE, 500));
        batch.push("clip.mp4", entry(7, 5, FILE, 200));
        batch.push("pagefile.sys", entry(9, 8, FILE, 999));
        batch.push("big.bin", entry(11, 10, FILE, 300));
        b.add_batch(&batch);
        b.finish()
    }

    #[test]
    fn parse_splits_terms_ext_and_min_size() {
        let q = SearchQuery::parse("Report ext:.PDF >100mb");
        assert_eq!(q.name_terms, ["report"]);
        assert_eq!(q.ext.as_deref(), Some("pdf"));
        assert_eq!(q.min_size, 100 * 1024 * 1024);
    }

    #[test]
    fn parse_size_units_and_fallback() {
        assert_eq!(SearchQuery::parse(">150").min_size, 150);
        assert_eq!(SearchQuery::parse(">=1.5kb").min_size, 1536);
        assert_eq!(SearchQuery::parse(">2g").min_size, 2 * 1024 * 1024 * 1024);
        // Not a size → a name term, never an error.
        let q = SearchQuery::parse(">abc");
        assert_eq!(q.min_size, 0);
        assert_eq!(q.name_terms, [">abc"]);
    }

    #[test]
    fn empty_query_matches_nothing() {
        let tree = sample();
        assert!(SearchQuery::parse("   ").is_empty());
        let r = search(&tree, &SearchQuery::parse(""), 10, false);
        assert!(r.ids.is_empty());
        assert_eq!(r.total_matches, 0);
    }

    #[test]
    fn name_match_is_case_insensitive_over_files_and_dirs() {
        let tree = sample();
        let r = search(&tree, &SearchQuery::parse("report"), 10, false);
        assert_eq!(r.ids, [2, 3]); // report.pdf(100) before Draft-REPORT.txt(50)
        assert_eq!(r.total_matches, 2);
        assert_eq!(
            search(&tree, &SearchQuery::parse("docs"), 10, false).ids,
            [1]
        );
    }

    #[test]
    fn multiple_terms_all_must_match() {
        let tree = sample();
        let r = search(&tree, &SearchQuery::parse("draft report"), 10, false);
        assert_eq!(r.ids, [3]);
    }

    #[test]
    fn ext_filter_matches_files_only() {
        let tree = sample();
        let r = search(&tree, &SearchQuery::parse("ext:pdf"), 10, false);
        assert_eq!(r.ids, [2]); // archive.pdf is a directory — excluded
    }

    #[test]
    fn min_size_includes_directories_but_never_the_root() {
        let tree = sample();
        let r = search(&tree, &SearchQuery::parse(">400"), 10, false);
        // sys(999), pagefile.sys(999), media(700), movie.mkv(500);
        // root (2157) is not a hit.
        assert_eq!(r.ids, [8, 9, 5, 6]);
    }

    #[test]
    fn hide_system_prunes_system_subtrees() {
        let tree = sample();
        let r = search(&tree, &SearchQuery::parse(">400"), 10, true);
        assert_eq!(r.ids, [5, 6]);
        assert_eq!(r.total_matches, 2);
    }

    #[test]
    fn cap_bounds_results_but_not_the_count() {
        let tree = sample();
        let r = search(&tree, &SearchQuery::parse(">400"), 2, false);
        assert_eq!(r.ids, [8, 9]);
        assert_eq!(r.total_matches, 4);
    }

    #[test]
    fn combined_filters_intersect() {
        let tree = sample();
        let r = search(&tree, &SearchQuery::parse("report ext:pdf >60"), 10, false);
        assert_eq!(r.ids, [2]);
    }

    #[test]
    fn overlay_marks_matches_their_ancestors_and_bytes() {
        let tree = sample();
        let o = build_overlay(&tree, &SearchQuery::parse("ext:pdf"), false);
        // report.pdf(2)=100 is the only match; archive.pdf(10) is a dir.
        let visible: Vec<u32> = (0..tree.len() as u32)
            .filter(|&i| o.is_visible(i))
            .collect();
        assert_eq!(visible, [0, 1, 2], "match + ancestors only");
        assert_eq!(o.bytes_of(0), 100);
        assert_eq!(o.bytes_of(1), 100);
        assert_eq!(o.bytes_of(2), 100);
        assert_eq!(o.bytes_of(10), 0);
    }

    /// A matching directory includes its whole subtree at full size.
    #[test]
    fn overlay_dir_match_is_inherited_by_the_subtree() {
        let tree = sample();
        let o = build_overlay(&tree, &SearchQuery::parse("media"), false);
        assert!(o.is_visible(5) && o.is_visible(6) && o.is_visible(7));
        assert!(!o.is_visible(1) && !o.is_visible(8));
        assert_eq!(o.bytes_of(5), 700);
        assert_eq!(o.bytes_of(6), 500);
        assert_eq!(o.bytes_of(7), 200);
        assert_eq!(o.bytes_of(0), 700);
    }

    /// hide_system wins over a match: the system subtree contributes
    /// nothing even though pagefile.sys(999) satisfies `>400`.
    #[test]
    fn overlay_composes_hide_system() {
        let tree = sample();
        let o = build_overlay(&tree, &SearchQuery::parse(">400"), true);
        assert!(!o.is_visible(8) && !o.is_visible(9));
        assert_eq!(o.bytes_of(8), 0);
        // media(700) matches as a dir and pulls in both files.
        assert_eq!(o.bytes_of(5), 700);
        assert_eq!(o.bytes_of(0), 700);
    }

    /// Ids past the overlay's snapshot read as filtered-out, not a panic.
    #[test]
    fn overlay_reads_are_bounds_tolerant() {
        let tree = sample();
        let o = build_overlay(&tree, &SearchQuery::parse("media"), false);
        assert!(!o.is_visible(9999));
        assert_eq!(o.bytes_of(9999), 0);
    }
}
