//! Configuration model, persistence and validation.

pub mod keybind;

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use self::keybind::KeyCombo;

/// Name of the application, used for config and state directories.
pub const APP_NAME: &str = "pahiri";

/// Errors raised while loading or saving configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Reading or writing the config file failed.
    #[error("io error at {path}: {source}")]
    Io {
        /// The offending path.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The config file exists but is not valid TOML for [`Config`].
    #[error("invalid config at {path}: {message}")]
    Parse {
        /// The offending path.
        path: PathBuf,
        /// Human readable parse error.
        message: String,
    },
    /// Serialising the config failed (should not happen in practice).
    #[error("could not serialise config: {0}")]
    Serialize(String),
}

/// Built-in colour schemes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ColorScheme {
    /// Neutral dark palette (default).
    #[default]
    Dark,
    /// Light palette for bright terminals.
    Light,
    /// Warm, retro palette.
    Gruvbox,
    /// Cool, arctic palette.
    Nord,
    /// Solarized dark.
    Solarized,
}

impl ColorScheme {
    /// Every scheme, in the order the config page cycles through them.
    pub const ALL: [ColorScheme; 5] = [
        ColorScheme::Dark,
        ColorScheme::Light,
        ColorScheme::Gruvbox,
        ColorScheme::Nord,
        ColorScheme::Solarized,
    ];

    /// The scheme following this one (wrapping).
    #[must_use]
    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    /// The scheme preceding this one (wrapping).
    #[must_use]
    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

impl fmt::Display for ColorScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ColorScheme::Dark => "dark",
            ColorScheme::Light => "light",
            ColorScheme::Gruvbox => "gruvbox",
            ColorScheme::Nord => "nord",
            ColorScheme::Solarized => "solarized",
        };
        f.write_str(s)
    }
}

/// How shells are launched inside a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellConfig {
    /// Program to run, e.g. `zsh`.
    pub program: String,
    /// Extra arguments, e.g. `["-l"]`.
    pub args: Vec<String>,
    /// Run each shell inside its own tmux session (socket `pahiri`), so shells
    /// and the programs in them keep running when pahiri exits.
    pub tmux: bool,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            program: "zsh".to_owned(),
            args: vec!["-i".to_owned()],
            tmux: false,
        }
    }
}

/// A code checkout that can be attached to tasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    /// Short name used in the UI and in `cd code <name>`.
    pub name: String,
    /// Path to the git checkout.
    pub path: PathBuf,
    /// Main branch of this checkout; falls back to `default_main_branch`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_branch: Option<String>,
}

/// A vendor (e.g. yocto) build folder that can be attached to tasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VendorBuild {
    /// Short name used in the UI and in `cd build <name>`.
    pub name: String,
    /// Build folder.
    pub path: PathBuf,
}

/// An external script that lists tickets to create tasks from (Jira, Orbit, ...).
///
/// The command runs through `sh -c` and must print tickets on stdout, either
/// as a JSON array / JSON lines of `{"id", "title", "url", "description"}`
/// or as tab separated `id<TAB>title<TAB>url<TAB>description` lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSource {
    /// Name shown in the "new task" chooser, e.g. `jira`.
    pub name: String,
    /// Shell command line to run.
    pub command: String,
}

/// One-shot agent used to generate context and checkpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    /// Program, e.g. `claude`. Empty disables the AI actions.
    pub command: String,
    /// Arguments. An argument containing `{prompt}` receives the prompt;
    /// without one the prompt is written to the agent's stdin.
    pub args: Vec<String>,
    /// Context prompt template (empty: `<config dir>/prompts/context.md`).
    pub context_prompt: PathBuf,
    /// Checkpoint prompt template (empty: `<config dir>/prompts/checkpoints.md`).
    pub checkpoint_prompt: PathBuf,
    /// Give up after this many seconds.
    pub timeout_secs: u64,
    /// Word limit for generated context (at most 1000).
    pub context_max_words: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            command: "claude".into(),
            args: vec!["-p".into()],
            context_prompt: PathBuf::new(),
            checkpoint_prompt: PathBuf::new(),
            timeout_secs: 600,
            context_max_words: 600,
        }
    }
}

/// Interactive coding agent launched in a task shell (`leader a`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CodingAgentConfig {
    /// Program, e.g. `claude`, `codex`, `aider`. Empty disables the shortcut.
    pub command: String,
    /// Arguments; `{prompt}` receives the startup prompt, else it is appended.
    pub args: Vec<String>,
    /// Startup prompt template (empty: `<config dir>/prompts/coding-agent.md`).
    pub startup_prompt: PathBuf,
    /// Start in the first attached code workspace (else the task folder).
    pub start_in_code: bool,
}

