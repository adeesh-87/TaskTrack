//! Modal dialogs.

use std::path::PathBuf;

use crate::ai::PromptKind;
use crate::tasks::{Checkpoint, Ticket};
use crate::terminal::ShellId;

use super::palette::Action;

/// What to do once a dialog is confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    /// Exit the application.
    Quit,
    /// Leave the settings page without saving.
    LeaveConfigDiscard,
    /// Create a custom task with the entered id.
    CreateTask,
    /// Run the task source script with this name.
    FetchTickets(String),
    /// Create a task from the selected ticket of this source.
    CreateFromTicket(String),
    /// Open the file after the large/binary warning.
    OpenConfirmed(PathBuf),
    /// Open the file, discarding the current editor buffer.
    OpenDiscard(PathBuf),
    /// Close the editor, discarding changes.
    CloseEditorDiscard,
    /// Rename the path to the entered name.
    Rename(PathBuf),
    /// Create a file with the entered name inside the folder.
    CreateFile(PathBuf),
    /// Create a folder with the entered name inside the folder.
    CreateDir(PathBuf),
    /// Delete the path.
    Delete(PathBuf),
    /// Kill and remove the shell.
    CloseShell(ShellId),
    /// Apply the attachment multi-select.
    ApplyAttachments,
    /// Run prepare; `commit_on_main` allows committing on main branches.
    RunPrepare {
        /// Commit dirty main branches instead of skipping those workspaces.
        commit_on_main: bool,
    },
    /// Run a palette action.
    Run(Action),
    /// Move the task to the trash.
    DeleteTask(String),
    /// Start a focus block of the entered minutes on the task.
    StartFocus(String),
    /// Tick the timed checkpoint and start the next.
    TimerDone,
    /// Add minutes to the timer.
    TimerExtend(u64),
    /// Pause the timer.
    TimerPause,
    /// Stop the timer and book the time.
    TimerStop,
    /// Replace the task's checkpoints.
    ReplaceCheckpoints(String, Vec<Checkpoint>),
    /// Record the entered outcome line for the task.
    Outcome(String),
    /// Open a prompt template in the editor.
    EditPrompt(PromptKind),
    /// Start the timer on this task's next checkpoint (or a focus block).
    TimerStart(String),
    /// Ask for focus block minutes for this task.
    StartFocusAsk(String),
    /// Resume the timer; `true` counts the time away.
    TimerResume(bool),
    /// Push for review / rebase.
    GitJob(super::gerrit::GitJob),
    /// Open this task.
    OpenTask(String),
    /// Replace the task's `## Description` with this text.
    RefreshDescription(String, String),
    /// Search the editor for the entered text.
    Find,
    /// Nothing (used by cancel options).
    Nothing,
}

/// An item of a single-choice list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// Text shown.
    pub label: String,
    /// What happens when chosen.
    pub pending: Pending,
}

/// A checkbox row of a multi-select list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckItem {
    /// Text shown.
    pub label: String,
    /// Identifier passed back on apply (`kind:name`).
    pub key: String,
    /// Whether it is ticked.
    pub checked: bool,
}

