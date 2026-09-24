//! Per-task working state: file tree, shells, editor, focus and metadata.

use std::io;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::editor::Buffer;
use crate::files::FileTree;
use crate::tasks::context::{read_meta, write_meta};
use crate::tasks::{TaskMeta, TaskSummary};
use crate::terminal::shellrc::{self, TaskEnv};
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
    /// Path of the context file.
    pub context_path: PathBuf,
    /// Metadata from the context file's managed block.
    pub meta: TaskMeta,
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
    /// Per-task env file consumed by the `cd` wrapper.
    pub env_file: Option<PathBuf>,
    next_number: usize,
}

impl TaskContext {
    /// Build the context for a task folder.
    pub fn new(summary: &TaskSummary, context_path: &Path, show_hidden: bool) -> io::Result<Self> {
        let tree = FileTree::new(&summary.dir, show_hidden)?;
        let meta = read_meta(context_path)?;
        Ok(Self {
            id: summary.id.clone(),
            dir: summary.dir.clone(),
            context_path: context_path.to_path_buf(),
            meta,
            tree,
            shells: Vec::new(),
            selected_shell: 0,
            editor: None,
            focus: Focus::Tree,
            show_shell: true,
            zoomed: false,
            env_file: None,
            next_number: 1,
        })
    }

    /// Re-read metadata from the context file (e.g. after the user edited it).
    pub fn reload_meta(&mut self) -> io::Result<()> {
        self.meta = read_meta(&self.context_path)?;
        Ok(())
    }

    /// Persist metadata into the context file.
    pub fn save_meta(&self) -> io::Result<()> {
        write_meta(&self.context_path, &self.id, &self.meta)
    }

    /// Environment describing this task for shells.
    pub fn task_env(&self, config: &Config) -> TaskEnv {
        TaskEnv {
            task: self.id.clone(),
            task_dir: self.dir.clone(),
            code: self
                .meta
                .workspaces
                .iter()
                .filter_map(|n| {
                    config
                        .workspace(n)
                        .map(|w| (w.name.clone(), w.path.clone()))
                })
                .collect(),
            builds: self
                .meta
                .builds
                .iter()
                .filter_map(|n| config.build(n).map(|b| (b.name.clone(), b.path.clone())))
                .collect(),
        }
    }

    /// Rewrite the env file so open shells see the current attachments.
    pub fn refresh_env(&mut self, config: &Config, state_dir: &Path) -> io::Result<()> {
        self.env_file = Some(self.task_env(config).write(state_dir)?);
        Ok(())
    }

    /// Launch a shell in the task folder and focus it.
    pub fn spawn_shell(
        &mut self,
        id: ShellId,
        config: &Config,
        state_dir: &Path,
        events: EventSender,
    ) -> anyhow::Result<()> {
        if self.env_file.is_none() {
            self.refresh_env(config, state_dir)?;
        }
        let launch = shellrc::prepare(state_dir, &config.shell.program, &config.shell.args)?;
        let mut env = self.task_env(config).exports();
        env.push(("PAHIRI".into(), env!("CARGO_PKG_VERSION").into()));
        if let Some(f) = &self.env_file {
            env.push(("PAHIRI_ENV_FILE".into(), f.display().to_string()));
        }
        env.extend(launch.env);
        let opts = SpawnOptions {
            program: config.shell.program.clone(),
            args: launch.args,
            cwd: self.dir.clone(),
            env,
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
        self.selected_shell =
            (self.selected_shell as i64 + i64::from(delta)).clamp(0, max) as usize;
    }

    /// Select shell number `index` (0-based) and focus the terminal. Returns `false` if absent.
    pub fn focus_shell(&mut self, index: usize) -> bool {
        if index >= self.shells.len() {
            return false;
        }
        self.selected_shell = index;
        self.show_shell = true;
        self.focus = Focus::Terminal;
        true
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

    /// Panes that can currently take focus, in cycling order.
    pub fn focus_order(&self) -> Vec<Focus> {
        let mut order = vec![Focus::Tree];
        if self.editor.is_some() {
            order.push(Focus::Editor);
        }
        order.push(Focus::Shells);
        if self.shell_pane_visible() {
            order.push(Focus::Terminal);
        }
        order
    }

    /// Focus the next (or previous) visible pane.
    pub fn cycle_focus(&mut self, forward: bool) {
        let order = self.focus_order();
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

    /// Focus a pane if it is available.
    pub fn set_focus(&mut self, focus: Focus) {
        if self.focus_order().contains(&focus) {
            self.focus = focus;
            if focus != Focus::Terminal {
                self.zoomed = false;
            }
        }
    }

    /// One-line summary of attachments for the sidebar.
    pub fn attachment_summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.meta.workspaces.is_empty() {
            parts.push(format!("code: {}", self.meta.workspaces.join(", ")));
        }
        if !self.meta.builds.is_empty() {
            parts.push(format!("build: {}", self.meta.builds.join(", ")));
        }
        if let Some(b) = &self.meta.branch {
            parts.push(format!("branch: {b}"));
        }
        parts.join(" · ")
    }
}
