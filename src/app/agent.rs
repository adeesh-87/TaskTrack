//! Agent-backed actions (context, checkpoints, coding agent) and Gerrit detection.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use crate::ai::{self, AgentCall, PromptKind, PROMPT_ARG};
use crate::config::Config;
use crate::tasks::checkpoints::{self, fmt_minutes};
use crate::tasks::context::{self as ctxfile};
use crate::time::now_rfc3339;

use super::{App, AppEvent, Choice, JobEvent, Pending, Popup, TaskContext};

impl App {
    pub(super) fn log_line(&mut self, line: impl Into<String>) {
        if let Some(Popup::Log { lines, .. }) = &mut self.popup {
            lines.push(line.into());
        }
    }

    pub(super) fn finish_log(&mut self) {
        if let Some(Popup::Log { done, .. }) = &mut self.popup {
            *done = true;
        }
    }

    /// Placeholder values shared by all prompt templates.
    fn prompt_vars(&self, ctx: &TaskContext) -> ai::Vars {
        let context = fs::read_to_string(&ctx.context_path).unwrap_or_default();
        let list = |items: Vec<String>| {
            if items.is_empty() {
                "none attached".to_owned()
            } else {
                items.join(", ")
            }
        };
        let workspaces = list(
            ctx.meta
                .workspaces
                .iter()
                .filter_map(|n| self.config.workspace(n))
                .map(|w| format!("{} ({})", w.name, w.path.display()))
                .collect(),
        );
        let builds = list(
            ctx.meta
                .builds
                .iter()
                .filter_map(|n| self.config.build(n))
                .map(|b| format!("{} ({})", b.name, b.path.display()))
                .collect(),
        );
        let next = checkpoints::next_open(&ctx.checkpoints).map_or_else(
            || "none yet".to_owned(),
            |i| {
                let c = &ctx.checkpoints[i];
                format!("{} ({})", c.title, fmt_minutes(c.estimate_min))
            },
        );
        vec![
            ("task", ctx.id.clone()),
            ("task_dir", ctx.dir.display().to_string()),
            ("context_file", ctx.context_path.display().to_string()),
            ("context", context),
            ("workspaces", workspaces),
            ("builds", builds),
            (
                "branch",
                ctx.meta.branch.clone().unwrap_or_else(|| ctx.id.clone()),
            ),
            ("next_checkpoint", next),
            ("max_words", self.max_words().to_string()),
            (
                "estimate_factor",
                self.today.estimate_factor.map_or_else(
                    || "1.0 (not enough history yet)".to_owned(),
                    |(f, _)| format!("{f:.2}"),
                ),
            ),
        ]
    }

    fn max_words(&self) -> usize {
        self.config.agent.context_max_words.clamp(1, 1000)
    }

    /// First attached code workspace, else the task folder.
    fn work_dir(&self, ctx: &TaskContext, prefer_code: bool) -> PathBuf {
        prefer_code
            .then(|| {
                ctx.meta
                    .workspaces
                    .iter()
                    .find_map(|n| self.config.workspace(n))
                    .map(|w| w.path.clone())
                    .filter(|p| p.is_dir())
            })
            .flatten()
            .unwrap_or_else(|| ctx.dir.clone())
    }

    // ----- one-shot agent ----------------------------------------------------------------

