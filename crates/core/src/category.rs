//! File-type categories shared by the treemap and type panel.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Category {
    Directory = 0,
    Video = 1,
    Audio = 2,
    Image = 3,
    Archive = 4,
    Document = 5,
    Code = 6,
    Executable = 7,
    System = 8,
    Data = 9,
    Other = 10,
}

pub const CATEGORY_COUNT: usize = 11;

/// Inline lowercased extension, capped at 8 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExtKey {
    len: u8,
    bytes: [u8; 8],
}

impl ExtKey {
    pub fn as_str(&self) -> &str {
        // Bytes were sliced from a valid &str; ASCII-lowercasing keeps UTF-8.
        std::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }
}

/// Extension after the last dot, ASCII-lowercased, capped at 8 bytes.
pub fn extension_key(name: &str) -> Option<ExtKey> {
    let dot = name.rfind('.')?;
    let ext = &name[dot + 1..];
    if ext.is_empty() || ext.len() > 8 {
        return None;
    }
    let mut bytes = [0u8; 8];
    bytes[..ext.len()].copy_from_slice(ext.as_bytes());
    bytes.make_ascii_lowercase();
    Some(ExtKey {
        len: ext.len() as u8,
        bytes,
    })
}

/// Categorize by file extension; unknowns are `Other`.
pub fn categorize(name: &str, is_dir: bool) -> Category {
    if is_dir {
        return Category::Directory;
    }
    match extension_key(name) {
        Some(key) => categorize_ext(&key),
        None => Category::Other,
    }
}

pub fn categorize_ext(key: &ExtKey) -> Category {
    match &key.bytes[..key.len as usize] {
        // "ts" is ambiguous (MPEG stream vs TypeScript); it lives in Code.
        b"mp4" | b"mkv" | b"avi" | b"mov" | b"wmv" | b"flv" | b"webm" | b"m4v" | b"mpg"
        | b"mpeg" | b"m2ts" | b"vob" => Category::Video,
        b"mp3" | b"flac" | b"wav" | b"aac" | b"ogg" | b"opus" | b"m4a" | b"wma" | b"mid"
        | b"aiff" => Category::Audio,
        b"jpg" | b"jpeg" | b"png" | b"gif" | b"bmp" | b"webp" | b"tiff" | b"tif" | b"svg"
        | b"ico" | b"heic" | b"raw" | b"cr2" | b"nef" | b"dng" | b"psd" => Category::Image,
        b"zip" | b"rar" | b"7z" | b"tar" | b"gz" | b"bz2" | b"xz" | b"zst" | b"iso" | b"cab"
        | b"wim" => Category::Archive,
        b"pdf" | b"doc" | b"docx" | b"xls" | b"xlsx" | b"ppt" | b"pptx" | b"txt" | b"md"
        | b"rtf" | b"odt" | b"ods" | b"epub" | b"mobi" | b"csv" => Category::Document,
        b"rs" | b"c" | b"cpp" | b"h" | b"hpp" | b"cs" | b"java" | b"py" | b"js" | b"ts"
        | b"tsx" | b"jsx" | b"go" | b"rb" | b"php" | b"html" | b"css" | b"scss" | b"json"
        | b"xml" | b"yaml" | b"yml" | b"toml" | b"sql" | b"sh" | b"ps1" | b"bat" | b"lock" => {
            Category::Code
        }
        b"exe" | b"msi" | b"com" | b"scr" | b"appx" | b"msix" | b"jar" => Category::Executable,
        b"dll" | b"sys" | b"drv" | b"ocx" | b"cpl" | b"efi" | b"mui" | b"etl" | b"dmp" | b"pdb"
        | b"winmd" => Category::System,
        b"db" | b"sqlite" | b"sqlite3" | b"mdb" | b"log" | b"dat" | b"bin" | b"idx" | b"bak"
        | b"pak" | b"vhd" | b"vhdx" | b"vmdk" | b"qcow2" => Category::Data,
        _ => Category::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_extensions_map_to_their_category() {
        assert_eq!(categorize("movie.mkv", false), Category::Video);
        assert_eq!(categorize("song.flac", false), Category::Audio);
        assert_eq!(categorize("photo.jpeg", false), Category::Image);
        assert_eq!(categorize("backup.7z", false), Category::Archive);
        assert_eq!(categorize("thesis.pdf", false), Category::Document);
        assert_eq!(categorize("main.rs", false), Category::Code);
        assert_eq!(categorize("setup.exe", false), Category::Executable);
        assert_eq!(categorize("kernel32.dll", false), Category::System);
        assert_eq!(categorize("index.db", false), Category::Data);
    }

    #[test]
    fn ts_means_typescript_not_transport_stream() {
        assert_eq!(categorize("main.ts", false), Category::Code);
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        assert_eq!(categorize("MOVIE.MKV", false), Category::Video);
        assert_eq!(categorize("Photo.JPG", false), Category::Image);
    }

    #[test]
    fn unknown_or_missing_extension_is_other() {
        assert_eq!(categorize("README", false), Category::Other);
        assert_eq!(categorize("weird.xyz123", false), Category::Other);
        assert_eq!(categorize("trailing.", false), Category::Other);
    }

    #[test]
    fn directories_are_directory_regardless_of_name() {
        assert_eq!(categorize("videos.mp4", true), Category::Directory);
    }
}
