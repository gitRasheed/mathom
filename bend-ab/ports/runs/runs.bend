# Port of mathom crates/scanner-ntfs/src/runs.rs: NTFS data-run decoding.
# Bend has no 64-bit integers, so u64/i64 are a W64{hi, lo} pair of U32.
import Base

type W64 is Data:
  W64{hi: U32, lo: U32}

type Ext is Data:
  Ext{lcn: W64, clusters: W64}

type Runs is Data:
  RErr{msg: String}
  ROk{exts: List<&2, Ext>}

type Backed is Data:
  BErr{msg: String}
  BOk{total: W64}

type Split is Data:
  Short{}
  Cut{taken: List<&2, U32>, rest: List<&2, U32>}

# W64
# ---

# (v << 8) | b
def W64.push(v: W64, +b: U32) -> W64:
  match v:
    case W64{hi, +lo}:
      W64{((hi << 8n) .|. (lo >> 24n) : U32), ((lo << 8n) .|. b : U32)}

def W64.from_be(xs: List<&2, U32>, acc: W64) -> W64:
  match xs:
    case Nil{}:
      acc
    case h <> t:
      W64.from_be(t, W64.push(acc, h))

# uint_le: byte i lands at bit 8*i
def W64.from_le(xs: List<&2, U32>) -> W64:
  W64.from_be(List.reverse(&2, U32, xs), W64{0, 0})

def W64.is_zero(v: W64) -> Bool:
  match v:
    case W64{hi, lo}:
      Bool.and(U32.is_zero(hi), U32.is_zero(lo))

def W64.is_neg(v: W64) -> Bool:
  match v:
    case W64{hi, lo}:
      U32.is_ne((hi >> 31n : U32), 0)

# Signed 64-bit add; None on overflow (i64::checked_add).
def W64.sadd.fin(r: W64, ovf: Bool) -> Maybe<&2, W64>:
  match ovf:
    case True{}:
      None{}
    case False{}:
      Some{r}

def W64.sadd.go(+ahi: U32, +bhi: U32, +lo: U32, +alo: U32) -> Maybe<&2, W64>:
  +hi = U32.add(U32.add(ahi, bhi), Bool.to_u32(U32.is_lt(lo, alo)))
  +x = ((ahi .^. hi) .&. (bhi .^. hi) : U32)
  W64.sadd.fin(W64{hi, lo}, U32.is_ne((x >> 31n : U32), 0))

def W64.sadd(a: W64, b: W64) -> Maybe<&2, W64>:
  match a b:
    case W64{+ahi, +alo} W64{+bhi, +blo}:
      W64.sadd.go(ahi, bhi, U32.add(alo, blo), alo)

# Unsigned 64-bit add; None on overflow (u64::checked_add).
def W64.uadd.go(+t: U32, +c: U32, +lo: U32, c1: Bool) -> Maybe<&2, W64>:
  +hi = U32.add(t, c)
  W64.sadd.fin(W64{hi, lo}, Bool.or(c1, U32.is_lt(hi, t)))

def W64.uadd(a: W64, b: W64) -> Maybe<&2, W64>:
  match a b:
    case W64{+ahi, +alo} W64{+bhi, +blo}:
      +lo = U32.add(alo, blo)
      +t = U32.add(ahi, bhi)
      W64.uadd.go(t, Bool.to_u32(U32.is_lt(lo, alo)), lo, U32.is_lt(t, ahi))

# Hex
# ---

def hex.digit.if(+d: U32, small: Bool) -> Char:
  match small:
    case True{}:
      Chr{U32.add(48, d)}
    case False{}:
      Chr{U32.add(87, d)}

def hex.digit(+d: U32) -> Char:
  hex.digit.if(d, U32.is_lt(d, 10))

# the low `n` nibbles of x, most significant first, before acc
def hex.go(n: Nat, +x: U32, acc: String) -> String:
  match n:
    case 0n:
      acc
    case 1n+p:
      hex.go(p, (x >> 4n : U32), SCon{hex.digit((x .&. 15 : U32)), acc})

def W64.show(v: W64) -> String:
  match v:
    case W64{hi, lo}:
      hex.go(8n, hi, SNil{}) ++ hex.go(8n, lo, SNil{})

# Bytes
# -----

def take.put(h: U32, s: Split) -> Split:
  match s:
    case Short{}:
      Short{}
    case Cut{a, r}:
      Cut{h <> a, r}

def take(n: Nat, xs: List<&2, U32>) -> Split:
  match n:
    case 0n:
      Cut{Nil{}, xs}
    case 1n+p:
      match xs:
        case Nil{}:
          Short{}
        case h <> t:
          take.put(h, take(p, t))

