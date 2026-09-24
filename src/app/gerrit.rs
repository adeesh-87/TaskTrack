//! Gerrit: find changes on task branches, fetch their status, push, rebase.

use std::path::PathBuf;

use crate::git;
use crate::hooks::HookEvent;
use crate::tasks::context as ctxfile;
use crate::tasks::sources::parse_gerrit_status;
use crate::tasks::GerritRef;

use super::hooks::AfterHook;
use super::{App, AppEvent, JobEvent, Pending, Popup};

/// A workspace to act on: name, path, main branch.
type Job = (String, PathBuf, String);

/// Which git job ran (for its completion message).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitJob {
    /// `push HEAD:refs/for/<main>`.
    Push,
    /// Rebase onto main.
    Rebase,
}

fn age(secs: u64) -> String {
    match secs {
        s if s < 3600 => format!("{} min", s / 60),
        s if s < 86_400 => format!("{} h", s / 3600),
        s => format!("{} days", s / 86_400),
    }
}

impl App {
    fn task_jobs(&mut self) -> Option<(String, String, Vec<Job>)> {
        let Some(ctx) = self.active_context() else {
            self.set_status("open a task first");
            return None;
        };
        if ctx.meta.workspaces.is_empty() {
            self.error("no code workspace attached to this task yet (Esc a to attach)".into());
            return None;
        }
        let branch = ctx.meta.branch.clone().unwrap_or_else(|| ctx.id.clone());
        let jobs = ctx
            .meta
            .workspaces
            .iter()
            .filter_map(|n| self.config.workspace(n))
            .map(|w| {
                (
                    w.name.clone(),
                    w.path.clone(),
                    self.config.main_branch_for(w),
                )
            })
            .collect();
        Some((ctx.id.clone(), branch, jobs))
    }

    /// `g`: fetch, then find commits with a `Change-Id` on the task branch of
    /// every attached workspace, then ask the status command about them.
    pub(super) fn find_gerrit(&mut self) {
        let Some((task_id, branch, jobs)) = self.task_jobs() else {
            return;
        };
        let override_url = self
            .config
            .gerrit_url
            .trim()
            .trim_end_matches('/')
            .to_owned();
        let status_cmd = self.config.gerrit_status_command.trim().to_owned();
        let env = self.hook_env(HookEvent::Gerrit, Some(&task_id), Vec::new());
        let cwd = self.config.tasks_dir.join(&task_id);
        self.popup = Some(Popup::log(format!("Gerrit changes · {task_id}")));
        let events = self.events.clone();
        let spawned = std::thread::Builder::new()
            .name("gerrit".into())
            .spawn(move || {
                let log = |l: String| events.send(AppEvent::Job(JobEvent::Log(l)));
                let mut found = Vec::new();
                let mut scanned = false;
                for (name, path, main) in &jobs {
                    if !git::branch_exists(path, &branch) {
                        log(format!("[{name}] no branch '{branch}' yet (Esc p prepares it)"));
                        continue;
                    }
                    if git::has_origin(path) {
                        match git::fetch_branch(path, main) {
                            Ok(()) => log(format!("[{name}] fetched origin/{main}")),
                            Err(e) => log(format!(
                                "[{name}] fetch failed ({}), using origin/{main} from {} ago",
                                e.to_string().lines().last().unwrap_or("error"),
                                git::last_fetch_age(path).map_or_else(|| "?".into(), age)
                            )),
                        }
                    }
                    if !git::has_change_id_hook(path) {
                        log(format!(
                            "[{name}] WARNING: Gerrit's commit-msg hook is not installed; new commits get no Change-Id"
                        ));
                    }
                    match git::gerrit_commits(path, main, &branch) {
                        Ok(commits) => {
                            scanned = true;
                            let base = if override_url.is_empty() {
                                git::gerrit_base_url(path)
                            } else {
                                Some(override_url.clone())
                            };
                            log(format!("[{name}] {main}..{branch}: {} change(s)", commits.len()));
                            for c in commits {
                                let url = base.as_ref().map(|b| format!("{b}/q/{}", c.change_id));
                                log(format!(
                                    "  {} {}  {}",
                                    &c.sha[..c.sha.len().min(8)],
                                    c.subject,
                                    url.as_deref().unwrap_or(&c.change_id)
                                ));
                                found.push(GerritRef {
                                    workspace: name.clone(),
                                    change_id: c.change_id,
                                    url,
                                    subject: c.subject,
                                    status: None,
                                });
                            }
                        }
                        Err(e) => log(format!("[{name}] skipped: {e}")),
                    }
                }
                if !status_cmd.is_empty() && !found.is_empty() {
                    let ids: Vec<String> = found.iter().map(|g| g.change_id.clone()).collect();
                    let mut env = env;
                    env.push(("PAHIRI_GERRIT_CHANGES".into(), ids.join(" ")));
                    // Inline code that uses $@ / $1 gets the ids as positional
                    // parameters; a plain command gets them as arguments.
                    let joined = shell_words::join(&ids);
                    let command = if ["$@", "$1", "$*", "${@"].iter().any(|p| status_cmd.contains(p)) {
                        format!("set -- {joined}\n{status_cmd}")
                    } else {
                        format!("{status_cmd} {joined}")
                    };
                    log(format!("$ {command}"));
                    let cancel = std::sync::atomic::AtomicBool::new(false);
                    let result = crate::hooks::run(
                        &command,
                        cwd,
                        env,
                        std::time::Duration::from_secs(60),
                        &cancel,
                        &mut |_| {},
                    )
                    .and_then(|out| parse_gerrit_status(&out));
                    match result {
                        Ok(statuses) => {
                            for st in statuses {
                                if let Some(g) = found.iter_mut().find(|g| g.change_id == st.change_id) {
                                    g.status = Some(st.summary()).filter(|s| !s.is_empty());
                                    if st.url.is_some() {
                                        g.url.clone_from(&st.url);
                                    }
                                    if let Some(sub) = st.subject {
                                        g.subject = sub;
                                    }
                                    log(format!("  {} {}", g.change_id, g.status.as_deref().unwrap_or("")));
                                }
                            }
                        }
                        Err(e) => log(format!("status command failed: {e}")),
                    }
                }
                events.send(AppEvent::Job(JobEvent::Gerrit {
                    task_id,
                    found,
                    scanned,
                }));
            });
        if let Err(e) = spawned {
            self.error(format!("could not start: {e}"));
        }
    }

