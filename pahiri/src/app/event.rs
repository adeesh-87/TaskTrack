//! Events flowing into the application from input and PTY threads.

use std::sync::mpsc;

use crate::terminal::PtyEvent;

/// Everything the main loop reacts to.
#[derive(Debug)]
pub enum AppEvent {
    /// A terminal input event.
    Input(crossterm::event::Event),
    /// Output or exit from a shell.
    Pty(PtyEvent),
    /// Periodic tick (used to redraw when nothing else happens).
    Tick,
}

/// Cloneable sender handed to background threads.
#[derive(Debug, Clone)]
pub struct EventSender(mpsc::Sender<AppEvent>);

impl EventSender {
    /// Create a channel pair.
    pub fn channel() -> (Self, mpsc::Receiver<AppEvent>) {
        let (tx, rx) = mpsc::channel();
        (Self(tx), rx)
    }

    /// Send an event (ignored if the receiver is gone).
    pub fn send(&self, event: AppEvent) {
        let _ = self.0.send(event);
    }
}
