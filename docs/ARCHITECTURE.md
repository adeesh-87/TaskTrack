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
- **`pahiri edit`** is the same `App` built by `App::new_editor`: `code_mode`
  is set, the one `TaskContext` comes from `TaskContext::standalone` (its
  roots are `fixed_roots`, not attachments), and task-only things refuse to
  run (`Action::in_editor`, `fire_hook`, `save_session`, `back_to_list`).
  Code-editing features must work in both.
- **Language servers** are child processes with their own reader thread
  (`src/lsp`); `App::lsp_sync` (every tick and before a request) sends
  opened / changed / closed documents, answers come back as `JobEvent::Lsp`
  and are matched to what was asked in `LspState::pending`.

## Modules

| path | what |
| ---- | ---- |
| `src/main.rs` | CLI parsing (clap), subcommands, terminal setup, event loop, bell |
| `src/cli.rs` | `pahiri task ready/log/next`, `report`, `install-skills` (bundled skills are `include_str!`'d from `.agents/skills/`) |
| `src/config/` | `Config` (TOML), defaults, `validate()`, key-combo parsing |
| `src/app/mod.rs` | `App`, views (`Mode::Home`/`Plan`/`Task`/`Settings`; Help is a popup), key handling, popups, `run_action`, `run_pending`, job results |
| `src/app/audit.rs` | the audit: fetch job, agent step, Audit view (`AuditView`), applying (create / update tasks, report) |
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
| `src/app/context.rs` | `TaskContext`: per-task tree, shells, editor + `recent` buffers (the Ctrl+Tab stack), focus, meta, checkpoints |
| `src/app/config_form.rs` | settings page model (`FieldKey`, fields, list editing, `to_config`) |
| `src/app/find.rs` | the editor's find / replace bar (`FindBar`, `build_regex`, match cache per buffer revision) |
| `src/app/finder.rs` | Ctrl+P quick open (`FileIndex` built in a thread with the `ignore` crate, `nucleo-matcher` fuzzy / regex) and Ctrl+O path completion (`Popup::Finder`) |
| `src/app/search.rs` | search in files: `SearchPanel` (kept in `App::search` for F4, shown as `Popup::Search`), parallel walk + regex in a thread, results as `JobEvent::SearchHits` |
| `src/app/complete.rs` | Ctrl+Space: the completion list (server items via `lsp.rs`, else words of the open buffers) |
| `src/app/lsp.rs` | code intelligence glue: `LspState` (servers per command + project root, open documents synced on tick, diagnostics, pending requests, jump stack) and the ctags `TagIndex`; F12 / Shift+F12 / Ctrl+K / Ctrl+T / Ctrl+Shift+O / F8 / Alt+←→ |
| `src/app/mouse.rs`, `ui_state.rs` | mouse handling (editor drag / word / line selection) and recorded geometry |
| `src/app/clipboard.rs` | editor copy/paste: internal clipboard, `copy_command` / OSC 52 (written by `main.rs` after a frame), `paste_command` |
| `src/app/keymap.rs` | leader commands table, key overrides (`[keys]`) and their validation |
| `src/hooks.rs` | hook events (names, descriptions, env docs) and the `sh -c` runner |
| `src/tasks/` | disk model: `audit.rs` (matching tickets and changes to tasks → proposals; agent answer parsing), `plan.rs` (day plan files in `.pahiri/plans/`), `store.rs` (folders, board, trash, outside changes), `board.rs` (`status.md`), `context.rs` (managed block, log, context, dates), `checkpoints.rs`, `sections.rs` (marker helpers), `merge.rs` (save-merge of CONTEXT.md), `ledger.rs` (`timelog.tsv`), `record.rs` (read-only summary), `sources.rs` (ticket and Gerrit status script output) |
| `src/ai/mod.rs` | prompt templates, fixed output formats, `run()` for one-shot agents |
| `src/git/mod.rs` | `prepare`, main-branch detection, `Change-Id` scan, fetch, push for review, rebase |
| `src/terminal/` | PTY sessions, key/mouse encoding, VT rendering, shell `cd` integration |
| `src/lsp/` | LSP transport, no app types: `Server` (spawn, JSON-RPC framing, reader thread → `JobEvent::Lsp`), UTF-16/32 columns, URIs, `find_root`, answer parsers; `ctags.rs` builds / parses / ranks tags |
| `src/editor/`, `src/files/`, `src/highlight/` | text buffer (cursor, selection anchor, word moves, undo, indentation, CRLF, disk mtime), file tree (task folder + attached roots, `reveal`) / ops, syntax lexers |
| `src/ui/` | ratatui drawing: `mod.rs` (layout, status bar, flash), `task_list` (the board), `today` (Home's Today pane), `task_card` (Home's task card), `plan_view`, `audit_view`, `task_view` (sidebar, editor with find bar / match highlights / completion list, terminal), `popup`, `search_view`, `config_page`, `theme` |
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
they changed on disk (and `config.toml`: `App::reload_config_if_changed`,
applied like a save from Settings through `apply_config`), so hooks, agents and `pahiri task …` can edit them
while the TUI runs. Saving `CONTEXT.md` from the editor merges changes made
meanwhile (`tasks/merge.rs`).
