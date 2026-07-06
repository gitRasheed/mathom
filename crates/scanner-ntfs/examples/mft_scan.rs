//! Live MFT scan harness (elevation required). Prints totals, throughput,
//! and per-stage timings; `--walker` runs the generic walker on the same
//! path for a direct comparison.
//!
//! Run from an elevated terminal:
//! `cargo run --release -p mathom-scanner-ntfs --features mft-backend --example mft_scan -- C:\`

#[cfg(all(windows, feature = "mft-backend"))]
fn main() {
    use std::time::Instant;

    use mathom_scanner::{GenericScanner, ScanEvent, ScanOptions, Scanner};
    use mathom_scanner_ntfs::MftScanner;

    let args: Vec<String> = std::env::args().skip(1).collect();
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
    let mut batches = 0u64;
    let mut entries = 0u64;
    for event in handle.events().iter() {
        match event {
            ScanEvent::Batch(b) => {
                batches += 1;
                entries += b.len() as u64;
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
            }
        }
    }
}

#[cfg(not(all(windows, feature = "mft-backend")))]
fn main() {
    eprintln!("mft_scan needs Windows and --features mft-backend");
}
