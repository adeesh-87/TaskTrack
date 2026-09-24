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
    let items: Vec<ListItem<'_>> = rows
        .iter()
        .map(|row| match row {
            ListRow::Header(ci) => {
                let (name, count) = store
                    .and_then(|s| s.board().columns.get(*ci))
                    .map_or(("?".to_owned(), 0), |c| (c.name.clone(), c.tasks.len()));
                ListItem::new(Line::from(vec![
                    Span::styled(
                        name.to_uppercase(),
                        Style::new().fg(theme.header).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!(" {count}"), Style::new().fg(theme.muted)),
                ]))
            }
            ListRow::Task(_, id) => {
                let missing = store.is_some_and(|s| !s.summary(id).has_context);
                let marker = if missing { " ⚠" } else { "" };
                ListItem::new(Line::from(vec![
                    Span::raw(format!("  {id}")),
                    Span::styled(marker, Style::new().fg(theme.warning)),
                ]))
            }
        })
        .collect();

    let title = match app.store() {
        Some(s) => format!(" tasks · {} ", s.tasks_dir().display()),
        None => " tasks (no folder configured) ".to_owned(),
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
