//! Configuration model, persistence and validation.

pub mod keybind;

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
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            program: "zsh".to_owned(),
            args: vec!["-i".to_owned()],
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
    /// Branch name used as "main" for workspaces that do not set their own.
    pub default_main_branch: String,
    /// Code checkouts available for attaching to tasks.
    pub workspaces: Vec<Workspace>,
    /// Vendor builds available for attaching to tasks.
    pub builds: Vec<VendorBuild>,
    /// Ticket sources for creating tasks.
    pub task_sources: Vec<TaskSource>,
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
            default_main_branch: "main".into(),
            workspaces: Vec::new(),
            builds: Vec::new(),
            task_sources: Vec::new(),
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
            if !w.path.is_dir() {
                errors.push(format!(
                    "workspace {} folder does not exist: {}",
                    w.name,
                    w.path.display()
                ));
            }
        }
        let mut names = std::collections::HashSet::new();
        for b in &self.builds {
            if !is_plain_name(&b.name) {
                errors.push(format!("build name {:?} must be a single word", b.name));
            } else if !names.insert(b.name.clone()) {
                errors.push(format!("duplicate build name {:?}", b.name));
            }
            if !b.path.is_dir() {
                errors.push(format!(
                    "build {} folder does not exist: {}",
                    b.name,
                    b.path.display()
                ));
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
        errors
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
        assert!(errors.iter().any(|e| e.contains("does not exist")));
        assert!(errors.iter().any(|e| e.contains("no command")));
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
    fn partial_file_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, format!("tasks_dir = {:?}\n", dir.path())).unwrap();
        let cfg = Config::load(&path).unwrap().unwrap();
        assert_eq!(cfg.categories, Config::default().categories);
        assert_eq!(cfg.shell.program, "zsh");
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
