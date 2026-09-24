# pahiri — notes for coding agents

Rust TUI (ratatui + crossterm) for task execution. One binary, no services.

## Before you change anything

Read, in this order, only what the change needs:

1. `docs/ARCHITECTURE.md` — module map, event loop, where state lives.
2. `docs/HOWTO.md` — recipes for the usual changes (key, palette command,
   setting, popup, CONTEXT.md field, background job, CLI command).
3. `docs/FORMATS.md` — files on disk and what agents/scripts must print.

## Commands (all must pass before you finish)

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
python3 scripts/smoke.py   # optional: drives the real binary (pip install pexpect pyte)
```

## Rules

- Small, local changes. Follow the recipe in `docs/HOWTO.md` for the kind
  of change; do not restructure modules.
- Every behaviour change gets a test. App behaviour is tested by driving
  key events through `Harness` in `src/app/tests.rs`; copy a nearby test.
- `src/ui/` only draws; it never changes files or starts processes.
- Never write to `CONTEXT.md` outside the helpers in `src/tasks/`
  (`context.rs`, `checkpoints.rs`, `sections.rs`): they keep the user's text intact.
- Clippy is pedantic (`Cargo.toml [lints]`). Common fixes: `let _ = write!(s, …)`
  instead of `s.push_str(&format!(…))`, inline `{var}` in `format!`, end
  statements with `;`, backticks around code in doc comments.
- MSRV is 1.80: no `is_none_or`, no `iter::repeat_n`.
- User-facing change → update `README.md` (and the help text in
  `src/app/keymap.rs` if keys change).
