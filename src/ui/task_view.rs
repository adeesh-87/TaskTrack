//! The task workspace: file tree, shell list, editor and terminal panes.

use std::fmt::Write as _;

use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Focus};
use crate::terminal::{cursor_position, TerminalView};

use super::task_list::shorten;
use super::Theme;

/// Right-hand placeholder shown in task-list mode.
pub fn draw_placeholder(frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
    let block = Block::bordered().border_style(theme.border(false));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = vec![
        Line::styled(
            "pahiri",
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::styled(
            "Select a task on the left and press Enter.",
            Style::new().fg(theme.muted),
        ),
        Line::styled(
            "Esc opens the command palette (n new task · c settings · q quit).",
            Style::new().fg(theme.muted),
        ),
    ];
    let height = lines.len() as u16;
    let rect = super::popup::centered(inner, inner.width.saturating_sub(4).max(1), height);
    frame.render_widget(Paragraph::new(lines).centered(), rect);
}

/// Left column inside a task: task header, file tree, shell list.
pub fn draw_sidebar(frame: &mut Frame<'_>, app: &App, area: Rect, theme: &Theme) {
    let Some(ctx) = app.active_context() else {
        return;
    };
    let [header, tree_area, shells_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Percentage(60),
        Constraint::Min(4),
    ])
    .areas(area);

    let title = format!(" ◀ {} ", ctx.id);
    frame.render_widget(
        Paragraph::new(Span::styled(
            shorten(&title, area.width as usize),
            Style::new().fg(theme.header).add_modifier(Modifier::BOLD),
        )),
        header,
    );

    // File tree.
    let focused = ctx.focus == Focus::Tree;
    let items: Vec<ListItem<'_>> = ctx
        .tree
        .nodes()
        .iter()
        .map(|n| {
            let indent = "  ".repeat(n.depth);
            let icon = if n.is_dir {
                if n.expanded {
                    "▾ "
                } else {
                    "▸ "
                }
            } else {
                "  "
            };
            let style = if n.is_dir {
                Style::new().fg(theme.accent)
            } else {
                Style::new()
            };
            ListItem::new(Line::from(vec![
                Span::raw(indent),
                Span::styled(format!("{icon}{}", n.name), style),
            ]))
        })
        .collect();
    let block = Block::bordered()
        .title(Span::styled(" files ", Style::new().fg(theme.accent)))
        .border_style(theme.border(focused));
    let list = List::new(items)
        .block(block)
        .highlight_style(theme.selected(focused));
    let mut state = ListState::default();
    if !ctx.tree.nodes().is_empty() {
        state.select(Some(ctx.tree.selected_index()));
    }
    frame.render_stateful_widget(list, tree_area, &mut state);

    // Shell list.
    let focused = ctx.focus == Focus::Shells;
    let items: Vec<ListItem<'_>> = ctx
        .shells
        .iter()
        .map(|s| {
            let name = s.display_name();
            let (marker, style) = if s.session.has_exited() {
                ("✗ ", Style::new().fg(theme.muted))
            } else {
                ("● ", Style::new().fg(theme.fg))
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    marker,
                    if s.session.has_exited() {
                        Style::new().fg(theme.muted)
                    } else {
                        Style::new().fg(theme.accent)
                    },
                ),
                Span::styled(name, style),
            ]))
        })
        .collect();
    let block = Block::bordered()
        .title(Span::styled(
            format!(" shells {} ", ctx.shells.len()),
            Style::new().fg(theme.accent),
        ))
        .border_style(theme.border(focused));
    if items.is_empty() {
        let inner = block.inner(shells_area);
        frame.render_widget(block, shells_area);
        frame.render_widget(
            Paragraph::new("no shells · t opens one (Esc, s from anywhere)")
                .style(Style::new().fg(theme.muted))
                .wrap(Wrap { trim: true }),
            inner,
        );
    } else {
        let list = List::new(items)
            .block(block)
            .highlight_style(theme.selected(focused));
        let mut state = ListState::default();
        state.select(Some(ctx.selected_shell));
        frame.render_stateful_widget(list, shells_area, &mut state);
    }
}

/// Right column inside a task: editor on top, terminal below (each optional).
pub fn draw_workspace(frame: &mut Frame<'_>, app: &mut App, area: Rect, theme: &Theme) {
    let (has_editor, has_shell) = match app.active_context() {
        Some(c) => (c.editor.is_some(), c.shell_pane_visible()),
        None => (false, false),
    };
    match (has_editor, has_shell) {
        (true, true) => {
            let [top, bottom] =
                Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .areas(area);
            draw_editor(frame, app, top, theme);
            draw_terminal(frame, app, bottom, theme, false);
        }
        (true, false) => draw_editor(frame, app, area, theme),
        (false, true) => draw_terminal(frame, app, area, theme, false),
        (false, false) => {
            let block = Block::bordered().border_style(theme.border(false));
            let inner = block.inner(area);
            frame.render_widget(block, area);
            let lines = vec![
                Line::styled("Nothing open.", Style::new().fg(theme.muted)),
                Line::styled(
                    "Enter opens a file here · t opens a shell below · Esc for commands.",
                    Style::new().fg(theme.muted),
                ),
            ];
            let rect = super::popup::centered(inner, inner.width.saturating_sub(4).max(1), 2);
            frame.render_widget(Paragraph::new(lines).centered(), rect);
        }
    }
}

