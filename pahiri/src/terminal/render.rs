//! Paint a [`vt100::Screen`] into a ratatui buffer.

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;

/// Widget rendering the visible part of a virtual terminal screen.
pub struct TerminalView<'a> {
    screen: &'a vt100::Screen,
}

impl<'a> TerminalView<'a> {
    /// Create a view over `screen`.
    pub fn new(screen: &'a vt100::Screen) -> Self {
        Self { screen }
    }
}

fn convert_color(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Style for one screen cell.
pub fn cell_style(cell: &vt100::Cell) -> Style {
    let mut style = Style::new()
        .fg(convert_color(cell.fgcolor()))
        .bg(convert_color(cell.bgcolor()));
    if cell.bold() {
        style = style.add_modifier(Modifier::BOLD);
    }
    if cell.dim() {
        style = style.add_modifier(Modifier::DIM);
    }
    if cell.italic() {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if cell.underline() {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if cell.inverse() {
        style = style.add_modifier(Modifier::REVERSED);
    }
    style
}

impl Widget for TerminalView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (rows, cols) = self.screen.size();
        for row in 0..rows.min(area.height) {
            let mut col = 0u16;
            while col < cols.min(area.width) {
                let Some(cell) = self.screen.cell(row, col) else {
                    break;
                };
                let x = area.x + col;
                let y = area.y + row;
                if cell.is_wide_continuation() {
                    col += 1;
                    continue;
                }
                if let Some(target) = buf.cell_mut(Position::new(x, y)) {
                    let contents = cell.contents();
                    target.set_symbol(if contents.is_empty() { " " } else { contents });
                    target.set_style(cell_style(cell));
                }
                if cell.is_wide() {
                    if let Some(next) = buf.cell_mut(Position::new(x + 1, y)) {
                        next.reset();
                    }
                    col += 2;
                } else {
                    col += 1;
                }
            }
        }
    }
}

/// Where to place the hardware cursor for `screen` drawn at `area`, unless hidden.
pub fn cursor_position(screen: &vt100::Screen, area: Rect) -> Option<Position> {
    if screen.hide_cursor() || screen.scrollback() > 0 {
        return None;
    }
    let (row, col) = screen.cursor_position();
    if row >= area.height || col >= area.width {
        return None;
    }
    Some(Position::new(area.x + col, area.y + row))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_text_and_attributes() {
        let mut parser = vt100::Parser::new(3, 10, 0);
        parser.process(b"ab\x1b[1;31mC\x1b[m\r\n\xe4\xbd\xa0x");
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
        TerminalView::new(parser.screen()).render(Rect::new(0, 0, 10, 3), &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "a");
        assert_eq!(buf[(1, 0)].symbol(), "b");
        assert_eq!(buf[(2, 0)].symbol(), "C");
        assert_eq!(buf[(2, 0)].fg, Color::Indexed(1));
        assert!(buf[(2, 0)].modifier.contains(Modifier::BOLD));
        assert_eq!(buf[(0, 1)].symbol(), "你");
        assert_eq!(buf[(2, 1)].symbol(), "x");
    }

    #[test]
    fn cursor_position_tracks_screen() {
        let mut parser = vt100::Parser::new(3, 10, 0);
        parser.process(b"abc");
        let area = Rect::new(5, 5, 10, 3);
        assert_eq!(
            cursor_position(parser.screen(), area),
            Some(Position::new(8, 5))
        );
        parser.process(b"\x1b[?25l");
        assert_eq!(cursor_position(parser.screen(), area), None);
    }

    #[test]
    fn clips_to_area() {
        let mut parser = vt100::Parser::new(5, 20, 0);
        parser.process(b"0123456789");
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        TerminalView::new(parser.screen()).render(Rect::new(0, 0, 4, 2), &mut buf);
        assert_eq!(buf[(3, 0)].symbol(), "3");
    }
}
