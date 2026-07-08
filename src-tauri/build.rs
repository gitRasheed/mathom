fn main() {
    // tauri-build compiles icon.ico into the exe's icon resource but never
    // tells cargo to watch it — without this, icon edits ship stale.
    println!("cargo:rerun-if-changed=icons/icon.ico");
    tauri_build::build()
}
