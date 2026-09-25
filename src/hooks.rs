//! User hooks: shell commands pahiri runs when something happens.
//!
//! Configured as `event = command` under `[hooks]`. Commands run through
//! `sh -c` in the task folder (the tasks folder for events without a task)
//! with the `PAHIRI_*` variables below. Hooks talk back to pahiri by editing
//! files or calling `$PAHIRI_BIN task …`; pahiri reloads what changed.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::ai::{self, AgentCall};

/// Things that can trigger a hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookEvent {
    /// pahiri started.
    Startup,
    /// A task folder was created (custom or from a ticket).
    TaskCreate,
    /// A task is being opened; the task view is shown after the hook finishes.
    TaskEnter,
    /// The task view was left (to the list or another task).
    TaskLeave,
    /// A task moved to another board column.
    TaskMove,
    /// A task was moved to the trash.
    TaskDelete,
    /// Attachments changed.
    Attach,
    /// `prepare` finished.
    PrepareDone,
    /// The timer started.
    TimerStart,
    /// The timer was paused (by you or by the idle check).
    TimerPause,
    /// The timer was resumed.
    TimerResume,
    /// The timer was stopped and its time booked.
    TimerStop,
    /// The timer ran out.
    TimerExpire,
    /// A checkpoint was ticked.
    CheckpointDone,
    /// The AI wrote the task context.
    ContextGenerated,
    /// The AI wrote checkpoints.
    CheckpointsGenerated,
    /// Gerrit changes were scanned.
    Gerrit,
    /// pahiri ran for the first time on a new day.
    DayStart,
    /// The day plan was saved from the Plan view.
    PlanSave,
}

/// Variables every hook gets.
pub const COMMON_ENV: &[(&str, &str)] = &[
    ("PAHIRI_HOOK", "event name, e.g. task_enter"),
    (
        "PAHIRI_BIN",
        "path of the pahiri binary (call `$PAHIRI_BIN task …`)",
    ),
    ("PAHIRI_CONFIG", "config file"),
    ("PAHIRI_TASKS_DIR", "tasks folder"),
    ("PAHIRI_TASK", "task id (empty for startup)"),
    ("PAHIRI_TASK_DIR", "task folder"),
    ("PAHIRI_CONTEXT_FILE", "the task's CONTEXT.md"),
    ("PAHIRI_TASK_TITLE", "title from CONTEXT.md"),
    ("PAHIRI_COLUMN", "board column name"),
    ("PAHIRI_COLUMN_INDEX", "board column, 0-based"),
    ("PAHIRI_LINK", "ticket link, if any"),
    ("PAHIRI_BRANCH", "task branch"),
    ("PAHIRI_CODE_DIR", "first attached code workspace"),
    ("PAHIRI_CODE_DIRS", "all of them as name=path;name=path"),
    ("PAHIRI_BUILD_DIR", "first attached vendor build"),
    ("PAHIRI_BUILD_DIRS", "all of them as name=path;…"),
    ("PAHIRI_NEXT_CHECKPOINT", "next open checkpoint title"),
    ("PAHIRI_CONTEXT_READY", "yes / no"),
];

impl HookEvent {
    /// All events, in help order.
    pub const ALL: [HookEvent; 19] = [
        Self::Startup,
        Self::TaskCreate,
        Self::TaskEnter,
        Self::TaskLeave,
        Self::TaskMove,
        Self::TaskDelete,
        Self::Attach,
        Self::PrepareDone,
        Self::TimerStart,
        Self::TimerPause,
        Self::TimerResume,
        Self::TimerStop,
        Self::TimerExpire,
        Self::CheckpointDone,
        Self::ContextGenerated,
        Self::CheckpointsGenerated,
        Self::Gerrit,
        Self::DayStart,
        Self::PlanSave,
    ];

