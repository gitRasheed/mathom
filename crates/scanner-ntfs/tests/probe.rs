//! Smoke tests for the Windows layer that run without elevation: the probe
//! gate and the scanner's error funnel. Live-volume scans are exercised by
//! `examples/mft_scan.rs` (elevation required).
#![cfg(all(windows, feature = "mft-backend"))]

use std::path::Path;

use mathom_scanner::{ScanEvent, ScanOptions, Scanner};
use mathom_scanner_ntfs::MftScanner;

#[test]
fn probe_of_nonexistent_path_is_none() {
    assert!(MftScanner::probe(Path::new("Q:\\mathom\\does\\not\\exist")).is_none());
}

#[test]
fn probe_of_real_roots_never_panics() {
    // Some(_) when elevated on NTFS, None otherwise — both are correct.
    let _ = MftScanner::probe(Path::new("C:\\"));
    let _ = MftScanner::probe(&std::env::temp_dir());
}

#[test]
fn scan_of_dead_root_reports_error_then_done() {
    let handle = MftScanner.scan(ScanOptions::new("Q:\\mathom\\does\\not\\exist"));
    let events: Vec<ScanEvent> = handle.events().iter().collect();
    assert!(
        matches!(events.first(), Some(ScanEvent::DirError { id: 0, .. })),
        "first event should be the root error, got {:?}",
        events.first()
    );
    match events.last() {
        Some(ScanEvent::Done(stats)) => assert_eq!(stats.errors, 1),
        other => panic!("expected Done last, got {other:?}"),
    }
}
