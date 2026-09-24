//! A shell running on a pseudo-terminal, parsed into a virtual screen.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::thread;

use anyhow::Context as _;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

/// Identifier of a shell within the application.
pub type ShellId = u64;

/// Events emitted by the background reader thread.
#[derive(Debug)]
pub enum PtyEvent {
    /// Bytes arrived from the child.
    Output {
        /// Owning shell.
        id: ShellId,
        /// Raw bytes (may be a partial escape sequence; the parser copes).
        data: Vec<u8>,
    },
    /// The child exited (or the PTY hit EOF).
    Exited {
        /// Owning shell.
        id: ShellId,
    },
}

/// How to launch a shell.
#[derive(Debug, Clone)]
pub struct SpawnOptions {
    /// Program to run.
    pub program: String,
    /// Arguments.
    pub args: Vec<String>,
    /// Working directory.
    pub cwd: PathBuf,
    /// Extra environment variables.
    pub env: Vec<(String, String)>,
    /// Initial rows.
    pub rows: u16,
    /// Initial columns.
    pub cols: u16,
    /// Scrollback lines to keep.
    pub scrollback: usize,
    /// Name shown in the shell list (default: the program's file name).
    pub label: Option<String>,
}

/// Records the window title set by the child via OSC 0/2.
#[derive(Debug, Default)]
struct TitleCallbacks {
    title: Option<String>,
}

impl vt100::Callbacks for TitleCallbacks {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        let t = String::from_utf8_lossy(title).trim().to_owned();
        self.title = if t.is_empty() { None } else { Some(t) };
    }
}

/// One live (or finished) shell.
pub struct PtySession {
    id: ShellId,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    parser: vt100::Parser<TitleCallbacks>,
    size: (u16, u16),
    exited: bool,
    label: String,
    pid: Option<u32>,
}

impl std::fmt::Debug for PtySession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtySession")
            .field("id", &self.id)
            .field("size", &self.size)
            .field("exited", &self.exited)
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

