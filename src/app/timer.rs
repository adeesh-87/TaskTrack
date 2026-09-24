//! The checkpoint timer: counts down the current checkpoint's remaining
//! estimate, keeps counting into overtime, and hands out the minutes to be
//! written to `CONTEXT.md` without ever counting a second twice.

use std::time::{Duration, Instant};

/// A running or paused timer.
#[derive(Debug, Clone)]
pub struct Timer {
    /// Task the time is booked on.
    pub task_id: String,
    /// Checkpoint title, or `None` for a plain focus block.
    pub checkpoint: Option<String>,
    /// Time budget for this run.
    pub budget: Duration,
    running_since: Option<Instant>,
    banked: Duration,
    flushed: Duration,
    /// Whether the expiry alarm already fired for the current budget.
    pub expired: bool,
}

impl Timer {
    /// Start a running timer.
    pub fn start(
        task_id: impl Into<String>,
        checkpoint: Option<String>,
        budget: Duration,
        now: Instant,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            checkpoint,
            budget,
            running_since: Some(now),
            banked: Duration::ZERO,
            flushed: Duration::ZERO,
            expired: false,
        }
    }

    /// Total time counted so far.
    pub fn elapsed(&self, now: Instant) -> Duration {
        self.banked
            + self
                .running_since
                .map_or(Duration::ZERO, |s| now.saturating_duration_since(s))
    }

    /// Seconds left (negative when over time).
    pub fn remaining_secs(&self, now: Instant) -> i64 {
        self.budget.as_secs() as i64 - self.elapsed(now).as_secs() as i64
    }

    /// Whether it is counting.
    pub fn is_running(&self) -> bool {
        self.running_since.is_some()
    }

    /// Stop counting (keeps the elapsed time).
    pub fn pause(&mut self, now: Instant) {
        if let Some(s) = self.running_since.take() {
            self.banked += now.saturating_duration_since(s);
        }
    }

    /// Continue counting.
    pub fn resume(&mut self, now: Instant) {
        if self.running_since.is_none() {
            self.running_since = Some(now);
        }
    }

    /// Add minutes to the budget and re-arm the alarm.
    pub fn extend(&mut self, minutes: u64) {
        self.budget += Duration::from_secs(minutes * 60);
        self.expired = false;
    }

    /// `true` exactly once, when a running timer runs out.
    pub fn check_expiry(&mut self, now: Instant) -> bool {
        if self.expired || !self.is_running() || self.remaining_secs(now) > 0 {
            return false;
        }
        self.expired = true;
        true
    }

    /// Minutes not yet handed out. Whole minutes only, unless `last`, which
    /// rounds the remainder to the nearest minute.
    pub fn take_minutes(&mut self, now: Instant, last: bool) -> u64 {
        let pending = self.elapsed(now).saturating_sub(self.flushed).as_secs();
        let mins = if last {
            (pending + 30) / 60
        } else {
            pending / 60
        };
        self.flushed += Duration::from_secs(if last { pending } else { mins * 60 });
        mins
    }

    /// Unflushed seconds (for periodic saving).
    pub fn unflushed_secs(&self, now: Instant) -> u64 {
        self.elapsed(now).saturating_sub(self.flushed).as_secs()
    }

    /// `12:34 left`, `03:10 over`, with a `paused` prefix when paused.
    pub fn label(&self, now: Instant) -> String {
        let r = self.remaining_secs(now);
        let text = if r >= 0 {
            format!("{} left", clock(r.unsigned_abs()))
        } else {
            format!("{} over", clock(r.unsigned_abs()))
        };
        if self.is_running() {
            text
        } else {
            format!("paused · {text}")
        }
    }

    /// What is being timed.
    pub fn what(&self) -> &str {
        self.checkpoint.as_deref().unwrap_or("focus block")
    }

    /// Pretend `d` has already been counted (tests).
    #[cfg(test)]
    pub fn backdate(&mut self, d: Duration) {
        self.banked += d;
    }
}

/// `mm:ss`, or `h:mm:ss` from an hour on.
pub fn clock(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_pauses_expires_and_flushes() {
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        let mut t = Timer::start("T", Some("A".into()), Duration::from_secs(600), t0);
        assert_eq!(t.label(at(5)), "09:55 left");
        t.pause(at(100));
        assert_eq!(t.elapsed(at(500)), Duration::from_secs(100));
        assert_eq!(t.label(at(500)), "paused · 08:20 left");
        assert!(!t.check_expiry(at(10_000)), "paused timers never expire");
        t.resume(at(500));
        assert!(!t.check_expiry(at(999)));
        assert!(t.check_expiry(at(1000)));
        assert!(!t.check_expiry(at(1001)), "fires once");
        assert_eq!(t.label(at(1070)), "01:10 over");
        assert_eq!(t.take_minutes(at(1070), false), 11); // 670s
        assert_eq!(t.unflushed_secs(at(1070)), 10);
        assert_eq!(t.take_minutes(at(1100), true), 1); // 40s → rounds up
        assert_eq!(t.take_minutes(at(1100), true), 0);
        t.extend(5);
        assert!(!t.expired);
        assert_eq!(clock(3725), "1:02:05");
        assert_eq!(t.what(), "A");
    }
}
