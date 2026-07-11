//! Tauri bridge for scanner snapshots and tree queries.
//!
//! The tree never crosses IPC: ticks are a throttled dirty signal and the UI
//! re-queries only the slices it can see, so payloads stay O(visible rows).
//! Every query carries a scan `generation` so stale answers get dropped.
//! Lock order: never hold the builder lock and progress lock together.
//! Threading: commands that take the builder lock or touch the filesystem are
//! `(async)` (worker thread) so the window's event loop never blocks; O(1)
//! control commands stay sync and keep the main thread's serialization.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use mathom_core::export::ExportOptions;
use mathom_core::search::{FilterOverlay, SearchQuery};
use mathom_core::tree::{NodeId, Tree};
use mathom_core::{EntryFlags, TreeBuilder, TreemapOptions, Viewport, treemap};
use mathom_scanner::{GenericScanner, ScanEvent, ScanHandle, ScanOptions, Scanner};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

const TICK_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Default)]
pub struct AppState {
    last_generation: AtomicU64,
    current: Mutex<Option<Arc<Session>>>,
}

struct Session {
    generation: u64,
    started: Instant,
    builder: RwLock<TreeBuilder>,
    progress: Mutex<Progress>,
    handle: ScanHandle,
    /// View-filter overlay cache, keyed by (query, hide_system). Cleared on
    /// delete (ids shift); overlay reads are bounds-tolerant, so a tree that
    /// grew past the snapshot degrades to "filtered out", never a panic.
    /// Lock order: only ever taken while holding the builder lock.
    filter: Mutex<Option<FilterCache>>,
}

struct FilterCache {
    query: String,
    hide_system: bool,
    overlay: Arc<FilterOverlay>,
}

/// Overlay for `filter`, cached per session. `None` when the query is
/// absent or parses to nothing — callers then serve the unfiltered view.
fn overlay_for(
    session: &Session,
    tree: &Tree,
    filter: Option<&str>,
    hide_system: bool,
) -> Option<Arc<FilterOverlay>> {
    let text = filter?.trim();
    let query = SearchQuery::parse(text);
    if query.is_empty() {
        return None;
    }
    let mut cache = session.filter.lock().unwrap();
    if let Some(c) = cache.as_ref()
        && c.query == text
        && c.hide_system == hide_system
    {
        return Some(Arc::clone(&c.overlay));
    }
    let overlay = Arc::new(mathom_core::search::build_overlay(
        tree,
        &query,
        hide_system,
    ));
    *cache = Some(FilterCache {
        query: text.to_string(),
        hide_system,
        overlay: Arc::clone(&overlay),
    });
    Some(overlay)
}

#[derive(Default)]
struct Progress {
    files: u64,
    dirs: u64,
    bytes: u64,
    errors: u64,
    state: ScanState,
    root_error: Option<String>,
    finished_ms: Option<u64>,
}

