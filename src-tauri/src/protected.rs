//! Delete-boundary policy: paths Windows itself depends on.
//!
//! Blocks the OS structures living at a volume root ($Recycle.Bin, System
//! Volume Information, the paging files), the Windows directory itself, and
//! the well-known critical dirs inside it (System32 etc.) including their
//! contents — nothing in those is ever legitimate cleanup. Everything else
//! under Windows (Temp, SoftwareDistribution, Logs) and the contents of the
//! volume-root entries stay deletable — clearing orphaned recycle-bin data is
//! a legitimate use — and NTFS ACLs still guard individual system files. The
//! app usually runs elevated, so this policy is enforced before deletion.
//! Parent aliases and 8.3 names are resolved fail-closed before the check, but
//! the final leaf is not followed, so deleting a junction removes the link.

#[cfg(windows)]
use std::ffi::OsString;
#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};
#[cfg(windows)]
use std::path::{Path, PathBuf};

#[cfg(windows)]
use windows::Win32::Storage::FileSystem::GetLongPathNameW;
#[cfg(windows)]
use windows::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;
#[cfg(windows)]
use windows::core::PCWSTR;

#[cfg(windows)]
const UNVERIFIED_PATH: &str = "couldn't verify this path, so deletion was blocked";

/// Why `path` must never be deleted, or `None` if it's fair game.
pub fn deletion_block_reason(path: &str) -> Option<String> {
    #[cfg(windows)]
    {
        let Ok(system_root) = system_windows_directory() else {
            return Some(UNVERIFIED_PATH.into());
        };
        deletion_block_reason_with_root(Path::new(path), &system_root)
    }

    #[cfg(not(windows))]
    {
        block_reason(path, std::env::var("SystemRoot").ok().as_deref())
    }
}

#[cfg(windows)]
fn deletion_block_reason_with_root(path: &Path, system_root: &Path) -> Option<String> {
    let Ok(path) = resolve_policy_path(path) else {
        return Some(UNVERIFIED_PATH.into());
    };
    let Ok(system_root) = std::fs::canonicalize(system_root) else {
        return Some(UNVERIFIED_PATH.into());
    };
    let system_root = policy_path_string(&system_root);
    block_reason(&path, Some(&system_root))
}

#[cfg(windows)]
fn resolve_policy_path(path: &Path) -> std::io::Result<String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| std::io::Error::other("path has no parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("path has no file name"))?;
    let parent = std::fs::canonicalize(parent)?;
    let path = long_path_name(&parent.join(name))?;
    Ok(policy_path_string(&path))
}

#[cfg(windows)]
fn system_windows_directory() -> std::io::Result<PathBuf> {
    let mut buffer = vec![0u16; 260];
    loop {
        // SAFETY: `buffer` is writable for its full reported length.
        let len = unsafe { GetSystemWindowsDirectoryW(Some(&mut buffer)) } as usize;
        if len == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if len < buffer.len() {
            buffer.truncate(len);
            return Ok(OsString::from_wide(&buffer).into());
        }
        buffer.resize(len + 1, 0);
    }
}

#[cfg(windows)]
fn long_path_name(path: &Path) -> std::io::Result<PathBuf> {
    let input: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut buffer = vec![0u16; 260];
    loop {
        // SAFETY: `input` is NUL-terminated and `buffer` is writable.
        let len = unsafe { GetLongPathNameW(PCWSTR(input.as_ptr()), Some(&mut buffer)) } as usize;
        if len == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if len < buffer.len() {
            buffer.truncate(len);
            return Ok(OsString::from_wide(&buffer).into());
        }
        buffer.resize(len + 1, 0);
    }
}

#[cfg(windows)]
fn policy_path_string(path: &Path) -> String {
    let path = path.to_string_lossy();
    strip_verbatim_disk_prefix(&path).to_owned()
}

#[cfg(windows)]
fn strip_verbatim_disk_prefix(path: &str) -> &str {
    let Some(path_without_prefix) = path.strip_prefix(r"\\?\") else {
        return path;
    };
    let bytes = path_without_prefix.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\' {
        path_without_prefix
    } else {
        path
    }
}

