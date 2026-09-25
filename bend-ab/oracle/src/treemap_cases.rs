//! Treemap: mathom's real f64 layout, and an f32 mirror of it that performs
//! exactly the arithmetic treemap.bend performs (Bend has F32 only). The
//! mirror is the bit-exact reference for Bend's lanes; the real layout is
//! the tolerance reference for the port itself.

use std::fmt::Write as _;
use std::fs;

use mathom_core::tree::{NodeId, Tree};
use mathom_core::treemap::{TreemapOptions, Viewport, layout};

pub const VW: f32 = 1920.0;
pub const VH: f32 = 1080.0;
pub const PAD: f32 = 1.0;
pub const MIN_AREA: f32 = 1.0;
pub const MAX_DEPTH: u32 = 32;

#[derive(Clone, Copy)]
struct Fr {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

/// u64 -> f32 as the port does it: hi * 2^32 + lo, each an f32.
fn to_f32(v: u64) -> f32 {
    ((v >> 32) as u32 as f32) * 4294967296.0f32 + (v as u32 as f32)
}

fn worst(sum: f32, max: f32, min: f32, side: f32) -> f32 {
    let s2 = sum * sum;
    let w2 = side * side;
    let a = w2 * max / s2;
    let b = s2 / (w2 * min);
    if a > b { a } else { b }
}

fn fmin(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}

fn fmax(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

struct M<'a> {
    tree: &'a Tree,
    out: Vec<(NodeId, u32, Fr)>,
}

impl M<'_> {
    fn emit(&mut self, id: NodeId, f: Fr, depth: u32) {
        self.out.push((id, depth, f));
        let node = self.tree.node(id);
        if !node.is_dir() || depth >= MAX_DEPTH {
            return;
        }
        let inner = Fr {
            x: f.x + PAD,
            y: f.y + PAD,
            w: f.w - 2.0 * PAD,
            h: f.h - 2.0 * PAD,
        };
        if inner.w <= 0.0 || inner.h <= 0.0 {
            return;
        }
        self.children(id, inner, depth + 1);
    }

    fn children(&mut self, dir: NodeId, frame: Fr, depth: u32) {
        let mut items: Vec<(NodeId, u64)> = self
            .tree
            .children(dir)
            .map(|c| (c, self.tree.node(c).size))
            .filter(|&(_, s)| s > 0)
            .collect();
        if items.is_empty() {
            return;
        }
        items.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut total = 0.0f32;
        for &(_, s) in &items {
            total += to_f32(s);
        }
        let scale = frame.w * frame.h / total;
        let mut rem = frame;
        let mut i = 0;
        while i < items.len() {
            if rem.w <= 0.0 || rem.h <= 0.0 {
                return;
            }
            let side = fmin(rem.w, rem.h);
            let first = to_f32(items[i].1) * scale;
            let (mut sum, mut max, mut min) = (first, first, first);
            let mut wst = worst(sum, max, min, side);
            let mut j = i + 1;
            while j < items.len() {
                let a = to_f32(items[j].1) * scale;
                let cand = worst(sum + a, fmax(max, a), fmin(min, a), side);
                if cand > wst {
                    break;
                }
                wst = cand;
                sum += a;
                max = fmax(max, a);
                min = fmin(min, a);
                j += 1;
            }
            let row: Vec<(NodeId, u64)> = items[i..j].to_vec();
            self.row(&row, scale, sum, &mut rem, depth);
            i = j;
        }
    }

    fn row(&mut self, row: &[(NodeId, u64)], scale: f32, area: f32, rem: &mut Fr, depth: u32) {
        let horizontal = rem.w < rem.h;
        let side = if horizontal { rem.w } else { rem.h };
        let thick = fmin(area / side, if horizontal { rem.h } else { rem.w });
        let mut off = 0.0f32;
        for (k, &(id, s)) in row.iter().enumerate() {
            let len = if k == row.len() - 1 {
                side - off
            } else {
                to_f32(s) * scale / thick
            };
            let f = if horizontal {
                Fr {
                    x: rem.x + off,
                    y: rem.y,
                    w: len,
                    h: thick,
                }
            } else {
                Fr {
                    x: rem.x,
                    y: rem.y + off,
                    w: thick,
                    h: len,
                }
            };
            off += len;
            if f.w * f.h >= MIN_AREA {
                self.emit(id, f, depth);
            }
        }
        if horizontal {
            rem.y += thick;
            rem.h -= thick;
        } else {
            rem.x += thick;
            rem.w -= thick;
        }
    }
}

pub fn mirror(tree: &Tree) -> String {
    let mut m = M {
        tree,
        out: Vec::new(),
    };
    m.emit(
        0,
        Fr {
            x: 0.0,
            y: 0.0,
            w: VW,
            h: VH,
        },
        0,
    );
    let mut o = String::new();
    for (id, d, f) in m.out {
        let _ = writeln!(
            o,
            "{id} {d} {} {} {} {}",
            f.x.to_bits(),
            f.y.to_bits(),
            f.w.to_bits(),
            f.h.to_bits()
        );
    }
    o
}

pub fn real(tree: &Tree) -> String {
    let opts = TreemapOptions {
        min_area_px: MIN_AREA,
        padding_px: PAD,
        max_depth: MAX_DEPTH as u8,
        hide_system: false,
    };
    let rects = layout(tree, 0, Viewport { w: VW, h: VH }, &opts);
    let mut o = String::new();
    for r in rects {
        let _ = writeln!(o, "{} {} {} {} {} {}", r.id, r.depth, r.x, r.y, r.w, r.h);
    }
    o
}

pub fn emit(seed: usize, depth: usize, out: &str) {
    crate::tree_cases::emit(seed, depth, false, out);
    let tree = crate::tree_cases::build(seed as u32, depth as u32, false);
    fs::write(format!("{out}/expected.txt"), mirror(&tree)).unwrap();
    fs::write(format!("{out}/real.txt"), real(&tree)).unwrap();
}
