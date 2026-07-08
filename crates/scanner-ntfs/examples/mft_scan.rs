//! Live MFT scan harness (elevation required). Prints totals, throughput,
//! and per-stage timings; `--walker` runs the generic walker on the same
//! path for a direct comparison. `--dirs <out.csv>` additionally builds the
//! tree and dumps per-top-level-entry aggregates (for parity diffs against
//! other tools' exports, e.g. WizTree).
//!
//! Run from an elevated terminal:
//! `cargo run --release -p mathom-scanner-ntfs --features mft-backend --example mft_scan -- C:\`

#[cfg(all(windows, feature = "mft-backend"))]
fn main() {
    use std::time::Instant;

    use mathom_scanner::{GenericScanner, ScanEvent, ScanOptions, Scanner};
    use mathom_scanner_ntfs::MftScanner;

    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let dirs_out = match args.iter().position(|a| a == "--dirs") {
        Some(i) if i + 1 < args.len() => {
            let v = args.remove(i + 1);
            args.remove(i);
            Some(v)
        }
        Some(_) => {
            eprintln!("--dirs needs an output path");
            std::process::exit(2);
        }
        None => None,
    };
    let walker = args.iter().any(|a| a == "--walker");
    let path = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "C:\\".to_string());

    // SAFETY: single-threaded start of main, before any scan thread spawns.
    unsafe { std::env::set_var("MATHOM_MFT_TIMINGS", "1") };

    let options = ScanOptions::new(&path);
    let started = Instant::now();
    let (label, handle) = if walker {
        ("generic walker", GenericScanner.scan(options))
    } else {
        match MftScanner::probe(std::path::Path::new(&path)) {
            Some(s) => ("mft", s.scan(options)),
            None => {
                eprintln!(
                    "MFT backend unavailable for {path} — not NTFS, or not elevated. \
                     Run from an elevated terminal, or pass --walker."
                );
                std::process::exit(2);
            }
        }
    };

    println!("scanning {path} with the {label} backend…");
    let mut builder = dirs_out.as_ref().map(|_| mathom_core::TreeBuilder::new());
    let mut done_stats = None;
    let mut batches = 0u64;
    let mut entries = 0u64;
    for event in handle.events().iter() {
        match event {
            ScanEvent::Batch(b) => {
                batches += 1;
                entries += b.len() as u64;
                if let Some(builder) = builder.as_mut() {
                    builder.add_batch(&b);
                }
            }
            ScanEvent::Progress(p) => {
                eprint!(
                    "\r  {:>10} files  {:>9} dirs  {:>8.1} GiB",
                    p.files,
                    p.dirs,
                    p.bytes as f64 / (1u64 << 30) as f64
                );
            }
            ScanEvent::DirError { id: 0, message } => eprintln!("\nroot error: {message}"),
            ScanEvent::DirError { .. } => {}
            ScanEvent::Done(stats) => {
                let secs = stats.elapsed.as_secs_f64();
                println!(
                    "\n{label}: {} files + {} dirs = {} entries in {batches} batches",
                    stats.files, stats.dirs, entries
                );
                println!(
                    "  {:.2} GiB logical, {} errors, {:.3}s ({:.0} entries/s), wall {:.3}s",
                    stats.bytes as f64 / (1u64 << 30) as f64,
                    stats.errors,
                    secs,
                    (stats.files + stats.dirs) as f64 / secs,
                    started.elapsed().as_secs_f64(),
                );
                done_stats = Some(stats);
            }
        }
    }

    if let (Some(builder), Some(out)) = (builder, dirs_out) {
        let stats = done_stats.expect("scan ended without a Done event");
        let tree = builder.finish();
        dump_top_level(&tree, &stats, label, started.elapsed().as_secs_f64(), &out);
        println!("top-level aggregates written to {out}");
    }
}

/// One CSV row per direct child of the root (dirs and loose files), sorted by
/// allocated desc, plus a `<root>` total row. Counts are recursive and exclude
/// the row's own directory.
#[cfg(all(windows, feature = "mft-backend"))]
fn dump_top_level(
    tree: &mathom_core::Tree,
    stats: &mathom_scanner::ScanStats,
    label: &str,
    wall_secs: f64,
    out: &str,
) {
    use std::fmt::Write as _;

    use mathom_core::{NodeId, Tree};

    fn subtree_counts(tree: &Tree, id: NodeId) -> (u64, u64) {
        let (mut files, mut dirs) = (0u64, 0u64);
        let mut stack: Vec<NodeId> = tree.children(id).collect();
        while let Some(n) = stack.pop() {
            if tree.node(n).is_dir() {
                dirs += 1;
                stack.extend(tree.children(n));
            } else {
                files += 1;
            }
        }
        (files, dirs)
    }

    fn csv_quote(s: &str) -> String {
        format!("\"{}\"", s.replace('"', "\"\""))
    }

    let mut csv = String::new();
    let _ = writeln!(
        csv,
        "# backend={label} files={} dirs={} bytes={} scan_secs={:.3} wall_secs={wall_secs:.3}",
        stats.files,
        stats.dirs,
        stats.bytes,
        stats.elapsed.as_secs_f64(),
    );
    csv.push_str("name,kind,logical,allocated,files,dirs\n");

    let root = tree.node(Tree::ROOT);
    let _ = writeln!(
        csv,
        "\"<root>\",dir,{},{},{},{}",
        root.size, root.allocated, stats.files, stats.dirs
    );

    let mut top: Vec<NodeId> = tree.children(Tree::ROOT).collect();
    top.sort_by_key(|&id| std::cmp::Reverse(tree.node(id).allocated));
    for id in top {
        let node = tree.node(id);
        let (files, dirs) = subtree_counts(tree, id);
        let _ = writeln!(
            csv,
            "{},{},{},{},{files},{dirs}",
            csv_quote(tree.name(id)),
            if node.is_dir() { "dir" } else { "file" },
            node.size,
            node.allocated,
        );
    }
    std::fs::write(out, csv).expect("failed to write --dirs output");
}

#[cfg(not(all(windows, feature = "mft-backend")))]
fn main() {
    eprintln!("mft_scan needs Windows and --features mft-backend");
}
