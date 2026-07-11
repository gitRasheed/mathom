//! Tree export: streams rows to any `io::Write` (file, clipboard buffer).
//! Rows are node facts — `hide_system` prunes rows but never re-aggregates
//! sizes, so a pruned parent still reports its full subtree total.

use std::borrow::Cow;
use std::cmp::Reverse;
use std::io::{self, Write};

use crate::EntryFlags;
use crate::tree::{Node, NodeId, Tree};

#[derive(Clone, Copy, Debug, Default)]
pub struct ExportOptions {
    /// Depth relative to the export root (root = 0); `None` = unlimited.
    pub max_depth: Option<u32>,
    pub dirs_only: bool,
    pub hide_system: bool,
}

/// `path,kind,size,allocated,modified,items` — one row per node, children
/// size-descending. Returns the row count.
pub fn write_csv(
    tree: &Tree,
    root: NodeId,
    opts: &ExportOptions,
    w: &mut impl Write,
) -> io::Result<u64> {
    writeln!(w, "path,kind,size,allocated,modified,items")?;
    let mut rows = 0u64;
    walk(tree, root, opts, |path, node| {
        rows += 1;
        writeln!(
            w,
            "{},{},{},{},{},{}",
            csv_field(path),
            kind(node),
            node.size,
            node.allocated,
            modified(node.mtime),
            node.items
        )
    })?;
    Ok(rows)
}

/// One flat JSON array of row objects (same fields as the CSV).
pub fn write_json(
    tree: &Tree,
    root: NodeId,
    opts: &ExportOptions,
    w: &mut impl Write,
) -> io::Result<u64> {
    w.write_all(b"[")?;
    let mut rows = 0u64;
    walk(tree, root, opts, |path, node| {
        let sep = if rows == 0 { "\n" } else { ",\n" };
        rows += 1;
        write!(
            w,
            "{sep}{{\"path\":\"{}\",\"kind\":\"{}\",\"size\":{},\"allocated\":{},\"modified\":\"{}\",\"items\":{}}}",
            json_escape(path),
            kind(node),
            node.size,
            node.allocated,
            modified(node.mtime),
            node.items
        )
    })?;
    w.write_all(b"\n]\n")?;
    Ok(rows)
}

fn walk(
    tree: &Tree,
    root: NodeId,
    opts: &ExportOptions,
    mut emit: impl FnMut(&str, &Node) -> io::Result<()>,
) -> io::Result<()> {
    let mut path = String::new();
    // (id, depth, parent path length): truncate-then-append rebuilds each
    // node's path without per-node allocation.
    let mut stack: Vec<(NodeId, u32, usize)> = vec![(root, 0, 0)];
    while let Some((id, depth, parent_len)) = stack.pop() {
        path.truncate(parent_len);
        append_component(&mut path, tree.name(id));
        let node = tree.node(id);
        emit(&path, node)?;

        if node.is_dir() && opts.max_depth.is_none_or(|m| depth < m) {
            let mut kids: Vec<NodeId> =
                tree.children(id).filter(|&c| keep(tree, c, opts)).collect();
            kids.sort_by_key(|&c| (Reverse(tree.node(c).size), c));
            let len = path.len();
            // Reverse push: the largest child pops first.
            for &c in kids.iter().rev() {
                stack.push((c, depth + 1, len));
            }
        }
    }
    Ok(())
}

fn keep(tree: &Tree, id: NodeId, opts: &ExportOptions) -> bool {
    let node = tree.node(id);
    if opts.hide_system && node.flags.contains(EntryFlags::SYSTEM) {
        return false;
    }
    !opts.dirs_only || node.is_dir()
}

/// Same join rule as `Tree::path`: no doubling after a root like `C:\`.
fn append_component(path: &mut String, name: &str) {
    if !path.is_empty() && !path.ends_with(std::path::MAIN_SEPARATOR) {
        path.push(std::path::MAIN_SEPARATOR);
    }
    path.push_str(name);
}

fn kind(node: &Node) -> &'static str {
    if node.is_dir() { "dir" } else { "file" }
}

fn modified(mtime: i64) -> String {
    if mtime == 0 {
        return String::new();
    }
    let days = mtime.div_euclid(86_400);
    let sod = mtime.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        sod / 3600,
        (sod / 60) % 60,
        sod % 60
    )
}

/// Days since 1970-01-01 → (year, month, day). Howard Hinnant's civil
/// calendar algorithm; exact over the whole i64-seconds range we store.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn csv_field(s: &str) -> Cow<'_, str> {
    if s.contains([',', '"', '\n', '\r']) {
        Cow::Owned(format!("\"{}\"", s.replace('"', "\"\"")))
    } else {
        Cow::Borrowed(s)
    }
}

