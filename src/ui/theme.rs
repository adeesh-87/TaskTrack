//! Colour palettes.

use ratatui::style::{Color, Modifier, Style};

use crate::config::ColorScheme;
use crate::highlight::Kind;

/// Colours for syntax highlighting.
#[derive(Debug, Clone, Copy)]
pub struct Syntax {
    /// Keywords.
    pub keyword: Color,
    /// Types.
    pub type_: Color,
    /// Constants.
    pub constant: Color,
    /// Strings.
    pub string: Color,
    /// Comments.
    pub comment: Color,
    /// Numbers.
    pub number: Color,
    /// Preprocessor.
    pub preprocessor: Color,
    /// Functions.
    pub function: Color,
    /// Variables.
    pub variable: Color,
    /// Attributes / lifetimes / decorators.
    pub attribute: Color,
    /// Operators.
    pub operator: Color,
    /// Headings and labels.
    pub heading: Color,
    /// Log: error.
    pub error: Color,
    /// Log: warning.
    pub warn: Color,
    /// Log: info.
    pub info: Color,
    /// Log: debug.
    pub debug: Color,
    /// Log: timestamp.
    pub timestamp: Color,
}

impl Syntax {
    /// Style for a token kind (`None` for plain text).
    pub fn style(&self, kind: Kind) -> Option<Style> {
        let s = match kind {
            Kind::Text => return None,
            Kind::Keyword => Style::new().fg(self.keyword),
            Kind::Type => Style::new().fg(self.type_),
            Kind::Constant => Style::new().fg(self.constant),
            Kind::String => Style::new().fg(self.string),
            Kind::Comment => Style::new().fg(self.comment).add_modifier(Modifier::ITALIC),
            Kind::Number => Style::new().fg(self.number),
            Kind::Preprocessor => Style::new().fg(self.preprocessor),
            Kind::Function => Style::new().fg(self.function),
            Kind::Variable => Style::new().fg(self.variable),
            Kind::Attribute => Style::new().fg(self.attribute),
            Kind::Operator => Style::new().fg(self.operator),
            Kind::Heading => Style::new().fg(self.heading).add_modifier(Modifier::BOLD),
            Kind::Label => Style::new().fg(self.heading),
            Kind::Error => Style::new().fg(self.error).add_modifier(Modifier::BOLD),
            Kind::Warn => Style::new().fg(self.warn),
            Kind::Info => Style::new().fg(self.info),
            Kind::Debug => Style::new().fg(self.debug),
            Kind::Timestamp => Style::new().fg(self.timestamp),
        };
        Some(s)
    }
}

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
    /// Syntax colours.
    pub syntax: Syntax,
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
                syntax: Syntax {
                    keyword: Color::Magenta,
                    type_: Color::Yellow,
                    constant: Color::LightRed,
                    string: Color::Green,
                    comment: Color::DarkGray,
                    number: Color::LightCyan,
                    preprocessor: Color::LightMagenta,
                    function: Color::LightBlue,
                    variable: Color::Cyan,
                    attribute: Color::LightMagenta,
                    operator: Color::Gray,
                    heading: Color::LightYellow,
                    error: Color::Red,
                    warn: Color::Yellow,
                    info: Color::Green,
                    debug: Color::DarkGray,
                    timestamp: Color::Blue,
                },
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
                syntax: Syntax {
                    keyword: Color::Rgb(160, 30, 140),
                    type_: Color::Rgb(150, 90, 0),
                    constant: Color::Rgb(170, 40, 40),
                    string: Color::Rgb(0, 120, 60),
                    comment: Color::Rgb(130, 130, 130),
                    number: Color::Rgb(0, 110, 130),
                    preprocessor: Color::Rgb(120, 40, 160),
                    function: Color::Rgb(0, 80, 180),
                    variable: Color::Rgb(0, 110, 130),
                    attribute: Color::Rgb(120, 40, 160),
                    operator: Color::Rgb(90, 90, 90),
                    heading: Color::Rgb(150, 90, 0),
                    error: Color::Rgb(190, 30, 30),
                    warn: Color::Rgb(170, 110, 0),
                    info: Color::Rgb(0, 120, 60),
                    debug: Color::Rgb(130, 130, 130),
                    timestamp: Color::Rgb(0, 80, 180),
                },
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
                syntax: Syntax {
                    keyword: Color::Rgb(251, 73, 52),
                    type_: Color::Rgb(250, 189, 47),
                    constant: Color::Rgb(211, 134, 155),
                    string: Color::Rgb(184, 187, 38),
                    comment: Color::Rgb(146, 131, 116),
                    number: Color::Rgb(211, 134, 155),
                    preprocessor: Color::Rgb(142, 192, 124),
                    function: Color::Rgb(184, 187, 38),
                    variable: Color::Rgb(131, 165, 152),
                    attribute: Color::Rgb(142, 192, 124),
                    operator: Color::Rgb(235, 219, 178),
                    heading: Color::Rgb(250, 189, 47),
                    error: Color::Rgb(251, 73, 52),
                    warn: Color::Rgb(254, 128, 25),
                    info: Color::Rgb(184, 187, 38),
                    debug: Color::Rgb(146, 131, 116),
                    timestamp: Color::Rgb(131, 165, 152),
                },
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
                syntax: Syntax {
                    keyword: Color::Rgb(129, 161, 193),
                    type_: Color::Rgb(143, 188, 187),
                    constant: Color::Rgb(180, 142, 173),
                    string: Color::Rgb(163, 190, 140),
                    comment: Color::Rgb(97, 110, 136),
                    number: Color::Rgb(180, 142, 173),
                    preprocessor: Color::Rgb(94, 129, 172),
                    function: Color::Rgb(136, 192, 208),
                    variable: Color::Rgb(208, 135, 112),
                    attribute: Color::Rgb(94, 129, 172),
                    operator: Color::Rgb(216, 222, 233),
                    heading: Color::Rgb(235, 203, 139),
                    error: Color::Rgb(191, 97, 106),
                    warn: Color::Rgb(235, 203, 139),
                    info: Color::Rgb(163, 190, 140),
                    debug: Color::Rgb(97, 110, 136),
                    timestamp: Color::Rgb(129, 161, 193),
                },
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
                syntax: Syntax {
                    keyword: Color::Rgb(133, 153, 0),
                    type_: Color::Rgb(181, 137, 0),
                    constant: Color::Rgb(203, 75, 22),
                    string: Color::Rgb(42, 161, 152),
                    comment: Color::Rgb(88, 110, 117),
                    number: Color::Rgb(211, 54, 130),
                    preprocessor: Color::Rgb(203, 75, 22),
                    function: Color::Rgb(38, 139, 210),
                    variable: Color::Rgb(108, 113, 196),
                    attribute: Color::Rgb(203, 75, 22),
                    operator: Color::Rgb(147, 161, 161),
                    heading: Color::Rgb(181, 137, 0),
                    error: Color::Rgb(220, 50, 47),
                    warn: Color::Rgb(203, 75, 22),
                    info: Color::Rgb(133, 153, 0),
                    debug: Color::Rgb(88, 110, 117),
                    timestamp: Color::Rgb(38, 139, 210),
                },
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
