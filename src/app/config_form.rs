//! The settings page: scalar fields plus editable lists, backed by [`Config`].

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::{ColorScheme, Config, TaskSource, VendorBuild, Workspace};

/// Which configuration value a field edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKey {
    /// Tasks folder.
    TasksDir,
    /// Board categories.
    Categories,
    /// Colour scheme.
    ColorScheme,
    /// Shell program.
    ShellProgram,
    /// Shell arguments.
    ShellArgs,
    /// Leader key.
    LeaderKey,
    /// Font family (advisory).
    FontFamily,
    /// Default main branch.
    MainBranch,
    /// Code workspaces.
    Workspaces,
    /// Vendor builds.
    Builds,
    /// Task sources.
    TaskSources,
    /// Large-file threshold.
    LargeFileKb,
    /// Show hidden files.
    ShowHidden,
    /// Mouse capture.
    Mouse,
    /// Syntax highlighting.
    Highlighting,
    /// Editor tab width.
    TabWidth,
    /// Scrollback lines.
    Scrollback,
    /// Board file name.
    StatusFile,
    /// Context file name.
    ContextFile,
}

/// A field's value and editing behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// Free text.
    Text(String),
    /// Numeric text.
    Number(String),
    /// Boolean.
    Toggle(bool),
    /// Colour scheme.
    Scheme(ColorScheme),
    /// One string per item.
    List(Vec<String>),
}

/// One field of the settings page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// Which value.
    pub key: FieldKey,
    /// Label shown on the left.
    pub label: &'static str,
    /// Explanation shown for the selected row.
    pub help: &'static str,
    /// Current value.
    pub value: Value,
}

/// A visible row: a field, one list item, or the "add item" row of a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// Field header / scalar value.
    Field(usize),
    /// Item `item` of list field `field`.
    Item {
        /// Field index.
        field: usize,
        /// Item index.
        item: usize,
    },
    /// The "+ add" row of list field `field`.
    Add(usize),
}

/// State of the settings page.
#[derive(Debug, Clone)]
pub struct ConfigForm {
    fields: Vec<Field>,
    selected: usize,
    /// Edit buffer and cursor (char index) while editing text.
    editing: Option<(String, usize)>,
    errors: Vec<String>,
    first_run: bool,
    saved: Vec<Field>,
}

fn field(key: FieldKey, label: &'static str, help: &'static str, value: Value) -> Field {
    Field {
        key,
        label,
        help,
        value,
    }
}

fn workspace_item(w: &Workspace) -> String {
    match &w.main_branch {
        Some(b) => format!("{} = {} @{b}", w.name, w.path.display()),
        None => format!("{} = {}", w.name, w.path.display()),
    }
}

/// Parse `name = /path [@branch]`.
fn parse_workspace(item: &str) -> Result<Workspace, String> {
    let (name, rest) = item
        .split_once('=')
        .ok_or_else(|| format!("workspace {item:?}: expected `name = /path [@branch]`"))?;
    let rest = rest.trim();
    let (path, branch) = match rest.rsplit_once(" @") {
        Some((p, b)) if !p.trim().is_empty() && !b.trim().is_empty() => {
            (p.trim(), Some(b.trim().to_owned()))
        }
        _ => (rest, None),
    };
    if path.is_empty() {
        return Err(format!("workspace {item:?}: missing path"));
    }
    Ok(Workspace {
        name: name.trim().to_owned(),
        path: Config::expand_tilde(path),
        main_branch: branch,
    })
}

/// Parse `name = /path`.
fn parse_build(item: &str) -> Result<VendorBuild, String> {
    let (name, path) = item
        .split_once('=')
        .ok_or_else(|| format!("build {item:?}: expected `name = /path`"))?;
    if path.trim().is_empty() {
        return Err(format!("build {item:?}: missing path"));
    }
    Ok(VendorBuild {
        name: name.trim().to_owned(),
        path: Config::expand_tilde(path.trim()),
    })
}

/// Parse `name = command ...`.
fn parse_source(item: &str) -> Result<TaskSource, String> {
    let (name, command) = item
        .split_once('=')
        .ok_or_else(|| format!("task source {item:?}: expected `name = command`"))?;
    Ok(TaskSource {
        name: name.trim().to_owned(),
        command: command.trim().to_owned(),
    })
}

