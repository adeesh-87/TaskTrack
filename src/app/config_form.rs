//! The settings page: scalar fields plus editable lists, backed by [`Config`].

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use std::path::PathBuf;

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
    /// Gerrit web URL override.
    GerritUrl,
    /// One-shot agent program.
    AgentCommand,
    /// One-shot agent arguments.
    AgentArgs,
    /// One-shot agent timeout.
    AgentTimeout,
    /// Word limit for generated context.
    ContextWords,
    /// Context prompt template path.
    ContextPrompt,
    /// Checkpoint prompt template path.
    CheckpointPrompt,
    /// Audit prompt path.
    AuditPrompt,
    /// Command listing my Gerrit changes.
    AuditGerrit,
    /// Days the audit looks back.
    AuditSince,
    /// Let the agent match what the rules could not.
    AuditAgent,
    /// Extra "done" statuses.
    AuditDone,
    /// Extra "in progress" statuses.
    AuditProgress,
    /// Coding agent program.
    CodingCommand,
    /// Coding agent arguments.
    CodingArgs,
    /// Coding agent startup prompt path.
    CodingPrompt,
    /// Start the coding agent in the code workspace.
    CodingStartInCode,
    /// Focus block minutes.
    FocusMinutes,
    /// Planned minutes per day.
    PlannerDay,
    /// Columns offered for planning.
    PlannerColumns,
    /// Flash on timer end.
    TimerFlash,
    /// Bell on timer end.
    TimerBell,
    /// Idle pause minutes.
    IdleMinutes,
    /// Hooks, `event = command`.
    Hooks,
    /// Hook timeout.
    HookTimeout,
    /// Minutes between `periodic` hook runs.
    PeriodicMinutes,
    /// Gerrit status command.
    GerritStatus,
    /// Archive finished tasks after N days.
    ArchiveDays,
    /// Soft wrap in the editor.
    SoftWrap,
    /// Clipboard copy command.
    CopyCommand,
    /// Clipboard paste command.
    PasteCommand,
    /// Reopen shells after a restart.
    RestoreShells,
    /// Run shells inside tmux.
    ShellTmux,
    /// Key overrides.
    Keys,
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
    pub help: String,
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

fn field(key: FieldKey, label: &'static str, help: impl Into<String>, value: Value) -> Field {
    Field {
        key,
        label,
        help: help.into(),
        value,
    }
}

/// Marker shown after a workspace branch that comes from the default.
const DEFAULT_MARK: &str = " (default)";

fn workspace_item(w: &Workspace, default_branch: &str) -> String {
    match &w.main_branch {
        Some(b) => format!("{} = {} @{b}", w.name, w.path.display()),
        None => format!(
            "{} = {} @{default_branch}{DEFAULT_MARK}",
            w.name,
            w.path.display()
        ),
    }
}