#[derive(Clone, Copy, PartialEq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
enum ScanState {
    #[default]
    Idle,
    Scanning,
    Done,
    Cancelled,
    Failed,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    generation: u64,
    state: ScanState,
    files: u64,
    dirs: u64,
    bytes: u64,
    errors: u64,
    elapsed_ms: u64,
    nodes: usize,
    root_error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    id: NodeId,
    name: String,
    is_dir: bool,
    size: u64,
    allocated: u64,
    items: u32,
    mtime: i64,
    has_children: bool,
    is_reparse: bool,
    is_error: bool,
    pct: f64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirListing {
    id: NodeId,
    rows: Vec<Row>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreemapRectDto {
    id: NodeId,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    depth: u8,
    is_dir: bool,
    category: u8,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Crumb {
    id: NodeId,
    name: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResult {
    removed_bytes: u64,
    removed_files: u64,
    removed_dirs: u64,
    parent_id: Option<NodeId>,
    trashed: bool,
}

#[tauri::command]
pub fn start_scan(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<u64, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("choose a folder to scan".into());
    }

    let generation = state.last_generation.fetch_add(1, Ordering::Relaxed) + 1;
    let handle = spawn_backend(PathBuf::from(trimmed));
    let session = Arc::new(Session {
        generation,
        started: Instant::now(),
        builder: RwLock::new(TreeBuilder::new()),
        progress: Mutex::new(Progress {
            state: ScanState::Scanning,
            ..Progress::default()
        }),
        handle,
        filter: Mutex::new(None),
    });

    {
        let mut current = state.current.lock().unwrap();
        if let Some(old) = current.take() {
            old.handle.cancel();
        }
        *current = Some(Arc::clone(&session));
    }

    std::thread::Builder::new()
        .name("mathom-drain".into())
        .spawn(move || drain(app, session))
        .map_err(|e| e.to_string())?;
    Ok(generation)
}

fn spawn_backend(root: PathBuf) -> ScanHandle {
    #[cfg(all(windows, feature = "mft-backend"))]
    if let Some(mft) = mathom_scanner_ntfs::MftScanner::probe(&root) {
        return mft.scan(ScanOptions::new(root));
    }
    GenericScanner.scan(ScanOptions::new(root))
}

#[tauri::command]
pub fn cancel_scan(state: State<'_, AppState>) {
    if let Some(session) = state.current.lock().unwrap().as_ref() {
        session.handle.cancel();
    }
}

#[tauri::command]
pub fn scan_status(state: State<'_, AppState>) -> Snapshot {
    match state.current.lock().unwrap().as_ref() {
        Some(session) => snapshot(session),
        None => Snapshot {
            generation: 0,
            state: ScanState::Idle,
            files: 0,
            dirs: 0,
            bytes: 0,
            errors: 0,
            elapsed_ms: 0,
            nodes: 0,
            root_error: None,
        },
    }
}

#[tauri::command(async)]
pub fn get_children(
    state: State<'_, AppState>,
    generation: u64,
    ids: Vec<NodeId>,
    sort_by: String,
    descending: bool,
    hide_system: bool,
    filter: Option<String>,
) -> Result<Vec<DirListing>, String> {
    let session = session_for(&state, generation)?;
    let builder = session.builder.read().unwrap();
    let tree = builder.tree();
    let overlay = overlay_for(&session, tree, filter.as_deref(), hide_system);
    let mut listings = Vec::with_capacity(ids.len());
    for id in ids {
        if !tree.is_live(id) || !tree.node(id).is_dir() {
            continue;
        }
        // Filtered rows show matched bytes (the treemap already does), so
        // both views answer the question the query asked.
        let mut rows: Vec<Row> = match &overlay {
            Some(o) => {
                let parent_bytes = o.bytes_of(id);
                tree.children(id)
                    .filter(|&c| o.is_visible(c))
                    .map(|c| {
                        let mut row = make_row(tree, c, parent_bytes);
                        row.size = o.bytes_of(c);
                        row.pct = if parent_bytes == 0 {
                            0.0
                        } else {
                            row.size as f64 / parent_bytes as f64
                        };
                        row
                    })
                    .collect()
            }
            None => {
                let parent_size = tree.node(id).size;
                tree.children(id)
                    .filter(|&c| !hide_system || !tree.node(c).flags.contains(EntryFlags::SYSTEM))
                    .map(|c| make_row(tree, c, parent_size))
                    .collect()
            }
        };
        sort_rows(&mut rows, &sort_by, descending);
        listings.push(DirListing { id, rows });
    }
    Ok(listings)
}

#[tauri::command(async)]
pub fn get_node(
    state: State<'_, AppState>,
    generation: u64,
    id: NodeId,
) -> Result<Option<Row>, String> {
    let session = session_for(&state, generation)?;
    let builder = session.builder.read().unwrap();
    let tree = builder.tree();
    if !tree.is_live(id) {
        return Ok(None);
    }
    let parent_size = match tree.node(id).parent() {
        Some(p) => tree.node(p).size,
        None => tree.node(id).size,
    };
    Ok(Some(make_row(tree, id, parent_size)))
}

#[tauri::command(async)]
pub fn get_path(state: State<'_, AppState>, generation: u64, id: NodeId) -> Result<String, String> {
    let session = session_for(&state, generation)?;
    let builder = session.builder.read().unwrap();
    let tree = builder.tree();
    if !tree.is_live(id) {
        return Err("unknown node".into());
    }
    Ok(tree.path(id))
}

#[tauri::command(async)]
pub fn get_treemap(
    state: State<'_, AppState>,
    generation: u64,
    root_id: NodeId,
    width: f32,
    height: f32,
    hide_system: bool,
    filter: Option<String>,
) -> Result<Vec<TreemapRectDto>, String> {
    let session = session_for(&state, generation)?;
    let builder = session.builder.read().unwrap();
    let tree = builder.tree();
    // Empty, not an error: a stale view root is an expected race during
    // deletes; the UI re-points at the parent a frame later. (get_type_stats
    // answers the same race with "unknown node", which its caller tolerates.)
    if !tree.is_live(root_id) {
        return Ok(Vec::new());
    }
    let opts = TreemapOptions {
        min_area_px: 3.0,
        padding_px: 1.0,
        max_depth: 24,
        hide_system,
    };
    let viewport = Viewport {
        w: width,
        h: height,
    };
    let rects = match overlay_for(&session, tree, filter.as_deref(), hide_system) {
        Some(o) => treemap::layout_with_filter(tree, root_id, viewport, &opts, &o.bytes),
        None => treemap::layout(tree, root_id, viewport, &opts),
    };
    Ok(rects
        .into_iter()
        .map(|r| TreemapRectDto {
            id: r.id,
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
            depth: r.depth,
            is_dir: r.is_dir,
            category: r.category,
        })
        .collect())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeStatDto {
    ext: String,
    category: u8,
    bytes: u64,
    files: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypePanelData {
    types: Vec<TypeStatDto>,
    total_bytes: u64,
    total_files: u64,
    top_files: Vec<Row>,
}

#[tauri::command(async)]
pub fn get_type_stats(
    state: State<'_, AppState>,
    generation: u64,
    root_id: NodeId,
    hide_system: bool,
    filter: Option<String>,
) -> Result<TypePanelData, String> {
    const TOP_FILES: usize = 8;

    let session = session_for(&state, generation)?;
    let builder = session.builder.read().unwrap();
    let tree = builder.tree();
    if !tree.is_live(root_id) {
        return Err("unknown node".into());
    }

    let overlay = overlay_for(&session, tree, filter.as_deref(), hide_system);
    let visible = overlay.as_deref().map(|o| o.visible.as_slice());
    let bd = mathom_core::stats::type_breakdown(tree, root_id, hide_system, visible);
    let subtree_total = bd.total_bytes;
    let top_files =
        mathom_core::stats::largest_files(tree, root_id, TOP_FILES, hide_system, visible)
            .into_iter()
            .map(|id| make_row(tree, id, subtree_total))
            .collect();
    Ok(TypePanelData {
        types: bd
            .types
            .iter()
            .map(|t| TypeStatDto {
                ext: t.ext.as_ref().map_or("", |k| k.as_str()).to_string(),
                category: t.category as u8,
                bytes: t.bytes,
                files: t.files,
            })
            .collect(),
        total_bytes: bd.total_bytes,
        total_files: bd.total_files,
        top_files,
    })
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    id: NodeId,
    name: String,
    is_dir: bool,
    size: u64,
    path: String,
}

#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultsDto {
    hits: Vec<SearchHit>,
    total: u64,
}

#[tauri::command(async)]
pub fn search(
    state: State<'_, AppState>,
    generation: u64,
    query: String,
    hide_system: bool,
) -> Result<SearchResultsDto, String> {
    const MAX_RESULTS: usize = 100;

    let q = mathom_core::search::SearchQuery::parse(&query);
    if q.is_empty() {
        return Ok(SearchResultsDto::default());
    }
    let session = session_for(&state, generation)?;
    let builder = session.builder.read().unwrap();
    let tree = builder.tree();
    let res = mathom_core::search::search(tree, &q, MAX_RESULTS, hide_system);
    Ok(SearchResultsDto {
        hits: res
            .ids
            .into_iter()
            .map(|id| {
                let n = tree.node(id);
                SearchHit {
                    id,
                    name: tree.name(id).to_string(),
                    is_dir: n.is_dir(),
                    size: n.size,
                    path: tree.path(id),
                }
            })
            .collect(),
        total: res.total_matches,
    })
}

#[tauri::command(async)]
pub fn get_ancestors(
    state: State<'_, AppState>,
    generation: u64,
    id: NodeId,
) -> Result<Vec<Crumb>, String> {
    let session = session_for(&state, generation)?;
    let builder = session.builder.read().unwrap();
    let tree = builder.tree();
    if !tree.is_live(id) {
        return Err("unknown node".into());
    }
    let mut chain = vec![id];
    let mut cur = id;
    while let Some(p) = tree.node(cur).parent() {
        chain.push(p);
        cur = p;
    }
    chain.reverse();
    Ok(chain
        .into_iter()
        .map(|n| Crumb {
            id: n,
            name: tree.name(n).to_string(),
        })
        .collect())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletePreflight {
    path: String,
    block_reason: Option<String>,
}

/// Why deleting `path` must be refused right now, or `None` if it's allowed.
/// Mid-scan deletes are refused: `remove` vacates subtree slots that in-flight
/// batches may still name as parents, which panics `add_batch` on the drain
/// thread. States past Scanning are terminal, so a non-Scanning answer here
/// can't be raced by a late batch.
fn delete_block_reason(scan_state: ScanState, path: &str) -> Option<String> {
    if scan_state == ScanState::Scanning {
        return Some(
            "can't delete while a scan is running — cancel it or wait for it to finish".into(),
        );
    }
    crate::protected::deletion_block_reason(path)
}

#[tauri::command(async)]
pub fn delete_preflight(
    state: State<'_, AppState>,
    generation: u64,
    id: NodeId,
) -> Result<DeletePreflight, String> {
    let session = session_for(&state, generation)?;
    let scan_state = session.progress.lock().unwrap().state;
    let builder = session.builder.read().unwrap();
    let tree = builder.tree();
    if !tree.is_live(id) {
        return Err("unknown item".into());
    }
    let path = tree.path(id);
    let block_reason = delete_block_reason(scan_state, &path);
    Ok(DeletePreflight { path, block_reason })
}

#[tauri::command(async)]
pub fn delete_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    generation: u64,
    id: NodeId,
    permanent: bool,
) -> Result<DeleteResult, String> {
    let session = session_for(&state, generation)?;
    let scan_state = session.progress.lock().unwrap().state;

    let (path, parent_id, is_dir) = {
        let builder = session.builder.read().unwrap();
        let tree = builder.tree();
        if !tree.is_live(id) {
            return Err("unknown item".into());
        }
        if id == Tree::ROOT {
            return Err("can't delete the scan root".into());
        }
        let node = tree.node(id);
        (tree.path(id), node.parent(), node.is_dir())
    };

    // Re-checked here even though the dialog ran the preflight — this is
    // the enforcement point; the preflight is UX.
    if let Some(reason) = delete_block_reason(scan_state, &path) {
        return Err(reason);
    }

    if permanent {
        let res = if is_dir {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        res.map_err(|e| format!("{path}: {e}"))?;
    } else {
        trash::delete(&path).map_err(|e| format!("{path}: {e}"))?;
    }

    let removed = session.builder.write().unwrap().remove(id);
    // The overlay's per-id arrays are stale after any tree mutation.
    *session.filter.lock().unwrap() = None;
    if let Some(r) = removed {
        {
            let mut p = session.progress.lock().unwrap();
            p.files = p.files.saturating_sub(r.files);
            p.dirs = p.dirs.saturating_sub(r.dirs);
            p.bytes = p.bytes.saturating_sub(r.size);
        }
        emit_tick(&app, &session);
    }

    Ok(DeleteResult {
        removed_bytes: removed.map_or(0, |r| r.size),
        removed_files: removed.map_or(0, |r| r.files),
        removed_dirs: removed.map_or(0, |r| r.dirs),
        parent_id,
        trashed: !permanent,
    })
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportArgs {
    max_depth: Option<u32>,
    dirs_only: bool,
    hide_system: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportText {
    rows: u64,
    text: String,
}

/// Export holds the builder read lock for the whole write, which would
/// stall the drain thread mid-scan — refused the same way deletes are.
fn export_block_reason(scan_state: ScanState) -> Option<String> {
    (scan_state == ScanState::Scanning)
        .then(|| "can't export while a scan is running — wait for it to finish".into())
}

fn write_export(
    tree: &Tree,
    root_id: NodeId,
    format: &str,
    opts: &ExportOptions,
    w: &mut impl std::io::Write,
) -> Result<u64, String> {
    match format {
        "csv" => mathom_core::export::write_csv(tree, root_id, opts, w),
        "json" => mathom_core::export::write_json(tree, root_id, opts, w),
        _ => return Err(format!("unknown export format: {format}")),
    }
    .map_err(|e| e.to_string())
}

impl ExportArgs {
    fn options(self) -> ExportOptions {
        ExportOptions {
            max_depth: self.max_depth,
            dirs_only: self.dirs_only,
            hide_system: self.hide_system,
        }
    }
}

#[tauri::command(async)]
pub fn export_tree(
    state: State<'_, AppState>,
    generation: u64,
    root_id: NodeId,
    format: String,
    dest: String,
    args: ExportArgs,
) -> Result<u64, String> {
    use std::io::Write as _;

    let session = session_for(&state, generation)?;
    let scan_state = session.progress.lock().unwrap().state;
    if let Some(reason) = export_block_reason(scan_state) {
        return Err(reason);
    }
    let builder = session.builder.read().unwrap();
    let tree = builder.tree();
    if !tree.is_live(root_id) {
        return Err("unknown node".into());
    }
    let file = std::fs::File::create(&dest).map_err(|e| format!("{dest}: {e}"))?;
    let mut w = std::io::BufWriter::new(file);
    let rows = write_export(tree, root_id, &format, &args.options(), &mut w)?;
    w.flush().map_err(|e| format!("{dest}: {e}"))?;
    Ok(rows)
}

/// Clipboard exports travel back over IPC as one string; past the cap the
/// right answer is "save to a file", not a bigger message.
const CLIPBOARD_CAP: usize = 16 * 1024 * 1024;
const CLIPBOARD_CAP_MSG: &str =
    "export is bigger than 16 MiB — save it to a file instead, or lower the depth";

struct CappedBuf {
    buf: Vec<u8>,
}

impl std::io::Write for CappedBuf {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        if self.buf.len() + data.len() > CLIPBOARD_CAP {
            return Err(std::io::Error::other(CLIPBOARD_CAP_MSG));
        }
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tauri::command(async)]
pub fn export_text(
    state: State<'_, AppState>,
    generation: u64,
    root_id: NodeId,
    format: String,
    args: ExportArgs,
) -> Result<ExportText, String> {
    let session = session_for(&state, generation)?;
    let scan_state = session.progress.lock().unwrap().state;
    if let Some(reason) = export_block_reason(scan_state) {
        return Err(reason);
    }
    let builder = session.builder.read().unwrap();
    let tree = builder.tree();
    if !tree.is_live(root_id) {
        return Err("unknown node".into());
    }
    let mut w = CappedBuf { buf: Vec::new() };
    let rows = write_export(tree, root_id, &format, &args.options(), &mut w)?;
    let text = String::from_utf8(w.buf).map_err(|e| e.to_string())?;
    Ok(ExportText { rows, text })
}

#[tauri::command(async)]
pub fn open_in_explorer(
    state: State<'_, AppState>,
    generation: u64,
    id: NodeId,
) -> Result<(), String> {
    let session = session_for(&state, generation)?;
    let (path, is_dir) = {
        let builder = session.builder.read().unwrap();
        let tree = builder.tree();
        if !tree.is_live(id) {
            return Err("unknown item".into());
        }
        (tree.path(id), tree.node(id).is_dir())
    };
    reveal_in_file_manager(&path, is_dir)
}

#[cfg(windows)]
fn reveal_in_file_manager(path: &str, is_dir: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    // Absolute path: a bare "explorer" resolves through a search order an
    // attacker-writable directory could shadow.
    let explorer = std::path::Path::new(&std::env::var_os("WINDIR").ok_or("WINDIR is not set")?)
        .join("explorer.exe");
    let mut cmd = Command::new(explorer);
    if is_dir {
        cmd.arg(path);
    } else {
        // explorer wants /select and the path as one token; raw_arg because
        // Command's normal escaping confuses explorer's parser.
        cmd.raw_arg(format!("/select,\"{path}\""));
    }
    // explorer exits non-zero even on success; only a spawn failure matters.
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn reveal_in_file_manager(_path: &str, _is_dir: bool) -> Result<(), String> {
    Err("opening the file manager is only supported on Windows".into())
}

fn session_for(state: &AppState, generation: u64) -> Result<Arc<Session>, String> {
    match state.current.lock().unwrap().as_ref() {
        Some(s) if s.generation == generation => Ok(Arc::clone(s)),
        Some(_) => Err("stale scan generation".into()),
        None => Err("no scan has been started".into()),
    }
}

fn drain(app: AppHandle, session: Arc<Session>) {
    let rx = session.handle.events().clone();
    let mut last_tick = Instant::now();
    emit_tick(&app, &session);

    for event in rx.iter() {
        match event {
            ScanEvent::Batch(batch) => {
                session.builder.write().unwrap().add_batch(&batch);
            }
            ScanEvent::DirError { id, message } => {
                let tree_empty = session.builder.read().unwrap().tree().is_empty();
                if id == Tree::ROOT && tree_empty {
                    let mut p = session.progress.lock().unwrap();
                    p.errors += 1;
                    p.root_error = Some(message);
                } else {
                    session.progress.lock().unwrap().errors += 1;
                    session.builder.write().unwrap().mark_error(id);
                }
            }
            ScanEvent::Progress(pr) => {
                let mut p = session.progress.lock().unwrap();
                p.files = pr.files;
                p.dirs = pr.dirs;
                p.bytes = pr.bytes;
            }
            ScanEvent::Done(stats) => {
                {
                    let mut p = session.progress.lock().unwrap();
                    p.files = stats.files;
                    p.dirs = stats.dirs;
                    p.bytes = stats.bytes;
                    p.errors = stats.errors;
                    // Time-to-ready: the drain can lag the scanner on fast scans.
                    p.finished_ms = Some(session.started.elapsed().as_millis() as u64);
                    p.state = if p.root_error.is_some() {
                        ScanState::Failed
                    } else if stats.cancelled {
                        ScanState::Cancelled
                    } else {
                        ScanState::Done
                    };
                }
                emit_tick(&app, &session);
                let _ = app.emit("scan://done", snapshot(&session));
                return;
            }
        }
        if last_tick.elapsed() >= TICK_INTERVAL {
            emit_tick(&app, &session);
            last_tick = Instant::now();
        }
    }

    // Done is mandatory; EOF means the worker died without reporting it.
    {
        let mut p = session.progress.lock().unwrap();
        p.errors += 1;
        p.root_error = Some("the scan worker stopped without finishing".into());
        p.finished_ms = Some(session.started.elapsed().as_millis() as u64);
        p.state = ScanState::Failed;
    }
    emit_tick(&app, &session);
    let _ = app.emit("scan://done", snapshot(&session));
}

fn emit_tick(app: &AppHandle, session: &Session) {
    let _ = app.emit("scan://tick", snapshot(session));
}

fn snapshot(session: &Session) -> Snapshot {
    let nodes = session.builder.read().unwrap().tree().len();
    let p = session.progress.lock().unwrap();
    Snapshot {
        generation: session.generation,
        state: p.state,
        files: p.files,
        dirs: p.dirs,
        bytes: p.bytes,
        errors: p.errors,
        elapsed_ms: p
            .finished_ms
            .unwrap_or_else(|| session.started.elapsed().as_millis() as u64),
        nodes,
        root_error: p.root_error.clone(),
    }
}

fn make_row(tree: &Tree, id: NodeId, parent_size: u64) -> Row {
    let n = tree.node(id);
    Row {
        id,
        name: tree.name(id).to_string(),
        is_dir: n.is_dir(),
        size: n.size,
        allocated: n.allocated,
        items: n.items,
        mtime: n.mtime,
        has_children: tree.children(id).next().is_some(),
        is_reparse: n.flags.contains(EntryFlags::REPARSE),
        is_error: n.flags.contains(EntryFlags::ERROR),
        pct: if parent_size == 0 {
            0.0
        } else {
            n.size as f64 / parent_size as f64
        },
    }
}

fn sort_rows(rows: &mut [Row], key: &str, descending: bool) {
    match key {
        "name" => rows.sort_by_cached_key(|r| r.name.to_ascii_lowercase()),
        "items" => rows.sort_unstable_by_key(|r| r.items),
        "mtime" => rows.sort_unstable_by_key(|r| r.mtime),
        _ => rows.sort_unstable_by_key(|r| r.size),
    }
    if descending {
        rows.reverse();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletes_are_blocked_while_scanning() {
        assert!(delete_block_reason(ScanState::Scanning, "C:\\Users\\me\\big.iso").is_some());
    }

    #[test]
    fn finished_scans_defer_to_path_policy() {
        assert!(delete_block_reason(ScanState::Done, "C:\\Users\\me\\big.iso").is_none());
        assert!(delete_block_reason(ScanState::Cancelled, "C:\\Users\\me\\big.iso").is_none());
        assert!(delete_block_reason(ScanState::Failed, "C:\\Users\\me\\big.iso").is_none());
        // Path policy still applies once the scan is over.
        assert!(delete_block_reason(ScanState::Done, "C:\\$Recycle.Bin").is_some());
    }
}
