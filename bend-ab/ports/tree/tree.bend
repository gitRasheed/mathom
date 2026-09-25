# Port of mathom core's tree aggregation, search, overlay and stats over a
# seeded tree that the Rust oracle generates identically (tree_cases.rs).
import Base
import ./gen/cases.bend as C

def pick(-A: Data, b: Bool, x: A, y: A) -> A:
  match b:
    case True{}:
      x
    case False{}:
      y

def H(+x: U32) -> U32:
  +a = (x .^. (x >> 16n) : U32)
  +b = U32.mul(a, 2146121005)
  +c = (b .^. (b >> 15n) : U32)
  +d = U32.mul(c, 2221713035)
  (d .^. (d >> 16n) : U32)

def nth(xs: List<&2, String>, n: Nat) -> String:
  match xs:
    case Nil{}:
      ""
    case h <> t:
      match n:
        case 0n:
          h
        case 1n+p:
          nth(t, p)

# W64
# ---

type W64 is Data:
  W64{hi: U32, lo: U32}

def W64.zero() -> W64:
  W64{0, 0}

def W64.max() -> W64:
  W64{4294967295, 4294967295}

def W64.is_zero(v: W64) -> Bool:
  match v:
    case W64{hi, lo}:
      Bool.and(U32.is_zero(hi), U32.is_zero(lo))

# Saturating u64 add.
def W64.sat.go(+t: U32, +c: U32, +lo: U32, c1: Bool) -> W64:
  +hi = U32.add(t, c)
  pick(W64, Bool.or(c1, U32.is_lt(hi, t)), W64.max(), W64{hi, lo})

def W64.sat(a: W64, b: W64) -> W64:
  match a b:
    case W64{+ahi, +alo} W64{+bhi, +blo}:
      +lo = U32.add(alo, blo)
      +t = U32.add(ahi, bhi)
      W64.sat.go(t, Bool.to_u32(U32.is_lt(lo, alo)), lo, U32.is_lt(t, ahi))

def W64.cmp.go(hc: Cmp, +alo: U32, +blo: U32) -> Cmp:
  match hc:
    case LT{}:
      LT{}
    case EQ{}:
      U32.cmp(alo, blo)
    case GT{}:
      GT{}

def W64.cmp(a: W64, b: W64) -> Cmp:
  match a b:
    case W64{ahi, alo} W64{bhi, blo}:
      W64.cmp.go(U32.cmp(ahi, bhi), alo, blo)

def W64.is_lt(a: W64, b: W64) -> Bool:
  Cmp.is_lt(W64.cmp(a, b))

def two32() -> Nat:
  Nat.mul(65536n, 65536n)

def W64.from_nat(+n: Nat) -> W64:
  W64{U32.from_nat(Nat.div(n, two32())), U32.from_nat(Nat.mod(n, two32()))}

def hex.digit(+d: U32) -> Char:
  pick(Char, U32.is_lt(d, 10), Chr{U32.add(48, d)}, Chr{U32.add(87, d)})

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

def hex8(x: U32) -> String:
  hex.go(8n, x, SNil{})

def mix64(acc: U32, v: W64) -> U32:
  match v:
    case W64{hi, lo}:
      H((acc .^. H((lo .^. H(hi) : U32)) : U32))

# Tree
# ----

type Node is Data:
  File{id: U32, name: String, sys: Bool, size: W64, alloc: W64}
  Dir{id: U32, name: String, sys: Bool, size: W64, kids: List<&2, Node>}

type Job is Data:
  JFile{node: Node}
  JDir{name: String, sys: Bool, seed: U32, depth: Nat, nk: Nat}
  JRange{seed: U32, depth: Nat, lo: U32, cnt: Nat}

def file.size(+seed: U32, +s: U32, +kind: U32) -> W64:
  pick(W64, Bool.and(U32.is_zero(kind), C.huge()), W64{H(U32.add(seed, 4)), s},
    pick(W64, U32.is_lt(kind, 4), W64.zero(), W64{0, U32.mod(s, 1048576)}))

