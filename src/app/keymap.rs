//! Configurable keys: palette shortcuts and leader commands.
//!
//! Overrides live in the config as `palette.<action> = "x"` and
//! `leader.<command> = "x"` (see [`Action::name`] and [`LeaderCmd::name`]).

use std::collections::BTreeMap;

use super::palette::{self, Action};

/// Commands available after the leader key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaderCmd {
    /// Open the command palette.
    Palette,
    /// Leave the terminal (focus the shell list).
    Leave,
    /// Zoom / unzoom the terminal.
    Zoom,
    /// Open a new shell.
    NewShell,
    /// Close the selected shell.
    CloseShell,
    /// Previous shell.
    PrevShell,
    /// Next shell.
    NextShell,
    /// Focus the editor (or files).
    Up,
    /// Focus the file tree.
    Files,
    /// Scroll the terminal back.
    ScrollUp,
    /// Scroll the terminal forward.
    ScrollDown,
    /// The help page.
    Help,
    /// Launch the coding agent.
    CodingAgent,
    /// Timer menu.
    Timer,
    /// Tick the current checkpoint.
    CheckpointDone,
}

impl LeaderCmd {
    /// All commands with their default key and extra aliases.
    pub const ALL: [(LeaderCmd, char, &'static str); 15] = [
        (Self::Palette, ':', "command palette (also Esc)"),
        (Self::Leave, 'q', "leave the shell (also d)"),
        (Self::Zoom, 'z', "zoom / unzoom (also f)"),
        (Self::NewShell, 'n', "new shell (also t c s)"),
        (Self::CloseShell, 'x', "close shell"),
        (Self::PrevShell, 'h', "previous shell (also p ←)"),
        (Self::NextShell, 'l', "next shell (also → Tab)"),
        (Self::Up, 'k', "focus editor / files (also ↑)"),
        (Self::Files, 'e', "focus files"),
        (Self::ScrollUp, '[', "scroll back (also PgUp)"),
        (Self::ScrollDown, ']', "scroll forward (also PgDn)"),
        (Self::Help, '?', "help page"),
        (Self::CodingAgent, 'a', "coding agent"),
        (Self::Timer, 'm', "timer menu"),
        (Self::CheckpointDone, 'v', "checkpoint done → next"),
    ];

    /// Built-in aliases (kept unless an override takes the key).
    const ALIASES: [(char, LeaderCmd); 6] = [
        ('d', Self::Leave),
        ('f', Self::Zoom),
        ('t', Self::NewShell),
        ('c', Self::NewShell),
        ('s', Self::NewShell),
        ('p', Self::PrevShell),
    ];

    /// Config name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Palette => "palette",
            Self::Leave => "leave",
            Self::Zoom => "zoom",
            Self::NewShell => "new_shell",
            Self::CloseShell => "close_shell",
            Self::PrevShell => "prev_shell",
            Self::NextShell => "next_shell",
            Self::Up => "up",
            Self::Files => "files",
            Self::ScrollUp => "scroll_up",
            Self::ScrollDown => "scroll_down",
            Self::Help => "help",
            Self::CodingAgent => "coding_agent",
            Self::Timer => "timer",
            Self::CheckpointDone => "checkpoint_done",
        }
    }

    /// Look up by config name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .map(|(c, _, _)| *c)
            .find(|c| c.name() == name)
    }
}

fn override_char(keys: &BTreeMap<String, String>, key: &str) -> Option<char> {
    let v = keys.get(key)?;
    let mut chars = v.chars();
    let c = chars.next()?;
    chars.next().is_none().then_some(c)
}

/// Leader key table: char → command, with overrides applied.
pub fn leader_table(keys: &BTreeMap<String, String>) -> Vec<(char, LeaderCmd)> {
    let mut table: Vec<(char, LeaderCmd)> = LeaderCmd::ALIASES.to_vec();
    for (cmd, default, _) in LeaderCmd::ALL {
        let c = override_char(keys, &format!("leader.{}", cmd.name())).unwrap_or(default);
        table.retain(|(k, _)| *k != c);
        table.insert(0, (c, cmd));
    }
    table
}

