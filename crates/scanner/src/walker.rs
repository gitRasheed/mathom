//! Generic parallel fallback walker.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

use crossbeam_channel::{RecvTimeoutError, Sender, bounded};
use mathom_core::{EntryBatch, EntryFlags, FileEntry};

use crate::{ScanEvent, ScanHandle, ScanOptions, ScanProgress, ScanStats, Scanner};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Default)]
pub struct GenericScanner;

impl Scanner for GenericScanner {
    fn scan(&self, options: ScanOptions) -> ScanHandle {
        crate::spawn_scan_thread("mathom-scan", options, run_scan)
    }
}

struct Ctx {
    tx: Sender<ScanEvent>,
    cancel: Arc<AtomicBool>,
    next_id: AtomicU32,
    files: AtomicU64,
    dirs: AtomicU64,
    bytes: AtomicU64,
    errors: AtomicU64,
    batch_size: usize,
}

impl Ctx {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn send(&self, event: ScanEvent) -> bool {
        if self.tx.send(event).is_err() {
            self.cancel.store(true, Ordering::Relaxed);
            return false;
        }
        true
    }

    fn progress(&self) -> ScanProgress {
        ScanProgress {
            files: self.files.load(Ordering::Relaxed),
            dirs: self.dirs.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
        }
    }
}

fn run_scan(options: ScanOptions, tx: Sender<ScanEvent>, cancel: Arc<AtomicBool>) {
    let start = Instant::now();
    let ctx = Arc::new(Ctx {
        tx,
        cancel,
        next_id: AtomicU32::new(1),
        files: AtomicU64::new(0),
        dirs: AtomicU64::new(0),
        bytes: AtomicU64::new(0),
        errors: AtomicU64::new(0),
        batch_size: options.batch_size.max(1),
    });

    let finish = |ctx: &Ctx| {
        let p = ctx.progress();
        let _ = ctx.tx.send(ScanEvent::Done(ScanStats {
            files: p.files,
            dirs: p.dirs,
            bytes: p.bytes,
            errors: ctx.errors.load(Ordering::Relaxed),
            elapsed: start.elapsed(),
            cancelled: ctx.cancelled(),
        }));
    };

    let root_meta = match fs::metadata(&options.root) {
        Ok(m) if m.is_dir() => m,
        Ok(_) | Err(_) => {
            ctx.errors.fetch_add(1, Ordering::Relaxed);
            let message = format!("{} is not a readable directory", options.root.display());
            let _ = ctx.tx.send(ScanEvent::DirError { id: 0, message });
            finish(&ctx);
            return;
        }
    };

    let mut root_batch = EntryBatch::with_capacity(1, 64);
    root_batch.push(
        &options.root.to_string_lossy(),
        FileEntry {
            path_id: 0,
            parent_id: 0,
            name_off: 0,
            name_len: 0,
            flags: EntryFlags::DIR,
            size: 0,
            allocated_size: 0,
            mtime: mtime_secs(&root_meta),
        },
    );
    ctx.dirs.fetch_add(1, Ordering::Relaxed);
    if !ctx.send(ScanEvent::Batch(root_batch)) {
        finish(&ctx);
        return;
    }

    let ticker = spawn_progress_ticker(Arc::clone(&ctx));

    // Directory enumeration is blocking-syscall bound; oversubscribe modestly.
    let threads = options.threads.unwrap_or_else(|| {
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        (cores * 4).min(64)
    });
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|i| format!("mathom-walk-{i}"))
        .build()
        .expect("failed to build scan thread pool");

    let root_path = options.root.clone();
    let walk_ctx = Arc::clone(&ctx);
    pool.scope(move |s| walk_dir(s, root_path, 0, &walk_ctx));

    drop(ticker); // stops the progress thread
    finish(&ctx);
}

fn walk_dir<'a>(scope: &rayon::Scope<'a>, path: PathBuf, dir_id: u32, ctx: &Arc<Ctx>) {
    if ctx.cancelled() {
        return;
    }
    let read_dir = match fs::read_dir(&path) {
        Ok(rd) => rd,
        Err(e) => {
            ctx.errors.fetch_add(1, Ordering::Relaxed);
            ctx.send(ScanEvent::DirError {
                id: dir_id,
                message: e.to_string(),
            });
            return;
        }
    };

    let mut batch = EntryBatch::with_capacity(64, 1024);
    let mut subdirs: Vec<(u32, PathBuf)> = Vec::new();
    let mut files = 0u64;
    let mut dirs = 0u64;
    let mut bytes = 0u64;

    for dent in read_dir {
        let Ok(dent) = dent else {
            ctx.errors.fetch_add(1, Ordering::Relaxed);
            continue;
        };
        let Ok(meta) = dent.metadata() else {
            ctx.errors.fetch_add(1, Ordering::Relaxed);
            continue;
        };

        let file_type = meta.file_type();
        let is_dir = file_type.is_dir();
        let is_reparse = file_type.is_symlink();
        let id = ctx.next_id.fetch_add(1, Ordering::Relaxed);

        let mut flags = EntryFlags(0);
        let size;
        let mut allocated = 0;
        if is_dir {
            flags.insert(EntryFlags::DIR);
            size = 0;
            dirs += 1;
        } else if is_reparse {
            flags.insert(EntryFlags::REPARSE);
            size = 0;
            files += 1;
        } else {
            size = meta.len();
            let attr_flags = allocation_flags(file_attributes(&meta));
            allocated = if attr_flags == EntryFlags(0) {
                size
            } else {
                flags = flags.union(attr_flags);
                allocated_on_disk(&dent.path()).unwrap_or(size)
            };
            files += 1;
            bytes += size;
        }
        if is_system(&meta) {
            flags.insert(EntryFlags::SYSTEM);
        }

        batch.push(
            &dent.file_name().to_string_lossy(),
            FileEntry {
                path_id: id,
                parent_id: dir_id,
                name_off: 0,
                name_len: 0,
                flags,
                size,
                allocated_size: allocated,
                mtime: mtime_secs(&meta),
            },
        );
        if is_dir {
            subdirs.push((id, dent.path()));
        }

        if batch.len() >= ctx.batch_size {
            let full = std::mem::take(&mut batch);
            if !ctx.send(ScanEvent::Batch(full)) {
                return;
            }
        }
    }

    // TreeBuilder requires parent entries before grandchildren.
    if !batch.is_empty() && !ctx.send(ScanEvent::Batch(batch)) {
        return;
    }

    ctx.files.fetch_add(files, Ordering::Relaxed);
    ctx.dirs.fetch_add(dirs, Ordering::Relaxed);
    ctx.bytes.fetch_add(bytes, Ordering::Relaxed);

    for (id, sub_path) in subdirs {
        let ctx = Arc::clone(ctx);
        scope.spawn(move |s| walk_dir(s, sub_path, id, &ctx));
    }
}