def file.alloc.fin(v: W64) -> W64:
  match v:
    case W64{hi, lo}:
      W64{hi, (lo .&. 4294963200 : U32)}

def file.alloc(v: W64) -> W64:
  file.alloc.fin(W64.sat(v, W64{0, 4095}))

def file.name(+seed: U32, +r: U32) -> String:
  +ext = nth(C.exts(), U32.to_nat(U32.mod(H(U32.add(seed, 6)), 18)))
  +base = nth(C.bases(), U32.to_nat(U32.mod(H(U32.add(seed, 5)), 14)))
  base ++ U32.show(U32.mod(r, 1000))
    ++ pick(String, String.is_empty(ext), "", "." ++ ext)

def classify(+seed: U32, +depth: Nat, +root: Bool) -> Job:
  +r = H(seed)
  +thresh = pick(U32, Nat.is_gt(depth, 4n), 3, 1)
  +dir = Bool.or(root, Bool.and(Nat.is_gt(depth, 0n), U32.is_lt(U32.mod(r, 8), thresh)))
  +sys = Bool.and(Bool.not(root),
    U32.is_zero(U32.mod(H((seed .^. 1540483477 : U32)), 16)))
  +size = file.size(seed, H(U32.add(seed, 2)), U32.mod(H(U32.add(seed, 3)), 64))
  pick(Job, dir,
    JDir{"dir" ++ U32.show(U32.mod(r, 1000)), sys, seed, depth,
      U32.to_nat(U32.mod(H(U32.add(seed, 1)), 13))},
    JFile{File{0, file.name(seed, r), sys, size, file.alloc(size)}})

def kid_seed(+seed: U32, +i: U32) -> U32:
  H((seed .^. H(U32.add(i, 2654435769)) : U32))

# One def generates a node, and a range of a dir's kids in two parallel
# halves (mutual recursion is not allowed, so a Job picks the branch).
def gen(+fuel: Nat, job: Job) -> List<&2, Node>:
  match fuel:
    case 0n:
      Nil{}
    case 1n+f:
      match job:
        case JFile{n}:
          [n]
        case JDir{name, sys, seed, depth, nk}:
          [Dir{0, name, sys, W64.zero(), gen(f, JRange{seed, depth, 0, nk})}]
        case JRange{+seed, +depth, +lo, +cnt}:
          match cnt:
            case 0n:
              Nil{}
            case 1n+p:
              match p:
                case 0n:
                  gen(f, classify(kid_seed(seed, lo), Nat.sub(depth, 1n), False{}))
                case 1n+q:
                  +half = Nat.div(cnt, 2n)
                  +mid = U32.add(lo, U32.from_nat(half))
                  a b = gen(f, JRange{seed, depth, lo, half}) gen(f, JRange{seed, depth, mid, Nat.sub(cnt, half)})
                  List.append(&2, Node, a, b)

type NR is Data:
  NR{xs: List<&2, Node>, next: U32}

def NR.xs(r: NR) -> List<&2, Node>:
  match r:
    case NR{xs, n}:
      xs

def NR.next(r: NR) -> U32:
  match r:
    case NR{xs, n}:
      n

# Preorder ids, kids in index order.
def number(xs: List<&2, Node>, +next: U32) -> NR:
  match xs:
    case Nil{}:
      NR{Nil{}, next}
    case File{i, name, sys, size, alloc} <> t:
      +r = number(t, U32.inc(next))
      NR{File{next, name, sys, size, alloc} <> NR.xs(r), NR.next(r)}
    case Dir{i, name, sys, size, kids} <> t:
      +k = number(kids, U32.inc(next))
      +r = number(t, NR.next(k))
      NR{Dir{next, name, sys, size, NR.xs(k)} <> NR.xs(r), NR.next(r)}

type SZ is Data:
  SZ{xs: List<&2, Node>, size: W64}

def SZ.xs(r: SZ) -> List<&2, Node>:
  match r:
    case SZ{xs, s}:
      xs

def SZ.size(r: SZ) -> W64:
  match r:
    case SZ{xs, s}:
      s

