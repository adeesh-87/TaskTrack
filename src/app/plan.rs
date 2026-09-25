//! Planning the day. The Plan view (`p` on Home) picks checkpoints of the
//! tasks in progress and orders them; the Today pane on Home works through
//! the plan, and the timer's "done → next" follows it.

use std::fs;
use std::time::SystemTime;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::hooks::HookEvent;
use crate::tasks::checkpoints::fmt_minutes;
use crate::tasks::plan::{self, PlanItem};
use crate::time::{local_offset_secs, now_secs};

use super::hooks::AfterHook;
use super::{App, Mode, Pending, Popup};

/// Estimate for items typed without one.
const DEFAULT_ITEM_MIN: u64 = 30;
/// Remembers the last day pahiri ran (for the `day_start` hook).
const LAST_DAY_FILE: &str = "last_day";

/// Which pane of Home has the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HomeFocus {
    /// The board (task list) on the left.
    #[default]
    Board,
    /// The Today pane on the right.
    Today,
}

/// Which pane of the Plan view has the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanFocus {
    /// Left: what can be planned.
    Pick,
    /// Right: the plan, in order.
    Order,
}

/// A row of the Plan view's left pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanRow {
    /// A heading ("CARRIED OVER · Thu 24 Sep", a column name).
    Section(String),
    /// A task: id and title.
    Task(String, String),
    /// Something that can be picked.
    Item(PlanItem),
    /// An explanation (a task without open checkpoints).
    Hint(String),
}

/// What a plan item looks like right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ItemStatus {
    /// Done (the checkpoint is ticked, or the item itself).
    pub done: bool,
    /// Minutes already booked on the checkpoint.
    pub spent_min: u64,
    /// Index among the task's checkpoints, when the item is one.
    pub checkpoint: Option<usize>,
    /// The item's task does not exist (any more).
    pub missing: bool,
    /// The timer runs on it.
    pub timed: bool,
}

impl ItemStatus {
    /// Estimate minus what was already spent.
    pub fn remaining_min(self, item: &PlanItem) -> u64 {
        item.estimate_min.saturating_sub(self.spent_min)
    }
}

/// The Plan view's state.
#[derive(Debug, Clone)]
pub struct PlanView {
    /// Day being planned (`YYYY-MM-DD`).
    pub date: String,
    /// Left pane.
    pub rows: Vec<PlanRow>,
    /// Right pane: the plan in order.
    pub picked: Vec<PlanItem>,
    /// Which pane has the keys.
    pub focus: PlanFocus,
    /// Selected row of the left pane.
    pub cursor: usize,
    /// Selected row of the right pane.
    pub order_cursor: usize,
    /// Planned minutes that fit in the day.
    pub capacity_min: u64,
    /// Your pace: spent ÷ estimated on finished checkpoints (1.0 when unknown).
    pub factor: f64,
    /// Offer every open column, not only the configured ones.
    pub all_columns: bool,
    /// Changed since opening.
    pub dirty: bool,
    /// Unfinished items of the last earlier plan.
    carried: Vec<PlanItem>,
    /// Open checkpoints per task, in board priority order (for suggest).
    groups: Vec<Vec<PlanItem>>,
    /// Status of every item on screen.
    statuses: Vec<(PlanItem, ItemStatus)>,
}

impl PlanView {
    /// Whether `item` is in the plan.
    pub fn is_picked(&self, item: &PlanItem) -> bool {
        self.picked.iter().any(|p| p.same(item))
    }

    /// Status of `item` when the view opened.
    pub fn status(&self, item: &PlanItem) -> ItemStatus {
        self.statuses
            .iter()
            .find(|(i, _)| i.same(item))
            .map(|(_, s)| *s)
            .unwrap_or_default()
    }

    /// Minutes `item` will likely take (remaining estimate × pace; done: 0).
    pub fn cost(&self, item: &PlanItem) -> u64 {
        let s = self.status(item);
        if s.done || item.done {
            return 0;
        }
        (s.remaining_min(item) as f64 * self.factor).round() as u64
    }

    /// Likely minutes of everything picked.
    pub fn planned_min(&self) -> u64 {
        self.picked.iter().map(|i| self.cost(i)).sum()
    }

    fn selectable(&self, i: usize) -> bool {
        matches!(self.rows.get(i), Some(PlanRow::Task(..) | PlanRow::Item(_)))
    }

