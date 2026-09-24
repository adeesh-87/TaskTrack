//! Encode mouse events for programs that enabled mouse reporting.

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use vt100::{MouseProtocolEncoding, MouseProtocolMode};

/// Encode a mouse event at pane-relative (0-based) `x`, `y`.
/// Returns `None` when the program did not ask for this kind of event.
pub fn encode_mouse(
    event: &MouseEvent,
    x: u16,
    y: u16,
    mode: MouseProtocolMode,
    encoding: MouseProtocolEncoding,
) -> Option<Vec<u8>> {
    let (button, release, motion) = match event.kind {
        MouseEventKind::Down(b) => (button_code(b), false, false),
        MouseEventKind::Up(b) => (button_code(b), true, false),
        MouseEventKind::Drag(b) => (button_code(b), false, true),
        MouseEventKind::Moved => (3, false, true),
        MouseEventKind::ScrollUp => (64, false, false),
        MouseEventKind::ScrollDown => (65, false, false),
        MouseEventKind::ScrollLeft => (66, false, false),
        MouseEventKind::ScrollRight => (67, false, false),
    };
    let wanted = match mode {
        MouseProtocolMode::None => false,
        MouseProtocolMode::Press => !release && !motion,
        MouseProtocolMode::PressRelease => !motion,
        MouseProtocolMode::ButtonMotion => !(motion && event.kind == MouseEventKind::Moved),
        MouseProtocolMode::AnyMotion => true,
    };
    if !wanted {
        return None;
    }
    let mut code = button;
    if motion {
        code += 32;
    }
    if event.modifiers.contains(KeyModifiers::SHIFT) {
        code += 4;
    }
    if event.modifiers.contains(KeyModifiers::ALT) {
        code += 8;
    }
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        code += 16;
    }
    let (col, row) = (u32::from(x) + 1, u32::from(y) + 1);
    Some(match encoding {
        MouseProtocolEncoding::Sgr => format!(
            "\x1b[<{code};{col};{row}{}",
            if release { 'm' } else { 'M' }
        )
        .into_bytes(),
        MouseProtocolEncoding::Default => {
            let code = if release { 3 } else { code };
            let mut out = b"\x1b[M".to_vec();
            out.push((32 + code).min(255) as u8);
            out.push((32 + col).min(255) as u8);
            out.push((32 + row).min(255) as u8);
            out
        }
        MouseProtocolEncoding::Utf8 => {
            let code = if release { 3 } else { code };
            let mut out = String::from("\x1b[M");
            for v in [32 + code, 32 + col, 32 + row] {
                out.push(char::from_u32(v).unwrap_or(' '));
            }
            out.into_bytes()
        }
    })
}

fn button_code(b: MouseButton) -> u32 {
    match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: MouseEventKind, modifiers: KeyModifiers) -> MouseEvent {
        MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers,
        }
    }

    #[test]
    fn sgr_encoding() {
        let e = ev(MouseEventKind::Down(MouseButton::Left), KeyModifiers::NONE);
        assert_eq!(
            encode_mouse(
                &e,
                4,
                2,
                MouseProtocolMode::Press,
                MouseProtocolEncoding::Sgr
            ),
            Some(b"\x1b[<0;5;3M".to_vec())
        );
        let e = ev(
            MouseEventKind::Up(MouseButton::Right),
            KeyModifiers::CONTROL,
        );
        assert_eq!(
            encode_mouse(
                &e,
                0,
                0,
                MouseProtocolMode::PressRelease,
                MouseProtocolEncoding::Sgr
            ),
            Some(b"\x1b[<18;1;1m".to_vec())
        );
        let e = ev(MouseEventKind::ScrollUp, KeyModifiers::NONE);
        assert_eq!(
            encode_mouse(
                &e,
                9,
                9,
                MouseProtocolMode::Press,
                MouseProtocolEncoding::Sgr
            ),
            Some(b"\x1b[<64;10;10M".to_vec())
        );
        let e = ev(MouseEventKind::Drag(MouseButton::Left), KeyModifiers::NONE);
        assert_eq!(
            encode_mouse(
                &e,
                1,
                1,
                MouseProtocolMode::ButtonMotion,
                MouseProtocolEncoding::Sgr
            ),
            Some(b"\x1b[<32;2;2M".to_vec())
        );
    }

    #[test]
    fn mode_filtering() {
        let up = ev(MouseEventKind::Up(MouseButton::Left), KeyModifiers::NONE);
        let moved = ev(MouseEventKind::Moved, KeyModifiers::NONE);
        let down = ev(MouseEventKind::Down(MouseButton::Left), KeyModifiers::NONE);
        assert!(encode_mouse(
            &down,
            0,
            0,
            MouseProtocolMode::None,
            MouseProtocolEncoding::Sgr
        )
        .is_none());
        assert!(encode_mouse(
            &up,
            0,
            0,
            MouseProtocolMode::Press,
            MouseProtocolEncoding::Sgr
        )
        .is_none());
        assert!(encode_mouse(
            &moved,
            0,
            0,
            MouseProtocolMode::ButtonMotion,
            MouseProtocolEncoding::Sgr
        )
        .is_none());
        assert!(encode_mouse(
            &moved,
            0,
            0,
            MouseProtocolMode::AnyMotion,
            MouseProtocolEncoding::Sgr
        )
        .is_some());
    }

    #[test]
    fn x10_encoding() {
        let e = ev(
            MouseEventKind::Down(MouseButton::Middle),
            KeyModifiers::NONE,
        );
        assert_eq!(
            encode_mouse(
                &e,
                0,
                0,
                MouseProtocolMode::Press,
                MouseProtocolEncoding::Default
            ),
            Some(vec![0x1b, b'[', b'M', 33, 33, 33])
        );
        let e = ev(MouseEventKind::Up(MouseButton::Middle), KeyModifiers::NONE);
        assert_eq!(
            encode_mouse(
                &e,
                0,
                0,
                MouseProtocolMode::PressRelease,
                MouseProtocolEncoding::Utf8
            ),
            Some(vec![0x1b, b'[', b'M', 35, 33, 33])
        );
    }
}
