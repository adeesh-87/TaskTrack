//! Drawing. Everything here reads [`App`] state and paints it with ratatui.

mod config_page;
mod plan_view;
mod popup;
mod task_list;
mod task_view;
pub mod theme;
mod today;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{App, Focus, HomeFocus, Mode, Popup};

pub use theme::Theme;

/// Width of the left column as a percentage of the screen.
pub const SIDEBAR_PERCENT: u16 = 20;

/// Draw one frame.
pub fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let theme = Theme::for_scheme(app.config().color_scheme);
    app.ui.begin_frame();
    let area = frame.area();
    frame.render_widget(
        ratatui::widgets::Block::new().style(Style::new().bg(theme.bg).fg(theme.fg)),
        area,
    );
    let [main, bar] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

    match app.mode() {
        Mode::Settings(_) => config_page::draw(frame, app, main, &theme),
        Mode::Home => {
            let [left, right] = split_columns(main);
            let board_focused = app.home_focus() == HomeFocus::Board;
            task_list::draw(frame, app, left, &theme, board_focused);
            today::draw_today(frame, app, right, &theme);
        }
        Mode::Plan(_) => plan_view::draw(frame, app, main, &theme),
        Mode::Task => {
            let zoomed = app
                .active_context()
                .is_some_and(|c| c.zoomed && c.shell_pane_visible());
            if zoomed {
                task_view::draw_terminal(frame, app, main, &theme, true);
            } else {
                let [left, right] = split_columns(main);
                task_view::draw_sidebar(frame, app, left, &theme);
                task_view::draw_workspace(frame, app, right, &theme);
            }
        }
    }

    draw_status_bar(frame, app, bar, &theme);

    if let Some(p) = app.popup() {
        popup::draw(frame, p, main, &theme);
    }

    if app.flash_on() {
        let buf = frame.buffer_mut();
        let area = buf.area;
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    let style = cell.style().add_modifier(Modifier::REVERSED);
                    cell.set_style(style);
                }
            }
        }
    }
}

fn split_columns(area: Rect) -> [Rect; 2] {
    Layout::horizontal([Constraint::Percentage(SIDEBAR_PERCENT), Constraint::Min(10)]).areas(area)
}

/// The timer chip: text and style (always shown; click it or Esc m).
fn timer_chip(app: &App, theme: &Theme) -> (String, Style) {
    let Some(t) = app.timer() else {
        return (
            " ⏱ timer · Esc m ".to_owned(),
            Style::new().fg(theme.muted).bg(theme.bar_bg),
        );
    };
    let now = std::time::Instant::now();
    let what: String = t.what().chars().take(32).collect();
    let over = t.remaining_secs(now) < 0;
    let text = if t.alarming() {
        format!(" ⏰ TIME'S UP · {} · {what} · {} ", t.task_id, t.label(now))
    } else if t.is_running() {
        format!(" ⏱ {} · {what} · {} ", t.task_id, t.label(now))
    } else {
        format!(" ⏸ {} · {what} · {} ", t.task_id, t.label(now))
    };
    let style = if t.alarming() {
        if app.timer_blink_on() {
            Style::new()
                .bg(theme.error)
                .fg(theme.selected_fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new()
                .fg(theme.error)
                .bg(theme.bar_bg)
                .add_modifier(Modifier::BOLD)
        }
    } else if over {
        Style::new()
            .fg(theme.error)
            .bg(theme.bar_bg)
            .add_modifier(Modifier::BOLD)
    } else if t.is_running() {
        Style::new()
            .bg(theme.selected_bg)
            .fg(theme.selected_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(theme.warning).bg(theme.bar_bg)
    };
    (text, style)
}

fn draw_status_bar(frame: &mut Frame<'_>, app: &mut App, area: Rect, theme: &Theme) {
    let leader = app.leader().to_string();
    let hint = match app.popup() {
        Some(Popup::Palette(p)) if p.typing => "type to filter · Enter run · Esc close".to_owned(),
        Some(Popup::Palette(_)) => "shortcut letter runs · : to type · ↑/↓ Enter · Esc close".to_owned(),
        Some(Popup::Log { done: false, .. }) => "working … Esc cancels agents and hooks".to_owned(),
        Some(Popup::Help { .. }) => "←/→ tabs · ↑/↓ scroll · any other key closes".to_owned(),
        Some(_) => "Enter confirm · Esc cancel".to_owned(),
        None => match app.mode() {
            Mode::Settings(form) if form.editing() => "Enter apply · Esc cancel edit".to_owned(),
            Mode::Settings(_) => {
                "↑/↓ select · Enter edit · a add · d delete · ←/→ cycle · ? help · t test source · Ctrl+S save · Esc back".to_owned()
            }
            Mode::Home if app.home_focus() == HomeFocus::Today => {
                "Enter timer · Space tick · J/K order · x remove · o open · p plan · Tab board · Esc commands".to_owned()
            }
            Mode::Home => match app.list_filter() {
                (f, true) => format!("filter: {f}▏ · Enter keep · Esc clear"),
                _ => "Esc commands · F1 help · Enter open · p plan day · Tab today · / filter · J/K reorder · m timer · d delete".to_owned(),
            },
            Mode::Plan(_) => {
                "Space pick · s suggest · a add · t add to task · Tab switch · J/K order · x remove · A all columns · Enter save · Esc cancel".to_owned()
            }
            Mode::Task => match app.active_context().map(|c| c.focus) {
                Some(Focus::Terminal) if app.leader_pending() => {
                    format!("{leader} + q leave · z zoom · n new · a agent · m timer · ? help")
                }
                Some(Focus::Terminal) => {
                    format!("keys go to the shell · {leader} q leave · {leader} ? help · Ctrl+Tab next pane")
                }
                Some(Focus::Editor) => "Esc commands · Ctrl+S save · Ctrl+C/X/V copy/cut/paste · Ctrl+Z undo · Ctrl+F find · F1 help".to_owned(),
                Some(Focus::Shells) => "Esc commands · Enter focus · n new · x close · Ctrl+1..9 select".to_owned(),
                _ => "Esc commands · F1 help · Enter open · a/A new · r rename · d delete · t shell".to_owned(),
            },
        },
    };
    // A message matters more than the key hints: it goes first, so a narrow
    // bar cuts the hints instead.
    let mut spans = Vec::new();
    if let Some(msg) = app.status() {
        spans.push(Span::styled(
            format!(" {msg} "),
            Style::new().fg(theme.accent),
        ));
    }
    if app.hooks_running() > 0 {
        spans.push(Span::styled(
            " ⚙ hook running ",
            Style::new().fg(theme.warning),
        ));
    }
    spans.push(Span::styled(
        format!(" {hint} "),
        Style::new().fg(theme.muted),
    ));
    let (right, right_style) = timer_chip(app, theme);
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let right_width = right.chars().count().min(area.width as usize);
    if used + right_width > area.width as usize {
        // The timer chip matters more than the hint: trim the left part.
        let keep = (area.width as usize).saturating_sub(right_width);
        let mut left: String = spans.iter().map(|s| s.content.as_ref()).collect();
        left = left.chars().take(keep).collect();
        spans = vec![Span::styled(left, Style::new().fg(theme.muted))];
    }
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let pad = (area.width as usize).saturating_sub(used + right_width);
    spans.push(Span::raw(" ".repeat(pad)));
    app.ui.timer_chip = Rect {
        x: area.x + area.width.saturating_sub(right_width as u16),
        y: area.y,
        width: right_width as u16,
        height: 1,
    };
    spans.push(Span::styled(right, right_style));
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::new().bg(theme.bar_bg)),
        area,
    );
}
