//! The Audit view: the proposal (left) and what the selected one does (right).

use std::fmt::Write as _;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{change_line, App, Mode};
use crate::tasks::audit::Proposal;
use crate::tasks::plan::day_label;

use super::Theme;

/// Draw the Audit view into `area`.
pub fn draw(frame: &mut Frame<'_>, app: &mut App, area: Rect, theme: &Theme) {
    let Mode::Audit(view) = app.mode() else {
        return;
    };
    let bold = Style::new().fg(theme.header).add_modifier(Modifier::BOLD);
    let muted = Style::new().fg(theme.muted);
    let accent = Style::new().fg(theme.accent);
    let warn = Style::new().fg(theme.warning);
    let columns: Vec<String> = app.store().map_or_else(Vec::new, |s| {
        s.board().columns.iter().map(|c| c.name.clone()).collect()
    });

    let (new, upd, orphans, ticked) = view.counts();
    let mut header = vec![Line::from(vec![
        Span::styled(format!(" Audit · since {} ", day_label(&view.since)), bold),
        Span::styled(
            format!(
                " {} ticket(s) · {} change(s) → {new} new · {upd} update(s) · {orphans} need you · {ticked} ticked",
                view.input.tickets.len(),
                view.input.changes.len()
            ),
            muted,
        ),
    ])];
    for n in &view.notes {
        // Fetch and agent failures stand out; the agent's summary does not.
        let style = if n.starts_with("the agent decided") {
            muted
        } else {
            warn
        };
        header.push(Line::styled(format!(" {n}"), style));
    }
    let header_height = header.len() as u16;
    let [top, body] =
        Layout::vertical([Constraint::Length(header_height), Constraint::Min(3)]).areas(area);
    frame.render_widget(Paragraph::new(header), top);
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).areas(body);

    // The proposal, grouped.
    let mut items: Vec<ListItem<'static>> = Vec::new();
    let mut rows: Vec<Option<usize>> = Vec::new();
    let mut group = "";
    for (i, item) in view.items.iter().enumerate() {
        let (name, count) = match item.proposal {
            Proposal::Create(_) => ("NEW TASKS", new),
            Proposal::Update(_) => ("UPDATES", upd),
            Proposal::Orphan(_) => ("NEED YOU: changes nothing claimed", orphans),
        };
        if name != group {
            group = name;
            items.push(ListItem::new(Line::styled(
                format!(" {name} ({count})"),
                bold,
            )));
            rows.push(None);
        }
        let mark = match item.proposal {
            Proposal::Orphan(_) => "  ? ",
            _ if view.accepted(item) => "[x] ",
            _ => "[ ] ",
        };
        let mut spans = vec![Span::styled(format!(" {mark}"), accent)];
        match &item.proposal {
            Proposal::Create(n) => {
                spans.push(Span::styled(
                    format!("{} ", view.id_of(n)),
                    accent.add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::raw(n.title.clone()));
                let col = columns.get(n.column).cloned().unwrap_or_default();
                let mut tail = format!("  → {col}");
                if !n.changes.is_empty() {
                    let _ = write!(tail, " · {} CR", n.changes.len());
                }
                spans.push(Span::styled(tail, muted));
            }
            Proposal::Update(u) => {
                spans.push(Span::styled(
                    format!("{} ", u.id),
                    accent.add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(u.what.join(" · "), muted));
            }
            Proposal::Orphan(o) => {
                spans.push(Span::raw(change_line(&o.change)));
                spans.push(Span::styled(format!("  {}", o.reason), muted));
            }
        }
        if item.by_ai {
            spans.push(Span::styled("  AI", warn));
        }
        items.push(ListItem::new(Line::from(spans)));
        rows.push(Some(i));
    }
    let selected_row = rows.iter().position(|r| *r == Some(view.selected));
    let details: Vec<Line<'static>> = view
        .items
        .get(view.selected)
        .map(|item| {
            view.describe(item, &columns)
                .into_iter()
                .map(|l| Line::raw(format!(" {l}")))
                .collect()
        })
        .unwrap_or_default();

    let list = List::new(items)
        .block(
            Block::bordered()
                .title(Span::styled(" Proposal ", accent))
                .border_style(theme.border(true)),
        )
        .highlight_style(theme.selected(true));
    app.ui.audit_list = left;
    app.ui.audit_rows = rows;
    app.ui.audit_state.select(selected_row);
    frame.render_stateful_widget(list, left, &mut app.ui.audit_state);
    frame.render_widget(
        Paragraph::new(details).wrap(Wrap { trim: false }).block(
            Block::bordered()
                .title(Span::styled(" What it does ", accent))
                .border_style(theme.border(false)),
        ),
        right,
    );
}
