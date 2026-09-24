//! Copy and paste for the editor: pahiri keeps the last copied text itself and
//! also hands it to the system clipboard, through `copy_command` or OSC 52.

use std::io::{Read as _, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use super::App;

/// Largest copy sent to the terminal with OSC 52 (many terminals cap it).
const OSC52_MAX: usize = 512 * 1024;

/// How long `paste_command` may take.
const PASTE_TIMEOUT: Duration = Duration::from_secs(3);

impl App {
    /// Remember `text` as the clipboard and send it to the system clipboard.
    pub(super) fn copy_to_clipboard(&mut self, text: String) {
        let command = self.config.copy_command.trim().to_owned();
        let failed = if command.is_empty() && text.len() > OSC52_MAX {
            Some("too big for the terminal clipboard (set Copy command)".to_owned())
        } else if command.is_empty() {
            self.terminal_out.extend_from_slice(osc52(&text).as_bytes());
            None
        } else {
            pipe_to(&command, text.clone(), &self.clipboard_cwd())
                .err()
                .map(|e| format!("copy command failed: {e}"))
        };
        let lines = text.lines().count().max(1);
        let what = if lines > 1 {
            format!("{lines} lines")
        } else {
            format!("{} character(s)", text.chars().count())
        };
        self.set_status(match failed {
            None => format!("copied {what}"),
            Some(e) => format!("copied {what} inside pahiri only: {e}"),
        });
        self.clipboard = text;
    }

    /// Text for Ctrl+V: `paste_command`'s output, else the last copy.
    pub(super) fn clipboard_text(&self) -> Result<String, String> {
        let command = self.config.paste_command.trim();
        if command.is_empty() {
            return Ok(self.clipboard.clone());
        }
        read_from(command, &self.clipboard_cwd()).map_err(|e| format!("paste command failed: {e}"))
    }

    /// Clipboard commands run in the tasks folder.
    fn clipboard_cwd(&self) -> std::path::PathBuf {
        let dir = &self.config.tasks_dir;
        if dir.is_dir() {
            dir.clone()
        } else {
            std::env::temp_dir()
        }
    }

    /// Bytes (OSC 52) the main loop writes to the real terminal after drawing.
    pub fn take_terminal_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.terminal_out)
    }
}

/// Start `command` with `text` on stdin without waiting for it: clipboard
/// tools such as `xclip` keep running to serve the selection.
fn pipe_to(command: &str, text: String, cwd: &Path) -> std::io::Result<()> {
    let mut child = Command::new("sh")
        .args(["-c", command])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let stdin = child.stdin.take();
    std::thread::spawn(move || {
        if let Some(mut stdin) = stdin {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    });
    Ok(())
}

/// Run `command` and return its exact stdout, giving up after [`PASTE_TIMEOUT`].
fn read_from(command: &str, cwd: &Path) -> Result<String, String> {
    let mut child = Command::new("sh")
        .args(["-c", command])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = tx.send(stdout.read_to_end(&mut out).map(|_| out));
    });
    let result = rx.recv_timeout(PASTE_TIMEOUT);
    if result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    match result {
        Ok(Ok(bytes)) if status.success() => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        Ok(Ok(_)) => Err(format!("exited with {status}")),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err(format!("no answer in {}s", PASTE_TIMEOUT.as_secs())),
    }
}

/// The OSC 52 "set clipboard" sequence for `text`.
fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ABC[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64("héllo\n".as_bytes()), "aMOpbGxvCg==");
        assert_eq!(osc52("hi"), "\x1b]52;c;aGk=\x07");
    }
}
