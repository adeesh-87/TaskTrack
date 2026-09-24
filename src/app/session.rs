//! What survives a restart: the timer and each task's shells (`session.json`).

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::timer::{SavedTimer, Timer};
use super::App;

/// One shell to reopen.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SavedShell {
    /// Working directory when pahiri closed.
    pub cwd: Option<PathBuf>,
    /// tmux session to reattach.
    pub tmux: Option<String>,
}

/// Saved session state.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Session {
    /// The timer.
    pub timer: Option<SavedTimer>,
    /// Shells per task.
    pub shells: BTreeMap<String, Vec<SavedShell>>,
    /// Task that was open.
    pub active_task: Option<String>,
}

const FILE: &str = "session.json";

/// Read `session.json` (missing or broken → empty).
pub fn load(state_dir: &Path) -> Session {
    fs::read_to_string(state_dir.join(FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Write `session.json`.
pub fn save(state_dir: &Path, session: &Session) -> io::Result<()> {
    fs::create_dir_all(state_dir)?;
    let text = serde_json::to_string_pretty(session).map_err(io::Error::other)?;
    fs::write(state_dir.join(FILE), text)
}

impl App {
    /// Snapshot of the timer and the shells.
    pub(super) fn session_snapshot(&self) -> Session {
        let now = Instant::now();
        let secs = crate::time::now_secs();
        let mut shells = BTreeMap::new();
        for (id, ctx) in &self.contexts {
            let list: Vec<SavedShell> = ctx
                .shells
                .iter()
                .filter(|s| !s.session.has_exited())
                .map(|s| SavedShell {
                    cwd: s.session.cwd(),
                    tmux: s.tmux.clone(),
                })
                .collect();
            if !list.is_empty() {
                shells.insert(id.clone(), list);
            }
        }
        // Tasks not opened in this run keep what the last run saved.
        for (id, list) in &self.restore_shells {
            shells.entry(id.clone()).or_insert_with(|| list.clone());
        }
        Session {
            timer: self.timer.as_ref().map(|t| t.save(now, secs)),
            shells,
            active_task: self.active_task.clone(),
        }
    }

    pub(super) fn save_session(&mut self) {
        let snap = self.session_snapshot();
        if let Err(e) = save(&self.state_dir, &snap) {
            tracing::warn!("could not save session: {e}");
        }
        self.session_saved = Instant::now();
    }

    /// Load the timer and remember the shells to reopen.
    pub(super) fn restore_session(&mut self) {
        let s = load(&self.state_dir);
        if let Some(saved) = &s.timer {
            let exists = self
                .store
                .as_ref()
                .is_some_and(|st| st.tasks_dir().join(&saved.task_id).is_dir());
            if exists {
                let t = Timer::restore(saved, crate::time::now_secs());
                self.set_status(format!(
                    "timer restored (paused): {} · {} · Esc m to resume",
                    t.task_id,
                    t.what()
                ));
                self.timer = Some(t);
            }
        }
        if self.config.restore_shells {
            self.restore_shells = s.shells;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), Session::default());
        let mut s = Session {
            active_task: Some("A".into()),
            ..Session::default()
        };
        s.shells.insert(
            "A".into(),
            vec![SavedShell {
                cwd: Some("/tmp".into()),
                tmux: None,
            }],
        );
        save(dir.path(), &s).unwrap();
        assert_eq!(load(dir.path()), s);
        fs::write(dir.path().join(FILE), "{broken").unwrap();
        assert_eq!(load(dir.path()), Session::default());
    }
}
