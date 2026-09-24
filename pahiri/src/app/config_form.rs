//! The settings page: an editable list of fields backed by [`Config`].

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::{ColorScheme, Config};

/// Which configuration value a field edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKey {
    /// Tasks folder.
    TasksDir,
    /// Comma separated categories.
    Categories,
    /// Colour scheme.
    ColorScheme,
    /// Shell program.
    ShellProgram,
    /// Shell arguments.
    ShellArgs,
    /// Font family (advisory).
    FontFamily,
    /// Leader key.
    LeaderKey,
    /// Large-file threshold.
    LargeFileKb,
    /// Show hidden files.
    ShowHidden,
    /// Editor tab width.
    TabWidth,
    /// Scrollback lines.
    Scrollback,
    /// Board file name.
    StatusFile,
    /// Context file name.
    ContextFile,
}

/// How a field is edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// Free text (Enter to edit).
    Text,
    /// Numeric text (Enter to edit, ←/→ to nudge).
    Number,
    /// Boolean (Enter/Space/←/→ toggles).
    Toggle,
    /// Colour scheme (Enter/←/→ cycles).
    Scheme,
}

/// One row of the settings page.
#[derive(Debug, Clone)]
pub struct Field {
    /// Which value.
    pub key: FieldKey,
    /// Label shown on the left.
    pub label: &'static str,
    /// Explanation shown for the selected field.
    pub help: &'static str,
    /// Editing behaviour.
    pub kind: FieldKind,
    /// Current text value.
    pub value: String,
}

/// State of the settings page.
#[derive(Debug, Clone)]
pub struct ConfigForm {
    fields: Vec<Field>,
    selected: usize,
    /// Edit buffer and cursor (char index) while editing a text field.
    editing: Option<(String, usize)>,
    errors: Vec<String>,
    first_run: bool,
    saved: Vec<String>,
}

fn field(
    key: FieldKey,
    label: &'static str,
    help: &'static str,
    kind: FieldKind,
    value: String,
) -> Field {
    Field {
        key,
        label,
        help,
        kind,
        value,
    }
}

impl ConfigForm {
    /// Build the form from a config. `first_run` shows the welcome banner and
    /// prevents leaving until a valid config is saved.
    pub fn new(cfg: &Config, first_run: bool) -> Self {
        let fields = vec![
            field(
                FieldKey::TasksDir,
                "Tasks folder",
                "Folder containing one sub-folder per task (each with a CONTEXT.md). Required.",
                FieldKind::Text,
                cfg.tasks_dir.display().to_string(),
            ),
            field(
                FieldKey::Categories,
                "Categories",
                "Board columns, comma separated, in display order. At least one is required.",
                FieldKind::Text,
                cfg.categories.join(", "),
            ),
            field(
                FieldKey::ColorScheme,
                "Colour scheme",
                "UI palette. Shell colours always come from the programs themselves.",
                FieldKind::Scheme,
                cfg.color_scheme.to_string(),
            ),
            field(
                FieldKey::ShellProgram,
                "Shell",
                "Program launched for task terminals (zsh by default).",
                FieldKind::Text,
                cfg.shell.program.clone(),
            ),
            field(
                FieldKey::ShellArgs,
                "Shell arguments",
                "Space separated arguments, e.g. \"-i\" or \"-l\".",
                FieldKind::Text,
                cfg.shell.args.join(" "),
            ),
            field(
                FieldKey::LeaderKey,
                "Leader key",
                "Prefix intercepted while a shell has focus (tmux style). Press it twice to send it through.",
                FieldKind::Text,
                cfg.leader_key.clone(),
            ),
            field(
                FieldKey::FontFamily,
                "Font family",
                "Advisory: your terminal emulator owns the font. Recorded here and shown in the status bar.",
                FieldKind::Text,
                cfg.font_family.clone(),
            ),
            field(
                FieldKey::LargeFileKb,
                "Large file warning (KiB)",
                "Files bigger than this ask before opening. Binary files always ask.",
                FieldKind::Number,
                cfg.large_file_kb.to_string(),
            ),
            field(
                FieldKey::ShowHidden,
                "Show hidden files",
                "Show dot-files in the task file tree ('.' toggles at runtime).",
                FieldKind::Toggle,
                cfg.show_hidden.to_string(),
            ),
            field(
                FieldKey::TabWidth,
                "Tab width",
                "Columns per tab character in the editor.",
                FieldKind::Number,
                cfg.tab_width.to_string(),
            ),
            field(
                FieldKey::Scrollback,
                "Scrollback lines",
                "History kept per shell for Shift+PageUp scrolling.",
                FieldKind::Number,
                cfg.scrollback_lines.to_string(),
            ),
            field(
                FieldKey::StatusFile,
                "Status file",
                "Markdown board file inside the tasks folder (one heading per category).",
                FieldKind::Text,
                cfg.status_file.clone(),
            ),
            field(
                FieldKey::ContextFile,
                "Context file",
                "File each task folder is expected to contain.",
                FieldKind::Text,
                cfg.context_file.clone(),
            ),
        ];
        let saved = fields.iter().map(|f| f.value.clone()).collect();
        Self {
            fields,
            selected: 0,
            editing: None,
            errors: Vec::new(),
            first_run,
            saved,
        }
    }