    /// `i` / `b`: run the one-shot agent with the context or checkpoint prompt.
    pub(super) fn run_agent(&mut self, kind: PromptKind) {
        let Some(ctx) = self.active_context() else {
            self.set_status("open a task first");
            return;
        };
        if self.config.agent.command.trim().is_empty() {
            self.error(
                "No AI agent configured: set \"Agent command\" in the settings (Esc c).".into(),
            );
            return;
        }
        if kind == PromptKind::Checkpoints && !ctx.meta.context_ready {
            self.popup = Some(Popup::message(
                "Context not ready yet",
                "Checkpoints need a task definition that is good enough to plan from.\n\n\
                 Esc i   let the agent write the context (it also decides whether it is ready)\n\
                 Esc r   mark it ready yourself\n\
                 or run  pahiri task ready   from a shell or an agent.",
            ));
            return;
        }
        let template_path = self.config.prompt_path(kind, &self.config_path);
        let template = match ai::load_template(&template_path, kind) {
            Ok(t) => t,
            Err(e) => {
                self.error(format!("cannot read {}: {e}", template_path.display()));
                return;
            }
        };
        let vars = self.prompt_vars(ctx);
        let prompt = ai::compose(kind, &template, &vars, self.max_words());
        let mut env = ctx.task_env(&self.config).exports();
        env.push((
            "PAHIRI_CONTEXT_FILE".into(),
            ctx.context_path.display().to_string(),
        ));
        let call = AgentCall {
            program: self.config.agent.command.clone(),
            args: self.config.agent.args.clone(),
            cwd: self.work_dir(ctx, true),
            env,
            prompt,
            timeout: Duration::from_secs(self.config.agent.timeout_secs.max(1)),
        };
        let task_id = ctx.id.clone();
        let what = match kind {
            PromptKind::Context => "writing the context",
            _ => "breaking the task into checkpoints",
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.job_cancel = Some(Arc::clone(&cancel));
        self.popup = Some(Popup::log(format!("AI · {task_id} · {what}")));
        self.log_line(format!(
            "$ {} {}   (in {})",
            call.program,
            shell_words::join(&call.args),
            call.cwd.display()
        ));
        self.log_line(format!(
            "prompt: {} + fixed output format · {} words · Esc cancels",
            template_path.display(),
            ai::word_count(&call.prompt)
        ));
        self.log_line("");
        let events = self.events.clone();
        let spawned = std::thread::Builder::new()
            .name("agent".into())
            .spawn(move || {
                let ev = events.clone();
                let result = ai::run(&call, &cancel, &mut |l| {
                    ev.send(AppEvent::Job(JobEvent::Log(l)));
                });
                events.send(AppEvent::Job(JobEvent::Agent {
                    task_id,
                    kind,
                    result,
                }));
            });
        if let Err(e) = spawned {
            self.error(format!("could not start the agent: {e}"));
        }
    }

    pub(super) fn handle_agent_result(
        &mut self,
        task_id: &str,
        kind: PromptKind,
        result: Result<String, String>,
    ) {
        self.job_cancel = None;
        let Some(path) = self.context_path(task_id) else {
            return;
        };
        let output = match result {
            Ok(o) => o,
            Err(e) => {
                self.log_line("");
                for l in format!("FAILED: {e}").lines() {
                    self.log_line(l.to_owned());
                }
                self.finish_log();
                return;
            }
        };
        match kind {
            PromptKind::Context => self.apply_context_answer(task_id, &path, &output),
            PromptKind::Checkpoints => self.apply_checkpoint_answer(task_id, &path, &output),
            PromptKind::Coding | PromptKind::Audit => {}
        }
    }

    fn apply_context_answer(&mut self, id: &str, path: &Path, output: &str) {
        let answer = ai::parse_context_answer(output);
        self.log_line("");
        if answer.body.is_empty() {
            self.log_line("FAILED: the agent returned an empty answer");
            self.finish_log();
            return;
        }
        let max = self.max_words();
        let (mut body, cut) = ai::limit_words(&answer.body, max);
        if cut {
            let _ = write!(body, "\n\n_(cut at {max} words)_");
        }
        let words = ai::word_count(&body);
        let result = ctxfile::write_generated_context(path, id, &body).and_then(|()| {
            if let Some(r) = answer.ready {
                ctxfile::set_context_ready(path, id, r)?;
            }
            ctxfile::append_log(
                path,
                id,
                &now_rfc3339(),
                &format!(
                    "AI context: {words} words, ready: {}",
                    match answer.ready {
                        Some(true) => "yes".to_owned(),
                        Some(false) => format!("no ({})", answer.missing),
                        None => "not stated".to_owned(),
                    }
                ),
            )
        });
        match result {
            Ok(()) => {
                self.log_line(format!(
                    "wrote {words} words to ## Context in CONTEXT.md{}",
                    if cut { " (cut to the limit)" } else { "" }
                ));
                self.log_line(match answer.ready {
                    Some(true) => {
                        "context ready: yes · Esc b breaks the task into checkpoints".to_owned()
                    }
                    Some(false) => format!("context ready: no · missing: {}", answer.missing),
                    None => "the agent gave no CONTEXT_READY verdict; Esc r sets it".to_owned(),
                });
            }
            Err(e) => self.log_line(format!("FAILED to write CONTEXT.md: {e}")),
        }
        self.finish_log();
        self.after_task_file_change(id);
    }

    fn apply_checkpoint_answer(&mut self, id: &str, path: &Path, output: &str) {
        let items = checkpoints::parse_list(output);
        self.log_line("");
        if items.is_empty() {
            self.log_line("FAILED: no lines like  - [ ] step (30m)  in the answer");
            self.finish_log();
            return;
        }
        let total: u64 = items.iter().map(|c| c.estimate_min).sum();
        self.log_line(format!(
            "{} checkpoints · {} in total:",
            items.len(),
            fmt_minutes(total)
        ));
        for c in &items {
            self.log_line(format!("  {}", c.render()));
        }
        let existing = checkpoints::read(path).unwrap_or_default();
        if existing.iter().any(|c| c.done || c.spent_min > 0) {
            self.popup = Some(Popup::confirm(
                "Replace checkpoints?",
                format!(
                    "{} existing checkpoints have progress recorded ({}).\nReplace them with the {} new ones?",
                    existing.len(),
                    checkpoints::summary(&existing),
                    items.len()
                ),
                Pending::ReplaceCheckpoints(id.to_owned(), items),
            ));
            return;
        }
        self.replace_checkpoints(id, &items);
        self.log_line("written to ## Checkpoints · m starts the timer on the first one");
        self.finish_log();
    }

    /// `r`: flip the context-ready switch (same function as `pahiri task ready`).
    pub(super) fn toggle_context_ready(&mut self) {
        let Some(ctx) = self.active_context() else {
            self.set_status("open a task first");
            return;
        };
        let (id, path, ready) = (
            ctx.id.clone(),
            ctx.context_path.clone(),
            !ctx.meta.context_ready,
        );
        match ctxfile::set_context_ready(&path, &id, ready) {
            Ok(_) => {
                self.after_task_file_change(&id);
                self.set_status(if ready {
                    "context marked ready · Esc b breaks the task into checkpoints"
                } else {
                    "context marked not ready"
                });
            }
            Err(e) => self.error(format!("could not update CONTEXT.md: {e}")),
        }
    }

    // ----- prompts -----------------------------------------------------------------------

    /// `E`: choose a prompt template to open in the editor.
    pub(super) fn choose_prompt(&mut self) {
        if self.active_context().is_none() {
            self.set_status("open a task first (the editor lives in the task view)");
            return;
        }
        let choices = PromptKind::ALL
            .iter()
            .map(|k| Choice {
                label: format!(
                    "{}  {}",
                    k.label(),
                    self.config.prompt_path(*k, &self.config_path).display()
                ),
                pending: Pending::EditPrompt(*k),
            })
            .collect();
        self.popup = Some(Popup::choose("Edit prompt template", choices));
    }

    pub(super) fn edit_prompt(&mut self, kind: PromptKind) {
        let path = self.config.prompt_path(kind, &self.config_path);
        if let Err(e) = ai::ensure_template(&path, kind) {
            self.error(format!("cannot create {}: {e}", path.display()));
            return;
        }
        self.request_open_file(&path);
        self.set_status(
            "placeholders and the fixed output format: Esc c, then ? on an agent setting",
        );
    }

    // ----- coding agent ---------------------------------------------------------------------

    /// `l` / leader `a`: open a shell and start the interactive coding agent in it.
    pub(super) fn launch_coding_agent(&mut self) {
        let Some(ctx) = self.active_context() else {
            self.set_status("open a task first");
            return;
        };
        let cfg = self.config.coding_agent.clone();
        if cfg.command.trim().is_empty() {
            self.error(
                "No coding agent configured: set \"Coding agent command\" in the settings (Esc c)."
                    .into(),
            );
            return;
        }
        let template_path = self
            .config
            .prompt_path(PromptKind::Coding, &self.config_path);
        let template = match ai::load_template(&template_path, PromptKind::Coding) {
            Ok(t) => t,
            Err(e) => {
                self.error(format!("cannot read {}: {e}", template_path.display()));
                return;
            }
        };
        let prompt = ai::render(&template, &self.prompt_vars(ctx));
        let cwd = self.work_dir(ctx, cfg.start_in_code);
        let prompt_file = self
            .state_dir
            .join("prompts")
            .join(format!("{}.coding-agent.md", ctx.id));
        let has_prompt = !prompt.trim().is_empty();
        if has_prompt {
            let written = prompt_file
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|()| fs::write(&prompt_file, &prompt));
            if let Err(e) = written {
                self.error(format!("cannot write {}: {e}", prompt_file.display()));
                return;
            }
        }
        let line = coding_command_line(
            &cfg.command,
            &cfg.args,
            &cwd,
            has_prompt.then_some(prompt_file.as_path()),
        );
        self.new_shell();
        let Some(shell) = self
            .active_context_mut()
            .and_then(TaskContext::active_shell_mut)
        else {
            return;
        };
        let number = shell.number;
        if let Err(e) = shell.session.write(line.as_bytes()) {
            self.error(format!("could not start the coding agent: {e}"));
            return;
        }
        self.set_status(format!(
            "coding agent starting in shell #{number} · leader q leaves · leader z zooms"
        ));
    }
}

