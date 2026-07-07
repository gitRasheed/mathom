//! Elevation flow. Raw-MFT scans need administrator rights, so release
//! builds request them by default at launch (UAC prompt, WizTree-style).
//! Declining is never fatal: the app keeps running on the generic-walker
//! fallback and the UI shows a banner with a relaunch button.
//!
//! `MATHOM_NO_ELEVATE=1` suppresses the launch prompt. Dev builds never
//! auto-prompt: the relaunched process would escape the tauri-cli dev loop
//! and lose the Vite dev server (the banner button still works there, with
//! the same caveat).

use serde::Serialize;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElevationStatus {
    pub elevated: bool,
}

#[tauri::command]
pub fn elevation_status() -> ElevationStatus {
    ElevationStatus {
        elevated: is_elevated(),
    }
}

/// Relaunches the app elevated (UAC prompt) and exits this instance. A
/// declined prompt returns `Err` and the app keeps running as it is.
#[tauri::command]
pub fn relaunch_elevated(app: tauri::AppHandle) -> Result<(), String> {
    spawn_elevated()?;
    app.exit(0);
    Ok(())
}

/// Call first thing in `main`: exits the process when an elevated relaunch
/// was started, returns in every other case.
pub fn elevate_at_launch() {
    if cfg!(debug_assertions) || std::env::var_os("MATHOM_NO_ELEVATE").is_some() || is_elevated() {
        return;
    }
    if spawn_elevated().is_ok() {
        std::process::exit(0);
    }
    // Declined: continue non-elevated; the UI banner offers a retry.
}

#[cfg(windows)]
fn is_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // SAFETY: standard token query on our own process; the out-pointers
    // live across the calls and the token handle is closed on every path.
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return true; // can't tell — behave as elevated rather than nag
        }
        let mut info = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut info as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        let _ = CloseHandle(token);
        ok.is_ok() && info.TokenIsElevated != 0
    }
}

/// Starts an elevated copy of this executable via the shell's `runas` verb.
/// `Ok` means the new instance is launching and the caller should exit.
#[cfg(windows)]
fn spawn_elevated() -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{PCWSTR, w};

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut exe_w: Vec<u16> = exe.as_os_str().encode_wide().collect();
    exe_w.push(0);

    // SAFETY: both strings are NUL-terminated and outlive the call.
    let inst = unsafe {
        ShellExecuteW(
            None,
            w!("runas"),
            PCWSTR(exe_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW's contract: values above 32 mean the launch succeeded.
    if inst.0 as usize > 32 {
        Ok(())
    } else {
        Err("administrator relaunch was declined".into())
    }
}

#[cfg(not(windows))]
fn is_elevated() -> bool {
    true
}

#[cfg(not(windows))]
fn spawn_elevated() -> Result<(), String> {
    Err("elevation is only supported on Windows".into())
}