    /// All fields.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// Selected field index.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Whether a text field is being edited.
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
        self.fields.iter().map(|f| &f.value).ne(self.saved.iter())
    }

    /// Replace the error list.
    pub fn set_errors(&mut self, errors: Vec<String>) {
        self.errors = errors;
    }

    /// Record the current values as saved.
    pub fn mark_saved(&mut self) {
        self.saved = self.fields.iter().map(|f| f.value.clone()).collect();
        self.first_run = false;
    }

    /// Handle a key while not editing.
    pub fn handle_nav_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.selected = (self.selected + 1) % self.fields.len();
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                self.selected = (self.selected + self.fields.len() - 1) % self.fields.len();
            }
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = self.fields.len() - 1,
            KeyCode::Enter | KeyCode::Char('e') | KeyCode::Char(' ') => match self.current().kind {
                FieldKind::Text | FieldKind::Number => {
                    let v = self.current().value.clone();
                    let len = v.chars().count();
                    self.editing = Some((v, len));
                }
                FieldKind::Toggle => self.toggle(),
                FieldKind::Scheme => self.cycle_scheme(true),
            },
            KeyCode::Right | KeyCode::Char('l') => self.nudge(true),
            KeyCode::Left | KeyCode::Char('h') => self.nudge(false),
            _ => {}
        }
    }

    /// Handle a key while editing a text field.
    pub fn handle_edit_key(&mut self, key: KeyEvent) {
        let Some((buf, cursor)) = &mut self.editing else {
            return;
        };
        match (key.code, key.modifiers) {
            (KeyCode::Enter, _) => {
                let (value, _) = self.editing.take().expect("editing");
                self.fields[self.selected].value = value;
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

    /// Insert pasted text into the edit buffer (starts editing a text field if needed).
    pub fn paste(&mut self, text: &str) {
        let text: String = text.lines().next().unwrap_or("").to_owned();
        if self.editing.is_none()
            && matches!(self.current().kind, FieldKind::Text | FieldKind::Number)
        {
            let v = self.current().value.clone();
            let len = v.chars().count();
            self.editing = Some((v, len));
        }
        if let Some((buf, cursor)) = &mut self.editing {
            let idx = byte_index(buf, *cursor);
            buf.insert_str(idx, &text);
            *cursor += text.chars().count();
        }
    }

    fn current(&self) -> &Field {
        &self.fields[self.selected]
    }

    fn toggle(&mut self) {
        let f = &mut self.fields[self.selected];
        f.value = if f.value == "true" {
            "false".into()
        } else {
            "true".into()
        };
    }

    fn cycle_scheme(&mut self, forward: bool) {
        let f = &mut self.fields[self.selected];
        let current = parse_scheme(&f.value).unwrap_or_default();
        f.value = if forward {
            current.next()
        } else {
            current.prev()
        }
        .to_string();
    }

    fn nudge(&mut self, up: bool) {
        match self.current().kind {
            FieldKind::Toggle => self.toggle(),
            FieldKind::Scheme => self.cycle_scheme(up),
            FieldKind::Number => {
                let f = &mut self.fields[self.selected];
                if let Ok(n) = f.value.trim().parse::<i64>() {
                    let step = if n >= 1000 {
                        100
                    } else if n >= 100 {
                        10
                    } else {
                        1
                    };
                    let next = if up { n + step } else { (n - step).max(0) };
                    f.value = next.to_string();
                }
            }
            FieldKind::Text => {}
        }
    }

    /// Build a config from the form. Fields that fail to parse keep `base`'s value
    /// and are reported in the returned error list.
    pub fn to_config(&self, base: &Config) -> (Config, Vec<String>) {
        let mut cfg = base.clone();
        let mut errors = Vec::new();
        for f in &self.fields {
            let v = f.value.trim();
            match f.key {
                FieldKey::TasksDir => cfg.tasks_dir = Config::expand_tilde(v),
                FieldKey::Categories => {
                    cfg.categories = v
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect();
                }
                FieldKey::ColorScheme => match parse_scheme(v) {
                    Some(s) => cfg.color_scheme = s,
                    None => errors.push(format!("unknown colour scheme {v:?}")),
                },
                FieldKey::ShellProgram => v.clone_into(&mut cfg.shell.program),
                FieldKey::ShellArgs => {
                    cfg.shell.args = v.split_whitespace().map(str::to_owned).collect();
                }
                FieldKey::FontFamily => v.clone_into(&mut cfg.font_family),
                FieldKey::LeaderKey => v.clone_into(&mut cfg.leader_key),
                FieldKey::LargeFileKb => match v.parse() {
                    Ok(n) => cfg.large_file_kb = n,
                    Err(_) => {
                        errors.push(format!("large file warning must be a number, got {v:?}"));
                    }
                },
                FieldKey::ShowHidden => cfg.show_hidden = v == "true",
                FieldKey::TabWidth => match v.parse() {
                    Ok(n) => cfg.tab_width = n,
                    Err(_) => errors.push(format!("tab width must be a number 1-255, got {v:?}")),
                },
                FieldKey::Scrollback => match v.parse() {
                    Ok(n) => cfg.scrollback_lines = n,
                    Err(_) => errors.push(format!("scrollback must be a number, got {v:?}")),
                },
                FieldKey::StatusFile => v.clone_into(&mut cfg.status_file),
                FieldKey::ContextFile => v.clone_into(&mut cfg.context_file),
            }
        }
        (cfg, errors)
    }
}

fn parse_scheme(s: &str) -> Option<ColorScheme> {
    ColorScheme::ALL
        .iter()
        .copied()
        .find(|c| c.to_string() == s.trim().to_lowercase())
}

fn byte_index(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map_or(s.len(), |(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
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
        assert_eq!(cfg.tasks_dir, std::path::PathBuf::from("/tmp/tasks"));
    }

    #[test]
    fn categories_are_split_and_trimmed() {
        let base = Config::default();
        let mut form = ConfigForm::new(&base, false);
        form.handle_nav_key(key(KeyCode::Down));
        form.handle_nav_key(key(KeyCode::Enter));
        form.handle_edit_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        form.paste("Todo ,Doing,, Done");
        form.handle_edit_key(key(KeyCode::Enter));
        let (cfg, _) = form.to_config(&base);
        assert_eq!(cfg.categories, vec!["Todo", "Doing", "Done"]);
    }

    #[test]
    fn toggles_and_cycles() {
        let base = Config::default();
        let mut form = ConfigForm::new(&base, false);
        let scheme_idx = form
            .fields()
            .iter()
            .position(|f| f.key == FieldKey::ColorScheme)
            .unwrap();
        for _ in 0..scheme_idx {
            form.handle_nav_key(key(KeyCode::Down));
        }
        form.handle_nav_key(key(KeyCode::Right));
        assert_eq!(form.to_config(&base).0.color_scheme, ColorScheme::Light);
        form.handle_nav_key(key(KeyCode::Left));
        assert_eq!(form.to_config(&base).0.color_scheme, ColorScheme::Dark);

        let hidden_idx = form
            .fields()
            .iter()
            .position(|f| f.key == FieldKey::ShowHidden)
            .unwrap();
        form.handle_nav_key(key(KeyCode::Home));
        for _ in 0..hidden_idx {
            form.handle_nav_key(key(KeyCode::Down));
        }
        form.handle_nav_key(key(KeyCode::Enter));
        assert!(form.to_config(&base).0.show_hidden);
    }

    #[test]
    fn bad_numbers_are_reported() {
        let base = Config::default();
        let mut form = ConfigForm::new(&base, false);
        let idx = form
            .fields()
            .iter()
            .position(|f| f.key == FieldKey::TabWidth)
            .unwrap();
        for _ in 0..idx {
            form.handle_nav_key(key(KeyCode::Down));
        }
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