    pub(super) fn handle_gerrit(
        &mut self,
        task_id: &str,
        mut found: Vec<GerritRef>,
        scanned: bool,
    ) {
        self.log_line("");
        if scanned {
            let n = found.len();
            let path = self.context_path(task_id);
            // Keep an earlier status when the status command did not report one.
            let old = path
                .as_ref()
                .and_then(|p| ctxfile::read_meta(p).ok())
                .map(|m| m.gerrit)
                .unwrap_or_default();
            for g in &mut found {
                if g.status.is_none() {
                    if let Some(o) = old.iter().find(|o| o.change_id == g.change_id) {
                        g.status.clone_from(&o.status);
                    }
                }
            }
            let ids: Vec<String> = found.iter().map(|g| g.change_id.clone()).collect();
            let result = path.map(|p| ctxfile::update_meta(&p, task_id, |m| m.gerrit = found));
            match result {
                Some(Err(e)) => self.log_line(format!("FAILED to update CONTEXT.md: {e}")),
                _ => self.log_line(format!("done: {n} change(s) recorded in CONTEXT.md")),
            }
            self.after_task_file_change(task_id);
            self.fire_hook(
                HookEvent::Gerrit,
                Some(task_id),
                vec![
                    ("PAHIRI_GERRIT_COUNT".into(), n.to_string()),
                    ("PAHIRI_GERRIT_CHANGES".into(), ids.join(" ")),
                ],
                AfterHook::Nothing,
            );
        } else {
            self.log_line("done: nothing scanned, CONTEXT.md unchanged");
        }
        self.finish_log();
    }

    /// `P` / `R`: confirm, then push for review or rebase in every workspace
    /// that is on the task branch.
    pub(super) fn request_git_job(&mut self, job: GitJob) {
        let Some((task_id, branch, jobs)) = self.task_jobs() else {
            return;
        };
        let names: Vec<String> = jobs
            .iter()
            .map(|(n, p, m)| {
                let on = git::inspect(p).map(|s| s.branch).unwrap_or_default();
                if on == branch {
                    format!("  {n}: {branch} → {m}")
                } else {
                    format!("  {n}: skipped (on '{on}', not '{branch}')")
                }
            })
            .collect();
        let (title, what) = match job {
            GitJob::Push => ("Push for review?", "git push origin HEAD:refs/for/<main>"),
            GitJob::Rebase => ("Rebase onto main?", "fetch, then git rebase origin/<main>"),
        };
        self.popup = Some(Popup::confirm(
            title,
            format!("{what} for {task_id}:\n{}", names.join("\n")),
            Pending::GitJob(job),
        ));
    }

    pub(super) fn run_git_job(&mut self, job: GitJob) {
        let Some((task_id, branch, jobs)) = self.task_jobs() else {
            return;
        };
        let title = match job {
            GitJob::Push => format!("Push for review · {task_id}"),
            GitJob::Rebase => format!("Rebase onto main · {task_id}"),
        };
        self.popup = Some(Popup::log(title));
        let events = self.events.clone();
        let spawned = std::thread::Builder::new()
            .name("git-job".into())
            .spawn(move || {
                let log = |l: String| events.send(AppEvent::Job(JobEvent::Log(l)));
                let mut ok = 0;
                for (name, path, main) in &jobs {
                    match git::inspect(path) {
                        Ok(s) if s.branch == branch => {}
                        Ok(s) => {
                            log(format!(
                                "[{name}] skipped: on '{}', not '{branch}'",
                                s.branch
                            ));
                            continue;
                        }
                        Err(e) => {
                            log(format!("[{name}] {e}"));
                            continue;
                        }
                    }
                    let result = match job {
                        GitJob::Push => git::push_for_review(path, main),
                        GitJob::Rebase => git::rebase_on_main(path, main),
                    };
                    match result {
                        Ok(out) => {
                            ok += 1;
                            for l in out.lines().filter(|l| !l.trim().is_empty()) {
                                log(format!("[{name}] {l}"));
                            }
                            log(format!("[{name}] ok"));
                        }
                        Err(e) => {
                            for l in e.to_string().lines() {
                                log(format!("[{name}] {l}"));
                            }
                        }
                    }
                }
                let tail = match job {
                    GitJob::Push if ok > 0 => " · Esc g records the changes",
                    _ => "",
                };
                events.send(AppEvent::Job(JobEvent::Finished(format!(
                    "done: {ok} of {} workspace(s){tail}",
                    jobs.len()
                ))));
            });
        if let Err(e) = spawned {
            self.error(format!("could not start: {e}"));
        }
    }
}
