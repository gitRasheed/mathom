//! Scan output data model: what any `Scanner` backend emits and what
//! `TreeBuilder` consumes. Plain data, no behavior.

/// Bit flags describing a scanned entry. Stored per node (u16).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct EntryFlags(pub u16);

impl EntryFlags {
    pub const DIR: EntryFlags = EntryFlags(1 << 0);
    /// Symlink, junction, or other name-surrogate reparse point. Never descended.
    pub const REPARSE: EntryFlags = EntryFlags(1 << 1);
    /// Directory could not be read (access denied, vanished mid-scan, ...).
    pub const ERROR: EntryFlags = EntryFlags(1 << 2);
    /// NTFS-compressed (MFT backend only).
    pub const COMPRESSED: EntryFlags = EntryFlags(1 << 3);
    /// Sparse file (MFT backend only).
    pub const SPARSE: EntryFlags = EntryFlags(1 << 4);
    /// Cloud placeholder, dehydrated (MFT backend only).
    pub const PLACEHOLDER: EntryFlags = EntryFlags(1 << 5);
    /// One of several hardlinks to the same file record (MFT backend only).
    pub const HARDLINK: EntryFlags = EntryFlags(1 << 6);

    pub fn contains(self, other: EntryFlags) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn insert(&mut self, other: EntryFlags) {
        self.0 |= other.0;
    }

    pub fn union(self, other: EntryFlags) -> EntryFlags {
        EntryFlags(self.0 | other.0)
    }

    pub fn is_dir(self) -> bool {
        self.contains(EntryFlags::DIR)
    }
}

/// One scanned file or directory. Names live in the owning batch's `names`
/// buffer (`name_off..name_off + name_len`) so a batch is two allocations,
/// not thousands.
#[derive(Clone, Copy, Debug)]
pub struct FileEntry {
    /// Dense id assigned by the scanner; doubles as the arena node index.
    pub path_id: u32,
    /// Parent's `path_id`. The scanner guarantees the parent entry was
    /// emitted in an earlier batch (root: `parent_id == path_id == 0`).
    pub parent_id: u32,
    pub name_off: u32,
    pub name_len: u16,
    pub flags: EntryFlags,
    /// Logical size in bytes. Directories report 0 (their aggregate is
    /// computed from children).
    pub size: u64,
    /// On-disk allocated size. Generic walker approximates this as `size`;
    /// the MFT backend reports real allocation.
    pub allocated_size: u64,
    /// Modification time, seconds since Unix epoch (0 if unavailable).
    pub mtime: i64,
}

/// A batch of entries sharing one name buffer.
#[derive(Clone, Debug, Default)]
pub struct EntryBatch {
    pub names: String,
    pub entries: Vec<FileEntry>,
}

impl EntryBatch {
    pub fn with_capacity(entries: usize, name_bytes: usize) -> Self {
        EntryBatch {
            names: String::with_capacity(name_bytes),
            entries: Vec::with_capacity(entries),
        }
    }

    /// Appends an entry, copying `name` into the shared buffer.
    /// Names longer than u16::MAX bytes are truncated (path components on
    /// every real filesystem are far shorter).
    pub fn push(&mut self, name: &str, mut entry: FileEntry) {
        let name = truncate_to_u16(name);
        entry.name_off = self.names.len() as u32;
        entry.name_len = name.len() as u16;
        self.names.push_str(name);
        self.entries.push(entry);
    }

    pub fn name_of(&self, entry: &FileEntry) -> &str {
        let start = entry.name_off as usize;
        &self.names[start..start + entry.name_len as usize]
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

fn truncate_to_u16(name: &str) -> &str {
    if name.len() <= u16::MAX as usize {
        return name;
    }
    let mut end = u16::MAX as usize;
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}
