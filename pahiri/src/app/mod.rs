//! Application state and event handling.
//!
//! The [`App`] owns everything the UI shows and mutates it in response to
//! [`AppEvent`]s. Drawing lives in [`crate::ui`] and only reads this state
//! (plus lazily resizing shells to fit their pane).

pub mod config_form;
pub mod context;
pub mod event;
pub mod popup;

mod keymap;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tracing::{info, warn};

use crate::config::keybind::KeyCombo;
use crate::config::Config;
use crate::editor::Buffer;
use crate::files::{ops, probe};
use crate::tasks::TaskStore;
use crate::terminal::{keys, PtyEvent, ShellId};

pub use self::config_form::ConfigForm;
pub use self::context::{Focus, Shell, TaskContext};
pub use self::event::{AppEvent, EventSender};
pub use self::popup::{Pending, Popup};

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
    Back,
    OpenFile(PathBuf),
    Popup(Popup),
    NewShell,
    CloseShell,
    Quit,
    Status(String),
    Error(String),
}

/// The whole application state.
pub struct App {
    config: Config,
    config_path: PathBuf,
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
}

impl App {
    /// Create the app. Opens the config page when no valid config exists.
    pub fn new(config: Option<Config>, config_path: PathBuf, events: EventSender) -> Self {
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

    /// Current mode.
    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    /// Mutable access to the mode (the config form is edited in place).
    pub fn mode_mut(&mut self) -> &mut Mode {
        &mut self.mode
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
            AppEvent::Input(
                Event::Resize(..) | Event::FocusGained | Event::FocusLost | Event::Mouse(_),
            )
            | AppEvent::Tick => {}
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
        }
    }

    fn shell_mut(&mut self, id: ShellId) -> Option<&mut Shell> {
        self.contexts.values_mut().find_map(|c| c.shell_mut(id))
    }

    fn handle_paste(&mut self, text: &str) {
        if let Some(Popup::Input { value, .. }) = &mut self.popup {
            value.push_str(text.lines().next().unwrap_or(""));
            return;
        }
        if self.popup.is_some() {
            return;
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
            _ => {
                if let Mode::Config(form) = &mut self.mode {
                    form.handle_nav_key(key);
                }
            }
        }
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
        self.mode = Mode::TaskList;
        if self.store.is_none() {
            self.open_store();
        }
    }

    // ----- task list ---------------------------------------------------------------

    fn handle_list_key(&mut self, key: KeyEvent) {
        let rows = self.rows();
        match (key.code, key.modifiers) {
            (KeyCode::Char('q'), KeyModifiers::NONE)
            | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.request_quit();
            }
            (KeyCode::Char('?'), _) => self.popup = Some(Popup::message("Keys", keymap::LIST_HELP)),
            (KeyCode::Char(','), _) | (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                self.mode = Mode::Config(ConfigForm::new(&self.config, false));
            }
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
                if let Some(ListRow::Task(_, id)) = rows.get(self.list_selected) {
                    let id = id.clone();
                    self.enter_task(&id);
                }
            }
            (KeyCode::Char(']'), _) | (KeyCode::Char('>'), _) => self.move_task(1, &rows),
            (KeyCode::Char('['), _) | (KeyCode::Char('<'), _) => self.move_task(-1, &rows),
            (KeyCode::Char('n'), _) => {
                self.popup = Some(Popup::input("New task", "task id", "", Pending::CreateTask));
            }
            (KeyCode::Char('r'), _) | (KeyCode::F(5), _) => self.refresh_store(),
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

