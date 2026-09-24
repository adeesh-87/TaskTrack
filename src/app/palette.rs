//! The vim-style command palette opened with `Esc`.

/// Everything the palette can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Go to the task list.
    TaskList,
    /// Open settings.
    Config,
    /// Quit pahiri.
    Quit,
    /// Create a task (custom or from a source).
    NewTask,
    /// Rescan the tasks folder.
    Refresh,
    /// Attach workspaces / builds to the task.
    Attach,
    /// Commit, pull and switch attached workspaces to the task branch.
    Prepare,
    /// Open a new shell.
    NewShell,
    /// Close the selected shell.
    CloseShell,
    /// Open the task's context file.
    OpenContext,
    /// Focus the file tree.
    FocusFiles,
    /// Focus the editor.
    FocusEditor,
    /// Focus the shell list.
    FocusShells,
    /// Focus the terminal.
    FocusTerminal,
    /// Zoom the terminal.
    Zoom,
    /// Move task to the next category.
    MoveNext,
    /// Move task to the previous category.
    MovePrev,
    /// Toggle hidden files.
    ToggleHidden,
    /// Save the editor buffer.
    Save,
    /// Close the editor buffer.
    CloseEditor,
    /// Show the key reference.
    Help,
    /// Move the task folder to the trash.
    DeleteTask,
    /// Start / pause / resume the checkpoint timer.
    Timer,
    /// Stop the timer and book the time.
    StopTimer,
    /// Tick the current checkpoint and move on to the next.
    CheckpointDone,
    /// Show the checkpoints.
    Checkpoints,
    /// Find Gerrit changes on the task branches.
    Gerrit,
    /// Ask the agent to write the task context.
    GenerateContext,
    /// Mark the context ready / not ready.
    ToggleContextReady,
    /// Ask the agent to break the task into checkpoints.
    BreakDown,
    /// Launch the interactive coding agent in a new shell.
    CodingAgent,
    /// Open a prompt template in the editor.
    EditPrompts,
}

/// A palette entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// Single-key shortcut (runs immediately when the filter is empty).
    pub key: char,
    /// Label.
    pub label: &'static str,
    /// Action.
    pub action: Action,
}

const fn cmd(key: char, label: &'static str, action: Action) -> Command {
    Command { key, label, action }
}

/// Commands available from the task list.
pub fn list_commands() -> Vec<Command> {
    vec![
        cmd(
            'n',
            "new task (custom or from Jira/Orbit/...)",
            Action::NewTask,
        ),
        cmd(
            'm',
            "timer: start / pause / resume (selected task)",
            Action::Timer,
        ),
        cmd('M', "timer: stop and book the time", Action::StopTimer),
        cmd('v', "checkpoint done → next", Action::CheckpointDone),
        cmd('k', "checkpoints of the selected task", Action::Checkpoints),
        cmd(']', "move task to next category", Action::MoveNext),
        cmd('[', "move task to previous category", Action::MovePrev),
        cmd('D', "delete task (moves it to .trash)", Action::DeleteTask),
        cmd('r', "rescan tasks folder", Action::Refresh),
        cmd('c', "configuration", Action::Config),
        cmd('h', "help / keys", Action::Help),
        cmd('q', "quit pahiri", Action::Quit),
    ]
}