fn block_reason(path: &str, system_root: Option<&str>) -> Option<String> {
    let trimmed = path.trim_end_matches(['\\', '/']);

    if let Some(name) = volume_root_child(trimmed) {
        match name.to_ascii_lowercase().as_str() {
            "$recycle.bin" | "system volume information" => {
                return Some(format!("{name} is managed by Windows and can't be deleted"));
            }
            "pagefile.sys" | "hiberfil.sys" | "swapfile.sys" => {
                return Some(format!("{name} is in use by Windows and can't be deleted"));
            }
            _ => {}
        }
    }

    if let Some(root) = system_root {
        let root = root.trim_end_matches(['\\', '/']);
        if trimmed.eq_ignore_ascii_case(root) {
            return Some("the Windows directory can't be deleted".into());
        }
        if let Some(rest) = strip_dir_prefix(trimmed, root) {
            let name = rest.split(['\\', '/']).next().unwrap_or(rest);
            if CRITICAL_WINDOWS_DIRS
                .iter()
                .any(|d| name.eq_ignore_ascii_case(d))
            {
                return Some(if name.len() == rest.len() {
                    format!("{name} is part of Windows and can't be deleted")
                } else {
                    format!("items inside {name} are part of Windows and can't be deleted")
                });
            }
        }
    }
    None
}

/// Direct children of the Windows directory that are blocked wholesale,
/// contents included. Everything else under Windows stays deletable.
const CRITICAL_WINDOWS_DIRS: [&str; 5] = ["System32", "SysWOW64", "WinSxS", "Boot", "Fonts"];

/// `C:\Windows\System32\x` with prefix `C:\Windows` → `Some("System32\x")`;
/// equal or unrelated paths → `None`.
fn strip_dir_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if path.len() <= prefix.len() {
        return None;
    }
    let (head, tail) = path.split_at(prefix.len());
    (head.eq_ignore_ascii_case(prefix) && tail.starts_with(['\\', '/'])).then(|| &tail[1..])
}

