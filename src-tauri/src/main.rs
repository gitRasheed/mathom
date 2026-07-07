#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod elevation;
mod scan;

fn main() {
    elevation::elevate_at_launch();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(scan::AppState::default())
        .invoke_handler(tauri::generate_handler![
            scan::start_scan,
            scan::cancel_scan,
            scan::scan_status,
            scan::get_children,
            scan::get_node,
            scan::get_path,
            scan::get_treemap,
            scan::get_ancestors,
            scan::delete_entry,
            scan::open_in_explorer,
            elevation::elevation_status,
            elevation::relaunch_elevated,
        ])
        .run(tauri::generate_context!())
        .expect("error while running mathom");
}
