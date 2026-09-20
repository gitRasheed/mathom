//! Squarified treemap layout (Bruls / Huizing / van Wijk, 2000).
//!
//! Rects are emitted parents-before-children: forward iteration is painter's
//! order for drawing, reverse is deepest-first for hit-testing.
//!
//! `TreemapOptions::min_side_px` is a floor, not a cut-off line. A child too
//! small to earn its proportional share is raised to one minimum block and
//! drawn anyway, so a directory tiles edge to edge instead of opening holes
//! nobody can attribute to anything. What still does not fit at that size is
//! dropped — smallest first — and the survivors re-normalize into the space,
//! so dropping leaves no hole either.
//!
//! How deep that goes is decided by the pixels, not by a number: a directory
//! subdivides only while its interior can hold a legible child, its biggest
//! child would land well clear of the floor, and it has room to give each
//! child a block worth looking at. Without the last two, folders nest into
//! folders of specks and the map ends up a mesh nothing can be read from.
//!
//! A directory's children get its frame whole — no inset, at any depth. What
//! separates two blocks is the renderer's 1px and nothing else, so the seams
//! are the same width however the nesting falls.

use crate::category::categorize;
use crate::entry::EntryFlags;
use crate::tree::{NodeId, Tree};

#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct TreemapOptions {
    /// Legibility floor, in pixels: a child is sized to be at least this wide
    /// *and* this tall. A child whose proportional share falls short is raised
    /// to one minimum block rather than culled, so the parent has no hole in
    /// it; an area test alone would let a 40×0.1 sliver through and read as
    /// blank. Children that no longer fit even at this size are dropped,
    /// smallest first, and the rest spread into their space. Zero lays the
    /// children out strictly proportionally.
    pub min_side_px: f32,
    /// Hard ceiling on how deep the layout goes. The only brake on a tree
    /// whose depth is unbounded, so it holds in both modes below — and it is
    /// deliberately not lifted by `layout_with_force`.
    pub max_depth: u8,
    /// Whether a level is taken because the pixels say it is worth taking, or
    /// simply because `max_depth` has not been reached yet.
    ///
    /// True is what the map does by default: a directory stops when nothing
    /// inside it would be legible, whatever the cap says. False is the
    /// original behaviour — subdivide until the frame runs out — kept because
    /// a fixed depth is a legitimate thing to want from a treemap, and it is
    /// what the Depth setting's All/1/2/3 choose.
    pub adaptive_depth: bool,
    /// Omit SYSTEM entries and proportion tiles by visible bytes.
    pub hide_system: bool,
}

/// How much clear of the floor a directory's *biggest* child has to land
/// before the directory is worth subdividing at all — as a multiple of
/// `min_side_px`, measured on a side.
///
/// Without this a directory subdivides as long as it can fit one minimum
/// block, so folding folders end up as folders-of-specks-of-specks: every
/// level eats padding and cuts the last one finer, and what you get is a
/// field of blocks too small to read or to point at. A folder whose best
/// child would still be a speck says more as one plate.
const DETAIL_FACTOR: f64 = 2.0;

/// The smallest block a directory is willing to *average* when it subdivides.
///
/// The legibility gate asks whether the biggest child is worth a block. This
/// one asks how thinly the whole set is spread — which is what a folder of
/// three hundred small files is — and the answer depends on how much room the
/// folder has, not on a count: a small folder showing twenty things and a
/// large one showing thirty are the same number and not the same picture. So
/// the limit is a capacity derived from the folder's own area, and a directory
/// that cannot give each child this much folds into one plate instead.
///
/// Counting visible children, not descendants: what the layout has to place at
/// this level is its own children, and a folder of three folders holding a
/// thousand files each is three blocks here, not a thousand. Clicking a plate
/// is the ask that overrides all of this.
const COMFORT_PX: f64 = 32.0;

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

/// Lays out any directory subtree into `viewport`.
pub fn layout(
    tree: &Tree,
    root: NodeId,
    viewport: Viewport,
    opts: &TreemapOptions,
) -> Vec<TreemapRect> {
    layout_impl(tree, root, viewport, opts, None, &[])
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
    layout_impl(tree, root, viewport, opts, Some(bytes), &[])
}

/// Layout with a directory forced open — the one the user asked to see inside
/// of, whatever the legibility rules made of it.
///
/// `force_open` names the *deepest* directory to open, and the chain from it
/// up to `root` opens with it. One value therefore carries the whole
/// accordion: asking for a directory outside the open one leaves the old one
/// off the chain (so it closes), and asking for one inside keeps it on (so it
/// stays). An id that is not in this subtree, or no longer names a live
/// directory, is not an error — it simply opens nothing.
pub fn layout_with_force(
    tree: &Tree,
    root: NodeId,
    viewport: Viewport,
    opts: &TreemapOptions,
    filter: Option<&[u64]>,
    force_open: Option<NodeId>,
) -> Vec<TreemapRect> {
    let mut chain = Vec::new();
    if let Some(mut cur) = force_open.filter(|&id| tree.is_live(id) && tree.node(id).is_dir()) {
        while cur != root {
            chain.push(cur);
            match tree.node(cur).parent() {
                // Walked off the top without meeting the root: the node is in
                // another branch, so there is nothing here to open. (The tree
                // root can report itself as its own parent, hence the second
                // arm — without it this would spin.)
                Some(parent) if parent != cur => cur = parent,
                _ => return layout_impl(tree, root, viewport, opts, filter, &[]),
            }
        }
        chain.push(root);
    }
    layout_impl(tree, root, viewport, opts, filter, &chain)
}

