//! Squarified treemap layout (Bruls / Huizing / van Wijk, 2000).
//!
//! Pure math over the arena tree so it stays portable and unit-testable; the
//! Tauri layer ships the rect list to a canvas renderer. Rects are emitted
//! depth-first with every parent before its children: forward iteration is
//! painter's order for drawing, reverse iteration is deepest-first for
//! hit-testing. Children below `min_area_px` are culled but still consume
//! their share of space, so dense regions of tiny files read as the parent
//! directory's color.

use crate::category::categorize;
use crate::tree::{NodeId, Tree};

#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct TreemapOptions {
    /// Rects with area below this many px² are culled.
    pub min_area_px: f32,
    /// Inset applied inside a directory rect before laying out its children.
    pub padding_px: f32,
    /// Maximum nesting depth (root = 0).
    pub max_depth: u8,
}

impl Default for TreemapOptions {
    fn default() -> Self {
        TreemapOptions {
            min_area_px: 1.0,
            padding_px: 1.0,
            max_depth: 32,
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
}

/// Lays out the subtree under `root` into `viewport`. `root` may be any
/// directory (drill-down = re-layout from that directory).
///
/// Space is normalized by the sum of the children's sizes, not the parent's
/// aggregate, so the rect always fills completely (once directories carry
/// own-size from the MFT backend, those bytes are simply not visible as a
/// tile of their own).
pub fn layout(
    tree: &Tree,
    root: NodeId,
    viewport: Viewport,
    opts: &TreemapOptions,
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
    emit(tree, root, frame, 0, opts, &mut out);
    out
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

fn emit(
    tree: &Tree,
    id: NodeId,
    frame: Frame,
    depth: u8,
    opts: &TreemapOptions,
    out: &mut Vec<TreemapRect>,
) {
    let node = tree.node(id);
    out.push(TreemapRect {
        id,
        x: frame.x as f32,
        y: frame.y as f32,
        w: frame.w as f32,
        h: frame.h as f32,
        depth,
        is_dir: node.is_dir(),
        category: categorize(tree.name(id), node.is_dir()) as u8,
    });
    if !node.is_dir() || depth >= opts.max_depth {
        return;
    }
    let inner = frame.inset(opts.padding_px as f64);
    if inner.w <= 0.0 || inner.h <= 0.0 {
        return;
    }
    lay_children(tree, id, inner, depth + 1, opts, out);
}

fn lay_children(
    tree: &Tree,
    dir: NodeId,
    frame: Frame,
    depth: u8,
    opts: &TreemapOptions,
    out: &mut Vec<TreemapRect>,
) {
    let mut items: Vec<(NodeId, u64)> = tree
        .children(dir)
        .map(|c| (c, tree.node(c).size))
        .filter(|&(_, size)| size > 0)
        .collect();
    if items.is_empty() {
        return;
    }
    items.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let total: f64 = items.iter().map(|&(_, s)| s as f64).sum();
    let scale = frame.area() / total;
    let min_area = opts.min_area_px as f64;

    let mut remaining = frame;
    let mut i = 0;
    while i < items.len() {
        if remaining.w <= 0.0 || remaining.h <= 0.0 {
            return;
        }
        let side = remaining.w.min(remaining.h);

        // Greedy row: keep adding items while the worst aspect ratio improves.
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

        lay_row(
            tree,
            &items[i..j],
            scale,
            sum,
            &mut remaining,
            depth,
            min_area,
            opts,
            out,
        );
        i = j;
    }
}

/// Worst aspect ratio of a row with total area `sum`, extreme item areas
/// `max`/`min`, laid along a side of length `side` (Bruls et al., eq. in §4).
fn worst_aspect(sum: f64, max: f64, min: f64, side: f64) -> f64 {
    let s2 = sum * sum;
    let w2 = side * side;
    (w2 * max / s2).max(s2 / (w2 * min))
}

/// Lays one row along the shorter side of `remaining` and shrinks it.
#[allow(clippy::too_many_arguments)]
fn lay_row(
    tree: &Tree,
    row: &[(NodeId, u64)],
    scale: f64,
    row_area: f64,
    remaining: &mut Frame,
    depth: u8,
    min_area: f64,
    opts: &TreemapOptions,
    out: &mut Vec<TreemapRect>,
) {
    let horizontal = remaining.w < remaining.h; // row spans the full width
    let side = if horizontal { remaining.w } else { remaining.h };
    // Clamp so the final row can never overshoot the frame on float error.
    let thickness = (row_area / side).min(if horizontal { remaining.h } else { remaining.w });

    let mut offset = 0.0;
    for (k, &(id, size)) in row.iter().enumerate() {
        // Snap the last item to the far edge: cumulative float drift must
        // not leave hairline gaps or overshoot.
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
        if frame.area() >= min_area {
            emit(tree, id, frame, depth, opts, out);
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

    /// Flat tree: root (id 0) + one file per (name, size).
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
            max_depth: 32,
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
            min_area_px: 0.0,
            padding_px: 2.0,
            max_depth: 32,
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
            padding_px: 0.0,
            max_depth: 32,
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
            min_area_px: 0.0,
            padding_px: 0.0,
            max_depth: 1,
        };
        let rects = layout(&tree, 0, Viewport { w: 100.0, h: 100.0 }, &opts);

        assert!(rects.iter().any(|r| r.id == 1), "depth-1 dir emitted");
        assert!(rects.iter().all(|r| r.id != 2), "depth-2 file not emitted");
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
}
