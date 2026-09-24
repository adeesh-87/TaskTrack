//! pahiri — a terminal workspace for executing and managing tasks.
//!
//! The crate is split into small, independently testable layers:
//!
//! * [`config`] — on-disk configuration and key-combo parsing.
//! * [`tasks`] — task discovery and the `status.md` board (state file).
//! * [`files`] — the lazily expanded file tree and file operations.
//! * [`git`] — the "prepare" sequence run against attached workspaces.
//! * [`highlight`] — small hand-written syntax highlighters for the editor.
//! * [`editor`] — a minimal text buffer for viewing/editing one file.
//! * [`terminal`] — PTY sessions, key encoding, and VT screen rendering.
//! * [`ai`] — prompt templates and one-shot agent calls.
//! * [`app`] — the state machine that ties everything together.
//! * [`ui`] — pure drawing code (ratatui) over the app state.
//! * [`cli`] — `pahiri task …`, `pahiri report`, `pahiri install-skills`.

pub mod ai;
pub mod app;
pub mod cli;
pub mod config;
pub mod editor;
pub mod files;
pub mod git;
pub mod highlight;
pub mod tasks;
pub mod terminal;
pub mod time;
pub mod ui;
