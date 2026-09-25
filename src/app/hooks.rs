//! Running user hooks from the app.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tracing::{info, warn};

use crate::hooks::{self, HookEvent};
use crate::tasks::checkpoints;
use crate::tasks::record::TaskRecord;

use super::{App, AppEvent, Choice, JobEvent, Pending, Popup};

/// Keep this many runs for the help page.
const KEEP_RUNS: usize = 20;

/// What to do once a waited-for hook finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfterHook {
    /// Nothing.
    Nothing,
    /// Open the task view of this task.
    EnterTask(String),
    /// Select (and in the task view, open) a newly created task.
    ShowCreated(String),
}

/// One finished hook run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookRun {
    /// Event name.
    pub event: &'static str,
    /// Task, if any.
    pub task: Option<String>,
    /// The command.
    pub command: String,
    /// Exit status 0.
    pub ok: bool,
    /// stdout (or the error).
    pub output: String,
    /// How long it took.
    pub millis: u128,
    /// When it finished (RFC 3339).
    pub at: String,
}

impl App {
    /// The `PAHIRI_*` variables for a hook.
    pub(super) fn hook_env(
        &self,
        event: HookEvent,
        task: Option<&str>,
        extra: Vec<(String, String)>,
    ) -> Vec<(String, String)> {
        let mut env: Vec<(String, String)> = vec![
            ("PAHIRI_HOOK".into(), event.name().into()),
            ("PAHIRI_BIN".into(), hooks::current_bin()),
            (
                "PAHIRI_CONFIG".into(),
                self.config_path.display().to_string(),
            ),
            (
                "PAHIRI_TASKS_DIR".into(),
                self.config.tasks_dir.display().to_string(),
            ),
        ];
        if let (Some(id), Some(store)) = (task, &self.store) {
            let record = TaskRecord::load(id, "", &store.context_path(id));
            let (col_name, col_index) = store
                .board()
                .locate(id)
                .map_or((String::new(), String::new()), |(c, _)| {
                    (store.board().columns[c].name.clone(), c.to_string())
                });
            let next = record
                .next_checkpoint()
                .map(|c| c.title.clone())
                .unwrap_or_default();
            let task_env = if let Some(ctx) = self.contexts.get(id) {
                ctx.task_env(&self.config).exports()
            } else {
                let meta =
                    crate::tasks::context::read_meta(&store.context_path(id)).unwrap_or_default();
                let pick = |names: &[String], f: &dyn Fn(&str) -> Option<std::path::PathBuf>| {
                    names
                        .iter()
                        .filter_map(|n| f(n).map(|p| (n.clone(), p)))
                        .collect::<Vec<_>>()
                };
                crate::terminal::shellrc::TaskEnv {
                    task: id.to_owned(),
                    task_dir: store.tasks_dir().join(id),
                    code: pick(&meta.workspaces, &|n| {
                        self.config.workspace(n).map(|w| w.path.clone())
                    }),
                    builds: pick(&meta.builds, &|n| {
                        self.config.build(n).map(|b| b.path.clone())
                    }),
                }
                .exports()
            };
            env.extend(task_env);
            let meta =
                crate::tasks::context::read_meta(&store.context_path(id)).unwrap_or_default();
            env.extend([
                (
                    "PAHIRI_CONTEXT_FILE".into(),
                    store.context_path(id).display().to_string(),
                ),
                ("PAHIRI_TASK_TITLE".into(), record.title),
                ("PAHIRI_COLUMN".into(), col_name),
                ("PAHIRI_COLUMN_INDEX".into(), col_index),
                ("PAHIRI_LINK".into(), record.link.unwrap_or_default()),
                (
                    "PAHIRI_BRANCH".into(),
                    meta.branch.unwrap_or_else(|| id.to_owned()),
                ),
                ("PAHIRI_NEXT_CHECKPOINT".into(), next),
                (
                    "PAHIRI_CONTEXT_READY".into(),
                    if meta.context_ready { "yes" } else { "no" }.into(),
                ),
            ]);
        } else {
            env.push(("PAHIRI_TASK".into(), String::new()));
        }
        env.extend(extra);
        env
    }

    /// Timer variables for the `timer_*` hooks.
    pub(super) fn timer_env(&self) -> Vec<(String, String)> {
        let Some(t) = &self.timer else {
            return Vec::new();
        };
        let now = Instant::now();
        vec![
            (
                "PAHIRI_CHECKPOINT".into(),
                t.checkpoint.clone().unwrap_or_default(),
            ),
            (
                "PAHIRI_BUDGET_MIN".into(),
                (t.budget.as_secs() / 60).to_string(),
            ),
            (
                "PAHIRI_ELAPSED_MIN".into(),
                (t.elapsed(now).as_secs() / 60).to_string(),
            ),
            (
                "PAHIRI_REMAINING_MIN".into(),
                (t.remaining_secs(now) / 60).to_string(),
            ),
            (
                "PAHIRI_IDLE".into(),
                if matches!(t.away, Some((_, super::timer::Away::Idle))) {
                    "1"
                } else {
                    "0"
                }
                .into(),
            ),
        ]
    }

