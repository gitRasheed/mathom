# Port of mathom crates/core/src/category.rs: extension key + category.
# Rust measures the extension in UTF-8 bytes and lowercases ASCII only.
import Base

def pick(-A: Data, b: Bool, x: A, y: A) -> A:
  match b:
    case True{}:
      x
    case False{}:
      y

# Extension key
# -------------

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

# extension_key: after the last '.', non-empty, at most 8 UTF-8 bytes
def extension_key(name: String) -> Maybe<&2, String>:
  ext.of(String.split(name, '.'))

# Categories
# ----------

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

# first row wins; the whole tail is evaluated (strict), then picked
def lookup(xs: List<&2, Ent>, +key: String) -> U32:
  match xs:
    case Nil{}:
      10
    case Ent{e, c} <> t:
      pick(U32, String.eq(e, key), c, lookup(t, key))

def categorize_ext(+key: String) -> U32:
  lookup(table(), key)

def cat.of(m: Maybe<&2, String>) -> U32:
  match m:
    case None{}:
      10
    case Some{k}:
      categorize_ext(k)

def categorize(name: String, is_dir: Bool) -> U32:
  match is_dir:
    case True{}:
      0
    case False{}:
      cat.of(extension_key(name))

# Output
# ------

def show.key(m: Maybe<&2, String>) -> String:
  match m:
    case None{}:
      "-"
    case Some{k}:
      k

def line(+name: String) -> String:
  +k = extension_key(name)
  U32.show(cat.of(k)) ++ " " ++ show.key(k)

def run.one(cs: List<&2, String>) -> IO(Unit):
  match cs:
    case Nil{}:
      IO.pure(Unit, Unit{})
    case h <> t:
      do IO<Unit>:
        IO.print(line(h))
        run.one(t)

def run(chs: List<&2, List<&2, String>>) -> IO(Unit):
  match chs:
    case Nil{}:
      IO.pure(Unit, Unit{})
    case h <> t:
      do IO<Unit>:
        run.one(h)
        run(t)

def lines.one(cs: List<&2, String>) -> String:
  match cs:
    case Nil{}:
      ""
    case h <> t:
      line(h) ++ "\n" ++ lines.one(t)