def sizes.dir(id: U32, name: String, sys: Bool, +k: SZ, +r: SZ) -> SZ:
  SZ{Dir{id, name, sys, SZ.size(k), SZ.xs(k)} <> SZ.xs(r), W64.sat(SZ.size(k), SZ.size(r))}

# Directory sizes: the kids and the siblings sum in parallel.
def sizes(xs: List<&2, Node>) -> SZ:
  match xs:
    case Nil{}:
      SZ{Nil{}, W64.zero()}
    case File{id, name, sys, +size, alloc} <> t:
      +r = sizes(t)
      SZ{File{id, name, sys, size, alloc} <> SZ.xs(r), W64.sat(size, SZ.size(r))}
    case Dir{id, name, sys, size, kids} <> t:
      k r = sizes(kids) sizes(t)
      sizes.dir(id, name, sys, k, r)

def build.fin(r: NR) -> Node & U32:
  match r:
    case NR{xs, n}:
      match xs:
        case Nil{}:
          (File{0, "", False{}, W64.zero(), W64.zero()}, n)
        case h <> t:
          (h, n)

def build() -> Node & U32:
  build.fin(number(SZ.xs(sizes(gen(1000n, classify(C.seed(), C.depth(), True{})))), 0))

# Aggregates
# ----------

type Agg is Data:
  Agg{size: W64, alloc: W64, count: U32, sum: U32}

def agg.node(+id: U32, +items: U32, +size: W64, +alloc: W64) -> U32:
  mix64(mix64(H((id .^. H(items) : U32)), size), alloc)

def agg.join(a: Agg, b: Agg) -> Agg:
  match a b:
    case Agg{s1, a1, c1, x1} Agg{s2, a2, c2, x2}:
      Agg{W64.sat(s1, s2), W64.sat(a1, a2), U32.add(c1, c2), U32.add(x1, x2)}

def agg.dir(+id: U32, k: Agg) -> Agg:
  match k:
    case Agg{+size, +alloc, +count, sum}:
      Agg{size, alloc, U32.inc(count), U32.add(sum, agg.node(id, count, size, alloc))}

# The kids and the siblings fold in parallel.
def agg(xs: List<&2, Node>) -> Agg:
  match xs:
    case Nil{}:
      Agg{W64.zero(), W64.zero(), 0, 0}
    case File{+id, name, sys, +size, +alloc} <> t:
      agg.join(Agg{size, alloc, 1, agg.node(id, 0, size, alloc)}, agg(t))
    case Dir{+id, name, sys, size, kids} <> t:
      a b = agg(kids) agg(t)
      agg.join(agg.dir(id, a), b)

# Extension key (category.rs)
# ---------------------------

def utf8_len.of(+c: U32) -> Nat:
  pick(Nat, U32.is_lt(c, 128), 1n,
    pick(Nat, U32.is_lt(c, 2048), 2n, pick(Nat, U32.is_lt(c, 65536), 3n, 4n)))

def utf8_len(s: String, acc: Nat) -> Nat:
  match s:
    case SNil{}:
      acc
    case SCon{Chr{c}, t}:
      utf8_len(t, Nat.add(acc, utf8_len.of(c)))

def last(xs: List<&2, String>, acc: String) -> String:
  match xs:
    case Nil{}:
      acc
    case h <> t:
      last(t, h)

def ext.fits(+e: String) -> Bool:
  Bool.and(Bool.not(String.is_empty(e)), Nat.is_le(utf8_len(e, 0n), 8n))

def ext.keep(ok: Bool, e: String) -> Maybe<&2, String>:
  match ok:
    case True{}:
      Some{String.to_lower(e)}
    case False{}:
      None{}

def ext.of(parts: List<&2, String>) -> Maybe<&2, String>:
  match parts:
    case Nil{}:
      None{}
    case h <> t:
      match t:
        case Nil{}:
          None{}
        case h2 <> t2:
          +e = last(t2, h2)
          ext.keep(ext.fits(e), e)

def extension_key(name: String) -> Maybe<&2, String>:
  ext.of(String.split(name, '.'))

