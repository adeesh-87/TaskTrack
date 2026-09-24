//! Filesystem-backed task store: discovers task folders and persists the board.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::Board;

/// Errors from the task store.
#[derive(Debug, Error)]
pub enum StoreError {
    /// Filesystem failure.
    #[error("{context}: {source}")]
    Io {
        /// What we were doing.
        context: String,
        /// Underlying error.
        #[source]
        source: io::Error,
    },
    /// A task id that is not a plain folder name.
    #[error("invalid task id {0:?}: use letters, digits, '-', '_' or '.'")]
    InvalidId(String),
    /// Creating a task that already exists.
    #[error("task {0:?} already exists")]
    Exists(String),
}

fn io_err(context: impl Into<String>, source: io::Error) -> StoreError {
    StoreError::Io {
        context: context.into(),
        source,
    }
}

/// A task as shown in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSummary {
    /// Folder name.
    pub id: String,
    /// Absolute folder path.
    pub dir: PathBuf,
    /// Whether the context file is present.
    pub has_context: bool,
}

/// Discovers tasks on disk and reads/writes the board file.
#[derive(Debug)]
pub struct TaskStore {
    tasks_dir: PathBuf,
    status_path: PathBuf,
    context_file: String,
    categories: Vec<String>,
    board: Board,
}

impl TaskStore {
    /// Open the store, reading the board file (if any) and reconciling it with
    /// the folders present. Any correction is written back immediately.
    pub fn open(
        tasks_dir: &Path,
        status_file: &str,
        context_file: &str,
        categories: &[String],
    ) -> Result<Self, StoreError> {
        let status_path = tasks_dir.join(status_file);
        let text = match fs::read_to_string(&status_path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(io_err(format!("reading {}", status_path.display()), e)),
        };
        let mut store = Self {
            tasks_dir: tasks_dir.to_path_buf(),
            status_path,
            context_file: context_file.to_owned(),
            categories: categories.to_vec(),
            board: Board::parse(&text, categories),
        };
        let existed = !text.is_empty();
        if store.refresh()? || !existed {
            store.save()?;
        }
        Ok(store)
    }

    /// Folder the tasks live in.
    pub fn tasks_dir(&self) -> &Path {
        &self.tasks_dir
    }

    /// The current board.
    pub fn board(&self) -> &Board {
        &self.board
    }

    /// Category names.
    pub fn categories(&self) -> &[String] {
        &self.categories
    }

    /// Re-scan the tasks folder and reconcile. Returns whether the board changed.
    pub fn refresh(&mut self) -> Result<bool, StoreError> {
        let ids = discover(&self.tasks_dir)?;
        Ok(self.board.reconcile(&ids))
    }

    /// Persist the board file.
    pub fn save(&self) -> Result<(), StoreError> {
        fs::write(&self.status_path, self.board.render())
            .map_err(|e| io_err(format!("writing {}", self.status_path.display()), e))
    }

    /// Summary of a task by id.
    pub fn summary(&self, id: &str) -> TaskSummary {
        let dir = self.tasks_dir.join(id);
        let has_context = dir.join(&self.context_file).is_file();
        TaskSummary {
            id: id.to_owned(),
            dir,
            has_context,
        }
    }

    /// Move a task to another column and save.
    pub fn move_task(&mut self, id: &str, column: usize) -> Result<bool, StoreError> {
        let moved = self.board.move_to(id, column);
        if moved {
            self.save()?;
        }
        Ok(moved)
    }

    /// Create a new task folder with a context file and add it to `column`.
    pub fn create_task(&mut self, id: &str, column: usize) -> Result<TaskSummary, StoreError> {
        self.create_task_with_context(id, column, &super::context::render_new(id, None, None))
    }

    /// Create a task whose context file starts with `context` (e.g. from a ticket).
    pub fn create_task_with_context(
        &mut self,
        id: &str,
        column: usize,
        context: &str,
    ) -> Result<TaskSummary, StoreError> {
        if !is_valid_id(id) {
            return Err(StoreError::InvalidId(id.to_owned()));
        }
        let dir = self.tasks_dir.join(id);
        if dir.exists() {
            return Err(StoreError::Exists(id.to_owned()));
        }
        fs::create_dir(&dir).map_err(|e| io_err(format!("creating {}", dir.display()), e))?;
        let ctx = dir.join(&self.context_file);
        fs::write(&ctx, context).map_err(|e| io_err(format!("creating {}", ctx.display()), e))?;
        self.board.add(id, column);
        self.save()?;
        Ok(self.summary(id))
    }

    /// Path of a task's context file.
    pub fn context_path(&self, id: &str) -> PathBuf {
        self.tasks_dir.join(id).join(&self.context_file)
    }
}

/// List task folders (non-hidden directories) sorted by name.
pub fn discover(tasks_dir: &Path) -> Result<Vec<String>, StoreError> {
    let entries = fs::read_dir(tasks_dir)
        .map_err(|e| io_err(format!("listing {}", tasks_dir.display()), e))?;
    let mut ids = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| io_err("reading directory entry", e))?;
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        ids.push(name);
    }
    ids.sort();
    Ok(ids)
}

/// Task ids must be plain folder names that are also valid git branch names:
/// no spaces, no leading `-` or `.`, no `..`, not ending in `.lock`.
pub fn is_valid_id(id: &str) -> bool {
    crate::config::is_plain_name(id)
        && !id.contains("..")
        && !id.to_ascii_lowercase().ends_with(".lock")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cats() -> Vec<String> {
        vec!["Planned".into(), "Doing".into(), "Done".into()]
    }

    #[test]
    fn open_creates_status_and_discovers_folders() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("beta")).unwrap();
        fs::create_dir(dir.path().join("alpha")).unwrap();
        fs::create_dir(dir.path().join(".hidden")).unwrap();
        fs::write(dir.path().join("alpha/CONTEXT.md"), "# alpha").unwrap();
        let store = TaskStore::open(dir.path(), "status.md", "CONTEXT.md", &cats()).unwrap();
        assert_eq!(store.board().columns[0].tasks, vec!["alpha", "beta"]);
        assert!(dir.path().join("status.md").is_file());
        assert!(store.summary("alpha").has_context);
        assert!(!store.summary("beta").has_context);
    }

    #[test]
    fn existing_status_is_respected_and_reconciled() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("a")).unwrap();
        fs::create_dir(dir.path().join("b")).unwrap();
        fs::write(dir.path().join("status.md"), "## Doing\n- a\n- ghost\n").unwrap();
        let store = TaskStore::open(dir.path(), "status.md", "CONTEXT.md", &cats()).unwrap();
        assert_eq!(store.board().columns[1].tasks, vec!["a"]);
        assert_eq!(store.board().columns[0].tasks, vec!["b"]);
        let written = fs::read_to_string(dir.path().join("status.md")).unwrap();
        assert!(written.contains("## Doing\n- a\n"));
        assert!(!written.contains("ghost"));
    }

    #[test]
    fn create_and_move_task() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = TaskStore::open(dir.path(), "status.md", "CONTEXT.md", &cats()).unwrap();
        let t = store.create_task("new-task", 0).unwrap();
        assert!(t.has_context);
        assert!(matches!(
            store.create_task("new-task", 0),
            Err(StoreError::Exists(_))
        ));
        assert!(matches!(
            store.create_task("bad/id", 0),
            Err(StoreError::InvalidId(_))
        ));
        assert!(store.move_task("new-task", 2).unwrap());
        let again = TaskStore::open(dir.path(), "status.md", "CONTEXT.md", &cats()).unwrap();
        assert_eq!(again.board().locate("new-task"), Some((2, 0)));
    }
}
