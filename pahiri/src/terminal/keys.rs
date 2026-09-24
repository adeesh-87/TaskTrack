//! Translate crossterm key events into the byte sequences a terminal sends.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// xterm-style modifier parameter: 1 + shift(1) + alt(2) + ctrl(4).
fn modifier_param(mods: KeyModifiers) -> u8 {
    let mut m = 1;
    if mods.contains(KeyModifiers::SHIFT) {
        m += 1;
    }
    if mods.contains(KeyModifiers::ALT) {
        m += 2;
    }
    if mods.contains(KeyModifiers::CONTROL) {
        m += 4;
    }
    m
}

fn cursor_key(letter: char, mods: KeyModifiers, app_cursor: bool) -> Vec<u8> {
    let m = modifier_param(mods);
    if m > 1 {
        format!("\x1b[1;{m}{letter}").into_bytes()
    } else if app_cursor {
        format!("\x1bO{letter}").into_bytes()
    } else {
        format!("\x1b[{letter}").into_bytes()
    }
}

fn tilde_key(code: u8, mods: KeyModifiers) -> Vec<u8> {
    let m = modifier_param(mods);
    if m > 1 {
        format!("\x1b[{code};{m}~").into_bytes()
    } else {
        format!("\x1b[{code}~").into_bytes()
    }
}

fn control_char(c: char) -> Option<u8> {
    match c {
        'a'..='z' => Some(c as u8 - b'a' + 1),
        'A'..='Z' => Some(c as u8 - b'A' + 1),
        ' ' | '@' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '7' | '-' | '/' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

/// Encode a key press for the child terminal. Returns `None` for key releases
/// and keys that have no terminal representation.
pub fn encode_key(key: &KeyEvent, app_cursor: bool) -> Option<Vec<u8>> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let mods = key.modifiers;
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    let alt = mods.contains(KeyModifiers::ALT);
    let bytes = match key.code {
        KeyCode::Char(c) => {
            let mut out = Vec::new();
            if alt {
                out.push(0x1b);
            }
            if ctrl {
                match control_char(c) {
                    Some(b) => out.push(b),
                    None => out.extend(c.to_string().into_bytes()),
                }
            } else {
                out.extend(c.to_string().into_bytes());
            }
            out
        }
        KeyCode::Enter => {
            if alt {
                vec![0x1b, b'\r']
            } else {
                vec![b'\r']
            }
        }
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => {
            let b = if ctrl { 0x08 } else { 0x7f };
            if alt {
                vec![0x1b, b]
            } else {
                vec![b]
            }
        }
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => cursor_key('A', mods, app_cursor),
        KeyCode::Down => cursor_key('B', mods, app_cursor),
        KeyCode::Right => cursor_key('C', mods, app_cursor),
        KeyCode::Left => cursor_key('D', mods, app_cursor),
        KeyCode::Home => cursor_key('H', mods, app_cursor),
        KeyCode::End => cursor_key('F', mods, app_cursor),
        KeyCode::Insert => tilde_key(2, mods),
        KeyCode::Delete => tilde_key(3, mods),
        KeyCode::PageUp => tilde_key(5, mods),
        KeyCode::PageDown => tilde_key(6, mods),
        KeyCode::F(n @ 1..=4) => {
            let letter = (b'P' + n - 1) as char;
            let m = modifier_param(mods);
            if m > 1 {
                format!("\x1b[1;{m}{letter}").into_bytes()
            } else {
                format!("\x1bO{letter}").into_bytes()
            }
        }
        KeyCode::F(n @ 5..=12) => {
            let code = match n {
                5 => 15,
                6 => 17,
                7 => 18,
                8 => 19,
                9 => 20,
                10 => 21,
                11 => 23,
                _ => 24,
            };
            tilde_key(code, mods)
        }
        _ => return None,
    };
    Some(bytes)
}

/// Wrap pasted text in bracketed-paste markers when the application asked for them.
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    let body = text.replace('\n', "\r");
    if bracketed {
        let mut out = b"\x1b[200~".to_vec();
        out.extend(body.into_bytes());
        out.extend(b"\x1b[201~");
        out
    } else {
        body.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn plain_and_control_chars() {
        assert_eq!(
            encode_key(&key(KeyCode::Char('a'), KeyModifiers::NONE), false),
            Some(b"a".to_vec())
        );
        assert_eq!(
            encode_key(&key(KeyCode::Char('c'), KeyModifiers::CONTROL), false),
            Some(vec![3])
        );
        assert_eq!(
            encode_key(&key(KeyCode::Char('x'), KeyModifiers::ALT), false),
            Some(vec![0x1b, b'x'])
        );
        assert_eq!(
            encode_key(&key(KeyCode::Char(' '), KeyModifiers::CONTROL), false),
            Some(vec![0])
        );
        assert_eq!(
            encode_key(&key(KeyCode::Char('é'), KeyModifiers::NONE), false),
            Some("é".as_bytes().to_vec())
        );
    }

    #[test]
    fn cursor_keys_respect_application_mode() {
        assert_eq!(
            encode_key(&key(KeyCode::Up, KeyModifiers::NONE), false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            encode_key(&key(KeyCode::Up, KeyModifiers::NONE), true),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            encode_key(&key(KeyCode::Left, KeyModifiers::CONTROL), true),
            Some(b"\x1b[1;5D".to_vec())
        );
        assert_eq!(
            encode_key(
                &key(KeyCode::Right, KeyModifiers::SHIFT | KeyModifiers::ALT),
                false
            ),
            Some(b"\x1b[1;4C".to_vec())
        );
    }

    #[test]
    fn special_keys() {
        assert_eq!(
            encode_key(&key(KeyCode::Enter, KeyModifiers::NONE), false),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            encode_key(&key(KeyCode::Backspace, KeyModifiers::NONE), false),
            Some(vec![0x7f])
        );
        assert_eq!(
            encode_key(&key(KeyCode::Delete, KeyModifiers::NONE), false),
            Some(b"\x1b[3~".to_vec())
        );
        assert_eq!(
            encode_key(&key(KeyCode::PageUp, KeyModifiers::SHIFT), false),
            Some(b"\x1b[5;2~".to_vec())
        );
        assert_eq!(
            encode_key(&key(KeyCode::F(1), KeyModifiers::NONE), false),
            Some(b"\x1bOP".to_vec())
        );
        assert_eq!(
            encode_key(&key(KeyCode::F(5), KeyModifiers::NONE), false),
            Some(b"\x1b[15~".to_vec())
        );
        assert_eq!(
            encode_key(&key(KeyCode::F(12), KeyModifiers::NONE), false),
            Some(b"\x1b[24~".to_vec())
        );
        assert_eq!(
            encode_key(&key(KeyCode::Esc, KeyModifiers::NONE), false),
            Some(vec![0x1b])
        );
        assert_eq!(
            encode_key(&key(KeyCode::BackTab, KeyModifiers::SHIFT), false),
            Some(b"\x1b[Z".to_vec())
        );
    }

    #[test]
    fn releases_and_unknown_keys_are_ignored() {
        let mut k = key(KeyCode::Char('a'), KeyModifiers::NONE);
        k.kind = KeyEventKind::Release;
        assert_eq!(encode_key(&k, false), None);
        assert_eq!(
            encode_key(&key(KeyCode::Null, KeyModifiers::NONE), false),
            None
        );
    }

    #[test]
    fn paste_encoding() {
        assert_eq!(encode_paste("a\nb", false), b"a\rb".to_vec());
        assert_eq!(encode_paste("x", true), b"\x1b[200~x\x1b[201~".to_vec());
    }
}