    /// Name used in the config.
    pub fn name(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::TaskCreate => "task_create",
            Self::TaskEnter => "task_enter",
            Self::TaskLeave => "task_leave",
            Self::TaskMove => "task_move",
            Self::TaskDelete => "task_delete",
            Self::Attach => "attach",
            Self::PrepareDone => "prepare_done",
            Self::TimerStart => "timer_start",
            Self::TimerPause => "timer_pause",
            Self::TimerResume => "timer_resume",
            Self::TimerStop => "timer_stop",
            Self::TimerExpire => "timer_expire",
            Self::CheckpointDone => "checkpoint_done",
            Self::ContextGenerated => "context_generated",
            Self::CheckpointsGenerated => "checkpoints_generated",
            Self::Gerrit => "gerrit",
            Self::DayStart => "day_start",
            Self::PlanSave => "plan_save",
        }
    }

    /// Look up an event by config name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.name() == name.trim())
    }

    /// When it fires.
    pub fn description(self) -> &'static str {
        match self {
            Self::Startup => "pahiri started (no task)",
            Self::TaskCreate => "a task was created; pahiri waits for the hook, then shows it",
            Self::TaskEnter => {
                "a task is opened; pahiri waits for the hook, then renders the task view"
            }
            Self::TaskLeave => "you left a task's view",
            Self::TaskMove => "a task moved to another column",
            Self::TaskDelete => "a task was moved to the trash",
            Self::Attach => "workspaces / builds were attached or detached",
            Self::PrepareDone => "prepare (commit, pull, switch branch) finished",
            Self::TimerStart => "the timer started",
            Self::TimerPause => "the timer paused (you, or the idle check)",
            Self::TimerResume => "the timer resumed",
            Self::TimerStop => "the timer stopped and its time was booked",
            Self::TimerExpire => "the timer ran out (use it for desktop notifications)",
            Self::CheckpointDone => "a checkpoint was ticked",
            Self::ContextGenerated => "the AI wrote the ## Context section",
            Self::CheckpointsGenerated => "the AI wrote checkpoints",
            Self::Gerrit => "Gerrit changes were scanned (Esc g)",
            Self::DayStart => "pahiri ran for the first time today (at start or past midnight)",
            Self::PlanSave => "the day plan was saved in the Plan view (p)",
        }
    }

    /// Whether pahiri waits for the hook before continuing.
    pub fn blocks(self) -> bool {
        matches!(self, Self::TaskEnter | Self::TaskCreate)
    }

    /// Variables specific to this event.
    pub fn extra_env(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::TaskEnter => &[(
                "PAHIRI_PREV_TASK",
                "task that was open before (may be empty)",
            )],
            Self::TaskLeave => &[(
                "PAHIRI_NEXT_TASK",
                "task being opened next (empty: back to the list)",
            )],
            Self::TaskCreate => &[
                (
                    "PAHIRI_SOURCE",
                    "ticket source name (empty for custom tasks)",
                ),
                ("PAHIRI_TICKET_URL", "ticket link"),
            ],
            Self::TaskMove => &[
                ("PAHIRI_FROM_COLUMN", "previous column name"),
                ("PAHIRI_TO_COLUMN", "new column name (= PAHIRI_COLUMN)"),
                ("PAHIRI_FINISHED", "1 when moved into the last column"),
                ("PAHIRI_REOPENED", "1 when moved out of the last column"),
            ],
            Self::TaskDelete => &[("PAHIRI_TRASH_PATH", "where the folder went")],
            Self::PrepareDone => &[
                ("PAHIRI_PREPARED", "workspaces switched, space separated"),
                ("PAHIRI_FAILED", "workspaces that failed"),
            ],
            Self::TimerStart
            | Self::TimerPause
            | Self::TimerResume
            | Self::TimerStop
            | Self::TimerExpire => &[
                (
                    "PAHIRI_CHECKPOINT",
                    "what is timed (empty for a focus block)",
                ),
                ("PAHIRI_BUDGET_MIN", "minutes budgeted"),
                ("PAHIRI_ELAPSED_MIN", "minutes counted so far"),
                ("PAHIRI_REMAINING_MIN", "minutes left (negative when over)"),
                ("PAHIRI_IDLE", "1 when paused by the idle check"),
            ],
            Self::CheckpointDone => &[
                ("PAHIRI_CHECKPOINT", "the ticked checkpoint"),
                ("PAHIRI_ESTIMATE_MIN", "its estimate"),
                ("PAHIRI_SPENT_MIN", "time booked on it"),
            ],
            Self::ContextGenerated => &[("PAHIRI_WORDS", "words written")],
            Self::CheckpointsGenerated => &[("PAHIRI_CHECKPOINT_COUNT", "how many")],
            Self::Gerrit => &[
                ("PAHIRI_GERRIT_COUNT", "changes found"),
                ("PAHIRI_GERRIT_CHANGES", "their Change-Ids, space separated"),
            ],
            Self::DayStart => &[
                ("PAHIRI_DATE", "today, YYYY-MM-DD"),
                ("PAHIRI_PLAN_FILE", "today's plan file (may not exist yet)"),
                (
                    "PAHIRI_CARRIED",
                    "unfinished items in the last earlier plan",
                ),
            ],
            Self::PlanSave => &[
                ("PAHIRI_DATE", "the plan's day, YYYY-MM-DD"),
                ("PAHIRI_PLAN_FILE", "the plan file"),
                ("PAHIRI_PLAN_ITEMS", "number of items"),
                ("PAHIRI_PLAN_MINUTES", "minutes planned (open items)"),
            ],
            Self::Startup | Self::Attach => &[],
        }
    }
}

/// Path of the running binary (for `$PAHIRI_BIN`).
pub fn current_bin() -> String {
    std::env::current_exe().map_or_else(|_| "pahiri".into(), |p| p.display().to_string())
}

/// Run a hook command through `sh -c`. Returns stdout, or stderr's tail on failure.
pub fn run(
    command: &str,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    timeout: Duration,
    cancel: &AtomicBool,
    on_line: &mut dyn FnMut(String),
) -> Result<String, String> {
    let call = AgentCall {
        program: "sh".into(),
        args: vec!["-c".into(), command.into()],
        cwd,
        env,
        prompt: String::new(),
        timeout,
    };
    ai::run(&call, cancel, on_line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_roundtrip_and_env_docs() {
        for e in HookEvent::ALL {
            assert_eq!(HookEvent::from_name(e.name()), Some(e));
            assert!(!e.description().is_empty());
        }
        assert_eq!(HookEvent::from_name("nope"), None);
        assert!(HookEvent::TaskEnter.blocks());
        assert!(!HookEvent::TimerExpire.blocks());
    }

    #[test]
    fn runs_with_env() {
        let cancel = AtomicBool::new(false);
        let out = run(
            "echo \"$PAHIRI_HOOK:$PAHIRI_TASK\"",
            std::env::temp_dir(),
            vec![
                ("PAHIRI_HOOK".into(), "task_enter".into()),
                ("PAHIRI_TASK".into(), "T-1".into()),
            ],
            Duration::from_secs(5),
            &cancel,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(out, "task_enter:T-1\n");
        let err = run(
            "echo oops >&2; exit 4",
            std::env::temp_dir(),
            vec![],
            Duration::from_secs(5),
            &cancel,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(err.contains("oops"));
    }
}