impl ConfigForm {
    /// Build the form from a config. `first_run` shows the welcome banner and
    /// prevents leaving until a valid config is saved.
    pub fn new(cfg: &Config, first_run: bool) -> Self {
        let fields = vec![
            field(
                FieldKey::TasksDir,
                "Tasks folder",
                "Folder with one sub-folder per task (each with a CONTEXT.md). Required.",
                Value::Text(cfg.tasks_dir.display().to_string()),
            ),
            field(
                FieldKey::Categories,
                "Categories",
                "Board columns in display order (Enter on '+ add' adds, d deletes). At least one.",
                Value::List(cfg.categories.clone()),
            ),
            field(
                FieldKey::Workspaces,
                "Code workspaces",
                "One per line: `name = /path/to/checkout [@main-branch]`. Attach them to tasks with Esc, a.",
                Value::List(cfg.workspaces.iter().map(workspace_item).collect()),
            ),
            field(
                FieldKey::Builds,
                "Vendor builds",
                "One per line: `name = /path/to/yocto/build`. Attach them to tasks with Esc, a.",
                Value::List(cfg.builds.iter().map(|b| format!("{} = {}", b.name, b.path.display())).collect()),
            ),
            field(
                FieldKey::TaskSources,
                "Task sources",
                "One per line: `name = command`. The command prints tickets as JSON (id,title,url,description) or TSV.",
                Value::List(cfg.task_sources.iter().map(|t| format!("{} = {}", t.name, t.command)).collect()),
            ),
            field(
                FieldKey::MainBranch,
                "Default main branch",
                "Main branch for workspaces that do not set their own (`@branch`).",
                Value::Text(cfg.default_main_branch.clone()),
            ),
            field(
                FieldKey::ColorScheme,
                "Colour scheme",
                "UI palette. Shell colours always come from the programs themselves.",
                Value::Scheme(cfg.color_scheme),
            ),
            field(
                FieldKey::ShellProgram,
                "Shell",
                "Program launched for task terminals (zsh by default). zsh and bash get `cd task/code/build`.",
                Value::Text(cfg.shell.program.clone()),
            ),
            field(
                FieldKey::ShellArgs,
                "Shell arguments",
                "Space separated arguments, e.g. \"-i\" or \"-l\".",
                Value::Text(cfg.shell.args.join(" ")),
            ),
            field(
                FieldKey::LeaderKey,
                "Leader key",
                "Prefix intercepted while a shell has focus (tmux style). Press it twice to send it through.",
                Value::Text(cfg.leader_key.clone()),
            ),
            field(
                FieldKey::FontFamily,
                "Font family",
                "Advisory: your terminal emulator owns the font. Recorded here and shown in the status bar.",
                Value::Text(cfg.font_family.clone()),
            ),
            field(
                FieldKey::LargeFileKb,
                "Large file warning (KiB)",
                "Files bigger than this ask before opening. Binary files always ask.",
                Value::Number(cfg.large_file_kb.to_string()),
            ),
            field(
                FieldKey::ShowHidden,
                "Show hidden files",
                "Show dot-files in the task file tree ('.' toggles at runtime).",
                Value::Toggle(cfg.show_hidden),
            ),
            field(
                FieldKey::Mouse,
                "Mouse",
                "Click to focus and select, wheel to scroll, forwarded to programs that ask. Shift+drag selects text. Takes effect on restart.",
                Value::Toggle(cfg.mouse),
            ),
            field(
                FieldKey::Highlighting,
                "Syntax highlighting",
                "Colour C, C++, Rust, Bash, Python, CMake, Makefiles, logs and Markdown in the editor.",
                Value::Toggle(cfg.syntax_highlighting),
            ),
            field(
                FieldKey::TabWidth,
                "Tab width",
                "Columns per tab character in the editor.",
                Value::Number(cfg.tab_width.to_string()),
            ),
            field(
                FieldKey::Scrollback,
                "Scrollback lines",
                "History kept per shell for Shift+PageUp scrolling.",
                Value::Number(cfg.scrollback_lines.to_string()),
            ),
            field(
                FieldKey::StatusFile,
                "Status file",
                "Markdown board file inside the tasks folder (one heading per category).",
                Value::Text(cfg.status_file.clone()),
            ),
            field(
                FieldKey::ContextFile,
                "Context file",
                "File each task folder is expected to contain.",
                Value::Text(cfg.context_file.clone()),
            ),
        ];
        Self {
            saved: fields.clone(),
            fields,
            selected: 0,
            editing: None,
            errors: Vec::new(),
            first_run,
        }
    }

