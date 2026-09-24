//! Modal dialog rendering.

use ratatui::layout::{Constraint, Flex, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::Popup;

use super::Theme;

/// Centre a box of the given size inside `area`.
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [v] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [h] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(v);
    h
}

/// Draw the popup over `area`.
pub fn draw(frame: &mut Frame<'_>, popup: &Popup, area: Rect, theme: &Theme) {
    let width = (area.width * 3 / 5).clamp(30.min(area.width), 90.min(area.width));
    let is_warning =
        popup.title().eq_ignore_ascii_case("warning") || popup.title().starts_with("Delete");
    let title_style = if is_warning {
        Style::new().fg(theme.warning).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(theme.accent).add_modifier(Modifier::BOLD)
    };

    let (lines, cursor): (Vec<Line<'_>>, Option<(u16, u16)>) = match popup {
        Popup::Message { body, .. } => {
            let mut lines: Vec<Line<'_>> = body.lines().map(|l| Line::raw(l.to_owned())).collect();
            lines.push(Line::raw(""));
            lines.push(Line::styled("press any key", Style::new().fg(theme.muted)));
            (lines, None)
        }
        Popup::Confirm { body, .. } => {
            let mut lines: Vec<Line<'_>> = body.lines().map(|l| Line::raw(l.to_owned())).collect();
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled("[y] yes / Enter", Style::new().fg(theme.accent)),
                Span::raw("    "),
                Span::styled("[n] no / Esc", Style::new().fg(theme.muted)),
            ]));
            (lines, None)
        }
        Popup::Input { label, value, .. } => {
            let lines = vec![
                Line::styled(label.clone(), Style::new().fg(theme.muted)),
                Line::from(vec![Span::raw("> "), Span::raw(value.clone())]),
                Line::raw(""),
                Line::styled(
                    "Enter confirm · Esc cancel · Ctrl+U clear",
                    Style::new().fg(theme.muted),
                ),
            ];
            (lines, Some((2 + value.chars().count() as u16, 1)))
        }
    };

    let inner_width = width.saturating_sub(4).max(1) as usize;
    let wrapped: usize = lines
        .iter()
        .map(|l| {
            let w = l.width();
            if w == 0 {
                1
            } else {
                w.div_ceil(inner_width)
            }
        })
        .sum();
    let height = (wrapped as u16 + 2).min(area.height);
    let rect = centered(area, width, height);
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .title(Span::styled(format!(" {} ", popup.title()), title_style))
        .border_style(Style::new().fg(if is_warning {
            theme.warning
        } else {
            theme.border_focus
        }))
        .style(Style::new().bg(theme.bg).fg(theme.fg));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let text_area = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), text_area);
    if let Some((cx, cy)) = cursor {
        let x = text_area.x + cx.min(text_area.width.saturating_sub(1));
        frame.set_cursor_position(Position::new(x, text_area.y + cy));
    }
}