def key.str(m: Maybe<&2, String>) -> String:
  match m:
    case None{}:
      ""
    case Some{k}:
      k

type Ent is Data:
  Ent{ext: String, cat: U32}

def table.row(+cat: U32, xs: List<&2, String>) -> List<&2, Ent>:
  match xs:
    case Nil{}:
      Nil{}
    case h <> t:
      Ent{h, cat} <> table.row(cat, t)

def table() -> List<&2, Ent>:
  List.concat(&2, Ent, [
    table.row(1, ["mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg",
      "mpeg", "m2ts", "vob"]),
    table.row(2, ["mp3", "flac", "wav", "aac", "ogg", "opus", "m4a", "wma", "mid",
      "aiff"]),
    table.row(3, ["jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff", "tif", "svg",
      "ico", "heic", "raw", "cr2", "nef", "dng", "psd"]),
    table.row(4, ["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "iso", "cab",
      "wim"]),
    table.row(5, ["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "md",
      "rtf", "odt", "ods", "epub", "mobi", "csv"]),
    table.row(6, ["rs", "c", "cpp", "h", "hpp", "cs", "java", "py", "js", "ts",
      "tsx", "jsx", "go", "rb", "php", "html", "css", "scss", "json", "xml",
      "yaml", "yml", "toml", "sql", "sh", "ps1", "bat", "lock"]),
    table.row(7, ["exe", "msi", "com", "scr", "appx", "msix", "jar"]),
    table.row(8, ["dll", "sys", "drv", "ocx", "cpl", "efi", "mui", "etl", "dmp",
      "pdb", "winmd"]),
    table.row(9, ["db", "sqlite", "sqlite3", "mdb", "log", "dat", "bin", "idx",
      "bak", "pak", "vhd", "vhdx", "vmdk", "qcow2"])])

def lookup(xs: List<&2, Ent>, +key: String) -> U32:
  match xs:
    case Nil{}:
      10
    case Ent{e, c} <> t:
      pick(U32, String.eq(e, key), c, lookup(t, key))

# "" is the no-extension bucket (Other)
def categorize_key(+key: String) -> U32:
  pick(U32, String.is_empty(key), 10, lookup(table(), key))

# Hits and top-N
# --------------

type Hit is Data:
  Hit{size: W64, id: U32}

def cmp.then(c: Cmp, tie: Bool) -> Bool:
  match c:
    case GT{}:
      True{}
    case LT{}:
      False{}
    case EQ{}:
      tie

# size descending, then id ascending (the display order)
def hit.le(a: Hit, b: Hit) -> Bool:
  match a b:
    case Hit{sa, +ia} Hit{sb, +ib}:
      cmp.then(W64.cmp(sa, sb), U32.is_le(ia, ib))

# size descending, then id descending: what a min-heap of (size, id) keeps
def hit.keep(a: Hit, b: Hit) -> Bool:
  match a b:
    case Hit{sa, +ia} Hit{sb, +ib}:
      cmp.then(W64.cmp(sa, sb), U32.is_ge(ia, ib))

def top(+cap: Nat, hits: List<&2, Hit>) -> List<&2, Hit>:
  List.sort(~Hit, ~hit.le,
    List.take(&2, Hit, List.sort(~Hit, ~hit.keep, hits), cap))

def show.ids.go(xs: List<&2, Hit>) -> String:
  match xs:
    case Nil{}:
      ""
    case Hit{s, i} <> t:
      ", " ++ U32.show(i) ++ show.ids.go(t)

def show.ids(xs: List<&2, Hit>) -> String:
  match xs:
    case Nil{}:
      "[]"
    case Hit{s, i} <> t:
      "[" ++ U32.show(i) ++ show.ids.go(t) ++ "]"

# Files (stats.rs for_each_file): a hidden system subtree is skipped, never
# the root itself.
type FileHit is Data:
  FileHit{id: U32, name: String, size: W64}

