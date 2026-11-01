//! Squarified treemap layout (Bruls / Huizing / van Wijk, 2000).
//!
//! Rects are emitted parents-before-children: forward iteration is painter's
//! order for drawing, reverse is deepest-first for hit-testing. Children
//! below `min_area_px` are culled but still consume their share of space.

use crate::category::categorize;
use crate::entry::EntryFlags;
use crate::tree::{NodeId, Tree};

#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub w: f32,
    pub h: f32,
}

/// The deepest level any layout draws: tree depth is unbounded and `emit` recurses.
pub const MAX_LAYOUT_DEPTH: u8 = 24;

#[derive(Clone, Copy, Debug)]
pub struct TreemapOptions {
    pub min_area_px: f32,
    pub padding_px: f32,
    pub max_depth: u8,
    /// Omit SYSTEM entries and proportion tiles by visible bytes.
    pub hide_system: bool,
    /// Kept visible past `max_depth`: its ancestors draw open, and so does it if a directory.
    pub reveal: Option<NodeId>,
}

impl Default for TreemapOptions {
    fn default() -> Self {
        TreemapOptions {
            min_area_px: 1.0,
            padding_px: 1.0,
            max_depth: MAX_LAYOUT_DEPTH,
            hide_system: false,
            reveal: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreemapRect {
    pub id: NodeId,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub depth: u8,
    pub is_dir: bool,
    pub category: u8,
    /// A directory whose children were laid out; false for files.
    pub expanded: bool,
}

/// Lays out any directory subtree into `viewport`.
pub fn layout(
    tree: &Tree,
    root: NodeId,
    viewport: Viewport,
    opts: &TreemapOptions,
) -> Vec<TreemapRect> {
    layout_impl(tree, root, viewport, opts, None)
}

/// Layout under a view filter: per-node effective bytes (0 = omit), as
/// built by `search::build_overlay`. hide_system is already baked into the
/// overlay, so `opts.hide_system` is not consulted here.
pub fn layout_with_filter(
    tree: &Tree,
    root: NodeId,
    viewport: Viewport,
    opts: &TreemapOptions,
    bytes: &[u64],
) -> Vec<TreemapRect> {
    layout_impl(tree, root, viewport, opts, Some(bytes))
}

fn layout_impl(
    tree: &Tree,
    root: NodeId,
    viewport: Viewport,
    opts: &TreemapOptions,
    filter: Option<&[u64]>,
) -> Vec<TreemapRect> {
    let mut out = Vec::new();
    if tree.is_empty() || (root as usize) >= tree.len() || viewport.w <= 0.0 || viewport.h <= 0.0 {
        return out;
    }
    let frame = Frame {
        x: 0.0,
        y: 0.0,
        w: viewport.w as f64,
        h: viewport.h as f64,
    };
    let owned: Vec<u64>;
    let visible: Option<&[u64]> = match filter {
        Some(bytes) => Some(bytes),
        None if opts.hide_system => {
            let mut v = vec![0u64; tree.len()];
            fill_visible(tree, root, &mut v);
            owned = v;
            Some(&owned)
        }
        None => None,
    };
    let pass = Pass {
        tree,
        opts,
        visible,
        max_depth: opts.max_depth.min(MAX_LAYOUT_DEPTH),
        reveal_path: reveal_path(tree, root, opts.reveal),
    };
    pass.emit(root, frame, 0, &mut out);
    out
}

/// Root first, so index `d` is the node revealed at depth `d`. Empty unless
/// `reveal` is live and under `root`.
fn reveal_path(tree: &Tree, root: NodeId, reveal: Option<NodeId>) -> Vec<NodeId> {
    let Some(reveal) = reveal.filter(|&id| tree.is_live(id)) else {
        return Vec::new();
    };
    let mut path = vec![reveal];
    let mut cur = reveal;
    while cur != root {
        match tree.node(cur).parent() {
            Some(parent) => {
                path.push(parent);
                cur = parent;
            }
            None => return Vec::new(),
        }
    }
    path.reverse();
    path
}

/// Fills `visible` with subtree sizes that omit SYSTEM entries, for the
/// subtree under `root` only. SYSTEM subtrees are never visited, so `visible`
/// must arrive zeroed. Explicit stack: tree depth is unbounded.
fn fill_visible(tree: &Tree, root: NodeId, visible: &mut [u64]) {
    // (id, exiting): a dir is pushed twice — once to expand, once to fold its
    // finished total into its parent. Mirrors `search::build_overlay`.
    let mut stack = vec![(root, false)];
    while let Some((id, exiting)) = stack.pop() {
        let node = tree.node(id);
        if !exiting {
            if node.flags.contains(EntryFlags::SYSTEM) {
                continue;
            }
            if node.is_dir() {
                stack.push((id, true));
                stack.extend(tree.children(id).map(|c| (c, false)));
                continue;
            }
            visible[id as usize] = node.size;
        }
        // The traversal root's parent lies outside the laid-out subtree.
        if id != root
            && let Some(parent) = node.parent()
        {
            visible[parent as usize] =
                visible[parent as usize].saturating_add(visible[id as usize]);
        }
    }
}

fn effective_size(tree: &Tree, id: NodeId, visible: Option<&[u64]>) -> u64 {
    match visible {
        // Bounds-tolerant: a tree that grew past a filter's snapshot reads
        // as size 0 here rather than panicking.
        Some(v) => v.get(id as usize).copied().unwrap_or(0),
        None => tree.node(id).size,
    }
}

#[derive(Clone, Copy)]
struct Frame {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Frame {
    fn area(&self) -> f64 {
        self.w * self.h
    }

    fn inset(&self, pad: f64) -> Frame {
        Frame {
            x: self.x + pad,
            y: self.y + pad,
            w: self.w - 2.0 * pad,
            h: self.h - 2.0 * pad,
        }
    }
}

struct Pass<'a> {
    tree: &'a Tree,
    opts: &'a TreemapOptions,
    visible: Option<&'a [u64]>,
    max_depth: u8,
    reveal_path: Vec<NodeId>,
}

impl Pass<'_> {
    fn emit(&self, id: NodeId, frame: Frame, depth: u8, out: &mut Vec<TreemapRect>) {
        let node = self.tree.node(id);
        let index = out.len();
        out.push(TreemapRect {
            id,
            x: frame.x as f32,
            y: frame.y as f32,
            w: frame.w as f32,
            h: frame.h as f32,
            depth,
            is_dir: node.is_dir(),
            category: categorize(self.tree.name(id), node.is_dir()) as u8,
            expanded: false,
        });
        let limit = if self.reveal_path.get(depth as usize) == Some(&id) {
            MAX_LAYOUT_DEPTH
        } else {
            self.max_depth
        };
        if !node.is_dir() || depth >= limit {
            return;
        }
        let inner = frame.inset(self.opts.padding_px as f64);
        if inner.w <= 0.0 || inner.h <= 0.0 {
            return;
        }
        self.lay_children(id, inner, depth + 1, out);
        out[index].expanded = out.len() > index + 1;
    }

    fn lay_children(&self, dir: NodeId, frame: Frame, depth: u8, out: &mut Vec<TreemapRect>) {
        let mut items: Vec<(NodeId, u64)> = self
            .tree
            .children(dir)
            .map(|c| (c, effective_size(self.tree, c, self.visible)))
            .filter(|&(_, size)| size > 0)
            .collect();
        if items.is_empty() {
            return;
        }
        items.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

        let total: f64 = items.iter().map(|&(_, s)| s as f64).sum();
        let scale = frame.area() / total;

        let mut remaining = frame;
        let mut i = 0;
        while i < items.len() {
            if remaining.w <= 0.0 || remaining.h <= 0.0 {
                return;
            }
            let side = remaining.w.min(remaining.h);

            let first = items[i].1 as f64 * scale;
            let (mut sum, mut max, mut min) = (first, first, first);
            let mut worst = worst_aspect(sum, max, min, side);
            let mut j = i + 1;
            while j < items.len() {
                let a = items[j].1 as f64 * scale;
                let candidate = worst_aspect(sum + a, max.max(a), min.min(a), side);
                if candidate > worst {
                    break;
                }
                worst = candidate;
                sum += a;
                max = max.max(a);
                min = min.min(a);
                j += 1;
            }

            self.lay_row(&items[i..j], scale, sum, &mut remaining, depth, out);
            i = j;
        }
    }

    fn lay_row(
        &self,
        row: &[(NodeId, u64)],
        scale: f64,
        row_area: f64,
        remaining: &mut Frame,
        depth: u8,
        out: &mut Vec<TreemapRect>,
    ) {
        let horizontal = remaining.w < remaining.h; // row spans the full width
        let side = if horizontal { remaining.w } else { remaining.h };
        let thickness = (row_area / side).min(if horizontal { remaining.h } else { remaining.w });

        let mut offset = 0.0;
        for (k, &(id, size)) in row.iter().enumerate() {
            let len = if k == row.len() - 1 {
                side - offset
            } else {
                (size as f64 * scale) / thickness
            };
            let frame = if horizontal {
                Frame {
                    x: remaining.x + offset,
                    y: remaining.y,
                    w: len,
                    h: thickness,
                }
            } else {
                Frame {
                    x: remaining.x,
                    y: remaining.y + offset,
                    w: thickness,
                    h: len,
                }
            };
            offset += len;
            if frame.area() >= self.opts.min_area_px as f64 {
                self.emit(id, frame, depth, out);
            }
        }

        if horizontal {
            remaining.y += thickness;
            remaining.h -= thickness;
        } else {
            remaining.x += thickness;
            remaining.w -= thickness;
        }
    }
}

fn worst_aspect(sum: f64, max: f64, min: f64, side: f64) -> f64 {
    let s2 = sum * sum;
    let w2 = side * side;
    (w2 * max / s2).max(s2 / (w2 * min))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{EntryBatch, EntryFlags, FileEntry};
    use crate::tree::TreeBuilder;

    const DIR: EntryFlags = EntryFlags::DIR;
    const FILE: EntryFlags = EntryFlags(0);

    fn entry(id: u32, parent: u32, flags: EntryFlags, size: u64) -> FileEntry {
        FileEntry {
            path_id: id,
            parent_id: parent,
            name_off: 0,
            name_len: 0,
            flags,
            size,
            allocated_size: size,
            mtime: 0,
        }
    }

    fn flat_tree(files: &[(&str, u64)]) -> Tree {
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        for (i, &(name, size)) in files.iter().enumerate() {
            b.push(name, entry(i as u32 + 1, 0, FILE, size));
        }
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        builder.finish()
    }

    fn no_padding() -> TreemapOptions {
        TreemapOptions {
            min_area_px: 0.0,
            padding_px: 0.0,
            ..Default::default()
        }
    }

    fn rect_of(rects: &[TreemapRect], id: NodeId) -> TreemapRect {
        *rects.iter().find(|r| r.id == id).expect("rect missing")
    }

    fn area(r: &TreemapRect) -> f64 {
        r.w as f64 * r.h as f64
    }

    #[test]
    fn areas_are_proportional_to_sizes() {
        let tree = flat_tree(&[("a", 500), ("b", 300), ("c", 200)]);
        let rects = layout(&tree, 0, Viewport { w: 100.0, h: 100.0 }, &no_padding());

        assert!((area(&rect_of(&rects, 1)) - 5000.0).abs() < 1.0);
        assert!((area(&rect_of(&rects, 2)) - 3000.0).abs() < 1.0);
        assert!((area(&rect_of(&rects, 3)) - 2000.0).abs() < 1.0);
    }

    #[test]
    fn children_tile_the_parent_without_overlap() {
        let tree = flat_tree(&[
            ("a", 600),
            ("b", 600),
            ("c", 400),
            ("d", 300),
            ("e", 200),
            ("f", 200),
            ("g", 100),
        ]);
        let rects = layout(&tree, 0, Viewport { w: 600.0, h: 400.0 }, &no_padding());
        let leaves: Vec<_> = rects.iter().filter(|r| !r.is_dir).collect();

        let total: f64 = leaves.iter().map(|r| area(r)).sum();
        assert!((total - 240_000.0).abs() < 1.0, "leaves cover the viewport");

        for r in &leaves {
            assert!(r.x >= -0.01 && r.y >= -0.01);
            assert!(r.x as f64 + r.w as f64 <= 600.01);
            assert!(r.y as f64 + r.h as f64 <= 400.01);
        }
        for (i, a) in leaves.iter().enumerate() {
            for b in leaves.iter().skip(i + 1) {
                let x_overlap = (a.x + a.w).min(b.x + b.w) as f64 - a.x.max(b.x) as f64;
                let y_overlap = (a.y + a.h).min(b.y + b.h) as f64 - a.y.max(b.y) as f64;
                assert!(
                    x_overlap <= 0.01 || y_overlap <= 0.01,
                    "rects {} and {} overlap",
                    a.id,
                    b.id
                );
            }
        }
    }

    /// The canonical example from the squarified-treemap paper: sizes
    /// 6,6,4,3,2,2,1 in a 6×4 rectangle. Squarification keeps rects
    /// near-square (slice-and-dice would reach ratios up to 16); the paper's
    /// own layout of this example ends with the 1-unit item at 0.6×1.67 —
    /// aspect 25/9 ≈ 2.78 — so that is the exact expected worst case here.
    #[test]
    fn canonical_example_stays_near_square() {
        let tree = flat_tree(&[
            ("a", 6),
            ("b", 6),
            ("c", 4),
            ("d", 3),
            ("e", 2),
            ("f", 2),
            ("g", 1),
        ]);
        let rects = layout(&tree, 0, Viewport { w: 600.0, h: 400.0 }, &no_padding());

        let mut worst = 0.0f32;
        for r in rects.iter().filter(|r| !r.is_dir) {
            let aspect = (r.w / r.h).max(r.h / r.w);
            worst = worst.max(aspect);
        }
        assert!(
            (worst - 25.0 / 9.0).abs() < 0.01,
            "worst aspect {worst} differs from the paper's 25/9"
        );
    }

    #[test]
    fn parents_are_emitted_before_children_and_contain_them() {
        // root / dir(1, contains f1 900 + f2 100) + file(4, 1000)
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        b.push("dir", entry(1, 0, DIR, 0));
        b.push("f1", entry(2, 1, FILE, 900));
        b.push("f2", entry(3, 1, FILE, 100));
        b.push("big", entry(4, 0, FILE, 1000));
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        let tree = builder.finish();

        let rects = layout(&tree, 0, Viewport { w: 200.0, h: 100.0 }, &no_padding());

        let pos = |id: NodeId| rects.iter().position(|r| r.id == id).expect("missing");
        assert!(pos(0) < pos(1));
        assert!(pos(1) < pos(2));
        assert!(pos(1) < pos(3));

        let dir = rect_of(&rects, 1);
        for id in [2u32, 3] {
            let c = rect_of(&rects, id);
            assert!(c.x >= dir.x - 0.01 && c.y >= dir.y - 0.01);
            assert!(c.x + c.w <= dir.x + dir.w + 0.01);
            assert!(c.y + c.h <= dir.y + dir.h + 0.01);
            assert_eq!(c.depth, dir.depth + 1);
        }
        // dir and big split the root 50/50
        assert!((area(&dir) - 10_000.0).abs() < 1.0);
    }

    #[test]
    fn padding_insets_children_inside_their_directory() {
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        b.push("dir", entry(1, 0, DIR, 0));
        b.push("f", entry(2, 1, FILE, 100));
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        let tree = builder.finish();

        let opts = TreemapOptions {
            padding_px: 2.0,
            ..no_padding()
        };
        let rects = layout(&tree, 0, Viewport { w: 100.0, h: 100.0 }, &opts);

        let dir = rect_of(&rects, 1);
        let f = rect_of(&rects, 2);
        assert!((f.x - (dir.x + 2.0)).abs() < 0.01);
        assert!((f.y - (dir.y + 2.0)).abs() < 0.01);
        assert!((f.w - (dir.w - 4.0)).abs() < 0.01);
        assert!((f.h - (dir.h - 4.0)).abs() < 0.01);
    }

    #[test]
    fn tiny_children_are_culled_without_inflating_the_rest() {
        // 1,000,000 vs 1: in 100×100 the small file gets ~0.01 px² < 1 px².
        let tree = flat_tree(&[("big", 1_000_000), ("tiny", 1)]);
        let opts = TreemapOptions {
            min_area_px: 1.0,
            ..no_padding()
        };
        let rects = layout(&tree, 0, Viewport { w: 100.0, h: 100.0 }, &opts);

        assert!(rects.iter().all(|r| r.id != 2), "tiny rect must be culled");
        let big = rect_of(&rects, 1);
        // big keeps its proportional share; the sliver is just not drawn
        assert!((area(&big) - 10_000.0 * (1_000_000.0 / 1_000_001.0)).abs() < 1.0);
    }

    #[test]
    fn zero_size_children_emit_nothing_and_nothing_is_nan() {
        let tree = flat_tree(&[("empty", 0), ("real", 10)]);
        let rects = layout(&tree, 0, Viewport { w: 100.0, h: 100.0 }, &no_padding());

        assert!(rects.iter().all(|r| r.id != 1), "zero-size file skipped");
        for r in &rects {
            assert!(r.x.is_finite() && r.y.is_finite());
            assert!(r.w.is_finite() && r.h.is_finite());
        }
        assert!((area(&rect_of(&rects, 2)) - 10_000.0).abs() < 1.0);
    }

    #[test]
    fn max_depth_stops_recursion() {
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        b.push("dir", entry(1, 0, DIR, 0));
        b.push("f", entry(2, 1, FILE, 100));
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        let tree = builder.finish();

        let opts = TreemapOptions {
            max_depth: 1,
            ..no_padding()
        };
        let rects = layout(&tree, 0, Viewport { w: 100.0, h: 100.0 }, &opts);

        assert!(rects.iter().any(|r| r.id == 1), "depth-1 dir emitted");
        assert!(rects.iter().all(|r| r.id != 2), "depth-2 file not emitted");
        assert_eq!(area(&rect_of(&rects, 1)), 10_000.0);

        let drilled = layout(&tree, 1, Viewport { w: 100.0, h: 100.0 }, &opts);
        assert_eq!(rect_of(&drilled, 2).depth, 1);
        assert_eq!(area(&rect_of(&drilled, 2)), 10_000.0);
    }

    #[test]
    fn drill_down_layouts_from_a_subdirectory() {
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        b.push("dir", entry(1, 0, DIR, 0));
        b.push("f1", entry(2, 1, FILE, 300));
        b.push("f2", entry(3, 1, FILE, 100));
        b.push("elsewhere", entry(4, 0, FILE, 9999));
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        let tree = builder.finish();

        let rects = layout(&tree, 1, Viewport { w: 100.0, h: 100.0 }, &no_padding());

        assert_eq!(rects[0].id, 1, "drill root comes first at depth 0");
        assert_eq!(rects[0].depth, 0);
        assert!(rects.iter().all(|r| r.id != 4), "siblings of root excluded");
        // f1:f2 = 3:1 of the full viewport
        assert!((area(&rect_of(&rects, 2)) - 7500.0).abs() < 1.0);
        assert!((area(&rect_of(&rects, 3)) - 2500.0).abs() < 1.0);
    }

    fn hide_system_opts() -> TreemapOptions {
        TreemapOptions {
            hide_system: true,
            ..no_padding()
        }
    }

    /// A filter's byte array drives both areas and omissions.
    #[test]
    fn external_filter_bytes_drive_areas_and_omissions() {
        let tree = flat_tree(&[("a", 500), ("b", 300), ("c", 200)]);
        let bytes = vec![600u64, 500, 0, 100]; // root, a, b(filtered out), c
        let rects = layout_with_filter(
            &tree,
            0,
            Viewport { w: 100.0, h: 100.0 },
            &no_padding(),
            &bytes,
        );
        assert!(rects.iter().all(|r| r.id != 2), "zero-byte node omitted");
        assert!((area(&rect_of(&rects, 1)) - 10_000.0 * 5.0 / 6.0).abs() < 1.0);
        assert!((area(&rect_of(&rects, 3)) - 10_000.0 / 6.0).abs() < 1.0);
    }

    #[test]
    fn hide_system_omits_system_files_and_reproportions() {
        // root / a (non-system, 500) + sys (system, 500)
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        b.push("a", entry(1, 0, FILE, 500));
        b.push("sys", entry(2, 0, EntryFlags::SYSTEM, 500));
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        let tree = builder.finish();

        let vp = Viewport { w: 100.0, h: 100.0 };
        assert!(
            layout(&tree, 0, vp, &no_padding())
                .iter()
                .any(|r| r.id == 2)
        );

        let hidden = layout(&tree, 0, vp, &hide_system_opts());
        assert!(hidden.iter().all(|r| r.id != 2), "system file hidden");
        // "a" now fills the whole viewport instead of half.
        assert!((area(&rect_of(&hidden, 1)) - 10_000.0).abs() < 1.0);
    }

    #[test]
    fn visible_sizes_stay_zero_under_a_system_directory() {
        // root / sysdir(SYSTEM) { deep { g 9999 } } + keep 100
        let sys_dir = DIR.union(EntryFlags::SYSTEM);
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        b.push("sysdir", entry(1, 0, sys_dir, 0));
        b.push("deep", entry(2, 1, DIR, 0));
        b.push("g", entry(3, 2, FILE, 9999));
        b.push("keep", entry(4, 0, FILE, 100));
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        let tree = builder.finish();

        let mut visible = vec![0u64; tree.len()];
        fill_visible(&tree, 0, &mut visible);

        assert_eq!(visible[0], 100, "root counts only the non-system file");
        assert_eq!(visible[1], 0);
        assert_eq!(visible[2], 0, "descendants of a system dir stay zero");
        assert_eq!(visible[3], 0);
        assert_eq!(visible[4], 100);
    }

    #[test]
    fn visible_sizes_never_write_outside_the_traversal_subtree() {
        // root { a { f 70 } + b 900 }, filled from `a`
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        b.push("a", entry(1, 0, DIR, 0));
        b.push("f", entry(2, 1, FILE, 70));
        b.push("b", entry(3, 0, FILE, 900));
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        let tree = builder.finish();

        let mut visible = vec![0u64; tree.len()];
        fill_visible(&tree, 1, &mut visible);

        assert_eq!(visible[1], 70, "the traversal root gets its nested total");
        assert_eq!(visible[2], 70);
        assert_eq!(visible[0], 0, "the parent outside the subtree is untouched");
        assert_eq!(visible[3], 0);
    }

    #[test]
    fn a_file_as_traversal_root_keeps_its_own_size() {
        let tree = flat_tree(&[("a.txt", 42)]);

        let mut visible = vec![0u64; tree.len()];
        fill_visible(&tree, 1, &mut visible);

        assert_eq!(visible[1], 42);
        assert_eq!(visible[0], 0);
    }

    #[test]
    fn hide_system_hides_whole_system_subtree() {
        // root / dir { f 500 } + sysdir(SYSTEM) { g 9999 }
        let sys_dir = DIR.union(EntryFlags::SYSTEM);
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        b.push("dir", entry(1, 0, DIR, 0));
        b.push("f", entry(2, 1, FILE, 500));
        b.push("sysdir", entry(3, 0, sys_dir, 0));
        b.push("g", entry(4, 3, FILE, 9999));
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        let tree = builder.finish();

        let hidden = layout(
            &tree,
            0,
            Viewport { w: 100.0, h: 100.0 },
            &hide_system_opts(),
        );
        // The huge system subtree is gone entirely, not just visually blank.
        assert!(hidden.iter().all(|r| r.id != 3 && r.id != 4));
        assert!((area(&rect_of(&hidden, 1)) - 10_000.0).abs() < 1.0);
        assert!((area(&rect_of(&hidden, 2)) - 10_000.0).abs() < 1.0);
    }

    /// root { x { xa 300, y { ya 100 } }, z { za 400 }, w 200 }
    fn open_tree() -> Tree {
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        b.push("x", entry(1, 0, DIR, 0));
        b.push("xa", entry(2, 1, FILE, 300));
        b.push("y", entry(3, 1, DIR, 0));
        b.push("ya", entry(4, 3, FILE, 100));
        b.push("z", entry(5, 0, DIR, 0));
        b.push("za", entry(6, 5, FILE, 400));
        b.push("w", entry(7, 0, FILE, 200));
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        builder.finish()
    }

    fn capped(max_depth: u8, reveal: Option<NodeId>) -> TreemapOptions {
        TreemapOptions {
            max_depth,
            reveal,
            ..no_padding()
        }
    }

    const VIEW: Viewport = Viewport { w: 100.0, h: 100.0 };

    fn geometry(rects: &[TreemapRect]) -> Vec<(NodeId, [u32; 4], u8)> {
        rects
            .iter()
            .map(|r| (r.id, [r.x, r.y, r.w, r.h].map(f32::to_bits), r.depth))
            .collect()
    }

    #[test]
    fn revealing_a_folder_adds_its_children_and_moves_nothing() {
        let tree = open_tree();
        let closed = layout(&tree, 0, VIEW, &capped(1, None));
        let opened = layout(&tree, 0, VIEW, &capped(1, Some(1)));

        let before = geometry(&closed);
        let after = geometry(&opened);
        for g in &before {
            assert!(after.contains(g), "rect {} moved or vanished", g.0);
        }
        let mut added: Vec<_> = after.iter().filter(|g| !before.contains(g)).collect();
        added.sort_by_key(|g| g.0);
        assert_eq!(added.len(), 2);
        assert_eq!((added[0].0, added[0].2), (2, 2));
        assert_eq!((added[1].0, added[1].2), (3, 2));
        assert!(!rect_of(&closed, 1).expanded);
        assert!(rect_of(&opened, 1).expanded);
    }

    #[test]
    fn revealing_a_nested_folder_keeps_its_parent_open() {
        let tree = open_tree();
        let rects = layout(&tree, 0, VIEW, &capped(1, Some(3)));

        assert!(rects.iter().any(|r| r.id == 2), "x stays open");
        assert!(rects.iter().any(|r| r.id == 4), "y opens");
        assert!(rects.iter().all(|r| r.id != 6), "z stays closed");
    }

    #[test]
    fn revealing_a_file_opens_the_folders_above_it() {
        let tree = open_tree();
        let rects = layout(&tree, 0, VIEW, &capped(1, Some(4)));

        assert!(rects.iter().any(|r| r.id == 4), "ya drawn");
        assert!(rect_of(&rects, 1).expanded);
        assert!(rect_of(&rects, 3).expanded);
        assert!(!rect_of(&rects, 5).expanded);
    }

    #[test]
    fn a_reveal_that_is_not_hidden_under_the_view_changes_nothing() {
        let tree = open_tree();
        let closed = layout(&tree, 1, VIEW, &capped(1, None));

        assert_eq!(
            layout(&tree, 1, VIEW, &capped(1, Some(5))),
            closed,
            "other branch"
        );
        assert_eq!(
            layout(&tree, 1, VIEW, &capped(1, Some(999))),
            closed,
            "dead id"
        );
        assert_eq!(
            layout(&tree, 1, VIEW, &capped(1, Some(2))),
            closed,
            "visible file"
        );
        assert_eq!(
            layout(&tree, 1, VIEW, &capped(1, Some(1))),
            closed,
            "the root"
        );
        let wide = layout(&tree, 0, VIEW, &capped(2, None));
        assert_eq!(
            layout(&tree, 0, VIEW, &capped(2, Some(1))),
            wide,
            "already open"
        );
    }

    #[test]
    fn only_folders_with_children_drawn_are_expanded() {
        let tree = open_tree();
        let rects = layout(&tree, 0, VIEW, &capped(1, None));

        assert!(rect_of(&rects, 0).expanded, "root");
        assert!(!rect_of(&rects, 1).expanded, "x at the cap");
        assert!(!rect_of(&rects, 7).expanded, "file");
    }

    #[test]
    fn nothing_is_drawn_past_the_hard_depth_limit() {
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        for id in 1..40 {
            b.push("d", entry(id, id - 1, DIR, 0));
        }
        b.push("f", entry(40, 39, FILE, 1));
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        let tree = builder.finish();

        let deepest_closed = layout(&tree, 0, VIEW, &capped(32, None))
            .iter()
            .map(|r| r.depth)
            .max();
        let deepest_open = layout(&tree, 0, VIEW, &capped(1, Some(39)))
            .iter()
            .map(|r| r.depth)
            .max();

        assert_eq!(deepest_closed, Some(MAX_LAYOUT_DEPTH));
        assert_eq!(deepest_open, Some(MAX_LAYOUT_DEPTH));
    }

    #[test]
    fn an_open_folder_shows_only_what_the_filter_keeps() {
        let tree = open_tree();
        // root, x, xa, y(out), ya(out), z, za, w
        let bytes = vec![900u64, 300, 300, 0, 0, 400, 400, 200];
        let rects = layout_with_filter(&tree, 0, VIEW, &capped(1, Some(1)), &bytes);

        assert!(
            rects.iter().all(|r| r.id != 3),
            "filtered-out folder omitted"
        );
        assert_eq!(area(&rect_of(&rects, 2)), area(&rect_of(&rects, 1)));
    }
}
