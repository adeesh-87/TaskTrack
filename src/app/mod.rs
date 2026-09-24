//! Application state and event handling.
//!
//! The [`App`] owns everything the UI shows and mutates it in response to
//! [`AppEvent`]s. Drawing lives in [`crate::ui`] and only reads this state
//! (plus lazily resizing shells to fit their pane).

pub mod config_form;
pub mod context;
pub mod event;
pub mod palette;
pub mod popup;
pub mod timer;
pub mod ui_state;

mod agent;
mod keymap;
mod mouse;
mod work;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tracing::{info, warn};

use crate::ai::PromptKind;
use crate::config::keybind::KeyCombo;
use crate::config::Config;
use crate::config::TaskSource;
use crate::editor::Buffer;
use crate::files::{ops, probe};
use crate::git::{self, PrepareRequest};
use crate::tasks::context::render_new;
use crate::tasks::record::TaskRecord;
use crate::tasks::{sources, TaskStore, Ticket};
use crate::terminal::{keys, PtyEvent, ShellId};

pub use self::config_form::ConfigForm;
pub use self::context::{Focus, Shell, TaskContext};
pub use self::event::{AppEvent, EventSender, JobEvent};
pub use self::palette::{Action, Palette};
pub use self::popup::{CheckItem, Choice, Pending, Popup};
pub use self::ui_state::UiState;
pub use crate::time::{format_rfc3339, now_rfc3339};

/// Which top-level screen is showing.
#[derive(Debug)]
pub enum Mode {
    /// The settings page.
    Config(ConfigForm),
    /// The task board with an empty right side.
    TaskList,
    /// Working inside one task.
    Task,
}

/// A flattened row of the task list: either a category header or a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListRow {
    /// Category header (index into the board columns).
    Header(usize),
    /// Task (column index, task id).
    Task(usize, String),
}

/// Deferred follow-up computed while a task context is borrowed.
#[derive(Debug)]
enum Next {
    None,
    Palette,
    OpenFile(PathBuf),
    Popup(Popup),
    Action(Action),
    Status(String),
    Error(String),
}

/// The whole application state.
pub struct App {
    config: Config,
    config_path: PathBuf,
    state_dir: PathBuf,
    store: Option<TaskStore>,
    mode: Mode,
    /// Index into [`App::rows`].
    list_selected: usize,
    contexts: HashMap<String, TaskContext>,
    active_task: Option<String>,
    popup: Option<Popup>,
    status: Option<String>,
    should_quit: bool,
    next_shell_id: ShellId,
    leader: KeyCombo,
    leader_pending: bool,
    events: EventSender,
    editor_height: usize,
    /// Whether leaving the settings page returns to the task view (else the list).
    config_returns_to_task: bool,
    /// Geometry of the last frame, for mouse hit-testing.
    pub ui: UiState,
    last_click: Option<(std::time::Instant, u16, u16)>,
    timer: Option<timer::Timer>,
    flash_start: Option<Instant>,
    bell: bool,
    /// Cancels the running agent job (Esc in its log popup).
    job_cancel: Option<Arc<AtomicBool>>,
    next_up: Vec<TaskRecord>,
}

impl App {
    /// Create the app. Opens the config page when no valid config exists.
    pub fn new(
        config: Option<Config>,
        config_path: PathBuf,
        state_dir: PathBuf,
        events: EventSender,
    ) -> Self {
        let (config, mode) = match config {
            Some(cfg) if cfg.validate().is_empty() => (cfg, Mode::TaskList),
            Some(cfg) => {
                let mut form = ConfigForm::new(&cfg, true);
                form.set_errors(cfg.validate());
                (cfg, Mode::Config(form))
            }
            None => {
                let cfg = Config::default();
                let form = ConfigForm::new(&cfg, true);
                (cfg, Mode::Config(form))
            }
        };
        let leader = config.leader();
        let mut app = Self {
            config,
            config_path,
            state_dir,
            store: None,
            mode,
            list_selected: 0,
            contexts: HashMap::new(),
            active_task: None,
            popup: None,
            status: None,
            should_quit: false,
            next_shell_id: 1,
            leader,
            leader_pending: false,
            events,
            editor_height: 20,
            config_returns_to_task: false,
            ui: UiState::default(),
            last_click: None,
            timer: None,
            flash_start: None,
            bell: false,
            job_cancel: None,
            next_up: Vec::new(),
        };
        if matches!(app.mode, Mode::TaskList) {
            app.open_store();
        }
        app
    }

    // ----- accessors used by the UI -------------------------------------------------

    /// Current configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Where the config file lives.
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    /// Current mode.
    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    /// The task store, when configured.
    pub fn store(&self) -> Option<&TaskStore> {
        self.store.as_ref()
    }

    /// Active popup.
    pub fn popup(&self) -> Option<&Popup> {
        self.popup.as_ref()
    }

    /// Transient status message.
    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// Whether the main loop should exit.
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// The leader (prefix) key.
    pub fn leader(&self) -> KeyCombo {
        self.leader
    }

    /// Whether the leader key was just pressed and we await the next key.
    pub fn leader_pending(&self) -> bool {
        self.leader_pending
    }

    /// Selected row in the task list.
    pub fn list_selected(&self) -> usize {
        self.list_selected
    }

    /// Id of the active task.
    pub fn active_task(&self) -> Option<&str> {
        self.active_task.as_deref()
    }

    /// The active task context.
    pub fn active_context(&self) -> Option<&TaskContext> {
        self.active_task
            .as_ref()
            .and_then(|id| self.contexts.get(id))
    }

    /// Mutable active task context.
    pub fn active_context_mut(&mut self) -> Option<&mut TaskContext> {
        let id = self.active_task.clone()?;
        self.contexts.get_mut(&id)
    }

    /// Flattened rows of the task list.
    pub fn rows(&self) -> Vec<ListRow> {
        let mut rows = Vec::new();
        if let Some(store) = &self.store {
            for (ci, col) in store.board().columns.iter().enumerate() {
                rows.push(ListRow::Header(ci));
                for t in &col.tasks {
                    rows.push(ListRow::Task(ci, t.clone()));
                }
            }
        }
        rows
    }

    /// Tell the app how tall the editor viewport is (for paging and scrolling).
    pub fn set_editor_height(&mut self, h: usize) {
        self.editor_height = h.max(1);
    }

    // ----- lifecycle ---------------------------------------------------------------

    fn open_store(&mut self) {
        match TaskStore::open(
            &self.config.tasks_dir,
            &self.config.status_file,
            &self.config.context_file,
            &self.config.categories,
        ) {
            Ok(store) => {
                self.store = Some(store);
                self.select_first_task();
                self.refresh_next_up();
            }
            Err(e) => {
                self.store = None;
                self.error(format!("could not open tasks folder: {e}"));
            }
        }
    }

    fn select_first_task(&mut self) {
        let rows = self.rows();
        self.list_selected = rows
            .iter()
            .position(|r| matches!(r, ListRow::Task(..)))
            .unwrap_or(0)
            .min(rows.len().saturating_sub(1));
    }

    fn select_task_row(&mut self, id: &str) {
        let rows = self.rows();
        if let Some(i) = rows
            .iter()
            .position(|r| matches!(r, ListRow::Task(_, t) if t == id))
        {
            self.list_selected = i;
        }
    }

    fn selected_task_id(&self) -> Option<String> {
        match self.rows().get(self.list_selected) {
            Some(ListRow::Task(_, id)) => Some(id.clone()),
            _ => None,
        }
    }

    /// Task the commands act on: the active one inside a task, else the selected row.
    fn current_task_id(&self) -> Option<String> {
        match self.mode {
            Mode::Task => self.active_task.clone(),
            _ => self.selected_task_id(),
        }
    }

    fn error(&mut self, message: String) {
        warn!("{message}");
        self.popup = Some(Popup::message("Error", message));
    }

    fn set_status(&mut self, message: impl Into<String>) {
        self.status = Some(message.into());
    }

