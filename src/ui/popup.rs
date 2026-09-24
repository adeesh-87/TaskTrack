//! Modal dialog rendering.

use ratatui::layout::{Constraint, Flex, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::popup::filter_tickets;
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

fn hint(text: &str, theme: &Theme) -> Line<'static> {
    Line::styled(text.to_owned(), Style::new().fg(theme.muted))
}

/// Rows for a scrollable selection list, keeping `selected` visible.
fn list_lines<'a>(
    rows: impl Iterator<Item = Line<'a>>,
    selected: usize,
    max_rows: usize,
) -> (Vec<Line<'a>>, usize) {
    let all: Vec<Line<'a>> = rows.collect();
    let first = selected
        .saturating_sub(max_rows.saturating_sub(1))
        .min(all.len().saturating_sub(max_rows));
    let shown: Vec<Line<'a>> = all.into_iter().skip(first).take(max_rows).collect();
    (shown, first)
}

/// The help page: a large box with a tab bar.
fn draw_help(
    frame: &mut Frame<'_>,
    tabs: &[(String, Vec<String>)],
    tab: usize,
    scroll: usize,
    area: Rect,
    theme: &Theme,
) {
    let rect = centered(
        area,
        area.width.saturating_sub(4).max(20).min(area.width),
        area.height.saturating_sub(2).max(6).min(area.height),
    );
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .title(Span::styled(
            " pahiri help ",
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::new().fg(theme.border_focus))
        .style(Style::new().bg(theme.bg).fg(theme.fg));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let [bar, body, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(inner);
    let mut spans = Vec::new();
    for (i, (title, _)) in tabs.iter().enumerate() {
        let style = if i == tab {
            theme.selected(true)
        } else {
            Style::new().fg(theme.muted)
        };
        spans.push(Span::styled(format!(" {} {title} ", i + 1), style));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), bar);
    let lines: Vec<Line<'_>> = tabs
        .get(tab)
        .map(|(_, l)| {
            l.iter()
                .skip(scroll)
                .take(body.height as usize)
                .map(|l| {
                    let is_heading = !l.starts_with(' ')
                        && l.len() > 2
                        && l.chars()
                            .take_while(|c| *c != ' ')
                            .all(|c| c.is_uppercase() || !c.is_alphabetic());
                    if is_heading {
                        Line::styled(
                            l.clone(),
                            Style::new().fg(theme.header).add_modifier(Modifier::BOLD),
                        )
                    } else {
                        Line::raw(l.clone())
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let text = Rect {
        x: body.x + 1,
        width: body.width.saturating_sub(2),
        ..body
    };
    frame.render_widget(Paragraph::new(lines), text);
    let total = tabs.get(tab).map_or(0, |t| t.1.len());
    frame.render_widget(
        Paragraph::new(hint(
            &format!(
                " ←/→ or 1-9 tabs · ↑/↓ PgUp/PgDn scroll ({}/{total}) · any other key closes",
                (scroll + 1).min(total.max(1))
            ),
            theme,
        )),
        foot,
    );
}

/// Draw the popup over `area`.
pub fn draw(frame: &mut Frame<'_>, popup: &Popup, area: Rect, theme: &Theme) {
    if let Popup::Help { tabs, tab, scroll } = popup {
        draw_help(frame, tabs, *tab, *scroll, area, theme);
        return;
    }
    let wide = matches!(
        popup,
        Popup::Log { .. }
            | Popup::Tickets { .. }
            | Popup::MultiSelect { .. }
            | Popup::Palette(_)
            | Popup::Choose { .. }
            | Popup::Doc { .. }
            | Popup::Checkpoints { .. }
    );
    let width = if wide {
        (area.width * 4 / 5).clamp(40.min(area.width), 120.min(area.width))
    } else {
        (area.width * 3 / 5).clamp(30.min(area.width), 90.min(area.width))
    };
    let max_list = (area.height as usize).saturating_sub(8).max(3);
    let is_warning = popup.title().to_uppercase().starts_with("WARNING")
        || popup.title().starts_with("Delete")
        || popup.title().starts_with("Time's up");
    let title_style = if is_warning {
        Style::new().fg(theme.warning).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(theme.accent).add_modifier(Modifier::BOLD)
    };
    let sel = theme.selected(true);

    let mut cursor: Option<(u16, u16)> = None;
    let mut title = format!(" {} ", popup.title());
    let lines: Vec<Line<'_>> = match popup {
        Popup::Message { body, .. } => {
            let mut lines: Vec<Line<'_>> = body.lines().map(|l| Line::raw(l.to_owned())).collect();
            lines.push(Line::raw(""));
            lines.push(hint("press any key", theme));
            lines
        }
        Popup::Confirm { body, .. } => {
            let mut lines: Vec<Line<'_>> = body.lines().map(|l| Line::raw(l.to_owned())).collect();
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled("[y] yes / Enter", Style::new().fg(theme.accent)),
                Span::raw("    "),
                Span::styled("[n] no / Esc", Style::new().fg(theme.muted)),
            ]));
            lines
        }
        Popup::Input { label, value, .. } => {
            cursor = Some((2 + value.chars().count() as u16, 1));
            vec![
                Line::styled(label.clone(), Style::new().fg(theme.muted)),
                Line::from(vec![Span::raw("> "), Span::raw(value.clone())]),
                Line::raw(""),
                hint("Enter confirm · Esc cancel · Ctrl+U clear", theme),
            ]
        }
        Popup::Choose {
            choices, selected, ..
        } => {
            let rows = choices.iter().enumerate().map(|(i, c)| {
                let style = if i == *selected { sel } else { Style::new() };
                Line::styled(format!(" {} {}", i + 1, c.label), style)
            });
            let (mut lines, _) = list_lines(rows, *selected, max_list);
            lines.push(Line::raw(""));
            lines.push(hint("↑/↓ or number · Enter choose · Esc cancel", theme));
            lines
        }
        Popup::MultiSelect {
            items, selected, ..
        } => {
            let rows = items.iter().enumerate().map(|(i, it)| {
                let style = if i == *selected { sel } else { Style::new() };
                let mark = if it.checked { "[x]" } else { "[ ]" };
                Line::styled(format!(" {mark} {}", it.label), style)
            });
            let (mut lines, _) = list_lines(rows, *selected, max_list);
            if items.is_empty() {
                lines.push(hint(" nothing to attach", theme));
            }
            lines.push(Line::raw(""));
            lines.push(hint("Space toggle · Enter apply · Esc cancel", theme));
            lines
        }
        Popup::Tickets {
            source,
            tickets,
            filter,
            selected,
        } => {
            let filtered = filter_tickets(tickets, filter);
            title = format!(
                " new task from {source} · {}/{} ",
                filtered.len(),
                tickets.len()
            );
            cursor = Some((2 + filter.chars().count() as u16, 0));
            let rows = filtered.iter().enumerate().map(|(i, t)| {
                let style = if i == *selected { sel } else { Style::new() };
                Line::styled(format!(" {:<14} {}", t.id, t.title), style)
            });
            let (list, _) = list_lines(rows, *selected, max_list.saturating_sub(4).max(2));
            let mut lines = vec![Line::from(vec![Span::raw("> "), Span::raw(filter.clone())])];
            lines.extend(list);
            if let Some(t) = filtered.get(*selected) {
                lines.push(Line::raw(""));
                if !t.url.is_empty() {
                    lines.push(Line::styled(
                        format!(" {}", t.url),
                        Style::new().fg(theme.accent),
                    ));
                }
                let desc: String = t.description.lines().take(3).collect::<Vec<_>>().join(" ");
                if !desc.trim().is_empty() {
                    lines.push(hint(&format!(" {desc}"), theme));
                }
            }
            lines.push(Line::raw(""));
            lines.push(hint(
                "type to filter · ↑/↓ · Enter create task · Esc cancel",
                theme,
            ));
            lines
        }
        Popup::Log {
            lines: log, done, ..
        } => {
            let max = max_list + 2;
            let start = log.len().saturating_sub(max);
            let mut lines: Vec<Line<'_>> =
                log[start..].iter().map(|l| Line::raw(l.clone())).collect();
            lines.push(Line::raw(""));
            lines.push(if *done {
                hint("done · press any key", theme)
            } else {
                hint("working …", theme)
            });
            lines
        }
        Popup::Help { .. } => Vec::new(),
        Popup::Doc { lines, scroll, .. } => {
            let max = max_list + 2;
            let mut out: Vec<Line<'_>> = lines
                .iter()
                .skip(*scroll)
                .take(max)
                .map(|l| Line::raw(l.clone()))
                .collect();
            out.push(Line::raw(""));
            let more = lines.len() > scroll + max;
            out.push(hint(
                if more || *scroll > 0 {
                    "↑/↓ PgUp/PgDn scroll · any other key closes"
                } else {
                    "press any key"
                },
                theme,
            ));
            out
        }
        Popup::Checkpoints {
            task_id,
            items,
            selected,
        } => {
            title = format!(
                " {task_id} · checkpoints · {} ",
                crate::tasks::checkpoints::summary(items)
            );
            let rows = items.iter().enumerate().map(|(i, c)| {
                let style = if i == *selected {
                    sel
                } else if c.done {
                    Style::new().fg(theme.muted)
                } else {
                    Style::new()
                };
                let mark = if c.done { "[x]" } else { "[ ]" };
                let spent = if c.spent_min > 0 {
                    format!(
                        " · spent {}",
                        crate::tasks::checkpoints::fmt_minutes(c.spent_min)
                    )
                } else {
                    String::new()
                };
                Line::styled(
                    format!(
                        " {mark} {} ({}{spent})",
                        c.title,
                        crate::tasks::checkpoints::fmt_minutes(c.estimate_min)
                    ),
                    style,
                )
            });
            let (mut lines, _) = list_lines(rows, *selected, max_list);
            lines.push(Line::raw(""));
            lines.push(hint(
                "Space tick/untick · Enter start the timer on it · Esc close · edit freely in CONTEXT.md",
                theme,
            ));
            lines
        }
        Popup::Palette(p) => {
            " commands ".clone_into(&mut title);
            let prompt = if p.typing { ":" } else { " " };
            cursor = Some((2 + p.filter.chars().count() as u16, 0));
            let matches = p.matches();
            let rows = matches.iter().enumerate().map(|(i, c)| {
                let style = if i == p.selected { sel } else { Style::new() };
                Line::from(vec![
                    Span::styled(
                        format!(" {} ", c.key),
                        Style::new()
                            .fg(theme.header)
                            .add_modifier(Modifier::BOLD)
                            .patch(style),
                    ),
                    Span::styled(format!(" {}", c.label), style),
                ])
            });
            let (list, _) = list_lines(rows, p.selected, max_list);
            let mut lines = vec![Line::from(vec![
                Span::styled(format!("{prompt} "), Style::new().fg(theme.accent)),
                Span::raw(p.filter.clone()),
            ])];
            lines.extend(list);
            lines.push(Line::raw(""));
            lines.push(hint(
                if p.typing {
                    "type to filter · Enter run · Esc close"
                } else {
                    "press a shortcut letter · : to type a command · Enter run · Esc close"
                },
                theme,
            ));
            lines
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
        .title(Span::styled(title, title_style))
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
