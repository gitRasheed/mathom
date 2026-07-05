//! mathom-scanner: the `Scanner` trait every backend implements, plus the
//! portable generic parallel walker. Streaming is channel-based (no async
//! runtime here); the Tauri layer bridges the receiver to events.

mod walker;

pub use walker::GenericScanner;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossbeam_channel::Receiver;
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
    DirError { id: NodeId, message: String },
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
