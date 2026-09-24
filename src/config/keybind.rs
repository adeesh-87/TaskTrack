//! Parsing of human readable key combos such as `ctrl+b` or `ctrl+space`.

use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A single key plus modifiers, as written in the config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyCombo {
    /// The key.
    pub code: KeyCode,
    /// Required modifiers.
    pub modifiers: KeyModifiers,
}

/// Error returned when a key combo cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unrecognised key combo {0:?} (try e.g. \"ctrl+b\" or \"ctrl+space\")")]
pub struct ParseKeyError(pub String);

impl KeyCombo {
    /// Parse `"ctrl+b"`, `"alt+x"`, `"ctrl+space"`, `"f5"` etc. Case-insensitive.
    pub fn parse(text: &str) -> Result<Self, ParseKeyError> {
        let raw = text.trim();
        if raw.is_empty() {
            return Err(ParseKeyError(text.to_owned()));
        }
        let mut modifiers = KeyModifiers::NONE;
        let parts: Vec<&str> = raw.split('+').map(str::trim).collect();
        let (key, mods) = parts
            .split_last()
            .ok_or_else(|| ParseKeyError(text.to_owned()))?;
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "c" => modifiers |= KeyModifiers::CONTROL,
                "alt" | "meta" | "m" | "opt" => modifiers |= KeyModifiers::ALT,
                "shift" | "s" => modifiers |= KeyModifiers::SHIFT,
                _ => return Err(ParseKeyError(text.to_owned())),
            }
        }
        let lower = key.to_ascii_lowercase();
        let code = match lower.as_str() {
            "" => return Err(ParseKeyError(text.to_owned())),
            "space" | "spc" => KeyCode::Char(' '),
            "esc" | "escape" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "enter" | "return" => KeyCode::Enter,
            "backspace" => KeyCode::Backspace,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" | "pgup" => KeyCode::PageUp,
            "pagedown" | "pgdn" => KeyCode::PageDown,
            "insert" | "ins" => KeyCode::Insert,
            "delete" | "del" => KeyCode::Delete,
            s if s.len() > 1 && s.starts_with('f') => {
                let n: u8 = s[1..].parse().map_err(|_| ParseKeyError(text.to_owned()))?;
                if n == 0 || n > 24 {
                    return Err(ParseKeyError(text.to_owned()));
                }
                KeyCode::F(n)
            }
            s => {
                let mut chars = s.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => KeyCode::Char(c),
                    _ => return Err(ParseKeyError(text.to_owned())),
                }
            }
        };
        Ok(Self { code, modifiers })
    }

    /// Whether `event` is this combo. Shift is ignored for plain characters so that
    /// e.g. `ctrl+b` matches regardless of how the terminal reports it.
    pub fn matches(&self, event: &KeyEvent) -> bool {
        let code_matches = match (self.code, event.code) {
            (KeyCode::Char(a), KeyCode::Char(b)) => a.eq_ignore_ascii_case(&b),
            (a, b) => a == b,
        };
        let relevant = KeyModifiers::CONTROL | KeyModifiers::ALT;
        code_matches && (event.modifiers & relevant) == (self.modifiers & relevant)
    }
}

impl fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            f.write_str("ctrl+")?;
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            f.write_str("alt+")?;
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            f.write_str("shift+")?;
        }
        match self.code {
            KeyCode::Char(' ') => f.write_str("space"),
            KeyCode::Char(c) => write!(f, "{c}"),
            KeyCode::F(n) => write!(f, "f{n}"),
            other => write!(f, "{}", format!("{other:?}").to_lowercase()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ctrl_letter() {
        let k = KeyCombo::parse("Ctrl+B").unwrap();
        assert_eq!(k.code, KeyCode::Char('b'));
        assert_eq!(k.modifiers, KeyModifiers::CONTROL);
        assert!(k.matches(&KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL)));
        assert!(!k.matches(&KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE)));
        assert!(!k.matches(&KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn parses_named_keys() {
        assert_eq!(
            KeyCombo::parse("ctrl+space").unwrap().code,
            KeyCode::Char(' ')
        );
        assert_eq!(KeyCombo::parse("f5").unwrap().code, KeyCode::F(5));
        assert_eq!(
            KeyCombo::parse("alt+esc").unwrap().modifiers,
            KeyModifiers::ALT
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(KeyCombo::parse("").is_err());
        assert!(KeyCombo::parse("bogus").is_err());
        assert!(KeyCombo::parse("hyper+x").is_err());
        assert!(KeyCombo::parse("f99").is_err());
    }

    #[test]
    fn display_roundtrips() {
        for s in ["ctrl+b", "ctrl+space", "alt+f3", "ctrl+alt+x"] {
            let k = KeyCombo::parse(s).unwrap();
            assert_eq!(KeyCombo::parse(&k.to_string()).unwrap(), k, "{s}");
        }
    }
}
