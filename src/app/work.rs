//! Doing the work: deleting tasks, checkpoints, the timer and its alarm,
//! task timestamps, and the "next up" overview.

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tracing::warn;

use crate::tasks::checkpoints::{self, fmt_minutes};
use crate::tasks::context::{self as ctxfile, OUTCOME_HEADING};
use crate::tasks::record::TaskRecord;
use crate::time::now_rfc3339;

use super::timer::Timer;
use super::{App, Choice, ListRow, Mode, Pending, Popup, TaskContext};

/// Save the running timer to disk this often (crash safety).
const AUTOSAVE_SECS: u64 = 300;
/// How long the screen flashes when time is up.
const FLASH: Duration = Duration::from_millis(1600);
/// Length of one flash pulse.
const FLASH_PULSE_MS: u128 = 200;

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

    pub(super) fn refresh_next_up(&mut self) {
        let Some(store) = &self.store else {
            self.next_up.clear();
            return;
        };
        let cols = &store.board().columns;
        let open = if cols.len() > 1 {
            cols.len() - 1
        } else {
            cols.len()
        };
        let mut out = Vec::new();
        for col in cols[..open].iter().rev() {
            for id in &col.tasks {
                out.push(TaskRecord::load(id, &col.name, &store.context_path(id)));
            }
        }
        self.next_up = out;
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
                "Move {id} and everything in its folder to\n{}\n\nRestore it by moving the folder back; delete it there for good.",
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
                self.contexts.remove(id);
                if self.active_task.as_deref() == Some(id) {
                    self.active_task = None;
                    self.mode = Mode::TaskList;
                }
                self.fix_list_selection();
                self.refresh_next_up();
                self.set_status(format!("{id} moved to {}", target.display()));
            }
            Err(e) => self.error(e.to_string()),
        }
    }

    // ----- timestamps ---------------------------------------------------------------

    /// Record started / finished when a task changes column; ask for an outcome
    /// line when it lands in the last column.
    pub(super) fn on_task_moved(&mut self, id: &str, from: usize, to: usize) {
        let Some(n) = self.store.as_ref().map(|s| s.categories().len()) else {
            return;
        };
        let Some(path) = self.context_path(id) else {
            return;
        };
        let last = n.saturating_sub(1);
        let finished = n > 1 && to == last;
        let now = now_rfc3339();
        let result = ctxfile::update_meta(&path, id, |m| {
            if to > 0 && m.started.is_none() {
                m.started = Some(now.clone());
            }
            if finished {
                m.finished = Some(now.clone());
            } else if from == last {
                m.finished = None;
            }
        });
        if let Err(e) = result {
            warn!("could not record dates for {id}: {e}");
        }
        if finished {
            if self.timer.as_ref().is_some_and(|t| t.task_id == id) {
                self.stop_timer();
            }
            self.popup = Some(Popup::input(
                format!("{id} is done"),
                "Outcome in one line, for your review log (Esc skips)",
                "",
                Pending::Outcome(id.to_owned()),
            ));
        }
        self.after_task_file_change(id);
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

    /// Move a task out of the first column when work starts (boards with 3+ columns).
    fn promote_from_first_column(&mut self, id: &str) {
        let Some(store) = &mut self.store else { return };
        if store.categories().len() < 3 || store.board().locate(id).map(|(c, _)| c) != Some(0) {
            return;
        }
        if matches!(store.move_task(id, 1), Ok(true)) {
            let name = store.categories()[1].clone();
            self.select_task_row(id);
            self.set_status(format!("{id} moved to {name}"));
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
        let line = if c.done {
            format!("✓ {}", c.title)
        } else {
            format!("reopened: {}", c.title)
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
    }

    pub(super) fn replace_checkpoints(&mut self, id: &str, items: &[checkpoints::Checkpoint]) {
        let Some(path) = self.context_path(id) else {
            return;
        };
        match checkpoints::write(&path, id, items) {
            Ok(()) => {
                self.after_task_file_change(id);
                self.set_status(format!(
                    "{} checkpoints written · m starts the timer on the first",
                    items.len()
                ));
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
            .filter(|t| t.checkpoint.is_some())
            .map(|t| t.task_id.clone());
        if let Some(id) = timed {
            self.book_time(true, true);
            self.timer = None;
            let Some(path) = self.context_path(&id) else {
                return;
            };
            let items = checkpoints::read(&path).unwrap_or_default();
            match checkpoints::next_open(&items) {
                Some(i) => {
                    let title = items[i].title.clone();
                    self.start_timer(&id, Some(&title));
                    self.set_status(format!(
                        "next: {title} ({}) · timer running · m pauses",
                        fmt_minutes(items[i].estimate_min)
                    ));
                }
                None => self.set_status(format!("all checkpoints of {id} are done")),
            }
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

    /// `m`: pause / resume the timer of the current task, or start one.
    pub(super) fn toggle_timer(&mut self) {
        let now = Instant::now();
        let current = self.current_task_id();
        if let Some(t) = &mut self.timer {
            if current.is_none() || current.as_deref() == Some(t.task_id.as_str()) {
                if t.is_running() {
                    t.pause(now);
                    self.book_time(false, false);
                    self.set_status("timer paused · m resumes · M stops");
                } else {
                    t.resume(now);
                    self.set_status("timer resumed");
                }
                return;
            }
            self.stop_timer();
        }
        let Some(id) = current else {
            self.set_status("select a task first");
            return;
        };
        self.start_timer(&id, None);
    }

    /// Start timing `title` (or the next open checkpoint) of task `id`. Without
    /// open checkpoints, ask for a focus block length.
    pub(super) fn start_timer(&mut self, id: &str, title: Option<&str>) {
        let Some(path) = self.context_path(id) else {
            return;
        };
        let items = checkpoints::read(&path).unwrap_or_default();
        let chosen = match title {
            Some(t) => items.iter().find(|c| c.title == t),
            None => items.iter().find(|c| !c.done),
        };
        if let Some(c) = chosen {
            let budget = Duration::from_secs(c.estimate_min.saturating_sub(c.spent_min) * 60);
            let title = c.title.clone();
            self.begin_timer(id, Some(title), budget);
            return;
        }
        self.popup = Some(Popup::input(
            "Focus block",
            format!("{id} has no open checkpoints. Minutes to focus (Esc b plans checkpoints):"),
            self.config.timer.focus_minutes.to_string(),
            Pending::StartFocus(id.to_owned()),
        ));
    }

    pub(super) fn start_focus(&mut self, id: &str, minutes: &str) {
        match minutes.trim().parse::<u64>() {
            Ok(m) if m > 0 => self.begin_timer(id, None, Duration::from_secs(m * 60)),
            _ => self.set_status(format!("not a number of minutes: {minutes}")),
        }
    }

    fn begin_timer(&mut self, id: &str, checkpoint: Option<String>, budget: Duration) {
        if self.timer.is_some() {
            self.stop_timer();
        }
        let timer = Timer::start(id, checkpoint, budget, Instant::now());
        let label = format!("⏱ {} · {}", timer.what(), timer.label(Instant::now()));
        self.timer = Some(timer);
        if let Some(path) = self.context_path(id) {
            let now = now_rfc3339();
            if let Err(e) = ctxfile::update_meta(&path, id, |m| {
                m.started.get_or_insert(now);
            }) {
                warn!("could not record start of {id}: {e}");
            }
        }
        self.promote_from_first_column(id);
        self.after_task_file_change(id);
        self.set_status(label);
    }

    /// `M`: stop the timer and book the time.
    pub(super) fn stop_timer(&mut self) {
        if self.timer.is_none() {
            self.set_status("no timer running");
            return;
        }
        self.book_time(true, false);
        self.timer = None;
    }

    /// Write the minutes not yet recorded to the checkpoint and the task.
    /// `last` rounds the remainder and logs the session; `done` ticks the checkpoint.
    pub(super) fn book_time(&mut self, last: bool, done: bool) {
        let now = Instant::now();
        let Some(t) = &mut self.timer else { return };
        let mins = t.take_minutes(now, last);
        let session = (t.elapsed(now).as_secs() + 30) / 60;
        let id = t.task_id.clone();
        let what = t.checkpoint.clone();
        if mins == 0 && !last && !done {
            return;
        }
        let Some(path) = self.context_path(&id) else {
            return;
        };
        let result = (|| -> io::Result<()> {
            let mut spent_total = None;
            if let Some(title) = &what {
                let mut items = checkpoints::read(&path)?;
                if let Some(c) = items.iter_mut().find(|c| &c.title == title) {
                    c.spent_min += mins;
                    c.done |= done;
                    spent_total = Some((c.spent_min, c.estimate_min));
                }
                checkpoints::write(&path, &id, &items)?;
            }
            if mins > 0 {
                ctxfile::update_meta(&path, &id, |m| m.time_spent_min += mins)?;
            }
            if last && (session > 0 || done) {
                let line = match (&what, done, spent_total) {
                    (Some(t), true, Some((s, e))) => {
                        format!(
                            "✓ {t} (spent {}, estimated {})",
                            fmt_minutes(s),
                            fmt_minutes(e)
                        )
                    }
                    (Some(t), _, _) => format!("⏱ {} on {t}", fmt_minutes(session)),
                    (None, _, _) => format!("⏱ {} focus block", fmt_minutes(session)),
                };
                ctxfile::append_log(&path, &id, &now_rfc3339(), &line)?;
            }
            Ok(())
        })();
        if let Err(e) = result {
            warn!("could not record time for {id}: {e}");
            self.set_status(format!("could not record time: {e}"));
        }
        self.after_task_file_change(&id);
    }

    pub(super) fn extend_timer(&mut self, minutes: u64) {
        if let Some(t) = &mut self.timer {
            t.extend(minutes);
            t.resume(Instant::now());
            self.set_status(format!("+{minutes} min"));
        }
    }

    pub(super) fn pause_timer(&mut self) {
        if let Some(t) = &mut self.timer {
            t.pause(Instant::now());
            self.book_time(false, false);
            self.set_status("timer paused · m resumes");
        }
    }

    /// Periodic work: timer expiry, autosave, and picking up outside edits.
    pub(super) fn on_tick(&mut self) {
        let now = Instant::now();
        if self.flash_start.is_some_and(|s| s.elapsed() >= FLASH) {
            self.flash_start = None;
        }
        let mut alarm = false;
        let mut autosave = false;
        if let Some(t) = &mut self.timer {
            alarm = t.check_expiry(now);
            autosave = t.is_running() && t.unflushed_secs(now) >= AUTOSAVE_SECS;
        }
        if alarm {
            self.alarm();
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
        self.watch_context_file();
    }

    /// Reload the active task when its context file changed on disk (an agent or
    /// `pahiri task …` wrote to it).
    fn watch_context_file(&mut self) {
        let Some(id) = self.active_task.clone() else {
            return;
        };
        let changed = self
            .contexts
            .get(&id)
            .is_some_and(TaskContext::context_changed_on_disk);
        if changed {
            if let Some(ctx) = self.contexts.get_mut(&id) {
                let _ = ctx.reload_meta();
            }
            self.reload_editor_if_context();
        }
    }

    fn alarm(&mut self) {
        if self.config.timer.flash {
            self.flash_start = Some(Instant::now());
        }
        self.bell = self.config.timer.bell;
        let Some(t) = &self.timer else { return };
        let what = t.what().to_owned();
        let id = t.task_id.clone();
        let is_checkpoint = t.checkpoint.is_some();
        let mut choices = Vec::new();
        if is_checkpoint {
            let next = self
                .context_path(&id)
                .and_then(|p| checkpoints::read(&p).ok())
                .and_then(|items| {
                    items
                        .iter()
                        .find(|c| !c.done && c.title != what)
                        .map(|c| format!("{} ({})", c.title, fmt_minutes(c.estimate_min)))
                });
            choices.push(Choice {
                label: match next {
                    Some(n) => format!("done — start next: {n}"),
                    None => "done — that was the last checkpoint".to_owned(),
                },
                pending: Pending::TimerDone,
            });
        } else {
            choices.push(Choice {
                label: "stop and book the time".into(),
                pending: Pending::TimerStop,
            });
        }
        for m in [5, 15] {
            choices.push(Choice {
                label: format!("+{m} min on this"),
                pending: Pending::TimerExtend(m),
            });
        }
        choices.push(Choice {
            label: "keep going (overtime is counted)".into(),
            pending: Pending::Nothing,
        });
        choices.push(Choice {
            label: "pause".into(),
            pending: Pending::TimerPause,
        });
        if is_checkpoint {
            choices.push(Choice {
                label: "stop and book the time".into(),
                pending: Pending::TimerStop,
            });
        }
        let title = format!("Time's up · {id} · {what}");
        if self.popup.is_none() || matches!(self.popup, Some(Popup::Palette(_))) {
            self.leader_pending = false;
            self.popup = Some(Popup::choose(title, choices));
        } else {
            self.set_status(title);
        }
    }
}
