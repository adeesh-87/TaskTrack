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

/// Run git for a network operation: never prompt (batch-mode SSH unless the
/// user set their own `GIT_SSH_COMMAND`), and return stdout + stderr.
fn git_net(path: &Path, args: &[&str]) -> Result<String, GitError> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(path)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0");
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    let output = cmd
        .output()
        .map_err(|e| GitError::Spawn(path.to_path_buf(), e))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .trim_end()
    .to_owned();
    if output.status.success() {
        Ok(text)
    } else {
        Err(GitError::Failed {
            args: args.join(" "),
            path: path.to_path_buf(),
            stderr: text,
        })
    }
}

/// Whether the checkout has an `origin` remote.
pub fn has_origin(path: &Path) -> bool {
    git(path, &["remote", "get-url", "origin"]).is_ok()
}

/// `git fetch origin <branch>`.
pub fn fetch_branch(path: &Path, branch: &str) -> Result<(), GitError> {
    git_net(path, &["fetch", "-q", "origin", branch]).map(|_| ())
}

/// Seconds since the last fetch (from `FETCH_HEAD`), if there was one.
pub fn last_fetch_age(path: &Path) -> Option<u64> {
    let p = git(path, &["rev-parse", "--git-path", "FETCH_HEAD"]).ok()?;
    let p = if Path::new(&p).is_absolute() {
        PathBuf::from(p)
    } else {
        path.join(p)
    };
    let modified = std::fs::metadata(p).and_then(|m| m.modified()).ok()?;
    modified.elapsed().ok().map(|d| d.as_secs())
}

/// Whether Gerrit's `commit-msg` hook (which adds `Change-Id`) is installed.
pub fn has_change_id_hook(path: &Path) -> bool {
    let Ok(p) = git(path, &["rev-parse", "--git-path", "hooks/commit-msg"]) else {
        return false;
    };
    let p = if Path::new(&p).is_absolute() {
        PathBuf::from(p)
    } else {
        path.join(p)
    };
    std::fs::read_to_string(p).is_ok_and(|t| t.contains("Change-Id"))
}

/// `git push origin HEAD:refs/for/<main>`; returns git's output (Gerrit prints the change URLs).
pub fn push_for_review(path: &Path, main_branch: &str) -> Result<String, GitError> {
    git_net(
        path,
        &["push", "origin", &format!("HEAD:refs/for/{main_branch}")],
    )
}

/// Rebase the current branch onto the latest main (`origin/<main>` after a
/// fetch when there is an origin). A conflicting rebase is aborted.
pub fn rebase_on_main(path: &Path, main_branch: &str) -> Result<String, GitError> {
    let state = inspect(path)?;
    if state.dirty {
        return Err(GitError::Failed {
            args: "rebase".into(),
            path: path.to_path_buf(),
            stderr: "uncommitted changes; commit or stash them first".into(),
        });
    }
    let onto = if has_origin(path) {
        fetch_branch(path, main_branch)?;
        format!("origin/{main_branch}")
    } else {
        main_branch.to_owned()
    };
    match git(path, &["rebase", "-q", &onto]) {
        Ok(_) => Ok(format!("rebased '{}' onto {onto}", state.branch)),
        Err(e) => {
            let _ = git(path, &["rebase", "--abort"]);
            Err(e)
        }
    }
}

/// Guess a checkout's main branch: `origin/HEAD` when the remote advertises
/// it, else the first of `main`, `master`, `develop` that exists locally.
pub fn detect_main_branch(path: &Path) -> Option<String> {
    if let Ok(r) = git(
        path,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) {
        if let Some(b) = r.strip_prefix("origin/") {
            return Some(b.to_owned());
        }
    }
    ["main", "master", "develop"]
        .into_iter()
        .find(|b| branch_exists(path, b))
        .map(str::to_owned)
}

/// A commit on the task branch carrying a Gerrit `Change-Id` trailer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GerritCommit {
    /// Commit sha.
    pub sha: String,
    /// Subject line.
    pub subject: String,
    /// `I` + 40 hex digits.
    pub change_id: String,
}

/// The `Change-Id` trailer of a commit message body (last one wins, like Gerrit).
pub fn change_id(body: &str) -> Option<String> {
    body.lines().rev().find_map(|l| {
        let v = l.trim().strip_prefix("Change-Id:")?.trim();
        (v.len() == 41 && v.starts_with('I') && v[1..].bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| v.to_owned())
    })
}

/// Commits reachable from `task_branch` but not from the main branch (preferring
/// `origin/<main>` when it exists), that carry a `Change-Id`. Oldest first.
pub fn gerrit_commits(
    path: &Path,
    main_branch: &str,
    task_branch: &str,
) -> Result<Vec<GerritCommit>, GitError> {
    let remote_main = format!("origin/{main_branch}");
    let base = if git(path, &["rev-parse", "--verify", "--quiet", &remote_main]).is_ok() {
        remote_main
    } else {
        main_branch.to_owned()
    };
    let range = format!("{base}..{task_branch}");
    let out = git(
        path,
        &["log", "--reverse", "--format=%H%x1f%s%x1f%B%x1e", &range],
    )?;
    Ok(out
        .split('\u{1e}')
        .filter_map(|rec| {
            let mut f = rec.trim_start_matches('\n').splitn(3, '\u{1f}');
            let sha = f.next()?.trim().to_owned();
            let subject = f.next()?.to_owned();
            let body = f.next().unwrap_or("");
            let change_id = change_id(body)?;
            Some(GerritCommit {
                sha,
                subject,
                change_id,
            })
        })
        .collect())
}

