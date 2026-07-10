//! Delete-boundary policy: paths Windows itself depends on.
//!
//! Blocks the OS structures living at a volume root ($Recycle.Bin, System
//! Volume Information, the paging files), the Windows directory itself, and
//! the well-known critical dirs inside it (System32 etc.) including their
//! contents — nothing in those is ever legitimate cleanup. Everything else
//! under Windows (Temp, SoftwareDistribution, Logs) and the contents of the
//! volume-root entries stay deletable — clearing orphaned recycle-bin data is
//! a legitimate use — and NTFS ACLs still guard individual system files. The
//! check is string-level and runs before the filesystem call: mathom usually
//! runs elevated, so "the OS will refuse" is not a safety net here.

/// Why `path` must never be deleted, or `None` if it's fair game.
pub fn deletion_block_reason(path: &str) -> Option<String> {
    block_reason(path, std::env::var("SystemRoot").ok().as_deref())
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
}
