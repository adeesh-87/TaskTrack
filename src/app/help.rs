//! The help page: tabs of reference text, built from the live config so keys,
//! hooks and agent settings show what is actually in effect.

use std::fmt::Write as _;

use crate::hooks::{HookEvent, COMMON_ENV};

use super::keymap::{self, LeaderCmd};
use super::palette;
use super::App;

/// Help tabs, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpTopic {
    /// Keys.
    Keys,
    /// Checkpoints, timer, today view.
    Work,
    /// AI agents and prompt templates.
    Agents,
    /// Hooks and their variables.
    Hooks,
    /// Task source scripts (Jira, Orbit, …).
    Sources,
    /// Gerrit.
    Gerrit,
    /// Command line.
    Cli,
    /// Files pahiri reads and writes.
    Files,
    /// When something does not work.
    Troubleshooting,
}

impl HelpTopic {
    /// All tabs.
    pub const ALL: [HelpTopic; 9] = [
        Self::Keys,
        Self::Work,
        Self::Agents,
        Self::Hooks,
        Self::Sources,
        Self::Gerrit,
        Self::Cli,
        Self::Files,
        Self::Troubleshooting,
    ];

    /// Tab title.
    pub fn title(self) -> &'static str {
        match self {
            Self::Keys => "Keys",
            Self::Work => "Timer & work",
            Self::Agents => "AI agents",
            Self::Hooks => "Hooks",
            Self::Sources => "Jira / Orbit",
            Self::Gerrit => "Gerrit",
            Self::Cli => "CLI",
            Self::Files => "Files",
            Self::Troubleshooting => "Trouble",
        }
    }

    /// Index in [`HelpTopic::ALL`].
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

/// Shown on the Jira / Orbit tab (and after a failed test run).
pub const TASK_SOURCE_FORMAT: &str = "\
A task source is any program that prints your tickets on stdout.

SETTING   Esc c → Task sources → `name = command`, e.g.
            jira  = ~/bin/jira.sh
            orbit = python3 ~/bin/orbit.py --mine
          The command runs through `sh -c` with $PAHIRI_TASKS_DIR set.
          Exit non-zero on failure; the last lines of stderr are shown.
          In the settings, `t` on a source runs it now and shows what pahiri
          parsed (nothing is created).

OUTPUT    one of these three shapes:

  1) JSON array
     [
       {\"id\": \"PROJ-123\",
        \"title\": \"Fix login after password reset\",
        \"url\": \"https://jira.example.com/browse/PROJ-123\",
        \"description\": \"Users cannot log in …\\nSteps: …\"}
     ]

  2) JSON lines — one object like the above per line.

  3) Tab separated — one ticket per line, `#` starts a comment,
     `\\n` in the description becomes a newline:
     PROJ-123<TAB>Fix login<TAB>https://…/PROJ-123<TAB>line one\\nline two

FIELDS    id           required. Becomes the task folder and git branch:
                       spaces and unsafe characters turn into `-`.
          title        one line, shown in the picker and CONTEXT.md heading
          url          link, recorded as `- link:` in CONTEXT.md
          description  text for `## Description`
          Aliases: key = id, summary = title, link = url, body = description.
          Unknown fields are ignored, so you can print extra ones.

EXISTING  Picking a ticket whose task exists offers to open it or refresh
          its `## Description` from the ticket.

EXAMPLES  examples/jira.sh (curl + jq; $JIRA_URL, $JIRA_TOKEN, $JIRA_JQL),
          examples/template.py (skeleton for Orbit or anything else).";

/// Shown on the Gerrit tab.
pub const GERRIT_FORMAT: &str = "\
DETECTION (Esc g)
  For every attached workspace pahiri
    1. fetches origin/<main branch> (batch-mode ssh, never prompts); if that
       fails it says how old the local copy is,
    2. warns when Gerrit's commit-msg hook is missing (no hook, no Change-Id),
    3. lists commits on the task branch that are not on origin/<main> and
       have a `Change-Id: I…` trailer,
    4. links each as https://<host>/q/<Change-Id> (host from the origin
       remote, or the Gerrit URL setting),
    5. runs the Gerrit status command, if set, and stores what it prints.
  Result in CONTEXT.md:
    - gerrit: fw I8f3c…41chars https://review.example.com/c/fw/+/1234 [NEW #1234 CR+2 V+1] :: Fix login