    /// All fields.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// Visible rows in order.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for (fi, f) in self.fields.iter().enumerate() {
            rows.push(Row::Field(fi));
            if let Value::List(items) = &f.value {
                for ii in 0..items.len() {
                    rows.push(Row::Item {
                        field: fi,
                        item: ii,
                    });
                }
                rows.push(Row::Add(fi));
            }
        }
        rows
    }

    /// Selected row index (into [`ConfigForm::rows`]).
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Select row `index` (clamped).
    pub fn select(&mut self, index: usize) {
        self.selected = index.min(self.rows().len() - 1);
    }

    /// The selected row.
    pub fn selected_row(&self) -> Row {
        let rows = self.rows();
        rows[self.selected.min(rows.len() - 1)]
    }

    /// Field index of the selected row.
    pub fn selected_field(&self) -> usize {
        match self.selected_row() {
            Row::Field(f) | Row::Add(f) | Row::Item { field: f, .. } => f,
        }
    }

    /// Whether a text value is being edited.
    pub fn editing(&self) -> bool {
        self.editing.is_some()
    }

    /// Edit buffer and cursor while editing.
    pub fn edit_state(&self) -> Option<(&str, usize)> {
        self.editing.as_ref().map(|(s, c)| (s.as_str(), *c))
    }

    /// Validation errors from the last save attempt.
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Whether this is the first-run wizard.
    pub fn first_run(&self) -> bool {
        self.first_run
    }

    /// Whether values differ from the last save.
    pub fn dirty(&self) -> bool {
        self.fields != self.saved
    }

    /// Replace the error list.
    pub fn set_errors(&mut self, errors: Vec<String>) {
        self.errors = errors;
    }

    /// Record the current values as saved.
    pub fn mark_saved(&mut self) {
        self.saved.clone_from(&self.fields);
        self.first_run = false;
    }

    fn start_edit(&mut self, text: String) {
        let len = text.chars().count();
        self.editing = Some((text, len));
    }

    /// Handle a key while not editing.
    pub fn handle_nav_key(&mut self, key: KeyEvent) {
        let rows = self.rows();
        let n = rows.len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.selected = (self.selected + 1) % n;
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                self.selected = (self.selected + n - 1) % n;
            }
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = n - 1,
            KeyCode::Enter | KeyCode::Char('e') | KeyCode::Char(' ') => match self.selected_row() {
                Row::Field(fi) => match &self.fields[fi].value {
                    Value::Text(t) | Value::Number(t) => {
                        let t = t.clone();
                        self.start_edit(t);
                    }
                    Value::Toggle(_) => self.toggle(fi),
                    Value::Scheme(_) => self.cycle_scheme(fi, true),
                    Value::List(_) => {
                        // Enter on a list header jumps to its "+ add" row.
                        if let Some(i) = rows.iter().position(|r| *r == Row::Add(fi)) {
                            self.selected = i;
                            self.start_edit(String::new());
                        }
                    }
                },
                Row::Item { field, item } => {
                    if let Value::List(items) = &self.fields[field].value {
                        let t = items[item].clone();
                        self.start_edit(t);
                    }
                }
                Row::Add(_) => self.start_edit(String::new()),
            },
            KeyCode::Char('a') => {
                let fi = self.selected_field();
                if let Some(i) = rows.iter().position(|r| *r == Row::Add(fi)) {
                    self.selected = i;
                    self.start_edit(String::new());
                }
            }
            KeyCode::Delete | KeyCode::Char('d') => {
                if let Row::Item { field, item } = self.selected_row() {
                    if let Value::List(items) = &mut self.fields[field].value {
                        items.remove(item);
                    }
                }
            }
            KeyCode::Right | KeyCode::Char('l') => self.nudge(true),
            KeyCode::Left | KeyCode::Char('h') => self.nudge(false),
            _ => {}
        }
        let n = self.rows().len();
        self.selected = self.selected.min(n - 1);
    }

    /// Handle a key while editing.
    pub fn handle_edit_key(&mut self, key: KeyEvent) {
        let Some((buf, cursor)) = &mut self.editing else {
            return;
        };
        match (key.code, key.modifiers) {
            (KeyCode::Enter, _) => {
                let (value, _) = self.editing.take().expect("editing");
                self.commit_edit(value);
            }
            (KeyCode::Esc, _) => self.editing = None,
            (KeyCode::Left, _) => *cursor = cursor.saturating_sub(1),
            (KeyCode::Right, _) => *cursor = (*cursor + 1).min(buf.chars().count()),
            (KeyCode::Home, _) | (KeyCode::Char('a'), KeyModifiers::CONTROL) => *cursor = 0,
            (KeyCode::End, _) | (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                *cursor = buf.chars().count();
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                buf.clear();
                *cursor = 0;
            }
            (KeyCode::Backspace, _) => {
                if *cursor > 0 {
                    let idx = byte_index(buf, *cursor - 1);
                    buf.remove(idx);
                    *cursor -= 1;
                }
            }
            (KeyCode::Delete, _) => {
                if *cursor < buf.chars().count() {
                    let idx = byte_index(buf, *cursor);
                    buf.remove(idx);
                }
            }
            (KeyCode::Char(c), m)
                if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) =>
            {
                let idx = byte_index(buf, *cursor);
                buf.insert(idx, c);
                *cursor += 1;
            }
            _ => {}
        }
    }

    fn commit_edit(&mut self, value: String) {
        match self.selected_row() {
            Row::Field(fi) => match &mut self.fields[fi].value {
                Value::Text(t) | Value::Number(t) => *t = value,
                _ => {}
            },
            Row::Item { field, item } => {
                if let Value::List(items) = &mut self.fields[field].value {
                    if value.trim().is_empty() {
                        items.remove(item);
                    } else {
                        items[item] = value;
                    }
                }
            }
            Row::Add(fi) => {
                if !value.trim().is_empty() {
                    if let Value::List(items) = &mut self.fields[fi].value {
                        items.push(value);
                    }
                    // Keep the cursor on the "+ add" row, which moved down by one.
                    self.selected += 1;
                }
            }
        }
    }

    /// Insert pasted text into the edit buffer (starts editing if needed).
    pub fn paste(&mut self, text: &str) {
        let text: String = text.lines().next().unwrap_or("").to_owned();
        if self.editing.is_none() {
            match self.selected_row() {
                Row::Field(fi) => match &self.fields[fi].value {
                    Value::Text(t) | Value::Number(t) => {
                        let t = t.clone();
                        self.start_edit(t);
                    }
                    _ => return,
                },
                Row::Item { field, item } => {
                    if let Value::List(items) = &self.fields[field].value {
                        let t = items[item].clone();
                        self.start_edit(t);
                    }
                }
                Row::Add(_) => self.start_edit(String::new()),
            }
        }
        if let Some((buf, cursor)) = &mut self.editing {
            let idx = byte_index(buf, *cursor);
            buf.insert_str(idx, &text);
            *cursor += text.chars().count();
        }
    }

    fn toggle(&mut self, fi: usize) {
        if let Value::Toggle(b) = &mut self.fields[fi].value {
            *b = !*b;
        }
    }

    fn cycle_scheme(&mut self, fi: usize, forward: bool) {
        if let Value::Scheme(s) = &mut self.fields[fi].value {
            *s = if forward { s.next() } else { s.prev() };
        }
    }

    fn nudge(&mut self, up: bool) {
        let Row::Field(fi) = self.selected_row() else {
            return;
        };
        match &mut self.fields[fi].value {
            Value::Toggle(_) => self.toggle(fi),
            Value::Scheme(_) => self.cycle_scheme(fi, up),
            Value::Number(t) => {
                if let Ok(n) = t.trim().parse::<i64>() {
                    let step = if n >= 1000 {
                        100
                    } else if n >= 100 {
                        10
                    } else {
                        1
                    };
                    *t = if up { n + step } else { (n - step).max(0) }.to_string();
                }
            }
            Value::Text(_) | Value::List(_) => {}
        }
    }

    /// Build a config from the form. Values that fail to parse keep `base`'s
    /// value and are reported in the returned error list.
    pub fn to_config(&self, base: &Config) -> (Config, Vec<String>) {
        let mut cfg = base.clone();
        let mut errors = Vec::new();
        for f in &self.fields {
            match (&f.key, &f.value) {
                (FieldKey::TasksDir, Value::Text(v)) => {
                    cfg.tasks_dir = Config::expand_tilde(v.trim());
                }
                (FieldKey::Categories, Value::List(items)) => {
                    cfg.categories = items
                        .iter()
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                (FieldKey::Workspaces, Value::List(items)) => {
                    cfg.workspaces.clear();
                    for item in items {
                        match parse_workspace(item) {
                            Ok(w) => cfg.workspaces.push(w),
                            Err(e) => errors.push(e),
                        }
                    }
                }
                (FieldKey::Builds, Value::List(items)) => {
                    cfg.builds.clear();
                    for item in items {
                        match parse_build(item) {
                            Ok(b) => cfg.builds.push(b),
                            Err(e) => errors.push(e),
                        }
                    }
                }
                (FieldKey::TaskSources, Value::List(items)) => {
                    cfg.task_sources.clear();
                    for item in items {
                        match parse_source(item) {
                            Ok(s) => cfg.task_sources.push(s),
                            Err(e) => errors.push(e),
                        }
                    }
                }
                (FieldKey::MainBranch, Value::Text(v)) => {
                    v.trim().clone_into(&mut cfg.default_main_branch);
                }
                (FieldKey::ColorScheme, Value::Scheme(s)) => cfg.color_scheme = *s,
                (FieldKey::ShellProgram, Value::Text(v)) => {
                    v.trim().clone_into(&mut cfg.shell.program);
                }
                (FieldKey::ShellArgs, Value::Text(v)) => {
                    cfg.shell.args = v.split_whitespace().map(str::to_owned).collect();
                }
                (FieldKey::LeaderKey, Value::Text(v)) => v.trim().clone_into(&mut cfg.leader_key),
                (FieldKey::FontFamily, Value::Text(v)) => v.trim().clone_into(&mut cfg.font_family),
                (FieldKey::LargeFileKb, Value::Number(v)) => match v.trim().parse() {
                    Ok(n) => cfg.large_file_kb = n,
                    Err(_) => {
                        errors.push(format!("large file warning must be a number, got {v:?}"));
                    }
                },
                (FieldKey::ShowHidden, Value::Toggle(b)) => cfg.show_hidden = *b,
                (FieldKey::Mouse, Value::Toggle(b)) => cfg.mouse = *b,
                (FieldKey::Highlighting, Value::Toggle(b)) => cfg.syntax_highlighting = *b,
                (FieldKey::TabWidth, Value::Number(v)) => match v.trim().parse() {
                    Ok(n) => cfg.tab_width = n,
                    Err(_) => errors.push(format!("tab width must be a number 1-255, got {v:?}")),
                },
                (FieldKey::Scrollback, Value::Number(v)) => match v.trim().parse() {
                    Ok(n) => cfg.scrollback_lines = n,
                    Err(_) => errors.push(format!("scrollback must be a number, got {v:?}")),
                },
                (FieldKey::StatusFile, Value::Text(v)) => v.trim().clone_into(&mut cfg.status_file),
                (FieldKey::ContextFile, Value::Text(v)) => {
                    v.trim().clone_into(&mut cfg.context_file);
                }
                _ => {}
            }
        }
        (cfg, errors)
    }
}

