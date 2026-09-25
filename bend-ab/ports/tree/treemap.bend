# Port of mathom crates/core/src/treemap.rs (squarified layout) in F32.
# One def walks a Job; the helpers above it decide what the next Job is,
# since a def may not call one below it and a match cannot scrutinize a
# computed value.
import Base
import ./tree.bend as T

def VW() -> F32:
  1920.0

def VH() -> F32:
  1080.0

def PAD() -> F32:
  1.0

def MIN_AREA() -> F32:
  1.0

def MAX_DEPTH() -> U32:
  32

type Fr is Data:
  Fr{x: F32, y: F32, w: F32, h: F32}

type Item is Data:
  Item{node: T.Node, id: U32, size: T.W64}

type Job is Data:
  JNil{}
  JRect{id: U32, depth: U32, f: Fr, then: Job}
  JCat{a: Job, b: Job}
  JRows{items: List<&2, Item>, rem: Fr, scale: F32, depth: U32}

# u64 -> f32 as the Rust mirror does it: hi * 2^32 + lo
def to_f32(v: T.W64) -> F32:
  match v:
    case T.W64{hi, lo}:
      F32.add(F32.mul(U32.to_f32(hi), 4294967296.0), U32.to_f32(lo))

def worst(+sum: F32, max: F32, min: F32, +side: F32) -> F32:
  +s2 = F32.mul(sum, sum)
  +w2 = F32.mul(side, side)
  F32.max(F32.div(F32.mul(w2, max), s2), F32.div(s2, F32.mul(w2, min)))

# Children
# --------

def item.of(n: T.Node) -> Item:
  match n:
    case T.File{+id, name, sys, +size, alloc}:
      Item{T.File{id, name, sys, size, alloc}, id, size}
    case T.Dir{+id, name, sys, +size, kids}:
      Item{T.Dir{id, name, sys, size, kids}, id, size}

def items.keep(it: Item, +rest: List<&2, Item>) -> List<&2, Item>:
  match it:
    case Item{n, i, +s}:
      Bool.pick(List<&2, Item>, T.W64.is_zero(s), rest, Item{n, i, s} <> rest)

def items.of(xs: List<&2, T.Node>) -> List<&2, Item>:
  match xs:
    case Nil{}:
      Nil{}
    case h <> t:
      items.keep(item.of(h), items.of(t))

# size descending, then id ascending
def item.le(a: Item, b: Item) -> Bool:
  match a b:
    case Item{na, +ia, sa} Item{nb, +ib, sb}:
      T.cmp.then(T.W64.cmp(sa, sb), U32.is_le(ia, ib))

def total(xs: List<&2, Item>, acc: F32) -> F32:
  match xs:
    case Nil{}:
      acc
    case Item{n, i, s} <> t:
      total(t, F32.add(acc, to_f32(s)))

def kids.job(+items: List<&2, Item>, +f: Fr, +depth: U32) -> Job:
  match f:
    case Fr{x, y, +w, +h}:
      JRows{items, Fr{x, y, w, h},
        F32.div(F32.mul(w, h), total(items, 0.0)), depth}

# emit: the rect, then (dirs under max_depth, with a positive inset) the kids
def emit.kids(go: Bool, n: T.Node, +inner: Fr, +depth: U32) -> Job:
  match go:
    case False{}:
      JNil{}
    case True{}:
      match n:
        case T.File{id, name, sys, size, alloc}:
          JNil{}
        case T.Dir{id, name, sys, size, kids}:
          kids.job(List.sort(~Item, ~item.le, items.of(kids)), inner, U32.inc(depth))

def inset(f: Fr) -> Fr:
  match f:
    case Fr{x, y, w, h}:
      +p2 = F32.mul(2.0, PAD())
      Fr{F32.add(x, PAD()), F32.add(y, PAD()), F32.sub(w, p2), F32.sub(h, p2)}

def fr.positive(f: Fr) -> Bool:
  match f:
    case Fr{x, y, w, h}:
      Bool.and(F32.is_gt(w, 0.0), F32.is_gt(h, 0.0))

def is_dir(n: T.Node) -> Bool:
  match n:
    case T.File{id, name, sys, size, alloc}:
      False{}
    case T.Dir{id, name, sys, size, kids}:
      True{}

def emit.job(+n: T.Node, +id: U32, +f: Fr, +depth: U32) -> Job:
  +inner = inset(f)
  +go = Bool.and(Bool.and(is_dir(n), U32.is_lt(depth, MAX_DEPTH())), fr.positive(inner))
  JRect{id, depth, f, emit.kids(go, n, inner, depth)}

# Rows
# ----

type Row is Data:
  Row{taken: List<&2, Item>, rest: List<&2, Item>, sum: F32}

def row.cons(h: Item, r: Row) -> Row:
  match r:
    case Row{tk, rs, s}:
      Row{h <> tk, rs, s}

