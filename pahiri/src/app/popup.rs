//! Modal dialogs.

use std::path::PathBuf;

use crate::terminal::ShellId;

/// What to do once a dialog is confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    /// Exit the application.
    Quit,
    /// Leave the settings page without saving.
    LeaveConfigDiscard,
    /// Create a task with the entered id.
    CreateTask,
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

    /// Title shown in the border.
    pub fn title(&self) -> &str {
        match self {
            Popup::Message { title, .. }
            | Popup::Confirm { title, .. }
            | Popup::Input { title, .. } => title,
        }
    }
}
