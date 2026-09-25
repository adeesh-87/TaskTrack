//! Home's task card: what the selected task is about, above the Today pane.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::tasks::checkpoints::fmt_minutes;
use crate::tasks::plan::day_label;
use crate::tasks::record::TaskRecord;

use super::Theme;

/// Gerrit changes shown before "+N more".
const MAX_CHANGES: usize = 3;

/// Lines of the card for `r` (without the border).
pub fn lines(r: &TaskRecord, theme: &Theme) -> Vec<Line<'static>> {
    let muted = Style::new().fg(theme.muted);
    let mut out = Vec::new();
    let title = if r.title.is_empty() {
        "(no title: the first # heading of CONTEXT.md)".to_owned()
    } else {
        r.title.clone()
    };
    out.push(Line::styled(
        format!(" {title}"),
        Style::new().fg(theme.fg).add_modifier(Modifier::BOLD),
    ));
    if !r.summary.is_empty() && r.summary != r.title {
        out.push(Line::styled(format!(" {}", r.summary), muted));
    }
    if let Some(link) = &r.link {
        out.push(Line::styled(
            format!(" ↗ {link}"),
            Style::new().fg(theme.accent),
        ));
    }
    for c in r.changes.iter().take(MAX_CHANGES) {
        let status = c.status.clone().unwrap_or_default();
        let style = if status.starts_with("MERGED") {
            Style::new().fg(theme.accent)
        } else if status.starts_with("ABANDONED") {
            muted
        } else {
            Style::new().fg(theme.warning)
        };
        out.push(Line::from(vec![
            Span::styled(" CR ", muted),
            Span::styled(
                if status.is_empty() {
                    String::new()
                } else {
                    format!("{status} ")
                },
                style,
            ),
            Span::raw(if c.subject.is_empty() {
                c.change_id.clone()
            } else {
                c.subject.clone()
            }),
        ]));
    }
    if r.changes.len() > MAX_CHANGES {
        out.push(Line::styled(
            format!(" CR +{} more", r.changes.len() - MAX_CHANGES),
            muted,
        ));
    }
    let mut facts = Vec::new();
    if let Some(c) = r.next_checkpoint() {
        facts.push(format!(
            "next: {} ({})",
            c.title,
            fmt_minutes(c.estimate_min)
        ));
    }
    if !r.checkpoints.is_empty() {
        let done = r.checkpoints.iter().filter(|c| c.done).count();
        facts.push(format!("{done}/{} checkpoints", r.checkpoints.len()));
    }
    if r.time_spent_min > 0 {
        facts.push(format!("{} booked", fmt_minutes(r.time_spent_min)));
    }
    let day = |d: &Option<String>| d.as_deref().and_then(|s| s.get(..10)).map(day_label);
    if let Some(d) = day(&r.finished) {
        facts.push(format!("finished {d}"));
    } else if let Some(d) = day(&r.started) {
        facts.push(format!("started {d}"));
    } else if let Some(d) = day(&r.created) {
        facts.push(format!("created {d}"));
    }
    if !facts.is_empty() {
        out.push(Line::styled(format!(" {}", facts.join(" · ")), muted));
    }
    out
}

/// Height the card needs for `r` (with its border).
pub fn height(r: &TaskRecord, theme: &Theme) -> u16 {
    lines(r, theme).len() as u16 + 2
}

/// Draw the card for `r` into `area`.
pub fn draw(frame: &mut Frame<'_>, r: &TaskRecord, area: Rect, theme: &Theme) {
    let block = Block::bordered()
        .title(Span::styled(
            format!(" {} · {} ", r.id, r.column.to_uppercase()),
            Style::new().fg(theme.accent),
        ))
        .border_style(theme.border(false));
    frame.render_widget(Paragraph::new(lines(r, theme)).block(block), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line<'_>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn card_shows_title_summary_link_changes_and_progress() {
        let md = "# PROJ-7: Fix login\n\n## Description\n\nUsers get logged out after an hour.\n\n## Checkpoints\n<!-- pahiri:checkpoints -->\n- [x] Read (10m)\n- [ ] Patch (1h)\n<!-- /pahiri:checkpoints -->\n\n## Attachments\n<!-- pahiri:begin -->\n- link: https://jira/PROJ-7\n- gerrit: fw I1 https://g/1 [MERGED #1] :: Refresh the token\n- gerrit: fw I2 https://g/2 [NEW #2 CR+1] :: Add a test\n- gerrit: fw I3 :: Three\n- gerrit: fw I4 :: Four\n- started: 2026-09-21T09:00:00Z\n- time_spent: 70m\n<!-- pahiri:end -->\n";
        let r = TaskRecord::from_markdown("PROJ-7", "Doing", md);
        let t = text(&lines(
            &r,
            &Theme::for_scheme(crate::config::ColorScheme::Dark),
        ));
        assert_eq!(
            t,
            [
                " Fix login",
                " Users get logged out after an hour.",
                " ↗ https://jira/PROJ-7",
                " CR MERGED #1 Refresh the token",
                " CR NEW #2 CR+1 Add a test",
                " CR Three",
                " CR +1 more",
                " next: Patch (1h) · 1/2 checkpoints · 1h10m booked · started Mon 21 Sep",
            ]
        );
    }
}
