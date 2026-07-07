//! mathom-scanner: the `Scanner` trait every backend implements, plus the
//! portable generic parallel walker. Streaming is channel-based (no async
//! runtime here); the Tauri layer bridges the receiver to events.

mod walker;

pub use walker::GenericScanner;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, bounded};
use mathom_core::EntryBatch;
use mathom_core::tree::NodeId;

#[derive(Clone, Debug)]
pub struct ScanOptions {
    pub root: PathBuf,
    /// Worker threads. Defaults to available parallelism.
    pub threads: Option<usize>,
    /// Max entries per emitted batch (a directory's children are split
    /// across batches if larger).
    pub batch_size: usize,
}

impl ScanOptions {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        ScanOptions {
            root: root.into(),
            threads: None,
            batch_size: 4096,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ScanProgress {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ScanStats {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    /// Directories that could not be read.
    pub errors: u64,
    pub elapsed: Duration,
    pub cancelled: bool,
}

#[derive(Debug)]
pub enum ScanEvent {
    Batch(EntryBatch),
    /// Directory `id` could not be read; its children will never arrive.
    DirError {
        id: NodeId,
        message: String,
    },
    /// Periodic (~100ms) totals for live UI.
    Progress(ScanProgress),
    /// Always the final event, even when cancelled or failed.
    Done(ScanStats),
}

/// A running scan: drain `events()` until `Done`. Dropping the handle without
/// draining cancels the scan.
pub struct ScanHandle {
    events: Receiver<ScanEvent>,
    cancel: Arc<AtomicBool>,
}

impl ScanHandle {
    pub fn new(events: Receiver<ScanEvent>, cancel: Arc<AtomicBool>) -> Self {
        ScanHandle { events, cancel }
    }

    pub fn events(&self) -> &Receiver<ScanEvent> {
        &self.events
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for ScanHandle {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// A scan backend (generic walker, NTFS MFT reader, ...). Emits the root as
/// entry 0, then batches where every entry's parent has already been sent.
pub trait Scanner: Send + Sync {
    fn scan(&self, options: ScanOptions) -> ScanHandle;
}

/// Spawns a scan worker thread wired to a fresh handle. Backends route
/// their `scan()` through this so the `Done`-is-always-last contract holds
/// for every exit: `body` must send `Done` as its final event when it
/// returns normally, and a panicking `body` becomes a root `DirError` plus
/// a failed `Done` instead of a channel that silently closes (the default
/// panic hook still prints the backtrace first).
pub fn spawn_scan_thread(
    name: &str,
    options: ScanOptions,
    body: fn(ScanOptions, Sender<ScanEvent>, Arc<AtomicBool>),
) -> ScanHandle {
    let (tx, rx) = bounded(512);
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    let guard_cancel = Arc::clone(&cancel);
    let guard_tx = tx.clone();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let start = std::time::Instant::now();
            let run = std::panic::AssertUnwindSafe(|| body(options, tx, worker_cancel));
            if let Err(panic) = std::panic::catch_unwind(run) {
                let message = format!("internal scan error: {}", panic_message(panic.as_ref()));
                let _ = guard_tx.send(ScanEvent::DirError { id: 0, message });
                let _ = guard_tx.send(ScanEvent::Done(ScanStats {
                    errors: 1,
                    elapsed: start.elapsed(),
                    cancelled: guard_cancel.load(Ordering::Relaxed),
                    ..ScanStats::default()
                }));
            }
        })
        .expect("failed to spawn scan thread");
    ScanHandle::new(rx, cancel)
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> &str {
    panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("scan worker panicked")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panicking_backend_still_delivers_done() {
        fn exploding(_: ScanOptions, tx: Sender<ScanEvent>, _: Arc<AtomicBool>) {
            let _ = tx.send(ScanEvent::Progress(ScanProgress::default()));
            panic!("backend bug");
        }
        let handle = spawn_scan_thread("test-explode", ScanOptions::new("."), exploding);
        let events: Vec<ScanEvent> = handle.events().iter().collect();

        let Some(ScanEvent::Done(stats)) = events.last() else {
            panic!("scan ended without a Done event: {events:?}");
        };
        assert_eq!(stats.errors, 1);
        assert!(
            events.iter().any(|e| matches!(
                e,
                ScanEvent::DirError { id: 0, message } if message.contains("backend bug")
            )),
            "the panic message should surface as a root DirError: {events:?}"
        );
    }
}
