//! The Plan view: what can be planned (left) and today's plan in order (right).

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::app::{App, Mode, PlanFocus, PlanRow};
use crate::tasks::checkpoints::fmt_minutes;
use crate::tasks::plan::day_label;

use super::Theme;

/// Width of the capacity bar in cells.
const BAR: usize = 12;

/// Draw the Plan view into `area`.
pub fn draw(frame: &mut Frame<'_>, app: &mut App, area: Rect, theme: &Theme) {
    let Mode::Plan(view) = app.mode() else {
        return;
    };
    let bold = Style::new().fg(theme.header).add_modifier(Modifier::BOLD);
    let muted = Style::new().fg(theme.muted);
    let accent = Style::new().fg(theme.accent);
    let [header, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(3)]).areas(area);
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(body);

    // Header: day, capacity bar, pace.
    let planned = view.planned_min();
    let cap = view.capacity_min.max(1);
    let filled = ((planned as usize * BAR) / cap as usize).min(BAR);
    let over = planned > view.capacity_min;
    let mut spans = vec![
        Span::styled(format!(" Plan · {} ", day_label(&view.date)), bold),
        Span::styled(
            format!(
                " {} of {} ",
                fmt_minutes(planned),
                fmt_minutes(view.capacity_min)
            ),
            if over {
                Style::new().fg(theme.warning)
            } else {
                accent
            },
        ),
        Span::styled(
            "█".repeat(filled),
            Style::new().fg(if over { theme.warning } else { theme.accent }),
        ),
        Span::styled("░".repeat(BAR - filled), muted),
    ];
    if over {
        spans.push(Span::styled(
            format!(" over by {}", fmt_minutes(planned - view.capacity_min)),
            Style::new().fg(theme.warning),
        ));
    }
    if (view.factor - 1.0).abs() > 0.01 {
        spans.push(Span::styled(
            format!("  · estimates × {:.2} (your pace)", view.factor),
            muted,
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), header);

    // Left: what can be planned.
    let pick_items: Vec<ListItem<'static>> = view
        .rows
        .iter()
        .map(|row| match row {
            PlanRow::Section(s) => ListItem::new(Line::styled(format!(" {s}"), bold)),
            PlanRow::Task(id, title) => ListItem::new(Line::from(vec![
                Span::styled(format!("  {id} "), accent.add_modifier(Modifier::BOLD)),
                Span::styled(title.clone(), muted),
            ])),
            PlanRow::Item(item) => {
                let s = view.status(item);
                let mark = if view.is_picked(item) { "[x]" } else { "[ ]" };
                let mut spans = vec![
                    Span::styled(format!("    {mark} "), accent),
                    Span::raw(item.title.clone()),
                    Span::styled(format!("  {}", fmt_minutes(s.remaining_min(item))), muted),
                ];
                if s.spent_min > 0 {
                    spans.push(Span::styled(
                        format!(" (of {})", fmt_minutes(item.estimate_min)),
                        muted,
                    ));
                }
                if item.task.is_none() {
                    spans.push(Span::styled("  free item", muted));
                } else if is_carried(view, item) {
                    spans.push(Span::styled(
                        format!("  {}", item.task.as_deref().unwrap_or_default()),
                        muted,
                    ));
                }
                ListItem::new(Line::from(spans))
            }
            PlanRow::Hint(h) => ListItem::new(Line::styled(
                format!("    {h}"),
                muted.add_modifier(Modifier::ITALIC),
            )),
        })
        .collect();
    let pick_focused = view.focus == PlanFocus::Pick;
    let pick_cursor = (!view.rows.is_empty()).then_some(view.cursor);
    let pick_empty = view.rows.is_empty();

    // Right: the plan in order.
    let order_items: Vec<ListItem<'static>> = view
        .picked
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let s = view.status(item);
            let done = s.done || item.done;
            let mut spans = vec![Span::styled(format!(" {:>2}. ", i + 1), muted)];
            if done {
                spans.push(Span::styled("✓ ", accent));
            }
            if let Some(t) = &item.task {
                spans.push(Span::styled(format!("{t} · "), accent));
            }
            spans.push(Span::styled(
                item.title.clone(),
                if done {
                    muted.add_modifier(Modifier::CROSSED_OUT)
                } else {
                    Style::new().fg(theme.fg)
                },
            ));
            if !done {
                spans.push(Span::styled(
                    format!("  {}", fmt_minutes(view.cost(item))),
                    muted,
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let order_focused = view.focus == PlanFocus::Order;
    let order_cursor = (!view.picked.is_empty()).then_some(view.order_cursor);
    let order_empty = view.picked.is_empty();

    let pick = List::new(pick_items)
        .block(
            Block::bordered()
                .title(Span::styled(" What can be planned ", accent))
                .border_style(theme.border(pick_focused)),
        )
        .highlight_style(theme.selected(pick_focused));
    app.ui.plan_pick = left;
    app.ui.plan_pick_state.select(pick_cursor);
    frame.render_stateful_widget(pick, left, &mut app.ui.plan_pick_state);
    if pick_empty {
        hint(
            frame,
            left,
            "No tasks in the columns you plan from · A shows all",
            theme,
        );
    }

    let order = List::new(order_items)
        .block(
            Block::bordered()
                .title(Span::styled(" Today, in order ", accent))
                .title_bottom(Line::styled(" J/K move · x remove ", muted).right_aligned())
                .border_style(theme.border(order_focused)),
        )
        .highlight_style(theme.selected(order_focused));
    app.ui.plan_order = right;
    app.ui.plan_order_state.select(order_cursor);
    frame.render_stateful_widget(order, right, &mut app.ui.plan_order_state);
    if order_empty {
        hint(
            frame,
            right,
            "Nothing planned yet · Space picks on the left, s suggests a day",
            theme,
        );
    }
}

/// Whether `item` sits in the carried-over section (shown with its task id).
fn is_carried(view: &crate::app::PlanView, item: &crate::tasks::plan::PlanItem) -> bool {
    let mut in_carried = false;
    for row in &view.rows {
        match row {
            PlanRow::Section(s) => in_carried = s.starts_with("CARRIED"),
            PlanRow::Item(i) if in_carried && i.same(item) => return true,
            _ => {}
        }
    }
    false
}

fn hint(frame: &mut Frame<'_>, area: Rect, text: &str, theme: &Theme) {
    let rect = Rect {
        x: area.x + 2,
        y: area.y + 2,
        width: area.width.saturating_sub(4),
        height: 2.min(area.height.saturating_sub(3)),
    };
    if rect.height > 0 && rect.width > 0 {
        frame.render_widget(
            Paragraph::new(text.to_owned())
                .style(Style::new().fg(theme.muted))
                .wrap(ratatui::widgets::Wrap { trim: true }),
            rect,
        );
    }
}
