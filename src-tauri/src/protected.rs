//! Delete-boundary policy: paths Windows itself depends on.
//!
//! Blocks the OS structures living at a volume root ($Recycle.Bin, System
//! Volume Information, the paging files) and the Windows directory itself.
//! Their *contents* stay deletable — clearing orphaned recycle-bin data is a
//! legitimate use — and NTFS ACLs still guard individual system files. The
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

    if let Some(root) = system_root
        && trimmed.eq_ignore_ascii_case(root.trim_end_matches(['\\', '/']))
    {
        return Some("the Windows directory can't be deleted".into());
    }
    None
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
    fn blocks_the_windows_directory_itself_only() {
        assert!(block_reason("C:\\Windows", SYSROOT).is_some());
        assert!(block_reason("c:\\WINDOWS\\", SYSROOT).is_some());
        // Same name on another drive is just a folder.
        assert!(block_reason("D:\\Windows", SYSROOT).is_none());
        // Contents stay deletable (Temp cleanup is legitimate; ACLs guard
        // the rest).
        assert!(block_reason("C:\\Windows\\Temp", SYSROOT).is_none());
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