impl Default for CodingAgentConfig {
    fn default() -> Self {
        Self {
            command: "claude".into(),
            args: Vec::new(),
            startup_prompt: PathBuf::new(),
            start_in_code: true,
        }
    }
}

/// Checkpoint timer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimerConfig {
    /// Length of a focus block when the task has no checkpoints.
    pub focus_minutes: u64,
    /// Flash the whole screen when time is up.
    pub flash: bool,
    /// Ring the terminal bell when time is up.
    pub bell: bool,
    /// Pause the timer after this many minutes without a key press or click (0: never).
    pub idle_minutes: u64,
}

impl Default for TimerConfig {
    fn default() -> Self {
        Self {
            focus_minutes: 25,
            flash: true,
            bell: true,
            idle_minutes: 15,
        }
    }
}

/// Day planner (the Plan view and the Today pane on Home).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlannerConfig {
    /// Minutes of planned work that fit in a day.
    pub day_minutes: u64,
    /// Columns whose tasks the Plan view offers. Empty: every column except
    /// the last, and except the first when there are three or more.
    pub columns: Vec<String>,
}

impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            day_minutes: 360,
            columns: Vec::new(),
        }
    }
}

/// The persisted configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Root folder containing one sub-folder per task.
    pub tasks_dir: PathBuf,
    /// Board columns, in display order. Must contain at least one entry.
    pub categories: Vec<String>,
    /// Colour scheme for the UI chrome.
    pub color_scheme: ColorScheme,
    /// Shell launched for task terminals.
    pub shell: ShellConfig,
    /// Preferred font family. Advisory only: terminal emulators own the font,
    /// pahiri records the preference and shows it in the status line.
    pub font_family: String,
    /// Prefix key that pahiri intercepts while a shell has focus
    /// (tmux style; press it twice to send it to the shell).
    pub leader_key: String,
    /// Files larger than this (in KiB) prompt before opening.
    pub large_file_kb: u64,
    /// Show dot-files in the task file tree.
    pub show_hidden: bool,
    /// Tab width used by the editor when rendering.
    pub tab_width: u8,
    /// Scrollback lines kept per shell.
    pub scrollback_lines: usize,
    /// Name of the board file inside `tasks_dir`.
    pub status_file: String,
    /// File that every task folder is expected to contain.
    pub context_file: String,
    /// Capture the mouse (click to focus/select, wheel to scroll, forwarded to programs
    /// that ask for it). Hold Shift to select text with the terminal emulator instead.
    pub mouse: bool,
    /// Syntax highlighting in the editor.
    pub syntax_highlighting: bool,
    /// Branch name used as "main" for workspaces that do not set their own.
    pub default_main_branch: String,
    /// Code checkouts available for attaching to tasks.
    pub workspaces: Vec<Workspace>,
    /// Vendor builds available for attaching to tasks.
    pub builds: Vec<VendorBuild>,
    /// Ticket sources for creating tasks.
    pub task_sources: Vec<TaskSource>,
    /// Gerrit web URL; empty derives it from each workspace's `origin` remote.
    pub gerrit_url: String,
    /// Command that reports Gerrit change status (see the help page, Gerrit tab).
    pub gerrit_status_command: String,
    /// Shell commands run on events, `event = command` (see the help page, Hooks tab).
    pub hooks: BTreeMap<String, String>,
    /// Give up on a hook after this many seconds.
    pub hook_timeout_secs: u64,
    /// Hide tasks finished more than this many days ago (0: never).
    pub archive_after_days: u64,
    /// Soft-wrap long lines of Markdown and plain text in the editor.
    pub soft_wrap: bool,
    /// Command that receives copied text on stdin (e.g. `wl-copy`,
    /// `xclip -selection clipboard`, `pbcopy`). Empty: tell the terminal with OSC 52.
    pub copy_command: String,
    /// Command whose stdout Ctrl+V pastes (e.g. `wl-paste -n`, `pbpaste`).
    /// Empty: Ctrl+V pastes what pahiri copied last.
    pub paste_command: String,
    /// Reopen each task's shells (in the same folders) after a restart.
    pub restore_shells: bool,
    /// Key overrides: `palette.<action> = "x"` or `leader.<command> = "x"`.
    pub keys: BTreeMap<String, String>,
    /// One-shot agent for context and checkpoints.
    pub agent: AgentConfig,
    /// Interactive coding agent.
    pub coding_agent: CodingAgentConfig,
    /// Checkpoint timer.
    pub timer: TimerConfig,
    /// Day planner.
    pub planner: PlannerConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tasks_dir: PathBuf::new(),
            categories: vec!["Planned".into(), "Doing".into(), "Done".into()],
            color_scheme: ColorScheme::Dark,
            shell: ShellConfig::default(),
            font_family: "JetBrains Mono".into(),
            leader_key: "ctrl+b".into(),
            large_file_kb: 512,
            show_hidden: false,
            tab_width: 4,
            scrollback_lines: 5000,
            status_file: "status.md".into(),
            context_file: "CONTEXT.md".into(),
            mouse: true,
            syntax_highlighting: true,
            default_main_branch: "main".into(),
            workspaces: Vec::new(),
            builds: Vec::new(),
            task_sources: Vec::new(),
            gerrit_url: String::new(),
            gerrit_status_command: String::new(),
            hooks: BTreeMap::new(),
            hook_timeout_secs: 15,
            archive_after_days: 14,
            soft_wrap: true,
            copy_command: String::new(),
            paste_command: String::new(),
            restore_shells: true,
            keys: BTreeMap::new(),
            agent: AgentConfig::default(),
            coding_agent: CodingAgentConfig::default(),
            timer: TimerConfig::default(),
            planner: PlannerConfig::default(),
        }
    }
}

