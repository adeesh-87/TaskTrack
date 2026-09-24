//! Task discovery and board state.
//!
//! A task is a folder under the configured tasks directory. The board (which
//! category each task is in) lives in a small Markdown file, `status.md` by
//! default, with one heading per category and one list item per task:
//!
//! ```markdown
//! # Status
//!
//! ## Planned
//! - write-docs
//!
//! ## Doing
//! - fix-login
//!
//! ## Done
//! ```
//!
//! Keeping the board in Markdown means it stays human-editable, diffs well in
//! git and matches the file the user already maintains by hand.

pub mod board;
pub mod store;

pub use board::Board;
pub use store::{TaskStore, TaskSummary};