fn spawn_progress_ticker(ctx: Arc<Ctx>) -> TickerGuard {
    let (stop_tx, stop_rx) = bounded::<()>(0);
    let handle = std::thread::Builder::new()
        .name("mathom-progress".into())
        .spawn(move || {
            while let Err(RecvTimeoutError::Timeout) = stop_rx.recv_timeout(PROGRESS_INTERVAL) {
                if !ctx.send(ScanEvent::Progress(ctx.progress())) {
                    break;
                }
            }
        })
        .expect("failed to spawn progress thread");
    TickerGuard {
        stop: Some(stop_tx),
        handle: Some(handle),
    }
}

struct TickerGuard {
    stop: Option<Sender<()>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for TickerGuard {
    fn drop(&mut self) {
        // Signal before joining, or the join waits on a ticker that never stops.
        drop(self.stop.take());
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[cfg(windows)]
fn is_system(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x0000_0004;
    meta.file_attributes() & FILE_ATTRIBUTE_SYSTEM != 0
}

#[cfg(not(windows))]
fn is_system(_meta: &fs::Metadata) -> bool {
    false
}

#[cfg(windows)]
fn file_attributes(meta: &fs::Metadata) -> u32 {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes()
}

#[cfg(not(windows))]
fn file_attributes(_meta: &fs::Metadata) -> u32 {
    0
}

fn allocation_flags(attrs: u32) -> EntryFlags {
    const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x0200;
    const FILE_ATTRIBUTE_COMPRESSED: u32 = 0x0800;
    const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
    const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x4_0000;
    const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

    let mut f = EntryFlags(0);
    if attrs & FILE_ATTRIBUTE_SPARSE_FILE != 0 {
        f.insert(EntryFlags::SPARSE);
    }
    if attrs & FILE_ATTRIBUTE_COMPRESSED != 0 {
        f.insert(EntryFlags::COMPRESSED);
    }
    if attrs
        & (FILE_ATTRIBUTE_OFFLINE
            | FILE_ATTRIBUTE_RECALL_ON_OPEN
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
        != 0
    {
        f.insert(EntryFlags::PLACEHOLDER);
    }
    f
}

/// True on-disk allocation via `GetCompressedFileSizeW` — a path-only query.
/// Never open these files for data: that triggers Defender scans and, for
/// cloud placeholders, hydration (mass-downloading the user's OneDrive).
#[cfg(windows)]
fn allocated_on_disk(path: &std::path::Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::Foundation::GetLastError;
    use windows::Win32::Storage::FileSystem::{GetCompressedFileSizeW, INVALID_FILE_SIZE};
    use windows::core::PCWSTR;

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut high = 0u32;
    // SAFETY: NUL-terminated path and valid out-pointer for the call.
    let low = unsafe { GetCompressedFileSizeW(PCWSTR(wide.as_ptr()), Some(&mut high)) };
    if low == INVALID_FILE_SIZE && unsafe { GetLastError() }.is_err() {
        return None;
    }
    Some((high as u64) << 32 | low as u64)
}

#[cfg(not(windows))]
fn allocated_on_disk(_path: &std::path::Path) -> Option<u64> {
    None
}

fn mtime_secs(meta: &fs::Metadata) -> i64 {
    let Ok(modified) = meta.modified() else {
        return 0;
    };
    match modified.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_flags_map_the_documented_attribute_bits() {
        assert_eq!(allocation_flags(0), EntryFlags(0));
        assert_eq!(allocation_flags(0x0200), EntryFlags::SPARSE);
        assert_eq!(allocation_flags(0x0800), EntryFlags::COMPRESSED);
        assert_eq!(allocation_flags(0x1000), EntryFlags::PLACEHOLDER); // OFFLINE
        assert_eq!(allocation_flags(0x4_0000), EntryFlags::PLACEHOLDER); // RECALL_ON_OPEN
        assert_eq!(allocation_flags(0x40_0000), EntryFlags::PLACEHOLDER); // RECALL_ON_DATA_ACCESS
        // A dehydrated OneDrive file carries several at once.
        assert_eq!(
            allocation_flags(0x0200 | 0x1000 | 0x40_0000),
            EntryFlags::SPARSE.union(EntryFlags::PLACEHOLDER)
        );
        // Unrelated attributes (readonly, hidden, system, directory) map to nothing.
        assert_eq!(allocation_flags(0x1 | 0x2 | 0x4 | 0x10), EntryFlags(0));
    }
}