/// Web base URL of the Gerrit server behind the `origin` remote:
/// `ssh://me@review.example.com:29418/proj` → `https://review.example.com`.
pub fn gerrit_base_url(path: &Path) -> Option<String> {
    let url = git(path, &["config", "--get", "remote.origin.url"]).ok()?;
    gerrit_base_from_remote(&url)
}

/// See [`gerrit_base_url`].
pub fn gerrit_base_from_remote(url: &str) -> Option<String> {
    let url = url.trim();
    let host = if let Some(rest) = url
        .strip_prefix("ssh://")
        .or_else(|| url.strip_prefix("https://"))
        .or_else(|| url.strip_prefix("http://"))
    {
        let authority = rest.split('/').next()?;
        let host = authority.rsplit('@').next()?;
        host.split(':').next()?.to_owned()
    } else if let Some((user_host, _)) = url.split_once(':') {
        // scp-like: me@host:project
        user_host.rsplit('@').next()?.to_owned()
    } else {
        return None;
    };
    (!host.is_empty() && !url.starts_with('/')).then(|| format!("https://{host}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn gerrit_detection() {
        let dir = repo("main");
        let p = dir.path();
        git(p, &["switch", "-q", "-c", "T-1"]).unwrap();
        fs::write(p.join("x"), "1").unwrap();
        git(p, &["add", "-A"]).unwrap();
        let id = format!("I{}", "a".repeat(40));
        git(
            p,
            &[
                "commit",
                "-q",
                "-m",
                &format!("Fix it\n\nBody.\n\nChange-Id: {id}"),
            ],
        )
        .unwrap();
        fs::write(p.join("y"), "1").unwrap();
        git(p, &["add", "-A"]).unwrap();
        git(p, &["commit", "-q", "-m", "wip without id"]).unwrap();
        let found = gerrit_commits(p, "main", "T-1").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].subject, "Fix it");
        assert_eq!(found[0].change_id, id);
        assert!(gerrit_commits(p, "main", "nope").is_err());
        assert_eq!(detect_main_branch(p).as_deref(), Some("main"));
        assert!(gerrit_base_url(p).is_none());
        git(
            p,
            &[
                "remote",
                "add",
                "origin",
                "ssh://me@review.example.com:29418/fw",
            ],
        )
        .unwrap();
        assert_eq!(
            gerrit_base_url(p).as_deref(),
            Some("https://review.example.com")
        );
    }

    #[test]
    fn rebase_hook_check_and_push_errors() {
        let dir = repo("main");
        let p = dir.path();
        assert!(!has_change_id_hook(p));
        fs::write(
            p.join(".git/hooks/commit-msg"),
            "#!/bin/sh\n# adds Change-Id\n",
        )
        .unwrap();
        assert!(has_change_id_hook(p));
        git(p, &["switch", "-q", "-c", "T"]).unwrap();
        fs::write(p.join("t.txt"), "t").unwrap();
        git(p, &["add", "-A"]).unwrap();
        git(p, &["commit", "-q", "-m", "task"]).unwrap();
        git(p, &["switch", "-q", "main"]).unwrap();
        fs::write(p.join("m.txt"), "m").unwrap();
        git(p, &["add", "-A"]).unwrap();
        git(p, &["commit", "-q", "-m", "main moved"]).unwrap();
        git(p, &["switch", "-q", "T"]).unwrap();
        assert!(rebase_on_main(p, "main").unwrap().contains("onto main"));
        assert!(p.join("m.txt").exists());
        // A conflict is aborted and reported.
        fs::write(p.join("m.txt"), "task version").unwrap();
        git(p, &["commit", "-qam", "conflict"]).unwrap();
        git(p, &["switch", "-q", "main"]).unwrap();
        fs::write(p.join("m.txt"), "main version").unwrap();
        git(p, &["commit", "-qam", "main conflict"]).unwrap();
        git(p, &["switch", "-q", "T"]).unwrap();
        assert!(rebase_on_main(p, "main").is_err());
        assert_eq!(inspect(p).unwrap().branch, "T");
        fs::write(p.join("dirty"), "x").unwrap();
        assert!(rebase_on_main(p, "main")
            .unwrap_err()
            .to_string()
            .contains("uncommitted"));
        assert!(push_for_review(p, "main").is_err(), "no origin");
        assert!(last_fetch_age(p).is_none());
    }

    #[test]
    fn remote_url_shapes() {
        for (url, want) in [
            (
                "ssh://me@g.example.com:29418/a/b",
                Some("https://g.example.com"),
            ),
            (
                "https://g.example.com/a/proj",
                Some("https://g.example.com"),
            ),
            ("me@g.example.com:proj.git", Some("https://g.example.com")),
            ("/local/path", None),
        ] {
            assert_eq!(gerrit_base_from_remote(url).as_deref(), want, "{url}");
        }
        assert_eq!(change_id("x\nChange-Id: Ibad"), None);
    }

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