/// A modal dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Popup {
    /// Informational text, dismissed by any key.
    Message {
        /// Title.
        title: String,
        /// Body.
        body: String,
    },
    /// Yes/no question.
    Confirm {
        /// Title.
        title: String,
        /// Body.
        body: String,
        /// Action on yes.
        pending: Pending,
    },
    /// Single-line text input.
    Input {
        /// Title.
        title: String,
        /// Field label.
        label: String,
        /// Current value.
        value: String,
        /// Action on submit.
        pending: Pending,
    },
    /// Pick one option.
    Choose {
        /// Title.
        title: String,
        /// Options.
        choices: Vec<Choice>,
        /// Highlighted option.
        selected: usize,
    },
    /// Tick any number of options.
    MultiSelect {
        /// Title.
        title: String,
        /// Rows.
        items: Vec<CheckItem>,
        /// Highlighted row.
        selected: usize,
        /// Action on apply.
        pending: Pending,
    },
    /// Pick a ticket (filterable).
    Tickets {
        /// Source name.
        source: String,
        /// All tickets.
        tickets: Vec<Ticket>,
        /// Filter text.
        filter: String,
        /// Highlighted index into the filtered list.
        selected: usize,
    },
    /// Streaming log of a background job.
    Log {
        /// Title.
        title: String,
        /// Lines so far.
        lines: Vec<String>,
        /// Whether the job finished (any key closes).
        done: bool,
    },
    /// The command palette.
    Palette(super::palette::Palette),
    /// Scrollable read-only text.
    Doc {
        /// Title.
        title: String,
        /// Lines.
        lines: Vec<String>,
        /// First visible line.
        scroll: usize,
    },
    /// The help page.
    Help {
        /// (title, lines) per tab.
        tabs: Vec<(String, Vec<String>)>,
        /// Current tab.
        tab: usize,
        /// First visible line.
        scroll: usize,
    },
    /// The checkpoint list of a task.
    Checkpoints {
        /// Task id.
        task_id: String,
        /// Checkpoints.
        items: Vec<Checkpoint>,
        /// Highlighted row.
        selected: usize,
    },
}

impl Popup {
    /// Informational popup.
    pub fn message(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self::Message {
            title: title.into(),
            body: body.into(),
        }
    }

    /// Yes/no popup.
    pub fn confirm(title: impl Into<String>, body: impl Into<String>, pending: Pending) -> Self {
        Self::Confirm {
            title: title.into(),
            body: body.into(),
            pending,
        }
    }

    /// Text input popup.
    pub fn input(
        title: impl Into<String>,
        label: impl Into<String>,
        value: impl Into<String>,
        pending: Pending,
    ) -> Self {
        Self::Input {
            title: title.into(),
            label: label.into(),
            value: value.into(),
            pending,
        }
    }

    /// Single-choice popup.
    pub fn choose(title: impl Into<String>, choices: Vec<Choice>) -> Self {
        Self::Choose {
            title: title.into(),
            choices,
            selected: 0,
        }
    }

    /// Scrollable text popup.
    pub fn doc(title: impl Into<String>, text: &str) -> Self {
        Self::Doc {
            title: title.into(),
            lines: text.lines().map(str::to_owned).collect(),
            scroll: 0,
        }
    }

    /// Log popup.
    pub fn log(title: impl Into<String>) -> Self {
        Self::Log {
            title: title.into(),
            lines: Vec::new(),
            done: false,
        }
    }

    /// Title shown in the border.
    pub fn title(&self) -> &str {
        match self {
            Popup::Message { title, .. }
            | Popup::Confirm { title, .. }
            | Popup::Input { title, .. }
            | Popup::Choose { title, .. }
            | Popup::MultiSelect { title, .. }
            | Popup::Log { title, .. }
            | Popup::Doc { title, .. } => title,
            Popup::Tickets { source, .. } => source,
            Popup::Palette(_) => "commands",
            Popup::Checkpoints { .. } => "checkpoints",
            Popup::Help { .. } => "help",
        }
    }

    /// Whether the popup blocks until a job finishes.
    pub fn is_busy(&self) -> bool {
        matches!(self, Popup::Log { done: false, .. })
    }
}

/// Tickets matching a filter (case-insensitive substring on id, title, url).
pub fn filter_tickets<'a>(tickets: &'a [Ticket], filter: &str) -> Vec<&'a Ticket> {
    let f = filter.to_lowercase();
    tickets
        .iter()
        .filter(|t| {
            f.is_empty()
                || t.id.to_lowercase().contains(&f)
                || t.title.to_lowercase().contains(&f)
                || t.url.to_lowercase().contains(&f)
        })
        .collect()
}