    /// Put the cursor on the first row that can be picked.
    pub fn cursor_to_first(&mut self) {
        self.cursor = (0..self.rows.len())
            .find(|i| self.selectable(*i))
            .unwrap_or(0);
    }

    /// Move the cursor of the focused pane.
    pub fn move_cursor(&mut self, delta: i32) {
        match self.focus {
            PlanFocus::Pick => {
                let mut i = self.cursor as i64;
                loop {
                    i += i64::from(delta.signum());
                    if i < 0 || i >= self.rows.len() as i64 {
                        return;
                    }
                    if self.selectable(i as usize) {
                        self.cursor = i as usize;
                        return;
                    }
                }
            }
            PlanFocus::Order => {
                let last = self.picked.len().saturating_sub(1) as i64;
                self.order_cursor =
                    (self.order_cursor as i64 + i64::from(delta)).clamp(0, last) as usize;
            }
        }
    }

    /// The items of the task whose header is at row `i`.
    fn task_items(&self, i: usize) -> Vec<PlanItem> {
        self.rows[i + 1..]
            .iter()
            .take_while(|r| matches!(r, PlanRow::Item(_) | PlanRow::Hint(_)))
            .filter_map(|r| match r {
                PlanRow::Item(item) => Some(item.clone()),
                _ => None,
            })
            .collect()
    }

    /// Space: pick or unpick the item under the cursor (on a task: all its items).
    pub fn toggle(&mut self) {
        let items = match self.rows.get(self.cursor) {
            Some(PlanRow::Item(item)) => vec![item.clone()],
            Some(PlanRow::Task(..)) => self.task_items(self.cursor),
            _ => return,
        };
        if items.is_empty() {
            return;
        }
        if items.iter().all(|i| self.is_picked(i)) {
            self.picked.retain(|p| !items.iter().any(|i| i.same(p)));
        } else {
            for item in items {
                self.add(item);
            }
        }
        self.dirty = true;
        self.order_cursor = self.order_cursor.min(self.picked.len().saturating_sub(1));
    }

    /// Add `item` at the end unless it is already planned.
    pub fn add(&mut self, item: PlanItem) {
        if !self.is_picked(&item) {
            self.picked.push(item);
            self.dirty = true;
        }
    }

    /// Task under the cursor (its header or one of its items).
    pub fn task_at_cursor(&self) -> Option<String> {
        match self.focus {
            PlanFocus::Pick => match self.rows.get(self.cursor)? {
                PlanRow::Task(id, _) => Some(id.clone()),
                PlanRow::Item(item) => item.task.clone(),
                _ => None,
            },
            PlanFocus::Order => self.picked.get(self.order_cursor)?.task.clone(),
        }
    }

    /// Move the selected plan item up or down.
    pub fn shift_picked(&mut self, delta: i32) {
        let i = self.order_cursor;
        let j = i as i64 + i64::from(delta);
        if i >= self.picked.len() || j < 0 || j >= self.picked.len() as i64 {
            return;
        }
        self.picked.swap(i, j as usize);
        self.order_cursor = j as usize;
        self.dirty = true;
    }

    /// Drop the selected plan item.
    pub fn remove_picked(&mut self) {
        if self.order_cursor < self.picked.len() {
            self.picked.remove(self.order_cursor);
            self.order_cursor = self.order_cursor.min(self.picked.len().saturating_sub(1));
            self.dirty = true;
        }
    }

    /// Fill the day: what was carried over, then each task's next checkpoint
    /// in board order, round after round, while it fits the capacity.
    pub fn suggest(&mut self) {
        let mut picked: Vec<PlanItem> = self.carried.clone();
        let mut total: u64 = picked.iter().map(|i| self.cost(i)).sum();
        let mut next = vec![0usize; self.groups.len()];
        let mut open = vec![true; self.groups.len()];
        loop {
            let mut added = false;
            for (g, group) in self.groups.iter().enumerate() {
                if !open[g] {
                    continue;
                }
                while next[g] < group.len() && picked.iter().any(|p| p.same(&group[next[g]])) {
                    next[g] += 1;
                }
                let Some(item) = group.get(next[g]) else {
                    open[g] = false;
                    continue;
                };
                let cost = self.cost(item);
                if total + cost > self.capacity_min {
                    // Checkpoints go in order: this task is done for today.
                    open[g] = false;
                    continue;
                }
                total += cost;
                picked.push(item.clone());
                next[g] += 1;
                added = true;
            }
            if !added {
                break;
            }
        }
        self.picked = picked;
        self.order_cursor = 0;
        self.dirty = true;
    }
}

