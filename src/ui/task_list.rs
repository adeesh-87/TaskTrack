//! The task board sidebar.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem};
use ratatui::Frame;

use crate::app::{App, ListRow};

use super::Theme;

/// Draw the categorised task list into `area`.
pub fn draw(frame: &mut Frame<'_>, app: &mut App, area: Rect, theme: &Theme, focused: bool) {
    let rows = app.rows();
    let store = app.store();
    let width = area.width.saturating_sub(2) as usize;
    let items: Vec<ListItem<'_>> = rows
        .iter()
        .map(|row| match row {
            ListRow::Header(ci) => {
                let (name, count) = store
                    .and_then(|s| s.board().columns.get(*ci))
                    .map_or(("?".to_owned(), 0), |c| (c.name.clone(), c.tasks.len()));
                let archived = app.archived_count(*ci);
                let mut spans = vec![
                    Span::styled(
                        name.to_uppercase(),
                        Style::new().fg(theme.header).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" {}", count - archived),
                        Style::new().fg(theme.muted),
                    ),
                ];
                if archived > 0 {
                    spans.push(Span::styled(
                        format!(" +{archived} archived (A)"),
                        Style::new().fg(theme.muted),
                    ));
                }
                ListItem::new(Line::from(spans))
            }
            ListRow::Task(_, id) => {
                let missing = store.is_some_and(|s| !s.summary(id).has_context);
                let marker = if missing { " ⚠" } else { "" };
                let timed = app.timer().is_some_and(|t| t.task_id == *id);
                let lead = if timed { "⏱ " } else { "  " };
                let used = lead.chars().count() + id.chars().count() + marker.chars().count();
                let title: String = app
                    .record(id)
                    .map(|r| r.title.as_str())
                    .filter(|t| !t.is_empty() && width > used + 4)
                    .map(|t| {
                        let room = width - used - 1;
                        if t.chars().count() > room {
                            let cut: String = t.chars().take(room.saturating_sub(1)).collect();
                            format!(" {cut}…")
                        } else {
                            format!(" {t}")
                        }
                    })
                    .unwrap_or_default();
                ListItem::new(Line::from(vec![
                    Span::styled(lead, Style::new().fg(theme.accent)),
                    Span::raw(id.clone()),
                    Span::styled(marker, Style::new().fg(theme.warning)),
                    Span::styled(title, Style::new().fg(theme.muted)),
                ]))
            }
        })
        .collect();

    let title = match (app.store(), app.list_filter()) {
        (Some(_), (f, typing)) if !f.is_empty() || typing => format!(" filter: {f} "),
        (Some(s), _) => format!(" tasks · {} ", s.tasks_dir().display()),
        (None, _) => " tasks (no folder configured) ".to_owned(),
    };
    let block = Block::bordered()
        .title(Span::styled(
            shorten(&title, area.width.saturating_sub(2) as usize),
            Style::new().fg(theme.accent),
        ))
        .border_style(theme.border(focused));
    let list = List::new(items)
        .block(block)
        .highlight_style(theme.selected(focused));
    let selected = (!rows.is_empty()).then(|| app.list_selected().min(rows.len() - 1));
    app.ui.task_list = area;
    app.ui.task_list_state.select(selected);
    frame.render_stateful_widget(list, area, &mut app.ui.task_list_state);

    if rows.iter().all(|r| matches!(r, ListRow::Header(_))) {
        let hint = Rect {
            x: area.x + 2,
            y: area.y + area.height.saturating_sub(3),
            width: area.width.saturating_sub(4),
            height: 2,
        };
        if hint.width > 0 && hint.y > area.y {
            frame.render_widget(
                ratatui::widgets::Paragraph::new("no tasks yet · press n to create one")
                    .style(Style::new().fg(theme.muted))
                    .wrap(ratatui::widgets::Wrap { trim: true }),
                hint,
            );
        }
    }
}

/// Truncate from the left to at most `width` chars so the most specific part
/// of a path stays visible.
pub fn shorten(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width || width < 4 {
        return text.to_owned();
    }
    let tail: String = text.chars().skip(count - (width - 1)).collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::shorten;

    #[test]
    fn shorten_keeps_tail() {
        assert_eq!(shorten("abc", 10), "abc");
        assert_eq!(shorten("/very/long/path/tasks", 11), "…path/tasks");
        assert_eq!(shorten("abcdef", 3), "abcdef");
    }
}