fn layout_impl(
    tree: &Tree,
    root: NodeId,
    viewport: Viewport,
    opts: &TreemapOptions,
    filter: Option<&[u64]>,
    open: &[NodeId],
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
    emit(tree, root, frame, 0, opts, visible, open, &mut out);
    out
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
}

// `open` is threaded down the recursion rather than carried in a context
// struct: it is one slice, and every function here already passes the same six
// things along.
#[allow(clippy::too_many_arguments)]
fn emit(
    tree: &Tree,
    id: NodeId,
    frame: Frame,
    depth: u8,
    opts: &TreemapOptions,
    visible: Option<&[u64]>,
    open: &[NodeId],
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
    // `open` — the directories the user asked to see inside of — is consulted
    // only at `lay_children`'s two gates, both of which return before a single
    // child is placed. It never reaches `items`, `scale`, `fit` or a frame, so
    // opening a directory adds rects under it and moves nothing: a forced
    // layout is a *superset* of the unforced one, not merely a different one.
    // That is what lets the map expand in place without a jump.
    if !node.is_dir() || depth >= opts.max_depth {
        return;
    }
    // Everything below changes only the *children's* frame. The rect pushed
    // above must stay a function of this directory's own frame and nothing
    // else, or a cap that stops recursion early would move the rects that
    // survive it — the UI switches depth live and relies on them holding
    // still. The floor, the dropping and the re-normalizing in `lay_children`
    // are functions of this directory alone for the same reason: never of
    // `max_depth`, and never of anything global.
    //
    // Children get the frame *whole* — the seam between two blocks is the
    // renderer's 1px and only that, whatever depth either block sits at. An
    // inset here would stack with it, so a block two levels down from another
    // would sit behind a 3px seam and the map would look chewed.
    if frame.w <= 0.0 || frame.h <= 0.0 {
        return;
    }
    // A body too thin to hold a legible block stops here — geometry, not
    // policy, so this holds in both depth modes. `max_depth` is the other
    // bound, and it is the one forcing does *not* lift: it is the only brake
    // on recursion, and the tree's depth is unbounded.
    if frame.w < opts.min_side_px as f64 || frame.h < opts.min_side_px as f64 {
        return;
    }
    lay_children(tree, id, frame, depth + 1, opts, visible, open, out);
}

#[allow(clippy::too_many_arguments)]
fn lay_children(
    tree: &Tree,
    dir: NodeId,
    frame: Frame,
    depth: u8,
    opts: &TreemapOptions,
    visible: Option<&[u64]>,
    open: &[NodeId],
    out: &mut Vec<TreemapRect>,
) {
    let forced = open.contains(&dir);
    let mut items: Vec<(NodeId, u64)> = tree
        .children(dir)
        .map(|c| (c, effective_size(tree, c, visible)))
        .filter(|&(_, size)| size > 0)
        .collect();
    if items.is_empty() {
        return;
    }
    let min_side = opts.min_side_px as f64;
    let mut total = 0.0f64;
    let mut biggest = 0u64;
    for &(_, size) in &items {
        total += size as f64;
        biggest = biggest.max(size);
    }
    let scale = frame.area() / total;

    // Adaptive depth, first half — the two reasons a level is not worth
    // drawing. Both are policy rather than geometry, hence the `min_side > 0`
    // guard: zero means "no floor, lay it out as it is", which is what the
    // exact-geometry tests ask for and what makes those reasons moot. Neither
    // applies to a directory the user opened by hand; that ask outranks them.
    //
    // `adaptive_depth` off means the caller asked for the *other* rule: take
    // every level the cap allows and let the floor deal with whatever it
    // turns up. That is the original behaviour, and the Depth setting's
    // All/1/2/3 are how it is asked for.
    if opts.adaptive_depth && !forced && min_side > 0.0 {
        // How many children this much room can hold at a size worth looking
        // at, from the frame's own area — so the same directory opens up when
        // it has room and folds when it does not. At least a few, so the count
        // alone never folds a directory holding only a handful: whether those
        // are readable is the legibility gate's question, below.
        let capacity = ((frame.area() / (COMFORT_PX * COMFORT_PX)) as usize).max(4);
        // Counted from the visible children rather than `tree.children`,
        // because under a filter or hide_system what matters is how many
        // blocks would actually be drawn. Ahead of the sort, so a directory
        // this is meant to keep off the map never pays to order it.
        if items.len() > capacity {
            return;
        }
        // The biggest thing in here would come out a speck: the subdivision
        // says no more than the plate it replaces did, and says it one folder
        // deeper every time.
        let detail = min_side * DETAIL_FACTOR;
        if biggest as f64 * scale < detail * detail {
            return;
        }
    }
    items.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let rows = if min_side <= 0.0 {
        // No floor to honour: strictly proportional, nothing to refit.
        let weights: Vec<f64> = items.iter().map(|&(_, s)| s as f64 * scale).collect();
        squarify(&weights, frame)
    } else {
        // The floor is a floor, not a cut-off line. A child too small to earn
        // its proportional share is raised to one minimum block and drawn
        // anyway, so the directory fills edge to edge. What does not fit at
        // that size is dropped — smallest first — and the survivors spread
        // into the space that frees, so dropping leaves no hole either. That
        // is the whole trade: draw the small stuff when there is room for it,
        // and when there is not, the user is here for the big blocks anyway.
        //
        // `scale` stays the proportions-only scale throughout. Raising the
        // weights and then scaling *those* up overshoots the frame, and the
        // clamp that triggers is exactly the gap this exists to remove.
        let weights: Vec<f64> = items
            .iter()
            .map(|&(_, s)| (s as f64 * scale).max(min_side * min_side))
            .collect();
        fit(&weights, frame, min_side)
    };

    for row in &rows {
        for (k, f) in row.frames.iter().enumerate() {
            emit(
                tree,
                items[row.start + k].0,
                *f,
                depth,
                opts,
                visible,
                open,
                out,
            );
        }
    }
}

