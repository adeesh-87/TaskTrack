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
| `m` / `M` | timer: start · pause · resume / stop and book the time |
| `v` | checkpoint done → timer moves on to the next |
| `i` | AI: write the task context (≤ word limit) and judge if it is ready |
| `r` | context ready: toggle |
| `b` | AI: break the task into checkpoints (needs a ready context) |
| `l` | launch the coding agent in a new shell (also `leader a`) |
| `g` | find Gerrit changes on the task branches |
| `E` | edit the AI prompt templates |
| `D` | delete the task (moved to `.trash`) |
| `T` | back to the task list |
| `p` | **prepare**: commit, pull main, switch attached repos to the task branch |
| `a` | **attach** code workspaces / vendor builds to the task |
| `s` / `x` | new shell / close selected shell |
| `o` | open `CONTEXT.md` |
| `f` `e` `w` `t` | focus files / editor / shell list / terminal |
| `z` | zoom the terminal |
| `[` `]` | move the task to the previous / next category |
| `n` | new task (custom, or from a task source such as Jira or Orbit) |
| `c` | configuration |
| `h` | help |
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
and the terminal's scrollback, click in the editor to place the cursor.
Programs that ask for mouse reporting (vim, less, agent CLIs) get the events
forwarded in the encoding they requested. Hold **Shift** while dragging to
select text with your terminal emulator as usual. `mouse = false` in the
config turns capture off.

### Syntax highlighting

The editor highlights C, C++, Rust, Bash/zsh, Python, CMake, Makefiles, log
files (timestamps, `ERROR`/`WARN`/`INFO`/`DEBUG` levels, `key=value` pairs,
`[tags]`) and Markdown, detected from the file name or a `#!` line. The
lexers are small and hand-written (no grammar files, instant startup) and
follow the colour scheme. `syntax_highlighting = false` turns it off.

## Doing the work

pahiri's job is that you always know what to do next.

* **Next up.** The task list's right side shows, per open column, each
  task's next checkpoint (or what it is missing: a context, a plan).
* **Context → ready → checkpoints.** `Esc i` asks your AI agent to write a
  short `## Context` for the task (goal, background, done-when, risks,
  review notes) from `CONTEXT.md` and the attached code, capped at the word
  limit (max 1000). The agent also says whether the task is defined well
  enough to plan; that flips `context_ready`. You can flip it yourself with
  `Esc r`, and scripts/agents with `pahiri task ready` — all three go
  through the same function. With a ready context, `Esc b` breaks the task
  into checkpoints: `- [ ] step (40m)`, written to `## Checkpoints`.
* **Timer.** `m` starts a countdown on the next open checkpoint (its
  estimate minus time already spent), shown in the status bar everywhere.
  When it runs out the whole screen flashes, the bell rings, and you
  choose: done → next checkpoint, +5 / +15 min, keep going (overtime
  counts), pause, stop. `v` ticks the checkpoint and moves the timer on.
  Tasks without checkpoints get a focus block (25 min by default).
* **Accounting.** Minutes are booked to the checkpoint (`spent 25m`) and
  the task (`time_spent`), with a `## Log` line per session and per ticked
  checkpoint. pahiri records `created`, `started` (first timer / moved out
  of the first column — which it does for you when you start the timer)
  and `finished` (moved to the last column, where it asks for a one-line
  outcome). That is what `pahiri report` and the `task-retro` skill use
  for reviews.

## AI agents

Two kinds, both configured on the settings page (`?` there explains):

| | one-shot agent (`Esc i`, `Esc b`) | coding agent (`leader a`, `Esc l`) |
| --- | --- | --- |
| default | `claude -p {prompt}` | `claude` |
| runs | in the background, output → `CONTEXT.md` | interactively in a new task shell |
| prompt | template + fixed output format | startup template |

Templates live in `prompts/` next to the config file (created on first
use; `Esc E` opens them). They are Markdown with placeholders such as
`{{task}}`, `{{context}}`, `{{workspaces}}`, `{{next_checkpoint}}`.
pahiri always appends a fixed "output format" section to the one-shot
prompts, so the part it parses cannot be edited away — including "less is
more" and the word limit. Any CLI agent works: an argument containing
`{prompt}` receives the prompt, otherwise it goes to stdin.

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

`Esc g` scans every attached workspace for commits on the task branch
that are not on its main branch (`origin/<main>` when present) and carry a
`Change-Id:` trailer — the id Gerrit's commit-msg hook adds. Each becomes a
`- gerrit:` line in `CONTEXT.md` with a link `https://<host>/q/<Change-Id>`;
the host comes from the `origin` remote (`ssh://you@host:29418/project` →
`https://host`) unless *Gerrit URL* is set.

## Command line

```sh
pahiri task next                     # current checkpoint ($PAHIRI_TASK or --task ID)
pahiri task log "found the root cause"
pahiri task ready [--off]
pahiri report --from 2026-01-01 --to 2026-06-30 [--json]
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
| `agent.command` / `agent.args` | `claude` / `["-p", "{prompt}"]` | one-shot agent; empty command disables |
| `agent.timeout_secs` | 600 | |
| `agent.context_max_words` | 600 | 1–1000 |
| `agent.context_prompt` / `agent.checkpoint_prompt` | "" | template paths; empty: `prompts/*.md` next to the config |
| `coding_agent.command` / `.args` | `claude` / [] | `{prompt}` receives the startup prompt, else appended |
| `coding_agent.startup_prompt` | "" | template path |
| `coding_agent.start_in_code` | true | else the task folder |
| `timer.focus_minutes` | 25 | timer for tasks without checkpoints |
| `timer.flash` / `timer.bell` | true / true | when time is up |
| `color_scheme` | `dark` | `dark`, `light`, `gruvbox`, `nord`, `solarized` |
| `shell.program` | `zsh` | `shell.args` defaults to `["-i"]` |
| `leader_key` | `ctrl+b` | e.g. `ctrl+a`, `ctrl+space`, `alt+x` |
| `font_family` | JetBrains Mono | advisory: terminal emulators own the font |
| `large_file_kb` | 512 | bigger files ask before opening; binaries always ask |
| `show_hidden` | false | `.` toggles at runtime |
| `mouse` | true | mouse capture (Shift+drag still selects text) |
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
| task list | `↑/↓` move · `Enter` open · `[`/`]` move task · `n` new · `d` delete · `m` timer · `v` checkpoint done · `K` checkpoints · `q` quit |
| files | `Enter` open/toggle · `←/→` collapse/expand · `a`/`A` new file/folder · `r` rename · `d` delete · `.` hidden · `t` shell |
| shells | `Enter` focus · `n` new · `x` close · `←` hide pane |
| editor | `Ctrl+S` save · `Ctrl+W` close |

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
