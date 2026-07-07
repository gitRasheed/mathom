//! `MftScanner`: the `Scanner`-trait backend that reads the raw $MFT.
//!
//! Pipeline: a reader thread streams the $MFT's extents into a small ring
//! of aligned buffers (next read in flight while the previous buffer
//! parses); the scan thread sweeps each buffer with rayon, then assembles
//! and emits batches. Same `ScanEvent` contract as the generic walker —
//! the UI never learns which backend ran.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Sender, bounded};
use mathom_scanner::{ScanEvent, ScanHandle, ScanOptions, ScanProgress, ScanStats, Scanner};

use crate::assemble::assemble;
use crate::pipeline::Sweep;
use crate::volume::{AlignedBuf, MftMap, Volume, is_ntfs, locate, map_mft};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
/// 16 MiB per read: a multiple of every record/cluster size, big enough to
/// saturate NVMe sequential reads.
const BUF_BYTES: usize = 16 * 1024 * 1024;
/// Buffers in flight: one being read, one being parsed, two queued.
const POOL: usize = 4;

pub struct MftScanner;

impl MftScanner {
    /// Cheap availability gate: NTFS volume + openable raw-volume handle
    /// (elevation) + sane geometry. `None` means "use the generic walker".
    pub fn probe(root: &Path) -> Option<MftScanner> {
        let loc = locate(root).ok()?;
        if !is_ntfs(&loc.mount) {
            return None;
        }
        let volume = Volume::open(&loc.mount).ok()?; // access denied ⇒ not elevated
        map_mft(&volume, &loc.mount).ok()?;
        Some(MftScanner)
    }
}

impl Scanner for MftScanner {
    fn scan(&self, options: ScanOptions) -> ScanHandle {
        mathom_scanner::spawn_scan_thread("mathom-mft-scan", options, run_scan)
    }
}

fn run_scan(options: ScanOptions, tx: Sender<ScanEvent>, cancel: Arc<AtomicBool>) {
    let start = Instant::now();
    match scan_inner(&options, &tx, &cancel, start) {
        Ok(stats) => {
            let _ = tx.send(ScanEvent::Done(stats));
        }
        Err(message) => {
            // Setup failed after the probe said yes (rare race, corrupt
            // volume): surface it the way the walker surfaces a dead root.
            let _ = tx.send(ScanEvent::DirError { id: 0, message });
            let _ = tx.send(ScanEvent::Done(ScanStats {
                errors: 1,
                elapsed: start.elapsed(),
                cancelled: cancel.load(Ordering::Relaxed),
                ..ScanStats::default()
            }));
        }
    }
}