def files(xs: List<&2, Node>, +hide: Bool, +acc: List<&2, FileHit>) ->
    List<&2, FileHit>:
  match xs:
    case Nil{}:
      acc
    case File{id, name, sys, size, alloc} <> t:
      files(t, hide, pick(List<&2, FileHit>, Bool.and(hide, sys), acc,
        FileHit{id, name, size} <> acc))
    case Dir{id, name, sys, size, kids} <> t:
      files(t, hide, pick(List<&2, FileHit>, Bool.and(hide, sys), acc,
        files(kids, hide, acc)))

def root.files(n: Node, +hide: Bool) -> List<&2, FileHit>:
  match n:
    case File{id, name, sys, size, alloc}:
      [FileHit{id, name, size}]
    case Dir{id, name, sys, size, kids}:
      files(kids, hide, Nil{})

def to_hits(xs: List<&2, FileHit>) -> List<&2, Hit>:
  match xs:
    case Nil{}:
      Nil{}
    case FileHit{i, n, s} <> t:
      Hit{s, i} <> to_hits(t)

# Type breakdown
# --------------

type Grp is Data:
  Grp{bytes: W64, files: U32}

def grp.add(g: Grp, size: W64) -> Grp:
  match g:
    case Grp{b, f}:
      Grp{W64.sat(b, size), U32.inc(f)}

def groups.put(mg: Map<&2, Grp> & Grp, +key: String, size: W64) -> Map<&2, Grp>:
  match mg:
    case (m, g):
      Map.set(&2, Grp, m, key, grp.add(g, size))

def groups(xs: List<&2, FileHit>, m: Map<&2, Grp>) -> Map<&2, Grp>:
  match xs:
    case Nil{}:
      m
    case FileHit{i, name, size} <> t:
      +k = key.str(extension_key(name))
      groups(t, groups.put(Map.get(Grp, Grp{W64.zero(), 0}, m, k), k, size))

type Row is Data:
  Row{ext: String, bytes: W64, files: U32}

def rows(xs: List<&2, Sigma<&2, &2, String, _ => Grp>>) -> List<&2, Row>:
  match xs:
    case Nil{}:
      Nil{}
    case (k, Grp{b, f}) <> t:
      Row{k, b, f} <> rows(t)

# bytes descending, then extension ascending
def row.le(a: Row, b: Row) -> Bool:
  match a b:
    case Row{ea, ba, fa} Row{eb, bb, fb}:
      cmp.then(W64.cmp(ba, bb), String.is_le(ea, eb))

def rows.total(xs: List<&2, Row>, b: W64, f: U32) -> Row:
  match xs:
    case Nil{}:
      Row{"", b, f}
    case Row{e, rb, rf} <> t:
      rows.total(t, W64.sat(b, rb), U32.add(f, rf))

def show.rows(xs: List<&2, Row>) -> String:
  match xs:
    case Nil{}:
      ""
    case Row{+e, b, f} <> t:
      " " ++ e ++ "|" ++ W64.show(b) ++ " " ++ U32.show(f) ++ " "
        ++ U32.show(categorize_key(e)) ++ "\n" ++ show.rows(t)

def show.total(r: Row) -> String:
  match r:
    case Row{e, b, f}:
      W64.show(b) ++ " " ++ U32.show(f)

def breakdown(+label: String, +fs: List<&2, FileHit>) -> String:
  +rs = List.sort(~Row, ~row.le,
    rows(Map.to_list(&2, Grp, groups(fs, Map.new(&2, Grp)))))
  "bd" ++ label ++ " " ++ show.total(rows.total(rs, W64.zero(), 0)) ++ "\n"
    ++ show.rows(rs)
    ++ "top" ++ label ++ " " ++ show.ids(top(20n, to_hits(fs))) ++ "\n"

# Search query (search.rs SearchQuery::parse; integer sizes only)
# ----------------------------------------------------------------

type Query is Data:
  Query{terms: List<&2, String>, exts: List<&2, String>, min: W64}

def nonempty(xs: List<&2, String>) -> List<&2, String>:
  match xs:
    case Nil{}:
      Nil{}
    case +h <> t:
      +rest = nonempty(t)
      pick(List<&2, String>, String.is_empty(h), rest, h <> rest)

