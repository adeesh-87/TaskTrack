//! The time ledger: one line per booking in `<tasks>/timelog.tsv`.
//!
//! ```text
//! 2026-09-24T10:05:00Z<TAB>PROJ-42<TAB>7<TAB>Read the spec
//! ```
//!
//! Timestamp (UTC), task id, minutes, what was timed. Append-only, easy to
//! grep, and the source for "today / this week" and per-day reports.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

/// Ledger file name inside the tasks folder.
pub const FILE: &str = "timelog.tsv";

/// One booking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// When it was booked (epoch seconds).
    pub at: u64,
    /// Task id.
    pub task: String,
    /// Minutes.
    pub minutes: u64,
    /// Checkpoint title or "focus block".
    pub what: String,
}

/// Append a booking.
pub fn append(tasks_dir: &Path, at: &str, task: &str, minutes: u64, what: &str) -> io::Result<()> {
    let clean = |s: &str| s.replace(['\t', '\n'], " ");
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(tasks_dir.join(FILE))?;
    writeln!(f, "{at}\t{}\t{minutes}\t{}", clean(task), clean(what))
}

/// All bookings (unreadable lines are skipped).
pub fn read(tasks_dir: &Path) -> Vec<Entry> {
    let text = fs::read_to_string(tasks_dir.join(FILE)).unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            Some(Entry {
                at: crate::time::parse_rfc3339(f.next()?)?,
                task: f.next()?.to_owned(),
                minutes: f.next()?.trim().parse().ok()?,
                what: f.next().unwrap_or("").to_owned(),
            })
        })
        .collect()
}

/// Minutes per task booked at or after `since`, largest first.
pub fn per_task_since(entries: &[Entry], since: u64) -> Vec<(String, u64)> {
    let mut map: Vec<(String, u64)> = Vec::new();
    for e in entries.iter().filter(|e| e.at >= since) {
        match map.iter_mut().find(|(t, _)| *t == e.task) {
            Some((_, m)) => *m += e.minutes,
            None => map.push((e.task.clone(), e.minutes)),
        }
    }
    map.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_read_and_sum() {
        let dir = tempfile::tempdir().unwrap();
        append(dir.path(), "2026-09-24T10:00:00Z", "A", 10, "x\ty").unwrap();
        append(dir.path(), "2026-09-24T11:00:00Z", "B", 30, "z").unwrap();
        append(dir.path(), "2026-09-25T09:00:00Z", "A", 5, "w").unwrap();
        fs::write(
            dir.path().join(FILE),
            fs::read_to_string(dir.path().join(FILE)).unwrap() + "garbage\n",
        )
        .unwrap();
        let e = read(dir.path());
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].what, "x y");
        assert_eq!(
            per_task_since(&e, 0),
            vec![("B".into(), 30), ("A".into(), 15)]
        );
        let day2 = crate::time::parse_rfc3339("2026-09-25").unwrap();
        assert_eq!(per_task_since(&e, day2), vec![("A".into(), 5)]);
    }
}
