//! Doing the work: deleting tasks, checkpoints, the timer (menu, alarm, idle
//! check, bookkeeping), task dates, and the "today / next up" overview.

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use tracing::warn;

use crate::hooks::HookEvent;
use crate::tasks::checkpoints::{self, fmt_minutes};
use crate::tasks::context::{self as ctxfile, OUTCOME_HEADING};
use crate::tasks::ledger;
use crate::tasks::record::TaskRecord;
use crate::time::{local_day_start, local_offset_secs, local_week_start, now_rfc3339, now_secs};

use super::hooks::AfterHook;
use super::timer::{Away, Timer};
use super::{App, Choice, ListRow, Mode, Pending, Popup, TaskContext};

/// Book the running timer to disk this often (crash safety).
const AUTOSAVE_SECS: u64 = 300;
/// Save `session.json` this often.
const SESSION_SECS: u64 = 60;
/// Look for outside changes (board, folders, CONTEXT.md, file tree) this often.
const WATCH: Duration = Duration::from_millis(900);
/// How long the screen flashes when time is up.
const FLASH: Duration = Duration::from_millis(1600);
/// Length of one flash pulse.
const FLASH_PULSE_MS: u128 = 200;

/// Time booked recently, for the task list's right side.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Today {
    /// Minutes booked today (local time).
    pub today_min: u64,
    /// Minutes booked this week (since Monday).
    pub week_min: u64,
    /// Today's minutes per task, largest first.
    pub today_by_task: Vec<(String, u64)>,
    /// Tasks finished this week.
    pub finished_week: usize,
    /// Spent ÷ estimated over finished checkpoints, and how many there were.
    pub estimate_factor: Option<(f64, usize)>,
}

