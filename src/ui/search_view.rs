//! The search-in-files panel (Ctrl+Shift+F).

use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::search::{Field, SearchPanel};
use crate::app::App;

use super::popup::centered;
use super::Theme;

/// Draw the panel over `area`.
pub fn draw(frame: &mut Frame<'_>, app: &mut App, area: Rect, theme: &Theme) {
    let Some(p) = app.search_panel() else {
        return;
    };
    let rect = centered(
        area,
        area.width.saturating_sub(4).max(20).min(area.width),
        area.height.saturating_sub(2).max(10).min(area.height),
    );
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .title(Span::styled(
            " search in files ",
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::new().fg(theme.border_focus))
        .style(Style::new().bg(theme.bg).fg(theme.fg));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    if inner.height < 8 || inner.width < 20 {
        return;
    }
    let label = |field: Field, text: &'static str| {
        if p.field == field {
            Span::styled(
                text,
                Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(text, Style::new().fg(theme.muted))
        }
    };
    let switch = |on: bool, text: &'static str| {
        if on {
            Span::styled(
                text,
                Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(text, Style::new().fg(theme.muted))
        }
    };
    let mut head: Vec<Line<'_>> = vec![
        Line::from(vec![
            label(Field::Query, " find    "),
            Span::raw(p.query.clone()),
            Span::raw("   "),
            switch(p.case, "Aa "),
            switch(p.word, "ab "),
            switch(p.regex, ".*"),
        ]),
        Line::from(vec![
            label(Field::Include, " include "),
            if p.include.is_empty() && p.field != Field::Include {
                Span::styled(
                    "all files (e.g. *.c, *.h, src/**)",
                    Style::new().fg(theme.muted),
                )
            } else {
                Span::raw(p.include.clone())
            },
        ]),
        Line::from(vec![
            label(Field::Exclude, " exclude "),
            if p.exclude.is_empty() && p.field != Field::Exclude {
                Span::styled(
                    "nothing more than .gitignore (e.g. build, *.o)",
                    Style::new().fg(theme.muted),
                )
            } else {
                Span::raw(p.exclude.clone())
            },
        ]),
    ];
    let mut roots = vec![label(Field::Roots, " in      ")];
    for (i, r) in p.roots.iter().enumerate() {
        let mut style = if r.on {
            Style::new().fg(theme.fg)
        } else {
            Style::new().fg(theme.muted)
        };
        if p.field == Field::Roots && i == p.root_cursor {
            style = theme.selected(true);
        }
        let mark = if r.on { "[x] " } else { "[ ] " };
        roots.push(Span::styled(format!("{mark}{}", r.label), style));
        roots.push(Span::raw("  "));
    }
    head.push(Line::from(roots));
    if let Some(scope) = &p.scope {
        head.push(Line::from(vec![
            Span::styled(" only    ", Style::new().fg(theme.muted)),
            Span::raw(scope.display().to_string()),
            Span::styled("  (Alt+X: everywhere)", Style::new().fg(theme.muted)),
        ]));
    }
    let note_style = if p.note.starts_with("pattern") || p.note.contains("glob") {
        Style::new().fg(theme.error)
    } else {
        Style::new().fg(theme.muted)
    };
    head.push(Line::styled(format!(" {}", p.note), note_style));
    let head_h = head.len() as u16;
    frame.render_widget(
        Paragraph::new(head),
        Rect {
            height: head_h,
            ..inner
        },
    );

    // Text cursor at the end of the focused field.
    let field_text = match p.field {
        Field::Query => Some((0u16, &p.query)),
        Field::Include => Some((1, &p.include)),
        Field::Exclude => Some((2, &p.exclude)),
        Field::Roots | Field::Results => None,
    };
    if let Some((row, text)) = field_text {
        let x = inner.x + 9 + text.chars().count() as u16;
        if x < inner.x + inner.width {
            frame.set_cursor_position(Position::new(x, inner.y + row));
        }
    }

    let list = Rect {
        y: inner.y + head_h,
        height: inner.height.saturating_sub(head_h + 1),
        ..inner
    };
    draw_results(frame, p, list, theme);
    frame.render_widget(
        Paragraph::new(Line::styled(
            " Enter search / open · Tab next field · Space switches a folder · Alt+C/W/R case/word/regex · F4 next result in the editor · Esc",
            Style::new().fg(theme.muted),
        )),
        Rect {
            y: inner.y + inner.height - 1,
            height: 1,
            ..inner
        },
    );
}

fn draw_results(frame: &mut Frame<'_>, p: &mut SearchPanel, area: Rect, theme: &Theme) {
    let height = area.height as usize;
    if height == 0 {
        return;
    }
    let focused = p.field == Field::Results;
    let selected = p.selected;
    if selected < p.scroll {
        p.scroll = selected;
    } else if selected >= p.scroll + height {
        p.scroll = selected + 1 - height;
    }
    let scroll = p.scroll;
    let rows: Vec<(usize, Option<usize>)> =
        p.rows().iter().skip(scroll).take(height).copied().collect();
    let width = area.width as usize;
    let hit_style = Style::new().bg(theme.warning).fg(theme.bg);
    let mut lines = Vec::new();
    for (n, (f, h)) in rows.into_iter().enumerate() {
        let file = &p.files[f];
        let is_sel = scroll + n == selected;
        let base = if is_sel {
            theme.selected(focused)
        } else {
            Style::new()
        };
        let line = match h {
            None => Line::from(vec![
                Span::styled(
                    format!(" {}", file.label),
                    Style::new()
                        .fg(theme.header)
                        .add_modifier(Modifier::BOLD)
                        .patch(base),
                ),
                Span::styled(
                    format!(" ({})", file.hits.len()),
                    Style::new().fg(theme.muted).patch(base),
                ),
            ]),
            Some(h) => {
                let hit = &file.hits[h];
                let number = format!("   {:>6}  ", hit.line + 1);
                let chars: Vec<char> = hit
                    .text
                    .chars()
                    .map(|c| if c == '\t' { ' ' } else { c })
                    .collect();
                let lead = chars
                    .iter()
                    .take_while(|c| **c == ' ')
                    .count()
                    .min(hit.start);
                let room = width.saturating_sub(number.len());
                // Show the match even far along a long line.
                let from = if hit.end.saturating_sub(lead) > room {
                    hit.start.saturating_sub(room / 3)
                } else {
                    lead
                };
                let take = |a: usize, b: usize| -> String {
                    chars
                        .get(a.min(chars.len())..b.min(chars.len()))
                        .map_or_else(String::new, |s| s.iter().collect())
                };
                Line::from(vec![
                    Span::styled(number, Style::new().fg(theme.muted).patch(base)),
                    Span::styled(take(from, hit.start), base),
                    Span::styled(take(hit.start, hit.end), hit_style),
                    Span::styled(take(hit.end, from + room), base),
                ])
            }
        };
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), area);
}