impl App {
    // ----- accessors used by the UI ------------------------------------------------

    /// Today's plan, in order.
    pub fn day_plan(&self) -> &[PlanItem] {
        &self.plan
    }

    /// The day the plan is for (`YYYY-MM-DD`).
    pub fn plan_date(&self) -> &str {
        &self.plan_date
    }

    /// Which pane of Home has the keys.
    pub fn home_focus(&self) -> HomeFocus {
        self.home_focus
    }

    /// Selected item of the Today pane.
    pub fn today_selected(&self) -> usize {
        self.today_selected
    }

    /// Unfinished items of the last earlier plan while today has none: (day, count).
    pub fn carry_hint(&self) -> Option<&(String, usize)> {
        self.carry_hint.as_ref()
    }

    /// Your pace (spent ÷ estimated), 1.0 until enough checkpoints are finished.
    pub fn pace(&self) -> f64 {
        self.today().estimate_factor.map_or(1.0, |(f, _)| f)
    }

    /// Likely minutes of the open plan items (remaining × pace).
    pub fn planned_open_min(&self) -> u64 {
        let pace = self.pace();
        self.plan
            .iter()
            .map(|i| (i, self.item_status(i)))
            .filter(|(i, s)| !s.done && !i.done)
            .map(|(i, s)| (s.remaining_min(i) as f64 * pace).round() as u64)
            .sum()
    }

    /// What a plan item looks like now (from the task records and the timer).
    pub fn item_status(&self, item: &PlanItem) -> ItemStatus {
        let mut s = ItemStatus {
            done: item.done,
            ..ItemStatus::default()
        };
        let Some(task) = &item.task else {
            return s;
        };
        let Some(record) = self.record(task) else {
            s.missing = true;
            return s;
        };
        if let Some(i) = record
            .checkpoints
            .iter()
            .position(|c| c.title == item.title)
        {
            let c = &record.checkpoints[i];
            s.done = c.done;
            s.spent_min = c.spent_min;
            s.checkpoint = Some(i);
        }
        s.timed = self
            .timer()
            .is_some_and(|t| t.task_id == *task && t.checkpoint.as_deref() == Some(&item.title));
        s
    }

    // ----- loading and saving ------------------------------------------------------

    /// Today's date in local time.
    pub(super) fn today_date() -> String {
        plan::local_date(now_secs(), local_offset_secs())
    }

    fn plan_mtime(&self) -> Option<SystemTime> {
        fs::metadata(plan::path(&self.config.tasks_dir, &self.plan_date))
            .and_then(|m| m.modified())
            .ok()
    }

    /// (Re)read today's plan and the carry-over hint.
    pub(super) fn load_plan(&mut self) {
        self.plan_date = Self::today_date();
        let path = plan::path(&self.config.tasks_dir, &self.plan_date);
        self.plan = plan::read(&path).unwrap_or_default();
        self.plan_mtime = self.plan_mtime();
        self.today_selected = self.today_selected.min(self.plan.len().saturating_sub(1));
        if self.plan.is_empty() {
            self.home_focus = HomeFocus::Board;
        }
        self.carry_hint = if self.plan.is_empty() {
            self.carried_items().map(|(day, items)| (day, items.len()))
        } else {
            None
        };
        if self.carry_hint.as_ref().is_some_and(|(_, n)| *n == 0) {
            self.carry_hint = None;
        }
    }

    /// Unfinished items of the last plan before today, as (day, items).
    fn carried_items(&self) -> Option<(String, Vec<PlanItem>)> {
        let (day, items) = plan::previous(&self.config.tasks_dir, &self.plan_date)?;
        let open = items
            .into_iter()
            .filter(|i| {
                let s = self.item_status(i);
                !s.done && !s.missing
            })
            .collect();
        Some((day, open))
    }