fn mtime(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl App {
    // ----- accessors used by the UI ------------------------------------------------

    /// The timer, if one is set.
    pub fn timer(&self) -> Option<&Timer> {
        self.timer.as_ref()
    }

    /// Whether the screen is in the "on" phase of the time's-up flash.
    pub fn flash_on(&self) -> bool {
        self.flash_start.is_some_and(|s| {
            let e = s.elapsed();
            e < FLASH && (e.as_millis() / FLASH_PULSE_MS) % 2 == 0
        })
    }

    /// Whether the timer chip is in the bright phase of its "time is up" blink.
    pub fn timer_blink_on(&self) -> bool {
        self.timer.as_ref().is_some_and(Timer::alarming)
            && (self.started.elapsed().as_millis() / 500) % 2 == 0
    }

    /// How often the main loop should redraw without input.
    pub fn tick_interval(&self) -> Duration {
        if self.flash_start.is_some_and(|s| s.elapsed() < FLASH) {
            Duration::from_millis(50)
        } else {
            Duration::from_millis(250)
        }
    }

    /// Whether to ring the terminal bell now (resets the request).
    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell)
    }

    /// Tasks that are not finished, most active column first.
    pub fn next_up(&self) -> &[TaskRecord] {
        &self.next_up
    }

    /// Time booked today and this week.
    pub fn today(&self) -> &Today {
        &self.today
    }

    /// Cached record of a task (title, dates, checkpoints).
    pub fn record(&self, id: &str) -> Option<&TaskRecord> {
        self.records.get(id).map(|(_, r)| r)
    }

    /// Number of hooks still running.
    pub fn hooks_running(&self) -> usize {
        self.hooks_running
    }

    // ----- helpers -----------------------------------------------------------------

    pub(super) fn context_path(&self, id: &str) -> Option<PathBuf> {
        self.store.as_ref().map(|s| s.context_path(id))
    }

    /// Re-read what changed on disk for `id` and refresh the overview.
    pub(super) fn after_task_file_change(&mut self, id: &str) {
        if let Some(ctx) = self.contexts.get_mut(id) {
            let _ = ctx.reload_meta();
        }
        if self.active_task.as_deref() == Some(id) {
            self.reload_editor_if_context();
        }
        self.refresh_next_up();
    }

    /// Whether a task is finished and old enough to hide.
    pub fn is_archived(&self, id: &str) -> bool {
        let days = self.config.archive_after_days;
        if days == 0 || self.show_archived {
            return false;
        }
        let Some(store) = &self.store else {
            return false;
        };
        let last = store.board().columns.len().saturating_sub(1);
        if store.board().columns.len() < 2 || store.board().locate(id).map(|l| l.0) != Some(last) {
            return false;
        }
        self.record(id)
            .and_then(|r| r.finished.as_deref())
            .and_then(crate::time::parse_rfc3339)
            .is_some_and(|f| now_secs().saturating_sub(f) > days * 86_400)
    }

    /// Refresh the record cache (by file mtime), "next up" and "today".
    pub(super) fn refresh_next_up(&mut self) {
        let Some(store) = &self.store else {
            self.next_up.clear();
            self.records.clear();
            return;
        };
        let cols = &store.board().columns;
        let mut seen = Vec::new();
        for col in cols {
            for id in &col.tasks {
                seen.push(id.clone());
                let path = store.context_path(id);
                let m = mtime(&path);
                let fresh = self
                    .records
                    .get(id)
                    .is_some_and(|(old, r)| *old == m && r.column == col.name);
                if !fresh {
                    self.records
                        .insert(id.clone(), (m, TaskRecord::load(id, &col.name, &path)));
                }
            }
        }
        self.records.retain(|id, _| seen.contains(id));
        let open = if cols.len() > 1 {
            cols.len() - 1
        } else {
            cols.len()
        };
        self.next_up = cols[..open]
            .iter()
            .rev()
            .flat_map(|c| c.tasks.iter())
            .filter_map(|id| self.records.get(id).map(|(_, r)| r.clone()))
            .collect();
        self.today = self.compute_today();
        if !self.plan.is_empty() {
            self.sync_plan_marks();
        }
    }

    fn compute_today(&self) -> Today {
        let now = now_secs();
        let offset = local_offset_secs();
        let day = local_day_start(now, offset);
        let week = local_week_start(now, offset);
        let entries = ledger::read(&self.config.tasks_dir);
        let today_by_task = ledger::per_task_since(&entries, day);
        let week_min = entries
            .iter()
            .filter(|e| e.at >= week)
            .map(|e| e.minutes)
            .sum();
        let finished_week = self
            .records
            .values()
            .filter_map(|(_, r)| r.finished.as_deref().and_then(crate::time::parse_rfc3339))
            .filter(|f| *f >= week)
            .count();
        let (mut spent, mut est, mut n) = (0u64, 0u64, 0usize);
        for (_, r) in self.records.values() {
            for c in r
                .checkpoints
                .iter()
                .filter(|c| c.done && c.spent_min > 0 && c.estimate_min > 0)
            {
                spent += c.spent_min;
                est += c.estimate_min;
                n += 1;
            }
        }
        Today {
            today_min: today_by_task.iter().map(|(_, m)| m).sum(),
            week_min,
            today_by_task,
            finished_week,
            estimate_factor: (n >= 5 && est > 0).then(|| (spent as f64 / est as f64, n)),
        }
    }

    /// Keep the list selection on a task row after the rows changed.
    pub(super) fn fix_list_selection(&mut self) {
        let rows = self.rows();
        if rows.is_empty() {
            self.list_selected = 0;
            return;
        }
        let i = self.list_selected.min(rows.len() - 1);
        self.list_selected = (i..rows.len())
            .chain((0..i).rev())
            .find(|&j| matches!(rows[j], ListRow::Task(..)))
            .unwrap_or(i);
    }

    fn context_dirty_in_editor(&self, id: &str) -> bool {
        self.contexts.get(id).is_some_and(|c| {
            c.editor
                .as_ref()
                .is_some_and(|e| e.path() == c.context_path && e.is_dirty())
        })
    }

    /// Pick up changes made outside pahiri: the board, task folders, the open
    /// task's CONTEXT.md and its file tree.
    pub(super) fn reload_outside_changes(&mut self) {
        let selected = self.selected_task_id();
        let changed = match &mut self.store {
            Some(store) => store.reload_if_changed().unwrap_or(false),
            None => false,
        };
        if changed {
            if let Some(id) = selected {
                self.select_task_row(&id);
            }
            self.fix_list_selection();
            let gone = self
                .active_task
                .as_ref()
                .is_some_and(|id| !self.config.tasks_dir.join(id).is_dir());
            if gone {
                if let Some(id) = self.active_task.take() {
                    self.contexts.remove(&id);
                }
                self.mode = Mode::Home;
            }
            self.refresh_next_up();
        }
        let Some(id) = self.active_task.clone() else {
            return;
        };
        let Some(ctx) = self.contexts.get_mut(&id) else {
            return;
        };
        if ctx.tree.changed_on_disk() {
            let _ = ctx.tree.refresh();
        }
        if ctx.context_changed_on_disk() {
            let _ = ctx.reload_meta();
            self.reload_editor_if_context();
            self.refresh_next_up();
        }
    }

    // ----- delete ------------------------------------------------------------------

    pub(super) fn request_delete_task(&mut self) {
        let Some(id) = self.current_task_id() else {
            self.set_status("no task selected");
            return;
        };
        let live = self.contexts.get(&id).map_or(0, TaskContext::live_shells);
        if live > 0 {
            self.error(format!(
                "{id} has {live} running shell(s). Close them first (Esc x), then delete."
            ));
            return;
        }
        let trash = self
            .store
            .as_ref()
            .map(|s| s.tasks_dir().join(crate::tasks::store::TRASH_DIR))
            .unwrap_or_default();
        self.popup = Some(Popup::confirm(
            "Delete task?",
            format!(
                "Move {id} and everything in its folder to\n{}\n\nRestore it by moving the folder back; `pahiri trash empty` deletes old ones.",
                trash.display()
            ),
            Pending::DeleteTask(id),
        ));
    }

    pub(super) fn delete_task(&mut self, id: &str) {
        if self.timer.as_ref().is_some_and(|t| t.task_id == id) {
            self.timer = None;
        }
        let Some(store) = &mut self.store else { return };
        match store.trash_task(id) {
            Ok(target) => {
                if let Some(ctx) = self.contexts.remove(id) {
                    ctx.kill_tmux();
                }
                self.restore_shells.remove(id);
                if self.active_task.as_deref() == Some(id) {
                    self.active_task = None;
                    self.mode = Mode::Home;
                }
                self.fix_list_selection();
                self.refresh_next_up();
                self.set_status(format!("{id} moved to {}", target.display()));
                let extra = vec![
                    ("PAHIRI_TRASH_PATH".into(), target.display().to_string()),
                    ("PAHIRI_TASK".into(), id.to_owned()),
                    ("PAHIRI_TASK_DIR".into(), target.display().to_string()),
                ];
                self.fire_hook(HookEvent::TaskDelete, None, extra, AfterHook::Nothing);
            }
            Err(e) => self.error(e.to_string()),
        }
    }

    // ----- dates and outcome ----------------------------------------------------------

    /// Record started / finished for a move and run the `task_move` hook.
    pub(super) fn on_task_moved(&mut self, id: &str, from: usize, to: usize) {
        let Some(store) = &self.store else { return };
        let n = store.categories().len();
        let from_name = store.categories()[from].clone();
        let to_name = store.categories()[to].clone();
        let Some(path) = self.context_path(id) else {
            return;
        };
        if let Err(e) = ctxfile::record_move(&path, id, from, to, n, &now_rfc3339()) {
            warn!("could not record dates for {id}: {e}");
        }
        let last = n.saturating_sub(1);
        let finished = n > 1 && to == last;
        if finished && self.timer.as_ref().is_some_and(|t| t.task_id == id) {
            self.stop_timer();
        }
        self.after_task_file_change(id);
        let flag = |b: bool| if b { "1" } else { "0" }.to_owned();
        self.fire_hook(
            HookEvent::TaskMove,
            Some(id),
            vec![
                ("PAHIRI_FROM_COLUMN".into(), from_name),
                ("PAHIRI_TO_COLUMN".into(), to_name),
                ("PAHIRI_FINISHED".into(), flag(finished)),
                (
                    "PAHIRI_REOPENED".into(),
                    flag(n > 1 && from == last && to != last),
                ),
            ],
            AfterHook::Nothing,
        );
    }

    /// `O`: ask for a one-line outcome.
    pub(super) fn request_outcome(&mut self) {
        let Some(id) = self.current_task_id() else {
            self.set_status("no task selected");
            return;
        };
        self.popup = Some(Popup::input(
            format!("Outcome · {id}"),
            "One line for your review log: what changed, for whom",
            "",
            Pending::Outcome(id),
        ));
    }

    pub(super) fn record_outcome(&mut self, id: &str, line: &str) {
        let Some(path) = self.context_path(id) else {
            return;
        };
        match ctxfile::append_under(&path, id, OUTCOME_HEADING, &now_rfc3339(), line) {
            Ok(()) => {
                self.after_task_file_change(id);
                self.set_status(format!("outcome recorded in {id}"));
            }
            Err(e) => self.error(format!("could not write the outcome: {e}")),
        }
    }

    // ----- checkpoints ---------------------------------------------------------------

    pub(super) fn show_checkpoints(&mut self) {
        let Some(id) = self.current_task_id() else {
            self.set_status("no task selected");
            return;
        };
        let Some(path) = self.context_path(&id) else {
            return;
        };
        let items = checkpoints::read(&path).unwrap_or_default();
        if items.is_empty() {
            self.popup = Some(Popup::message(
                format!("{id}: no checkpoints yet"),
                "Esc b asks the AI agent to break the task down (it needs a ready\n\
                 context: Esc i writes one, Esc r marks it ready yourself).\n\n\
                 Or write them in CONTEXT.md, under ## Checkpoints:\n  - [ ] Read the datasheet chapter (30m)\n  - [ ] Write the driver skeleton (1h)",
            ));
            return;
        }
        let selected = checkpoints::next_open(&items).unwrap_or(0);
        self.popup = Some(Popup::Checkpoints {
            task_id: id,
            items,
            selected,
        });
    }

    /// Toggle a checkpoint from the checkpoint popup (written immediately).
    pub(super) fn toggle_checkpoint(
        &mut self,
        id: &str,
        items: &mut [checkpoints::Checkpoint],
        i: usize,
    ) {
        let Some(c) = items.get_mut(i) else { return };
        c.done = !c.done;
        let done = c.clone();
        let line = if done.done {
            format!("✓ {}", done.title)
        } else {
            format!("reopened: {}", done.title)
        };
        let Some(path) = self.context_path(id) else {
            return;
        };
        let result = checkpoints::write(&path, id, items)
            .and_then(|()| ctxfile::append_log(&path, id, &now_rfc3339(), &line));
        if let Err(e) = result {
            self.error(format!("could not update checkpoints: {e}"));
        }
        self.after_task_file_change(id);
        if done.done {
            self.fire_hook(
                HookEvent::CheckpointDone,
                Some(id),
                Self::checkpoint_env(&done),
                AfterHook::Nothing,
            );
        }
    }

    pub(super) fn replace_checkpoints(&mut self, id: &str, items: &[checkpoints::Checkpoint]) {
        let Some(path) = self.context_path(id) else {
            return;
        };
        match checkpoints::write(&path, id, items) {
            Ok(()) => {
                self.after_task_file_change(id);
                self.set_status(format!(
                    "{} checkpoints written · Esc m starts the timer on the first",
                    items.len()
                ));
                self.fire_hook(
                    HookEvent::CheckpointsGenerated,
                    Some(id),
                    vec![("PAHIRI_CHECKPOINT_COUNT".into(), items.len().to_string())],
                    AfterHook::Nothing,
                );
            }
            Err(e) => self.error(format!("could not write checkpoints: {e}")),
        }
    }

    /// `v`: tick the timed checkpoint and move the timer on to the next one; with
    /// no timer on a checkpoint, tick the task's next open checkpoint.
    pub(super) fn checkpoint_done(&mut self) {
        let timed = self
            .timer
            .as_ref()
            .and_then(|t| t.checkpoint.clone().map(|c| (t.task_id.clone(), c)));
        if let Some((id, title)) = timed {
            let finished = self.book_time(true, true);
            self.timer = None;
            self.mark_plan_item_done(&id, &title);
            if let Some(c) = &finished {
                self.fire_hook(
                    HookEvent::CheckpointDone,
                    Some(&id),
                    Self::checkpoint_env(c),
                    AfterHook::Nothing,
                );
            }
            // The day plan decides what is next, when the timed item is in it.
            if let Some(next) = self.next_in_plan(&id, &title) {
                let label = self.plan_item_label(next);
                self.start_plan_item(next);
                self.set_status(format!("next in today's plan: {label} · timer running"));
                self.save_session();
                return;
            }
            let Some(path) = self.context_path(&id) else {
                return;
            };
            let items = checkpoints::read(&path).unwrap_or_default();
            match checkpoints::next_open(&items) {
                Some(i) => {
                    let title = items[i].title.clone();
                    self.start_timer(&id, Some(i));
                    self.set_status(format!(
                        "next: {title} ({}) · timer running",
                        fmt_minutes(items[i].estimate_min)
                    ));
                }
                None => self.set_status(format!("all checkpoints of {id} are done")),
            }
            self.save_session();
            return;
        }
        let Some(id) = self.current_task_id() else {
            self.set_status("no task selected");
            return;
        };
        let Some(path) = self.context_path(&id) else {
            return;
        };
        let mut items = checkpoints::read(&path).unwrap_or_default();
        let Some(i) = checkpoints::next_open(&items) else {
            self.set_status(format!("{id} has no open checkpoints"));
            return;
        };
        self.toggle_checkpoint(&id, &mut items, i);
        match checkpoints::next_open(&items) {
            Some(n) => self.set_status(format!(
                "✓ {} · next: {} ({})",
                items[i].title,
                items[n].title,
                fmt_minutes(items[n].estimate_min)
            )),
            None => self.set_status(format!("✓ {} · that was the last one", items[i].title)),
        }
    }

    // ----- timer -----------------------------------------------------------------------

    /// The timer menu (Esc m, leader m, or a click on the timer chip).
    pub(super) fn open_timer_menu(&mut self) {
        let current = self.current_task_id();
        let now = Instant::now();
        let mut choices = Vec::new();
        let title;
        if let Some(t) = &mut self.timer {
            t.acknowledged = true;
        }
        if let Some(t) = &self.timer {
            title = format!("Timer · {} · {} · {}", t.task_id, t.what(), t.label(now));
            let in_plan = t
                .checkpoint
                .as_deref()
                .and_then(|c| self.next_in_plan(&t.task_id, c))
                .map(|j| self.plan_item_label(j));
            let next =
                in_plan.or_else(|| self.next_checkpoint_label(&t.task_id, t.checkpoint.as_deref()));
            let over = t.remaining_secs(now) < 0;
            if t.is_running() {
                if !over {
                    choices.push(("pause".to_owned(), Pending::TimerPause));
                }
            } else {
                match t.away {
                    Some((d, why)) => {
                        let why = match why {
                            Away::Idle => "while idle",
                            Away::Closed => "while pahiri was closed",
                        };
                        let m = fmt_minutes((d.as_secs() + 30) / 60);
                        choices.push((
                            format!("resume, counting the {m} {why}"),
                            Pending::TimerResume(true),
                        ));
                        choices.push((
                            format!("resume without the {m}"),
                            Pending::TimerResume(false),
                        ));
                    }
                    None => choices.push(("resume".to_owned(), Pending::TimerResume(false))),
                }
            }
            if t.checkpoint.is_some() {
                choices.push((
                    match next {
                        Some(n) => format!("done → next: {n}"),
                        None => "done (that is the last checkpoint)".to_owned(),
                    },
                    Pending::TimerDone,
                ));
            }
            choices.push(("+5 min".to_owned(), Pending::TimerExtend(5)));
            choices.push(("+15 min".to_owned(), Pending::TimerExtend(15)));
            if t.is_running() && over {
                choices.push(("keep going (overtime counts)".to_owned(), Pending::Nothing));
                choices.push(("pause".to_owned(), Pending::TimerPause));
            }
            choices.push(("stop and book the time".to_owned(), Pending::TimerStop));
            if let Some(cur) = current.filter(|c| *c != t.task_id) {
                choices.push((
                    format!("stop it and start on {cur}"),
                    Pending::TimerStart(cur),
                ));
            }
            choices.push((
                "checkpoints…".to_owned(),
                Pending::Run(super::Action::Checkpoints),
            ));
        } else {
            let Some(id) = current else {
                self.set_status("select a task, then Esc m starts the timer on it");
                return;
            };
            title = format!("Timer · {id}");
            let planned = (matches!(self.mode, Mode::Home)
                && self.home_focus() == super::HomeFocus::Today)
                .then_some(self.today_selected())
                .filter(|&i| i < self.day_plan().len());
            if let Some(i) = planned {
                choices.push((
                    format!("start: {}", self.plan_item_label(i)),
                    Pending::PlanStart(i),
                ));
            }
            match self.next_checkpoint_label(&id, None) {
                Some(n) => choices.push((format!("start: {n}"), Pending::TimerStart(id.clone()))),
                None => choices.push((
                    format!(
                        "start a focus block ({} min)",
                        self.config.timer.focus_minutes
                    ),
                    Pending::TimerStart(id.clone()),
                )),
            }
            choices.push((
                "focus block of … minutes".to_owned(),
                Pending::StartFocusAsk(id),
            ));
            choices.push((
                "checkpoints…".to_owned(),
                Pending::Run(super::Action::Checkpoints),
            ));
        }
        self.popup = Some(Popup::choose(
            title,
            choices
                .into_iter()
                .map(|(label, pending)| Choice { label, pending })
                .collect(),
        ));
    }

    fn next_checkpoint_label(&self, id: &str, after: Option<&str>) -> Option<String> {
        let items = checkpoints::read(&self.context_path(id)?).ok()?;
        items
            .iter()
            .find(|c| !c.done && Some(c.title.as_str()) != after)
            .map(|c| {
                format!(
                    "{} ({})",
                    c.title,
                    fmt_minutes(c.estimate_min.saturating_sub(c.spent_min))
                )
            })
    }

    /// Start timing checkpoint `index` (or the next open one) of task `id`.
    /// Without open checkpoints, a focus block of the configured length.
    pub(super) fn start_timer(&mut self, id: &str, index: Option<usize>) {
        let Some(path) = self.context_path(id) else {
            return;
        };
        let items = checkpoints::read(&path).unwrap_or_default();
        let chosen = match index {
            Some(i) => items.get(i).map(|c| (i, c)),
            None => items.iter().enumerate().find(|(_, c)| !c.done),
        };
        if let Some((i, c)) = chosen {
            let budget = Duration::from_secs(c.estimate_min.saturating_sub(c.spent_min) * 60);
            let title = c.title.clone();
            self.begin_timer(id, Some((Some(i), title)), budget);
        } else {
            let m = self.config.timer.focus_minutes;
            self.begin_timer(id, None, Duration::from_secs(m * 60));
        }
    }

    pub(super) fn start_focus(&mut self, id: &str, minutes: &str) {
        match minutes.trim().parse::<u64>() {
            Ok(m) if m > 0 => self.begin_timer(id, None, Duration::from_secs(m * 60)),
            _ => self.set_status(format!("not a number of minutes: {minutes}")),
        }
    }

    /// Start timing `checkpoint` (its index, when it is one of the task's
    /// checkpoints, and its title) or a focus block.
    pub(super) fn begin_timer(
        &mut self,
        id: &str,
        checkpoint: Option<(Option<usize>, String)>,
        budget: Duration,
    ) {
        if self.timer.is_some() {
            self.stop_timer();
        }
        let mut timer = Timer::start(
            id,
            checkpoint.as_ref().map(|(_, t)| t.clone()),
            budget,
            Instant::now(),
        );
        timer.checkpoint_index = checkpoint.and_then(|(i, _)| i);
        let label = format!("⏱ {} · {}", timer.what(), timer.label(Instant::now()));
        self.timer = Some(timer);
        self.last_input = Instant::now();
        if let Some(path) = self.context_path(id) {
            let now = now_rfc3339();
            if let Err(e) = ctxfile::update_meta(&path, id, |m| {
                m.started.get_or_insert(now);
            }) {
                warn!("could not record start of {id}: {e}");
            }
        }
        self.after_task_file_change(id);
        self.set_status(label);
        self.save_session();
        let env = self.timer_env();
        self.fire_hook(HookEvent::TimerStart, Some(id), env, AfterHook::Nothing);
    }

    /// `M`: stop the timer and book the time.
    pub(super) fn stop_timer(&mut self) {
        let Some(id) = self.timer.as_ref().map(|t| t.task_id.clone()) else {
            self.set_status("no timer running");
            return;
        };
        let env = self.timer_env();
        self.book_time(true, false);
        self.timer = None;
        self.save_session();
        self.fire_hook(HookEvent::TimerStop, Some(&id), env, AfterHook::Nothing);
    }

    /// Write the minutes not yet recorded to the checkpoint, the task and the
    /// ledger. `last` rounds the remainder and logs the session; `done` ticks
    /// the checkpoint. Returns the checkpoint as written.
    pub(super) fn book_time(&mut self, last: bool, done: bool) -> Option<checkpoints::Checkpoint> {
        let now = Instant::now();
        let t = self.timer.as_mut()?;
        let mins = t.take_minutes(now, last);
        let session = (t.elapsed(now).as_secs() + 30) / 60;
        let id = t.task_id.clone();
        let what = t.checkpoint.clone();
        let index = t.checkpoint_index;
        if mins == 0 && !last && !done {
            return None;
        }
        let path = self.context_path(&id)?;
        let tasks_dir = self.config.tasks_dir.clone();
        let mut written = None;
        let result = (|| -> io::Result<()> {
            if let Some(title) = &what {
                let mut items = checkpoints::read(&path)?;
                let pos = index
                    .filter(|i| items.get(*i).is_some_and(|c| &c.title == title))
                    .or_else(|| items.iter().position(|c| &c.title == title));
                if let Some(c) = pos.and_then(|i| items.get_mut(i)) {
                    c.spent_min += mins;
                    c.done |= done;
                    written = Some(c.clone());
                }
                checkpoints::write(&path, &id, &items)?;
            }
            let stamp = now_rfc3339();
            if mins > 0 {
                ctxfile::update_meta(&path, &id, |m| m.time_spent_min += mins)?;
                ledger::append(
                    &tasks_dir,
                    &stamp,
                    &id,
                    mins,
                    what.as_deref().unwrap_or("focus block"),
                )?;
            }
            if last && (session > 0 || done) {
                let line = match (&what, done, &written) {
                    (Some(t), true, Some(c)) => format!(
                        "✓ {t} (spent {}, estimated {})",
                        fmt_minutes(c.spent_min),
                        fmt_minutes(c.estimate_min)
                    ),
                    (Some(t), _, _) => format!("⏱ {} on {t}", fmt_minutes(session)),
                    (None, _, _) => format!("⏱ {} focus block", fmt_minutes(session)),
                };
                ctxfile::append_log(&path, &id, &stamp, &line)?;
            }
            Ok(())
        })();
        if let Err(e) = result {
            warn!("could not record time for {id}: {e}");
            self.set_status(format!("could not record time: {e}"));
        }
        self.after_task_file_change(&id);
        written
    }

    pub(super) fn extend_timer(&mut self, minutes: u64) {
        if let Some(t) = &mut self.timer {
            t.extend(minutes);
            t.resume_with(Instant::now(), false);
            self.set_status(format!("+{minutes} min"));
            self.save_session();
        }
    }

    pub(super) fn pause_timer(&mut self) {
        let Some(t) = &mut self.timer else { return };
        t.pause(Instant::now());
        let id = t.task_id.clone();
        self.book_time(false, false);
        self.set_status("timer paused · Esc m resumes");
        self.save_session();
        let env = self.timer_env();
        self.fire_hook(HookEvent::TimerPause, Some(&id), env, AfterHook::Nothing);
    }

    pub(super) fn resume_timer(&mut self, count_away: bool) {
        let Some(t) = &mut self.timer else { return };
        t.resume_with(Instant::now(), count_away);
        let id = t.task_id.clone();
        self.last_input = Instant::now();
        self.set_status("timer resumed");
        self.save_session();
        let env = self.timer_env();
        self.fire_hook(HookEvent::TimerResume, Some(&id), env, AfterHook::Nothing);
    }

    /// Periodic work: timer expiry and idle check, autosave, outside changes.
    pub(super) fn on_tick(&mut self) {
        let now = Instant::now();
        if self.flash_start.is_some_and(|s| s.elapsed() >= FLASH) {
            self.flash_start = None;
        }
        let idle_after = Duration::from_secs(self.config.timer.idle_minutes * 60);
        let (mut alarm, mut autosave, mut idle) = (false, false, false);
        if let Some(t) = &mut self.timer {
            alarm = t.check_expiry(now);
            autosave = t.is_running() && t.unflushed_secs(now) >= AUTOSAVE_SECS;
            idle = t.is_running()
                && !idle_after.is_zero()
                && now.saturating_duration_since(self.last_input) >= idle_after;
        }
        if alarm {
            self.alarm();
        }
        if idle {
            let last = self.last_input;
            if let Some(t) = &mut self.timer {
                t.pause_idle(last, now);
            }
            self.book_time(false, false);
            self.set_status(format!(
                "timer paused: no input for {} min · Esc m to resume",
                self.config.timer.idle_minutes
            ));
            self.save_session();
            let id = self.timer.as_ref().map(|t| t.task_id.clone());
            let env = self.timer_env();
            self.fire_hook(
                HookEvent::TimerPause,
                id.as_deref(),
                env,
                AfterHook::Nothing,
            );
        } else if autosave {
            let id = self
                .timer
                .as_ref()
                .map(|t| t.task_id.clone())
                .unwrap_or_default();
            if !self.context_dirty_in_editor(&id) {
                self.book_time(false, false);
            }
        }
        if self.timer.as_ref().is_some_and(Timer::is_running)
            && self.session_saved.elapsed() >= Duration::from_secs(SESSION_SECS)
        {
            self.save_session();
        }
        if self.watched.elapsed() >= WATCH {
            self.watched = now;
            self.reload_outside_changes();
            if self.store.is_some() {
                self.check_day();
            }
        }
    }

    /// Time is up: flash, bell, a blinking chip and the hook. No popup — the
    /// keys stay with whatever you are typing into.
    fn alarm(&mut self) {
        if self.config.timer.flash {
            self.flash_start = Some(Instant::now());
        }
        self.bell = self.config.timer.bell;
        let Some(t) = &self.timer else { return };
        let (id, what) = (t.task_id.clone(), t.what().to_owned());
        self.set_status(format!("time's up: {what} · click the timer or Esc m"));
        let env = self.timer_env();
        self.fire_hook(HookEvent::TimerExpire, Some(&id), env, AfterHook::Nothing);
    }
}