/// Parse `name = /path [@branch]`; `@branch (default)` means "no own branch".
fn parse_workspace(item: &str) -> Result<Workspace, String> {
    let (name, rest) = item
        .split_once('=')
        .ok_or_else(|| format!("workspace {item:?}: expected `name = /path [@branch]`"))?;
    let rest = rest.trim();
    if let Some(stripped) = rest.strip_suffix(DEFAULT_MARK) {
        let path = stripped
            .rsplit_once(" @")
            .map_or(stripped, |(p, _)| p)
            .trim();
        if path.is_empty() {
            return Err(format!("workspace {item:?}: missing path"));
        }
        return Ok(Workspace {
            name: name.trim().to_owned(),
            path: Config::expand_tilde(path),
            main_branch: None,
        });
    }
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
pub fn parse_source(item: &str) -> Result<TaskSource, String> {
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
                "`name = /path/to/checkout @main-branch` — each workspace has its own main branch (edit the @part; \"(default)\" follows Default main branch; new ones are detected from git). Attach with Esc a.",
                Value::List(
                    cfg.workspaces
                        .iter()
                        .map(|w| workspace_item(w, &cfg.default_main_branch))
                        .collect(),
                ),
            ),
            field(
                FieldKey::MainBranch,
                "Default main branch",
                "Main branch for workspaces marked (default).",
                Value::Text(cfg.default_main_branch.clone()),
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
                "`name = /path/to/script [args]` e.g. `jira = ~/bin/jira-mine.sh`. ? shows the output format · t on a source runs it now.",
                Value::List(cfg.task_sources.iter().map(|t| format!("{} = {}", t.name, t.command)).collect()),
            ),
            field(
                FieldKey::Hooks,
                "Hooks",
                "`event = command`, e.g. `task_enter = ~/bin/on-enter.sh` or `timer_expire = notify-send pahiri done`. ? lists events and variables.",
                Value::List(cfg.hooks.iter().map(|(k, v)| format!("{k} = {v}")).collect()),
            ),
            field(
                FieldKey::HookTimeout,
                "Hook timeout (s)",
                "Give up on a hook after this many seconds.",
                Value::Number(cfg.hook_timeout_secs.to_string()),
            ),
            field(
                FieldKey::PeriodicMinutes,
                "Periodic hook (min)",
                "Run the `periodic` hook every this many minutes (0: never) — for scripts that keep things in sync, like examples/hooks/discover-workspaces.sh.",
                Value::Number(cfg.periodic_minutes.to_string()),
            ),
            field(
                FieldKey::GerritUrl,
                "Gerrit URL",
                "Web URL of your Gerrit, e.g. https://review.example.com. Empty: derived from each workspace's origin remote. Used by Esc g.",
                Value::Text(cfg.gerrit_url.clone()),
            ),
            field(
                FieldKey::GerritStatus,
                "Gerrit status command",
                "Optional. Gets the Change-Ids as arguments and prints JSON lines {change_id, number, status, url, labels}. ? shows the format.",
                Value::Text(cfg.gerrit_status_command.clone()),
            ),
            field(
                FieldKey::AgentCommand,
                "Agent command",
                "One-shot AI agent for Esc i (context) and Esc b (checkpoints), e.g. claude. Empty disables. ? for details.",
                Value::Text(cfg.agent.command.clone()),
            ),
            field(
                FieldKey::AgentArgs,
                "Agent arguments",
                "Shell-style. {prompt} receives the prompt, else it goes to stdin. e.g. -p {prompt}",
                Value::Text(shell_words::join(&cfg.agent.args)),
            ),
            field(
                FieldKey::AgentTimeout,
                "Agent timeout (s)",
                "Give up on the agent after this many seconds (Esc cancels earlier).",
                Value::Number(cfg.agent.timeout_secs.to_string()),
            ),
            field(
                FieldKey::ContextWords,
                "Context word limit",
                "Maximum words of generated context (1-1000). Less is more.",
                Value::Number(cfg.agent.context_max_words.to_string()),
            ),
            field(
                FieldKey::ContextPrompt,
                "Context prompt file",
                "Template for Esc i. Empty: prompts/context.md next to the config file. Esc E in a task edits it.",
                Value::Text(cfg.agent.context_prompt.display().to_string()),
            ),
            field(
                FieldKey::CheckpointPrompt,
                "Checkpoint prompt file",
                "Template for Esc b. Empty: prompts/checkpoints.md next to the config file.",
                Value::Text(cfg.agent.checkpoint_prompt.display().to_string()),
            ),
            field(
                FieldKey::AuditPrompt,
                "Audit prompt file",
                "Template for the audit's agent step (Esc U). Empty: prompts/audit.md next to the config file.",
                Value::Text(cfg.agent.audit_prompt.display().to_string()),
            ),
            field(
                FieldKey::AuditGerrit,
                "Audit: Gerrit command",
                "Prints your Gerrit changes since $PAHIRI_AUDIT_SINCE as JSON lines (? shows the format; examples/gerrit-mine.sh).",
                Value::Text(cfg.audit.gerrit_command.clone()),
            ),
            field(
                FieldKey::AuditSince,
                "Audit: days back",
                "How far back the audit looks (scripts get PAHIRI_AUDIT_SINCE).",
                Value::Number(cfg.audit.since_days.to_string()),
            ),
            field(
                FieldKey::AuditAgent,
                "Audit: use the agent",
                "Let the agent match the changes and tickets the rules could not (you still review everything).",
                Value::Toggle(cfg.audit.use_agent),
            ),
            field(
                FieldKey::AuditDone,
                "Audit: done statuses",
                "Ticket statuses that mean finished, besides Done, Closed, Resolved, Fixed, Complete(d), Verified, Released.",
                Value::List(cfg.audit.done_statuses.clone()),
            ),
            field(
                FieldKey::AuditProgress,
                "Audit: in-progress statuses",
                "Ticket statuses that mean being worked on, besides In Progress, In Review, Review, Code Review, Testing, …",
                Value::List(cfg.audit.progress_statuses.clone()),
            ),
            field(
                FieldKey::CodingCommand,
                "Coding agent command",
                "Interactive agent started in a new task shell by leader a / Esc l, e.g. claude, codex, aider. Empty disables.",
                Value::Text(cfg.coding_agent.command.clone()),
            ),
            field(
                FieldKey::CodingArgs,
                "Coding agent arguments",
                "Shell-style. {prompt} receives the startup prompt, else it is appended as the last argument.",
                Value::Text(shell_words::join(&cfg.coding_agent.args)),
            ),
            field(
                FieldKey::CodingPrompt,
                "Coding agent prompt file",
                "Startup prompt template. Empty: prompts/coding-agent.md next to the config file.",
                Value::Text(cfg.coding_agent.startup_prompt.display().to_string()),
            ),
            field(
                FieldKey::CodingStartInCode,
                "Coding agent in code dir",
                "Start the coding agent in the first attached code workspace (off: the task folder).",
                Value::Toggle(cfg.coding_agent.start_in_code),
            ),
            field(
                FieldKey::FocusMinutes,
                "Focus block (min)",
                "Timer length for tasks without checkpoints.",
                Value::Number(cfg.timer.focus_minutes.to_string()),
            ),
            field(
                FieldKey::PlannerDay,
                "Day capacity",
                "How much planned work fits in a day, e.g. 6h or 5h30m. The Plan view fills up to this.",
                Value::Text(crate::tasks::checkpoints::fmt_minutes(cfg.planner.day_minutes)),
            ),
            field(
                FieldKey::PlannerColumns,
                "Plan from columns",
                "Columns whose tasks the Plan view offers (Enter on '+ add' adds, d deletes). Empty: all but the last column, and but the first when there are three or more.",
                Value::List(cfg.planner.columns.clone()),
            ),
            field(
                FieldKey::IdleMinutes,
                "Idle pause (min)",
                "Pause the timer after this many minutes without a key press or click (0: never). You choose whether the gap counts.",
                Value::Number(cfg.timer.idle_minutes.to_string()),
            ),
            field(
                FieldKey::TimerFlash,
                "Flash when time is up",
                "Flash the whole screen when a checkpoint's time runs out.",
                Value::Toggle(cfg.timer.flash),
            ),
            field(
                FieldKey::TimerBell,
                "Bell when time is up",
                "Ring the terminal bell (many terminals turn it into a notification).",
                Value::Toggle(cfg.timer.bell),
            ),
            field(
                FieldKey::ArchiveDays,
                "Archive after (days)",
                "Hide tasks finished more than this many days ago (A in the list shows them; 0: never).",
                Value::Number(cfg.archive_after_days.to_string()),
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
                FieldKey::RestoreShells,
                "Restore shells",
                "Reopen each task's shells, in the same folders, after pahiri restarts.",
                Value::Toggle(cfg.restore_shells),
            ),
            field(
                FieldKey::ShellTmux,
                "Shells in tmux",
                "Run every shell in its own tmux session so programs keep running when pahiri exits (needs tmux ≥ 3.0).",
                Value::Toggle(cfg.shell.tmux),
            ),
            field(
                FieldKey::LeaderKey,
                "Leader key",
                "Prefix intercepted while a shell has focus (tmux style). Press it twice to send it through.",
                Value::Text(cfg.leader_key.clone()),
            ),
            field(
                FieldKey::Keys,
                "Key overrides",
                "`palette.<action> = x` or `leader.<command> = x`, e.g. `palette.timer = u`. ? lists every name.",
                Value::List(cfg.keys.iter().map(|(k, v)| format!("{k} = {v}")).collect()),
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
                "Click to focus and select, drag to select in the editor, wheel to scroll, forwarded to programs that ask. Shift+drag uses the terminal's own selection. Takes effect on restart.",
                Value::Toggle(cfg.mouse),
            ),
            field(
                FieldKey::Highlighting,
                "Syntax highlighting",
                "Colour C, C++, Rust, Bash, Python, CMake, Makefiles, logs and Markdown in the editor.",
                Value::Toggle(cfg.syntax_highlighting),
            ),
            field(
                FieldKey::SoftWrap,
                "Soft wrap",
                "Wrap long lines of Markdown and plain text in the editor.",
                Value::Toggle(cfg.soft_wrap),
            ),
            field(
                FieldKey::CopyCommand,
                "Copy command",
                "Gets copied text on stdin, e.g. wl-copy, xclip -selection clipboard, pbcopy. Empty: the terminal's clipboard via OSC 52.",
                Value::Text(cfg.copy_command.clone()),
            ),
            field(
                FieldKey::PasteCommand,
                "Paste command",
                "Ctrl+V in the editor pastes its output, e.g. wl-paste -n, xclip -o -selection clipboard, pbpaste. Empty: what pahiri copied last. The terminal's own paste always works.",
                Value::Text(cfg.paste_command.clone()),
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

    /// The text of the selected list item, when it belongs to field `key`.
    pub fn selected_item(&self, key: FieldKey) -> Option<String> {
        match self.selected_row() {
            Row::Item { field, item } if self.fields[field].key == key => {
                match &self.fields[field].value {
                    Value::List(items) => items.get(item).cloned(),
                    _ => None,
                }
            }
            _ => None,
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
                    let value = if self.fields[fi].key == FieldKey::Workspaces {
                        with_detected_branch(value)
                    } else {
                        value
                    };
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
                (FieldKey::GerritUrl, Value::Text(v)) => v.trim().clone_into(&mut cfg.gerrit_url),
                (FieldKey::AgentCommand, Value::Text(v)) => {
                    v.trim().clone_into(&mut cfg.agent.command);
                }
                (FieldKey::AgentArgs, Value::Text(v)) => match shell_words::split(v) {
                    Ok(a) => cfg.agent.args = a,
                    Err(e) => errors.push(format!("agent arguments: {e}")),
                },
                (FieldKey::AgentTimeout, Value::Number(v)) => match v.trim().parse() {
                    Ok(n) => cfg.agent.timeout_secs = n,
                    Err(_) => errors.push(format!("agent timeout must be a number, got {v:?}")),
                },
                (FieldKey::ContextWords, Value::Number(v)) => match v.trim().parse() {
                    Ok(n) => cfg.agent.context_max_words = n,
                    Err(_) => {
                        errors.push(format!("context word limit must be a number, got {v:?}"));
                    }
                },
                (FieldKey::ContextPrompt, Value::Text(v)) => {
                    cfg.agent.context_prompt = path_or_empty(v);
                }
                (FieldKey::CheckpointPrompt, Value::Text(v)) => {
                    cfg.agent.checkpoint_prompt = path_or_empty(v);
                }
                (FieldKey::AuditPrompt, Value::Text(v)) => {
                    cfg.agent.audit_prompt = path_or_empty(v);
                }
                (FieldKey::AuditGerrit, Value::Text(v)) => {
                    v.trim().clone_into(&mut cfg.audit.gerrit_command);
                }
                (FieldKey::AuditSince, Value::Number(v)) => match v.trim().parse() {
                    Ok(n) if n > 0 => cfg.audit.since_days = n,
                    _ => errors.push(format!(
                        "audit days back must be a number above 0, got {v:?}"
                    )),
                },
                (FieldKey::AuditAgent, Value::Toggle(b)) => cfg.audit.use_agent = *b,
                (FieldKey::AuditDone, Value::List(items)) => {
                    cfg.audit.done_statuses = items
                        .iter()
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                (FieldKey::AuditProgress, Value::List(items)) => {
                    cfg.audit.progress_statuses = items
                        .iter()
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                (FieldKey::CodingCommand, Value::Text(v)) => {
                    v.trim().clone_into(&mut cfg.coding_agent.command);
                }
                (FieldKey::CodingArgs, Value::Text(v)) => match shell_words::split(v) {
                    Ok(a) => cfg.coding_agent.args = a,
                    Err(e) => errors.push(format!("coding agent arguments: {e}")),
                },
                (FieldKey::CodingPrompt, Value::Text(v)) => {
                    cfg.coding_agent.startup_prompt = path_or_empty(v);
                }
                (FieldKey::CodingStartInCode, Value::Toggle(b)) => {
                    cfg.coding_agent.start_in_code = *b;
                }
                (FieldKey::FocusMinutes, Value::Number(v)) => match v.trim().parse() {
                    Ok(n) => cfg.timer.focus_minutes = n,
                    Err(_) => errors.push(format!("focus block must be a number, got {v:?}")),
                },
                (FieldKey::PlannerDay, Value::Text(v)) => {
                    match crate::tasks::checkpoints::parse_minutes(v.trim()) {
                        Some(n) if n > 0 => cfg.planner.day_minutes = n,
                        _ => {
                            errors.push(format!("day capacity must be like 6h or 330m, got {v:?}"));
                        }
                    }
                }
                (FieldKey::PlannerColumns, Value::List(items)) => {
                    cfg.planner.columns = items
                        .iter()
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                (FieldKey::TimerFlash, Value::Toggle(b)) => cfg.timer.flash = *b,
                (FieldKey::TimerBell, Value::Toggle(b)) => cfg.timer.bell = *b,
                (FieldKey::IdleMinutes, Value::Number(v)) => match v.trim().parse() {
                    Ok(n) => cfg.timer.idle_minutes = n,
                    Err(_) => errors.push(format!("idle pause must be a number, got {v:?}")),
                },
                (FieldKey::Hooks, Value::List(items)) => {
                    cfg.hooks.clear();
                    for item in items {
                        match item.split_once('=') {
                            Some((k, v)) => {
                                cfg.hooks.insert(k.trim().to_owned(), v.trim().to_owned());
                            }
                            None => {
                                errors.push(format!("hook {item:?}: expected `event = command`"));
                            }
                        }
                    }
                }
                (FieldKey::PeriodicMinutes, Value::Number(v)) => match v.trim().parse() {
                    Ok(n) => cfg.periodic_minutes = n,
                    Err(_) => {
                        errors.push(format!("periodic hook minutes must be a number, got {v:?}"));
                    }
                },
                (FieldKey::HookTimeout, Value::Number(v)) => match v.trim().parse() {
                    Ok(n) => cfg.hook_timeout_secs = n,
                    Err(_) => errors.push(format!("hook timeout must be a number, got {v:?}")),
                },
                (FieldKey::GerritStatus, Value::Text(v)) => {
                    v.trim().clone_into(&mut cfg.gerrit_status_command);
                }
                (FieldKey::ArchiveDays, Value::Number(v)) => match v.trim().parse() {
                    Ok(n) => cfg.archive_after_days = n,
                    Err(_) => errors.push(format!("archive days must be a number, got {v:?}")),
                },
                (FieldKey::SoftWrap, Value::Toggle(b)) => cfg.soft_wrap = *b,
                (FieldKey::CopyCommand, Value::Text(v)) => {
                    v.trim().clone_into(&mut cfg.copy_command);
                }
                (FieldKey::PasteCommand, Value::Text(v)) => {
                    v.trim().clone_into(&mut cfg.paste_command);
                }
                (FieldKey::RestoreShells, Value::Toggle(b)) => cfg.restore_shells = *b,
                (FieldKey::ShellTmux, Value::Toggle(b)) => cfg.shell.tmux = *b,
                (FieldKey::Keys, Value::List(items)) => {
                    cfg.keys.clear();
                    for item in items {
                        match item.split_once('=') {
                            Some((k, v)) => {
                                cfg.keys.insert(k.trim().to_owned(), v.trim().to_owned());
                            }
                            None => errors.push(format!(
                                "key override {item:?}: expected `palette.<action> = x`"
                            )),
                        }
                    }
                }
                _ => {}
            }
        }
        (cfg, errors)
    }
}

fn path_or_empty(v: &str) -> PathBuf {
    let v = v.trim();
    if v.is_empty() {
        PathBuf::new()
    } else {
        Config::expand_tilde(v)
    }
}

/// A new `name = /path` workspace without `@branch` gets the branch git reports.
fn with_detected_branch(item: String) -> String {
    let Ok(w) = parse_workspace(&item) else {
        return item;
    };
    if w.main_branch.is_some() || item.contains(" @") {
        return item;
    }
    match crate::git::detect_main_branch(&w.path) {
        Some(b) => format!("{} @{b}", item.trim_end()),
        None => item,
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
                format!("app = {} @main (default)", cfg.workspaces[1].path.display())
            ])
        );
        // "(default)" parses back to "no own branch".
        let (again_cfg, errors) = again.to_config(&cfg);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(again_cfg.workspaces, cfg.workspaces);
    }

    #[test]
    fn new_workspace_gets_detected_main_branch_and_agent_fields_parse() {
        let repo = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(repo.path())
                .args(args)
                .output()
                .unwrap()
                .status
                .success());
        };
        git(&["init", "-q", "-b", "develop"]);
        git(&[
            "-c",
            "user.email=t@x",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "i",
        ]);
        let base = Config::default();
        let mut form = ConfigForm::new(&base, false);
        goto(&mut form, FieldKey::Workspaces);
        form.handle_nav_key(key(KeyCode::Char('a')));
        form.paste(&format!("fw = {}", repo.path().display()));
        form.handle_edit_key(key(KeyCode::Enter));
        let (cfg, _) = form.to_config(&base);
        assert_eq!(cfg.workspaces[0].main_branch.as_deref(), Some("develop"));

        goto(&mut form, FieldKey::AgentArgs);
        form.handle_nav_key(key(KeyCode::Enter));
        form.handle_edit_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        form.paste("--model 'big one' -p {prompt}");
        form.handle_edit_key(key(KeyCode::Enter));
        let (cfg, errors) = form.to_config(&base);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(cfg.agent.args, vec!["--model", "big one", "-p", "{prompt}"]);
        goto(&mut form, FieldKey::TaskSources);
        assert!(form.selected_item(FieldKey::TaskSources).is_none());
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