impl Config {
    /// Default location: `$XDG_CONFIG_HOME/pahiri/config.toml` (or the OS equivalent).
    pub fn default_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", APP_NAME).map(|d| d.config_dir().join("config.toml"))
    }

    /// Default location for logs and other runtime state.
    pub fn state_dir() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", APP_NAME).map(|d| {
            d.state_dir()
                .map_or_else(|| d.data_local_dir().to_path_buf(), Path::to_path_buf)
        })
    }

    /// Load the config from `path`. Returns `Ok(None)` when the file does not exist.
    pub fn load(path: &Path) -> Result<Option<Self>, ConfigError> {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(ConfigError::Io {
                    path: path.to_path_buf(),
                    source,
                })
            }
        };
        toml::from_str(&text)
            .map(Some)
            .map_err(|e| ConfigError::Parse {
                path: path.to_path_buf(),
                message: e.to_string(),
            })
    }

    /// Write the config to `path`, creating parent directories as needed.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| ConfigError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let text =
            toml::to_string_pretty(self).map_err(|e| ConfigError::Serialize(e.to_string()))?;
        fs::write(path, text).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Problems that do not stop pahiri: workspace and build folders that are
    /// missing (a checkout removed, a drive not mounted). `pahiri config prune`
    /// drops them.
    pub fn warnings(&self) -> Vec<String> {
        let workspaces = self
            .workspaces
            .iter()
            .filter(|w| !w.path.is_dir())
            .map(|w| {
                format!(
                    "workspace {} folder is missing: {}",
                    w.name,
                    w.path.display()
                )
            });
        let builds = self
            .builds
            .iter()
            .filter(|b| !b.path.is_dir())
            .map(|b| format!("build {} folder is missing: {}", b.name, b.path.display()));
        workspaces.chain(builds).collect()
    }

    /// Validate the configuration, returning every problem found (empty means valid).
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.tasks_dir.as_os_str().is_empty() {
            errors.push("tasks folder is required".into());
        } else if !self.tasks_dir.is_dir() {
            errors.push(format!(
                "tasks folder does not exist: {}",
                self.tasks_dir.display()
            ));
        }
        if self.categories.is_empty() {
            errors.push("at least one category is required".into());
        }
        let mut seen = std::collections::HashSet::new();
        for c in &self.categories {
            if c.trim().is_empty() {
                errors.push("category names must not be empty".into());
            } else if !seen.insert(c.trim().to_lowercase()) {
                errors.push(format!("duplicate category: {c}"));
            }
        }
        if self.shell.program.trim().is_empty() {
            errors.push("shell program is required".into());
        }
        if let Err(e) = KeyCombo::parse(&self.leader_key) {
            errors.push(format!("leader key: {e}"));
        }
        if self.tab_width == 0 {
            errors.push("tab width must be at least 1".into());
        }
        if self.status_file.trim().is_empty() {
            errors.push("status file name is required".into());
        }
        if self.default_main_branch.trim().is_empty() {
            errors.push("default main branch is required".into());
        }
        let mut names = std::collections::HashSet::new();
        for w in &self.workspaces {
            if !is_plain_name(&w.name) {
                errors.push(format!("workspace name {:?} must be a single word", w.name));
            } else if !names.insert(w.name.clone()) {
                errors.push(format!("duplicate workspace name {:?}", w.name));
            }
        }
        let mut names = std::collections::HashSet::new();
        for b in &self.builds {
            if !is_plain_name(&b.name) {
                errors.push(format!("build name {:?} must be a single word", b.name));
            } else if !names.insert(b.name.clone()) {
                errors.push(format!("duplicate build name {:?}", b.name));
            }
        }
        let mut names = std::collections::HashSet::new();
        for t in &self.task_sources {
            if !is_plain_name(&t.name) {
                errors.push(format!(
                    "task source name {:?} must be a single word",
                    t.name
                ));
            } else if !names.insert(t.name.clone()) {
                errors.push(format!("duplicate task source name {:?}", t.name));
            }
            if t.command.trim().is_empty() {
                errors.push(format!("task source {} has no command", t.name));
            }
        }
        if self.agent.timeout_secs == 0 {
            errors.push("agent timeout must be at least 1 second".into());
        }
        if !(1..=1000).contains(&self.agent.context_max_words) {
            errors.push("context word limit must be between 1 and 1000".into());
        }
        if self.timer.focus_minutes == 0 {
            errors.push("focus block must be at least 1 minute".into());
        }
        for (event, command) in &self.hooks {
            if crate::hooks::HookEvent::from_name(event).is_none() {
                errors.push(format!("unknown hook event {event:?} (see help → Hooks)"));
            }
            if command.trim().is_empty() {
                errors.push(format!("hook {event} has no command"));
            }
        }
        if self.hook_timeout_secs == 0 {
            errors.push("hook timeout must be at least 1 second".into());
        }
        errors.extend(crate::app::keymap::validate_overrides(&self.keys));
        errors
    }

    /// Where a prompt template lives: the configured path, or
    /// `<folder of the config file>/prompts/<name>.md`.
    pub fn prompt_path(&self, kind: crate::ai::PromptKind, config_path: &Path) -> PathBuf {
        use crate::ai::PromptKind;
        let configured = match kind {
            PromptKind::Context => &self.agent.context_prompt,
            PromptKind::Checkpoints => &self.agent.checkpoint_prompt,
            PromptKind::Coding => &self.coding_agent.startup_prompt,
        };
        if configured.as_os_str().is_empty() {
            config_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("prompts")
                .join(kind.file_name())
        } else {
            Self::expand_tilde(&configured.display().to_string())
        }
    }

    /// The parsed leader key. Falls back to the default when the configured one is invalid.
    pub fn leader(&self) -> KeyCombo {
        KeyCombo::parse(&self.leader_key)
            .unwrap_or_else(|_| KeyCombo::parse("ctrl+b").expect("default leader parses"))
    }

    /// Path to the board file.
    pub fn status_path(&self) -> PathBuf {
        self.tasks_dir.join(&self.status_file)
    }

    /// Main branch for a workspace.
    pub fn main_branch_for(&self, ws: &Workspace) -> String {
        ws.main_branch
            .clone()
            .unwrap_or_else(|| self.default_main_branch.clone())
    }

    /// Look up a workspace by name.
    pub fn workspace(&self, name: &str) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.name == name)
    }

    /// Look up a build by name.
    pub fn build(&self, name: &str) -> Option<&VendorBuild> {
        self.builds.iter().find(|b| b.name == name)
    }

    /// Expand a leading `~` to the user's home directory.
    pub fn expand_tilde(input: &str) -> PathBuf {
        if let Some(rest) = input.strip_prefix('~') {
            if let Some(home) = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()) {
                let rest = rest.trim_start_matches('/');
                return if rest.is_empty() {
                    home
                } else {
                    home.join(rest)
                };
            }
        }
        PathBuf::from(input)
    }
}

