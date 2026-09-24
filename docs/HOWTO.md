# How to make common changes

Each recipe lists every place to touch. Run fmt, clippy and tests after.

## Add a palette command (Esc menu)

1. `src/app/palette.rs`: add a variant to `Action`; add
   `cmd('<key>', "<label>", Action::X)` to `list_commands()` and/or
   `task_commands()`. Keys must be unique per list (a test checks).
2. `src/app/mod.rs` → `run_action`: add `Action::X => self.do_x(),`.
3. Implement `do_x` in the fitting file (`work.rs`, `agent.rs`, or `mod.rs`).
4. Give it a name in `Action::name` / `Action::ALL` (used by `[keys]` overrides and
   the help page, which lists the palette automatically). README palette table.
5. Test in `src/app/tests.rs`: `h.press(key(KeyCode::Esc)); h.press(key(KeyCode::Char('<key>')));`
   then assert on `h.app` or the files.

## Add a direct key

- Task list: `handle_list_key` in `src/app/mod.rs`.
- Task panes: `handle_tree_key` / `handle_editor_key` / `handle_shell_list_key`.
  Editor movement keys go through `move_in_editor` (Shift selects); editing
  operations belong on `Buffer` (`src/editor/mod.rs`) with a unit test there.
- After the leader key (any task pane): add a `LeaderCmd` in `src/app/keymap.rs`
  (name, default key, description) and handle it in `handle_leader_command`.
Prefer calling `self.run_action(Action::X)` so keys and palette stay in sync.

## Add a setting

1. `src/config/mod.rs`: field on `Config` (or `AgentConfig` / `CodingAgentConfig` /
   `TimerConfig`) with a doc comment, a default in `Default`, a check in
   `validate()` if values can be wrong. `#[serde(default)]` keeps old files loading.
2. `src/app/config_form.rs`: `FieldKey` variant; a `field(...)` entry in
   `ConfigForm::new` (label, help, `Value::Text|Number|Toggle|List`); a match arm in
   `to_config`. Map the key to a help tab in `App::config_help` (`src/app/mod.rs`).
3. README settings table.

## Add a popup

1. `src/app/popup.rs`: variant on `Popup` (and `title()` arm).
2. `src/app/mod.rs` → `handle_popup_key`: the popup is `take()`n; put it back
   (`self.popup = Some(...)`) unless the key closes it.
3. `src/ui/popup.rs` → `draw`: produce its `lines`.
For yes/no, input or choices, reuse `Popup::confirm/input/choose` with a new
`Pending` variant handled in `run_pending`.

## Record something in CONTEXT.md

- Machine field (dates, flags): add it to `TaskMeta` in `src/tasks/context.rs`
  (`parse` + `render_block`), write with `update_meta(path, id, |m| …)`.
- Timestamped line: `append_log(path, id, &now_rfc3339(), "text")` or
  `append_under(..., "## Heading", ...)`.
- A pahiri-owned block of text: `sections::upsert` with new markers.
Then call `self.after_task_file_change(id)` so the UI reloads it.

## Add a hook event

1. `src/hooks.rs`: variant in `HookEvent` (+ `ALL`, `name`, `description`,
   `extra_env` for its own variables; `blocks` if pahiri must wait for it).
2. Call `self.fire_hook(HookEvent::X, Some(&task_id), extra_env, AfterHook::Nothing)`
   where it happens. The help page's Hooks tab picks it up automatically.
3. A test: configure `cfg.hooks.insert("x".into(), "echo … > file".into())` and
   `h.pump_until` for the file (see `task_enter_hook_runs_before_the_task_view…`).

## Add help text

Edit `help_text` in `src/app/help.rs` (one match arm per tab). Script formats
users must follow live there too (`TASK_SOURCE_FORMAT`, `GERRIT_FORMAT`).

## Run something in the background

Copy `run_agent` (`src/app/agent.rs`) or `find_gerrit` (`src/app/gerrit.rs`): open `Popup::log`, spawn a
thread that sends `JobEvent::Log(line)` for progress and one final `JobEvent`
(add a variant in `event.rs`), handle it in `handle_job` (`mod.rs`). For
cancellation keep an `Arc<AtomicBool>` in `self.job_cancel`.

## Add a CLI subcommand

`src/main.rs`: variant in `Cmd`/`TaskCmd` + arm in `run_command`;
logic as a function returning `Result<String>` in `src/cli.rs` with a test.

## Change a prompt or its fixed format

Editable defaults: `DEFAULT_*` in `src/ai/mod.rs` (users' copies live in their
prompts folder and win). Fixed formats: `CONTEXT_FORMAT` / `CHECKPOINT_FORMAT`;
if you change them, keep `parse_context_answer` / `checkpoints::parse_list` in sync.

## Add a skill

Write `.agents/skills/<name>/SKILL.md` (front matter `name`, `description`) and
add it to `SKILLS` in `src/cli.rs`.

## Tests

- Unit tests live next to the code (`#[cfg(test)] mod tests`).
- App behaviour: `Harness::new(true)` gives tasks `alpha` (with `scripts/run.sh`)
  and `beta` (in Doing), bash shells, and a temp dir; `h.press`, `h.type_str`,
  `h.pump_until(|app| …)` for background jobs, `h.context_md("alpha")`.
- Fake agents: `fake_agent(cfg, "<sh script>")` — the prompt arrives on stdin.
- Timer: `h.app.timer.as_mut().unwrap().backdate(d)` pretends time passed;
  `h.choose("pause")` picks a menu entry by label; `h.watch()` runs the
  outside-change check now; `h.restart()` quits and starts a new app.