    fn move_task(&mut self, delta: i32, rows: &[ListRow]) {
        let Some(ListRow::Task(col, id)) = rows.get(self.list_selected).cloned() else {
            return;
        };
        let Some(store) = &mut self.store else { return };
        let target = col as i64 + i64::from(delta);
        if target < 0 || target >= store.categories().len() as i64 {
            return;
        }
        match store.move_task(&id, target as usize) {
            Ok(true) => {
                let rows = self.rows();
                if let Some(i) = rows
                    .iter()
                    .position(|r| matches!(r, ListRow::Task(_, t) if *t == id))
                {
                    self.list_selected = i;
                }
                self.set_status(format!("Moved {id}"));
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
        let rows = self.rows();
        self.list_selected = self.list_selected.min(rows.len().saturating_sub(1));
    }

    fn create_task(&mut self, id: &str) {
        let column = match self.rows().get(self.list_selected) {
            Some(ListRow::Task(c, _) | ListRow::Header(c)) => *c,
            None => 0,
        };
        let Some(store) = &mut self.store else { return };
        match store.create_task(id.trim(), column) {
            Ok(summary) => {
                let rows = self.rows();
                if let Some(i) = rows
                    .iter()
                    .position(|r| matches!(r, ListRow::Task(_, t) if *t == summary.id))
                {
                    self.list_selected = i;
                }
                self.set_status(format!("Created {}", summary.id));
            }
            Err(e) => self.error(e.to_string()),
        }
    }

    fn enter_task(&mut self, id: &str) {
        let Some(store) = &self.store else { return };
        let summary = store.summary(id);
        if !self.contexts.contains_key(id) {
            match TaskContext::new(&summary, self.config.show_hidden) {
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
        if let Some(ctx) = self.active_context_mut() {
            let _ = ctx.tree.refresh();
        }
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

    // ----- task view ---------------------------------------------------------------

    fn handle_task_key(&mut self, key: KeyEvent) {
        let Some(focus) = self.active_context().map(|c| c.focus) else {
            self.mode = Mode::TaskList;
            return;
        };
        if focus == Focus::Terminal {
            self.handle_terminal_key(key);
            return;
        }
        // Global keys while not in a terminal.
        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.request_quit();
                return;
            }
            (KeyCode::Char('?'), _) if focus != Focus::Editor => {
                self.popup = Some(Popup::message("Keys", keymap::TASK_HELP));
                return;
            }
            (KeyCode::Tab, _) => {
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

    fn back_to_list(&mut self) {
        self.mode = Mode::TaskList;
        if let Some(id) = &self.active_task {
            let rows = self.rows();
            if let Some(i) = rows
                .iter()
                .position(|r| matches!(r, ListRow::Task(_, t) if t == id))
            {
                self.list_selected = i;
            }
        }
    }

    fn handle_tree_key(&mut self, key: KeyEvent) {
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        let root = ctx.tree.root().to_path_buf();
        let mut next = Next::None;
        let result: std::io::Result<()> = match (key.code, key.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char('q'), _) => {
                next = Next::Back;
                Ok(())
            }
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
                next = Next::NewShell;
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
            Next::Back => self.back_to_list(),
            Next::OpenFile(path) => self.request_open_file(&path),
            Next::Popup(p) => self.popup = Some(p),
            Next::NewShell => self.new_shell(),
            Next::CloseShell => self.close_shell(),
            Next::Quit => self.request_quit(),
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
                }
            }
            Err(e) => self.error(format!("cannot open {}: {e}", path.display())),
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
            (KeyCode::Esc, _) => ctx.focus = Focus::Tree,
            (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                if ed.is_read_only() {
                    next = Next::Status("read-only buffer".into());
                } else if let Err(e) = ed.save() {
                    next = Next::Error(format!("save failed: {e}"));
                } else {
                    next = Next::Status(format!("Saved {}", ed.path().display()));
                    let _ = ctx.tree.refresh();
                }
            }
            (KeyCode::Char('w'), KeyModifiers::CONTROL)
            | (KeyCode::Char('q'), KeyModifiers::CONTROL) => {
                if ed.is_dirty() {
                    next = Next::Popup(Popup::confirm(
                        "Discard changes?",
                        format!("{} has unsaved changes.", ed.path().display()),
                        Pending::CloseEditorDiscard,
                    ));
                } else {
                    ctx.editor = None;
                    ctx.focus = Focus::Tree;
                }
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
            (KeyCode::Esc, _) | (KeyCode::Char('q'), _) => Next::Back,
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
                    Next::NewShell
                }
            }
            (KeyCode::Char('n'), _) | (KeyCode::Char('t'), _) => Next::NewShell,
            (KeyCode::Char('x'), _) | (KeyCode::Char('d'), _) | (KeyCode::Delete, _) => {
                Next::CloseShell
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
        let id = self.next_shell_id;
        let events = self.events.clone();
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        match ctx.spawn_shell(id, &config, events) {
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
            (KeyCode::Char('q'), _) | (KeyCode::Char('d'), _) | (KeyCode::Esc, _) => {
                ctx.zoomed = false;
                ctx.focus = Focus::Shells;
                Next::None
            }
            (KeyCode::Char('z'), _) | (KeyCode::Char('f'), _) => {
                ctx.zoomed = !ctx.zoomed;
                Next::None
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => Next::Quit,
            (KeyCode::Char('n' | 't' | 'c'), _) => Next::NewShell,
            (KeyCode::Char('x'), _) => Next::CloseShell,
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
            (KeyCode::Char('?'), _) => Next::Popup(Popup::message("Keys", keymap::TASK_HELP)),
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
            Popup::Confirm {
                title,
                body,
                pending,
            } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    self.run_pending(pending, None);
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc | KeyCode::Char('q') => {}
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
        }
    }

    fn run_pending(&mut self, pending: Pending, input: Option<String>) {
        match pending {
            Pending::Quit => self.should_quit = true,
            Pending::LeaveConfigDiscard => {
                self.mode = Mode::TaskList;
                if self.store.is_none() {
                    self.open_store();
                }
            }
            Pending::CreateTask => {
                if let Some(id) = input {
                    self.create_task(&id);
                }
            }
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

    /// Kill all shells before exit.
    pub fn shutdown(&mut self) {
        for ctx in self.contexts.values_mut() {
            ctx.shells.clear();
        }
    }
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