/// `C:\$Recycle.Bin` → `Some("$Recycle.Bin")`; deeper or relative paths → `None`.
fn volume_root_child(path: &str) -> Option<&str> {
    let b = path.as_bytes();
    let has_drive_root = b.len() > 3
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b[2] == b'\\' || b[2] == b'/');
    if !has_drive_root {
        return None;
    }
    let child = &path[3..];
    (!child.contains(['\\', '/'])).then_some(child)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    use std::path::PathBuf;

    #[cfg(windows)]
    fn fixture_dir(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "mathom-delete-policy-{name}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }

    const SYSROOT: Option<&str> = Some("C:\\Windows");

    #[test]
    fn blocks_recycle_bin_at_any_volume_root() {
        assert!(block_reason("C:\\$Recycle.Bin", SYSROOT).is_some());
        assert!(block_reason("d:/$RECYCLE.BIN", SYSROOT).is_some());
        assert!(block_reason("E:\\$Recycle.Bin\\", SYSROOT).is_some());
    }

    #[test]
    fn blocks_system_volume_information_and_paging_files() {
        assert!(block_reason("C:\\System Volume Information", SYSROOT).is_some());
        assert!(block_reason("C:\\pagefile.sys", SYSROOT).is_some());
        assert!(block_reason("C:\\hiberfil.sys", SYSROOT).is_some());
        assert!(block_reason("D:\\swapfile.sys", SYSROOT).is_some());
    }

    #[test]
    fn blocks_the_windows_directory_itself() {
        assert!(block_reason("C:\\Windows", SYSROOT).is_some());
        assert!(block_reason("c:\\WINDOWS\\", SYSROOT).is_some());
        // Same name on another drive is just a folder.
        assert!(block_reason("D:\\Windows", SYSROOT).is_none());
    }

    #[test]
    fn blocks_critical_windows_dirs_and_their_contents() {
        assert!(block_reason("C:\\Windows\\System32", SYSROOT).is_some());
        assert!(block_reason("c:\\windows\\system32\\drivers", SYSROOT).is_some());
        assert!(block_reason("C:\\Windows\\SysWOW64", SYSROOT).is_some());
        assert!(block_reason("C:\\Windows\\WinSxS\\amd64_microsoft-windows", SYSROOT).is_some());
        assert!(block_reason("C:\\Windows\\Boot\\", SYSROOT).is_some());
        assert!(block_reason("C:\\Windows\\Fonts\\arial.ttf", SYSROOT).is_some());
    }

    #[test]
    fn temp_class_paths_inside_windows_stay_deletable() {
        assert!(block_reason("C:\\Windows\\Temp", SYSROOT).is_none());
        assert!(block_reason("C:\\Windows\\Temp\\junk.tmp", SYSROOT).is_none());
        assert!(block_reason("C:\\Windows\\SoftwareDistribution\\Download", SYSROOT).is_none());
        assert!(block_reason("C:\\Windows\\Logs", SYSROOT).is_none());
    }

    #[test]
    fn critical_dir_lookalikes_elsewhere_stay_deletable() {
        assert!(block_reason("D:\\Windows\\System32", SYSROOT).is_none());
        assert!(block_reason("C:\\backup\\Windows\\System32", SYSROOT).is_none());
        assert!(block_reason("C:\\System32", SYSROOT).is_none());
        // "C:\Windows2" shares a prefix with the system root but isn't it.
        assert!(block_reason("C:\\Windows2\\System32", SYSROOT).is_none());
    }

    #[test]
    fn allows_recycle_bin_contents_and_lookalikes() {
        // Deleting orphaned per-user recycle data is the point of finding it.
        assert!(block_reason("C:\\$Recycle.Bin\\S-1-5-21-303019", SYSROOT).is_none());
        // Only the real one at a volume root is special.
        assert!(block_reason("C:\\backup\\$Recycle.Bin", SYSROOT).is_none());
    }

    #[test]
    fn allows_ordinary_paths() {
        assert!(block_reason("C:\\Users\\me\\big.iso", SYSROOT).is_none());
        assert!(block_reason("C:\\ProgramData", SYSROOT).is_none());
        assert!(block_reason("relative\\pagefile.sys", SYSROOT).is_none());
    }

    #[test]
    fn volume_root_rules_hold_without_a_system_root() {
        assert!(block_reason("C:\\$Recycle.Bin", None).is_some());
        assert!(block_reason("C:\\Windows", None).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn resolved_policy_paths_use_plain_drive_syntax() {
        let fixture = fixture_dir("plain-path");
        let file = fixture.join("ordinary.txt");
        std::fs::write(&file, b"test").unwrap();

        let resolved = resolve_policy_path(&file).unwrap();
        assert!(!resolved.starts_with(r"\\?\"));
        assert!(resolved.ends_with(r"\ordinary.txt"));

        std::fs::remove_dir_all(fixture).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn strips_only_verbatim_disk_prefixes() {
        assert_eq!(
            strip_verbatim_disk_prefix(r"\\?\C:\$Recycle.Bin"),
            r"C:\$Recycle.Bin"
        );
        assert_eq!(
            strip_verbatim_disk_prefix(r"\\?\UNC\server\share"),
            r"\\?\UNC\server\share"
        );
    }

    #[cfg(windows)]
    #[test]
    fn resolves_ancestors_without_following_the_deleted_junction() {
        let fixture = fixture_dir("junction");
        let windows = fixture.join("windows");
        let system32 = windows.join("System32");
        let alias = fixture.join("alias");
        std::fs::create_dir_all(&system32).unwrap();

        let output = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&alias)
            .arg(&windows)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "mklink failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        assert!(deletion_block_reason_with_root(&alias, &windows).is_none());
        let reason = deletion_block_reason_with_root(&alias.join("System32"), &windows)
            .expect("System32 reached through a junction must be blocked");
        assert!(reason.contains("System32"));

        std::fs::remove_dir(&alias).unwrap();
        std::fs::remove_dir_all(fixture).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn unverifiable_paths_fail_closed() {
        let fixture = fixture_dir("missing");
        let windows = fixture.join("windows");
        std::fs::create_dir(&windows).unwrap();

        let reason = deletion_block_reason_with_root(&fixture.join("missing"), &windows)
            .expect("an unverifiable path must be blocked");
        assert!(reason.contains("couldn't verify"));

        std::fs::remove_dir_all(fixture).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn expands_a_short_leaf_name_when_the_volume_provides_one() {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use windows::Win32::Storage::FileSystem::GetShortPathNameW;
        use windows::core::PCWSTR;

        let fixture = fixture_dir("short-name");
        let long = fixture.join("System Volume Information");
        std::fs::create_dir(&long).unwrap();

        let input: Vec<u16> = long.as_os_str().encode_wide().chain([0]).collect();
        let mut buffer = vec![0u16; 260];
        // SAFETY: `input` is NUL-terminated and `buffer` is writable.
        let len = unsafe { GetShortPathNameW(PCWSTR(input.as_ptr()), Some(&mut buffer)) } as usize;
        if len == 0 || len >= buffer.len() {
            std::fs::remove_dir_all(fixture).unwrap();
            return;
        }
        buffer.truncate(len);
        let short = PathBuf::from(std::ffi::OsString::from_wide(&buffer));
        if short.file_name() == long.file_name() {
            std::fs::remove_dir_all(fixture).unwrap();
            return;
        }

        assert_eq!(
            resolve_policy_path(&short).unwrap(),
            resolve_policy_path(&long).unwrap()
        );

        std::fs::remove_dir_all(fixture).unwrap();
    }
}