impl PtySession {
    /// Spawn the shell. `on_event` is invoked from a background thread for
    /// every chunk of output and once on exit.
    pub fn spawn<F>(id: ShellId, opts: &SpawnOptions, on_event: F) -> anyhow::Result<Self>
    where
        F: Fn(PtyEvent) + Send + 'static,
    {
        let rows = opts.rows.max(2);
        let cols = opts.cols.max(2);
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("opening pty")?;

        let mut cmd = CommandBuilder::new(&opts.program);
        cmd.args(&opts.args);
        cmd.cwd(&opts.cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for (k, v) in &opts.env {
            cmd.env(k, v);
        }
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("launching {}", opts.program))?;
        // Drop the slave so that EOF reaches us when the child exits.
        drop(pair.slave);

        let killer = child.clone_killer();
        let pid = child.process_id();
        let mut reader = pair
            .master
            .try_clone_reader()
            .context("cloning pty reader")?;
        let writer = pair.master.take_writer().context("taking pty writer")?;

        thread::Builder::new()
            .name(format!("pty-reader-{id}"))
            .spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => on_event(PtyEvent::Output {
                            id,
                            data: buf[..n].to_vec(),
                        }),
                    }
                }
                // Reap the child so it does not linger as a zombie.
                let _ = child.wait();
                on_event(PtyEvent::Exited { id });
            })
            .context("spawning pty reader thread")?;

        let label = opts.label.clone().unwrap_or_else(|| {
            std::path::Path::new(&opts.program).file_name().map_or_else(
                || opts.program.clone(),
                |n| n.to_string_lossy().into_owned(),
            )
        });

        Ok(Self {
            id,
            master: pair.master,
            writer,
            killer,
            parser: vt100::Parser::new_with_callbacks(
                rows,
                cols,
                opts.scrollback,
                TitleCallbacks::default(),
            ),
            size: (rows, cols),
            exited: false,
            label,
            pid,
        })
    }

    /// Process id of the child.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Current working directory of the child (Linux: `/proc/<pid>/cwd`).
    pub fn cwd(&self) -> Option<PathBuf> {
        let pid = self.pid?;
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
    }

    /// Identifier.
    pub fn id(&self) -> ShellId {
        self.id
    }

    /// Short program name, e.g. `zsh`.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Window title set by the running program, if any.
    pub fn title(&self) -> Option<&str> {
        self.parser.callbacks().title.as_deref()
    }

    /// Feed output bytes into the screen.
    pub fn process(&mut self, data: &[u8]) {
        self.parser.process(data);
    }

    /// Send bytes to the child.
    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.exited {
            return Ok(());
        }
        self.writer.write_all(bytes)?;
        self.writer.flush()
    }

    /// Resize the pty and the virtual screen when the size actually changes.
    pub fn resize(&mut self, rows: u16, cols: u16) -> anyhow::Result<()> {
        let rows = rows.max(2);
        let cols = cols.max(2);
        if self.size == (rows, cols) {
            return Ok(());
        }
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("resizing pty")?;
        self.parser.screen_mut().set_size(rows, cols);
        self.size = (rows, cols);
        Ok(())
    }

    /// Current (rows, cols).
    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    /// The virtual screen.
    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// Scroll the view into scrollback by `delta` lines (positive = older).
    pub fn scroll_by(&mut self, delta: i32) {
        let screen = self.parser.screen_mut();
        let current = i64::from(u32::try_from(screen.scrollback()).unwrap_or(u32::MAX));
        let next = (current + i64::from(delta)).max(0);
        screen.set_scrollback(usize::try_from(next).unwrap_or(0));
    }

    /// Jump back to the live view.
    pub fn scroll_to_bottom(&mut self) {
        self.parser.screen_mut().set_scrollback(0);
    }

    /// Record that the child has exited.
    pub fn mark_exited(&mut self) {
        self.exited = true;
    }

    /// Whether the child has exited.
    pub fn has_exited(&self) -> bool {
        self.exited
    }

    /// Terminate the child.
    pub fn kill(&mut self) {
        if !self.exited {
            let _ = self.killer.kill();
        }
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn opts(program: &str, args: &[&str]) -> SpawnOptions {
        SpawnOptions {
            program: program.into(),
            args: args.iter().map(|s| (*s).to_owned()).collect(),
            cwd: std::env::temp_dir(),
            env: vec![("PAHIRI_TEST".into(), "1".into())],
            rows: 5,
            cols: 40,
            scrollback: 100,
            label: None,
        }
    }

    #[test]
    fn runs_a_command_and_captures_output() {
        let (tx, rx) = mpsc::channel();
        let mut session = PtySession::spawn(
            7,
            &opts(
                "sh",
                &[
                    "-c",
                    "printf \"hi $PAHIRI_TEST\"; printf '\\033]0;named\\007'",
                ],
            ),
            move |e| {
                let _ = tx.send(e);
            },
        )
        .unwrap();
        let mut exited = false;
        while let Ok(ev) = rx.recv_timeout(Duration::from_secs(10)) {
            match ev {
                PtyEvent::Output { id, data } => {
                    assert_eq!(id, 7);
                    session.process(&data);
                }
                PtyEvent::Exited { .. } => {
                    exited = true;
                    break;
                }
            }
        }
        assert!(exited, "child should exit");
        assert!(session.screen().contents().contains("hi 1"));
        assert_eq!(session.title(), Some("named"));
        assert_eq!(session.label(), "sh");
    }

    #[test]
    fn resize_updates_screen() {
        let (tx, _rx) = mpsc::channel();
        let mut session = PtySession::spawn(1, &opts("sh", &["-c", "sleep 5"]), move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        session.resize(10, 50).unwrap();
        assert_eq!(session.screen().size(), (10, 50));
        assert_eq!(session.size(), (10, 50));
        session.kill();
    }
}
