//! Git operations for the "prepare" command.
//!
//! `prepare` makes every attached workspace ready for a task:
//!
//! 1. If the checkout is dirty, commit everything as a "state saved" commit
//!    (refused on the main branch unless explicitly allowed).
//! 2. `git checkout <main>` and `git pull --ff-only`.
//! 3. `git switch <task>` or `git switch -c <task>` when the branch is new.
//!
//! All commands go through the `git` binary so the behaviour matches what
//! the user would do by hand.

use std::path::{Path, PathBuf};
use std::process::Command;

/// What we know about a checkout before touching it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoState {
    /// Current branch name (`HEAD` when detached).
    pub branch: String,
    /// Whether there are uncommitted changes (tracked or untracked).
    pub dirty: bool,
}

/// One workspace to prepare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareRequest {
    /// Display name.
    pub name: String,
    /// Checkout path.
    pub path: PathBuf,
    /// Main branch for this checkout.
    pub main_branch: String,
    /// Task id, used as the branch name.
    pub task_id: String,
    /// Commit uncommitted work even when the checkout is on the main branch.
    pub commit_on_main: bool,
}

/// Errors from git.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// `git` could not be started.
    #[error("could not run git in {0}: {1}")]
    Spawn(PathBuf, std::io::Error),
    /// A git command exited non-zero.
    #[error("git {args} failed in {path}: {stderr}")]
    Failed {
        /// Arguments that failed.
        args: String,
        /// Checkout path.
        path: PathBuf,
        /// Trimmed stderr.
        stderr: String,
    },
    /// The checkout is dirty on its main branch and committing there was not allowed.
    #[error("{0} is on its main branch '{1}' with uncommitted changes; not a task branch")]
    DirtyOnMain(String, String),
}

