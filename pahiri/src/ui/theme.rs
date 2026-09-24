//! Colour palettes.

use ratatui::style::{Color, Modifier, Style};

use crate::config::ColorScheme;

/// Colours used by the UI chrome.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// Page background.
    pub bg: Color,
    /// Default text.
    pub fg: Color,
    /// De-emphasised text.
    pub muted: Color,
    /// Highlights and titles.
    pub accent: Color,
    /// Unfocused borders.
    pub border: Color,
    /// Focused borders.
    pub border_focus: Color,
    /// Category headers.
    pub header: Color,
    /// Selection background.
    pub selected_bg: Color,
    /// Selection foreground.
    pub selected_fg: Color,
    /// Warning text.
    pub warning: Color,
    /// Error text.
    pub error: Color,
    /// Status bar background.
    pub bar_bg: Color,
}

impl Theme {
    /// Palette for a scheme.
    pub fn for_scheme(scheme: ColorScheme) -> Self {
        match scheme {
            ColorScheme::Dark => Self {
                bg: Color::Reset,
                fg: Color::Reset,
                muted: Color::DarkGray,
                accent: Color::Cyan,
                border: Color::DarkGray,
                border_focus: Color::Cyan,
                header: Color::Yellow,
                selected_bg: Color::Rgb(50, 60, 80),
                selected_fg: Color::White,
                warning: Color::Yellow,
                error: Color::Red,
                bar_bg: Color::Rgb(30, 30, 38),
            },
            ColorScheme::Light => Self {
                bg: Color::Rgb(250, 250, 247),
                fg: Color::Rgb(40, 40, 40),
                muted: Color::Rgb(130, 130, 130),
                accent: Color::Rgb(0, 100, 170),
                border: Color::Rgb(190, 190, 190),
                border_focus: Color::Rgb(0, 100, 170),
                header: Color::Rgb(150, 90, 0),
                selected_bg: Color::Rgb(215, 227, 245),
                selected_fg: Color::Rgb(20, 20, 20),
                warning: Color::Rgb(170, 110, 0),
                error: Color::Rgb(190, 30, 30),
                bar_bg: Color::Rgb(230, 230, 226),
            },
            ColorScheme::Gruvbox => Self {
                bg: Color::Rgb(40, 40, 40),
                fg: Color::Rgb(235, 219, 178),
                muted: Color::Rgb(146, 131, 116),
                accent: Color::Rgb(250, 189, 47),
                border: Color::Rgb(80, 73, 69),
                border_focus: Color::Rgb(250, 189, 47),
                header: Color::Rgb(184, 187, 38),
                selected_bg: Color::Rgb(80, 73, 69),
                selected_fg: Color::Rgb(251, 241, 199),
                warning: Color::Rgb(254, 128, 25),
                error: Color::Rgb(251, 73, 52),
                bar_bg: Color::Rgb(29, 32, 33),
            },
            ColorScheme::Nord => Self {
                bg: Color::Rgb(46, 52, 64),
                fg: Color::Rgb(216, 222, 233),
                muted: Color::Rgb(76, 86, 106),
                accent: Color::Rgb(136, 192, 208),
                border: Color::Rgb(67, 76, 94),
                border_focus: Color::Rgb(136, 192, 208),
                header: Color::Rgb(235, 203, 139),
                selected_bg: Color::Rgb(67, 76, 94),
                selected_fg: Color::Rgb(236, 239, 244),
                warning: Color::Rgb(235, 203, 139),
                error: Color::Rgb(191, 97, 106),
                bar_bg: Color::Rgb(36, 41, 51),
            },
            ColorScheme::Solarized => Self {
                bg: Color::Rgb(0, 43, 54),
                fg: Color::Rgb(147, 161, 161),
                muted: Color::Rgb(88, 110, 117),
                accent: Color::Rgb(42, 161, 152),
                border: Color::Rgb(7, 54, 66),
                border_focus: Color::Rgb(42, 161, 152),
                header: Color::Rgb(181, 137, 0),
                selected_bg: Color::Rgb(7, 54, 66),
                selected_fg: Color::Rgb(238, 232, 213),
                warning: Color::Rgb(203, 75, 22),
                error: Color::Rgb(220, 50, 47),
                bar_bg: Color::Rgb(0, 33, 43),
            },
        }
    }

    /// Border style depending on focus.
    pub fn border(&self, focused: bool) -> Style {
        if focused {
            Style::new()
                .fg(self.border_focus)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(self.border)
        }
    }

    /// Style for the selected row of a list.
    pub fn selected(&self, focused: bool) -> Style {
        if focused {
            Style::new()
                .bg(self.selected_bg)
                .fg(self.selected_fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(self.accent)
        }
    }
}