def trim_dots(s: String) -> String:
  match s:
    case SNil{}:
      SNil{}
    case SCon{+h, +t}:
      pick(String, Char.is_eq(h, '.'), trim_dots(t), SCon{h, t})

def ext.norm(xs: List<&2, String>) -> List<&2, String>:
  match xs:
    case Nil{}:
      Nil{}
    case h <> t:
      String.to_lower(trim_dots(h)) <> ext.norm(t)

def is_num_char(+c: Char) -> Bool:
  Bool.or(Char.is_digit(c), Char.is_eq(c, '.'))

# rest split at the first char that is neither a digit nor '.'
def num_prefix(s: String) -> String:
  match s:
    case SNil{}:
      SNil{}
    case SCon{+h, t}:
      pick(String, is_num_char(h), SCon{h, num_prefix(t)}, SNil{})

def unit_shift(+u: String) -> Maybe<&2, Nat>:
  pick(Maybe<&2, Nat>, Bool.or(String.eq(u, ""), String.eq(u, "b")), Some{0n},
  pick(Maybe<&2, Nat>, Bool.or(String.eq(u, "k"), String.eq(u, "kb")), Some{10n},
  pick(Maybe<&2, Nat>, Bool.or(String.eq(u, "m"), String.eq(u, "mb")), Some{20n},
  pick(Maybe<&2, Nat>, Bool.or(String.eq(u, "g"), String.eq(u, "gb")), Some{30n},
  pick(Maybe<&2, Nat>, Bool.or(String.eq(u, "t"), String.eq(u, "tb")), Some{40n},
    None{})))))

def size.fin(v: Maybe<&2, Nat>, sh: Maybe<&2, Nat>) -> Maybe<&2, W64>:
  match v sh:
    case None{} _:
      None{}
    case Some{n} None{}:
      None{}
    case Some{n} Some{k}:
      Some{W64.from_nat(Nat.mul(n, Nat.pow(2n, k)))}

def size.rest(+rest: String) -> Maybe<&2, W64>:
  +num = num_prefix(rest)
  size.fin(Nat.read(num),
    unit_shift(String.to_lower(String.drop(rest, String.length(num)))))

def size.strip(+tok: String, ge: Bool, gt: Bool) -> Maybe<&2, W64>:
  match ge:
    case True{}:
      size.rest(String.drop(tok, 2n))
    case False{}:
      match gt:
        case True{}:
          size.rest(String.drop(tok, 1n))
        case False{}:
          None{}

def parse_min_size(+tok: String) -> Maybe<&2, W64>:
  size.strip(tok, String.starts_with(tok, ">="), String.starts_with(tok, ">"))

def parse.size(q: Query, +tok: String, m: Maybe<&2, W64>) -> Query:
  match q:
    case Query{terms, exts, min}:
      match m:
        case None{}:
          Query{List.append(&2, String, terms, [String.to_lower(tok)]), exts, min}
        case Some{v}:
          Query{terms, exts, v}

def parse.tok(is_ext: Bool, q: Query, +tok: String) -> Query:
  match is_ext:
    case True{}:
      match q:
        case Query{terms, exts, min}:
          Query{terms, nonempty(ext.norm(String.split(String.drop(tok, 4n), ','))), min}
    case False{}:
      parse.size(q, tok, parse_min_size(tok))

def parse.go(toks: List<&2, String>, q: Query) -> Query:
  match toks:
    case Nil{}:
      q
    case +h <> t:
      parse.go(t, parse.tok(String.starts_with(h, "ext:"), q, h))

def parse(text: String) -> Query:
  parse.go(nonempty(String.split(text, ' ')), Query{Nil{}, Nil{}, W64.zero()})

def query.is_empty(q: Query) -> Bool:
  match q:
    case Query{terms, exts, min}:
      Bool.and(List.is_empty(&2, String, terms),
        Bool.and(List.is_empty(&2, String, exts), W64.is_zero(min)))

# Matching
# --------

