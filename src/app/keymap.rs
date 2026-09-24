//! Help text shown by the help popup.

pub const HELP: &str = "\
Esc opens the command palette (anywhere except inside a shell; there use
leader Esc). Press a shortcut letter, or : to type and filter, then Enter:
  T task list · c config · q quit · n new task · p prepare · a attach
  s new shell · x close shell · o open CONTEXT.md · z zoom · h this help

Panes (task view)
  Ctrl+Tab / Ctrl+Shift+Tab   next / previous pane (files → editor → shells → terminal)
  Alt+] / Alt+[               same, for terminals without the kitty keyboard protocol
  Ctrl+1..9 / Alt+1..9        select shell N and focus the terminal

Task list
  ↑/↓ or j/k move · Enter open task · [ / ] move between categories · n new task

Files
  Enter open file / toggle folder · ←/→ collapse / expand · a new file · A new folder
  r rename · d delete (asks first) · . hidden files · t new shell · R refresh

Shells pane
  Enter focus · n new · x close · ← hide pane

Editor
  Ctrl+S save · Ctrl+W close · Esc palette

Terminal (all keys go to the shell; leader = prefix key, default ctrl+b)
  leader q      leave the shell        leader z      zoom / unzoom
  leader n      new shell              leader x      close shell
  leader Esc    command palette        leader leader send the leader key
  leader [ / ]  scroll back / forward  Shift+PgUp/PgDn scroll

In the shell: cd task · cd code [name] · cd build [name]";