/// Commands available inside a task.
pub fn task_commands() -> Vec<Command> {
    vec![
        cmd('T', "back to the task list", Action::TaskList),
        cmd(
            'p',
            "prepare: commit, pull main, switch attached repos to task branch",
            Action::Prepare,
        ),
        cmd(
            'a',
            "attach code workspaces / vendor builds",
            Action::Attach,
        ),
        cmd('m', "timer: start / pause / resume", Action::Timer),
        cmd('M', "timer: stop and book the time", Action::StopTimer),
        cmd('v', "checkpoint done → next", Action::CheckpointDone),
        cmd('k', "checkpoints", Action::Checkpoints),
        cmd('i', "AI: write the task context", Action::GenerateContext),
        cmd('r', "context ready: toggle", Action::ToggleContextReady),
        cmd(
            'b',
            "AI: break the task into checkpoints",
            Action::BreakDown,
        ),
        cmd(
            'l',
            "coding agent: launch it for this task",
            Action::CodingAgent,
        ),
        cmd(
            'g',
            "find Gerrit changes on the task branches",
            Action::Gerrit,
        ),
        cmd('E', "edit AI prompt templates", Action::EditPrompts),
        cmd('s', "new shell", Action::NewShell),
        cmd('x', "close selected shell", Action::CloseShell),
        cmd('o', "open CONTEXT.md", Action::OpenContext),
        cmd('f', "focus files", Action::FocusFiles),
        cmd('e', "focus editor", Action::FocusEditor),
        cmd('w', "focus shell list", Action::FocusShells),
        cmd('t', "focus terminal", Action::FocusTerminal),
        cmd('z', "zoom terminal", Action::Zoom),
        cmd(']', "move task to next category", Action::MoveNext),
        cmd('[', "move task to previous category", Action::MovePrev),
        cmd('.', "toggle hidden files", Action::ToggleHidden),
        cmd('S', "save editor buffer", Action::Save),
        cmd('W', "close editor buffer", Action::CloseEditor),
        cmd('n', "new task", Action::NewTask),
        cmd('D', "delete task (moves it to .trash)", Action::DeleteTask),
        cmd('c', "configuration", Action::Config),
        cmd('h', "help / keys", Action::Help),
        cmd('q', "quit pahiri", Action::Quit),
    ]
}

/// Palette state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    commands: Vec<Command>,
    /// Filter text typed so far.
    pub filter: String,
    /// Highlighted index into [`Palette::matches`].
    pub selected: usize,
    /// Whether typed letters filter (after `:` or `/`) instead of running shortcuts.
    pub typing: bool,
}

impl Palette {
    /// New palette over `commands`.
    pub fn new(commands: Vec<Command>) -> Self {
        Self {
            commands,
            filter: String::new(),
            selected: 0,
            typing: false,
        }
    }

    /// Commands matching the filter (substring on the label, or the key).
    pub fn matches(&self) -> Vec<&Command> {
        let f = self.filter.to_lowercase();
        self.commands
            .iter()
            .filter(|c| {
                f.is_empty() || c.label.to_lowercase().contains(&f) || c.key.to_string() == f
            })
            .collect()
    }

    /// Direct shortcut lookup (only meaningful while the filter is empty).
    pub fn shortcut(&self, key: char) -> Option<Action> {
        self.commands
            .iter()
            .find(|c| c.key == key)
            .map(|c| c.action)
    }

    /// Highlighted command.
    pub fn current(&self) -> Option<Action> {
        self.matches().get(self.selected).map(|c| c.action)
    }

    /// Move the highlight.
    pub fn step(&mut self, delta: i32) {
        let n = self.matches().len();
        if n == 0 {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected as i64 + i64::from(delta)).rem_euclid(n as i64) as usize;
    }

    /// Switch to typed filtering (vim's `:`).
    pub fn start_typing(&mut self) {
        self.typing = true;
        self.selected = 0;
    }

    /// Append to the filter.
    pub fn push(&mut self, c: char) {
        self.typing = true;
        self.filter.push(c);
        self.selected = 0;
    }

    /// Remove the last filter char.
    pub fn pop(&mut self) {
        self.filter.pop();
        self.selected = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_are_unique_per_palette() {
        for cmds in [list_commands(), task_commands()] {
            let mut keys: Vec<char> = cmds.iter().map(|c| c.key).collect();
            keys.sort_unstable();
            keys.dedup();
            assert_eq!(keys.len(), cmds.len());
        }
    }

    #[test]
    fn filter_and_navigation() {
        let mut p = Palette::new(task_commands());
        assert_eq!(p.shortcut('T'), Some(Action::TaskList));
        assert_eq!(p.shortcut('q'), Some(Action::Quit));
        assert!(!p.typing);
        p.start_typing();
        for c in "shell".chars() {
            p.push(c);
        }
        let labels: Vec<&str> = p.matches().iter().map(|c| c.label).collect();
        assert!(labels.iter().all(|l| l.contains("shell")));
        assert_eq!(p.current(), Some(Action::NewShell));
        p.step(1);
        assert_eq!(p.current(), Some(Action::CloseShell));
        p.step(-2);
        assert_eq!(p.current(), Some(Action::FocusShells));
        p.pop();
        assert_eq!(p.selected, 0);
    }
}
