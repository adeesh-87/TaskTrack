# Architecture

## Flow

```
main.rs ── input thread ─┐
          PTY readers ───┼─► mpsc<AppEvent> ─► App::handle(event) ─► state changes
          job threads ───┘                                   │
          tick (≤250ms) ─────────────────────────────────────┘
loop: terminal.draw(ui::draw(&mut app)) → wait for next event → app.handle
```

- **One owner of state:** `App` (`src/app/mod.rs`). Everything mutates it
  inside `App::handle`. No locks.
- **Slow work runs in threads** (git, scripts, agents) and reports back with
  `AppEvent::Job(JobEvent::…)` (`src/app/event.rs`). Progress lines go to the
  log popup via `JobEvent::Log`.
- **Drawing is read-only** (`src/ui/`), except recording geometry in
  `app.ui` for mouse hit-testing and resizing PTYs to their pane.

## Modules

| path | what |
| ---- | ---- |
| `src/main.rs` | CLI parsing (clap), subcommands, terminal setup, event loop, bell |
| `src/cli.rs` | `pahiri task ready/log/next`, `report`, `install-skills` (bundled skills are `include_str!`'d from `.agents/skills/`) |
| `src/config/` | `Config` (TOML), defaults, `validate()`, key-combo parsing |
| `src/app/mod.rs` | `App`, views (`Mode::Home`/`Plan`/`Task`/`Settings`; Help is a popup), key handling, popups, `run_action`, `run_pending`, job results |
| `src/app/plan.rs` | the Plan view (`PlanView`: pick, suggest, order, save), the Today pane's keys, plan ↔ timer (`next_in_plan`), `day_start` |
| `src/app/palette.rs` | `Action` enum + palette entries (key, label) for list and task view |
| `src/app/popup.rs` | `Popup` variants and `Pending` (what a confirmed popup does) |
| `src/app/work.rs` | delete (to `.trash`), timer menu / alarm / idle / booking, checkpoints, dates, records cache, "today / next up", watching outside changes |
| `src/app/agent.rs` | AI context / checkpoints, coding agent launch, prompt editing |
| `src/app/gerrit.rs` | Gerrit scan (fetch, hook check, status command), push for review, rebase |
| `src/app/hooks.rs` | running user hooks: env, waiting hooks (`task_enter`, `task_create`), recent runs |
| `src/app/help.rs` | the tabbed help page (built from the live config) and the script format texts |
| `src/app/session.rs` | `session.json`: timer and shells across restarts |
| `src/app/timer.rs` | pure timer arithmetic (budget, overtime, idle/away, booking minutes, save/restore) |
| `src/app/context.rs` | `TaskContext`: per-task tree, shells, editor, focus, meta, checkpoints |
| `src/app/config_form.rs` | settings page model (`FieldKey`, fields, list editing, `to_config`) |
| `src/app/mouse.rs`, `ui_state.rs` | mouse handling (editor drag / word / line selection) and recorded geometry |
| `src/app/clipboard.rs` | editor copy/paste: internal clipboard, `copy_command` / OSC 52 (written by `main.rs` after a frame), `paste_command` |
| `src/app/keymap.rs` | leader commands table, key overrides (`[keys]`) and their validation |
| `src/hooks.rs` | hook events (names, descriptions, env docs) and the `sh -c` runner |
| `src/tasks/` | disk model: `plan.rs` (day plan files in `.pahiri/plans/`), `store.rs` (folders, board, trash, outside changes), `board.rs` (`status.md`), `context.rs` (managed block, log, context, dates), `checkpoints.rs`, `sections.rs` (marker helpers), `merge.rs` (save-merge of CONTEXT.md), `ledger.rs` (`timelog.tsv`), `record.rs` (read-only summary), `sources.rs` (ticket and Gerrit status script output) |
| `src/ai/mod.rs` | prompt templates, fixed output formats, `run()` for one-shot agents |
| `src/git/mod.rs` | `prepare`, main-branch detection, `Change-Id` scan, fetch, push for review, rebase |
| `src/terminal/` | PTY sessions, key/mouse encoding, VT rendering, shell `cd` integration |
| `src/editor/`, `src/files/`, `src/highlight/` | text buffer (cursor, selection anchor, word moves, undo), file tree/ops, syntax lexers |
| `src/ui/` | ratatui drawing: `mod.rs` (layout, status bar, flash), `task_list` (the board), `today` (Home's Today pane), `plan_view`, `task_view` (sidebar, editor, terminal), `popup`, `config_page`, `theme` |
| `src/time.rs` | RFC 3339 formatting without a date crate |

## State on disk

- Config: `~/.config/pahiri/config.toml`; prompt templates in `prompts/` next to it.
- Tasks: `<tasks_dir>/<ID>/CONTEXT.md` + anything else; board in `<tasks_dir>/status.md`;
  deleted tasks in `<tasks_dir>/.trash/`.
- Time ledger: `<tasks_dir>/timelog.tsv`.
- Day plans: `<tasks_dir>/.pahiri/plans/YYYY-MM-DD.md` (dot-folder: not a task).
- Runtime: `~/.local/state/pahiri/` (log, `session.json`, `last_day`, generated shell rc and tmux files, per-task env files, coding-agent prompts).

About once a second (`App::reload_outside_changes`) pahiri re-reads the
board, the task folders, the open task's `CONTEXT.md` and its file tree when
they changed on disk, so hooks, agents and `pahiri task …` can edit them
while the TUI runs. Saving `CONTEXT.md` from the editor merges changes made
meanwhile (`tasks/merge.rs`).
