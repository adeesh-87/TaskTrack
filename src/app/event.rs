//! Events flowing into the application from input, PTY and job threads.

use std::sync::mpsc;

use crate::tasks::{GerritRef, Ticket};
use crate::terminal::PtyEvent;

/// Progress and results of background jobs (scripts, git).
#[derive(Debug)]
pub enum JobEvent {
    /// A progress line for the log popup.
    Log(String),
    /// A task-source script finished.
    Tickets {
        /// Source name.
        source: String,
        /// Tickets, or an error message.
        result: Result<Vec<Ticket>, String>,
        /// A dry run from the settings page (show, don't create).
        test: bool,
    },
    /// Gerrit detection finished.
    Gerrit {
        /// Task id.
        task_id: String,
        /// Changes found.
        found: Vec<GerritRef>,
        /// Whether at least one workspace could be scanned.
        scanned: bool,
    },
    /// A one-shot agent call finished.
    Agent {
        /// Task id.
        task_id: String,
        /// Which prompt ran.
        kind: crate::ai::PromptKind,
        /// Agent stdout, or an error.
        result: Result<String, String>,
    },
    /// The prepare sequence finished for all workspaces.
    PrepareDone {
        /// Task id that was prepared.
        task_id: String,
        /// Workspaces that were switched successfully.
        succeeded: Vec<String>,
        /// Workspaces that failed, with the reason.
        failed: Vec<(String, String)>,
    },
}

/// Everything the main loop reacts to.
#[derive(Debug)]
pub enum AppEvent {
    /// A terminal input event.
    Input(crossterm::event::Event),
    /// Output or exit from a shell.
    Pty(PtyEvent),
    /// Progress from a background job.
    Job(JobEvent),
    /// Periodic tick (used to redraw when nothing else happens).
    Tick,
}

/// Cloneable sender handed to background threads.
#[derive(Debug, Clone)]
pub struct EventSender(mpsc::Sender<AppEvent>);

impl EventSender {
    /// Create a channel pair.
    pub fn channel() -> (Self, mpsc::Receiver<AppEvent>) {
        let (tx, rx) = mpsc::channel();
        (Self(tx), rx)
    }

    /// Send an event (ignored if the receiver is gone).
    pub fn send(&self, event: AppEvent) {
        let _ = self.0.send(event);
    }
}