/// The command bound to `c` after the leader.
pub fn leader_command(table: &[(char, LeaderCmd)], c: char) -> Option<LeaderCmd> {
    table.iter().find(|(k, _)| *k == c).map(|(_, cmd)| *cmd)
}

/// Current key of a leader command (for help texts).
pub fn leader_key_of(keys: &BTreeMap<String, String>, cmd: LeaderCmd) -> char {
    let default = LeaderCmd::ALL
        .iter()
        .find(|(c, _, _)| *c == cmd)
        .map_or('?', |(_, k, _)| *k);
    override_char(keys, &format!("leader.{}", cmd.name())).unwrap_or(default)
}

/// Replace palette shortcut letters with configured ones.
pub fn apply_palette_overrides(
    mut commands: Vec<palette::Command>,
    keys: &BTreeMap<String, String>,
) -> Vec<palette::Command> {
    for c in &mut commands {
        if let Some(k) = override_char(keys, &format!("palette.{}", c.action.name())) {
            c.key = k;
        }
    }
    commands
}

/// Problems with key overrides (empty = fine).
pub fn validate_overrides(keys: &BTreeMap<String, String>) -> Vec<String> {
    let mut errors = Vec::new();
    for (k, v) in keys {
        let known = match k.split_once('.') {
            Some(("palette", name)) => Action::from_name(name).is_some(),
            Some(("leader", name)) => LeaderCmd::from_name(name).is_some(),
            _ => false,
        };
        if !known {
            errors.push(format!(
                "unknown key setting {k:?}: use palette.<action> or leader.<command> (help → Keys)"
            ));
        }
        if v.chars().count() != 1 {
            errors.push(format!("key for {k} must be one character, got {v:?}"));
        }
    }
    for (label, cmds) in [
        ("task list", palette::list_commands()),
        ("task view", palette::task_commands()),
    ] {
        let cmds = apply_palette_overrides(cmds, keys);
        for (i, a) in cmds.iter().enumerate() {
            if let Some(b) = cmds[i + 1..].iter().find(|b| b.key == a.key) {
                errors.push(format!(
                    "palette key '{}' is used twice in the {label} palette ({} and {})",
                    a.key,
                    a.action.name(),
                    b.action.name()
                ));
            }
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leader_overrides_replace_and_keep_aliases() {
        let mut keys = BTreeMap::new();
        let t = leader_table(&keys);
        assert_eq!(leader_command(&t, 'a'), Some(LeaderCmd::CodingAgent));
        assert_eq!(leader_command(&t, 't'), Some(LeaderCmd::NewShell));
        keys.insert("leader.coding_agent".into(), "t".into());
        let t = leader_table(&keys);
        assert_eq!(leader_command(&t, 't'), Some(LeaderCmd::CodingAgent));
        assert_eq!(leader_command(&t, 'n'), Some(LeaderCmd::NewShell));
        assert_eq!(leader_key_of(&keys, LeaderCmd::CodingAgent), 't');
        for (cmd, _, _) in LeaderCmd::ALL {
            assert_eq!(LeaderCmd::from_name(cmd.name()), Some(cmd));
        }
    }

    #[test]
    fn palette_overrides_and_validation() {
        let mut keys = BTreeMap::new();
        keys.insert("palette.timer".into(), "u".into());
        let cmds = apply_palette_overrides(palette::task_commands(), &keys);
        assert!(cmds
            .iter()
            .any(|c| c.key == 'u' && c.action == Action::Timer));
        assert!(validate_overrides(&keys).is_empty());
        keys.insert("palette.gerrit".into(), "u".into());
        assert!(validate_overrides(&keys)
            .iter()
            .any(|e| e.contains("used twice")));
    }
}
