//! Home's right side, the Today pane: the day plan, what is timed now, time
//! booked, and the open work on the board.

use std::fmt::Write as _;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::app::{App, HomeFocus};
use crate::tasks::checkpoints::fmt_minutes;
use crate::tasks::plan::day_label;

use super::task_list::shorten;
use super::Theme;

/// Draw the Today pane into `area`.
pub fn draw_today(frame: &mut Frame<'_>, app: &mut App, area: Rect, theme: &Theme) {
    let focused = app.home_focus() == HomeFocus::Today;
    let bold = Style::new().fg(theme.header).add_modifier(Modifier::BOLD);
    let muted = Style::new().fg(theme.muted);
    let accent = Style::new().fg(theme.accent);
    let plan = app.day_plan();
    let capacity = app.config().planner.day_minutes;
    let planned = app.planned_open_min();

    let mut block = Block::bordered()
        .title(Span::styled(
            format!(" Today · {} ", day_label(app.plan_date())),
            accent,
        ))
        .border_style(theme.border(focused));
    if !plan.is_empty() {
        let style = if planned > capacity {
            Style::new().fg(theme.warning)
        } else {
            muted
        };
        block = block.title(
            Line::styled(
                format!(
                    " {} left of {} ",
                    fmt_minutes(planned),
                    fmt_minutes(capacity)
                ),
                style,
            )
            .right_aligned(),
        );
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = inner.width as usize;
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut item_rows: Vec<(u16, usize)> = Vec::new();

    if let Some(t) = app.timer() {
        let now = std::time::Instant::now();
        lines.push(Line::styled(" Now", bold));
        lines.push(Line::from(vec![
            Span::styled(
                format!("  ⏱ {} ", t.task_id),
                accent.add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("{} · {}", t.what(), t.label(now))),
        ]));
        lines.push(Line::raw(""));
    }

    // The plan.
    if plan.is_empty() {
        lines.push(Line::styled(
            " No plan for today yet · p plans the day",
            bold,
        ));
        if let Some((day, n)) = app.carry_hint() {
            lines.push(Line::styled(
                format!(
                    "  ↩ {n} unfinished from {} · p carries them over",
                    day_label(day)
                ),
                Style::new().fg(theme.warning),
            ));
        }
    } else {
        let done = plan.iter().filter(|i| app.item_status(i).done).count();
        lines.push(Line::from(vec![
            Span::styled(" Plan", bold),
            Span::styled(format!("  {done}/{} done", plan.len()), muted),
        ]));
        let id_width = plan
            .iter()
            .filter_map(|i| i.task.as_ref().map(|t| t.chars().count()))
            .max()
            .unwrap_or(0)
            .min(16);
        let selected = app.today_selected();
        for (i, item) in plan.iter().enumerate() {
            let s = app.item_status(item);
            let (mark, mark_style) = if s.done {
                ("✓", Style::new().fg(theme.accent))
            } else if s.timed {
                ("▶", accent.add_modifier(Modifier::BOLD))
            } else if s.missing {
                ("?", Style::new().fg(theme.warning))
            } else {
                ("·", muted)
            };
            let task = item
                .task
                .as_deref()
                .map_or_else(String::new, |t| shorten(t, id_width));
            let minutes = if s.done {
                if s.spent_min > 0 {
                    format!("spent {}", fmt_minutes(s.spent_min))
                } else {
                    String::new()
                }
            } else {
                fmt_minutes(s.remaining_min(item))
            };
            let title_room = width.saturating_sub(id_width + minutes.chars().count() + 9);
            let title = shorten_end(&item.title, title_room);
            let pad = title_room.saturating_sub(title.chars().count());
            let text_style = if s.done {
                muted.add_modifier(Modifier::CROSSED_OUT)
            } else {
                Style::new().fg(theme.fg)
            };
            let mut spans = vec![
                Span::styled(format!("  {mark} "), mark_style),
                Span::styled(format!("{task:<id_width$}  "), accent),
                Span::styled(title, text_style),
                Span::raw(" ".repeat(pad + 1)),
                Span::styled(minutes, muted),
            ];
            if focused && i == selected {
                let sel = theme.selected(true);
                for span in &mut spans {
                    span.style = span.style.patch(sel);
                }
            }
            item_rows.push((inner.y + lines.len() as u16, i));
            lines.push(Line::from(spans));
        }
    }
    lines.push(Line::raw(""));

    // Time booked.
    let today = app.today();
    if today.week_min > 0 || today.finished_week > 0 || today.estimate_factor.is_some() {
        let per: Vec<String> = today
            .today_by_task
            .iter()
            .take(4)
            .map(|(t, m)| format!("{t} {}", fmt_minutes(*m)))
            .collect();
        lines.push(Line::from(vec![
            Span::styled(" Booked", bold),
            Span::styled(format!("  {} today", fmt_minutes(today.today_min)), accent),
            Span::styled(
                if per.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", per.join(" · "))
                },
                muted,
            ),
        ]));
        let mut week = format!(
            "  this week {} · {} finished",
            fmt_minutes(today.week_min),
            today.finished_week
        );
        if let Some((f, n)) = today.estimate_factor {
            let _ = write!(week, " · your pace ×{f:.2} (from {n} checkpoints)");
        }
        lines.push(Line::styled(week, muted));
        lines.push(Line::raw(""));
    }

    // Open work on the board.
    let records = app.next_up();
    if records.is_empty() {
        lines.push(Line::styled(" Nothing open.", muted));
        lines.push(Line::styled(" n creates a task · Esc for commands.", muted));
    } else {
        lines.push(Line::styled(" Open work", bold));
        let id_width = records
            .iter()
            .map(|r| r.id.chars().count())
            .max()
            .unwrap_or(4)
            .min(18);
        let mut column = String::new();
        for r in records {
            if r.column != column {
                column.clone_from(&r.column);
                lines.push(Line::styled(format!("  {}", column.to_uppercase()), muted));
            }
            let id = shorten(&r.id, id_width);
            let what = match r.next_checkpoint() {
                Some(c) => {
                    let done = r.checkpoints.iter().filter(|c| c.done).count();
                    let mut spans = Vec::new();
                    // What the task is, then its next step.
                    if !r.title.is_empty() {
                        spans.push(Span::raw(shorten_end(&r.title, (width / 3).max(12))));
                        spans.push(Span::styled(" · next: ", muted));
                    }
                    spans.push(Span::raw(format!("{} ", c.title)));
                    spans.push(Span::styled(
                        format!(
                            "({}) · {done}/{}",
                            fmt_minutes(c.estimate_min),
                            r.checkpoints.len()
                        ),
                        muted,
                    ));
                    spans
                }
                None if !r.checkpoints.is_empty() => vec![
                    Span::raw(shorten_end(&r.title, (width / 3).max(12))),
                    Span::styled(
                        if r.title.is_empty() {
                            "all checkpoints done → ] to finish"
                        } else {
                            " · all checkpoints done → ] to finish"
                        },
                        muted,
                    ),
                ],
                None if r.context_ready => vec![Span::styled(
                    "context ready · Esc b plans checkpoints",
                    Style::new().fg(theme.warning),
                )],
                None => vec![Span::styled(
                    if r.title.is_empty() {
                        "define it: Esc i writes the context".to_owned()
                    } else {
                        format!("{} · Esc i to define it", r.title)
                    },
                    muted,
                )],
            };
            let mut spans = vec![Span::styled(format!("   {id:<id_width$}  "), accent)];
            spans.extend(what);
            lines.push(Line::from(spans));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        if focused {
            " Enter starts the timer · Space ticks · J/K order · x removes · o opens the task · Tab board"
        } else {
            " p plans the day · Tab → today's plan · Enter opens a task · m timer · F1 help"
        },
        muted,
    ));

    frame.render_widget(Paragraph::new(lines), inner);
    let bottom = inner.y + inner.height;
    item_rows.retain(|(y, _)| *y < bottom);
    app.ui.today = inner;
    app.ui.today_items = item_rows;
}

/// Cut `text` to `width` chars, ending with `…` when cut.
fn shorten_end(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}
