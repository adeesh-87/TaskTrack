//! Help text shown by the `?` popups.

pub const LIST_HELP: &str = "\
Task list
  ↑/↓ or j/k     move           Enter/l   open task
  [ / ]          move task to previous / next category
  n              new task       r / F5    rescan tasks folder
  , or Ctrl+S    settings       q         quit";

pub const TASK_HELP: &str = "\
Task view
  Tab / Shift+Tab   cycle focus (files → shells → editor → terminal)
  Esc               back (editor → files, files/shells → task list)

Files pane
  Enter    open file / toggle folder   ←/→  collapse / expand
  a / A    new file / new folder       r    rename
  d        delete (asks first)         .    toggle hidden files
  t        new shell                   R    refresh

Shells pane
  Enter    focus shell   n  new shell   x  close shell   ←  hide pane

Editor
  Ctrl+S save   Ctrl+W close   Esc back to files

Terminal (keys go to the shell; leader = prefix key)
  leader q/d   leave the shell         leader z    zoom / unzoom
  leader n     new shell               leader x    close shell
  leader h/l   previous / next shell   leader k    editor / files
  leader [ ]   scroll back / forward   leader leader  send leader
  Shift+PgUp/PgDn scroll";