def any_eq(xs: List<&2, String>, +k: String) -> Bool:
  match xs:
    case Nil{}:
      False{}
    case h <> t:
      Bool.or(String.eq(h, k), any_eq(t, k))

def all_in(xs: List<&2, String>, +hay: String) -> Bool:
  match xs:
    case Nil{}:
      True{}
    case h <> t:
      Bool.and(String.contains(hay, String.to_lower(h)), all_in(t, hay))

def ext.ok(key: Maybe<&2, String>, exts: List<&2, String>) -> Bool:
  match key:
    case None{}:
      False{}
    case Some{k}:
      any_eq(exts, k)

def matches(q: Query, is_dir: Bool, +name: String, size: W64) -> Bool:
  match q:
    case Query{terms, +exts, min}:
      +ext_pass = Bool.or(List.is_empty(&2, String, exts),
        Bool.and(Bool.not(is_dir), ext.ok(extension_key(name), exts)))
      Bool.and(Bool.not(W64.is_lt(size, min)),
        Bool.and(ext_pass, all_in(terms, String.to_lower(name))))

# System pruning (hide_system never prunes the root: callers pass its kids)
def prune(xs: List<&2, Node>, +hide: Bool) -> List<&2, Node>:
  match xs:
    case Nil{}:
      Nil{}
    case File{id, name, +sys, size, alloc} <> t:
      +rest = prune(t, hide)
      pick(List<&2, Node>, Bool.and(hide, sys), rest,
        File{id, name, sys, size, alloc} <> rest)
    case Dir{id, name, +sys, size, kids} <> t:
      +rest = prune(t, hide)
      pick(List<&2, Node>, Bool.and(hide, sys), rest,
        Dir{id, name, sys, size, prune(kids, hide)} <> rest)

def root.kids(n: Node, +hide: Bool) -> List<&2, Node>:
  match n:
    case File{id, name, sys, size, alloc}:
      Nil{}
    case Dir{id, name, sys, size, kids}:
      prune(kids, hide)

# Search
# ------

type SR is Data:
  SR{count: U32, hits: List<&2, Hit>}

def sr.add(m: Bool, +id: U32, +size: W64, r: SR) -> SR:
  match m:
    case True{}:
      match r:
        case SR{c, hs}:
          SR{U32.inc(c), Hit{size, id} <> hs}
    case False{}:
      r

def walk(xs: List<&2, Node>, +q: Query, acc: SR) -> SR:
  match xs:
    case Nil{}:
      acc
    case File{+id, +name, sys, +size, alloc} <> t:
      walk(t, q, sr.add(matches(q, False{}, name, size), id, size, acc))
    case Dir{+id, +name, sys, +size, kids} <> t:
      walk(t, q, walk(kids, q, sr.add(matches(q, True{}, name, size), id, size, acc)))

def search.fin(r: SR) -> String:
  match r:
    case SR{c, hs}:
      U32.show(c) ++ " " ++ show.ids(top(50n, hs))

def search.if(empty: Bool, +root: Node, +q: Query, +hide: Bool) -> String:
  match empty:
    case True{}:
      "0 []"
    case False{}:
      search.fin(walk(root.kids(root, hide), q, SR{0, Nil{}}))

def search(+root: Node, +q: Query, +hide: Bool) -> String:
  search.if(query.is_empty(q), root, q, hide)

# Overlay (search.rs build_overlay), checksummed per node
# -------------------------------------------------------

type Ov is Data:
  Ov{bytes: W64, vis: Bool, sum: U32}

def ov.zero() -> Ov:
  Ov{W64.zero(), False{}, 0}

def ov.join(a: Ov, b: Ov) -> Ov:
  match a b:
    case Ov{b1, v1, s1} Ov{b2, v2, s2}:
      Ov{W64.sat(b1, b2), Bool.or(v1, v2), U32.add(s1, s2)}

def ov.contrib(+id: U32, b: W64, +vis: Bool) -> U32:
  match b:
    case W64{+hi, +lo}:
      pick(U32, Bool.or(vis, Bool.not(Bool.and(U32.is_zero(hi), U32.is_zero(lo)))),
        H((id .^. H((lo .^. H((hi .^. Bool.to_u32(vis) : U32)) : U32)) : U32)), 0)