fn json_escape(s: &str) -> Cow<'_, str> {
    if !s.contains(['"', '\\']) && !s.chars().any(|c| (c as u32) < 0x20) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    Cow::Owned(out)
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

    /// root(0)/docs(1): a.pdf(2)=100, b.PDF(3)=50, notes(4)=8;
    /// media(5): movie.mkv(6)=500; sys(7)[SYSTEM]: pagefile.sys(8)=999; raw.bin(9)=30
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
            entry(7, 0, EntryFlags::DIR.union(EntryFlags::SYSTEM), 0),
        );
        batch.push("raw.bin", entry(9, 0, FILE, 30));
        b.add_batch(&batch);
        let mut batch = EntryBatch::default();
        batch.push("a.pdf", entry(2, 1, FILE, 100));
        batch.push("b.PDF", entry(3, 1, FILE, 50));
        batch.push("notes", entry(4, 1, FILE, 8));
        batch.push("movie.mkv", entry(6, 5, FILE, 500));
        batch.push("pagefile.sys", entry(8, 7, FILE, 999));
        b.add_batch(&batch);
        b.finish()
    }

    /// Goldens are written with `/`; translate to the platform separator.
    fn sep(golden: &str) -> String {
        golden.replace('/', std::path::MAIN_SEPARATOR_STR)
    }

    /// Same, but for JSON goldens where a `\` separator arrives escaped.
    fn json_sep(golden: &str) -> String {
        let s = if std::path::MAIN_SEPARATOR == '\\' {
            "\\\\"
        } else {
            "/"
        };
        golden.replace('/', s)
    }

    fn csv(tree: &Tree, opts: &ExportOptions) -> (String, u64) {
        let mut buf = Vec::new();
        let rows = write_csv(tree, 0, opts, &mut buf).unwrap();
        (String::from_utf8(buf).unwrap(), rows)
    }

    #[test]
    fn csv_full_export_is_exact() {
        let (got, rows) = csv(&sample(), &ExportOptions::default());
        let want = sep("path,kind,size,allocated,modified,items
root,dir,1687,1687,2023-11-14T22:13:20Z,9
root/sys,dir,999,999,2023-11-14T22:13:20Z,1
root/sys/pagefile.sys,file,999,999,2023-11-14T22:13:20Z,0
root/media,dir,500,500,2023-11-14T22:13:20Z,1
root/media/movie.mkv,file,500,500,2023-11-14T22:13:20Z,0
root/docs,dir,158,158,2023-11-14T22:13:20Z,3
root/docs/a.pdf,file,100,100,2023-11-14T22:13:20Z,0
root/docs/b.PDF,file,50,50,2023-11-14T22:13:20Z,0
root/docs/notes,file,8,8,2023-11-14T22:13:20Z,0
root/raw.bin,file,30,30,2023-11-14T22:13:20Z,0
");
        assert_eq!(got, want);
        assert_eq!(rows, 10);
    }

    #[test]
    fn json_depth_one_is_exact() {
        let mut buf = Vec::new();
        let opts = ExportOptions {
            max_depth: Some(1),
            ..Default::default()
        };
        let rows = write_json(&sample(), 0, &opts, &mut buf).unwrap();
        let want = json_sep(
            r#"[
{"path":"root","kind":"dir","size":1687,"allocated":1687,"modified":"2023-11-14T22:13:20Z","items":9},
{"path":"root/sys","kind":"dir","size":999,"allocated":999,"modified":"2023-11-14T22:13:20Z","items":1},
{"path":"root/media","kind":"dir","size":500,"allocated":500,"modified":"2023-11-14T22:13:20Z","items":1},
{"path":"root/docs","kind":"dir","size":158,"allocated":158,"modified":"2023-11-14T22:13:20Z","items":3},
{"path":"root/raw.bin","kind":"file","size":30,"allocated":30,"modified":"2023-11-14T22:13:20Z","items":0}
]
"#,
        );
        assert_eq!(String::from_utf8(buf).unwrap(), want);
        assert_eq!(rows, 5);
    }

    #[test]
    fn dirs_only_skips_files_everywhere() {
        let opts = ExportOptions {
            dirs_only: true,
            ..Default::default()
        };
        let (got, rows) = csv(&sample(), &opts);
        assert_eq!(rows, 4, "root + 3 dirs:\n{got}");
        assert!(!got.contains("raw.bin") && !got.contains("a.pdf"));
    }

    #[test]
    fn depth_zero_exports_only_the_root() {
        let opts = ExportOptions {
            max_depth: Some(0),
            ..Default::default()
        };
        let (got, rows) = csv(&sample(), &opts);
        assert_eq!(rows, 1);
        assert!(got.lines().nth(1).unwrap().starts_with("root,dir,1687"));
    }

    /// hide_system removes rows; it must NOT shrink the parent aggregates.
    #[test]
    fn hide_system_prunes_rows_but_keeps_aggregate_sizes() {
        let opts = ExportOptions {
            hide_system: true,
            ..Default::default()
        };
        let (got, rows) = csv(&sample(), &opts);
        assert_eq!(rows, 8, "sys subtree (2 rows) pruned:\n{got}");
        assert!(!got.contains("sys"));
        assert!(got.contains("root,dir,1687"), "root total unchanged");
    }

    #[test]
    fn csv_fields_with_commas_and_quotes_are_quoted() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn json_escape_handles_backslashes_quotes_and_controls() {
        assert_eq!(json_escape("C:\\Users"), "C:\\\\Users");
        assert_eq!(json_escape("a\"b"), "a\\\"b");
        assert_eq!(json_escape("tab\there"), "tab\\u0009here");
    }

    #[test]
    fn modified_formats_iso_utc_and_hides_unknown() {
        assert_eq!(modified(0), "");
        assert_eq!(modified(86_400), "1970-01-02T00:00:00Z");
        assert_eq!(modified(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(modified(-1), "1969-12-31T23:59:59Z");
    }
}