    /// Past midnight or changed on disk: reload the plan. The first run of a
    /// day fires `day_start`.
    pub(super) fn check_day(&mut self) {
        let today = Self::today_date();
        if today != self.plan_date || self.plan_mtime() != self.plan_mtime {
            self.load_plan();
        }
        let marker = self.state_dir.join(LAST_DAY_FILE);
        let last = fs::read_to_string(&marker).unwrap_or_default();
        if last.trim() == today {
            return;
        }
        let _ = fs::create_dir_all(&self.state_dir);
        if let Err(e) = fs::write(&marker, &today) {
            tracing::warn!("could not write {}: {e}", marker.display());
        }
        let carried = self.carried_items().map_or(0, |(_, i)| i.len());
        let path = plan::path(&self.config.tasks_dir, &today);
        self.fire_hook(
            HookEvent::DayStart,
            None,
            vec![
                ("PAHIRI_DATE".into(), today),
                ("PAHIRI_PLAN_FILE".into(), path.display().to_string()),
                ("PAHIRI_CARRIED".into(), carried.to_string()),
            ],
            AfterHook::Nothing,
        );
    }

    /// Mirror ticked checkpoints into the plan file's `[x]` marks.
    pub(super) fn sync_plan_marks(&mut self) {
        let mut changed = false;
        for i in 0..self.plan.len() {
            let s = self.item_status(&self.plan[i]);
            if s.checkpoint.is_some() && self.plan[i].done != s.done {
                self.plan[i].done = s.done;
                changed = true;
            }
        }
        if changed {
            self.write_plan();
        }
    }

    fn write_plan(&mut self) {
        let path = plan::path(&self.config.tasks_dir, &self.plan_date);
        if let Err(e) = plan::write(&path, &self.plan_date, &self.plan) {
            self.error(format!("could not write the plan: {e}"));
        }
        self.plan_mtime = self.plan_mtime();
    }

    // ----- the Plan view -----------------------------------------------------------

    /// `p`: open the Plan view for today.
    pub(super) fn open_plan(&mut self) {
        if self.store.is_none() {
            self.set_status("set up the tasks folder first");
            return;
        }
        self.refresh_next_up();
        self.load_plan();
        let view = self.build_plan_view(false, None);
        if view.dirty {
            self.set_status(format!(
                "{} unfinished item(s) carried over · Enter saves the plan",
                view.picked.len()
            ));
        }
        self.mode = Mode::Plan(view);
    }

    /// Board columns offered for planning, most active (rightmost) first.
    fn planning_columns(&self, all: bool) -> Vec<usize> {
        let Some(store) = &self.store else {
            return Vec::new();
        };
        let cols = &store.board().columns;
        let open: Vec<usize> = (0..cols.len().saturating_sub(1).max(1).min(cols.len())).collect();
        let chosen: Vec<usize> = if all {
            open
        } else if !self.config.planner.columns.is_empty() {
            open.into_iter()
                .filter(|&c| {
                    self.config
                        .planner
                        .columns
                        .iter()
                        .any(|n| n.eq_ignore_ascii_case(&cols[c].name))
                })
                .collect()
        } else if open.len() >= 2 {
            open[1..].to_vec()
        } else {
            open
        };
        chosen.into_iter().rev().collect()
    }

