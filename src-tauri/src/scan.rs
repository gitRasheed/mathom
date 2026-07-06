//! Bridge between the channel-based scanner and the Tauri UI.
//!
//! One drain thread per scan consumes `ScanEvent`s, feeds the shared
//! `TreeBuilder`, and emits a throttled (~100ms) `scan://tick` event. The UI
//! never receives the tree itself: it re-queries the slices it can see
//! (`get_children` for the root + expanded directories) on each tick, so
//! IPC payloads stay O(visible rows), not O(tree).
//!
//! Concurrency: the drain thread takes short write locks per batch; query
//! commands take read locks. A monotonic `generation` identifies each scan —
//! starting a new scan cancels the old handle, and the old drain thread keeps
//! writing only to its own orphaned `Session` until `Done` arrives. Ticks and
//! query results carry the generation so the UI can drop stale ones.
//! Lock order: never hold the builder lock and the progress lock at once.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use mathom_core::tree::{NodeId, Tree};
use mathom_core::{EntryFlags, TreeBuilder, TreemapOptions, Viewport, treemap};
use mathom_scanner::{GenericScanner, ScanEvent, ScanHandle, ScanOptions, Scanner};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

const TICK_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Default)]
pub struct AppState {
    /// Monotonic scan id, bumped on every `start_scan`.
    last_generation: AtomicU64,
    current: Mutex<Option<Arc<Session>>>,
}

struct Session {
    generation: u64,
    started: Instant,
    builder: RwLock<TreeBuilder>,
    progress: Mutex<Progress>,
    handle: ScanHandle,
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

/// Payload of `scan://tick` / `scan://done` and of `scan_status`.
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
    /// Arena length so far (>= entries received; includes vacant slots).
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
    /// Fraction of the parent directory's total size, 0..=1.
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

/// Backend selection, invisible to the UI: NTFS volume + elevation → raw
/// MFT reader; anything else (FAT/exFAT/ReFS/network, non-elevated,
/// unsupported geometry) → generic walker, silently.
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

/// Children of each requested directory, sorted. Non-directories and ids the
/// tree hasn't seen yet are silently skipped, so the UI can ask optimistically
/// mid-scan.
#[tauri::command]
pub fn get_children(
    state: State<'_, AppState>,
    generation: u64,
    ids: Vec<NodeId>,
    sort_by: String,
    descending: bool,
    hide_system: bool,
) -> Result<Vec<DirListing>, String> {
    let session = session_for(&state, generation)?;
    let builder = session.builder.read().unwrap();
    let tree = builder.tree();
    let mut listings = Vec::with_capacity(ids.len());
    for id in ids {
        if (id as usize) >= tree.len() || !tree.node(id).is_dir() {
            continue;
        }
        let parent_size = tree.node(id).size;
        let mut rows: Vec<Row> = tree
            .children(id)
            .filter(|&c| !hide_system || !tree.node(c).flags.contains(EntryFlags::SYSTEM))
            .map(|c| make_row(tree, c, parent_size))
            .collect();
        sort_rows(&mut rows, &sort_by, descending);
        listings.push(DirListing { id, rows });
    }
    Ok(listings)
}

#[tauri::command]
pub fn get_node(
    state: State<'_, AppState>,
    generation: u64,
    id: NodeId,
) -> Result<Option<Row>, String> {
    let session = session_for(&state, generation)?;
    let builder = session.builder.read().unwrap();
    let tree = builder.tree();
    if (id as usize) >= tree.len() {
        return Ok(None);
    }
    // Root has no parent: report pct 1.0 against itself.
    let parent_size = match tree.node(id).parent() {
        Some(p) => tree.node(p).size,
        None => tree.node(id).size,
    };
    Ok(Some(make_row(tree, id, parent_size)))
}

#[tauri::command]
pub fn get_path(state: State<'_, AppState>, generation: u64, id: NodeId) -> Result<String, String> {
    let session = session_for(&state, generation)?;
    let builder = session.builder.read().unwrap();
    let tree = builder.tree();
    if (id as usize) >= tree.len() {
        return Err("unknown node".into());
    }
    Ok(tree.path(id))
}

/// Squarified layout of the subtree under `root_id` for a `width`×`height`
/// CSS-pixel canvas. Options are fixed here (QDirStat-informed 3px² cull,
/// 1px nesting padding) until there's a reason to expose them.
#[tauri::command]
pub fn get_treemap(
    state: State<'_, AppState>,
    generation: u64,
    root_id: NodeId,
    width: f32,
    height: f32,
    hide_system: bool,
) -> Result<Vec<TreemapRectDto>, String> {
    let session = session_for(&state, generation)?;
    let builder = session.builder.read().unwrap();
    let tree = builder.tree();
    let opts = TreemapOptions {
        min_area_px: 3.0,
        padding_px: 1.0,
        max_depth: 24,
        hide_system,
    };
    let rects = treemap::layout(
        tree,
        root_id,
        Viewport {
            w: width,
            h: height,
        },
        &opts,
    );
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

/// Root-first chain of ancestors, ending with the node itself. Powers the
/// treemap breadcrumbs and "reveal in tree".
#[tauri::command]
pub fn get_ancestors(
    state: State<'_, AppState>,
    generation: u64,
    id: NodeId,
) -> Result<Vec<Crumb>, String> {
    let session = session_for(&state, generation)?;
    let builder = session.builder.read().unwrap();
    let tree = builder.tree();
    if (id as usize) >= tree.len() {
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

/// Deletes a node's file/directory (Recycle Bin unless `permanent`), then
/// subtracts its subtree from the live tree so the UI updates without a
/// rescan. The blocking filesystem op runs with no tree lock held.
#[tauri::command]
pub fn delete_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    generation: u64,
    id: NodeId,
    permanent: bool,
) -> Result<DeleteResult, String> {
    let session = session_for(&state, generation)?;

    let (path, parent_id, is_dir) = {
        let builder = session.builder.read().unwrap();
        let tree = builder.tree();
        if (id as usize) >= tree.len() {
            return Err("unknown item".into());
        }
        if id == Tree::ROOT {
            return Err("can't delete the scan root".into());
        }
        let node = tree.node(id);
        (tree.path(id), node.parent(), node.is_dir())
    };

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

    // The delete succeeded on disk. Reflect it in the tree if this scan still
    // owns it; a scan started meanwhile leaves an orphaned session we can
    // mutate harmlessly (the new scan reads the real disk anyway).
    let removed = session.builder.write().unwrap().remove(id);
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

/// Reveals the item in the OS file manager: folders open directly, files are
/// selected inside their parent folder.
#[tauri::command]
pub fn open_in_explorer(
    state: State<'_, AppState>,
    generation: u64,
    id: NodeId,
) -> Result<(), String> {
    let session = session_for(&state, generation)?;
    let (path, is_dir) = {
        let builder = session.builder.read().unwrap();
        let tree = builder.tree();
        if (id as usize) >= tree.len() {
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
    let mut cmd = Command::new("explorer");
    if is_dir {
        cmd.arg(path);
    } else {
        // explorer's /select wants path and flag as one token; quote it via
        // raw_arg (Command's normal escaping confuses explorer's own parser).
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
                    // Root itself unreadable: the scan produced nothing.
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
                    p.finished_ms = Some(stats.elapsed.as_millis() as u64);
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
