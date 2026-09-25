//! Mouse handling: click to focus/select, double-click to open, wheel to scroll,
//! drag / double / triple click to select in the editor, and forwarding to
//! programs that enabled mouse reporting.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use crate::editor::char_col_at;
use crate::terminal::mouse::encode_mouse;

use super::ui_state::UiState;
use super::{App, Focus, ListRow, Mode};

const DOUBLE_CLICK: Duration = Duration::from_millis(400);

impl App {
    /// Handle a mouse event.
    pub(super) fn handle_mouse(&mut self, m: MouseEvent) {
        let on_chip = self.ui.timer_chip.contains(Position::new(m.column, m.row));
        if on_chip && matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
            if !matches!(self.popup, Some(super::Popup::Log { done: false, .. })) {
                self.open_timer_menu();
            }
            return;
        }
        if self.popup.is_some() {
            match m.kind {
                MouseEventKind::ScrollUp => {
                    self.handle_popup_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
                }
                MouseEventKind::ScrollDown => {
                    self.handle_popup_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
                }
                _ => {}
            }
            return;
        }
        match self.mode {
            Mode::Settings(_) => self.mouse_config(m),
            Mode::Home if self.ui.today.contains(Position::new(m.column, m.row)) => {
                self.mouse_today(m);
            }
            Mode::Home => self.mouse_list(m),
            Mode::Plan(_) => self.mouse_plan(m),
            Mode::Audit(_) => self.mouse_audit(m),
            Mode::Task => self.mouse_task(m),
        }
    }

    /// Whether this press is the second click of a double-click (same cell, quickly).
    fn is_double_click(&mut self, m: MouseEvent) -> bool {
        let now = Instant::now();
        let double = self.last_click.is_some_and(|(t, c, r)| {
            c == m.column && r == m.row && now.duration_since(t) < DOUBLE_CLICK
        });
        self.last_click = if double {
            None
        } else {
            Some((now, m.column, m.row))
        };
        double
    }

    /// Count clicks in a row on the same cell: 1, 2 (word), 3 (line), then 1 again.
    fn count_click(&mut self, m: MouseEvent) -> u8 {
        let now = Instant::now();
        let same = self.last_click.is_some_and(|(t, c, r)| {
            c == m.column && r == m.row && now.duration_since(t) < DOUBLE_CLICK
        });
        self.click_count = if same { self.click_count % 3 + 1 } else { 1 };
        self.last_click = Some((now, m.column, m.row));
        self.click_count
    }

    fn mouse_config(&mut self, m: MouseEvent) {
        let Mode::Settings(form) = &mut self.mode else {
            return;
        };
        match m.kind {
            MouseEventKind::ScrollUp => {
                form.handle_nav_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
            }
            MouseEventKind::ScrollDown => {
                form.handle_nav_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let rect = self.ui.config_rows;
                if rect.contains(Position::new(m.column, m.row)) {
                    let idx = self.ui.config_first_row + usize::from(m.row - rect.y);
                    if form.editing() {
                        form.handle_edit_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                    }
                    form.select(idx);
                    if self.is_double_click(m) {
                        if let Mode::Settings(form) = &mut self.mode {
                            form.handle_nav_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Today pane: click selects a plan item, double-click starts its timer.
    fn mouse_today(&mut self, m: MouseEvent) {
        let n = self.plan.len();
        match m.kind {
            MouseEventKind::ScrollUp if n > 0 => {
                self.today_selected = self.today_selected.saturating_sub(1);
            }
            MouseEventKind::ScrollDown if n > 0 => {
                self.today_selected = (self.today_selected + 1).min(n - 1);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let hit = self
                    .ui
                    .today_items
                    .iter()
                    .find(|(y, _)| *y == m.row)
                    .map(|(_, i)| *i);
                if n > 0 {
                    self.home_focus = super::HomeFocus::Today;
                }
                if let Some(i) = hit {
                    self.today_selected = i;
                    if self.is_double_click(m) {
                        self.start_plan_item(i);
                    }
                }
            }
            _ => {}
        }
    }

    /// Plan view: click selects (and focuses that side), double-click picks
    /// or removes; the wheel moves the selection.
    fn mouse_plan(&mut self, m: MouseEvent) {
        let pos = Position::new(m.column, m.row);
        let in_pick = self.ui.plan_pick.contains(pos);
        let in_order = self.ui.plan_order.contains(pos);
        let row = if in_pick {
            UiState::list_row(self.ui.plan_pick, &self.ui.plan_pick_state, m.column, m.row)
        } else if in_order {
            UiState::list_row(
                self.ui.plan_order,
                &self.ui.plan_order_state,
                m.column,
                m.row,
            )
        } else {
            return;
        };
        let double =
            matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) && self.is_double_click(m);
        let Mode::Plan(view) = &mut self.mode else {
            return;
        };
        view.focus = if in_pick {
            super::PlanFocus::Pick
        } else {
            super::PlanFocus::Order
        };
        match m.kind {
            MouseEventKind::ScrollUp => view.move_cursor(-1),
            MouseEventKind::ScrollDown => view.move_cursor(1),
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(i) = row else { return };
                if in_pick {
                    if !matches!(
                        view.rows.get(i),
                        Some(super::PlanRow::Task(..) | super::PlanRow::Item(_))
                    ) {
                        return;
                    }
                    view.cursor = i;
                    if double {
                        view.toggle();
                    }
                } else if i < view.picked.len() {
                    view.order_cursor = i;
                    if double {
                        view.remove_picked();
                    }
                }
            }
            _ => {}
        }
    }

    /// Audit view: click selects, double-click decides / ticks, wheel moves.
    fn mouse_audit(&mut self, m: MouseEvent) {
        let row = UiState::list_row(self.ui.audit_list, &self.ui.audit_state, m.column, m.row);
        let item = row.and_then(|r| self.ui.audit_rows.get(r).copied().flatten());
        let double =
            matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) && self.is_double_click(m);
        let Mode::Audit(view) = &mut self.mode else {
            return;
        };
        let last = view.items.len().saturating_sub(1);
        match m.kind {
            MouseEventKind::ScrollUp => view.selected = view.selected.saturating_sub(1),
            MouseEventKind::ScrollDown => view.selected = (view.selected + 1).min(last),
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(i) = item {
                    view.selected = i;
                    if double {
                        self.handle_audit_key(KeyEvent::new(
                            KeyCode::Char(' '),
                            KeyModifiers::NONE,
                        ));
                    }
                }
            }
            _ => {}
        }
    }

    fn mouse_list(&mut self, m: MouseEvent) {
        if matches!(m.kind, MouseEventKind::Down(_)) {
            self.home_focus = super::HomeFocus::Board;
        }
        let rows = self.rows();
        match m.kind {
            MouseEventKind::ScrollUp => self.move_list(-1, &rows),
            MouseEventKind::ScrollDown => self.move_list(1, &rows),
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(idx) =
                    UiState::list_row(self.ui.task_list, &self.ui.task_list_state, m.column, m.row)
                else {
                    return;
                };
                if let Some(ListRow::Task(_, id)) = rows.get(idx) {
                    let id = id.clone();
                    self.list_selected = idx;
                    if self.is_double_click(m) {
                        self.enter_task(&id);
                    }
                }
            }
            _ => {}
        }
    }

    fn mouse_task(&mut self, m: MouseEvent) {
        let pos = Position::new(m.column, m.row);
        // A selection drag keeps going when the pointer leaves the editor.
        if self.editor_drag
            && matches!(
                m.kind,
                MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
            )
        {
            self.mouse_editor(m);
            return;
        }
        let ui_tree = self.ui.tree;
        let ui_shells = self.ui.shells;
        let ui_editor = self.ui.editor;
        let ui_terminal = self.ui.terminal;
        if ui_terminal.contains(pos) {
            self.mouse_terminal(m);
        } else if ui_tree.contains(pos) {
            self.mouse_tree(m);
        } else if ui_shells.contains(pos) {
            self.mouse_shells(m);
        } else if ui_editor.contains(pos) {
            self.mouse_editor(m);
        }
    }

    fn mouse_tree(&mut self, m: MouseEvent) {
        let row = UiState::list_row(self.ui.tree, &self.ui.tree_state, m.column, m.row);
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        match m.kind {
            MouseEventKind::ScrollUp => ctx.tree.select_prev(),
            MouseEventKind::ScrollDown => ctx.tree.select_next(),
            MouseEventKind::Down(MouseButton::Left) => {
                ctx.set_focus(Focus::Tree);
                let Some(idx) = row.filter(|i| *i < ctx.tree.nodes().len()) else {
                    return;
                };
                ctx.tree.select_index(idx);
                if self.is_double_click(m) {
                    self.handle_tree_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                }
            }
            _ => {}
        }
    }

    fn mouse_shells(&mut self, m: MouseEvent) {
        let row = UiState::list_row(self.ui.shells, &self.ui.shells_state, m.column, m.row);
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        match m.kind {
            MouseEventKind::ScrollUp => ctx.select_shell(-1),
            MouseEventKind::ScrollDown => ctx.select_shell(1),
            MouseEventKind::Down(MouseButton::Left) => {
                ctx.set_focus(Focus::Shells);
                let Some(idx) = row.filter(|i| *i < ctx.shells.len()) else {
                    return;
                };
                ctx.selected_shell = idx;
                if self.is_double_click(m) {
                    if let Some(ctx) = self.active_context_mut() {
                        ctx.focus_shell(idx);
                    }
                }
            }
            _ => {}
        }
    }

    fn mouse_editor(&mut self, m: MouseEvent) {
        let rect = self.ui.editor;
        let gutter = self.ui.editor_gutter;
        let hscroll = self.ui.editor_hscroll;
        let rows = self.ui.editor_rows.clone();
        let tab_width = usize::from(self.config.tab_width.max(1));
        let height = usize::from(rect.height.max(1));
        let clicks = if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
            self.count_click(m)
        } else {
            0
        };
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => self.editor_drag = clicks == 1,
            MouseEventKind::Up(MouseButton::Left) => {
                self.editor_drag = false;
                return;
            }
            _ => {}
        }
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        let Some(ed) = &mut ctx.editor else { return };
        // Buffer position under the pointer; above / below the pane is the
        // line just outside it, so dragging there scrolls.
        let position = |ed: &crate::editor::Buffer| {
            let (row, start) = if m.row < rect.y {
                (ed.scroll().saturating_sub(1), 0)
            } else if m.row >= rect.y + rect.height {
                let last = rows.last().map_or(ed.scroll() + height - 1, |r| r.0);
                (last + 1, 0)
            } else {
                let visual = usize::from(m.row - rect.y);
                rows.get(visual)
                    .copied()
                    .unwrap_or((ed.scroll() + visual, hscroll))
            };
            let disp = usize::from(m.column.saturating_sub(rect.x + gutter)) + start;
            let line = ed.lines().get(row).map_or("", String::as_str);
            (row, char_col_at(line, disp, tab_width))
        };
        match m.kind {
            MouseEventKind::ScrollUp => ed.scroll_by(-3, height),
            MouseEventKind::ScrollDown => ed.scroll_by(3, height),
            MouseEventKind::Down(MouseButton::Left) => {
                let (row, col) = position(ed);
                let extend = m.modifiers.contains(KeyModifiers::SHIFT);
                ed.select(extend);
                ed.set_cursor(row, col);
                match clicks {
                    2 => ed.select_word(),
                    3 => ed.select_line(),
                    _ if !extend => ed.set_anchor(),
                    _ => {}
                }
                ctx.set_focus(Focus::Editor);
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let (row, col) = position(ed);
                ed.set_cursor(row, col);
                ed.ensure_visible(height);
            }
            _ => {}
        }
    }

    fn mouse_terminal(&mut self, m: MouseEvent) {
        let rect = self.ui.terminal;
        let Some(ctx) = self.active_context_mut() else {
            return;
        };
        if matches!(m.kind, MouseEventKind::Down(_)) && ctx.shell_pane_visible() {
            ctx.focus = Focus::Terminal;
        }
        let Some(shell) = ctx.active_shell_mut() else {
            return;
        };
        let screen = shell.session.screen();
        let (mode, encoding) = (
            screen.mouse_protocol_mode(),
            screen.mouse_protocol_encoding(),
        );
        let x = m.column.saturating_sub(rect.x);
        let y = m.row.saturating_sub(rect.y);
        if let Some(bytes) = encode_mouse(&m, x, y, mode, encoding) {
            let _ = shell.session.write(&bytes);
            return;
        }
        match m.kind {
            MouseEventKind::ScrollUp => shell.session.scroll_by(3),
            MouseEventKind::ScrollDown => shell.session.scroll_by(-3),
            _ => {}
        }
    }
}
