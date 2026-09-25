# pahiri

A terminal workspace for executing and managing tasks. Tasks live on the
left, the files and shells of the selected task on the right — in the spirit
of [herdr](https://github.com/herdrdev/herdr), but organised around a task
board, with a vim-style command palette on `Esc`.

```
┌ tasks ───────┐ ◀ PROJ-42  code: fw · branch: PROJ-42
│PLANNED 2     │┌ files ─────────────┐┌ CONTEXT.md ───────────────────────────────┐
│  write-docs  ││  CONTEXT.md        ││ 1 # PROJ-42: Fix login                    │
│  spike-x     ││▾ scripts           ││ 2 Source: jira                            │
│DOING 1       ││    run.sh          ││ 3 Link: https://jira/browse/PROJ-42       │
│▶ PROJ-42     │└────────────────────┘└───────────────────────────────────────────┘
│DONE 0        │┌ shells 2 ──────────┐┌ zsh #1 · claude ──────────────────────────┐
│              ││● zsh #1 · claude   ││ $ cd code && claude                       │
│              ││● zsh #2            ││ ╭──────────────────────────╮              │
└──────────────┘└────────────────────┘└───────────────────────────────────────────┘
```

## Views

| view | what | how |
| ---- | ---- | --- |
| **Home** | the **Board** (columns of tasks, left), the **Task card** (what the selected task is: title, goal, link, its CRs, progress) and the **Today** pane (your plan, what is timed, time booked, open work) | start, `Esc T` |
| **Plan** | choose today's work: pick checkpoints of the tasks in progress, order them | `p` on Home, `Esc y` anywhere |
| **Audit** | review what the audit proposes: tasks to create, tasks to update, changes to place | `Esc U` |
| **Task** | one task: Files, Editor, Shells and Terminal panes | `Enter` on a task |
| **Settings** | the settings form | `Esc c`, `,` on Home |
| **Help** | an overlay on any view | `F1`, `?` |

Also named: the **Palette** (`Esc`), the **Leader** key (`Ctrl+B`), the
**Timer chip** (bottom right) and its **Timer menu**.

## Concepts

* **Task** — a folder `tasks/<task_id>/` with at least a `CONTEXT.md`, plus
  whatever scripts and data belong to it. Task ids never contain spaces so
  they double as git branch names (`PROJ-123`, `ORB-42`, `my-spike`).
* **Board** — `tasks/status.md`, a tiny Markdown file with one heading per
  category (`Planned`, `Doing`, `Done` by default) and one list item per
  task. Human-editable, diffs well, and pahiri keeps it in sync with the
  folders that exist.
* **Checkpoints** — the task's ordered steps with time estimates, in
  `CONTEXT.md`, driven by the timer (see *Doing the work*).
* **Attachments** — code workspaces (git checkouts) and vendor builds
  (e.g. yocto trees) declared once in the configuration and attached to
  tasks. pahiri records them in a managed block at the end of `CONTEXT.md`,
  so switching to a task only needs that file:

  ```markdown
  ## Attachments
  <!-- pahiri:begin -->
  - source: jira
  - link: https://jira.example.com/browse/PROJ-123
  - workspace: firmware
  - build: yocto-2024
  - branch: PROJ-123
  - prepared: 2026-09-24T10:00:00Z
  <!-- pahiri:end -->
  ```

## The command palette (`Esc`)

`Esc` opens the palette anywhere except inside a shell (there, `leader Esc`).
Press a shortcut letter to run a command straight away, or `:` to type and
filter, `Enter` to run, `Esc` to close.

| key | command |
| --- | ------- |
| `k` | checkpoints (tick with Space, Enter starts the timer on one) |
| `m` / `M` | timer menu (start · pause · resume · done → next · +5/+15 · stop) / stop and book |
| `v` | checkpoint done → timer moves on to the next |
| `i` | AI: write the task context (≤ word limit) and judge if it is ready |
| `r` | context ready: toggle |
| `b` | AI: break the task into checkpoints (needs a ready context) |
| `l` | launch the coding agent in a new shell (also `leader a`) |
| `g` | Gerrit: find changes on the task branches (+ their status) |
| `P` / `R` | Gerrit: push for review / rebase the task branches onto main |
| `O` | record a one-line outcome (for reviews) |
| `F` | find in the editor (F3 / Ctrl+G: next) |
| `E` | edit the AI prompt templates |
| `D` | delete the task (moved to `.trash`) |
| `T` | back to Home |
| `y` | plan your day (the Plan view; `p` on Home) |
| `U` | audit: match your tickets and Gerrit changes to tasks |
| `!` | run a hook now (e.g. re-run `startup`) |
| `p` | **prepare**: commit, pull main, switch attached repos to the task branch |
| `a` | **attach** code workspaces / vendor builds to the task |
| `s` / `x` | new shell / close selected shell |
| `o` | open `CONTEXT.md` |
| `f` `e` `w` `t` | focus files / editor / shell list / terminal |
| `z` | zoom the terminal |
| `[` `]` | move the task to the previous / next category |
| `n` | new task (custom, or from a task source such as Jira or Orbit) |
| `c` | configuration |
| `h` | the help page (also `F1`, `?`) |
| `q` | quit |

### Panes and shells

* `Ctrl+Tab` / `Ctrl+Shift+Tab` cycle files → editor → shells → terminal,
  from every pane including the terminal. `Alt+]` / `Alt+[` do the same.
* `Ctrl+1..9` (or `Alt+1..9`) select shell N and focus the terminal.
* Inside the terminal every other key goes to the shell. The leader
  (default `ctrl+b`, like tmux and herdr) prefixes pahiri commands:
  `leader q` leave, `leader z` zoom, `leader n` new, `leader x` close,
  `leader h`/`l` previous/next shell, `leader Esc` palette,
  `leader a` coding agent, `leader m` timer, `leader v` checkpoint done,
  `leader leader` sends the leader key itself. `Shift+PgUp/PgDn` scroll.
  The leader works from every task pane, not only the terminal.

Ctrl+Tab and Ctrl+digit need the
[kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)
(kitty, WezTerm, foot, Ghostty, Alacritty, recent iTerm2). pahiri enables it
when the terminal supports it; the `Alt` aliases work everywhere.

Shells run on a pseudo-terminal parsed by a VT emulator and painted into the
pane, so a shell — or a full-screen agent CLI started from it — can never
take over the screen. Full-screen programs simply see a terminal the size of
the pane; `leader z` gives them the whole screen.

After a restart pahiri reopens each task's shells in the folders they
were in (setting *Restore shells*). With *Shells in tmux* every shell runs
in its own tmux session on a private socket (`tmux -L pahiri ls`), so the
programs in it keep running while pahiri is closed and are reattached
next time; closing a shell in pahiri ends its session.

Keys can be changed in `[keys]`: `"palette.<action>" = "x"` for palette
letters and `"leader.<command>" = "x"` for leader commands; the help page
lists every name next to its current key.

Every shell starts in the task folder with `PAHIRI_TASK`, `PAHIRI_TASK_DIR`,
`PAHIRI_CODE_DIR(S)` and `PAHIRI_BUILD_DIR(S)` set, and (for zsh and bash) a
`cd` wrapper:

```sh
cd task           # the task folder
cd code           # the first attached code workspace
cd code app       # the workspace named "app"
cd build [name]   # an attached vendor build
```

pahiri never edits your dotfiles: zsh gets a generated `ZDOTDIR` whose
startup files source your own first; bash gets `--rcfile`. The wrapper reads
a per-task env file on each call, so attaching a workspace takes effect in
shells that are already open.

### Mouse

The mouse works everywhere: click a pane to focus it, click a task, file or
shell to select it, double-click to open, wheel to scroll lists, the editor
and the terminal's scrollback. In the editor, click places the cursor, drag
selects (past the pane edge it scrolls), double-click selects a word and
triple-click a line. Programs that ask for mouse reporting (vim, less, agent
CLIs) get the events forwarded in the encoding they requested. In shells and
elsewhere, hold **Shift** while dragging to select with your terminal
emulator as usual. `mouse = false` in the config turns capture off.

### Editing

| keys | what |
| ---- | ---- |
| `Shift` + arrows / `Home` / `End` / `PgUp` / `PgDn` | select |
| `Ctrl+←/→` (`Alt+←/→`, `Alt+B`/`Alt+F`) | move by word; add `Shift` to select |
| `Ctrl+Home` / `Ctrl+End` | start / end of the file |
| `Ctrl+A` | select all |
| `Ctrl+C` / `Ctrl+X` | copy / cut the selection (nothing selected: the whole line) |
| `Ctrl+V` | paste |
| `Ctrl+Backspace` (`Ctrl+H`, `Alt+Backspace`) / `Ctrl+Delete` (`Alt+D`) | delete a word back / forward |
| `Ctrl+Z` / `Ctrl+Y` | undo / redo |
| `Ctrl+F`, `F3` / `Ctrl+G` | find, next match |
| `Ctrl+S` / `Ctrl+W` | save / close |

Typing, `Enter`, `Backspace` or a paste replace the selection. In the editor
`Ctrl+C` copies; quit with `Esc` `q`.

**Clipboard.** Copied text goes to your system clipboard through OSC 52,
which most terminals support (kitty, WezTerm, Alacritty, foot, iTerm2,
Windows Terminal, xterm with `allowWindowOps`; inside tmux set
`set -g set-clipboard on`). If yours does not (e.g. GNOME Terminal), set
`copy_command` (`wl-copy`, `xclip -selection clipboard`, `pbcopy`).
`Ctrl+V` pastes what you copied in pahiri, or the output of `paste_command`
(`wl-paste -n`, `xclip -o -selection clipboard`, `pbpaste`) when it is set.
Your terminal's own paste (`Ctrl+Shift+V`, `Cmd+V`, middle click) always works
too.

### Syntax highlighting

The editor highlights C, C++, Rust, Bash/zsh, Python, CMake, Makefiles, log
files (timestamps, `ERROR`/`WARN`/`INFO`/`DEBUG` levels, `key=value` pairs,
`[tags]`) and Markdown, detected from the file name or a `#!` line. The
lexers are small and hand-written (no grammar files, instant startup) and
follow the colour scheme. `syntax_highlighting = false` turns it off.

## Help page

`F1` (or `?`, `Esc h`, `leader ?` in a shell) opens a tabbed help page:
keys (with your overrides), timer and work, AI agents and prompt
placeholders, **hooks** (events, variables, what you configured and the
last runs with their output), the **Jira / Orbit** script format, the
**Gerrit** status format, the CLI, the files pahiri writes, and
troubleshooting. `?` on a setting opens the matching tab.

## Doing the work

pahiri's job is that you always know what to do next.

* **Plan the day.** `p` on Home opens the Plan view:

  ```
   Plan · Fri 25 Sep  4h35m of 6h ████████░░░░  · estimates × 1.30 (your pace)
  ┌ What can be planned ───────────────┐┌ Today, in order ───────────────────┐
  │ CARRIED OVER · Thu 24 Sep          ││  1. PROJ-51 · Address review  40m  │
  │    [x] Address review  30m PROJ-51 ││  2. PROJ-42 · Write the parser 1h20m│
  │ DOING                              ││  3. Email the vendor           15m │
  │  PROJ-42 Fix login                 ││                                    │
  │    [x] Write the parser  1h        ││                                    │
  │    [ ] Write tests  45m            ││                                    │
  └────────────────────────────────────┘└────────────────────────────────────┘
  ```

  The left side lists the open checkpoints of the tasks in progress (the
  middle columns; setting *Plan from columns*, `A` shows every open column)
  and yesterday's unfinished items. `Space` picks (on a task: all its
  checkpoints), `s` suggests a day — carried-over items, then each task's
  next checkpoint in board order, round after round, until the *Day
  capacity* (6h) is full, with estimates scaled by your pace. `a` adds a
  free item (`Email the vendor 15m`), `t` an item for the task under the
  cursor. On the right, `J`/`K` order and `x` removes. `Enter` saves.