/// The line typed into a fresh shell to start the coding agent:
/// `cd <dir> && <program> <args…> "$(cat <prompt file>)"`.
pub fn coding_command_line(
    program: &str,
    args: &[String],
    cwd: &Path,
    prompt_file: Option<&Path>,
) -> String {
    let q = |s: &str| shell_words::quote(s).into_owned();
    let sub = prompt_file.map(|p| format!("\"$(cat {})\"", q(&p.display().to_string())));
    let program = Config::expand_tilde(program).display().to_string();
    let mut parts = vec![q(&program)];
    let mut used = false;
    for a in args {
        if a.contains(PROMPT_ARG) {
            used = true;
            if let Some(s) = &sub {
                let pieces: Vec<String> = a
                    .split(PROMPT_ARG)
                    .map(|p| if p.is_empty() { String::new() } else { q(p) })
                    .collect();
                parts.push(pieces.join(s));
            }
        } else {
            parts.push(q(a));
        }
    }
    if !used {
        if let Some(s) = sub {
            parts.push(s);
        }
    }
    format!(
        "cd {} && {}\r",
        q(&cwd.display().to_string()),
        parts.join(" ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_lines() {
        let p = Path::new("/state/p.md");
        assert_eq!(
            coding_command_line("claude", &[], Path::new("/src/fw"), Some(p)),
            "cd /src/fw && claude \"$(cat /state/p.md)\"\r"
        );
        assert_eq!(
            coding_command_line(
                "aider",
                &["--model".into(), "x y".into(), "--message={prompt}".into()],
                Path::new("/a b"),
                Some(p)
            ),
            "cd '/a b' && aider --model 'x y' '--message='\"$(cat /state/p.md)\"\r"
        );
        assert_eq!(
            coding_command_line("codex", &["{prompt}".into()], Path::new("/x"), None),
            "cd /x && codex\r"
        );
    }
}