    fn build_plan_view(&self, all_columns: bool, picked: Option<Vec<PlanItem>>) -> PlanView {
        let mut rows = Vec::new();
        let mut groups = Vec::new();
        let mut statuses = Vec::new();
        let carried = self.carried_items();
        if let Some((day, items)) = &carried {
            if !items.is_empty() {
                rows.push(PlanRow::Section(format!(
                    "CARRIED OVER · {}",
                    plan::day_label(day)
                )));
                rows.extend(items.iter().cloned().map(PlanRow::Item));
            }
        }
        if let Some(store) = &self.store {
            for c in self.planning_columns(all_columns) {
                let col = &store.board().columns[c];
                if col.tasks.is_empty() {
                    continue;
                }
                rows.push(PlanRow::Section(col.name.to_uppercase()));
                for id in &col.tasks {
                    let Some(record) = self.record(id) else {
                        continue;
                    };
                    rows.push(PlanRow::Task(id.clone(), record.title.clone()));
                    let open: Vec<PlanItem> = record
                        .checkpoints
                        .iter()
                        .filter(|c| !c.done)
                        .map(|c| PlanItem {
                            task: Some(id.clone()),
                            title: c.title.clone(),
                            estimate_min: c.estimate_min,
                            done: false,
                        })
                        .collect();
                    if open.is_empty() {
                        rows.push(PlanRow::Hint(if record.checkpoints.is_empty() {
                            "no checkpoints · t adds an item · Esc b in the task plans them".into()
                        } else {
                            "all checkpoints done · t adds an item".into()
                        }));
                    }
                    rows.extend(open.iter().cloned().map(PlanRow::Item));
                    groups.push(open);
                }
            }
        }
        let carried_items = carried.map(|(_, items)| items).unwrap_or_default();
        let dirty = picked.is_none() && self.plan.is_empty() && !carried_items.is_empty();
        let picked = picked.unwrap_or_else(|| {
            if self.plan.is_empty() {
                carried_items.clone()
            } else {
                self.plan.clone()
            }
        });
        for item in rows
            .iter()
            .filter_map(|r| match r {
                PlanRow::Item(i) => Some(i),
                _ => None,
            })
            .chain(picked.iter())
        {
            statuses.push((item.clone(), self.item_status(item)));
        }
        let mut view = PlanView {
            date: self.plan_date.clone(),
            rows,
            picked,
            focus: PlanFocus::Pick,
            cursor: 0,
            order_cursor: 0,
            capacity_min: self.config.planner.day_minutes,
            factor: self.pace(),
            all_columns,
            dirty,
            carried: carried_items,
            groups,
            statuses,
        };
        view.cursor_to_first();
        view
    }