    /// Handle one event.
    pub fn handle(&mut self, event: AppEvent) {
        match event {
            AppEvent::Input(Event::Key(key)) => self.handle_key(key),
            AppEvent::Input(Event::Paste(text)) => self.handle_paste(&text),
            AppEvent::Input(Event::Mouse(m)) => self.handle_mouse(m),
            AppEvent::Input(Event::Resize(..) | Event::FocusGained | Event::FocusLost) => {}
            AppEvent::Tick => self.on_tick(),
            AppEvent::Pty(PtyEvent::Output { id, data }) => {
                if let Some(shell) = self.shell_mut(id) {
                    shell.session.process(&data);
                }
            }
            AppEvent::Pty(PtyEvent::Exited { id }) => {
                if let Some(shell) = self.shell_mut(id) {
                    shell.session.mark_exited();
                    info!(id, "shell exited");
                }
            }
            AppEvent::Job(job) => self.handle_job(job),
        }
    }

    fn shell_mut(&mut self, id: ShellId) -> Option<&mut Shell> {
        self.contexts.values_mut().find_map(|c| c.shell_mut(id))
    }

    fn handle_paste(&mut self, text: &str) {
        match &mut self.popup {
            Some(Popup::Input { value, .. }) => {
                value.push_str(text.lines().next().unwrap_or(""));
                return;
            }
            Some(Popup::Tickets {
                filter, selected, ..
            }) => {
                filter.push_str(text.lines().next().unwrap_or(""));
                *selected = 0;
                return;
            }
            Some(_) => return,
            None => {}
        }
        if let Mode::Config(form) = &mut self.mode {
            form.paste(text);
            return;
        }
        if !matches!(self.mode, Mode::Task) {
            return;
        }
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        match ctx.focus {
            Focus::Terminal => {
                if let Some(shell) = ctx.active_shell_mut() {
                    let bracketed = shell.session.screen().bracketed_paste();
                    let _ = shell.session.write(&keys::encode_paste(text, bracketed));
                }
            }
            Focus::Editor => {
                if let Some(ed) = &mut ctx.editor {
                    ed.insert_str(text);
                }
            }
            Focus::Tree | Focus::Shells => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        self.status = None;
        if self.popup.is_some() {
            self.handle_popup_key(key);
            return;
        }
        match self.mode {
            Mode::Config(_) => self.handle_config_key(key),
            Mode::TaskList => self.handle_list_key(key),
            Mode::Task => self.handle_task_key(key),
        }
    }

    // ----- config page -------------------------------------------------------------

    fn handle_config_key(&mut self, key: KeyEvent) {
        let (editing, first_run) = match &self.mode {
            Mode::Config(form) => (form.editing(), form.first_run()),
            _ => return,
        };
        if editing {
            if let Mode::Config(form) = &mut self.mode {
                form.handle_edit_key(key);
            }
            return;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char('s'), KeyModifiers::CONTROL) | (KeyCode::F(2), _) => self.save_config(),
            (KeyCode::Esc, _) | (KeyCode::Char('q'), KeyModifiers::NONE) => {
                if first_run && !self.config.validate().is_empty() {
                    self.set_status("Set the tasks folder and press Ctrl+S to save first.");
                } else {
                    self.leave_config();
                }
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => self.should_quit = true,
            (KeyCode::Char('?'), _) | (KeyCode::F(1), _) => self.config_help(),
            (KeyCode::Char('t'), KeyModifiers::NONE) if self.test_selected_source() => {}
            _ => {
                if let Mode::Config(form) = &mut self.mode {
                    form.handle_nav_key(key);
                }
            }
        }
    }

    fn config_help(&mut self) {
        use config_form::FieldKey as K;
        let Mode::Config(form) = &self.mode else {
            return;
        };
        let field = &form.fields()[form.selected_field()];
        let (title, text) = match field.key {
            K::TaskSources => (
                "Task sources · script format",
                keymap::TASK_SOURCE_HELP.to_owned(),
            ),
            K::AgentCommand
            | K::AgentArgs
            | K::AgentTimeout
            | K::ContextWords
            | K::ContextPrompt
            | K::CheckpointPrompt
            | K::CodingCommand
            | K::CodingArgs
            | K::CodingPrompt
            | K::CodingStartInCode => ("AI agents", keymap::AGENT_HELP.to_owned()),
            _ => (field.label, field.help.clone()),
        };
        self.popup = Some(Popup::doc(title, &text));
    }

    /// `t` on a task-source item: run it now and show what was parsed.
    fn test_selected_source(&mut self) -> bool {
        let Mode::Config(form) = &self.mode else {
            return false;
        };
        let Some(item) = form.selected_item(config_form::FieldKey::TaskSources) else {
            return false;
        };
        match config_form::parse_source(&item) {
            Ok(src) => self.run_source(src, true),
            Err(e) => self.error(e),
        }
        true
    }

    fn save_config(&mut self) {
        let Mode::Config(form) = &mut self.mode else {
            return;
        };
        let (candidate, mut errors) = form.to_config(&self.config);
        errors.extend(candidate.validate());
        if !errors.is_empty() {
            form.set_errors(errors);
            return;
        }
        if let Err(e) = candidate.save(&self.config_path) {
            form.set_errors(vec![e.to_string()]);
            return;
        }
        form.set_errors(Vec::new());
        form.mark_saved();
        let reopen = candidate.categories != self.config.categories
            || candidate.tasks_dir != self.config.tasks_dir
            || candidate.status_file != self.config.status_file
            || candidate.context_file != self.config.context_file;
        self.config = candidate;
        self.leader = self.config.leader();
        if reopen || self.store.is_none() {
            self.contexts.clear();
            self.active_task = None;
            self.open_store();
        } else {
            // Attachments may now resolve to different paths.
            let cfg = self.config.clone();
            let state = self.state_dir.clone();
            for ctx in self.contexts.values_mut() {
                let _ = ctx.refresh_env(&cfg, &state);
            }
        }
        self.set_status(format!("Saved {}", self.config_path.display()));
    }

    fn leave_config(&mut self) {
        let dirty = matches!(&self.mode, Mode::Config(form) if form.dirty());
        if dirty {
            self.popup = Some(Popup::confirm(
                "Discard changes?",
                "The settings page has unsaved changes.",
                Pending::LeaveConfigDiscard,
            ));
            return;
        }
        self.return_from_config();
    }

    fn return_from_config(&mut self) {
        self.mode = if self.config_returns_to_task && self.active_task.is_some() {
            Mode::Task
        } else {
            Mode::TaskList
        };
        if self.store.is_none() {
            self.open_store();
        }
    }

    fn open_config(&mut self) {
        self.config_returns_to_task = matches!(self.mode, Mode::Task);
        self.mode = Mode::Config(ConfigForm::new(&self.config, false));
    }

    // ----- task list ---------------------------------------------------------------

    fn handle_list_key(&mut self, key: KeyEvent) {
        let rows = self.rows();
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => self.open_palette(),
            (KeyCode::Char('q'), KeyModifiers::NONE)
            | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.request_quit();
            }
            (KeyCode::Char('?'), _) => self.run_action(Action::Help),
            (KeyCode::Char(','), _) => self.open_config(),
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => self.move_list(1, &rows),
            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => self.move_list(-1, &rows),
            (KeyCode::Home, _) | (KeyCode::Char('g'), _) => {
                self.list_selected = 0;
                self.move_list(1, &rows);
                self.move_list(-1, &rows);
            }
            (KeyCode::End, _) | (KeyCode::Char('G'), _) => {
                self.list_selected = rows.len().saturating_sub(1);
                self.move_list(-1, &rows);
                self.move_list(1, &rows);
            }
            (KeyCode::Enter, _) | (KeyCode::Char('l'), _) | (KeyCode::Right, _) => {
                if let Some(id) = self.selected_task_id() {
                    self.enter_task(&id);
                }
            }
            (KeyCode::Char(']'), _) | (KeyCode::Char('>'), _) => self.run_action(Action::MoveNext),
            (KeyCode::Char('['), _) | (KeyCode::Char('<'), _) => self.run_action(Action::MovePrev),
            (KeyCode::Char('n'), _) => self.run_action(Action::NewTask),
            (KeyCode::Char('r'), _) | (KeyCode::F(5), _) => self.refresh_store(),
            (KeyCode::Char('d'), KeyModifiers::NONE) | (KeyCode::Delete, _) => {
                self.run_action(Action::DeleteTask);
            }
            (KeyCode::Char('D'), _) => self.run_action(Action::DeleteTask),
            (KeyCode::Char('m'), KeyModifiers::NONE) => self.run_action(Action::Timer),
            (KeyCode::Char('M'), _) => self.run_action(Action::StopTimer),
            (KeyCode::Char('v'), KeyModifiers::NONE) => self.run_action(Action::CheckpointDone),
            (KeyCode::Char('K'), _) => self.run_action(Action::Checkpoints),
            _ => {}
        }
    }

    fn move_list(&mut self, delta: i32, rows: &[ListRow]) {
        if rows.is_empty() {
            self.list_selected = 0;
            return;
        }
        let mut idx = self.list_selected as i64;
        let len = rows.len() as i64;
        loop {
            idx += i64::from(delta);
            if idx < 0 || idx >= len {
                return;
            }
            if matches!(rows[idx as usize], ListRow::Task(..)) {
                self.list_selected = idx as usize;
                return;
            }
        }
    }

    fn move_task(&mut self, delta: i32) {
        let Some(id) = self.current_task_id() else {
            return;
        };
        let Some(store) = &mut self.store else { return };
        let Some((col, _)) = store.board().locate(&id) else {
            return;
        };
        let target = col as i64 + i64::from(delta);
        if target < 0 || target >= store.categories().len() as i64 {
            return;
        }
        match store.move_task(&id, target as usize) {
            Ok(true) => {
                let name = self
                    .store
                    .as_ref()
                    .map(|s| s.categories()[target as usize].clone());
                self.select_task_row(&id);
                self.set_status(format!("Moved {id} to {}", name.unwrap_or_default()));
                self.on_task_moved(&id, col, target as usize);
            }
            Ok(false) => {}
            Err(e) => self.error(e.to_string()),
        }
    }

    fn refresh_store(&mut self) {
        if let Some(store) = &mut self.store {
            match store.refresh() {
                Ok(changed) => {
                    if changed {
                        if let Err(e) = store.save() {
                            self.error(e.to_string());
                            return;
                        }
                    }
                    self.set_status("Refreshed");
                }
                Err(e) => self.error(e.to_string()),
            }
        }
        self.fix_list_selection();
        self.refresh_next_up();
    }

    fn selected_column(&self) -> usize {
        match self.rows().get(self.list_selected) {
            Some(ListRow::Task(c, _) | ListRow::Header(c)) => *c,
            None => 0,
        }
    }

    fn create_task(&mut self, id: &str, context: Option<String>) {
        let column = self.selected_column();
        let Some(store) = &mut self.store else { return };
        let result = match context {
            Some(text) => store.create_task_with_context(id.trim(), column, &text),
            None => store.create_task(id.trim(), column),
        };
        match result {
            Ok(summary) => {
                self.select_task_row(&summary.id);
                self.refresh_next_up();
                self.set_status(format!("Created {}", summary.id));
                if matches!(self.mode, Mode::Task) {
                    self.enter_task(&summary.id);
                }
            }
            Err(e) => self.error(e.to_string()),
        }
    }

    fn start_new_task(&mut self) {
        if self.store.is_none() {
            self.error("configure a tasks folder first".into());
            return;
        }
        if self.config.task_sources.is_empty() {
            self.popup = Some(Popup::input(
                "New task",
                "task id (no spaces)",
                "",
                Pending::CreateTask,
            ));
            return;
        }
        let mut choices = vec![Choice {
            label: "custom task (type an id)".into(),
            pending: Pending::CreateTask,
        }];
        for s in &self.config.task_sources {
            choices.push(Choice {
                label: format!("from {}  ({})", s.name, s.command),
                pending: Pending::FetchTickets(s.name.clone()),
            });
        }
        self.popup = Some(Popup::choose("New task", choices));
    }

    fn fetch_tickets(&mut self, source: &str) {
        let Some(src) = self
            .config
            .task_sources
            .iter()
            .find(|s| s.name == source)
            .cloned()
        else {
            self.error(format!("unknown task source {source}"));
            return;
        };
        self.run_source(src, false);
    }

    /// Run a task-source script in the background; `test` only shows the result.
    fn run_source(&mut self, src: TaskSource, test: bool) {
        let tasks_dir = self.config.tasks_dir.clone();
        let events = self.events.clone();
        let name = src.name.clone();
        self.popup = Some(Popup::log(format!("Fetching tickets from {}", src.name)));
        if let Some(Popup::Log { lines, .. }) = &mut self.popup {
            lines.push(format!("$ {}", src.command));
        }
        let spawned = std::thread::Builder::new()
            .name(format!("source-{name}"))
            .spawn(move || {
                let result = sources::fetch(&src.command, Some(&tasks_dir));
                events.send(AppEvent::Job(JobEvent::Tickets {
                    source: name,
                    result,
                    test,
                }));
            });
        if let Err(e) = spawned {
            self.error(format!("could not start task source: {e}"));
        }
    }

    fn create_from_ticket(&mut self, source: &str, ticket: &Ticket) {
        let id = ticket.task_id();
        let context = render_new(&id, Some(source), Some(ticket), &now_rfc3339());
        self.create_task(&id, Some(context));
    }

    fn enter_task(&mut self, id: &str) {
        let Some(store) = &self.store else { return };
        let summary = store.summary(id);
        let context_path = store.context_path(id);
        if !self.contexts.contains_key(id) {
            match TaskContext::new(&summary, &context_path, self.config.show_hidden) {
                Ok(ctx) => {
                    self.contexts.insert(id.to_owned(), ctx);
                }
                Err(e) => {
                    self.error(format!("could not open task {id}: {e}"));
                    return;
                }
            }
        }
        self.active_task = Some(id.to_owned());
        self.mode = Mode::Task;
        let cfg = self.config.clone();
        let state = self.state_dir.clone();
        if let Some(ctx) = self.active_context_mut() {
            let _ = ctx.tree.refresh();
            let _ = ctx.reload_meta();
            let _ = ctx.refresh_env(&cfg, &state);
        }
    }

    fn back_to_list(&mut self) {
        self.mode = Mode::TaskList;
        if let Some(id) = self.active_task.clone() {
            self.select_task_row(&id);
        }
        self.refresh_next_up();
    }

    fn request_quit(&mut self) {
        let live: usize = self.contexts.values().map(TaskContext::live_shells).sum();
        let dirty = self
            .contexts
            .values()
            .any(|c| c.editor.as_ref().is_some_and(Buffer::is_dirty));
        if live > 0 || dirty {
            let mut parts = Vec::new();
            if live > 0 {
                parts.push(format!("{live} running shell(s) will be terminated"));
            }
            if dirty {
                parts.push("unsaved editor changes will be lost".to_owned());
            }
            self.popup = Some(Popup::confirm(
                "Quit pahiri?",
                parts.join("; "),
                Pending::Quit,
            ));
        } else {
            self.should_quit = true;
        }
    }

    // ----- palette and actions -----------------------------------------------------

    fn open_palette(&mut self) {
        let commands = match self.mode {
            Mode::Task => palette::task_commands(),
            _ => palette::list_commands(),
        };
        self.popup = Some(Popup::Palette(Palette::new(commands)));
    }

    fn run_action(&mut self, action: Action) {
        match action {
            Action::TaskList => self.back_to_list(),
            Action::Config => self.open_config(),
            Action::Quit => self.request_quit(),
            Action::NewTask => self.start_new_task(),
            Action::Refresh => {
                self.refresh_store();
                if let Some(ctx) = self.active_context_mut() {
                    let _ = ctx.tree.refresh();
                    let _ = ctx.reload_meta();
                }
            }
            Action::Attach => self.start_attach(),
            Action::Prepare => self.start_prepare(),
            Action::NewShell => self.new_shell(),
            Action::CloseShell => self.close_shell(),
            Action::OpenContext => {
                if let Some(path) = self.active_context().map(|c| c.context_path.clone()) {
                    self.request_open_file(&path);
                }
            }
            Action::FocusFiles => self.focus(Focus::Tree),
            Action::FocusEditor => self.focus(Focus::Editor),
            Action::FocusShells => self.focus(Focus::Shells),
            Action::FocusTerminal => self.focus(Focus::Terminal),
            Action::Zoom => {
                if let Some(ctx) = self.active_context_mut() {
                    if ctx.shell_pane_visible() {
                        ctx.focus = Focus::Terminal;
                        ctx.zoomed = !ctx.zoomed;
                    }
                }
            }
            Action::MoveNext => self.move_task(1),
            Action::MovePrev => self.move_task(-1),
            Action::ToggleHidden => {
                if let Some(ctx) = self.active_context_mut() {
                    if let Err(e) = ctx.tree.toggle_hidden() {
                        self.error(e.to_string());
                    }
                }
            }
            Action::Save => self.save_editor(),
            Action::CloseEditor => self.close_editor(),
            Action::Help => self.popup = Some(Popup::doc("Keys", keymap::HELP)),
            Action::DeleteTask => self.request_delete_task(),
            Action::Timer => self.toggle_timer(),
            Action::StopTimer => self.stop_timer(),
            Action::CheckpointDone => self.checkpoint_done(),
            Action::Checkpoints => self.show_checkpoints(),
            Action::Gerrit => self.find_gerrit(),
            Action::GenerateContext => self.run_agent(PromptKind::Context),
            Action::ToggleContextReady => self.toggle_context_ready(),
            Action::BreakDown => self.run_agent(PromptKind::Checkpoints),
            Action::CodingAgent => self.launch_coding_agent(),
            Action::EditPrompts => self.choose_prompt(),
        }
    }

    fn focus(&mut self, focus: Focus) {
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        if focus == Focus::Terminal && !ctx.shell_pane_visible() {
            if ctx.shells.is_empty() {
                self.set_status("no shells yet: Esc, s opens one");
            } else {
                ctx.show_shell = true;
                ctx.focus = Focus::Terminal;
            }
            return;
        }
        if focus == Focus::Editor && ctx.editor.is_none() {
            self.set_status("no document open: Enter on a file opens it");
            return;
        }
        ctx.set_focus(focus);
    }

    // ----- attach / prepare ----------------------------------------------------------

    fn start_attach(&mut self) {
        let Some(ctx) = self.active_context() else {
            self.set_status("open a task first");
            return;
        };
        if self.config.workspaces.is_empty() && self.config.builds.is_empty() {
            self.error(
                "no code workspaces or vendor builds configured yet (Esc, c to add them)".into(),
            );
            return;
        }
        let mut items = Vec::new();
        for w in &self.config.workspaces {
            items.push(CheckItem {
                label: format!(
                    "code   {:<14} {}  @{}",
                    w.name,
                    w.path.display(),
                    self.config.main_branch_for(w)
                ),
                key: format!("workspace:{}", w.name),
                checked: ctx.meta.workspaces.contains(&w.name),
            });
        }
        for b in &self.config.builds {
            items.push(CheckItem {
                label: format!("build  {:<14} {}", b.name, b.path.display()),
                key: format!("build:{}", b.name),
                checked: ctx.meta.builds.contains(&b.name),
            });
        }
        self.popup = Some(Popup::MultiSelect {
            title: format!("Attach to {}", ctx.id),
            items,
            selected: 0,
            pending: Pending::ApplyAttachments,
        });
    }

    fn apply_attachments(&mut self, keys: &[String]) {
        let cfg = self.config.clone();
        let state = self.state_dir.clone();
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        ctx.meta.workspaces = keys
            .iter()
            .filter_map(|k| k.strip_prefix("workspace:"))
            .map(str::to_owned)
            .collect();
        ctx.meta.builds = keys
            .iter()
            .filter_map(|k| k.strip_prefix("build:"))
            .map(str::to_owned)
            .collect();
        let summary = ctx.attachment_summary();
        if let Err(e) = ctx.save_meta() {
            self.error(format!("could not update CONTEXT.md: {e}"));
            return;
        }
        let _ = ctx.refresh_env(&cfg, &state);
        let _ = ctx.tree.refresh();
        self.reload_editor_if_context();
        self.set_status(if summary.is_empty() {
            "attachments cleared".to_owned()
        } else {
            format!("attached · {summary}")
        });
    }

    /// If the editor shows CONTEXT.md and is clean, reload it so managed edits show.
    fn reload_editor_if_context(&mut self) {
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        let path = ctx.context_path.clone();
        if let Some(ed) = &ctx.editor {
            if ed.path() == path && !ed.is_dirty() {
                if let Ok(buf) = Buffer::open(&path) {
                    ctx.editor = Some(buf);
                }
            }
        }
    }

    fn prepare_requests(&self, commit_on_main: bool) -> Option<Vec<PrepareRequest>> {
        let ctx = self.active_context()?;
        Some(
            ctx.meta
                .workspaces
                .iter()
                .filter_map(|name| self.config.workspace(name))
                .map(|w| PrepareRequest {
                    name: w.name.clone(),
                    path: w.path.clone(),
                    main_branch: self.config.main_branch_for(w),
                    task_id: ctx.id.clone(),
                    commit_on_main,
                })
                .collect(),
        )
    }

    fn start_prepare(&mut self) {
        let Some(ctx) = self.active_context() else {
            self.set_status("open a task first");
            return;
        };
        if ctx.meta.workspaces.is_empty() {
            self.error("no code workspace attached to this task yet (Esc, a to attach)".into());
            return;
        }
        let Some(requests) = self.prepare_requests(false) else {
            return;
        };
        let mut dirty_main = Vec::new();
        for r in &requests {
            match git::inspect(&r.path) {
                Ok(state) if state.dirty && state.branch == r.main_branch => {
                    dirty_main.push(format!("{} ({})", r.name, r.main_branch));
                }
                Ok(_) => {}
                Err(e) => {
                    self.error(format!("cannot inspect workspace {}: {e}", r.name));
                    return;
                }
            }
        }
        if dirty_main.is_empty() {
            self.run_prepare(false);
            return;
        }
        self.popup = Some(Popup::choose(
            "WARNING: not on a task branch",
            vec![
                Choice {
                    label: format!(
                        "{} on the main branch with uncommitted changes — commit there and continue",
                        dirty_main.join(", ")
                    ),
                    pending: Pending::RunPrepare { commit_on_main: true },
                },
                Choice {
                    label: "skip those workspaces, prepare the rest".into(),
                    pending: Pending::RunPrepare { commit_on_main: false },
                },
                Choice { label: "cancel".into(), pending: Pending::Nothing },
            ],
        ));
    }

    fn run_prepare(&mut self, commit_on_main: bool) {
        let Some(requests) = self.prepare_requests(commit_on_main) else {
            return;
        };
        let Some(task_id) = self.active_task.clone() else {
            return;
        };
        let events = self.events.clone();
        self.popup = Some(Popup::log(format!("Preparing workspaces for {task_id}")));
        let spawned = std::thread::Builder::new()
            .name("prepare".into())
            .spawn(move || {
                let mut succeeded = Vec::new();
                let mut failed = Vec::new();
                for req in &requests {
                    let ev = events.clone();
                    let mut log = |line: String| ev.send(AppEvent::Job(JobEvent::Log(line)));
                    match git::prepare(req, &mut log) {
                        Ok(()) => succeeded.push(req.name.clone()),
                        Err(e) => {
                            events.send(AppEvent::Job(JobEvent::Log(format!(
                                "[{}] FAILED: {e}",
                                req.name
                            ))));
                            failed.push((req.name.clone(), e.to_string()));
                        }
                    }
                }
                events.send(AppEvent::Job(JobEvent::PrepareDone {
                    task_id,
                    succeeded,
                    failed,
                }));
            });
        if let Err(e) = spawned {
            self.error(format!("could not start prepare: {e}"));
        }
    }

    fn handle_job(&mut self, job: JobEvent) {
        match job {
            JobEvent::Log(line) => {
                if let Some(Popup::Log { lines, .. }) = &mut self.popup {
                    lines.push(line);
                }
            }
            JobEvent::Tickets {
                source,
                result,
                test: true,
            } => {
                self.popup = Some(match result {
                    Ok(tickets) => {
                        let mut text = format!(
                            "{} ticket(s) parsed — test run, nothing was created.\n\n",
                            tickets.len()
                        );
                        for t in &tickets {
                            let _ = writeln!(text, "{:<14} {}", t.task_id(), t.title);
                            if !t.url.is_empty() {
                                let _ = writeln!(text, "{:<14} {}", "", t.url);
                            }
                        }
                        Popup::doc(format!("{source} · test"), &text)
                    }
                    Err(e) => Popup::doc(
                        format!("{source} · test failed"),
                        &format!("{e}\n\n{}", keymap::TASK_SOURCE_HELP),
                    ),
                });
            }
            JobEvent::Gerrit {
                task_id,
                found,
                scanned,
            } => self.handle_gerrit(&task_id, found, scanned),
            JobEvent::Agent {
                task_id,
                kind,
                result,
            } => self.handle_agent_result(&task_id, kind, result),
            JobEvent::Tickets { source, result, .. } => match result {
                Ok(tickets) if tickets.is_empty() => {
                    self.popup = Some(Popup::message(source, "the script returned no tickets"));
                }
                Ok(tickets) => {
                    self.popup = Some(Popup::Tickets {
                        source,
                        tickets,
                        filter: String::new(),
                        selected: 0,
                    });
                }
                Err(e) => self.popup = Some(Popup::message(format!("{source} failed"), e)),
            },
            JobEvent::PrepareDone {
                task_id,
                succeeded,
                failed,
            } => {
                if let Some(Popup::Log { lines, done, .. }) = &mut self.popup {
                    lines.push(String::new());
                    lines.push(format!(
                        "done: {} switched to '{task_id}', {} failed",
                        succeeded.len(),
                        failed.len()
                    ));
                    *done = true;
                }
                if !succeeded.is_empty() {
                    let cfg = self.config.clone();
                    let state = self.state_dir.clone();
                    if let Some(ctx) = self.contexts.get_mut(&task_id) {
                        ctx.meta.branch = Some(task_id.clone());
                        ctx.meta.prepared = Some(now_rfc3339());
                        if let Err(e) = ctx.save_meta() {
                            warn!("could not update CONTEXT.md: {e}");
                        }
                        let _ = ctx.refresh_env(&cfg, &state);
                        let _ = ctx.tree.refresh();
                    }
                    self.reload_editor_if_context();
                }
            }
        }
    }

    // ----- task view ---------------------------------------------------------------

    fn handle_task_key(&mut self, key: KeyEvent) {
        let Some(focus) = self.active_context().map(|c| c.focus) else {
            self.mode = Mode::TaskList;
            return;
        };
        // Pane navigation works from every pane, including the terminal.
        if !self.leader_pending {
            if let Some(handled) = self.handle_pane_keys(key) {
                if handled {
                    return;
                }
            }
        }
        if focus == Focus::Terminal {
            self.handle_terminal_key(key);
            return;
        }
        if self.leader_pending {
            self.leader_pending = false;
            self.handle_leader_command(key);
            return;
        }
        if self.leader.matches(&key) {
            self.leader_pending = true;
            return;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.open_palette();
                return;
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.request_quit();
                return;
            }
            (KeyCode::Char('?'), _) if focus != Focus::Editor => {
                self.run_action(Action::Help);
                return;
            }
            (KeyCode::Tab, m) if m.is_empty() => {
                if let Some(ctx) = self.active_context_mut() {
                    ctx.cycle_focus(true);
                }
                return;
            }
            (KeyCode::BackTab, _) => {
                if let Some(ctx) = self.active_context_mut() {
                    ctx.cycle_focus(false);
                }
                return;
            }
            _ => {}
        }
        match focus {
            Focus::Tree => self.handle_tree_key(key),
            Focus::Shells => self.handle_shell_list_key(key),
            Focus::Editor => self.handle_editor_key(key),
            Focus::Terminal => {}
        }
    }

    /// Ctrl+Tab / Ctrl+Shift+Tab (kitty protocol) and Alt+] / Alt+[ cycle panes;
    /// Ctrl+N / Alt+N focus shell N. Returns `Some(true)` when consumed.
    fn handle_pane_keys(&mut self, key: KeyEvent) -> Option<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let ctx = self.active_context_mut()?;
        match key.code {
            KeyCode::Tab if ctrl => {
                ctx.cycle_focus(!shift);
                Some(true)
            }
            KeyCode::BackTab if ctrl => {
                ctx.cycle_focus(false);
                Some(true)
            }
            KeyCode::Char(']') if alt => {
                ctx.cycle_focus(true);
                Some(true)
            }
            KeyCode::Char('[') if alt => {
                ctx.cycle_focus(false);
                Some(true)
            }
            KeyCode::Char(c @ '1'..='9') if ctrl || alt => {
                let n = c as usize - '1' as usize;
                if !ctx.focus_shell(n) {
                    self.set_status(format!("no shell #{}", n + 1));
                }
                Some(true)
            }
            _ => Some(false),
        }
    }

    fn handle_tree_key(&mut self, key: KeyEvent) {
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        let root = ctx.tree.root().to_path_buf();
        let mut next = Next::None;
        let result: std::io::Result<()> = match (key.code, key.modifiers) {
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                ctx.tree.select_next();
                Ok(())
            }
            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                ctx.tree.select_prev();
                Ok(())
            }
            (KeyCode::Home, _) | (KeyCode::Char('g'), _) => {
                ctx.tree.select_first();
                Ok(())
            }
            (KeyCode::End, _) | (KeyCode::Char('G'), _) => {
                ctx.tree.select_last();
                Ok(())
            }
            (KeyCode::Right, _) | (KeyCode::Char('l'), _) => ctx.tree.expand_selected(),
            (KeyCode::Left, _) | (KeyCode::Char('h'), _) => ctx.tree.collapse_selected(),
            (KeyCode::Enter, _) => match ctx.tree.toggle_selected() {
                Ok(true) => Ok(()),
                Ok(false) => {
                    if let Some(node) = ctx.tree.selected() {
                        next = Next::OpenFile(node.path.clone());
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            },
            (KeyCode::Char('.'), _) => ctx.tree.toggle_hidden(),
            (KeyCode::Char('r'), KeyModifiers::NONE) | (KeyCode::F(2), _) => {
                if let Some(node) = ctx.tree.selected() {
                    next = Next::Popup(Popup::input(
                        "Rename",
                        "new name",
                        node.name.clone(),
                        Pending::Rename(node.path.clone()),
                    ));
                }
                Ok(())
            }
            (KeyCode::Char('a'), _) | (KeyCode::Char('n'), KeyModifiers::NONE) => {
                let dir = ctx.tree.target_dir();
                next = Next::Popup(Popup::input(
                    "New file",
                    format!("name (in {})", short_path(&dir, &root)),
                    "",
                    Pending::CreateFile(dir),
                ));
                Ok(())
            }
            (KeyCode::Char('A'), _) | (KeyCode::Char('N'), _) => {
                let dir = ctx.tree.target_dir();
                next = Next::Popup(Popup::input(
                    "New folder",
                    format!("name (in {})", short_path(&dir, &root)),
                    "",
                    Pending::CreateDir(dir),
                ));
                Ok(())
            }
            (KeyCode::Char('d'), KeyModifiers::NONE) | (KeyCode::Delete, _) => {
                if let Some(node) = ctx.tree.selected() {
                    let what = if node.is_dir {
                        "folder and everything in it"
                    } else {
                        "file"
                    };
                    next = Next::Popup(Popup::confirm(
                        "Delete?",
                        format!("Delete {what}: {}", short_path(&node.path, &root)),
                        Pending::Delete(node.path.clone()),
                    ));
                }
                Ok(())
            }
            (KeyCode::F(5), _) | (KeyCode::Char('R'), _) => ctx.tree.refresh(),
            (KeyCode::Char('s'), KeyModifiers::NONE) => {
                ctx.focus = Focus::Shells;
                Ok(())
            }
            (KeyCode::Char('t'), KeyModifiers::NONE)
            | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                next = Next::Action(Action::NewShell);
                Ok(())
            }
            (KeyCode::Char('e'), KeyModifiers::NONE) => {
                if ctx.editor.is_some() {
                    ctx.focus = Focus::Editor;
                }
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(e) = result {
            self.error(e.to_string());
            return;
        }
        self.apply_next(next);
    }

    fn apply_next(&mut self, next: Next) {
        match next {
            Next::None => {}
            Next::Palette => self.open_palette(),
            Next::OpenFile(path) => self.request_open_file(&path),
            Next::Popup(p) => self.popup = Some(p),
            Next::Action(a) => self.run_action(a),
            Next::Status(s) => self.set_status(s),
            Next::Error(s) => self.error(s),
        }
    }

    fn request_open_file(&mut self, path: &Path) {
        if let Some(ed) = self.active_context().and_then(|c| c.editor.as_ref()) {
            if ed.path() == path {
                if let Some(ctx) = self.active_context_mut() {
                    ctx.focus = Focus::Editor;
                }
                return;
            }
            if ed.is_dirty() {
                self.popup = Some(Popup::confirm(
                    "Discard changes?",
                    format!("{} has unsaved changes.", ed.path().display()),
                    Pending::OpenDiscard(path.to_path_buf()),
                ));
                return;
            }
        }
        self.probe_and_open(path);
    }

    fn probe_and_open(&mut self, path: &Path) {
        match probe(path) {
            Ok(info) if info.needs_warning(self.config.large_file_kb) => {
                let kind = if info.is_binary { "binary" } else { "large" };
                self.popup = Some(Popup::confirm(
                    "WARNING",
                    format!(
                        "{} is a {kind} file ({}). Binary files open read-only as a hex dump. Open anyway?",
                        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                        human_size(info.size)
                    ),
                    Pending::OpenConfirmed(path.to_path_buf()),
                ));
            }
            Ok(_) => self.open_file(path),
            Err(e) => self.error(format!("cannot read {}: {e}", path.display())),
        }
    }

    fn open_file(&mut self, path: &Path) {
        match Buffer::open(path) {
            Ok(buffer) => {
                if let Some(ctx) = self.active_context_mut() {
                    ctx.editor = Some(buffer);
                    ctx.focus = Focus::Editor;
                    ctx.zoomed = false;
                }
            }
            Err(e) => self.error(format!("cannot open {}: {e}", path.display())),
        }
    }

    fn save_editor(&mut self) {
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        let Some(ed) = &mut ctx.editor else { return };
        let is_context = ed.path() == ctx.context_path;
        let next = if ed.is_read_only() {
            Next::Status("read-only buffer".into())
        } else if let Err(e) = ed.save() {
            Next::Error(format!("save failed: {e}"))
        } else {
            let p = ed.path().display().to_string();
            let _ = ctx.tree.refresh();
            if is_context {
                let _ = ctx.reload_meta();
            }
            Next::Status(format!("Saved {p}"))
        };
        if is_context {
            let cfg = self.config.clone();
            let state = self.state_dir.clone();
            if let Some(ctx) = self.active_context_mut() {
                let _ = ctx.refresh_env(&cfg, &state);
            }
        }
        self.apply_next(next);
    }

    fn close_editor(&mut self) {
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        let Some(ed) = &ctx.editor else { return };
        if ed.is_dirty() {
            self.popup = Some(Popup::confirm(
                "Discard changes?",
                format!("{} has unsaved changes.", ed.path().display()),
                Pending::CloseEditorDiscard,
            ));
        } else {
            ctx.editor = None;
            ctx.focus = Focus::Tree;
        }
    }

    fn handle_editor_key(&mut self, key: KeyEvent) {
        let height = self.editor_height;
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        let Some(ed) = &mut ctx.editor else {
            ctx.focus = Focus::Tree;
            return;
        };
        let mut next = Next::None;
        match (key.code, key.modifiers) {
            (KeyCode::Char('s'), KeyModifiers::CONTROL) => next = Next::Action(Action::Save),
            (KeyCode::Char('w'), KeyModifiers::CONTROL)
            | (KeyCode::Char('q'), KeyModifiers::CONTROL) => {
                next = Next::Action(Action::CloseEditor);
            }
            (KeyCode::Left, _) => ed.move_left(),
            (KeyCode::Right, _) => ed.move_right(),
            (KeyCode::Up, _) => ed.move_up(),
            (KeyCode::Down, _) => ed.move_down(),
            (KeyCode::Home, KeyModifiers::CONTROL) => ed.top(),
            (KeyCode::End, KeyModifiers::CONTROL) => ed.bottom(),
            (KeyCode::Home, _) => ed.home(),
            (KeyCode::End, _) => ed.end(),
            (KeyCode::PageUp, _) => ed.page_up(height),
            (KeyCode::PageDown, _) => ed.page_down(height),
            (KeyCode::Enter, _) => ed.insert_newline(),
            (KeyCode::Backspace, _) => ed.backspace(),
            (KeyCode::Delete, _) => ed.delete(),
            (KeyCode::Tab, _) => ed.insert_char('\t'),
            (KeyCode::Char(c), m)
                if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) =>
            {
                ed.insert_char(c);
            }
            _ => {}
        }
        if let Some(ed) = &mut ctx.editor {
            ed.ensure_visible(height);
        }
        self.apply_next(next);
    }

    fn handle_shell_list_key(&mut self, key: KeyEvent) {
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        let next = match (key.code, key.modifiers) {
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                ctx.select_shell(1);
                Next::None
            }
            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                ctx.select_shell(-1);
                Next::None
            }
            (KeyCode::Enter, _) | (KeyCode::Char('l'), _) | (KeyCode::Right, _) => {
                if ctx.active_shell().is_some() {
                    ctx.show_shell = true;
                    ctx.focus = Focus::Terminal;
                    Next::None
                } else {
                    Next::Action(Action::NewShell)
                }
            }
            (KeyCode::Char('n'), _) | (KeyCode::Char('t'), _) => Next::Action(Action::NewShell),
            (KeyCode::Char('x'), _) | (KeyCode::Char('d'), _) | (KeyCode::Delete, _) => {
                Next::Action(Action::CloseShell)
            }
            (KeyCode::Char('h'), _) | (KeyCode::Left, _) => {
                ctx.show_shell = false;
                Next::None
            }
            (KeyCode::Char('f'), _) => {
                ctx.focus = Focus::Tree;
                Next::None
            }
            _ => Next::None,
        };
        self.apply_next(next);
    }

    fn new_shell(&mut self) {
        let config = self.config.clone();
        let state = self.state_dir.clone();
        let id = self.next_shell_id;
        let events = self.events.clone();
        let Some(ctx) = self.active_context_mut() else {
            self.set_status("open a task first");
            return;
        };
        match ctx.spawn_shell(id, &config, &state, events) {
            Ok(()) => {
                self.next_shell_id += 1;
                info!(id, "spawned shell");
            }
            Err(e) => self.error(format!("could not start shell: {e:#}")),
        }
    }

    fn close_shell(&mut self) {
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        let Some(shell) = ctx.active_shell() else {
            return;
        };
        if shell.session.has_exited() {
            ctx.remove_active_shell();
            if ctx.shells.is_empty() && ctx.focus == Focus::Terminal {
                ctx.focus = Focus::Shells;
                ctx.zoomed = false;
            }
        } else {
            let id = shell.session.id();
            self.popup = Some(Popup::confirm(
                "Close shell?",
                "The running shell will be terminated.",
                Pending::CloseShell(id),
            ));
        }
    }

    fn handle_terminal_key(&mut self, key: KeyEvent) {
        let leader = self.leader;
        if self.leader_pending {
            self.leader_pending = false;
            self.handle_leader_command(key);
            return;
        }
        if leader.matches(&key) {
            self.leader_pending = true;
            return;
        }
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        // Shift+PageUp/PageDown scroll the pane like a real terminal emulator.
        if key.modifiers.contains(KeyModifiers::SHIFT)
            && matches!(key.code, KeyCode::PageUp | KeyCode::PageDown)
        {
            if let Some(shell) = ctx.active_shell_mut() {
                let rows = i32::from(shell.session.size().0.max(2)) / 2;
                let delta = if key.code == KeyCode::PageUp {
                    rows
                } else {
                    -rows
                };
                shell.session.scroll_by(delta);
            }
            return;
        }
        let Some(shell) = ctx.active_shell_mut() else {
            ctx.focus = Focus::Shells;
            return;
        };
        if shell.session.has_exited() {
            // Any key on a dead shell removes it and returns to the shell list.
            ctx.remove_active_shell();
            ctx.focus = Focus::Shells;
            ctx.zoomed = false;
            return;
        }
        let app_cursor = shell.session.screen().application_cursor();
        if let Some(bytes) = keys::encode_key(&key, app_cursor) {
            shell.session.scroll_to_bottom();
            if let Err(e) = shell.session.write(&bytes) {
                warn!("write to shell failed: {e}");
            }
        }
    }

    /// Commands after the leader key (tmux/herdr style).
    fn handle_leader_command(&mut self, key: KeyEvent) {
        let leader = self.leader;
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        if leader.matches(&key) {
            // Leader twice sends the leader itself to the shell.
            if let Some(shell) = ctx.active_shell_mut() {
                if let Some(bytes) = keys::encode_key(&key, false) {
                    let _ = shell.session.write(&bytes);
                }
            }
            return;
        }
        let next = match (key.code, key.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char(':'), _) => Next::Palette,
            (KeyCode::Char('a'), _) => Next::Action(Action::CodingAgent),
            (KeyCode::Char('m'), _) => Next::Action(Action::Timer),
            (KeyCode::Char('v'), _) => Next::Action(Action::CheckpointDone),
            (KeyCode::Char('q'), _) | (KeyCode::Char('d'), _) => {
                ctx.zoomed = false;
                ctx.focus = Focus::Shells;
                Next::None
            }
            (KeyCode::Char('z'), _) | (KeyCode::Char('f'), _) => {
                ctx.zoomed = !ctx.zoomed;
                Next::None
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => Next::Action(Action::Quit),
            (KeyCode::Char('n' | 't' | 'c' | 's'), _) => Next::Action(Action::NewShell),
            (KeyCode::Char('x'), _) => Next::Action(Action::CloseShell),
            (KeyCode::Char('h'), _) | (KeyCode::Char('p'), _) | (KeyCode::Left, _) => {
                ctx.select_shell(-1);
                Next::None
            }
            (KeyCode::Char('l'), _) | (KeyCode::Right, _) | (KeyCode::Tab, _) => {
                ctx.select_shell(1);
                Next::None
            }
            (KeyCode::Char('k'), _) | (KeyCode::Up, _) => {
                ctx.zoomed = false;
                ctx.focus = if ctx.editor.is_some() {
                    Focus::Editor
                } else {
                    Focus::Tree
                };
                Next::None
            }
            (KeyCode::Char('e'), _) => {
                ctx.zoomed = false;
                ctx.focus = Focus::Tree;
                Next::None
            }
            (KeyCode::Char('['), _) | (KeyCode::PageUp, _) => {
                if let Some(shell) = ctx.active_shell_mut() {
                    let rows = i32::from(shell.session.size().0.max(2)) / 2;
                    shell.session.scroll_by(rows);
                }
                Next::None
            }
            (KeyCode::Char(']'), _) | (KeyCode::PageDown, _) => {
                if let Some(shell) = ctx.active_shell_mut() {
                    let rows = i32::from(shell.session.size().0.max(2)) / 2;
                    shell.session.scroll_by(-rows);
                }
                Next::None
            }
            (KeyCode::Char('?'), _) => Next::Action(Action::Help),
            _ => Next::Status(format!("unknown leader command; {leader} ? for help")),
        };
        self.apply_next(next);
    }

    // ----- popups ------------------------------------------------------------------

    fn handle_popup_key(&mut self, key: KeyEvent) {
        let Some(popup) = self.popup.take() else {
            return;
        };
        match popup {
            Popup::Message { .. } => {}
            Popup::Log {
                title,
                mut lines,
                done,
            } => {
                if !done {
                    // Still running: keep it; Esc cancels jobs that can be cancelled.
                    if key.code == KeyCode::Esc {
                        if let Some(c) = &self.job_cancel {
                            if !c.swap(true, Ordering::Relaxed) {
                                lines.push("cancelling …".into());
                            }
                        }
                    }
                    self.popup = Some(Popup::Log { title, lines, done });
                }
            }
            Popup::Doc {
                title,
                lines,
                mut scroll,
            } => {
                let max = lines.len().saturating_sub(1);
                let step = match key.code {
                    KeyCode::Down | KeyCode::Char('j') => Some(1i64),
                    KeyCode::Up | KeyCode::Char('k') => Some(-1),
                    KeyCode::PageDown | KeyCode::Char(' ') => Some(10),
                    KeyCode::PageUp => Some(-10),
                    KeyCode::Home | KeyCode::Char('g') => Some(-(max as i64)),
                    KeyCode::End | KeyCode::Char('G') => Some(max as i64),
                    _ => None,
                };
                if let Some(d) = step {
                    scroll = (scroll as i64 + d).clamp(0, max as i64) as usize;
                    self.popup = Some(Popup::Doc {
                        title,
                        lines,
                        scroll,
                    });
                }
            }
            Popup::Checkpoints {
                task_id,
                mut items,
                mut selected,
            } => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {}
                KeyCode::Enter | KeyCode::Char('m') => {
                    if let Some(c) = items.get(selected).filter(|c| !c.done) {
                        let title = c.title.clone();
                        self.start_timer(&task_id, Some(&title));
                    } else {
                        self.popup = Some(Popup::Checkpoints {
                            task_id,
                            items,
                            selected,
                        });
                    }
                }
                KeyCode::Char(' ' | 'x' | 'v') => {
                    self.toggle_checkpoint(&task_id, &mut items, selected);
                    self.popup = Some(Popup::Checkpoints {
                        task_id,
                        items,
                        selected,
                    });
                }
                code => {
                    step_selection(&mut selected, items.len(), code);
                    self.popup = Some(Popup::Checkpoints {
                        task_id,
                        items,
                        selected,
                    });
                }
            },
            Popup::Confirm {
                title,
                body,
                pending,
            } => match key.code {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter => self.run_pending(pending, None),
                KeyCode::Char('n' | 'N' | 'q') | KeyCode::Esc => {}
                _ => {
                    self.popup = Some(Popup::Confirm {
                        title,
                        body,
                        pending,
                    });
                }
            },
            Popup::Input {
                title,
                label,
                mut value,
                pending,
            } => match (key.code, key.modifiers) {
                (KeyCode::Enter, _) => {
                    if value.trim().is_empty() {
                        self.popup = Some(Popup::Input {
                            title,
                            label,
                            value,
                            pending,
                        });
                    } else {
                        self.run_pending(pending, Some(value.trim().to_owned()));
                    }
                }
                (KeyCode::Esc, _) => {}
                (KeyCode::Backspace, _) => {
                    value.pop();
                    self.popup = Some(Popup::Input {
                        title,
                        label,
                        value,
                        pending,
                    });
                }
                (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                    value.clear();
                    self.popup = Some(Popup::Input {
                        title,
                        label,
                        value,
                        pending,
                    });
                }
                (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
                    value.push(c);
                    self.popup = Some(Popup::Input {
                        title,
                        label,
                        value,
                        pending,
                    });
                }
                _ => {
                    self.popup = Some(Popup::Input {
                        title,
                        label,
                        value,
                        pending,
                    });
                }
            },
            Popup::Choose {
                title,
                choices,
                mut selected,
            } => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {}
                KeyCode::Enter => {
                    if let Some(c) = choices.get(selected) {
                        let pending = c.pending.clone();
                        self.run_pending(pending, None);
                    }
                }
                KeyCode::Char(d @ '1'..='9') => {
                    let i = d as usize - '1' as usize;
                    if let Some(c) = choices.get(i) {
                        let pending = c.pending.clone();
                        self.run_pending(pending, None);
                    } else {
                        self.popup = Some(Popup::Choose {
                            title,
                            choices,
                            selected,
                        });
                    }
                }
                code => {
                    step_selection(&mut selected, choices.len(), code);
                    self.popup = Some(Popup::Choose {
                        title,
                        choices,
                        selected,
                    });
                }
            },
            Popup::MultiSelect {
                title,
                mut items,
                mut selected,
                pending,
            } => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {}
                KeyCode::Enter => {
                    let keys: Vec<String> = items
                        .iter()
                        .filter(|i| i.checked)
                        .map(|i| i.key.clone())
                        .collect();
                    match pending {
                        Pending::ApplyAttachments => self.apply_attachments(&keys),
                        other => self.run_pending(other, None),
                    }
                }
                KeyCode::Char(' ') | KeyCode::Char('x') => {
                    if let Some(item) = items.get_mut(selected) {
                        item.checked = !item.checked;
                    }
                    self.popup = Some(Popup::MultiSelect {
                        title,
                        items,
                        selected,
                        pending,
                    });
                }
                code => {
                    step_selection(&mut selected, items.len(), code);
                    self.popup = Some(Popup::MultiSelect {
                        title,
                        items,
                        selected,
                        pending,
                    });
                }
            },
            Popup::Tickets {
                source,
                tickets,
                mut filter,
                mut selected,
            } => match (key.code, key.modifiers) {
                (KeyCode::Esc, _) => {}
                (KeyCode::Enter, _) => {
                    let chosen = popup::filter_tickets(&tickets, &filter)
                        .get(selected)
                        .map(|t| (*t).clone());
                    match chosen {
                        Some(t) => self.create_from_ticket(&source, &t),
                        None => {
                            self.popup = Some(Popup::Tickets {
                                source,
                                tickets,
                                filter,
                                selected,
                            });
                        }
                    }
                }
                (KeyCode::Backspace, _) => {
                    filter.pop();
                    selected = 0;
                    self.popup = Some(Popup::Tickets {
                        source,
                        tickets,
                        filter,
                        selected,
                    });
                }
                (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                    filter.clear();
                    selected = 0;
                    self.popup = Some(Popup::Tickets {
                        source,
                        tickets,
                        filter,
                        selected,
                    });
                }
                (
                    KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::PageUp
                    | KeyCode::PageDown,
                    _,
                ) => {
                    let n = popup::filter_tickets(&tickets, &filter).len();
                    step_selection(&mut selected, n, key.code);
                    self.popup = Some(Popup::Tickets {
                        source,
                        tickets,
                        filter,
                        selected,
                    });
                }
                (KeyCode::Char(c), m)
                    if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) =>
                {
                    filter.push(c);
                    selected = 0;
                    self.popup = Some(Popup::Tickets {
                        source,
                        tickets,
                        filter,
                        selected,
                    });
                }
                _ => {
                    self.popup = Some(Popup::Tickets {
                        source,
                        tickets,
                        filter,
                        selected,
                    });
                }
            },
            Popup::Palette(mut palette) => match (key.code, key.modifiers) {
                (KeyCode::Esc, _) => {}
                (KeyCode::Enter, _) => {
                    if let Some(action) = palette.current() {
                        self.run_action(action);
                    } else {
                        self.popup = Some(Popup::Palette(palette));
                    }
                }
                (KeyCode::Up, _) => {
                    palette.step(-1);
                    self.popup = Some(Popup::Palette(palette));
                }
                (KeyCode::Down, _) | (KeyCode::Tab, _) => {
                    palette.step(1);
                    self.popup = Some(Popup::Palette(palette));
                }
                (KeyCode::Backspace, _) => {
                    palette.pop();
                    self.popup = Some(Popup::Palette(palette));
                }
                (KeyCode::Char(':' | '/'), _) if !palette.typing => {
                    palette.start_typing();
                    self.popup = Some(Popup::Palette(palette));
                }
                (KeyCode::Char(c), m)
                    if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) =>
                {
                    if let Some(action) = palette.shortcut(c).filter(|_| !palette.typing) {
                        self.run_action(action);
                    } else {
                        palette.push(c);
                        self.popup = Some(Popup::Palette(palette));
                    }
                }
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => self.request_quit(),
                _ => self.popup = Some(Popup::Palette(palette)),
            },
        }
    }

    fn run_pending(&mut self, pending: Pending, input: Option<String>) {
        match pending {
            Pending::Nothing | Pending::CreateFromTicket(_) | Pending::ApplyAttachments => {}
            Pending::Quit => self.should_quit = true,
            Pending::LeaveConfigDiscard => self.return_from_config(),
            Pending::CreateTask => match input {
                Some(id) => self.create_task(&id, None),
                None => {
                    self.popup = Some(Popup::input(
                        "New task",
                        "task id (no spaces)",
                        "",
                        Pending::CreateTask,
                    ));
                }
            },
            Pending::FetchTickets(source) => self.fetch_tickets(&source),
            Pending::OpenConfirmed(path) => self.open_file(&path),
            Pending::OpenDiscard(path) => self.probe_and_open(&path),
            Pending::CloseEditorDiscard => {
                if let Some(ctx) = self.active_context_mut() {
                    ctx.editor = None;
                    ctx.focus = Focus::Tree;
                }
            }
            Pending::Rename(path) => {
                if let Some(name) = input {
                    self.file_op(|_| ops::rename(&path, &name).map(Some));
                }
            }
            Pending::CreateFile(dir) => {
                if let Some(name) = input {
                    self.file_op(|_| ops::create_file(&dir, &name).map(Some));
                }
            }
            Pending::CreateDir(dir) => {
                if let Some(name) = input {
                    self.file_op(|_| ops::create_dir(&dir, &name).map(Some));
                }
            }
            Pending::Delete(path) => {
                let was_open = self
                    .active_context()
                    .and_then(|c| c.editor.as_ref())
                    .is_some_and(|e| e.path().starts_with(&path));
                self.file_op(|root| ops::delete(root, &path).map(|()| None));
                if was_open {
                    if let Some(ctx) = self.active_context_mut() {
                        ctx.editor = None;
                        if ctx.focus == Focus::Editor {
                            ctx.focus = Focus::Tree;
                        }
                    }
                }
            }
            Pending::CloseShell(id) => {
                if let Some(ctx) = self.active_context_mut() {
                    ctx.remove_shell(id);
                    if ctx.focus == Focus::Terminal && ctx.active_shell().is_none() {
                        ctx.focus = Focus::Shells;
                        ctx.zoomed = false;
                    }
                }
            }
            Pending::RunPrepare { commit_on_main } => self.run_prepare(commit_on_main),
            Pending::Run(action) => self.run_action(action),
            Pending::DeleteTask(id) => self.delete_task(&id),
            Pending::StartFocus(id) => {
                if let Some(m) = input {
                    self.start_focus(&id, &m);
                }
            }
            Pending::TimerDone => self.checkpoint_done(),
            Pending::TimerExtend(m) => self.extend_timer(m),
            Pending::TimerPause => self.pause_timer(),
            Pending::TimerStop => self.stop_timer(),
            Pending::ReplaceCheckpoints(id, items) => self.replace_checkpoints(&id, &items),
            Pending::Outcome(id) => {
                if let Some(line) = input {
                    self.record_outcome(&id, &line);
                }
            }
            Pending::EditPrompt(kind) => self.edit_prompt(kind),
        }
    }

    /// Run a file operation against the active task's tree, refresh, and select the result.
    fn file_op<F>(&mut self, op: F)
    where
        F: FnOnce(&Path) -> std::io::Result<Option<PathBuf>>,
    {
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        let root = ctx.tree.root().to_path_buf();
        match op(&root) {
            Ok(created) => {
                let _ = ctx.tree.refresh();
                if let Some(p) = created {
                    ctx.tree.select_path(&p);
                }
            }
            Err(e) => self.error(e.to_string()),
        }
    }

    /// Book the running timer and kill all shells before exit.
    pub fn shutdown(&mut self) {
        if self.timer.is_some() {
            self.book_time(true, false);
            self.timer = None;
        }
        for ctx in self.contexts.values_mut() {
            ctx.shells.clear();
        }
    }
}

fn step_selection(selected: &mut usize, len: usize, code: KeyCode) {
    if len == 0 {
        *selected = 0;
        return;
    }
    let last = len - 1;
    *selected = match code {
        KeyCode::Up | KeyCode::Char('k') => selected.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => (*selected + 1).min(last),
        KeyCode::Home => 0,
        KeyCode::End => last,
        KeyCode::PageUp => selected.saturating_sub(10),
        KeyCode::PageDown => (*selected + 10).min(last),
        _ => *selected,
    };
}

fn short_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root).map_or_else(
        |_| path.display().to_string(),
        |p| {
            let s = p.display().to_string();
            if s.is_empty() {
                "task folder".to_owned()
            } else {
                s
            }
        },
    )
}

/// Human readable byte size.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests;
