//! The settings page.

use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::config_form::{Row, Value};
use crate::app::{App, Mode};

use super::Theme;

/// Draw the settings page.
pub fn draw(frame: &mut Frame<'_>, app: &mut App, area: Rect, theme: &Theme) {
    let config_path = app.config_path().display().to_string();
    let Mode::Config(form) = app.mode() else {
        return;
    };
    let block = Block::bordered()
        .title(Span::styled(
            format!(" pahiri · settings · {config_path} "),
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        ))
        .border_style(theme.border(true));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let banner_height = if form.first_run() { 3 } else { 0 };
    let errors = form.errors();
    let error_height = errors.len().min(5) as u16;
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
    let rows = form.rows();
    let height = fields_area.height as usize;
    let first = form
        .selected()
        .saturating_sub(height.saturating_sub(1))
        .min(rows.len().saturating_sub(height));
    let mut cursor: Option<Position> = None;
    let mut lines: Vec<Line<'_>> = Vec::new();
    for (vi, (ri, row)) in rows.iter().enumerate().skip(first).take(height).enumerate() {
        let selected = ri == form.selected();
        let marker = if selected { "▶ " } else { "  " };
        let label_style = if selected {
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme.fg)
        };
        let value_style = if selected {
            theme.selected(true)
        } else {
            Style::new()
        };
        let edit_style = Style::new().bg(theme.selected_bg).fg(theme.selected_fg);
        let editing = selected.then_some(form.edit_state()).flatten();
        let y = fields_area.y + vi as u16;
        let line = match *row {
            Row::Field(fi) => {
                let f = &form.fields()[fi];
                let (value, style) = match (&f.value, editing) {
                    (Value::Text(_) | Value::Number(_), Some((buf, cur))) => {
                        cursor = Some(Position::new(
                            fields_area.x + 2 + label_width + cur as u16,
                            y,
                        ));
                        (buf.to_owned(), edit_style)
                    }
                    (Value::Text(t) | Value::Number(t), None) => (t.clone(), value_style),
                    (Value::Toggle(b), _) => {
                        (if *b { "on" } else { "off" }.to_owned(), value_style)
                    }
                    (Value::Scheme(s), _) => (format!("◂ {s} ▸"), value_style),
                    (Value::List(items), _) => (
                        format!("{} item(s)", items.len()),
                        Style::new().fg(theme.muted),
                    ),
                };
                Line::from(vec![
                    Span::styled(marker, label_style),
                    Span::styled(
                        format!("{:<w$}", f.label, w = label_width as usize),
                        label_style,
                    ),
                    Span::styled(value, style),
                ])
            }
            Row::Item { field, item } => {
                let text = match &form.fields()[field].value {
                    Value::List(items) => items[item].clone(),
                    _ => String::new(),
                };
                let (value, style) = match editing {
                    Some((buf, cur)) => {
                        cursor = Some(Position::new(fields_area.x + 6 + cur as u16, y));
                        (buf.to_owned(), edit_style)
                    }
                    None => (text, value_style),
                };
                Line::from(vec![
                    Span::styled(marker, label_style),
                    Span::styled("  · ", Style::new().fg(theme.muted)),
                    Span::styled(value, style),
                ])
            }
            Row::Add(_) => {
                let (value, style) = match editing {
                    Some((buf, cur)) => {
                        cursor = Some(Position::new(fields_area.x + 6 + cur as u16, y));
                        (buf.to_owned(), edit_style)
                    }
                    None => (
                        "+ add".to_owned(),
                        if selected {
                            value_style
                        } else {
                            Style::new().fg(theme.muted)
                        },
                    ),
                };
                Line::from(vec![
                    Span::styled(marker, label_style),
                    Span::raw("    "),
                    Span::styled(value, style),
                ])
            }
        };
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), fields_area);
    if let Some(pos) = cursor {
        if pos.y < fields_area.y + fields_area.height && pos.x < fields_area.x + fields_area.width {
            frame.set_cursor_position(pos);
        }
    }

    let (rows_rect, first_row) = (fields_area, first);
    let field = &form.fields()[form.selected_field()];
    let help_text = match form.selected_row() {
        Row::Field(_) => field.help.clone(),
        Row::Item { .. } => format!("{}  (Enter edit · d delete)", field.help),
        Row::Add(_) => format!("{}  (Enter to add)", field.help),
    };
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
    app.ui.config_rows = rows_rect;
    app.ui.config_first_row = first_row;
}