PUSH / REBASE
  Esc P   git push origin HEAD:refs/for/<main> in each workspace that is on
          the task branch (asks first; Gerrit's reply is shown).
  Esc R   fetch, then rebase the task branch onto origin/<main>; a conflict
          is aborted and reported, uncommitted changes refuse to rebase.

STATUS COMMAND (optional, Esc c → Gerrit status command)
  Called with the Change-Ids as arguments (also in $PAHIRI_GERRIT_CHANGES,
  space separated) and the usual PAHIRI_* variables. Print one JSON object
  per change (JSON lines) or one JSON array:

    {\"change_id\": \"I8f3c…\",        required (alias: id)
     \"number\": 1234,                optional
     \"status\": \"NEW\",               optional: NEW / MERGED / ABANDONED / …
     \"url\": \"https://…/+/1234\",     optional, replaces the /q/ link
     \"labels\": \"CR+2 V+1\",          optional, any short text
     \"subject\": \"Fix login\"}        optional

  Changes it does not mention keep their previous status.
  examples/gerrit-status.sh does this with `ssh gerrit query` and jq.";

fn palette_lines(out: &mut String, title: &str, cmds: Vec<palette::Command>) {
    let _ = writeln!(out, "{title}");
    for c in cmds {
        let _ = writeln!(
            out,
            "  {}  {:<55} palette.{}",
            c.key,
            c.label,
            c.action.name()
        );
    }
    out.push('\n');
}

impl App {
    /// Build every help tab.
    pub(super) fn help_tabs(&self) -> Vec<(String, Vec<String>)> {
        HelpTopic::ALL
            .iter()
            .map(|t| {
                let text = self.help_text(*t);
                (
                    t.title().to_owned(),
                    text.lines().map(str::to_owned).collect(),
                )
            })
            .collect()
    }

    fn help_text(&self, topic: HelpTopic) -> String {
        let cfg = &self.config;
        let leader = self.leader.to_string();
        let mut out = String::new();
        match topic {
            HelpTopic::Keys => {
                let _ = writeln!(
                    out,
                    "F1 or ? opens this page · ←/→ or Tab switch tabs · ↑/↓ PgUp/PgDn scroll · Esc closes\n\
                     Esc opens the command palette (inside a shell: {leader} Esc). Press a letter,\n\
                     or : to type and filter. Change any letter with `palette.<name> = \"x\"`\n\
                     in the [keys] table of the config (right column = name).\n"
                );
                palette_lines(
                    &mut out,
                    "HOME PALETTE",
                    keymap::apply_palette_overrides(palette::list_commands(), &cfg.keys),
                );
                palette_lines(
                    &mut out,
                    "TASK VIEW PALETTE",
                    keymap::apply_palette_overrides(palette::task_commands(), &cfg.keys),
                );
                let _ = writeln!(
                    out,
                    "LEADER ({leader}, then …) — works in every task pane; set with leader.<name>"
                );
                for (cmd, _, what) in LeaderCmd::ALL {
                    let _ = writeln!(
                        out,
                        "  {}  {:<40} leader.{}",
                        keymap::leader_key_of(&cfg.keys, cmd),
                        what,
                        cmd.name()
                    );
                }
                out.push_str("  Esc palette · leader twice sends the leader key to the shell\n\n");
                out.push_str(
                    "VIEWS  Home (board + Today) · Plan · Task · Settings · Help (this overlay)\n\n\
                     HOME: BOARD\n  ↑/↓ j/k move · Enter open · J/K (Shift+↑/↓) reorder in column · [ ] move column\n  \
                     / filter by id or title · A show/hide archived · d delete · n new · m timer · v done\n  \
                     p plan the day · Tab → Today\n\n\
                     HOME: TODAY\n  ↑/↓ move · Enter start the timer · Space tick · J/K order · x remove · o open task\n  \
                     p plan · Tab → board\n\n\
                     PLAN\n  Space pick · s suggest · a add a free item · t add one for the task · Tab/←→ sides\n  \
                     J/K order · x remove · A all columns · Enter save · Esc cancel\n\n\
                     PANES (task view)\n  Ctrl+Tab / Ctrl+Shift+Tab or Alt+] / Alt+[   next / previous pane\n  \
                     Ctrl+1..9 / Alt+1..9   select shell N\n\n\
                     FILES\n  Enter open/toggle · ←/→ collapse/expand · a new file · A new folder · r rename\n  \
                     d delete · . hidden · t shell · R refresh (the tree also refreshes by itself)\n\n\
                     EDITOR\n  Ctrl+S save · Ctrl+W close · Ctrl+Z undo · Ctrl+Y redo · Ctrl+F find · F3/Ctrl+G next\n  \
                     Shift+arrows/Home/End/PgUp/PgDn select · Ctrl+←/→ (Alt+←/→, Alt+B/F) by word, +Shift selects\n  \
                     Ctrl+Home/End file start/end · Ctrl+A select all\n  \
                     Ctrl+C copy · Ctrl+X cut · Ctrl+V paste (nothing selected: copy/cut the line)\n  \
                     Ctrl+Backspace (Ctrl+H, Alt+Backspace) / Ctrl+Delete (Alt+D) delete a word\n  \
                     Mouse: drag selects · double-click word · triple-click line\n  \
                     Typing replaces the selection. In the editor Ctrl+C copies: quit with Esc q.\n  \
                     Copies reach the system clipboard via OSC 52 (tmux: set -g set-clipboard on),\n  \
                     or via the Copy command setting (wl-copy, xclip -selection clipboard, pbcopy).\n  \
                     Ctrl+V pastes the last copy, or the Paste command's output (wl-paste -n, pbpaste).\n  \
                     Your terminal's own paste (Ctrl+Shift+V, Cmd+V) also works.\n  \
                     Long Markdown and text lines wrap (setting: Soft wrap).\n\n\
                     TERMINAL\n  all keys go to the shell · Shift+PgUp/PgDn scroll · mouse goes to programs that ask\n  \
                     Shift+drag selects with your terminal emulator (in every pane)\n",
                );
            }
            HelpTopic::Work => {
                let _ = write!(
                    out,
                    "CHECKPOINTS\n  Ordered steps with estimates in CONTEXT.md under ## Checkpoints:\n    \
                     - [ ] Read the datasheet chapter (30m)\n    - [x] Write the driver skeleton (1h; spent 1h10m)\n  \
                     Write them yourself, or Esc b lets the AI plan them (needs a ready context).\n  \
                     Esc k lists them: Space ticks, Enter starts the timer on one.\n\n\
                     THE TIMER CHIP (bottom-right corner, always visible)\n  \
                     Shows what is timed and the time left. Click it, or Esc m / {leader} m, for the\n  \
                     timer menu: start · pause · resume · done → next · +5 / +15 min · stop.\n  \
                     v ticks the current checkpoint and moves the timer on; M stops and books.\n  \
                     Tasks without open checkpoints get a focus block ({} min).\n\n\
                     WHEN TIME IS UP\n  The screen flashes, the bell rings (settings: flash / bell), the chip blinks\n  \
                     until you open the timer menu, and the timer_expire hook runs. Nothing takes\n  \
                     your keys away: you decide when to look. Overtime keeps counting.\n\n\
                     IDLE\n  No key press or click for {} min (0 = off) pauses the timer at your last\n  \
                     input. Resume from the menu and choose whether the time away counts.\n  \
                     The same happens after a restart: the timer comes back paused and offers the\n  \
                     time pahiri was closed.\n\n\
                     BOOKKEEPING\n  Minutes go to the checkpoint (spent), the task (time_spent), <tasks>/timelog.tsv\n  \
                     (one line per booking: time, task, minutes, what) and ## Log (one line per session).\n  \
                     Dates: created, started (first timer or first move out of column 1), finished\n  \
                     (moved to the last column). Esc O records a one-line outcome for reviews.\n\n\
                     PLAN THE DAY (p on Home, Esc y anywhere: the Plan view)\n  \
                     Left: open checkpoints of the tasks in progress (setting: Plan from columns;\n  \
                     A shows every open column) and what the last plan left unfinished.\n  \
                     Space picks (on a task: all of it) · s suggests: carried-over items, then each\n  \
                     task's next checkpoint in board order until the day is full ({}) ·\n  \
                     a adds a free item (\"Email the vendor 15m\") · t adds one for the task.\n  \
                     Right: the plan in order · J/K move · x removes · Enter saves · Esc cancels.\n  \
                     Estimates are scaled by your pace (spent ÷ estimated on finished checkpoints).\n\n\
                     TODAY (Home, right side; Tab moves there)\n  \
                     The plan: Enter starts the timer on an item · Space ticks it (a checkpoint is\n  \
                     ticked in its CONTEXT.md) · J/K order · x removes · o opens the task.\n  \
                     The timer's done → next follows the plan, across tasks.\n  \
                     Below: time booked today / this week, your pace, and the open work.\n  \
                     Finished tasks older than {} days are archived (A shows them).\n\n\
                     THE PLAN FILE  <tasks>/.pahiri/plans/<date>.md — yours to edit too:\n    \
                     - [ ] PROJ-42 · Write the parser (1h)     a checkpoint of PROJ-42\n    \
                     - [ ] PROJ-42 · Reply to review (20m)     a task item (timed on PROJ-42)\n    \
                     - [ ] Email the vendor (15m)              a free item\n  \
                     Scripts: pahiri plan show [--json] · pahiri plan add [--task ID] <text 20m>.\n  \
                     Hooks: day_start (first run of a day), plan_save.\n",
                    cfg.timer.focus_minutes,
                    cfg.timer.idle_minutes,
                    crate::tasks::checkpoints::fmt_minutes(cfg.planner.day_minutes),
                    cfg.archive_after_days
                );
            }
            HelpTopic::Agents => {
                let paths: Vec<String> = crate::ai::PromptKind::ALL
                    .iter()
                    .map(|k| {
                        format!(
                            "  {:<40} {}",
                            k.label(),
                            cfg.prompt_path(*k, &self.config_path).display()
                        )
                    })
                    .collect();
                let _ = write!(
                    out,
                    "ONE-SHOT AGENT (Esc i context · Esc b checkpoints)\n  \
                     now: {} {}\n  An argument containing {{prompt}} receives the prompt; otherwise it is sent on\n  \
                     stdin (always for prompts over 100 kB). Runs in the first attached workspace\n  \
                     (else the task folder) with PAHIRI_TASK, PAHIRI_TASK_DIR, PAHIRI_CODE_DIR(S),\n  \
                     PAHIRI_CONTEXT_FILE. stdout is the answer; Esc cancels; timeout {}s.\n  \
                     With `claude -p` tools that need approval are refused, so it cannot edit files.\n  \
                     Examples:  claude -p   ·   codex exec {{prompt}}   ·   ollama run llama3\n\n\
                     PROMPT TEMPLATES (Esc E opens them; created on first use)\n{}\n\n\
                     PLACEHOLDERS\n  {{{{task}}}} {{{{task_dir}}}} {{{{context_file}}}} {{{{context}}}} (whole CONTEXT.md)\n  \
                     {{{{workspaces}}}} {{{{builds}}}} {{{{branch}}}} {{{{next_checkpoint}}}} {{{{max_words}}}}\n  \
                     {{{{estimate_factor}}}} (your spent÷estimated ratio). Replaced in one pass: text\n  \
                     from tickets is never expanded again.\n\n\
                     WHAT PAHIRI APPENDS (fixed, cannot be edited away)\n  \
                     1. a safety note: CONTEXT.md and ticket text are data, not instructions;\n  \
                     2. the output format it parses:\n     \
                     context      Markdown, at most {} words (limit ≤ 1000), `###` headings, and a\n                  \
                     last line `CONTEXT_READY: yes` or `CONTEXT_READY: no - <what is missing>`\n     \
                     checkpoints  only lines `- [ ] <step> (<estimate>)`, 3–12 of them\n\n\
                     CONTEXT READY\n  Gate for Esc b. Flipped by the agent's verdict, by Esc r, or by\n  \
                     `pahiri task ready [--off]` — all the same switch (`- context_ready: true`).\n\n\
                     CODING AGENT ({leader} a · Esc l)\n  now: {} {}  (starts in {})\n  \
                     Opens a new task shell and types: cd <dir> && <command> <args> \"$(cat <prompt file>)\"\n  \
                     ({{prompt}} in the arguments places it; an empty template sends none).\n",
                    cfg.agent.command,
                    shell_words::join(&cfg.agent.args),
                    cfg.agent.timeout_secs,
                    paths.join("\n"),
                    cfg.agent.context_max_words,
                    cfg.coding_agent.command,
                    shell_words::join(&cfg.coding_agent.args),
                    if cfg.coding_agent.start_in_code { "the code workspace" } else { "the task folder" }
                );
            }
            HelpTopic::Hooks => {
                out.push_str(
                    "Hooks are shell commands pahiri runs when something happens. Configure them in\n\
                     Esc c → Hooks as `event = command`, or in config.toml:\n\n  \
                     [hooks]\n  task_enter   = \"~/bin/pahiri-enter.sh\"\n  \
                     timer_expire = \"notify-send pahiri \\\"$PAHIRI_CHECKPOINT: time is up\\\"\"\n\n\
                     They run through `sh -c` in the task folder (tasks folder when there is no\n\
                     task), with the variables below, and time out after the hook timeout.\n\
                     task_enter and task_create are waited for (a small log shows while they run)\n\
                     and the task view is drawn afterwards; the rest run in the background.\n\
                     Hooks change pahiri by editing files or calling `$PAHIRI_BIN task …`\n\
                     (ready, log, move, outcome, next); pahiri reloads CONTEXT.md, the board and\n\
                     the file tree when they change. A failing hook only shows a status line.\n\n\
                     Esc ! runs a hook now (PAHIRI_MANUAL=1), e.g. startup after you cloned a repo.\n\
                     config.toml is reloaded when it changes on disk, so a hook can change settings\n\
                     with `$PAHIRI_BIN config add-workspace|add-build|prune …` — see\n\
                     examples/hooks/discover-workspaces.sh, which fills in workspaces and yocto builds\n\
                     from what is on disk.\n\n\
                     EVENTS\n",
                );
                for e in HookEvent::ALL {
                    let mark = if cfg.hooks.contains_key(e.name()) {
                        "●"
                    } else {
                        " "
                    };
                    let _ = writeln!(out, "  {mark} {:<22} {}", e.name(), e.description());
                    for (k, v) in e.extra_env() {
                        let _ = writeln!(out, "        ${k:<24} {v}");
                    }
                }
                out.push_str("\nVARIABLES EVERY HOOK GETS\n");
                for (k, v) in COMMON_ENV {
                    let _ = writeln!(out, "  ${k:<24} {v}");
                }
                out.push_str("\nCONFIGURED\n");
                if cfg.hooks.is_empty() {
                    out.push_str("  none yet — examples/hooks/ has ready-made ones\n");
                }
                for (k, v) in &cfg.hooks {
                    let _ = writeln!(out, "  {k} = {v}");
                }
                out.push_str("\nRECENT RUNS (newest first)\n");
                if self.hook_runs.is_empty() {
                    out.push_str("  none yet\n");
                }
                for r in self.hook_runs.iter().rev() {
                    let _ = writeln!(
                        out,
                        "  {} {} {}{} · {} ms · {}",
                        crate::time::short(&r.at),
                        r.event,
                        r.task.as_deref().unwrap_or("-"),
                        if r.ok { "" } else { " · FAILED" },
                        r.millis,
                        r.command
                    );
                    for l in r
                        .output
                        .lines()
                        .rev()
                        .take(6)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                    {
                        let _ = writeln!(out, "      {l}");
                    }
                }
                out.push_str(
                    "\nEXAMPLES (examples/hooks/)\n  \
                     notify.sh            timer_expire → desktop notification\n  \
                     start-moves-task.sh  timer_start → move the task out of the first column\n  \
                     finish-reminder.sh   task_move → remind you to record an outcome\n  \
                     enter-fetch.sh       task_enter → fetch the attached repos before you look\n",
                );
            }
            HelpTopic::Sources => out.push_str(TASK_SOURCE_FORMAT),
            HelpTopic::Gerrit => {
                let _ = writeln!(
                    out,
                    "now: Gerrit URL = {} · status command = {}\n",
                    if cfg.gerrit_url.is_empty() {
                        "(from origin)"
                    } else {
                        &cfg.gerrit_url
                    },
                    if cfg.gerrit_status_command.is_empty() {
                        "(none)"
                    } else {
                        &cfg.gerrit_status_command
                    }
                );
                out.push_str(GERRIT_FORMAT);
            }
            HelpTopic::Cli => out.push_str(
                "For scripts, hooks and agents. The task defaults to $PAHIRI_TASK (set in every\n\
                 pahiri shell and hook); pass --task ID otherwise.\n\n  \
                 pahiri task next                 current checkpoint and the one after\n  \
                 pahiri task log <message…>       add a line to ## Log\n  \
                 pahiri task ready [--off]        flip context_ready\n  \
                 pahiri task move --to <column>   move on the board (name or 0-based index)\n  \
                 pahiri task outcome <text…>      add a line to ## Outcome\n  \
                 pahiri report [--from D] [--to D] [--json]   tasks active in a range (reviews)\n  \
                 pahiri plan show [--date D] [--json]         the day plan with each item's state\n  \
                 pahiri plan add [--task ID] <text 20m>       add to today's plan\n  \
                 pahiri config add-workspace NAME PATH [--main B]   add / update a workspace\n  \
                 pahiri config add-build NAME PATH            add / update a vendor build\n  \
                 pahiri config remove-workspace|remove-build NAME\n  \
                 pahiri config prune                          drop entries whose folder is gone\n  \
                 pahiri config list                           workspaces and builds, tab separated\n  \
                 pahiri trash empty [--older-than 30d]        delete old trashed tasks\n  \
                 pahiri install-skills <dir> [--force]        write the bundled agent skills\n  \
                 pahiri --show-config             config, log and state paths\n\n\
                 A running pahiri notices their changes within a second.\n",
            ),
            HelpTopic::Files => {
                let _ = write!(
                    out,
                    "config          {}\n\
                     prompts         {}\n\
                     tasks           {}\n\
                     board           {}   (## Column headings, - task lines)\n\
                     time ledger     {}/timelog.tsv   (UTC time<TAB>task<TAB>minutes<TAB>what)\n\
                     trash           {}/.trash/<id>-<timestamp>\n\
                     state           {}   (session.json: timer + shells; shell rc files)\n\n\
                     CONTEXT.md (one per task) — yours, except what pahiri manages:\n  \
                     ## Context      between <!-- pahiri:context --> markers (Esc i)\n  \
                     ## Checkpoints  between <!-- pahiri:checkpoints --> markers\n  \
                     ## Outcome      one line per Esc O / pahiri task outcome\n  \
                     ## Log          timestamped lines (timer, ✓ checkpoints, pahiri task log)\n  \
                     ## Attachments  <!-- pahiri:begin --> … <!-- pahiri:end -->: source, link,\n                  \
                     workspace, build, branch, prepared, gerrit, created, started,\n                  \
                     finished, time_spent, context_ready\n\
                     Saving CONTEXT.md while pahiri changed it on disk merges: your edits win, pahiri's\n\
                     updates to parts you did not touch are kept, new log lines are added.\n",
                    self.config_path.display(),
                    self.config
                        .prompt_path(crate::ai::PromptKind::Context, &self.config_path)
                        .parent()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                    cfg.tasks_dir.display(),
                    cfg.status_path().display(),
                    cfg.tasks_dir.display(),
                    cfg.tasks_dir.display(),
                    self.state_dir.display(),
                );
            }
            HelpTopic::Troubleshooting => {
                let _ = write!(
                    out,
                    "Log file: {} (pahiri --show-config prints it)\n\n\
                     A hook fails        → Hooks tab: recent runs with their output.\n\
                     A task source fails → settings, `t` on the source: shows the error and format.\n\
                     The agent fails     → its log shows the command, the prompt size and stderr.\n\
                     Gerrit finds nothing→ is the commit-msg hook installed? is the task branch pushed\n\
                                           to the right main? Esc g prints what it compared.\n\
                     Keys do nothing     → in a shell every key goes to the shell: use {leader} first.\n\
                     Ctrl+Tab / Ctrl+1   → need the kitty keyboard protocol; Alt+] / Alt+1 always work.\n\
                     Text selection      → hold Shift while dragging (pahiri captures the mouse).\n\
                     Shells after restart→ Restore shells reopens them in the same folders; with the\n\
                                           tmux setting the programs themselves keep running.\n",
                    crate::config::Config::state_dir().map_or_else(|| "(unknown)".into(), |d| d.join("pahiri.log").display().to_string()),
                );
            }
        }
        out
    }
}