def last_or0(xs: List<&2, U32>, acc: U32) -> U32:
  match xs:
    case Nil{}:
      acc
    case h <> t:
      last_or0(t, h)

def sext.fill(neg: Bool) -> U32:
  match neg:
    case True{}:
      255
    case False{}:
      0

# int_le: a w-byte little-endian field, sign-extended to 64 bits
def int_le(+w: Nat, +xs: List<&2, U32>) -> W64:
  +top = last_or0(xs, 0)
  +fill = sext.fill(U32.is_ne((top .&. 128 : U32), 0))
  W64.from_le(List.append(&2, U32, xs, List.replicate(U32, Nat.sub(8n, w), fill)))

# decode_runs
# -----------

type St is Data:
  SDone{runs: Runs}
  SMore{data: List<&2, U32>, lcn: W64, acc: List<&2, Ext>}

def err(msg: String) -> St:
  SDone{RErr{msg}}

def dec.push.nz(z: Bool, +l: W64, clusters: W64, rest: List<&2, U32>,
    acc: List<&2, Ext>) -> St:
  match z:
    case True{}:
      err("zero-length run")
    case False{}:
      SMore{rest, l, Ext{l, clusters} <> acc}

def dec.push.neg(neg: Bool, l: W64, +clusters: W64, rest: List<&2, U32>,
    acc: List<&2, Ext>) -> St:
  match neg:
    case True{}:
      err("run points before the volume start")
    case False{}:
      dec.push.nz(W64.is_zero(clusters), l, clusters, rest, acc)

# checked lcn, then negativity, then zero length, as runs.rs orders them
def dec.push(nl: Maybe<&2, W64>, clusters: W64, rest: List<&2, U32>,
    acc: List<&2, Ext>) -> St:
  match nl:
    case None{}:
      err("run LCN overflow")
    case Some{+l}:
      dec.push.neg(W64.is_neg(l), l, clusters, rest, acc)

def dec.fields(+lw: Nat, +ow: Nat, s: Split, lcn: W64, rest: List<&2, U32>,
    acc: List<&2, Ext>) -> St:
  match s:
    case Short{}:
      err("run list truncated")
    case Cut{lb, ob}:
      dec.push(W64.sadd(lcn, int_le(ow, ob)), W64.from_le(lb), rest, acc)

def dec.cut(+lw: Nat, +ow: Nat, s: Split, lcn: W64, acc: List<&2, Ext>) -> St:
  match s:
    case Short{}:
      err("run list truncated")
    case Cut{taken, rest}:
      dec.fields(lw, ow, take(lw, taken), lcn, rest, acc)

def dec.sparse(sparse: Bool, +lw: Nat, +ow: Nat, t: List<&2, U32>, lcn: W64,
    acc: List<&2, Ext>) -> St:
  match sparse:
    case True{}:
      err("sparse run in a run list that must be dense")
    case False{}:
      dec.cut(lw, ow, take(Nat.add(lw, ow), t), lcn, acc)

def bad_widths(+lw: U32, +ow: U32) -> Bool:
  Bool.or(U32.is_zero(lw), Bool.or(U32.is_gt(lw, 8), U32.is_gt(ow, 8)))

def dec.widths(bad: Bool, +lw: U32, +ow: U32, t: List<&2, U32>, lcn: W64,
    acc: List<&2, Ext>) -> St:
  match bad:
    case True{}:
      err("run header has implausible field widths")
    case False{}:
      dec.sparse(U32.is_zero(ow), U32.to_nat(lw), U32.to_nat(ow), t, lcn, acc)

def dec.hdr(z: Bool, +h: U32, t: List<&2, U32>, lcn: W64, acc: List<&2, Ext>)
    -> St:
  match z:
    case True{}:
      SDone{ROk{List.reverse(&2, Ext, acc)}}
    case False{}:
      +lw = (h .&. 15 : U32)
      +ow = (h >> 4n : U32)
      dec.widths(bad_widths(lw, ow), lw, ow, t, lcn, acc)

def dec.step(data: List<&2, U32>, lcn: W64, acc: List<&2, Ext>) -> St:
  match data:
    case Nil{}:
      err("run list missing terminator")
    case +h <> t:
      dec.hdr(U32.is_zero(h), h, t, lcn, acc)