def ov.file(m: Bool, +id: U32, +size: W64) -> Ov:
  match m:
    case True{}:
      Ov{size, True{}, ov.contrib(id, size, True{})}
    case False{}:
      ov.zero()

def ov.dir(+id: U32, +matched: Bool, k: Ov) -> Ov:
  match k:
    case Ov{+b, v, s}:
      +vis = Bool.or(matched, v)
      Ov{b, vis, U32.add(s, ov.contrib(id, b, vis))}

# A match is inherited by the whole subtree; kids and siblings in parallel.
def ov(xs: List<&2, Node>, +q: Query, +inh: Bool) -> Ov:
  match xs:
    case Nil{}:
      ov.zero()
    case File{+id, +name, sys, +size, alloc} <> t:
      ov.join(ov.file(Bool.or(inh, matches(q, False{}, name, size)), id, size),
        ov(t, q, inh))
    case Dir{+id, +name, sys, +size, kids} <> t:
      +m = Bool.or(inh, matches(q, True{}, name, size))
      a b = ov(kids, q, m) ov(t, q, inh)
      ov.join(ov.dir(id, m, a), b)

def ov.sum(o: Ov) -> U32:
  match o:
    case Ov{b, v, s}:
      s

def overlay.if(empty: Bool, +root: Node, +q: Query, +hide: Bool) -> String:
  match empty:
    case True{}:
      hex8(0)
    case False{}:
      hex8(ov.sum(ov.dir(0, False{}, ov(root.kids(root, hide), q, False{}))))

def overlay(+root: Node, +q: Query, +hide: Bool) -> String:
  overlay.if(query.is_empty(q), root, q, hide)

def report.q(+k: U32, +h: String, +root: Node, +q: Query, +hide: Bool) -> String:
  "q" ++ U32.show(k) ++ "." ++ h ++ " " ++ search(root, q, hide) ++ "\n"
    ++ "ov" ++ U32.show(k) ++ "." ++ h ++ " " ++ overlay(root, q, hide) ++ "\n"

def report.queries(qs: List<&2, String>, +k: U32, +root: Node) -> String:
  match qs:
    case Nil{}:
      ""
    case h <> t:
      +q = parse(h)
      report.q(k, "0", root, q, False{}) ++ report.q(k, "1", root, q, True{})
        ++ report.queries(t, U32.inc(k), root)

# Report
# ------

def report.agg(a: Agg) -> String:
  match a:
    case Agg{s, al, +c, x}:
      "nodes " ++ U32.show(c) ++ "\nroot " ++ W64.show(s) ++ " " ++ W64.show(al)
        ++ " " ++ U32.show(U32.sub(c, 1)) ++ "\naggsum " ++ hex8(x) ++ "\n"

def report.head(+root: Node) -> String:
  report.agg(agg([root]))
    ++ breakdown("0", root.files(root, False{}))
    ++ breakdown("1", root.files(root, True{}))
    ++ report.queries(C.queries(), 0, root)

def report.go(b: Node & U32) -> String:
  match b:
    case (root, n):
      report.head(root)

def report() -> String:
  report.go(build())

def main_io() -> IO(Unit):
  IO.write(report())

def phase.agg.go(b: Node & U32) -> String:
  match b:
    case (root, n):
      report.agg(agg([root]))

def phase.gen() -> String:
  phase.agg.go(build.fin(number(gen(1000n, classify(C.seed(), C.depth(), True{})), 0)))

def phase.build() -> String:
  phase.agg.go(build())

def phase.bd.go(b: Node & U32) -> String:
  match b:
    case (root, n):
      breakdown("0", root.files(root, False{}))

def phase.bd() -> String:
  phase.bd.go(build())

def phase.q.go(b: Node & U32) -> String:
  match b:
    case (root, n):
      report.queries(C.queries(), 0, root)

def phase.q() -> String:
  phase.q.go(build())
