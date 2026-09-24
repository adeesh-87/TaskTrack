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
| `src/app/mod.rs` | `App`, modes (`Config`/`TaskList`/`Task`), key handling, popups, `run_action`, `run_pending`, job results |
| `src/app/palette.rs` | `Action` enum + palette entries (key, label) for list and task view |
| `src/app/popup.rs` | `Popup` variants and `Pending` (what a confirmed popup does) |
| `src/app/work.rs` | delete (to `.trash`), timer start/pause/stop/alarm, checkpoints, dates, "next up" |
| `src/app/agent.rs` | AI context / checkpoints, coding agent launch, prompt editing, Gerrit |
| `src/app/timer.rs` | pure timer arithmetic (budget, overtime, booking minutes) |
| `src/app/context.rs` | `TaskContext`: per-task tree, shells, editor, focus, meta, checkpoints |
| `src/app/config_form.rs` | settings page model (`FieldKey`, fields, list editing, `to_config`) |
| `src/app/mouse.rs`, `ui_state.rs` | mouse handling and recorded geometry |
| `src/app/keymap.rs` | help texts (`HELP`, `TASK_SOURCE_HELP`, `AGENT_HELP`) |
| `src/tasks/` | disk model: `store.rs` (folders, board), `board.rs` (`status.md`), `context.rs` (managed block, log, context section), `checkpoints.rs`, `sections.rs` (marker helpers), `record.rs` (read-only summary), `sources.rs` (ticket scripts) |
| `src/ai/mod.rs` | prompt templates, fixed output formats, `run()` for one-shot agents |
| `src/git/mod.rs` | `prepare` sequence, main-branch detection, Gerrit `Change-Id` scan |
| `src/terminal/` | PTY sessions, key/mouse encoding, VT rendering, shell `cd` integration |
| `src/editor/`, `src/files/`, `src/highlight/` | text buffer, file tree/ops, syntax lexers |
| `src/ui/` | ratatui drawing: `mod.rs` (layout, status bar, flash), `task_list`, `task_view` (sidebar, next up, editor, terminal), `popup`, `config_page`, `theme` |
| `src/time.rs` | RFC 3339 formatting without a date crate |

## State on disk

- Config: `~/.config/pahiri/config.toml`; prompt templates in `prompts/` next to it.
- Tasks: `<tasks_dir>/<ID>/CONTEXT.md` + anything else; board in `<tasks_dir>/status.md`;
  deleted tasks in `<tasks_dir>/.trash/`.
- Runtime: `~/.local/state/pahiri/` (log, generated shell rc files, per-task env files, coding-agent prompts).

pahiri re-reads a task's `CONTEXT.md` when its mtime changes (checked every
tick), so agents and `pahiri task …` can edit it while the TUI runs.
