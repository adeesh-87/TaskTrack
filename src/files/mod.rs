//! File tree model and file operations for a task folder.

pub mod ops;
pub mod tree;

pub use ops::{probe, FileProbe};
pub use tree::{FileTree, Node};
