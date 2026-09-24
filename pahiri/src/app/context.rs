//! Per-task working state: file tree, shells, editor and focus.

use std::io;
use std::path::PathBuf;

use crate::config::Config;
use crate::editor::Buffer;
use crate::files::FileTree;
use crate::tasks::TaskSummary;
use crate::terminal::{PtySession, ShellId, SpawnOptions};

use super::event::{AppEvent, EventSender};

/// Which pane receives keys inside a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// The file tree (top-left).
    Tree,
    /// The shell list (bottom-left).
    Shells,
    /// The document editor (top-right).
    Editor,
    /// The embedded terminal (bottom-right).
    Terminal,
}

/// A shell belonging to a task.
#[derive(Debug)]
pub struct Shell {
    /// The PTY session.
    pub session: PtySession,
    /// Sequence number within the task (1-based, for display).
    pub number: usize,
}

impl Shell {
    /// Display name: `zsh #2` or the window title when the program set one.
    pub fn display_name(&self) -> String {
        let base = format!("{} #{}", self.session.label(), self.number);
        match self.session.title() {
            Some(t) if !t.is_empty() => format!("{base} · {t}"),
            _ => base,
        }
    }
}

/// Everything pahiri remembers about an opened task while it runs.
#[derive(Debug)]
pub struct TaskContext {
    /// Task id (folder name).
    pub id: String,
    /// Task folder.
    pub dir: PathBuf,
    /// File tree over the task folder.
    pub tree: FileTree,
    /// Shells opened for this task.
    pub shells: Vec<Shell>,
    /// Index of the selected shell.
    pub selected_shell: usize,
    /// The one open document, if any.
    pub editor: Option<Buffer>,
    /// Focused pane.
    pub focus: Focus,
    /// Whether the terminal pane is shown (when there are shells).
    pub show_shell: bool,
    /// Whether the terminal is zoomed to the whole screen.
    pub zoomed: bool,
    next_number: usize,
}

impl TaskContext {
    /// Build the context for a task folder.
    pub fn new(summary: &TaskSummary, show_hidden: bool) -> io::Result<Self> {
        let tree = FileTree::new(&summary.dir, show_hidden)?;
        Ok(Self {
            id: summary.id.clone(),
            dir: summary.dir.clone(),
            tree,
            shells: Vec::new(),
            selected_shell: 0,
            editor: None,
            focus: Focus::Tree,
            show_shell: true,
            zoomed: false,
            next_number: 1,
        })
    }

    /// Launch a shell in the task folder and focus it.
    pub fn spawn_shell(
        &mut self,
        id: ShellId,
        config: &Config,
        events: EventSender,
    ) -> anyhow::Result<()> {
        let opts = SpawnOptions {
            program: config.shell.program.clone(),
            args: config.shell.args.clone(),
            cwd: self.dir.clone(),
            env: vec![
                ("PAHIRI".into(), env!("CARGO_PKG_VERSION").into()),
                ("PAHIRI_TASK".into(), self.id.clone()),
                ("PAHIRI_TASK_DIR".into(), self.dir.display().to_string()),
            ],
            rows: 24,
            cols: 80,
            scrollback: config.scrollback_lines,
        };
        let session = PtySession::spawn(id, &opts, move |e| events.send(AppEvent::Pty(e)))?;
        let number = self.next_number;
        self.next_number += 1;
        self.shells.push(Shell { session, number });
        self.selected_shell = self.shells.len() - 1;
        self.show_shell = true;
        self.focus = Focus::Terminal;
        Ok(())
    }

    /// Whether the terminal pane should be drawn.
    pub fn shell_pane_visible(&self) -> bool {
        self.show_shell && !self.shells.is_empty()
    }

    /// The selected shell.
    pub fn active_shell(&self) -> Option<&Shell> {
        self.shells.get(self.selected_shell)
    }

    /// The selected shell, mutably.
    pub fn active_shell_mut(&mut self) -> Option<&mut Shell> {
        self.shells.get_mut(self.selected_shell)
    }

    /// Find a shell by id.
    pub fn shell_mut(&mut self, id: ShellId) -> Option<&mut Shell> {
        self.shells.iter_mut().find(|s| s.session.id() == id)
    }

    /// Move the shell selection by `delta`, clamped.
    pub fn select_shell(&mut self, delta: i32) {
        if self.shells.is_empty() {
            self.selected_shell = 0;
            return;
        }
        let max = self.shells.len() as i64 - 1;
        let next = (self.selected_shell as i64 + i64::from(delta)).clamp(0, max);
        self.selected_shell = next as usize;
    }

    /// Kill and drop the selected shell.
    pub fn remove_active_shell(&mut self) {
        if self.selected_shell < self.shells.len() {
            self.shells.remove(self.selected_shell);
            self.selected_shell = self.selected_shell.min(self.shells.len().saturating_sub(1));
        }
    }

    /// Kill and drop the shell with `id`.
    pub fn remove_shell(&mut self, id: ShellId) {
        if let Some(i) = self.shells.iter().position(|s| s.session.id() == id) {
            self.shells.remove(i);
            self.selected_shell = self.selected_shell.min(self.shells.len().saturating_sub(1));
        }
    }

    /// Number of shells whose process is still running.
    pub fn live_shells(&self) -> usize {
        self.shells
            .iter()
            .filter(|s| !s.session.has_exited())
            .count()
    }

    /// Focus the next (or previous) visible pane.
    pub fn cycle_focus(&mut self, forward: bool) {
        let mut order = vec![Focus::Tree, Focus::Shells];
        if self.editor.is_some() {
            order.push(Focus::Editor);
        }
        if self.shell_pane_visible() {
            order.push(Focus::Terminal);
        }
        let idx = order.iter().position(|f| *f == self.focus).unwrap_or(0);
        let next = if forward {
            (idx + 1) % order.len()
        } else {
            (idx + order.len() - 1) % order.len()
        };
        self.focus = order[next];
        if self.focus != Focus::Terminal {
            self.zoomed = false;
        }
    }
}
