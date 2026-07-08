<img src="src-tauri/icons/icon.svg" alt="" width="96" align="left">

# mathom

A fast disk space analyzer for Windows: scan a drive, see where the space
went, clean it up. Live tree + zoomable treemap + file-type breakdown +
search, with delete to the Recycle Bin.

<br clear="left">


**Status: alpha.** Scanning and the UI work end to end; expect rough edges,
missing polish, and breaking changes between releases. Windows is the only
supported platform for now (the core crates build everywhere by design).

<!-- screenshot goes here: docs/screenshot.png -->

## Why another one

- **Fast.** On NTFS volumes mathom reads the Master File Table directly
  instead of walking folders: a 2-million-file system drive maps in about
  3 seconds, where the folder walk takes over 30. Everything streams — the
  tree and treemap fill in live while the scan runs.
- **Honest sizes.** Logical vs allocated size, NTFS compression, sparse
  files, hardlinks (counted once), OneDrive placeholders that look like
  gigabytes but occupy almost nothing — measured, not guessed.
- **In-process scanner.** The scanner is a Rust crate inside the app, not a
  sidecar binary, which is what makes live streaming, instant cancel, and
  (eventually) incremental rescan possible.

## Install

Download the installer (MSI or setup.exe) or the portable zip from
[Releases](../../releases). The portable build needs the WebView2 runtime,
which ships with Windows 10/11.

The installers are not code-signed (yet), so Windows SmartScreen will warn
on first run — "More info" → "Run anyway".

Reading the MFT requires administrator rights; mathom asks once at launch.
Decline and it silently falls back to the slower folder walker — nothing
breaks.

## Build from source

Prerequisites: [Rust](https://rustup.rs) (stable, MSVC toolchain) and
[Node.js](https://nodejs.org) 20+.

```
cd ui && npm ci && cd ..
npm run dev          # development app with hot reload
npm run build:app    # release build + installers
cargo test --workspace
```

## Architecture, briefly

A cargo workspace: `crates/core` holds the arena tree model, aggregation,
treemap layout, and search (no platform-specific code); `crates/scanner` is
the generic parallel walker; `crates/scanner-ntfs` is the raw MFT reader
(Windows-only, behind a cargo feature); `src-tauri` + `ui/` are the Tauri v2
shell and React front-end. Both scan backends implement one `Scanner` trait —
the app probes per scan and picks the MFT path when the volume is NTFS and
the process is elevated, otherwise the walker. The UI never knows which ran.

No telemetry. Ever.

## Name

A *mathom* is what hobbits call a thing they have no use for but can't bring
themselves to throw away. Your disk is full of them.

## License

[MIT](LICENSE)