fn git(path: &Path, args: &[&str]) -> Result<String, GitError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| GitError::Spawn(path.to_path_buf(), e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_owned())
    } else {
        Err(GitError::Failed {
            args: args.join(" "),
            path: path.to_path_buf(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}

/// Inspect a checkout.
pub fn inspect(path: &Path) -> Result<RepoState, GitError> {
    let branch = git(path, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let status = git(path, &["status", "--porcelain"])?;
    Ok(RepoState {
        branch,
        dirty: !status.trim().is_empty(),
    })
}

/// Whether a local branch exists.
pub fn branch_exists(path: &Path, branch: &str) -> bool {
    git(
        path,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok()
}

/// Run the prepare sequence, reporting progress through `log`.
pub fn prepare(req: &PrepareRequest, log: &mut dyn FnMut(String)) -> Result<(), GitError> {
    let path = req.path.as_path();
    let state = inspect(path)?;
    log(format!(
        "[{}] on '{}'{}",
        req.name,
        state.branch,
        if state.dirty {
            ", uncommitted changes"
        } else {
            ""
        }
    ));

    if state.dirty {
        if state.branch == req.main_branch && !req.commit_on_main {
            return Err(GitError::DirtyOnMain(
                req.name.clone(),
                req.main_branch.clone(),
            ));
        }
        if state.branch == req.main_branch {
            log(format!(
                "[{}] WARNING: committing on main branch '{}'",
                req.name, req.main_branch
            ));
        }
        git(path, &["add", "-A"])?;
        let message = format!(
            "pahiri: state saved on {} before switching to {}",
            state.branch, req.task_id
        );
        git(path, &["commit", "-q", "-m", &message])?;
        log(format!("[{}] committed: {message}", req.name));
    }

    if state.branch == req.task_id {
        log(format!("[{}] already on '{}'", req.name, req.task_id));
        return Ok(());
    }

    if branch_exists(path, &req.task_id) {
        git(path, &["switch", "-q", &req.task_id])?;
        log(format!(
            "[{}] switched to existing branch '{}'",
            req.name, req.task_id
        ));
        return Ok(());
    }

    git(path, &["checkout", "-q", &req.main_branch])?;
    log(format!("[{}] checked out '{}'", req.name, req.main_branch));
    match git(path, &["pull", "-q", "--ff-only"]) {
        Ok(_) => log(format!(
            "[{}] pulled latest '{}'",
            req.name, req.main_branch
        )),
        Err(GitError::Failed { stderr, .. }) => {
            log(format!(
                "[{}] pull skipped: {}",
                req.name,
                stderr.lines().next().unwrap_or("no remote")
            ));
        }
        Err(e) => return Err(e),
    }
    git(path, &["switch", "-q", "-c", &req.task_id])?;
    log(format!(
        "[{}] created branch '{}' from '{}'",
        req.name, req.task_id, req.main_branch
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn repo(main: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", main]).unwrap();
        git(p, &["config", "user.email", "t@example.com"]).unwrap();
        git(p, &["config", "user.name", "t"]).unwrap();
        fs::write(p.join("a.txt"), "a").unwrap();
        git(p, &["add", "-A"]).unwrap();
        git(p, &["commit", "-q", "-m", "init"]).unwrap();
        dir
    }

    fn req(
        dir: &tempfile::TempDir,
        main: &str,
        task: &str,
        commit_on_main: bool,
    ) -> PrepareRequest {
        PrepareRequest {
            name: "ws".into(),
            path: dir.path().to_path_buf(),
            main_branch: main.into(),
            task_id: task.into(),
            commit_on_main,
        }
    }

    #[test]
    fn inspect_reports_branch_and_dirty() {
        let dir = repo("main");
        assert_eq!(
            inspect(dir.path()).unwrap(),
            RepoState {
                branch: "main".into(),
                dirty: false
            }
        );
        fs::write(dir.path().join("b.txt"), "b").unwrap();
        assert!(inspect(dir.path()).unwrap().dirty);
    }

    #[test]
    fn clean_main_creates_task_branch() {
        let dir = repo("main");
        let mut log = Vec::new();
        prepare(&req(&dir, "main", "PROJ-1", false), &mut |l| log.push(l)).unwrap();
        assert_eq!(inspect(dir.path()).unwrap().branch, "PROJ-1");
        assert!(
            log.iter().any(|l| l.contains("created branch 'PROJ-1'")),
            "{log:?}"
        );
        assert!(log.iter().any(|l| l.contains("pull skipped")), "{log:?}");
        // Second run: already on the branch.
        let mut log = Vec::new();
        prepare(&req(&dir, "main", "PROJ-1", false), &mut |l| log.push(l)).unwrap();
        assert!(log.iter().any(|l| l.contains("already on")));
    }

    #[test]
    fn dirty_task_branch_is_committed_before_switching() {
        let dir = repo("main");
        git(dir.path(), &["switch", "-q", "-c", "OLD-1"]).unwrap();
        fs::write(dir.path().join("work.txt"), "wip").unwrap();
        let mut log = Vec::new();
        prepare(&req(&dir, "main", "NEW-2", false), &mut |l| log.push(l)).unwrap();
        assert_eq!(inspect(dir.path()).unwrap().branch, "NEW-2");
        assert!(!inspect(dir.path()).unwrap().dirty);
        let msg = git(dir.path(), &["log", "-1", "--format=%s", "OLD-1"]).unwrap();
        assert_eq!(
            msg,
            "pahiri: state saved on OLD-1 before switching to NEW-2"
        );
        // Switching back reuses the existing branch.
        let mut log = Vec::new();
        prepare(&req(&dir, "main", "OLD-1", false), &mut |l| log.push(l)).unwrap();
        assert!(log.iter().any(|l| l.contains("existing branch")));
        assert!(dir.path().join("work.txt").exists());
    }

    #[test]
    fn dirty_main_is_refused_unless_allowed() {
        let dir = repo("develop");
        fs::write(dir.path().join("x.txt"), "x").unwrap();
        let mut log = Vec::new();
        let err = prepare(&req(&dir, "develop", "T-1", false), &mut |l| log.push(l)).unwrap_err();
        assert!(matches!(err, GitError::DirtyOnMain(..)));
        assert_eq!(inspect(dir.path()).unwrap().branch, "develop");
        prepare(&req(&dir, "develop", "T-1", true), &mut |l| log.push(l)).unwrap();
        assert_eq!(inspect(dir.path()).unwrap().branch, "T-1");
        assert!(log.iter().any(|l| l.contains("WARNING")));
    }

    #[test]
    fn not_a_repo_fails_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(inspect(dir.path()), Err(GitError::Failed { .. })));
    }
}