# Grow the row while the worst aspect does not get worse. The continuation
# is built strictly and dropped when the row stops.
def grow(xs: List<&2, Item>, +sum: F32, +max: F32, +min: F32, +wst: F32, +side: F32,
    +scale: F32) -> Row:
  match xs:
    case Nil{}:
      Row{Nil{}, Nil{}, sum}
    case +h <> +t:
      match h:
        case Item{n, i, +s}:
          +a = F32.mul(to_f32(s), scale)
          +cand = worst(F32.add(sum, a), F32.max(max, a), F32.min(min, a), side)
          Bool.pick(Row, F32.is_gt(cand, wst), Row{Nil{}, h <> t, sum},
            row.cons(h, grow(t, F32.add(sum, a), F32.max(max, a), F32.min(min, a),
              cand, side, scale)))

type Laid is Data:
  Laid{jobs: Job, rem: Fr}

def fr.area(f: Fr) -> F32:
  match f:
    case Fr{x, y, w, h}:
      F32.mul(w, h)

# the row's frames, each emitted if its area reaches MIN_AREA
def frames(xs: List<&2, Item>, +hz: Bool, +side: F32, +thick: F32, +rx: F32, +ry: F32,
    +off: F32, +scale: F32, +depth: U32) -> Job:
  match xs:
    case Nil{}:
      JNil{}
    case Item{+n, +i, +s} <> +t:
      +len = Bool.pick(F32, List.is_empty(&2, Item, t), F32.sub(side, off),
        F32.div(F32.mul(to_f32(s), scale), thick))
      +f = Bool.pick(Fr, hz, Fr{F32.add(rx, off), ry, len, thick},
        Fr{rx, F32.add(ry, off), thick, len})
      +rest = frames(t, hz, side, thick, rx, ry, F32.add(off, len), scale, depth)
      Bool.pick(Job, F32.is_ge(fr.area(f), MIN_AREA()), JCat{emit.job(n, i, f, depth), rest}, rest)

def lay.row(row: List<&2, Item>, +scale: F32, +area: F32, rem: Fr, +depth: U32) -> Laid:
  match rem:
    case Fr{+x, +y, +w, +h}:
      +hz = F32.is_lt(w, h)
      +side = Bool.pick(F32, hz, w, h)
      +thick = F32.min(F32.div(area, side), Bool.pick(F32, hz, h, w))
      Laid{frames(row, hz, side, thick, x, y, 0.0, scale, depth),
        Bool.pick(Fr, hz, Fr{x, F32.add(y, thick), w, F32.sub(h, thick)},
          Fr{F32.add(x, thick), y, F32.sub(w, thick), h})}

def rows.laid(l: Laid, rest: List<&2, Item>, +scale: F32, +depth: U32) -> Job:
  match l:
    case Laid{jobs, rem}:
      JCat{jobs, JRows{rest, rem, scale, depth}}

def rows.next(r: Row, +head: Item, +scale: F32, rem: Fr, +depth: U32) -> Job:
  match r:
    case Row{taken, rest, sum}:
      rows.laid(lay.row(head <> taken, scale, sum, rem, depth), rest, scale, depth)

def rows.go(ok: Bool, items: List<&2, Item>, +rem: Fr, +scale: F32, +depth: U32) -> Job:
  match ok:
    case False{}:
      JNil{}
    case True{}:
      match items:
        case Nil{}:
          JNil{}
        case +h <> t:
          match h:
            case Item{n, i, s}:
              match rem:
                case Fr{+x, +y, +w, +h2}:
                  +side = F32.min(w, h2)
                  +a = F32.mul(to_f32(s), scale)
                  rows.next(grow(t, a, a, a, worst(a, a, a, side), side, scale), h, scale,
                    Fr{x, y, w, h2}, depth)

def rows.job(items: List<&2, Item>, +rem: Fr, +scale: F32, +depth: U32) -> Job:
  rows.go(fr.positive(rem), items, rem, scale, depth)

def rect.show(+id: U32, +depth: U32, f: Fr) -> String:
  match f:
    case Fr{x, y, w, h}:
      U32.show(id) ++ " " ++ U32.show(depth) ++ " " ++ U32.show(F32.bits(x)) ++ " "
        ++ U32.show(F32.bits(y)) ++ " " ++ U32.show(F32.bits(w)) ++ " "
        ++ U32.show(F32.bits(h)) ++ "\n"

# The walker: rect, then its kids; a JCat's halves run in parallel.
def tm(+fuel: Nat, job: Job) -> String:
  match fuel:
    case 0n:
      "fuel\n"
    case 1n+k:
      match job:
        case JNil{}:
          ""
        case JRect{id, depth, f, then}:
          rect.show(id, depth, f) ++ tm(k, then)
        case JCat{a, b}:
          x y = tm(k, a) tm(k, b)
          x ++ y
        case JRows{items, rem, scale, depth}:
          tm(k, rows.job(items, rem, scale, depth))

def layout.go(b: T.Node & U32) -> String:
  match b:
    case (root, n):
      tm(100000n, emit.job(root, 0, Fr{0.0, 0.0, VW(), VH()}, 0))

def layout() -> String:
  layout.go(T.build())