* **Task card.** Above the Today pane, Home shows the selected task (on
  the board, or the plan item's task): its title, one line of what it is
  (the `### Goal` of its AI context, else the first line of its
  description), the ticket link, its Gerrit changes with their status,
  and the next checkpoint, progress and dates. The Today pane's open work
  lists each task's title next to its next step.
* **Today.** Home's right side shows the plan: `Tab` moves there, `Enter`
  starts the timer on an item, `Space` ticks it (a checkpoint is ticked in
  its task's `CONTEXT.md` — one source of truth), `J`/`K` reorder, `x`
  removes, `o` opens the task. **The timer's done → next follows the
  plan**, across tasks. Below the plan: time booked today and this week,
  your pace (spent ÷ estimated), and the open work on the board.
* **The plan file** is `<tasks>/.pahiri/plans/<date>.md`, plain Markdown
  you, scripts and agents can edit (`pahiri plan add`, `pahiri plan show`):
  `- [ ] PROJ-42 · Write the parser (1h)` points at a checkpoint,
  `- [ ] Email the vendor (15m)` is a free item. Hooks `day_start` and
  `plan_save` run on the first start of a day and when you save a plan.
* **Context → ready → checkpoints.** `Esc i` asks your AI agent to write a
  short `## Context` for the task (goal, background, done-when, risks,
  review notes) from `CONTEXT.md` and the attached code, capped at the word
  limit (max 1000). The agent also says whether the task is defined well
  enough to plan; that flips `context_ready`. You can flip it yourself with
  `Esc r`, and scripts/agents with `pahiri task ready` — all three go
  through the same function. With a ready context, `Esc b` breaks the task
  into checkpoints: `- [ ] step (40m)`, written to `## Checkpoints`.
* **The timer chip.** The bottom-right corner always shows the timer:
  what is timed and the time left. Click it — or `Esc m` / `leader m` — for
  the timer menu: start, pause, resume, done → next checkpoint, +5 / +15
  min, stop. `v` ticks the current checkpoint and moves the timer on.
  Tasks without checkpoints get a focus block (25 min by default).
* **Time's up** never takes your keys: the screen flashes, the bell rings,
  the chip blinks `TIME'S UP` until you open the menu yourself, and the
  `timer_expire` hook runs (use it for a desktop notification). Overtime
  keeps counting.
* **Idle and restarts.** No key press or click for 15 minutes pauses the
  timer at your last input; when you resume from the menu you choose
  whether the time away counts. Quitting keeps the timer: it comes back
  paused and offers the time pahiri was closed.
* **Accounting.** Minutes are booked to the checkpoint (`spent 25m`), the
  task (`time_spent`) and `<tasks>/timelog.tsv` (one line per booking),
  with a `## Log` line per session and per ticked checkpoint. pahiri
  records `created`, `started` (first timer / moved out of the first
  column) and `finished` (moved to the last column). `Esc O` adds a
  one-line outcome. That is what `pahiri report` and the `task-retro`
  skill use for reviews. Finished tasks older than 14 days are archived
  from the list (`A` shows them); `/` filters by id or title.

## Hooks

pahiri runs your scripts when things happen — no behaviour is baked in
that a hook can do instead. Configure `event = command` on the settings
page or in the config:

```toml
[hooks]
task_enter   = "~/bin/pahiri-enter.sh"            # waited for, then the task view is drawn
timer_expire = "notify-send pahiri \"$PAHIRI_CHECKPOINT: time is up\""
timer_start  = "~/src/pahiri/examples/hooks/start-moves-task.sh"
```

Events: `startup`, `task_create`, `task_enter`, `task_leave`,
`task_move`, `task_delete`, `attach`, `prepare_done`, `timer_start`,
`timer_pause`, `timer_resume`, `timer_stop`, `timer_expire`,
`checkpoint_done`, `context_generated`, `checkpoints_generated`, `gerrit`,
`day_start`, `plan_save`, `periodic`, `audit_apply`.
Every hook gets `PAHIRI_TASK`, `PAHIRI_TASK_DIR`, `PAHIRI_CONTEXT_FILE`,
`PAHIRI_COLUMN`, `PAHIRI_CODE_DIR(S)`, `PAHIRI_BIN` and more; each event
adds its own (e.g. `PAHIRI_FROM_COLUMN` / `PAHIRI_FINISHED` for
`task_move`). The help page's Hooks tab lists all of them. `task_enter`
and `task_create` are waited for; the rest run in the background. Hooks
talk back through files and `$PAHIRI_BIN task …`; pahiri picks up changes
to the board, the task folders, `CONTEXT.md` and the file tree within a
second. Ready-made ones are in `examples/hooks/`.

**Run a hook now:** `Esc !` lists your hooks and runs the one you pick for
the current task (with `PAHIRI_MANUAL=1`) — e.g. re-run `startup` after
you cloned a repo.

**Config changes on the fly.** pahiri reloads `config.toml` within a second
when it changes on disk — from a hook, `pahiri config …`, or your editor.
A file with errors is ignored (the status line says why) and the previous
settings stay; unsaved edits on the Settings view win until you leave it.
A workspace or build whose folder is missing is only a warning, so pahiri
still starts; `pahiri config prune` drops them.

**Example: workspaces and builds from what is on disk.**
`examples/hooks/discover-workspaces.sh` finds git checkouts (folders with a
`.git`, skipping ones nested in another checkout) and bitbake/yocto build
folders (with `conf/local.conf`), registers them with `pahiri config
add-workspace` / `add-build`, and prunes the ones that are gone:

```toml
[hooks]
startup = "CODE_ROOTS=~/src:~/work BUILD_ROOTS=~/yocto ~/src/pahiri/examples/hooks/discover-workspaces.sh"
```

Names come from the folder (`parent-name` when two share one); a new
workspace gets the main branch git reports. Existing entries keep their
name and branch, and a folder you registered under another name is not
added twice.

To keep it in sync without thinking about it, run it on a timer too: the
`periodic` hook runs every *Periodic hook (min)* minutes (setting
`periodic_minutes`, 0 = off; one run at a time):

```toml
periodic_minutes = 10

[hooks]
startup  = "~/src/pahiri/examples/hooks/discover-workspaces.sh"
periodic = "~/src/pahiri/examples/hooks/discover-workspaces.sh"
```

The script only writes when something changed, so pahiri reloads only
then. `Esc !` → `startup` runs it right now.

## Audit: tickets and changes → tasks

`Esc U` fetches your tickets (every task source, run with
`PAHIRI_AUDIT=1` and `PAHIRI_AUDIT_SINCE=<date>` so they include finished
ones) and your Gerrit changes (*Audit: Gerrit command*,
`examples/gerrit-mine.sh`), and matches them to your tasks:

* a ticket belongs to the task with its id or its link;
* a change belongs to the task that already lists its `Change-Id`, whose
  id / branch / ticket its topic or branch names, or whose ticket key
  (`PROJ-42`) is in its subject;
* what is left goes to your AI agent (*Audit: use the agent*), which maps
  changes to tasks, groups changes into new tasks, or maps a ticket to a
  task you made by hand — marked `AI` in the proposal.

The **Audit view** lists the proposal: new tasks (in the right column:
done tickets and merged-only change groups go to the last one), updates
(new changes and status changes, link, empty description, earlier
created / started dates, finished date, done → last column) and changes
nothing claimed. `Space` ticks, `Enter` decides (attach a change to a
task, make a task for it, leave it; rename a new task; record a ticket on
an existing task), `a` applies. Applying writes each task's
`CONTEXT.md` — title, link, `## Description`, `- gerrit:` lines, dates —
adds an `audit: …` line to its `## Log`, updates the board, writes a
report to `<tasks>/.pahiri/audit/` and runs the `audit_apply` hook.
Running it again only proposes what changed. Then `Esc i` in a task lets
the agent write its `## Context`. Formats: help page → Audit.

| setting | default | |
| ------- | ------- | - |
| `audit.gerrit_command` | "" | prints your changes (JSON lines) |
| `audit.since_days` | 90 | how far back |
| `audit.use_agent` | true | let the agent place what the rules could not |
| `audit.done_statuses` / `audit.progress_statuses` | [] | your workflow's extra status names |

## AI agents

Two kinds, both configured on the settings page (`?` there explains):

| | one-shot agent (`Esc i`, `Esc b`) | coding agent (`leader a`, `Esc l`) |
| --- | --- | --- |
| default | `claude -p` (prompt on stdin) | `claude` |
| runs | in the background, output → `CONTEXT.md` | interactively in a new task shell |
| prompt | template + fixed output format | startup template |

Templates live in `prompts/` next to the config file (created on first
use; `Esc E` opens them). They are Markdown with placeholders such as
`{{task}}`, `{{context}}`, `{{workspaces}}`, `{{next_checkpoint}}`.
pahiri always appends a fixed "output format" section to the one-shot
prompts, so the part it parses cannot be edited away — including "less is
more" and the word limit, and a note that ticket text is data, not
instructions. Placeholders are filled in one pass, so text from a ticket
is never expanded again. Any CLI agent works: an argument containing
`{prompt}` receives the prompt, otherwise it goes to stdin (always for
prompts over 100 kB).

### Skills

`.agents/skills/` holds skills for your agents; they are also built into
the binary:

```sh
pahiri install-skills ~/.claude/skills        # or <tasks>/.agents/skills
```

| skill | for |
| ----- | --- |
| `less` | terse, objective, technical answers with minimal tokens |
| `plan-first` | propose a structured plan (checkpoints in pahiri's format) and wait for a yes |
| `task-retro` | reviews: what you did in a period, impact, evidence, estimation accuracy, learnings |
| `standup` | yesterday / today / blockers from the task logs |
| `unstick` | one tiny next action when you are stuck or procrastinating |

## Gerrit

`Esc g` fetches `origin/<main>` in every attached workspace (batch-mode
ssh, never prompts; if that fails it says how old the local copy is),
warns when Gerrit's `commit-msg` hook is missing, and lists commits on
the task branch that are not on main and carry a `Change-Id:` trailer.
Each becomes a `- gerrit:` line in `CONTEXT.md` linked as
`https://<host>/q/<Change-Id>` (host from the `origin` remote, or
*Gerrit URL*). With a *Gerrit status command* set, pahiri also records
each change's status and votes, e.g. `[NEW #1234 CR+2 V+1]` — the
command gets the Change-Ids and prints JSON lines (format on the help
page; `examples/gerrit-status.sh` does it with `ssh gerrit query`).

`Esc P` pushes each workspace that is on the task branch for review
(`git push origin HEAD:refs/for/<main>`), `Esc R` rebases it onto the
latest `origin/<main>` (a conflict is aborted and reported). Both ask
first.

## Command line

```sh
pahiri task next                     # current checkpoint ($PAHIRI_TASK or --task ID)
pahiri task log "found the root cause"
pahiri task ready [--off]
pahiri task move --to Done           # column name or index; records the dates
pahiri task outcome "shipped the fix; root cause was a stale token"
pahiri report --from 2026-01-01 --to 2026-06-30 [--json]
pahiri plan show [--date D] [--json]  # today's plan with each item's state
pahiri plan add [--task ID] Reply to review 20m
pahiri config add-workspace fw ~/src/fw [--main develop]   # add or update
pahiri config add-build imx ~/yocto/build-imx
pahiri config remove-workspace fw | remove-build imx
pahiri config prune                  # drop workspaces / builds whose folder is gone
pahiri config list                   # kind, name, path, main branch (tab separated)
pahiri trash empty --older-than 30d
pahiri install-skills <dir> [--force]
```

## Task sources (Jira, Orbit, …)

A task source is a script that prints tickets. Configure one per tool:

```toml
[[task_sources]]
name = "jira"
command = "~/bin/jira-mine.sh"

[[task_sources]]
name = "orbit"
command = "python3 ~/tools/orbit-list.py --assigned-to-me"
```

On the settings page, `?` on *Task sources* shows the exact output format
and `t` on a source runs it and shows what pahiri parsed (nothing is
created). `examples/jira.sh` (curl + jq) and `examples/template.py` are
starting points.

`Esc`, `n` then lists *custom task* plus one entry per source. Picking a
source runs its command (through `sh -c`, with `PAHIRI_TASKS_DIR` set),
shows the tickets in a filterable list, and creates the chosen one:

* the task id is the ticket id (unsafe characters become `-`),
* `CONTEXT.md` gets the title, `Source:` and `Link:` lines, the description
  under `## Description`, and the managed block with `source` and `link`.

The script's stdout may be a JSON array, JSON lines, or tab separated lines:

```json
[{"id": "PROJ-123", "title": "Fix login", "url": "https://…/PROJ-123", "description": "…"}]
```

```
ORB-42<TAB>Do the thing<TAB>https://orbit/ORB-42<TAB>first line\nsecond line
```

`key`/`summary`/`link`/`body` are accepted as aliases. A non-zero exit shows
the script's stderr.

## Attach and prepare

Declare checkouts and builds once (`Esc`, `c`):

```toml
default_main_branch = "main"

[[workspaces]]
name = "firmware"
path = "/home/me/src/firmware"
main_branch = "develop"     # each workspace can have its own main branch

[[workspaces]]
name = "app"
path = "/home/me/src/app"

[[builds]]
name = "yocto-2024"
path = "/builds/yocto-2024"
```

Inside a task, `Esc`, `a` ticks the ones that belong to it. `Esc`, `p`
then prepares every attached workspace:

1. if the checkout has uncommitted changes, commit them as
   `pahiri: state saved on <branch> before switching to <task>` — on the
   main branch this is refused with a warning first, and you choose to
   commit there anyway, skip that workspace, or cancel;
2. `git checkout <main>` and `git pull --ff-only` (a missing remote is
   reported, not fatal);
3. `git switch <task>` if the branch exists, else `git switch -c <task>`.

Progress streams into a log popup, and `CONTEXT.md` records `branch` and
`prepared`. `cd code` in a shell then lands on the task branch.

## Getting started

```sh
cargo install --path .
pahiri
```

On first start the settings page opens: set the tasks folder, press
`Ctrl+S`, then `Esc`. Everything else can be changed later with `Esc`, `c`.

```
pahiri --tasks-dir ~/work/tasks   # seed/override the tasks folder
pahiri --config ./pahiri.toml     # use a different config file
pahiri --show-config              # print resolved config and paths
```

Config: `~/.config/pahiri/config.toml`. Logs and generated shell files:
`~/.local/state/pahiri/`.

### Settings

| key | default | notes |
| --- | ------- | ----- |
| `tasks_dir` | — | required |
| `categories` | Planned, Doing, Done | at least one; also the headings in `status.md` |
| `workspaces` | [] | `name`, `path`, optional `main_branch` |
| `builds` | [] | `name`, `path` |
| `task_sources` | [] | `name`, `command` |
| `default_main_branch` | `main` | for workspaces without their own `main_branch` |
| `gerrit_url` | "" | empty: derived from each workspace's `origin` |
| `gerrit_status_command` | "" | reports change status; format on the help page |
| `hooks` | {} | `event = "command"`, see *Hooks* |
| `hook_timeout_secs` | 15 | |
| `periodic_minutes` | 0 | run the `periodic` hook this often (0: never) |
| `keys` | {} | `"palette.timer" = "u"`, `"leader.coding_agent" = "A"` — names on the help page |
| `archive_after_days` | 14 | hide finished tasks older than this (0: never) |
| `soft_wrap` | true | wrap Markdown and text in the editor |
| `copy_command` | "" | gets copied text on stdin (`wl-copy`, `pbcopy`); empty: OSC 52 |
| `paste_command` | "" | `Ctrl+V` pastes its output (`wl-paste -n`, `pbpaste`); empty: pahiri's last copy |
| `restore_shells` | true | reopen each task's shells in the same folders after a restart |
| `shell.tmux` | false | run shells in tmux sessions (socket `pahiri`) that survive pahiri |
| `agent.command` / `agent.args` | `claude` / `["-p"]` | one-shot agent; empty command disables |
| `agent.timeout_secs` | 600 | |
| `agent.context_max_words` | 600 | 1–1000 |
| `agent.context_prompt` / `agent.checkpoint_prompt` | "" | template paths; empty: `prompts/*.md` next to the config |
| `coding_agent.command` / `.args` | `claude` / [] | `{prompt}` receives the startup prompt, else appended |
| `coding_agent.startup_prompt` | "" | template path |
| `coding_agent.start_in_code` | true | else the task folder |
| `timer.focus_minutes` | 25 | timer for tasks without checkpoints |
| `timer.flash` / `timer.bell` | true / true | when time is up |
| `timer.idle_minutes` | 15 | pause after this long without input (0: never) |
| `planner.day_minutes` | 360 | how much planned work fits in a day (form: `6h`) |
| `planner.columns` | [] | columns the Plan view offers; empty: all but the last, and but the first with 3+ columns |
| `color_scheme` | `dark` | `dark`, `light`, `gruvbox`, `nord`, `solarized` |
| `shell.program` | `zsh` | `shell.args` defaults to `["-i"]` |
| `leader_key` | `ctrl+b` | e.g. `ctrl+a`, `ctrl+space`, `alt+x` |
| `font_family` | JetBrains Mono | advisory: terminal emulators own the font |
| `large_file_kb` | 512 | bigger files ask before opening; binaries always ask |
| `show_hidden` | false | `.` toggles at runtime |
| `mouse` | true | mouse capture (drag selects in the editor; Shift+drag uses the terminal's selection) |
| `syntax_highlighting` | true | C, C++, Rust, Bash, Python, CMake, Make, logs, Markdown |
| `tab_width` | 4 | editor rendering |
| `scrollback_lines` | 5000 | per shell |
| `status_file` | `status.md` | board file inside `tasks_dir` |
| `context_file` | `CONTEXT.md` | expected in every task folder (⚠ shown when missing) |

On the settings page lists (categories, workspaces, builds, task sources)
are edited in place: `a` or `Enter` on `+ add` adds an item, `Enter` edits
one, `d` deletes. Workspaces are written as `name = /path @main-branch`
(a new workspace gets the branch git reports; `@main (default)` follows
*Default main branch*), builds as `name = /path`, task sources as
`name = command`. `?` explains the selected setting.

## Other keys

| where | keys |
| ----- | ---- |
| Home: board | `↑/↓` move · `Enter` open · `J`/`K` reorder · `[`/`]` move task · `/` filter · `A` archived · `n` new · `d` delete · `m` timer · `v` checkpoint done · `O` outcome · `p` plan · `Tab` Today · `q` quit |
| Home: Today | `↑/↓` move · `Enter` timer · `Space` tick · `J`/`K` order · `x` remove · `o` open task · `p` plan · `Tab` board |
| Audit | `Space` tick · `Enter` decide / rename · `A` all/none · `a` apply · `Esc` leave |
| Plan | `Space` pick · `s` suggest · `a` / `t` add · `Tab` / `←→` switch sides · `J`/`K` order · `x` remove · `A` all columns · `Enter` save · `Esc` cancel |
| files | `Enter` open/toggle · `←/→` collapse/expand · `a`/`A` new file/folder · `r` rename · `d` delete · `.` hidden · `t` shell |
| shells | `Enter` focus · `n` new · `x` close · `←` hide pane |
| editor | `Ctrl+S` save · `Ctrl+W` close · `Ctrl+Z`/`Ctrl+Y` undo/redo · `Ctrl+F` find · `F3`/`Ctrl+G` next · selection and clipboard: see *Editing* |

Large or binary files ask before opening; binaries open read-only as a hex
dump. Rename, create and delete always act on the task folder only.

## Development

Start with `AGENTS.md` (also for your own coding agent): it points to
`docs/ARCHITECTURE.md`, `docs/HOWTO.md` and `docs/FORMATS.md`.

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
python3 scripts/smoke.py   # drives the real binary in a PTY (pip install pexpect pyte)
```

```
src/
  ai/        prompt templates, fixed output formats, one-shot agent runner
  cli.rs     pahiri task/report/install-skills
  config/    config model, TOML persistence, key-combo parsing
  tasks/     task discovery, the Markdown board, CONTEXT.md metadata, ticket sources
  files/     lazy file tree and file operations
  editor/    minimal text buffer
  git/       prepare, main-branch detection, Gerrit Change-Id scan
  highlight/ hand-written lexers: C-like family, Makefile, log, Markdown
  terminal/  PTY sessions, key encoding, VT rendering, shell integration
  app/       state machine: modes, focus, palette, popups, jobs
  ui/        drawing only
```