/// A name usable in shell commands and file names: letters, digits, `-`, `_`, `.`.
pub fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspaces_and_sources_roundtrip_and_validate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let cfg = Config {
            tasks_dir: dir.path().to_path_buf(),
            workspaces: vec![Workspace {
                name: "fw".into(),
                path: dir.path().to_path_buf(),
                main_branch: Some("develop".into()),
            }],
            builds: vec![VendorBuild {
                name: "yocto".into(),
                path: dir.path().to_path_buf(),
            }],
            task_sources: vec![TaskSource {
                name: "jira".into(),
                command: "jira-list".into(),
            }],
            ..Config::default()
        };
        assert!(cfg.validate().is_empty(), "{:?}", cfg.validate());
        cfg.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap().unwrap(), cfg);
        assert_eq!(cfg.main_branch_for(&cfg.workspaces[0]), "develop");

        let bad = Config {
            workspaces: vec![Workspace {
                name: "two words".into(),
                path: PathBuf::from("/nonexistent/x"),
                main_branch: None,
            }],
            task_sources: vec![TaskSource {
                name: "jira".into(),
                command: " ".into(),
            }],
            ..cfg.clone()
        };
        let errors = bad.validate();
        assert!(errors.iter().any(|e| e.contains("single word")));
        assert!(errors.iter().any(|e| e.contains("no command")));
        // A missing folder is only a warning: pahiri still starts.
        assert!(!errors.iter().any(|e| e.contains("/nonexistent/x")));
        assert_eq!(
            bad.warnings(),
            ["workspace two words folder is missing: /nonexistent/x"]
        );
    }

    #[test]
    fn plain_names() {
        assert!(is_plain_name("PROJ-123"));
        assert!(is_plain_name("orbit_42.b"));
        assert!(!is_plain_name("has space"));
        assert!(!is_plain_name("-lead"));
        assert!(!is_plain_name(""));
    }

    #[test]
    fn roundtrips_through_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.toml");
        let mut cfg = Config {
            tasks_dir: dir.path().to_path_buf(),
            ..Config::default()
        };
        cfg.categories = vec!["Todo".into(), "Done".into()];
        cfg.color_scheme = ColorScheme::Nord;
        cfg.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap().unwrap();
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Config::load(&dir.path().join("nope.toml"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn hooks_and_keys_are_validated() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config {
            tasks_dir: dir.path().to_path_buf(),
            ..Config::default()
        };
        cfg.hooks.insert("task_enter".into(), "echo hi".into());
        cfg.keys.insert("palette.timer".into(), "u".into());
        cfg.keys.insert("leader.coding_agent".into(), "A".into());
        assert!(cfg.validate().is_empty(), "{:?}", cfg.validate());
        cfg.hooks.insert("nope".into(), " ".into());
        cfg.keys.insert("palette.bogus".into(), "y".into());
        cfg.keys.insert("leader.timer".into(), "long".into());
        let errors = cfg.validate();
        assert!(
            errors.iter().any(|e| e.contains("unknown hook event")),
            "{errors:?}"
        );
        assert!(errors.iter().any(|e| e.contains("no command")));
        assert!(errors.iter().any(|e| e.contains("bogus")));
        assert!(errors.iter().any(|e| e.contains("one character")));
        let path = dir.path().join("c.toml");
        cfg.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap().unwrap(), cfg);
    }

    #[test]
    fn partial_file_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, format!("tasks_dir = {:?}\n", dir.path())).unwrap();
        let cfg = Config::load(&path).unwrap().unwrap();
        assert_eq!(cfg.categories, Config::default().categories);
        assert_eq!(cfg.shell.program, "zsh");
        assert_eq!(cfg.agent.args, vec!["-p"]);
        assert_eq!(cfg.timer.focus_minutes, 25);
        assert_eq!(
            cfg.prompt_path(crate::ai::PromptKind::Context, &path),
            dir.path().join("prompts/context.md")
        );
    }

    #[test]
    fn validation_reports_problems() {
        let cfg = Config {
            categories: vec![],
            leader_key: "bogus".into(),
            ..Config::default()
        };
        let errors = cfg.validate();
        assert!(errors
            .iter()
            .any(|e| e.contains("tasks folder is required")));
        assert!(errors.iter().any(|e| e.contains("at least one category")));
        assert!(errors.iter().any(|e| e.contains("leader key")));
    }

    #[test]
    fn duplicate_categories_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config {
            tasks_dir: dir.path().to_path_buf(),
            categories: vec!["Doing".into(), "doing".into()],
            ..Config::default()
        };
        assert!(cfg.validate().iter().any(|e| e.contains("duplicate")));
    }

    #[test]
    fn valid_config_has_no_errors() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config {
            tasks_dir: dir.path().to_path_buf(),
            ..Config::default()
        };
        assert!(cfg.validate().is_empty(), "{:?}", cfg.validate());
    }

    #[test]
    fn color_scheme_cycles() {
        assert_eq!(ColorScheme::Dark.next(), ColorScheme::Light);
        assert_eq!(ColorScheme::Dark.prev(), ColorScheme::Solarized);
        assert_eq!(ColorScheme::Solarized.next(), ColorScheme::Dark);
    }
}
