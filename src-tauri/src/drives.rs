//! Volume enumeration for the Scan menu: every mounted drive letter with
//! capacity, so the UI can offer one-click whole-drive scans.

use serde::Serialize;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveInfo {
    /// Root path as the scanner expects it, e.g. `C:\`.
    pub path: String,
    /// Volume label; empty when the volume has none.
    pub label: String,
    pub total: u64,
    pub free: u64,
}

// async: GetDiskFreeSpaceExW on a network drive can block for seconds.
#[tauri::command(async)]
pub fn list_drives() -> Vec<DriveInfo> {
    list()
}

#[cfg(windows)]
fn list() -> Vec<DriveInfo> {
    use windows::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    };
    use windows::core::PCWSTR;

    const DRIVE_NO_ROOT_DIR: u32 = 1;

    let mask = unsafe { GetLogicalDrives() };
    let mut drives = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let path = format!("{}:\\", (b'A' + i as u8) as char);
        let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
        let root = PCWSTR(wide.as_ptr());
        if unsafe { GetDriveTypeW(root) } <= DRIVE_NO_ROOT_DIR {
            continue;
        }
        let (mut total, mut free) = (0u64, 0u64);
        // Fails on media-less removables (card readers, DVD drives) — skip, nothing to scan.
        if unsafe { GetDiskFreeSpaceExW(root, None, Some(&mut total), Some(&mut free)) }.is_err() {
            continue;
        }
        let mut name = [0u16; 33];
        let label =
            match unsafe { GetVolumeInformationW(root, Some(&mut name), None, None, None, None) } {
                Ok(()) => {
                    let len = name.iter().position(|&c| c == 0).unwrap_or(0);
                    String::from_utf16_lossy(&name[..len])
                }
                Err(_) => String::new(),
            };
        drives.push(DriveInfo {
            path,
            label,
            total,
            free,
        });
    }
    drives
}

#[cfg(not(windows))]
fn list() -> Vec<DriveInfo> {
    Vec::new()
}