    /// Run the hook for `event` (if configured). Blocking events show a log
    /// while they run and continue with `then` afterwards; without a hook
    /// `then` runs right away.
    /// `Esc !`: pick a configured hook to run now.
    pub(super) fn open_run_hook_menu(&mut self) {
        let choices: Vec<Choice> = self
            .config
            .hooks
            .iter()
            .filter(|(event, command)| {
                HookEvent::from_name(event).is_some() && !command.trim().is_empty()
            })
            .map(|(event, command)| Choice {
                label: format!("{event} · {}", cut(command.trim(), 60)),
                pending: Pending::RunHook(event.clone()),
            })
            .collect();
        if choices.is_empty() {
            self.set_status("no hooks configured · settings (Esc c) → Hooks");
            return;
        }
        let task = self.current_task_id().unwrap_or_default();
        let title = if task.is_empty() {
            "Run a hook now".to_owned()
        } else {
            format!("Run a hook now · {task}")
        };
        self.popup = Some(Popup::choose(title, choices));
    }

    /// Run the hook of `event` for the current task, as if the event happened.
    pub(super) fn run_hook_now(&mut self, event: &str) {
        let Some(event) = HookEvent::from_name(event) else {
            return;
        };
        let task = self.current_task_id();
        self.set_status(format!("running the {} hook …", event.name()));
        self.fire_hook(
            event,
            task.as_deref(),
            vec![("PAHIRI_MANUAL".into(), "1".into())],
            AfterHook::Nothing,
        );
    }

    pub(super) fn fire_hook(
        &mut self,
        event: HookEvent,
        task: Option<&str>,
        extra: Vec<(String, String)>,
        then: AfterHook,
    ) {
        let Some(command) = self
            .config
            .hooks
            .get(event.name())
            .cloned()
            .filter(|c| !c.trim().is_empty())
        else {
            self.after_hook(then);
            return;
        };
        let env = self.hook_env(event, task, extra);
        let cwd = task
            .and_then(|id| self.store.as_ref().map(|s| s.tasks_dir().join(id)))
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| self.config.tasks_dir.clone());
        let timeout = Duration::from_secs(self.config.hook_timeout_secs.max(1));
        let cancel = Arc::new(AtomicBool::new(false));
        let waits = event.blocks() && then != AfterHook::Nothing;
        if waits {
            self.job_cancel = Some(Arc::clone(&cancel));
            self.popup = Some(Popup::log(format!(
                "hook · {} · {}",
                event.name(),
                task.unwrap_or("")
            )));
            if let Some(Popup::Log { lines, .. }) = &mut self.popup {
                lines.push(format!("$ {command}"));
            }
        }
        self.hooks_running += 1;
        let events = self.events.clone();
        let task = task.map(str::to_owned);
        info!(event = event.name(), ?task, "running hook");
        let spawned = std::thread::Builder::new()
            .name(format!("hook-{}", event.name()))
            .spawn(move || {
                let started = Instant::now();
                let ev = events.clone();
                let result = hooks::run(&command, cwd, env, timeout, &cancel, &mut |l| {
                    if waits {
                        ev.send(AppEvent::Job(JobEvent::Log(l)));
                    }
                });
                events.send(AppEvent::Job(JobEvent::Hook {
                    event,
                    task,
                    command,
                    result,
                    millis: started.elapsed().as_millis(),
                    then,
                    waited: waits,
                }));
            });
        if let Err(e) = spawned {
            self.hooks_running = self.hooks_running.saturating_sub(1);
            self.set_status(format!("could not start hook: {e}"));
        }
    }

    #[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
    pub(super) fn handle_hook_done(
        &mut self,
        event: HookEvent,
        task: Option<String>,
        command: String,
        result: Result<String, String>,
        millis: u128,
        then: AfterHook,
        waited: bool,
    ) {
        self.hooks_running = self.hooks_running.saturating_sub(1);
        let ok = result.is_ok();
        let output = match &result {
            Ok(o) | Err(o) => o.trim_end().to_owned(),
        };
        if !ok {
            warn!(event = event.name(), "hook failed: {output}");
            let first = output.lines().last().unwrap_or("failed").to_owned();
            self.set_status(format!(
                "hook {} failed: {first} (F1 → Hooks)",
                event.name()
            ));
        } else if let Some(last) = output.lines().last().filter(|l| !l.trim().is_empty()) {
            self.set_status(format!("{}: {last}", event.name()));
        }
        self.hook_runs.push_back(HookRun {
            event: event.name(),
            task: task.clone(),
            command,
            ok,
            output,
            millis,
            at: crate::time::now_rfc3339(),
        });
        while self.hook_runs.len() > KEEP_RUNS {
            self.hook_runs.pop_front();
        }
        if waited {
            self.job_cancel = None;
            match &mut self.popup {
                Some(Popup::Log { lines, done, .. }) if !ok => {
                    lines.push(String::new());
                    lines.push("hook failed — continuing anyway".into());
                    *done = true;
                }
                Some(Popup::Log { .. }) => self.popup = None,
                _ => {}
            }
        }
        // The hook may have changed the board, the task files or the folders.
        self.reload_outside_changes();
        if let Some(id) = &task {
            self.after_task_file_change(id);
        }
        self.after_hook(then);
    }

    fn after_hook(&mut self, then: AfterHook) {
        match then {
            AfterHook::Nothing => {}
            AfterHook::EnterTask(id) => self.open_task_view(&id),
            AfterHook::ShowCreated(id) => self.show_created(&id),
        }
    }

    /// Variables for `checkpoint_done`.
    pub(super) fn checkpoint_env(c: &checkpoints::Checkpoint) -> Vec<(String, String)> {
        vec![
            ("PAHIRI_CHECKPOINT".into(), c.title.clone()),
            ("PAHIRI_ESTIMATE_MIN".into(), c.estimate_min.to_string()),
            ("PAHIRI_SPENT_MIN".into(), c.spent_min.to_string()),
        ]
    }
}

/// `text` cut to `width` chars with `…`.
fn cut(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}