fn scan_inner(
    options: &ScanOptions,
    tx: &Sender<ScanEvent>,
    cancel: &AtomicBool,
    start: Instant,
) -> Result<ScanStats, String> {
    // Set MATHOM_MFT_TIMINGS=1 for per-stage numbers on stderr (the
    // benchmark harness relies on this to keep regressions attributable).
    let timings = std::env::var_os("MATHOM_MFT_TIMINGS").is_some();
    let loc = locate(&options.root)?;
    if !is_ntfs(&loc.mount) {
        return Err(format!("{} is not on an NTFS volume", loc.mount));
    }
    let volume = Volume::open(&loc.mount)?;
    let map = map_mft(&volume, &loc.mount)?;
    let t_mapped = Instant::now();

    let record_size = map.geometry.record_size as usize;
    let mut sweep = Sweep::new(map.total_records, record_size)?;

    // Reader thread streams extents; this thread parses. The ring of
    // buffers travels full→parse→empty→read.
    let (full_tx, full_rx) = bounded::<ReaderMsg>(POOL);
    let (empty_tx, empty_rx) = bounded::<AlignedBuf>(POOL);
    for _ in 0..POOL {
        let _ = empty_tx.send(AlignedBuf::new(BUF_BYTES));
    }

    let whole_volume = loc.components.is_empty();
    let mut progress = ScanProgress::default();
    let mut last_tick = Instant::now();

    std::thread::scope(|scope| -> Result<(), String> {
        // The closure must own its sender: if this parse loop panics, the
        // drop unblocks the reader's `empty_rx.recv()` so the scope's
        // implicit join can finish and the panic can propagate (otherwise
        // the outer frame keeps `empty_tx` alive and the join deadlocks).
        let empty_tx = empty_tx;
        let map_ref = &map;
        scope.spawn(move || read_mft(volume, map_ref, full_tx, empty_rx, cancel));

        let mut read_error = None;
        for msg in full_rx.iter() {
            match msg {
                ReaderMsg::Chunk(first_record, mut buf, valid) => {
                    let counts = sweep.consume(first_record, &mut buf.as_mut_slice()[..valid]);
                    let _ = empty_tx.send(buf); // reader gone = fine
                    progress.files += counts.files;
                    progress.dirs += counts.dirs;
                    progress.bytes += counts.bytes;
                    // Live counters only make sense for whole-volume scans;
                    // subtree totals arrive with Done.
                    if whole_volume && last_tick.elapsed() >= PROGRESS_INTERVAL {
                        let _ = tx.send(ScanEvent::Progress(progress));
                        last_tick = Instant::now();
                    }
                }
                ReaderMsg::Failed(e) => read_error = Some(e),
            }
        }
        match read_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    })?;

    if cancel.load(Ordering::Relaxed) {
        return Ok(ScanStats {
            elapsed: start.elapsed(),
            cancelled: true,
            ..ScanStats::default()
        });
    }
    let t_swept = Instant::now();

    let table = sweep.finish();
    let components: Vec<&str> = loc.components.iter().map(String::as_str).collect();
    let root_display = options.root.to_string_lossy();
    let stats = assemble(
        &table,
        &components,
        &root_display,
        options.batch_size,
        |b| !cancel.load(Ordering::Relaxed) && tx.send(ScanEvent::Batch(b)).is_ok(),
    )
    .map_err(|e| e.to_string())?;

    if timings {
        eprintln!(
            "mft timings: map={:.1?} read+parse={:.1?} ({:.2} GB/s over {} MiB, {} records) \
             patch+assemble+emit={:.1?} total={:.1?}",
            t_mapped - start,
            t_swept - t_mapped,
            map.mft_bytes as f64 / 1e9 / (t_swept - t_mapped).as_secs_f64(),
            map.mft_bytes >> 20,
            map.total_records,
            t_swept.elapsed(),
            start.elapsed(),
        );
    }

    Ok(ScanStats {
        files: stats.files,
        dirs: stats.dirs,
        bytes: stats.bytes,
        errors: table.torn + table.dropped_patches + stats.orphans,
        elapsed: start.elapsed(),
        cancelled: stats.cancelled || cancel.load(Ordering::Relaxed),
    })
}

enum ReaderMsg {
    /// Records starting at this record number; `usize` = valid byte count.
    Chunk(usize, AlignedBuf, usize),
    Failed(String),
}

/// Streams the $MFT extent by extent in `BUF_BYTES` chunks. Every read
/// offset/length is record- (and therefore sector-) aligned: `mft_bytes`
/// rounds down to whole records, and a partial trailing record can't hold
/// a live FILE record anyway. Offset math can't overflow — `map`'s extents
/// are validated against the device size (`plan_mft_read`).
fn read_mft(
    volume: Volume,
    map: &MftMap,
    full_tx: Sender<ReaderMsg>,
    empty_rx: crossbeam_channel::Receiver<AlignedBuf>,
    cancel: &AtomicBool,
) {
    let cluster = map.geometry.cluster_size as u64;
    let record = map.geometry.record_size as u64;
    let mut remaining = map.total_records as u64 * record;
    let mut next_record = 0usize;

    for extent in &map.extents {
        let mut disk_off = extent.lcn * cluster;
        let mut left_in_extent = extent.clusters * cluster;
        while left_in_extent > 0 && remaining > 0 {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let Ok(mut buf) = empty_rx.recv() else { return };
            // All three are record-multiples (cluster % record == 0 is
            // guaranteed by map_mft), so reads stay aligned end to end.
            let want = (BUF_BYTES as u64).min(left_in_extent).min(remaining) as usize;
            if let Err(e) = volume.read_at(disk_off, &mut buf.as_mut_slice()[..want]) {
                let _ = full_tx.send(ReaderMsg::Failed(e));
                return;
            }
            if full_tx
                .send(ReaderMsg::Chunk(next_record, buf, want))
                .is_err()
            {
                return;
            }
            next_record += want / record as usize;
            disk_off += want as u64;
            left_in_extent -= want as u64;
            remaining = remaining.saturating_sub(want as u64);
        }
        if remaining == 0 {
            break;
        }
    }
}
