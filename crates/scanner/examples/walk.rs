//! Walker throughput measurement against a real volume.
//!
//! cargo run --release -p mathom-scanner --example walk -- <path> [threads]

use std::time::Instant;

use mathom_core::TreeBuilder;
use mathom_scanner::{GenericScanner, ScanEvent, ScanOptions, Scanner};

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("usage: walk <path> [threads]");
    let threads: Option<usize> = args.next().and_then(|t| t.parse().ok());

    let mut options = ScanOptions::new(&root);
    options.threads = threads;

    let start = Instant::now();
    let handle = GenericScanner.scan(options);
    let mut builder = TreeBuilder::new();
    let mut stats = None;
    for event in handle.events().iter() {
        match event {
            ScanEvent::Batch(b) => builder.add_batch(&b),
            ScanEvent::DirError { id, .. } => builder.mark_error(id),
            ScanEvent::Progress(_) => {}
            ScanEvent::Done(s) => stats = Some(s),
        }
    }
    let stats = stats.expect("scan ended without Done");
    let tree = builder.finish();
    let total = stats.files + stats.dirs;
    println!(
        "threads={} files={} dirs={} bytes={} errors={} nodes={} scan={:.2?} total={:.2?} ({:.0} entries/s)",
        threads.map_or_else(|| "auto".into(), |t| t.to_string()),
        stats.files,
        stats.dirs,
        stats.bytes,
        stats.errors,
        tree.len(),
        stats.elapsed,
        start.elapsed(),
        total as f64 / stats.elapsed.as_secs_f64(),
    );
}
