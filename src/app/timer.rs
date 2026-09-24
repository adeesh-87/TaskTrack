//! The checkpoint timer: counts down the current checkpoint's remaining
//! estimate, keeps counting into overtime, and hands out the minutes to be
//! written to `CONTEXT.md` without ever counting a second twice.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Why a paused timer has time it did not count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Away {
    /// No key press or click for a while.
    Idle,
    /// pahiri was not running.
    Closed,
}

/// A timer as saved in `session.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedTimer {
    /// Task id.
    pub task_id: String,
    /// Checkpoint title.
    pub checkpoint: Option<String>,
    /// Checkpoint position when started.
    pub checkpoint_index: Option<usize>,
    /// Budget in seconds.
    pub budget_secs: u64,
    /// Counted seconds.
    pub elapsed_secs: u64,
    /// Seconds already booked.
    pub flushed_secs: u64,
    /// Whether it was counting when saved.
    pub running: bool,
    /// Wall clock when saved (epoch seconds).
    pub saved_at: u64,
}

/// A running or paused timer.
#[derive(Debug, Clone)]
pub struct Timer {
    /// Task the time is booked on.
    pub task_id: String,
    /// Checkpoint title, or `None` for a plain focus block.
    pub checkpoint: Option<String>,
    /// Position of the checkpoint when the timer started (titles can repeat).
    pub checkpoint_index: Option<usize>,
    /// Time budget for this run.
    pub budget: Duration,
    running_since: Option<Instant>,
    banked: Duration,
    flushed: Duration,
    /// Whether the expiry alarm already fired for the current budget.
    pub expired: bool,
    /// Whether the timer menu was opened since the alarm (stops the blinking).
    pub acknowledged: bool,
    /// Time not counted while paused by the idle check or while pahiri was closed.
    pub away: Option<(Duration, Away)>,
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
            acknowledged: false,
            away: None,
            checkpoint_index: None,
        }
    }

    /// Snapshot for `session.json`.
    pub fn save(&self, now: Instant, now_secs: u64) -> SavedTimer {
        SavedTimer {
            task_id: self.task_id.clone(),
            checkpoint: self.checkpoint.clone(),
            checkpoint_index: self.checkpoint_index,
            budget_secs: self.budget.as_secs(),
            elapsed_secs: self.elapsed(now).as_secs(),
            flushed_secs: self.flushed.as_secs(),
            running: self.is_running(),
            saved_at: now_secs,
        }
    }

    /// Rebuild a saved timer, paused. If it was running, the time pahiri was
    /// closed is offered as "away" (count it or not when resuming).
    pub fn restore(saved: &SavedTimer, now_secs: u64) -> Self {
        let away = saved.running.then(|| {
            (
                Duration::from_secs(now_secs.saturating_sub(saved.saved_at)),
                Away::Closed,
            )
        });
        Self {
            task_id: saved.task_id.clone(),
            checkpoint: saved.checkpoint.clone(),
            checkpoint_index: saved.checkpoint_index,
            budget: Duration::from_secs(saved.budget_secs),
            running_since: None,
            banked: Duration::from_secs(saved.elapsed_secs),
            flushed: Duration::from_secs(saved.flushed_secs.min(saved.elapsed_secs)),
            expired: saved.elapsed_secs >= saved.budget_secs,
            acknowledged: true,
            away: away.filter(|(d, _)| d.as_secs() >= 60),
        }
    }

    /// Pause as of `at` (earlier than now, e.g. the last key press) and note the gap.
    pub fn pause_idle(&mut self, at: Instant, now: Instant) {
        if let Some(s) = self.running_since.take() {
            let at = at.max(s);
            self.banked += at.saturating_duration_since(s);
            self.away = Some((now.saturating_duration_since(at), Away::Idle));
        }
    }

    /// Resume; `count_away` adds the time spent away to the counted time.
    pub fn resume_with(&mut self, now: Instant, count_away: bool) {
        if let Some((d, _)) = self.away.take() {
            if count_away {
                self.banked += d;
            }
        }
        self.resume(now);
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
        self.acknowledged = false;
    }

    /// Whether time is up and nobody looked at the timer yet.
    pub fn alarming(&self) -> bool {
        self.expired && !self.acknowledged
    }

    /// `true` exactly once, when a running timer runs out.
    pub fn check_expiry(&mut self, now: Instant) -> bool {
        if self.expired || !self.is_running() || self.remaining_secs(now) > 0 {
            return false;
        }
        self.expired = true;
        self.acknowledged = false;
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
        match (self.is_running(), self.away) {
            (true, _) => text,
            (false, Some((d, Away::Idle))) => format!("idle {} · {text}", clock(d.as_secs())),
            (false, Some((d, Away::Closed))) => format!("closed {} · {text}", clock(d.as_secs())),
            (false, None) => format!("paused · {text}"),
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
        // Idle pause at the last input, then resume counting the gap or not.
        assert_eq!(t.what(), "A");
        let mut t = Timer::start("T", None, Duration::from_secs(600), t0);
        t.pause_idle(at(60), at(360));
        assert_eq!(t.elapsed(at(400)), Duration::from_secs(60));
        assert_eq!(t.label(at(400)), "idle 05:00 · 09:00 left");
        t.resume_with(at(400), true);
        assert_eq!(t.elapsed(at(400)), Duration::from_secs(360));
        // Save and restore.
        let saved = t.save(at(460), 1_000);
        assert_eq!(saved.elapsed_secs, 420);
        let r = Timer::restore(&saved, 1_000 + 3_600);
        assert!(!r.is_running());
        assert_eq!(r.away, Some((Duration::from_secs(3_600), Away::Closed)));
        assert_eq!(r.elapsed(at(99_999)), Duration::from_secs(420));
        assert_eq!(clock(3725), "1:02:05");
    }
}