/// One squarified row: consecutive items starting at `start`, laid out as a
/// slab. Every frame in a row shares the row's short side.
struct Row {
    start: usize,
    frames: Vec<Frame>,
}

impl Row {
    fn end(&self) -> usize {
        self.start + self.frames.len()
    }
}

/// Packs a floored weight list into `frame`: shed from the tail until the
/// survivors tile it in legible blocks. Every pass re-normalizes, so whatever
/// the dropped items were holding flows back into the ones that remain.
///
/// Shedding cannot stop at the first unfit row, tempting as that is: the row
/// that gave out is the *start of the run* that did, and when that run is the
/// whole tail, stopping there leaves a directory of equals showing as one
/// plate. Nor is a row fit or unfit on its own — a run of equal blocks tiles
/// legibly for some row counts and not others, so the whole layout has to be
/// tried again at each size. A slice at a time, therefore, dropping the
/// smallest first as the user would expect. `keep` only shrinks and a lone
/// block fills the frame, so this converges from any starting point.
fn fit(weights: &[f64], frame: Frame, min_side: f64) -> Vec<Row> {
    let area = frame.area();
    let mut keep = weights.len();
    loop {
        let mut sum: f64 = weights[..keep].iter().sum();
        while keep > 1 && sum > area * (1.0 + 1e-9) {
            keep -= 1;
            sum -= weights[keep];
        }
        let norm = area / sum;
        let fitted: Vec<f64> = weights[..keep].iter().map(|w| w * norm).collect();
        let rows = squarify(&fitted, frame);
        // A lone block fills the frame and cannot be unfit, so a clean run
        // here is also the only way out of the loop.
        if keep == 1 || first_unfit(&rows, keep, min_side).is_none() {
            return rows;
        }
        keep -= (keep / 16).max(1);
    }
}

/// The first item index the run could not place legibly: a frame with a side
/// under the floor, or an item the frame ran out before reaching. `None` when
/// the whole run is clean.
fn first_unfit(rows: &[Row], keep: usize, min_side: f64) -> Option<usize> {
    for row in rows {
        // A row's frames all carry its short side, so this covers that too.
        if row.frames.iter().any(|f| f.w.min(f.h) < min_side) {
            return Some(row.start);
        }
    }
    let covered = rows.last().map_or(0, Row::end);
    (covered < keep).then_some(covered)
}

fn worst_aspect(sum: f64, max: f64, min: f64, side: f64) -> f64 {
    let s2 = sum * sum;
    let w2 = side * side;
    (w2 * max / s2).max(s2 / (w2 * min))
}

