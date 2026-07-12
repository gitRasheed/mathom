#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod drives;
mod elevation;
mod protected;
mod scan;

fn main() {
    elevation::elevate_at_launch();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(scan::AppState::default())
        .setup(|app| {
            size_to_monitor(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            scan::start_scan,
            scan::cancel_scan,
            scan::scan_status,
            scan::get_children,
            scan::get_node,
            scan::get_path,
            scan::get_treemap,
            scan::get_type_stats,
            scan::get_ancestors,
            scan::search,
            scan::delete_preflight,
            scan::delete_entry,
            scan::export_tree,
            scan::export_text,
            scan::open_in_explorer,
            drives::list_drives,
            elevation::elevation_status,
            elevation::relaunch_elevated,
        ])
        .run(tauri::generate_context!())
        .expect("error while running mathom");
}

/// Open at 65% of the monitor, centered — one fixed size reads wrong on
/// laptops and 4K displays alike. Min sizes from the config still apply.
fn size_to_monitor(app: &tauri::App) {
    use tauri::Manager;
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let Ok(Some(mon)) = win.current_monitor() else {
        return;
    };
    let size = mon.size();
    let _ = win.set_size(tauri::PhysicalSize::new(
        size.width * 13 / 20,
        size.height * 13 / 20,
    ));
    let _ = win.center();
}