fn expand_tabs(line: &str, tab_width: usize) -> String {
    if !line.contains('\t') {
        return line.to_owned();
    }
    let mut out = String::new();
    let mut col = 0;
    for c in line.chars() {
        if c == '\t' {
            let n = tab_width - (col % tab_width);
            out.push_str(&" ".repeat(n));
            col += n;
        } else {
            out.push(c);
            col += UnicodeWidthStr::width(c.to_string().as_str());
        }
    }
    out
}

/// Display column of char index `col` in `line` after tab expansion.
fn display_col(line: &str, col: usize, tab_width: usize) -> usize {
    let prefix: String = line.chars().take(col).collect();
    UnicodeWidthStr::width(expand_tabs(&prefix, tab_width).as_str())
}

fn draw_editor(frame: &mut Frame<'_>, app: &mut App, area: Rect, theme: &Theme) {
    let tab_width = usize::from(app.config().tab_width.max(1));
    let Some(ctx) = app.active_context_mut() else {
        return;
    };
    let focused = ctx.focus == Focus::Editor;
    let Some(ed) = &mut ctx.editor else { return };

    let mut title = format!(
        " {} ",
        ed.path()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
    if ed.is_dirty() {
        title.push_str("● ");
    }
    if ed.is_read_only() {
        title.push_str("[read-only] ");
    }
    let block = Block::bordered()
        .title(Span::styled(title, Style::new().fg(theme.accent)))
        .title_bottom(Line::styled(
            format!(" {}:{} ", ed.cursor().0 + 1, ed.cursor().1 + 1),
            Style::new().fg(theme.muted),
        ))
        .border_style(theme.border(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    ed.ensure_visible(inner.height as usize);

    let gutter = (ed.lines().len().max(1).to_string().len() + 1) as u16;
    let text_width = inner.width.saturating_sub(gutter + 1) as usize;
    let (crow, ccol) = ed.cursor();
    let cursor_disp = display_col(
        ed.lines().get(crow).map_or("", String::as_str),
        ccol,
        tab_width,
    );
    let hscroll = if text_width == 0 {
        0
    } else {
        cursor_disp.saturating_sub(text_width.saturating_sub(1))
    };

    let lines: Vec<Line<'_>> = ed
        .lines()
        .iter()
        .enumerate()
        .skip(ed.scroll())
        .take(inner.height as usize)
        .map(|(i, l)| {
            let expanded = expand_tabs(l, tab_width);
            let visible: String = expanded.chars().skip(hscroll).collect();
            Line::from(vec![
                Span::styled(
                    format!("{:>w$} ", i + 1, w = gutter as usize - 1),
                    Style::new().fg(theme.muted),
                ),
                Span::raw(visible),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);

    if focused {
        let y = inner.y + (crow - ed.scroll()) as u16;
        let x = inner.x + gutter + (cursor_disp - hscroll) as u16;
        if y < inner.y + inner.height && x < inner.x + inner.width {
            frame.set_cursor_position(Position::new(x, y));
        }
    }
    let () = app.set_editor_height(inner.height as usize);
}

/// Draw the active shell into `area`, resizing the PTY to fit.
pub fn draw_terminal(
    frame: &mut Frame<'_>,
    app: &mut App,
    area: Rect,
    theme: &Theme,
    zoomed: bool,
) {
    let leader = app.leader().to_string();
    let leader_pending = app.leader_pending();
    let Some(ctx) = app.active_context_mut() else {
        return;
    };
    let focused = ctx.focus == Focus::Terminal;
    let count = ctx.shells.len();
    let index = ctx.selected_shell;
    let Some(shell) = ctx.active_shell_mut() else {
        return;
    };

    let mut title = format!(" {} ", shell.display_name());
    if count > 1 {
        let _ = write!(title, "({}/{count}) ", index + 1);
    }
    if shell.session.has_exited() {
        title.push_str("[exited · press any key] ");
    }
    if zoomed {
        title.push_str("[zoomed] ");
    }
    let scrollback = shell.session.screen().scrollback();
    let bottom = if leader_pending {
        format!(" {leader} … ")
    } else if scrollback > 0 {
        format!(" scrollback +{scrollback} ")
    } else if focused {
        format!(" {leader} q leave · {leader} z zoom ")
    } else {
        String::new()
    };
    let block = Block::bordered()
        .title(Span::styled(title, Style::new().fg(theme.accent)))
        .title_bottom(Line::styled(bottom, Style::new().fg(theme.muted)).right_aligned())
        .border_style(theme.border(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 2 || inner.height < 2 {
        return;
    }
    if let Err(e) = shell.session.resize(inner.height, inner.width) {
        tracing::warn!("pty resize failed: {e:#}");
    }
    frame.render_widget(TerminalView::new(shell.session.screen()), inner);
    if focused {
        if let Some(pos) = cursor_position(shell.session.screen(), inner) {
            frame.set_cursor_position(pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_expansion_and_display_columns() {
        assert_eq!(expand_tabs("a\tb", 4), "a   b");
        assert_eq!(expand_tabs("\t\tx", 2), "    x");
        assert_eq!(display_col("a\tb", 2, 4), 4);
        assert_eq!(display_col("你好", 1, 4), 2);
    }
}