/// Greedy squarification: packs `weights` into rows of near-square items,
/// largest first. Pure geometry — `frame` is left alone and nothing is
/// emitted, so a caller can look the result over and lay the items out again.
fn squarify(weights: &[f64], frame: Frame) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut remaining = frame;
    let mut i = 0;
    while i < weights.len() {
        if remaining.w <= 0.0 || remaining.h <= 0.0 {
            break;
        }
        let side = remaining.w.min(remaining.h);

        let (mut sum, mut max, mut min) = (weights[i], weights[i], weights[i]);
        let mut worst = worst_aspect(sum, max, min, side);
        let mut j = i + 1;
        while j < weights.len() {
            let a = weights[j];
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

        let horizontal = remaining.w < remaining.h; // the row spans the short side
        let thickness = (sum / side).min(if horizontal { remaining.h } else { remaining.w });

        let mut frames = Vec::with_capacity(j - i);
        let mut offset = 0.0;
        for (k, &a) in weights[i..j].iter().enumerate() {
            // The last item takes what is left rather than its own share, so
            // rounding cannot strand a hairline of frame at the far edge.
            let len = if k + 1 == j - i {
                (side - offset).max(0.0)
            } else {
                a / thickness
            };
            frames.push(if horizontal {
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
            });
            offset += len;
        }
        rows.push(Row { start: i, frames });

        if horizontal {
            remaining.y += thickness;
            remaining.h -= thickness;
        } else {
            remaining.x += thickness;
            remaining.w -= thickness;
        }
        i = j;
    }
    rows
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

    /// Nothing culled, nothing reserved: the pure squarified geometry the
    /// exact-area assertions below are written against.
    fn no_padding() -> TreemapOptions {
        TreemapOptions {
            min_side_px: 0.0,
            max_depth: 32,
            adaptive_depth: true,
            hide_system: false,
        }
    }

    fn rect_of(rects: &[TreemapRect], id: NodeId) -> TreemapRect {
        *rects.iter().find(|r| r.id == id).expect("rect missing")
    }

    fn area(r: &TreemapRect) -> f64 {
        r.w as f64 * r.h as f64
    }

    /// A flat directory of `n` files, all the same size.
    fn equal_files(n: u32, size: u64) -> Tree {
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        for i in 0..n {
            b.push("f", entry(i + 1, 0, FILE, size));
        }
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        builder.finish()
    }

    /// A flat directory of one `big` file and `n` one-byte specks.
    fn speck_tree(big: u64, n: u32) -> Tree {
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        b.push("big", entry(1, 0, FILE, big));
        for i in 0..n {
            b.push("speck", entry(2 + i, 0, FILE, 1));
        }
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        builder.finish()
    }

    /// A three-level sample holding both dominant files and speck swarms, so
    /// the floor has work to do at every level.
    fn mixed_tree() -> Tree {
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        b.push("media", entry(1, 0, DIR, 0));
        b.push("movie", entry(2, 1, FILE, 40_000));
        b.push("clips", entry(3, 1, DIR, 0));
        b.push("readme", entry(4, 0, FILE, 500));
        b.push("src", entry(5, 0, DIR, 0));
        let mut next = 6u32;
        for _ in 0..10 {
            b.push("clip", entry(next, 3, FILE, 1_000));
            next += 1;
        }
        for _ in 0..60 {
            b.push("dud", entry(next, 3, FILE, 1));
            next += 1;
        }
        for _ in 0..40 {
            b.push("mod", entry(next, 5, FILE, 300));
            next += 1;
        }
        for _ in 0..150 {
            b.push("crumb", entry(next, 5, FILE, 1));
            next += 1;
        }
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        builder.finish()
    }

    /// The area a directory hands to its children. With no padding anywhere in
    /// the layout that is simply its own rect — and the point of the tests
    /// below is that it stays that way: a block's frame is its parent's frame,
    /// so nothing but the renderer's 1px sits between two blocks.
    fn body_area(dir: &TreemapRect) -> f64 {
        dir.w as f64 * dir.h as f64
    }

    /// Total area `dir`'s direct children were laid into. Containment is what
    /// identifies them — it has to be, since two directories at the same depth
    /// are both `depth + 1` away from the same root.
    fn children_area(rects: &[TreemapRect], dir: &TreemapRect) -> f64 {
        let (x0, y0) = (dir.x, dir.y);
        let (x1, y1) = (dir.x + dir.w, dir.y + dir.h);
        rects
            .iter()
            .filter(|r| {
                r.depth == dir.depth + 1
                    && r.x >= x0 - 0.01
                    && r.y >= y0 - 0.01
                    && r.x + r.w <= x1 + 0.01
                    && r.y + r.h <= y1 + 0.01
            })
            .map(area)
            .sum()
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

    /// 1,000,000 vs 1 in 100×100: the small file's share is a 100×0.0001
    /// sliver, and it cannot buy a 1×1 block either — so it goes. Note where
    /// its space ends up: with the others, not left blank where it was.
    #[test]
    fn a_child_below_the_floor_loses_its_share_to_the_rest() {
        let tree = speck_tree(1_000_000, 1);
        let opts = TreemapOptions {
            min_side_px: 1.0,
            ..no_padding()
        };
        let rects = layout(&tree, 0, Viewport { w: 100.0, h: 100.0 }, &opts);

        assert!(rects.iter().all(|r| r.id != 2), "tiny rect must be dropped");
        // Not its proportional share — which would leave the sliver's width
        // blank — but the whole block.
        assert!((area(&rect_of(&rects, 1)) - 10_000.0).abs() < 0.01);
    }

    /// The floor is a side, not an area: 10,000 vs 1 in 100×100 gives the
    /// small file a 100×0.01 slice worth a whole square pixel, which an area
    /// test passes and nobody can see.
    #[test]
    fn a_sliver_cannot_hold_the_floor_and_is_dropped() {
        let tree = speck_tree(10_000, 1);
        let opts = TreemapOptions {
            min_side_px: 2.0,
            ..no_padding()
        };
        let rects = layout(&tree, 0, Viewport { w: 100.0, h: 100.0 }, &opts);

        assert!(rects.iter().all(|r| r.id != 2), "sliver must be dropped");
        assert!((area(&rect_of(&rects, 1)) - 10_000.0).abs() < 0.01);
    }

    /// Positively: children whose proportional share is under the floor are
    /// drawn at the floor anyway, and what they cover is the whole block. A
    /// culling layout would have left their corner of it flat and empty.
    #[test]
    fn small_children_are_raised_to_the_floor_and_fill_the_block() {
        // 300 bytes for `big`, one each for sixty specks: 111px² each here,
        // under the 6px (36px²) floor, so the floor is what sizes them.
        let tree = speck_tree(300, 60);
        let opts = TreemapOptions {
            min_side_px: 6.0,
            ..no_padding()
        };
        let rects = layout(&tree, 0, Viewport { w: 300.0, h: 300.0 }, &opts);

        let root = rect_of(&rects, 0);
        let drawn = rects.iter().filter(|r| r.depth == 1).count();
        assert!(drawn > 1, "the specks are drawn, not dropped: {drawn}");
        assert!(
            (children_area(&rects, &root) - body_area(&root)).abs() < 0.01,
            "the children cover the block"
        );
        for r in rects.iter().filter(|r| r.depth == 1) {
            assert!(
                r.w.min(r.h) >= 6.0,
                "id {} came out {}×{}, under the floor",
                r.id,
                r.w,
                r.h
            );
        }
    }

    /// "All one size": siblings equally far under the floor come out equally
    /// sized, whatever their byte counts.
    #[test]
    fn floored_siblings_all_get_the_same_size() {
        let tree = speck_tree(300, 60);
        let opts = TreemapOptions {
            min_side_px: 6.0,
            ..no_padding()
        };
        let rects = layout(&tree, 0, Viewport { w: 300.0, h: 300.0 }, &opts);

        // Everything but `big` is a speck, and they are all the same speck.
        let areas: Vec<f64> = rects
            .iter()
            .filter(|r| r.depth == 1 && r.id != 1)
            .map(area)
            .collect();
        assert!(areas.len() > 1, "specks were drawn");
        let small = areas.iter().copied().fold(f64::MAX, f64::min);
        let large = areas.iter().copied().fold(f64::MIN, f64::max);
        assert!(small / large > 0.99, "sizes spread {small} to {large}");
    }

    /// The same rule on a padded, labelled frame: the child that cannot buy a
    /// block is gone, and the one that is left covers the body exactly — the
    /// padding and the strip are all that shows through.
    #[test]
    fn a_dropped_tail_leaves_no_gap() {
        let tree = speck_tree(1_000_000, 1);
        let opts = TreemapOptions {
            min_side_px: 6.0,
            ..no_padding()
        };
        let rects = layout(&tree, 0, Viewport { w: 100.0, h: 100.0 }, &opts);

        let root = rect_of(&rects, 0);
        assert!(rects.iter().all(|r| r.id != 2), "tiny cannot buy a block");
        assert!(
            (children_area(&rects, &root) - body_area(&root)).abs() < 0.01,
            "the survivor takes the dropped child's share too"
        );
    }

    /// One file holding 99.985% of a 600×400 block leaves the rest able to buy
    /// exactly one minimum block between them. Laying them out anyway squeezes
    /// the remainder into a 0.09px column — and hit-testing walks deepest
    /// first, so that column would answer for the whole 400px of its height.
    #[test]
    fn a_degenerate_tail_is_dropped_instead_of_drawn_as_a_sliver() {
        let tree = speck_tree(999_850, 150);
        let opts = TreemapOptions {
            min_side_px: 6.0,
            ..no_padding()
        };
        let rects = layout(&tree, 0, Viewport { w: 600.0, h: 400.0 }, &opts);

        for r in &rects {
            assert!(
                r.w.min(r.h) >= 6.0,
                "id {} is {}×{}, under the floor",
                r.id,
                r.w,
                r.h
            );
        }
        let root = rect_of(&rects, 0);
        assert!(
            (children_area(&rects, &root) - body_area(&root)).abs() < 0.01,
            "dropping the tail does not open a hole"
        );
    }

    /// The Depth setting's fixed levels: every level the cap allows, and the
    /// legibility rules stand down. This is the original layout, kept as an
    /// option because a fixed depth is a legitimate thing to want.
    fn capped_depth(max_depth: u8) -> TreemapOptions {
        TreemapOptions {
            min_side_px: 6.0,
            max_depth,
            adaptive_depth: false,
            hide_system: false,
        }
    }

    /// The whole point of having both: a directory Auto folds because nothing
    /// inside it would be readable is opened by a cap, because a cap does not
    /// ask that question. Leave the gates on and this fails.
    #[test]
    fn a_cap_expands_what_the_adaptive_rule_folds() {
        let tree = legibility_gated_plate(200);
        let vp = Viewport { w: 400.0, h: 400.0 };

        let auto = layout(&tree, 0, vp, &floored(6.0));
        assert!(auto.iter().all(|r| r.depth < 2), "Auto folds it");

        let capped = layout(&tree, 0, vp, &capped_depth(2));
        assert!(
            capped.iter().any(|r| r.depth == 2),
            "a cap of 2 opens it anyway"
        );
    }

    /// A cap hides what is below it and moves nothing else — so switching the
    /// Depth setting cannot make the blocks above the line jump.
    #[test]
    fn a_cap_hides_only_the_rects_it_stops_short_of() {
        let tree = mixed_tree();
        let vp = Viewport { w: 900.0, h: 600.0 };

        let shallow = layout(&tree, 0, vp, &capped_depth(1));
        let deep: Vec<TreemapRect> = layout(&tree, 0, vp, &capped_depth(3))
            .into_iter()
            .filter(|r| r.depth <= 1)
            .collect();

        assert_eq!(shallow, deep);
        assert!(shallow.iter().any(|r| r.depth == 1), "depth 1 is drawn");
    }

    /// And the caps nest in order, which is what makes the setting a dial
    /// rather than four unrelated pictures: every level is the one above it,
    /// plus a level.
    #[test]
    fn the_caps_nest_inside_one_another() {
        let tree = mixed_tree();
        let vp = Viewport { w: 900.0, h: 600.0 };
        let at = |depth: u8| layout(&tree, 0, vp, &capped_depth(depth));

        for (shallow, deep) in [(1u8, 2u8), (2, 3)] {
            let filtered: Vec<TreemapRect> = at(deep)
                .into_iter()
                .filter(|r| r.depth <= shallow)
                .collect();
            assert_eq!(
                at(shallow),
                filtered,
                "cap {shallow} sits inside cap {deep}"
            );
        }
    }

    /// A block sits in the frame its parent was handed, exactly: the only thing
    /// between two blocks is the 1px the renderer draws, whatever depth either
    /// sits at. An inset here would stack with that one, so a block a level
    /// down from its neighbour would stand behind a 3px seam next to that
    /// neighbour's 1px — the map looked chewed, and this is the cause.
    #[test]
    fn a_block_sits_in_its_parents_frame_with_no_inset() {
        // root / d1 / d2 / f, each the only child of the one above it, so every
        // frame in the chain is the one the root was handed.
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        b.push("d1", entry(1, 0, DIR, 0));
        b.push("d2", entry(2, 1, DIR, 0));
        b.push("f", entry(3, 2, FILE, 100));
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        let tree = builder.finish();

        let rects = layout(&tree, 0, Viewport { w: 300.0, h: 200.0 }, &floored(6.0));

        let root = rect_of(&rects, 0);
        for id in [1u32, 2, 3] {
            let r = rect_of(&rects, id);
            assert_eq!(
                (r.x, r.y, r.w, r.h),
                (root.x, root.y, root.w, root.h),
                "id {id} is inset from the frame its parent was handed"
            );
        }
    }

    /// The whole rule as one invariant, under the shipped options: at every
    /// depth, every directory's children exactly cover the frame it hands
    /// down. A gap anywhere is the bug this exists to prevent.
    #[test]
    fn production_options_leave_no_gaps_at_any_depth() {
        let opts = TreemapOptions {
            min_side_px: 6.0,
            max_depth: 32,
            adaptive_depth: true,
            hide_system: false,
        };
        let tree = mixed_tree();
        let rects = layout(&tree, 0, Viewport { w: 900.0, h: 600.0 }, &opts);

        let dirs: Vec<TreemapRect> = rects.iter().copied().filter(|r| r.is_dir).collect();
        assert!(dirs.len() >= 4, "the sample really does nest");
        assert!(
            rects.iter().any(|r| r.depth == 3),
            "and runs deep enough for the floor to bite"
        );
        for dir in &dirs {
            let kids = children_area(&rects, dir);
            // No children means the directory stayed a plate: its body could
            // not hold a legible block, so it never subdivided.
            if kids == 0.0 {
                continue;
            }
            let body = body_area(dir);
            assert!(
                (kids - body).abs() < 0.001 * body,
                "dir {} covers {kids} of its {body}px² body",
                dir.id
            );
        }
    }

    /// Depth changes stay live because a capped layout is a prefix of a deeper
    /// one: the UI re-renders from the same rect list rather than re-fetching,
    /// so a rect a shallower cap drew has to hold still when a deeper one adds
    /// to it. Everything a level decides has to be local to that directory for
    /// this to survive the floor.
    #[test]
    fn a_depth_cap_only_hides_the_rects_it_stops_short_of() {
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        b.push("d1", entry(1, 0, DIR, 0));
        b.push("big", entry(2, 0, FILE, 400));
        b.push("d2", entry(3, 1, DIR, 0));
        b.push("f", entry(4, 1, FILE, 200));
        b.push("g", entry(5, 3, FILE, 100));
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        let tree = builder.finish();
        let vp = Viewport { w: 400.0, h: 300.0 };

        let capped = layout(&tree, 0, vp, &capped_at(2));
        let deep: Vec<TreemapRect> = layout(&tree, 0, vp, &capped_at(8))
            .into_iter()
            .filter(|r| r.depth <= 2)
            .collect();

        assert_eq!(capped, deep);
        assert!(capped.iter().any(|r| r.id == 3), "depth-2 dir is emitted");
        assert!(capped.iter().all(|r| r.id != 5), "depth-3 file is not");
    }

    /// Adaptive depth, one half: the same subtree expands when its biggest
    /// child is worth a level and stops when it is not — with `max_depth`
    /// nowhere near either, and the level's own cost priced in.
    #[test]
    fn a_level_that_would_only_show_a_speck_is_not_taken() {
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        b.push("wide", entry(1, 0, FILE, 800));
        b.push("mid", entry(2, 0, DIR, 0));
        b.push("deep", entry(3, 2, FILE, 200));
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        let tree = builder.finish();

        let opts = TreemapOptions {
            min_side_px: 25.0,
            ..no_padding()
        };

        // A 40px-tall viewport leaves `mid` a 40×40 body. Its child is worth
        // 1600px² there, against the 2500px² a 25px floor at twice over asks
        // for, so the level would show one speck: `mid` stays a plain plate.
        let cramped = layout(&tree, 0, Viewport { w: 200.0, h: 40.0 }, &opts);
        assert!(cramped.iter().any(|r| r.id == 2), "the dir itself is drawn");
        assert!(
            cramped.iter().all(|r| r.id != 3),
            "a speck of a level is not worth the descent"
        );

        // Same tree, taller: the body is 40×100, the child is worth 4000px²,
        // and the descent happens. Nobody raised a cap.
        let roomy = layout(&tree, 0, Viewport { w: 200.0, h: 100.0 }, &opts);
        assert!(roomy.iter().any(|r| r.id == 3), "now the level is worth it");
        assert!(opts.max_depth > 1, "the cap never came into it");
    }

    /// How many children a directory will show is `area / COMFORT_PX²`. In a
    /// 320×320 viewport that is 100, and each of those children comes out
    /// around 31×31 — far over the legibility gate's 12px — so only the
    /// capacity can be what folds the 101st.
    #[test]
    fn more_children_than_the_room_allows_stay_a_plate() {
        let vp = Viewport { w: 320.0, h: 320.0 };
        let opts = floored(6.0);
        let capacity = (320.0 * 320.0 / (COMFORT_PX * COMFORT_PX)) as u32;

        let under = layout(&equal_files(capacity, 1), 0, vp, &opts);
        assert!(under.len() > 1, "the capacity itself is still drawn");

        let over = layout(&equal_files(capacity + 1, 1), 0, vp, &opts);
        assert_eq!(over.len(), 1, "one plate once it is over");
    }

    /// The rule is about room, not about a number: the same directory with the
    /// same children folds in a small window and opens in a large one. A fixed
    /// limit cannot do both, which is the whole reason this one changed.
    #[test]
    fn the_child_limit_scales_with_the_room_a_directory_has() {
        let tree = equal_files(40, 1);
        let opts = floored(6.0);

        let roomy = layout(&tree, 0, Viewport { w: 320.0, h: 320.0 }, &opts);
        assert!(roomy.len() > 1, "100 will fit here");

        let cramped = layout(&tree, 0, Viewport { w: 200.0, h: 200.0 }, &opts);
        assert_eq!(cramped.len(), 1, "39 will not");
    }

    /// What the capacity counts is children that would be *drawn*. Hiding the
    /// system files takes this directory from 250 of them to 50, back under
    /// what 320×320 can show, so it opens again.
    #[test]
    fn the_child_limit_counts_visible_direct_children() {
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        let mut next = 1u32;
        for _ in 0..50 {
            b.push("keep", entry(next, 0, FILE, 1000));
            next += 1;
        }
        for _ in 0..200 {
            b.push("sys", entry(next, 0, EntryFlags::SYSTEM, 1000));
            next += 1;
        }
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        let tree = builder.finish();

        let vp = Viewport { w: 320.0, h: 320.0 };
        assert_eq!(
            layout(&tree, 0, vp, &floored(6.0)).len(),
            1,
            "250 of them is over the capacity"
        );
        let hidden = TreemapOptions {
            hide_system: true,
            ..floored(6.0)
        };
        assert!(
            layout(&tree, 0, vp, &hidden).len() > 1,
            "50 visible ones is not"
        );
    }

    /// Descendants are not children. Three folders holding thousands of files
    /// between them is three blocks at this level, and the limit has nothing
    /// to say about it.
    #[test]
    fn the_child_limit_counts_children_not_descendants() {
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        for d in 0..3u32 {
            b.push("dir", entry(1 + d, 0, DIR, 0));
        }
        let mut next = 4u32;
        for d in 0..3u32 {
            for _ in 0..700 {
                b.push("f", entry(next, 1 + d, FILE, 1));
                next += 1;
            }
        }
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        let tree = builder.finish();

        let rects = layout(&tree, 0, Viewport { w: 600.0, h: 600.0 }, &floored(6.0));

        assert_eq!(
            rects.iter().filter(|r| r.depth == 1).count(),
            3,
            "three folders, whatever is inside them"
        );
    }

    /// The ask: a plate the legibility rules made is still openable by hand.
    #[test]
    fn forcing_a_plate_open_reveals_what_the_floor_hid() {
        let tree = legibility_gated_plate(200);
        let vp = Viewport { w: 400.0, h: 400.0 };
        let opts = floored(6.0);

        let base = layout(&tree, 0, vp, &opts);
        assert!(
            base.iter().all(|r| r.depth < 2),
            "the plate really is a plate"
        );

        let forced = layout_with_force(&tree, 0, vp, &opts, None, Some(2));
        assert!(
            forced.iter().any(|r| r.depth == 2),
            "and it opens when asked"
        );
    }

    /// Opening a directory must not *move* anything, or the map would jump the
    /// moment it was clicked. A forced layout is a superset of the unforced
    /// one: same rects, same frames, plus whatever the override revealed.
    /// (That the revealed children sit inside their parent is a separate
    /// question — `parents_are_emitted_before_children_and_contain_them`.)
    #[test]
    fn forcing_a_plate_open_moves_no_rect_it_does_not_reveal() {
        let tree = legibility_gated_plate(200);
        let vp = Viewport { w: 400.0, h: 400.0 };
        let opts = floored(6.0);

        let base = layout(&tree, 0, vp, &opts);
        let forced = layout_with_force(&tree, 0, vp, &opts, None, Some(2));

        // Both legs matter: without the first, an override that did nothing at
        // all would satisfy the comparison.
        assert!(
            base.iter().all(|r| r.depth < 2),
            "the plate really is a plate"
        );
        assert!(forced.len() > base.len(), "and forcing really does open it");

        let seen: std::collections::HashSet<NodeId> = base.iter().map(|r| r.id).collect();
        let shared: Vec<TreemapRect> = forced
            .iter()
            .copied()
            .filter(|r| seen.contains(&r.id))
            .collect();
        assert_eq!(base, shared, "the rects that were already there held still");
    }

    /// The accordion, at the layout level: the node asked for opens, and so
    /// does everything between it and the root that was hiding it. Without
    /// that, asking for a folder inside a folded folder would open nothing.
    #[test]
    fn forcing_an_inside_node_opens_the_ancestors_it_hides_behind() {
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        b.push("outer", entry(1, 0, DIR, 0));
        b.push("inner", entry(2, 1, DIR, 0));
        let mut next = 3u32;
        for _ in 0..8 {
            b.push("leaf", entry(next, 2, FILE, 1000));
            next += 1;
        }
        for _ in 0..200 {
            b.push("f", entry(next, 1, FILE, 1));
            next += 1;
        }
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        let tree = builder.finish();

        // 320×320 holds 100, so `outer`'s 201 children are well past it.
        let vp = Viewport { w: 320.0, h: 320.0 };
        let opts = floored(6.0);
        let base = layout(&tree, 0, vp, &opts);
        assert!(base.iter().all(|r| r.depth < 2), "outer is folded");

        let forced = layout_with_force(&tree, 0, vp, &opts, None, Some(2));
        assert!(forced.iter().any(|r| r.depth == 2), "outer opened too");
        assert!(forced.iter().any(|r| r.depth == 3), "and inner with it");
    }

    /// A node in another branch is not ours to open.
    #[test]
    fn forcing_a_node_outside_the_layout_root_changes_nothing() {
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        b.push("a", entry(1, 0, DIR, 0));
        b.push("af", entry(2, 1, FILE, 500));
        b.push("b", entry(3, 0, DIR, 0));
        b.push("bf", entry(4, 3, FILE, 500));
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        let tree = builder.finish();

        let vp = Viewport { w: 300.0, h: 300.0 };
        let opts = floored(6.0);
        let plain = layout(&tree, 1, vp, &opts);
        let forced = layout_with_force(&tree, 1, vp, &opts, None, Some(3));

        assert_eq!(plain, forced, "another branch is not in this layout");
    }

    /// Opening something that was already open is not an event. `mid` fills
    /// the viewport on its own here, so it clears the legibility gate without
    /// anyone asking — the override has to leave a layout it agrees with
    /// exactly as it found it.
    #[test]
    fn forcing_an_already_open_directory_changes_nothing() {
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        b.push("mid", entry(1, 0, DIR, 0));
        b.push("a", entry(2, 1, FILE, 400));
        b.push("b", entry(3, 1, FILE, 400));
        b.push("c", entry(4, 1, FILE, 200));
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        let tree = builder.finish();

        let vp = Viewport { w: 400.0, h: 400.0 };
        let opts = floored(6.0);
        let base = layout(&tree, 0, vp, &opts);
        assert!(
            base.iter().any(|r| r.depth == 2),
            "mid subdivides on its own"
        );

        let forced = layout_with_force(&tree, 0, vp, &opts, None, Some(1));

        assert_eq!(base, forced);
    }

    /// Opening by hand overrides the legibility rules, not the floor: the
    /// revealed children are still blocks someone can see and click.
    #[test]
    fn forcing_still_honours_the_floor() {
        let tree = legibility_gated_plate(400);
        let vp = Viewport { w: 400.0, h: 400.0 };
        let opts = floored(6.0);

        let rects = layout_with_force(&tree, 0, vp, &opts, None, Some(2));

        assert!(rects.len() > 2, "the plate opened");
        for r in &rects {
            assert!(
                r.w.min(r.h) >= 6.0,
                "id {} is {}×{}, under the floor",
                r.id,
                r.w,
                r.h
            );
        }
    }

    /// A filter can zero out everything inside a folder. Opening it then finds
    /// nothing to draw — which is a plate, not a panic.
    #[test]
    fn forcing_a_directory_with_nothing_visible_stays_a_plate() {
        let tree = legibility_gated_plate(200);
        let mut bytes = vec![0u64; tree.len()];
        bytes[0] = 8000;
        bytes[1] = 8000;
        // Every leaf of `mid` is filtered out, so `mid` has no visible children.
        bytes[2] = 0;

        let vp = Viewport { w: 400.0, h: 400.0 };
        let opts = floored(6.0);
        let plain = layout_with_filter(&tree, 0, vp, &opts, &bytes);
        let forced = layout_with_force(&tree, 0, vp, &opts, Some(&bytes), Some(2));

        assert_eq!(plain, forced);
        assert!(forced.iter().all(|r| r.id != 2 || r.is_dir));
    }

    /// Adaptive depth, the other half: a body too thin for even one minimum
    /// block ends the descent there, whatever is inside it.
    #[test]
    fn a_body_thinner_than_one_block_is_not_subdivided() {
        let tree = flat_tree(&[("a", 100), ("b", 100)]);
        let opts = TreemapOptions {
            min_side_px: 25.0,
            ..no_padding()
        };

        // 1000 wide and 20 tall: a child could have 500×20, and 20 is under
        // the floor, so the root stays a plate.
        let rects = layout(&tree, 0, Viewport { w: 1000.0, h: 20.0 }, &opts);

        assert_eq!(rects.len(), 1, "only the root");
    }

    /// The nesting complaint, as a test. A folder of many equal small files
    /// has nothing to show one level down — every child would come out a
    /// speck — so it stays one plate instead of dissolving into a mesh, and
    /// so does every folder under it. Give the same folder real room and the
    /// same children are worth drawing.
    #[test]
    fn a_folder_of_equals_stays_a_plate_until_a_child_is_worth_drawing() {
        // 150 files is under the child limit, so what is being tested here is
        // the legibility gate and nothing else.
        let tree = equal_files(150, 1);
        let opts = TreemapOptions {
            min_side_px: 6.0,
            ..no_padding()
        };

        let tight = layout(&tree, 0, Viewport { w: 100.0, h: 100.0 }, &opts);
        assert_eq!(tight.len(), 1, "one plate, not 150 blocks");

        let roomy = layout(&tree, 0, Viewport { w: 600.0, h: 600.0 }, &opts);
        assert!(
            roomy.len() > 100,
            "at 2400px² a child the floor is not the point any more"
        );
    }

    /// The shipped floor, without padding — the cheapest options that still
    /// have the legibility rules switched on.
    fn floored(min_side: f32) -> TreemapOptions {
        TreemapOptions {
            min_side_px: min_side,
            ..no_padding()
        }
    }

    /// A root holding one big file and one directory of `n` one-byte children,
    /// sized so that directory is a plate for the *legibility* reason — its
    /// biggest child would come out around 20px² — and not because of the
    /// child count.
    fn legibility_gated_plate(n: u32) -> Tree {
        let mut b = EntryBatch::default();
        b.push("root", entry(0, 0, DIR, 0));
        b.push("wide", entry(1, 0, FILE, 8000));
        b.push("mid", entry(2, 0, DIR, 0));
        for i in 0..n {
            b.push("leaf", entry(3 + i, 2, FILE, 1));
        }
        let mut builder = TreeBuilder::new();
        builder.add_batch(&b);
        builder.finish()
    }

    fn capped_at(max_depth: u8) -> TreemapOptions {
        TreemapOptions {
            max_depth,
            ..no_padding()
        }
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
            min_side_px: 0.0,
            max_depth: 1,
            adaptive_depth: true,
            hide_system: false,
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
}