# every run eats its header byte, so len(data)+1 steps always suffice
def dec.go(fuel: Nat, st: St) -> Runs:
  match fuel:
    case 0n:
      RErr{"fuel"}
    case 1n+f:
      match st:
        case SDone{r}:
          r
        case SMore{d, l, a}:
          dec.go(f, dec.step(d, l, a))

def decode_runs(+data: List<&2, U32>) -> Runs:
  dec.go(2n+List.length(&2, U32, data), SMore{data, W64{0, 0}, Nil{}})

# backed_clusters
# ---------------

type Bt is Data:
  BDone{res: Backed}
  BMore{data: List<&2, U32>, total: W64}

def bk.err(msg: String) -> Bt:
  BDone{BErr{msg}}

def bk.sum(nt: Maybe<&2, W64>, rest: List<&2, U32>) -> Bt:
  match nt:
    case None{}:
      bk.err("backed cluster count overflow")
    case Some{t}:
      BMore{rest, t}

def bk.fields(s: Split, backed: Bool, total: W64, rest: List<&2, U32>) -> Bt:
  match s:
    case Short{}:
      bk.err("run list truncated")
    case Cut{lb, ob}:
      match backed:
        case True{}:
          bk.sum(W64.uadd(total, W64.from_le(lb)), rest)
        case False{}:
          BMore{rest, total}

def bk.cut(+lw: Nat, +ow: Nat, s: Split, total: W64) -> Bt:
  match s:
    case Short{}:
      bk.err("run list truncated")
    case Cut{taken, rest}:
      bk.fields(take(lw, taken), Nat.is_ne(ow, 0n), total, rest)

def bk.widths(bad: Bool, +lw: U32, +ow: U32, t: List<&2, U32>, total: W64)
    -> Bt:
  match bad:
    case True{}:
      bk.err("run header has implausible field widths")
    case False{}:
      +l = U32.to_nat(lw)
      +o = U32.to_nat(ow)
      bk.cut(l, o, take(Nat.add(l, o), t), total)

def bk.hdr(z: Bool, +h: U32, t: List<&2, U32>, total: W64) -> Bt:
  match z:
    case True{}:
      BDone{BOk{total}}
    case False{}:
      +lw = (h .&. 15 : U32)
      +ow = (h >> 4n : U32)
      bk.widths(bad_widths(lw, ow), lw, ow, t, total)

def bk.step(data: List<&2, U32>, total: W64) -> Bt:
  match data:
    case Nil{}:
      bk.err("run list missing terminator")
    case +h <> t:
      bk.hdr(U32.is_zero(h), h, t, total)

def bk.go(fuel: Nat, st: Bt) -> Backed:
  match fuel:
    case 0n:
      BErr{"fuel"}
    case 1n+f:
      match st:
        case BDone{r}:
          r
        case BMore{d, t}:
          bk.go(f, bk.step(d, t))

def backed_clusters(+data: List<&2, U32>) -> Backed:
  bk.go(2n+List.length(&2, U32, data), BMore{data, W64{0, 0}})

# Output
# ------

def show.exts(xs: List<&2, Ext>) -> String:
  match xs:
    case Nil{}:
      ""
    case Ext{l, c} <> t:
      " " ++ W64.show(l) ++ ":" ++ W64.show(c) ++ show.exts(t)

def show.runs(r: Runs) -> String:
  match r:
    case RErr{m}:
      "ERR " ++ m
    case ROk{xs}:
      "OK" ++ show.exts(xs)

def show.backed(r: Backed) -> String:
  match r:
    case BErr{m}:
      "ERR " ++ m
    case BOk{t}:
      "OK " ++ W64.show(t)

def line(+data: List<&2, U32>) -> String:
  show.runs(decode_runs(data)) ++ " | " ++ show.backed(backed_clusters(data))

def run.one(cs: List<&2, List<&2, U32>>) -> IO(Unit):
  match cs:
    case Nil{}:
      IO.pure(Unit, Unit{})
    case h <> t:
      do IO<Unit>:
        IO.print(line(h))
        run.one(t)

def run(chs: List<&2, List<&2, List<&2, U32>>>) -> IO(Unit):
  match chs:
    case Nil{}:
      IO.pure(Unit, Unit{})
    case h <> t:
      do IO<Unit>:
        run.one(h)
        run(t)

def lines.one(cs: List<&2, List<&2, U32>>) -> String:
  match cs:
    case Nil{}:
      ""
    case h <> t:
      line(h) ++ "\n" ++ lines.one(t)

def lines(chs: List<&2, List<&2, List<&2, U32>>>) -> String:
  match chs:
    case Nil{}:
      ""
    case h <> t:
      lines.one(h) ++ lines(t)
