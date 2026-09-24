//! The settings page.

use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::config_form::FieldKind;
use crate::app::{App, Mode};

use super::Theme;

/// Draw the settings page.
pub fn draw(frame: &mut Frame<'_>, app: &App, area: Rect, theme: &Theme) {
    let Mode::Config(form) = app.mode() else {
        return;
    };
    let block = Block::bordered()
        .title(Span::styled(
            " pahiri · settings ",
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        ))
        .border_style(theme.border(true));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let banner_height = if form.first_run() { 3 } else { 0 };
    let errors = form.errors();
    let error_height = errors.len().min(4) as u16;
    let [banner, fields_area, help, errors_area] = Layout::vertical([
        Constraint::Length(banner_height),
        Constraint::Min(3),
        Constraint::Length(3),
        Constraint::Length(error_height),
    ])
    .areas(inner);

    if form.first_run() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(" Welcome to pahiri.", Style::new().fg(theme.header).add_modifier(Modifier::BOLD)),
                Line::styled(
                    " Set the tasks folder (a folder with one sub-folder per task), then press Ctrl+S to save.",
                    Style::new().fg(theme.muted),
                ),
            ])
            .wrap(Wrap { trim: false }),
            banner,
        );
    }

    let label_width = form
        .fields()
        .iter()
        .map(|f| f.label.len())
        .max()
        .unwrap_or(10) as u16
        + 2;
    let mut cursor: Option<Position> = None;
    let lines: Vec<Line<'_>> = form
        .fields()
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let selected = i == form.selected();
            let marker = if selected { "▶ " } else { "  " };
            let label_style = if selected {
                Style::new().fg(theme.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(theme.fg)
            };
            let (value, value_style) =
                if let (true, Some((buf, cur))) = (selected, form.edit_state()) {
                    cursor = Some(Position::new(
                        fields_area.x + 2 + label_width + cur as u16,
                        fields_area.y + i as u16,
                    ));
                    (
                        buf.to_owned(),
                        Style::new().bg(theme.selected_bg).fg(theme.selected_fg),
                    )
                } else {
                    let shown = match f.kind {
                        FieldKind::Toggle => {
                            if f.value == "true" {
                                "on".to_owned()
                            } else {
                                "off".to_owned()
                            }
                        }
                        FieldKind::Scheme => format!("◂ {} ▸", f.value),
                        _ => f.value.clone(),
                    };
                    (
                        shown,
                        if selected {
                            theme.selected(true)
                        } else {
                            Style::new()
                        },
                    )
                };
            Line::from(vec![
                Span::styled(marker, label_style),
                Span::styled(
                    format!("{:<w$}", f.label, w = label_width as usize),
                    label_style,
                ),
                Span::styled(value, value_style),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), fields_area);
    if let Some(pos) = cursor {
        if pos.y < fields_area.y + fields_area.height {
            frame.set_cursor_position(pos);
        }
    }

    let help_text = form.fields().get(form.selected()).map_or("", |f| f.help);
    frame.render_widget(
        Paragraph::new(Line::styled(
            format!("  {help_text}"),
            Style::new().fg(theme.muted),
        ))
        .wrap(Wrap { trim: false }),
        help,
    );

    if !errors.is_empty() {
        let lines: Vec<Line<'_>> = errors
            .iter()
            .map(|e| Line::styled(format!("  ✗ {e}"), Style::new().fg(theme.error)))
            .collect();
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }),
            errors_area,
        );
    }
}
