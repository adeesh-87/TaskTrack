//! Embedded terminals: PTY sessions, key encoding and screen rendering.
//!
//! The important design point is that a shell never touches the real
//! terminal. It runs on a pseudo-terminal whose output is fed into a
//! [`vt100`] parser, and the parsed screen is painted into pahiri's own
//! layout. Full-screen programs (agent CLIs, editors, pagers) simply see a
//! terminal of the pane's size — including the alternate screen — so they
//! can never wipe out the surrounding UI.

pub mod keys;
pub mod pty;
pub mod render;
pub mod shellrc;

pub use pty::{PtyEvent, PtySession, ShellId, SpawnOptions};
pub use render::{cursor_position, TerminalView};
