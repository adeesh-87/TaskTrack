//! The task workspace: file tree, shell list, editor and terminal panes.

use std::fmt::Write as _;

use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Focus};
use crate::editor::{display_col, expand_tabs};
use crate::highlight::{Kind, Language, State};
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
pub fn draw_sidebar(frame: &mut Frame<'_>, app: &mut App, area: Rect, theme: &Theme) {
    let Some(ctx) = app.active_context() else {
        return;
    };
    let summary = ctx.attachment_summary();
    let header_height = if summary.is_empty() { 1 } else { 2 };
    let [header, tree_area, shells_area] = Layout::vertical([
        Constraint::Length(header_height),
        Constraint::Percentage(60),
        Constraint::Min(4),
    ])
    .areas(area);

    let title = format!(" ◀ {} ", ctx.id);
    let mut header_lines = vec![Line::styled(
        shorten(&title, area.width as usize),
        Style::new().fg(theme.header).add_modifier(Modifier::BOLD),
    )];
    if !summary.is_empty() {
        header_lines.push(Line::styled(
            shorten(&format!(" {summary}"), area.width as usize),
            Style::new().fg(theme.muted),
        ));
    }
    frame.render_widget(Paragraph::new(header_lines), header);

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
    let tree_selected = (!ctx.tree.nodes().is_empty()).then_some(ctx.tree.selected_index());

    // Shell list.
    let shells_focused = ctx.focus == Focus::Shells;
    let shell_items: Vec<ListItem<'_>> = ctx
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
    let shells_block = Block::bordered()
        .title(Span::styled(
            format!(" shells {} ", ctx.shells.len()),
            Style::new().fg(theme.accent),
        ))
        .border_style(theme.border(shells_focused));
    let selected_shell = ctx.selected_shell;
    let no_shells = shell_items.is_empty();

    app.ui.tree = tree_area;
    app.ui.tree_state.select(tree_selected);
    frame.render_stateful_widget(list, tree_area, &mut app.ui.tree_state);

    app.ui.shells = shells_area;
    if no_shells {
        let inner = shells_block.inner(shells_area);
        frame.render_widget(shells_block, shells_area);
        frame.render_widget(
            Paragraph::new("no shells · t opens one (Esc, s from anywhere)")
                .style(Style::new().fg(theme.muted))
                .wrap(Wrap { trim: true }),
            inner,
        );
        app.ui.shells_state.select(None);
    } else {
        let list = List::new(shell_items)
            .block(shells_block)
            .highlight_style(theme.selected(shells_focused));
        app.ui.shells_state.select(Some(selected_shell));
        frame.render_stateful_widget(list, shells_area, &mut app.ui.shells_state);
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

/// Build the styled spans of one editor line, clipped to `[hscroll, hscroll + width)`.
fn styled_line<'a>(
    expanded: &str,
    language: Option<Language>,
    state: State,
    hscroll: usize,
    width: usize,
    theme: &Theme,
) -> (Vec<Span<'a>>, State) {
    let chars: Vec<char> = expanded.chars().collect();
    let visible = |start: usize, end: usize| -> Option<String> {
        let s = start.max(hscroll);
        let e = end.min(hscroll + width);
        (s < e).then(|| chars[s..e].iter().collect())
    };
    let Some(lang) = language else {
        let text = visible(0, chars.len()).unwrap_or_default();
        return (vec![Span::raw(text)], State::Normal);
    };
    let (spans, next) = lang.highlight_line(expanded, state);
    let mut out = Vec::new();
    for sp in spans {
        if let Some(text) = visible(sp.start, sp.end) {
            match theme.syntax.style(sp.kind) {
                Some(style) if sp.kind != Kind::Text => out.push(Span::styled(text, style)),
                _ => out.push(Span::raw(text)),
            }
        }
    }
    (out, next)
}

fn draw_editor(frame: &mut Frame<'_>, app: &mut App, area: Rect, theme: &Theme) {
    let tab_width = usize::from(app.config().tab_width.max(1));
    let highlighting = app.config().syntax_highlighting;
    let Some(ctx) = app.active_context_mut() else {
        return;
    };
    let focused = ctx.focus == Focus::Editor;
    let Some(ed) = &ctx.editor else { return };

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
    if let Some(ed) = &mut ctx.editor {
        ed.ensure_visible(inner.height as usize);
    }
    if highlighting {
        ctx.refresh_highlight();
    } else {
        ctx.highlight = None;
    }
    let Some(ed) = &ctx.editor else { return };
    let language = ctx.highlight.as_ref().map(|h| h.language);
    let title_lang = language.map_or(String::new(), |l| format!(" {l:?} ").to_lowercase());

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

    let first = ed.scroll();
    let mut state = ctx
        .highlight
        .as_ref()
        .and_then(|h| h.states.get(first).copied())
        .unwrap_or_default();
    let lines: Vec<Line<'_>> = ed
        .lines()
        .iter()
        .enumerate()
        .skip(first)
        .take(inner.height as usize)
        .map(|(i, l)| {
            let expanded = expand_tabs(l, tab_width);
            let (spans, next) = styled_line(&expanded, language, state, hscroll, text_width, theme);
            state = next;
            let mut all = vec![Span::styled(
                format!("{:>w$} ", i + 1, w = gutter as usize - 1),
                Style::new().fg(theme.muted),
            )];
            all.extend(spans);
            Line::from(all)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
    if !title_lang.is_empty() {
        let w = title_lang.chars().count() as u16;
        if area.width > w + 4 {
            let rect = Rect {
                x: area.x + area.width - w - 2,
                y: area.y,
                width: w,
                height: 1,
            };
            frame.render_widget(
                Paragraph::new(Span::styled(title_lang, Style::new().fg(theme.muted))),
                rect,
            );
        }
    }

    if focused {
        let y = inner.y + (crow - ed.scroll()) as u16;
        let x = inner.x + gutter + (cursor_disp - hscroll) as u16;
        if y < inner.y + inner.height && x < inner.x + inner.width {
            frame.set_cursor_position(Position::new(x, y));
        }
    }
    app.ui.editor = inner;
    app.ui.editor_gutter = gutter;
    app.ui.editor_hscroll = hscroll;
    app.set_editor_height(inner.height as usize);
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
    app.ui.terminal = inner;
}
