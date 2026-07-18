# mathom

A fast disk space analyzer for Windows. Scan a drive, see where your space
went, clean it up: live tree, zoomable treemap, file-type breakdown, search,
and delete to the Recycle Bin.

![mathom after a 3-second full scan of a 363 GB NTFS drive: tree view, zoomable treemap, and file-type breakdown](docs/screenshot.png)

**Status: alpha.** Works end to end, expect rough edges and breaking changes.
Windows only; the portable crates build on Linux and macOS, there is just no
UI for them yet.

## Install

Download the MSI, setup.exe, or portable zip from [Releases](../../releases).

Not code-signed yet, so SmartScreen warns on first run: "More info", then
"Run anyway". Reading the MFT needs administrator rights; mathom asks once
at launch and falls back to the slower folder walker if you decline.

## Why another one

On NTFS volumes mathom reads the Master File Table directly instead of
walking folders, and scans faster than WizTree in local tests. Results
stream in while the scan runs, so the tree and treemap fill in live. Sizes
are reported truthfully: logical vs. allocated, NTFS compression, sparse
files, hardlinks counted once, OneDrive placeholders.

Also, a mathom is what hobbits call a thing they have no use for but can't
bring themselves to throw away. Disks are full of them.

## Layout

A cargo workspace. `crates/core` holds the tree model, treemap layout, and
search, with no platform-specific code. `crates/scanner` is the generic
parallel walker; `crates/scanner-ntfs` is the raw MFT reader, Windows-only.
`src-tauri` and `ui/` are the Tauri v2 shell and the React front end. Both
backends implement the same `Scanner` trait; per scan the app picks MFT when
the volume is NTFS and the process is elevated, otherwise the walker.

## Build from source

[Rust](https://rustup.rs) (stable, MSVC) and [Node.js](https://nodejs.org) 20+.

```text
cd ui && npm ci && cd ..
npm run dev          # development app with hot reload
npm run build:app    # release build + installers
cargo test --workspace
```

## License

[MIT](LICENSE)
