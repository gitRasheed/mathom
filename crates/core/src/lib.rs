//! mathom-core: platform-agnostic tree model, aggregation, and (later) treemap
//! layout + search index. Zero platform-specific code.

pub mod entry;
pub mod interner;
pub mod tree;

pub use entry::{EntryBatch, EntryFlags, FileEntry};
pub use tree::{Node, NodeId, Tree, TreeBuilder};