fn byte_index(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map_or(s.len(), |(i, _)| i)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn goto(form: &mut ConfigForm, key_: FieldKey) {
        form.handle_nav_key(key(KeyCode::Home));
        while form.fields()[form.selected_field()].key != key_ {
            form.handle_nav_key(key(KeyCode::Down));
        }
    }

    #[test]
    fn edits_text_field_and_builds_config() {
        let base = Config::default();
        let mut form = ConfigForm::new(&base, true);
        form.handle_nav_key(key(KeyCode::Enter));
        assert!(form.editing());
        form.handle_edit_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        for c in "/tmp/tasks".chars() {
            form.handle_edit_key(key(KeyCode::Char(c)));
        }
        form.handle_edit_key(key(KeyCode::Enter));
        assert!(!form.editing());
        assert!(form.dirty());
        let (cfg, errors) = form.to_config(&base);
        assert!(errors.is_empty());
        assert_eq!(cfg.tasks_dir, PathBuf::from("/tmp/tasks"));
    }

    #[test]
    fn list_fields_add_edit_delete() {
        let base = Config::default();
        let mut form = ConfigForm::new(&base, false);
        goto(&mut form, FieldKey::Categories);
        // Header → Enter jumps to "+ add" and starts editing.
        form.handle_nav_key(key(KeyCode::Enter));
        assert!(form.editing());
        form.paste("Blocked");
        form.handle_edit_key(key(KeyCode::Enter));
        assert_eq!(
            form.to_config(&base).0.categories,
            vec!["Planned", "Doing", "Done", "Blocked"]
        );
        assert!(matches!(form.selected_row(), Row::Add(_)));
        // Delete the first item.
        goto(&mut form, FieldKey::Categories);
        form.handle_nav_key(key(KeyCode::Down));
        assert!(matches!(form.selected_row(), Row::Item { item: 0, .. }));
        form.handle_nav_key(key(KeyCode::Char('d')));
        assert_eq!(
            form.to_config(&base).0.categories,
            vec!["Doing", "Done", "Blocked"]
        );
        // Edit an item to empty removes it.
        form.handle_nav_key(key(KeyCode::Enter));
        form.handle_edit_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        form.handle_edit_key(key(KeyCode::Enter));
        assert_eq!(form.to_config(&base).0.categories, vec!["Done", "Blocked"]);
    }

    #[test]
    fn workspaces_builds_sources_parse() {
        let base = Config::default();
        let mut form = ConfigForm::new(&base, false);
        goto(&mut form, FieldKey::Workspaces);
        form.handle_nav_key(key(KeyCode::Char('a')));
        form.paste("fw = /src/fw @develop");
        form.handle_edit_key(key(KeyCode::Enter));
        form.handle_nav_key(key(KeyCode::Char('a')));
        form.paste("app = ~/src/app");
        form.handle_edit_key(key(KeyCode::Enter));
        goto(&mut form, FieldKey::Builds);
        form.handle_nav_key(key(KeyCode::Char('a')));
        form.paste("yocto = /builds/yocto");
        form.handle_edit_key(key(KeyCode::Enter));
        goto(&mut form, FieldKey::TaskSources);
        form.handle_nav_key(key(KeyCode::Char('a')));
        form.paste("jira = ~/bin/jira-tasks.sh --mine");
        form.handle_edit_key(key(KeyCode::Enter));
        form.handle_nav_key(key(KeyCode::Char('a')));
        form.paste("broken");
        form.handle_edit_key(key(KeyCode::Enter));

        let (cfg, errors) = form.to_config(&base);
        assert_eq!(cfg.workspaces.len(), 2);
        assert_eq!(cfg.workspaces[0].name, "fw");
        assert_eq!(cfg.workspaces[0].path, PathBuf::from("/src/fw"));
        assert_eq!(cfg.workspaces[0].main_branch.as_deref(), Some("develop"));
        assert!(cfg.workspaces[1].main_branch.is_none());
        assert!(!cfg.workspaces[1].path.starts_with("~"));
        assert_eq!(cfg.builds[0].name, "yocto");
        assert_eq!(cfg.task_sources[0].command, "~/bin/jira-tasks.sh --mine");
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("broken"));
        // Round trip back into a form keeps the items.
        let again = ConfigForm::new(&cfg, false);
        let ws = again
            .fields()
            .iter()
            .find(|f| f.key == FieldKey::Workspaces)
            .unwrap();
        assert_eq!(
            ws.value,
            Value::List(vec![
                "fw = /src/fw @develop".into(),
                format!("app = {}", cfg.workspaces[1].path.display())
            ])
        );
    }

    #[test]
    fn toggles_cycles_and_numbers() {
        let base = Config::default();
        let mut form = ConfigForm::new(&base, false);
        goto(&mut form, FieldKey::ColorScheme);
        form.handle_nav_key(key(KeyCode::Right));
        assert_eq!(form.to_config(&base).0.color_scheme, ColorScheme::Light);
        form.handle_nav_key(key(KeyCode::Left));
        assert_eq!(form.to_config(&base).0.color_scheme, ColorScheme::Dark);
        goto(&mut form, FieldKey::ShowHidden);
        form.handle_nav_key(key(KeyCode::Enter));
        assert!(form.to_config(&base).0.show_hidden);
        goto(&mut form, FieldKey::TabWidth);
        form.handle_nav_key(key(KeyCode::Right));
        assert_eq!(form.to_config(&base).0.tab_width, 5);
        form.paste("x");
        form.handle_edit_key(key(KeyCode::Enter));
        let (cfg, errors) = form.to_config(&base);
        assert_eq!(cfg.tab_width, base.tab_width);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn escape_cancels_edit_and_mark_saved_clears_dirty() {
        let base = Config::default();
        let mut form = ConfigForm::new(&base, false);
        form.handle_nav_key(key(KeyCode::Enter));
        form.handle_edit_key(key(KeyCode::Char('x')));
        form.handle_edit_key(key(KeyCode::Esc));
        assert!(!form.dirty());
        form.handle_nav_key(key(KeyCode::Enter));
        form.handle_edit_key(key(KeyCode::Char('x')));
        form.handle_edit_key(key(KeyCode::Enter));
        assert!(form.dirty());
        form.mark_saved();
        assert!(!form.dirty());
        assert!(!form.first_run());
    }
}
