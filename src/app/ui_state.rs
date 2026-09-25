//! Screen geometry recorded while drawing, used for mouse hit-testing.

use ratatui::layout::{Position, Rect};
use ratatui::widgets::ListState;

/// Where things were drawn in the last frame, plus the list scroll states that
/// ratatui keeps for us. Reset at the start of every frame so stale rectangles
/// from another mode never match.
#[derive(Debug, Default)]
pub struct UiState {
    /// Task list block (with border).
    pub task_list: Rect,
    /// Scroll state of the task list.
    pub task_list_state: ListState,
    /// File tree block (with border).
    pub tree: Rect,
    /// Scroll state of the file tree.
    pub tree_state: ListState,
    /// Shell list block (with border).
    pub shells: Rect,
    /// Scroll state of the shell list.
    pub shells_state: ListState,
    /// Editor text area (inside the border).
    pub editor: Rect,
    /// Width of the editor's line-number gutter.
    pub editor_gutter: u16,
    /// Horizontal scroll of the editor in display columns.
    pub editor_hscroll: usize,
    /// Terminal area (inside the border).
    pub terminal: Rect,
    /// Rows area of the settings page.
    pub config_rows: Rect,
    /// Index of the first visible settings row.
    pub config_first_row: usize,
    /// The timer chip in the status bar.
    pub timer_chip: Rect,
    /// With soft wrap: (buffer line, first display column) of each editor row.
    pub editor_rows: Vec<(usize, usize)>,
    /// Today pane on Home (inside the border).
    pub today: Rect,
    /// Screen row of each plan item in the Today pane: (y, item index).
    pub today_items: Vec<(u16, usize)>,
    /// Plan view: left list block (with border).
    pub plan_pick: Rect,
    /// Scroll state of the Plan view's left list.
    pub plan_pick_state: ListState,
    /// Plan view: right list block (with border).
    pub plan_order: Rect,
    /// Scroll state of the Plan view's right list.
    pub plan_order_state: ListState,
    /// Audit view: the proposal list block (with border).
    pub audit_list: Rect,
    /// Scroll state of the audit list.
    pub audit_state: ListState,
    /// Audit list rows: the item each row shows (`None` for group headings).
    pub audit_rows: Vec<Option<usize>>,
}

impl UiState {
    /// Forget last frame's geometry (list scroll states are kept).
    pub fn begin_frame(&mut self) {
        self.task_list = Rect::default();
        self.tree = Rect::default();
        self.shells = Rect::default();
        self.editor = Rect::default();
        self.terminal = Rect::default();
        self.config_rows = Rect::default();
        self.timer_chip = Rect::default();
        self.editor_rows.clear();
        self.today = Rect::default();
        self.today_items.clear();
        self.plan_pick = Rect::default();
        self.plan_order = Rect::default();
        self.audit_list = Rect::default();
    }

    /// Row index of a click inside a bordered list, if any.
    pub fn list_row(rect: Rect, state: &ListState, col: u16, row: u16) -> Option<usize> {
        let inner = Rect {
            x: rect.x + 1,
            y: rect.y + 1,
            width: rect.width.saturating_sub(2),
            height: rect.height.saturating_sub(2),
        };
        if !inner.contains(Position::new(col, row)) {
            return None;
        }
        Some(state.offset() + usize::from(row - inner.y))
    }
}
