//! Name interning: every unique path component is stored once in a single
//! byte buffer. Nodes refer to names as (offset, len) pairs, so 10M nodes
//! with heavily repeated names (node_modules, .git, ...) cost a fraction of
//! per-node Strings.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

/// A reference to an interned name: offset + length into the shared buffer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NameRef {
    pub off: u32,
    pub len: u16,
}

#[derive(Default)]
pub struct NameInterner {
    bytes: String,
    /// name hash -> candidate refs (collision chains verified against `bytes`).
    map: HashMap<u64, Vec<NameRef>>,
}

impl NameInterner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Interns `name`, returning a stable reference. `name` must be at most
    /// u16::MAX bytes (callers batch-truncate before this point).
    pub fn intern(&mut self, name: &str) -> NameRef {
        assert!(name.len() <= u16::MAX as usize);
        let hash = hash_name(name);
        let candidates = self.map.entry(hash).or_default();
        for &r in candidates.iter() {
            if resolve(&self.bytes, r) == name {
                return r;
            }
        }
        let off = u32::try_from(self.bytes.len()).expect("name buffer exceeded 4 GiB");
        let r = NameRef {
            off,
            len: name.len() as u16,
        };
        self.bytes.push_str(name);
        candidates.push(r);
        r
    }

    pub fn get(&self, r: NameRef) -> &str {
        resolve(&self.bytes, r)
    }

    pub(crate) fn release_index(&mut self) -> HashMap<u64, Vec<NameRef>> {
        std::mem::take(&mut self.map)
    }

    /// Offsets in outstanding `NameRef`s survive: contents are unchanged.
    pub(crate) fn shrink_to_fit(&mut self) {
        self.bytes.shrink_to_fit();
    }

    /// Total bytes of unique name data stored.
    pub fn bytes_used(&self) -> usize {
        self.bytes.len()
    }
}

fn resolve(bytes: &str, r: NameRef) -> &str {
    let start = r.off as usize;
    &bytes[start..start + r.len as usize]
}

fn hash_name(name: &str) -> u64 {
    let mut h = DefaultHasher::new();
    name.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_same_name_returns_same_ref_and_stores_once() {
        let mut i = NameInterner::new();
        let a = i.intern("node_modules");
        let b = i.intern("node_modules");
        assert_eq!(a, b);
        assert_eq!(i.bytes_used(), "node_modules".len());
        assert_eq!(i.get(a), "node_modules");
    }

    #[test]
    fn distinct_names_get_distinct_refs() {
        let mut i = NameInterner::new();
        let a = i.intern("alpha");
        let b = i.intern("beta");
        assert_ne!(a, b);
        assert_eq!(i.get(a), "alpha");
        assert_eq!(i.get(b), "beta");
        assert_eq!(i.bytes_used(), "alphabeta".len());
    }

    #[test]
    fn empty_name_is_valid() {
        let mut i = NameInterner::new();
        let r = i.intern("");
        assert_eq!(i.get(r), "");
    }

    #[test]
    fn releasing_index_keeps_names_readable() {
        let mut i = NameInterner::new();
        let r = i.intern("node_modules");

        let dead = i.release_index();

        assert_eq!(dead.len(), 1);
        assert!(i.map.is_empty());
        assert_eq!(i.get(r), "node_modules");
    }
}
