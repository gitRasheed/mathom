//! mathom-core: platform-agnostic tree model, aggregation, treemap layout,
//! and (later) search index. Zero platform-specific code.

pub mod category;
pub mod entry;
pub mod interner;
pub mod tree;
pub mod treemap;

pub use category::{CATEGORY_COUNT, Category, categorize};
pub use entry::{EntryBatch, EntryFlags, FileEntry};
pub use tree::{Node, NodeId, Tree, TreeBuilder};
pub use treemap::{TreemapOptions, TreemapRect, Viewport};
