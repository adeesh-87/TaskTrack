# pahiri

A terminal workspace for executing and managing tasks. Tasks live on the
left, the files and shells of the selected task on the right — in the spirit
of [herdr](https://github.com/herdrdev/herdr), but organised around a task
board instead of a pane grid.

```
┌ tasks ───────┐┌ files ─────────────┐┌ CONTEXT.md ───────────────────────────────┐
│PLANNED 2     ││▾ scripts           ││ 1 # fix-login                             │
│  write-docs  ││    run.sh          ││ 2                                         │
│  spike-x     ││  CONTEXT.md        ││ 3 Repro steps ...                         │
│DOING 1       │└────────────────────┘└───────────────────────────────────────────┘
│▶ fix-login   │┌ shells 2 ──────────┐┌ zsh #1 · claude ──────────────────────────┐
│DONE 0        ││● zsh #1 · claude   ││ $ claude                                  │
│              ││● zsh #2            ││ ╭──────────────────────────╮              │
└──────────────┘└────────────────────┘└───────────────────────────────────────────┘
```

## What it does

* **Board.** The left 20 % lists tasks grouped by category (`Planned`,
  `Doing`, `Done` by default). A task is a folder `tasks/<task_id>/`
  containing at least a `CONTEXT.md`, plus whatever scripts and data belong
  to it. Which column a task is in is stored in `tasks/status.md` — a tiny
  Markdown file with one heading per category and one list item per task,
  so it stays human-editable and diffs well in git.
* **Task view.** `Enter` on a task opens it: the sidebar becomes a file tree
  (top) and the list of shells opened for that task (bottom); the right side
  shows the one open document (top) and the selected shell (bottom). Panes
  that have nothing to show are not drawn. Each task remembers its open
  document and its shells; switching tasks switches all of it.
* **Files.** `Enter` opens a file (folders toggle). Large or binary files
  ask first with a **WARNING** popup; binaries open read-only as a hex dump.
  Rename (`r`), new file (`a`), new folder (`A`) and delete (`d`, asks first)
  live in the tree.
* **Editor.** A small text editor: arrows, Home/End/PgUp/PgDn, insert,
  delete, `Ctrl+S` save, `Ctrl+W` close. One document at a time.
* **Shells.** `t` opens a shell (zsh by default) in the task folder with
  `PAHIRI_TASK` and `PAHIRI_TASK_DIR` set. Shells run on a pseudo-terminal
  and are rendered through a VT parser into pahiri's own pane, so a shell —
  or a full-screen agent CLI launched from it — can never take over the
  screen. Full-screen programs simply see a terminal the size of the pane;
  `leader z` zooms the pane to the whole screen when you want the room.
* **Leader key.** While a shell has focus every key goes to it. The only
  exception is the leader (default `ctrl+b`, like tmux and herdr):
  `leader q` leaves the shell, `leader z` zooms, `leader n` new shell,
  `leader x` close, `leader h`/`l` previous/next, `leader [`/`]` scroll,
  `leader leader` sends the leader key itself. `Shift+PgUp/PgDn` scroll too.

## Getting started

```sh
cd pahiri
cargo install --path .
pahiri
```

On first start there is no config, so the settings page opens: set the
tasks folder, adjust anything else, press `Ctrl+S`, then `Esc`. Settings
are always reachable with `,` from the task list.

Useful flags:

```
pahiri --tasks-dir ~/work/tasks   # seed/override the tasks folder
pahiri --config ./pahiri.toml     # use a different config file
pahiri --show-config              # print resolved config and paths
```

The config lives at `~/.config/pahiri/config.toml` (XDG) and logs go to
`~/.local/state/pahiri/pahiri.log`.

### Settings

| key               | default          | notes                                                     |
| ----------------- | ---------------- | --------------------------------------------------------- |
| `tasks_dir`       | —                | required                                                  |
| `categories`      | Planned, Doing, Done | at least one; also the headings in `status.md`        |
| `color_scheme`    | `dark`           | `dark`, `light`, `gruvbox`, `nord`, `solarized`           |
| `shell.program`   | `zsh`            | any program; `shell.args` defaults to `["-i"]`            |
| `leader_key`      | `ctrl+b`         | e.g. `ctrl+a`, `ctrl+space`, `alt+x`                      |
| `font_family`     | JetBrains Mono   | advisory — terminal emulators own the font                |
| `large_file_kb`   | 512              | bigger files ask before opening                           |
| `show_hidden`     | false            | `.` toggles at runtime                                    |
| `tab_width`       | 4                | editor rendering                                          |
| `scrollback_lines`| 5000             | per shell                                                 |
| `status_file`     | `status.md`      | board file inside `tasks_dir`                             |
| `context_file`    | `CONTEXT.md`     | expected in every task folder (⚠ shown when missing)      |

## Keys

Press `?` anywhere outside a shell for the full list.

| where       | keys                                                                 |
| ----------- | -------------------------------------------------------------------- |
| task list   | `↑/↓` move · `Enter` open · `[`/`]` move task between categories · `n` new task · `r` rescan · `,` settings · `q` quit |
| task view   | `Tab`/`Shift+Tab` cycle panes · `Esc` back                            |
| files       | `Enter` open/toggle · `←/→` collapse/expand · `a`/`A` new file/folder · `r` rename · `d` delete · `.` hidden · `t` shell |
| shells      | `Enter` focus · `n` new · `x` close · `←` hide pane                    |
| editor      | `Ctrl+S` save · `Ctrl+W` close · `Esc` back to files                  |
| terminal    | everything goes to the shell except `leader …` (see above)            |

## Development

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Layout:

```
src/
  config/    config model, TOML persistence, key-combo parsing
  tasks/     task discovery + the Markdown board (status.md)
  files/     lazy file tree and file operations
  editor/    minimal text buffer
  terminal/  PTY sessions, key encoding, VT screen → ratatui rendering
  app/       state machine (modes, focus, popups, leader handling)
  ui/        drawing only
```

`scripts/smoke.py` drives the real binary inside a pseudo-terminal and
prints the rendered screens (needs `pip install pexpect pyte`); handy for
checking a change end to end without a real terminal.