    /// Keys of the Plan view.
    pub(super) fn handle_plan_key(&mut self, key: KeyEvent) {
        let Mode::Plan(view) = &mut self.mode else {
            return;
        };
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char('q'), KeyModifiers::NONE) => {
                if view.dirty {
                    self.popup = Some(Popup::confirm(
                        "Discard the plan?",
                        "The plan has changes that are not saved (Enter saves).",
                        Pending::PlanDiscard,
                    ));
                } else {
                    self.mode = Mode::Home;
                }
            }
            (KeyCode::Enter, _) | (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                self.save_plan_view();
            }
            (KeyCode::Tab | KeyCode::BackTab, _)
            | (KeyCode::Left | KeyCode::Right, KeyModifiers::NONE) => {
                view.focus = match (key.code, view.focus) {
                    (KeyCode::Left, _) | (KeyCode::Tab | KeyCode::BackTab, PlanFocus::Order) => {
                        PlanFocus::Pick
                    }
                    _ => PlanFocus::Order,
                };
            }
            (KeyCode::Char('J'), _) | (KeyCode::Down, _)
                if view.focus == PlanFocus::Order && (shift || key.code == KeyCode::Char('J')) =>
            {
                view.shift_picked(1);
            }
            (KeyCode::Char('K'), _) | (KeyCode::Up, _)
                if view.focus == PlanFocus::Order && (shift || key.code == KeyCode::Char('K')) =>
            {
                view.shift_picked(-1);
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => view.move_cursor(1),
            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => view.move_cursor(-1),
            (KeyCode::Char(' '), _) => {
                if view.focus == PlanFocus::Pick {
                    view.toggle();
                } else {
                    view.remove_picked();
                }
            }
            (KeyCode::Char('x') | KeyCode::Delete | KeyCode::Char('d'), _)
                if view.focus == PlanFocus::Order =>
            {
                view.remove_picked();
            }
            (KeyCode::Char('s'), KeyModifiers::NONE) => {
                view.suggest();
                let (n, min, cap) = (view.picked.len(), view.planned_min(), view.capacity_min);
                self.set_status(format!(
                    "suggested {n} item(s): {} of {} · Enter saves, Esc discards",
                    fmt_minutes(min),
                    fmt_minutes(cap)
                ));
            }
            (KeyCode::Char('A'), _) => {
                let all = !view.all_columns;
                let picked = view.picked.clone();
                let dirty = view.dirty;
                let mut next = self.build_plan_view(all, Some(picked));
                next.dirty = dirty;
                self.mode = Mode::Plan(next);
                self.set_status(if all {
                    "showing every open column"
                } else {
                    "showing the columns you plan from (setting: Plan from columns)"
                });
            }
            (KeyCode::Char('a'), _) => {
                self.popup = Some(Popup::input(
                    "Add to the plan",
                    "what, and an estimate (e.g. Email the vendor 15m)",
                    "",
                    Pending::PlanAdd(None),
                ));
            }
            (KeyCode::Char('t'), _) => match view.task_at_cursor() {
                Some(id) => {
                    self.popup = Some(Popup::input(
                        format!("Add to the plan for {id}"),
                        "what, and an estimate (e.g. Reply to review 20m)",
                        "",
                        Pending::PlanAdd(Some(id)),
                    ));
                }
                None => self.set_status("put the cursor on a task first (a adds a free item)"),
            },
            (KeyCode::Char('?') | KeyCode::F(1), _) => {
                self.open_help(super::help::HelpTopic::Work);
            }
            _ => {}
        }
    }

    /// The input of `a` / `t`.
    pub(super) fn plan_add(&mut self, task: Option<String>, text: &str) {
        let Some(mut item) = PlanItem::free(text, DEFAULT_ITEM_MIN) else {
            return;
        };
        if let Some(c) = task
            .as_deref()
            .and_then(|t| self.record(t))
            .and_then(|r| r.checkpoints.iter().find(|c| c.title == item.title))
        {
            // A checkpoint of the task keeps its own estimate.
            item.estimate_min = c.estimate_min;
        }
        item.task = task;
        let status = self.item_status(&item);
        if let Mode::Plan(view) = &mut self.mode {
            view.statuses.push((item.clone(), status));
            view.add(item);
            view.focus = PlanFocus::Order;
            view.order_cursor = view.picked.len() - 1;
        }
    }

    /// Enter in the Plan view: write the plan and go back to Home.
    fn save_plan_view(&mut self) {
        let Mode::Plan(view) = &self.mode else {
            return;
        };
        let (date, items, minutes) = (view.date.clone(), view.picked.clone(), view.planned_min());
        self.plan_date.clone_from(&date);
        self.plan = items;
        self.write_plan();
        self.carry_hint = None;
        self.mode = Mode::Home;
        self.home_focus = if self.plan.is_empty() {
            HomeFocus::Board
        } else {
            HomeFocus::Today
        };
        self.today_selected = self
            .plan
            .iter()
            .position(|i| !self.item_status(i).done)
            .unwrap_or(0);
        self.set_status(format!(
            "plan saved: {} item(s), {} · Enter starts the timer on one",
            self.plan.len(),
            fmt_minutes(minutes)
        ));
        let path = plan::path(&self.config.tasks_dir, &date);
        self.fire_hook(
            HookEvent::PlanSave,
            None,
            vec![
                ("PAHIRI_DATE".into(), date),
                ("PAHIRI_PLAN_FILE".into(), path.display().to_string()),
                ("PAHIRI_PLAN_ITEMS".into(), self.plan.len().to_string()),
                ("PAHIRI_PLAN_MINUTES".into(), minutes.to_string()),
            ],
            AfterHook::Nothing,
        );
    }

    // ----- the Today pane ----------------------------------------------------------

    /// Keys while the Today pane has focus. Returns whether the key was used.
    pub(super) fn handle_today_key(&mut self, key: KeyEvent) -> bool {
        let n = self.plan.len();
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match (key.code, key.modifiers) {
            (KeyCode::Tab | KeyCode::BackTab | KeyCode::Left, _) | (KeyCode::Char('h'), _) => {
                self.home_focus = HomeFocus::Board;
            }
            (KeyCode::Char('J'), _) | (KeyCode::Down, _)
                if shift || key.code == KeyCode::Char('J') =>
            {
                self.shift_plan_item(1);
            }
            (KeyCode::Char('K'), _) | (KeyCode::Up, _)
                if shift || key.code == KeyCode::Char('K') =>
            {
                self.shift_plan_item(-1);
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                self.today_selected = (self.today_selected + 1).min(n.saturating_sub(1));
            }
            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                self.today_selected = self.today_selected.saturating_sub(1);
            }
            (KeyCode::Home, _) | (KeyCode::Char('g'), _) => self.today_selected = 0,
            (KeyCode::End, _) | (KeyCode::Char('G'), _) => {
                self.today_selected = n.saturating_sub(1);
            }
            (KeyCode::Enter, _) => self.start_plan_item(self.today_selected),
            (KeyCode::Char(' ') | KeyCode::Char('v'), KeyModifiers::NONE) => {
                self.toggle_plan_item(self.today_selected);
            }
            (KeyCode::Char('x') | KeyCode::Delete, _) => self.remove_plan_item(self.today_selected),
            (KeyCode::Char('o'), _) => {
                if let Some(id) = self
                    .plan
                    .get(self.today_selected)
                    .and_then(|i| i.task.clone())
                {
                    self.open_task_from_plan(&id);
                }
            }
            _ => return false,
        }
        true
    }

    fn open_task_from_plan(&mut self, id: &str) {
        if self.record(id).is_none() {
            self.set_status(format!("{id} is not on the board"));
            return;
        }
        self.select_task_row(id);
        self.enter_task(id);
    }

    fn shift_plan_item(&mut self, delta: i32) {
        let i = self.today_selected;
        let j = i as i64 + i64::from(delta);
        if i >= self.plan.len() || j < 0 || j >= self.plan.len() as i64 {
            return;
        }
        self.plan.swap(i, j as usize);
        self.today_selected = j as usize;
        self.write_plan();
    }

    fn remove_plan_item(&mut self, i: usize) {
        if i >= self.plan.len() {
            return;
        }
        let item = self.plan.remove(i);
        self.today_selected = self.today_selected.min(self.plan.len().saturating_sub(1));
        self.write_plan();
        self.set_status(format!(
            "removed from today: {} · p to plan again",
            item.title
        ));
    }

    /// Space in the Today pane: tick (or reopen) an item. Checkpoints are
    /// ticked in the task's `CONTEXT.md`.
    pub(super) fn toggle_plan_item(&mut self, i: usize) {
        let Some(item) = self.plan.get(i).cloned() else {
            return;
        };
        let status = self.item_status(&item);
        if status.timed && !status.done {
            // The timed item: book the time and move on, like the timer's "done → next".
            self.checkpoint_done();
            return;
        }
        if let (Some(id), Some(index)) = (&item.task, status.checkpoint) {
            let Some(path) = self.context_path(id) else {
                return;
            };
            let mut items = crate::tasks::checkpoints::read(&path).unwrap_or_default();
            self.toggle_checkpoint(id, &mut items, index);
            self.sync_plan_marks();
        } else {
            self.plan[i].done = !self.plan[i].done;
            self.write_plan();
        }
    }

    /// Enter in the Today pane: start the timer on an item.
    pub(super) fn start_plan_item(&mut self, i: usize) {
        let Some(item) = self.plan.get(i).cloned() else {
            return;
        };
        let status = self.item_status(&item);
        let Some(id) = item.task.clone() else {
            self.set_status("a free item has no task to book time on · Space ticks it");
            return;
        };
        if status.missing {
            self.set_status(format!("{id} is not on the board any more"));
            return;
        }
        if let Some(index) = status.checkpoint {
            self.start_timer(&id, Some(index));
        } else {
            let budget = std::time::Duration::from_secs(item.estimate_min.max(1) * 60);
            self.begin_timer(&id, Some((None, item.title.clone())), budget);
        }
    }

    /// The plan's next open item with a task after the one on (`task`, `title`),
    /// when that one is in the plan.
    pub(super) fn next_in_plan(&self, task: &str, title: &str) -> Option<usize> {
        let at = self
            .plan
            .iter()
            .position(|i| i.task.as_deref() == Some(task) && i.title == title)?;
        (at + 1..self.plan.len()).find(|&j| {
            let item = &self.plan[j];
            let s = self.item_status(item);
            item.task.is_some() && !s.done && !s.missing
        })
    }

    /// Label of plan item `i` for menus: `PROJ-42 · Write the parser (40m)`.
    pub(super) fn plan_item_label(&self, i: usize) -> String {
        let item = &self.plan[i];
        let s = self.item_status(item);
        let left = fmt_minutes(s.remaining_min(item));
        match &item.task {
            Some(t) => format!("{t} · {} ({left})", item.title),
            None => format!("{} ({left})", item.title),
        }
    }

    /// Mark a plan item that is not a checkpoint as done (after its timer).
    pub(super) fn mark_plan_item_done(&mut self, task: &str, title: &str) {
        let Some(i) = self
            .plan
            .iter()
            .position(|i| i.task.as_deref() == Some(task) && i.title == title)
        else {
            return;
        };
        if self.item_status(&self.plan[i]).checkpoint.is_none() && !self.plan[i].done {
            self.plan[i].done = true;
            self.write_plan();
        }
    }
}
