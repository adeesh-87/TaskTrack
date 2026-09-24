//! Drawing. Everything here reads [`App`] state and paints it with ratatui.

mod config_page;
mod popup;
mod task_list;
mod task_view;
pub mod theme;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{App, Focus, Mode, Popup};

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
        Mode::Config(_) => config_page::draw(frame, app, main, &theme),
        Mode::TaskList => {
            let [left, right] = split_columns(main);
            task_list::draw(frame, app, left, &theme, true);
            task_view::draw_next_up(frame, app, right, &theme);
        }
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

fn draw_status_bar(frame: &mut Frame<'_>, app: &App, area: Rect, theme: &Theme) {
    let leader = app.leader().to_string();
    let hint = match app.popup() {
        Some(Popup::Palette(p)) if p.typing => "type to filter · Enter run · Esc close".to_owned(),
        Some(Popup::Palette(_)) => "shortcut letter runs · : to type · ↑/↓ Enter · Esc close".to_owned(),
        Some(Popup::Log { done: false, .. }) => "working … please wait".to_owned(),
        Some(_) => "Enter confirm · Esc cancel".to_owned(),
        None => match app.mode() {
            Mode::Config(form) if form.editing() => "Enter apply · Esc cancel edit".to_owned(),
            Mode::Config(_) => {
                "↑/↓ select · Enter edit · a add · d delete · ←/→ cycle · ? help · t test source · Ctrl+S save · Esc back".to_owned()
            }
            Mode::TaskList => "Esc commands · Enter open · n new · d delete · m timer · v checkpoint done · [ ] move · q quit".to_owned(),
            Mode::Task => match app.active_context().map(|c| c.focus) {
                Some(Focus::Terminal) if app.leader_pending() => {
                    format!("{leader} + q leave · z zoom · n new · x close · Esc commands · ? keys")
                }
                Some(Focus::Terminal) => {
                    format!("keys go to the shell · {leader} q leave · Ctrl+Tab next pane · Ctrl+1..9 shells")
                }
                Some(Focus::Editor) => "Esc commands · Ctrl+S save · Ctrl+W close · Ctrl+Tab next pane".to_owned(),
                Some(Focus::Shells) => "Esc commands · Enter focus · n new · x close · Ctrl+1..9 select".to_owned(),
                _ => "Esc commands · Enter open · a/A new · r rename · d delete · t shell · Ctrl+Tab next pane".to_owned(),
            },
        },
    };
    let mut spans = vec![Span::styled(
        format!(" {hint} "),
        Style::new().fg(theme.muted),
    )];
    if let Some(msg) = app.status() {
        spans.push(Span::styled(
            format!(" {msg} "),
            Style::new().fg(theme.accent),
        ));
    }
    let (right, right_style) = if app.leader_pending() {
        (format!(" {leader} … "), Style::new().fg(theme.muted))
    } else if let Some(t) = app.timer() {
        let now = std::time::Instant::now();
        let over = t.remaining_secs(now) < 0;
        let style = if over {
            Style::new().fg(theme.error).add_modifier(Modifier::BOLD)
        } else if t.is_running() {
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme.warning)
        };
        let what: String = t.what().chars().take(40).collect();
        (
            format!(" ⏱ {} · {what} · {} ", t.task_id, t.label(now)),
            style,
        )
    } else {
        (
            format!(
                " {} · font: {} ",
                env!("CARGO_PKG_NAME"),
                app.config().font_family
            ),
            Style::new().fg(theme.muted),
        )
    };
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let right_width = right.chars().count();
    if used + right_width > area.width as usize {
        // The timer matters more than the hint: trim the hint.
        let keep = (area.width as usize).saturating_sub(right_width);
        let mut left: String = spans.iter().map(|s| s.content.as_ref()).collect();
        left = left.chars().take(keep).collect();
        spans = vec![Span::styled(left, Style::new().fg(theme.muted))];
    }
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let pad = (area.width as usize).saturating_sub(used + right_width);
    spans.push(Span::raw(" ".repeat(pad)));
    spans.push(Span::styled(right, right_style));
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::new().bg(theme.bar_bg)),
        area,
    );
}
