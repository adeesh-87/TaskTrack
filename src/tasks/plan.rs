//! The day plan: `<tasks_dir>/.pahiri/plans/YYYY-MM-DD.md`.
//!
//! A plain Markdown file that you, a script or an agent can edit. pahiri only
//! rewrites the list between its markers:
//!
//! ```markdown
//! # Plan 2026-09-25
//!
//! ## Plan
//! <!-- pahiri:plan -->
//! - [x] PROJ-42 · Read the spec (20m)
//! - [ ] PROJ-42 · Write the parser (1h)
//! - [ ] Email the vendor about samples (15m)
//! <!-- /pahiri:plan -->
//!
//! ## Notes
//! ```
//!
//! `<task> · <checkpoint>` items point at a checkpoint of a task (matched by
//! title); anything else is a free item. A checkpoint's done state lives in
//! the task's `CONTEXT.md`; the `[x]` here mirrors it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::checkpoints::{fmt_minutes, parse_minutes, Checkpoint};
use super::sections;

/// Plans folder inside the tasks folder (a dot-folder, so it is not a task).
pub const DIR: &str = ".pahiri/plans";
/// Start marker of the managed list.
pub const BEGIN: &str = "<!-- pahiri:plan -->";
/// End marker of the managed list.
pub const END: &str = "<!-- /pahiri:plan -->";
/// Heading above the list.
pub const HEADING: &str = "## Plan";
/// Separator between the task id and the checkpoint title.
pub const SEP: &str = " · ";

/// One planned piece of work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanItem {
    /// Task the checkpoint belongs to; `None` for a free item.
    pub task: Option<String>,
    /// Checkpoint title, or the free item's text.
    pub title: String,
    /// Estimated minutes.
    pub estimate_min: u64,
    /// Whether it is done.
    pub done: bool,
}

impl PlanItem {
    /// A checkpoint of `task`.
    pub fn checkpoint(task: &str, c: &Checkpoint) -> Self {
        Self {
            task: Some(task.to_owned()),
            title: c.title.clone(),
            estimate_min: c.estimate_min,
            done: c.done,
        }
    }

    /// Whether both point at the same work (task and title).
    pub fn same(&self, other: &Self) -> bool {
        self.task == other.task && self.title == other.title
    }

    /// Render as a checklist line.
    pub fn render(&self) -> String {
        let mark = if self.done { "x" } else { " " };
        let est = fmt_minutes(self.estimate_min);
        match &self.task {
            Some(t) => format!("- [{mark}] {t}{SEP}{} ({est})", self.title),
            None => format!("- [{mark}] {} ({est})", self.title),
        }
    }

    /// Parse one checklist line; `None` when it is not a plan item.
    pub fn parse(line: &str) -> Option<Self> {
        let c = Checkpoint::parse(line)?;
        let (task, title) = match c.title.split_once(SEP) {
            Some((t, rest))
                if !t.is_empty() && !t.contains(char::is_whitespace) && !rest.trim().is_empty() =>
            {
                (Some(t.to_owned()), rest.trim().to_owned())
            }
            _ => (None, c.title),
        };
        Some(Self {
            task,
            title,
            estimate_min: c.estimate_min,
            done: c.done,
        })
    }

    /// A free item from typed text: `Email the vendor 15m` or `… (15m)`;
    /// without an estimate it gets `default_min`.
    pub fn free(text: &str, default_min: u64) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        if let Some(item) = Self::parse(&format!("- [ ] {text}")) {
            return Some(Self { task: None, ..item });
        }
        let (title, estimate_min) = match text.rsplit_once(' ') {
            Some((head, last)) if !head.trim().is_empty() => match parse_minutes(last) {
                Some(m) if last.chars().any(|c| c.is_ascii_alphabetic()) => {
                    (head.trim().to_owned(), m)
                }
                _ => (text.to_owned(), default_min),
            },
            _ => (text.to_owned(), default_min),
        };
        Some(Self {
            task: None,
            title,
            estimate_min,
            done: false,
        })
    }
}

/// The plans folder.
pub fn dir(tasks_dir: &Path) -> PathBuf {
    tasks_dir.join(DIR)
}

/// The plan file for `date` (`YYYY-MM-DD`).
pub fn path(tasks_dir: &Path, date: &str) -> PathBuf {
    dir(tasks_dir).join(format!("{date}.md"))
}

/// The items of a plan file (missing file → none).
pub fn read(path: &Path) -> io::Result<Vec<PlanItem>> {
    match fs::read_to_string(path) {
        Ok(t) => Ok(parse(&t)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// The items between the markers of a plan file's text.
pub fn parse(markdown: &str) -> Vec<PlanItem> {
    sections::extract(markdown, BEGIN, END).map_or_else(Vec::new, |s| {
        s.lines().filter_map(PlanItem::parse).collect()
    })
}

/// Write `items` into the plan file for `date`, keeping everything else in it.
pub fn write(path: &Path, date: &str, items: &[PlanItem]) -> io::Result<()> {
    let existing = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            format!("# Plan {date}\n\n{HEADING}\n{BEGIN}\n{END}\n\n## Notes\n\n")
        }
        Err(e) => return Err(e),
    };
    let body: String = items.iter().map(|i| i.render() + "\n").collect();
    let text = sections::upsert(&existing, HEADING, BEGIN, END, &body, &["## Notes"]);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)
}

/// The latest plan before `date`, as (date, items).
pub fn previous(tasks_dir: &Path, date: &str) -> Option<(String, Vec<PlanItem>)> {
    let latest = fs::read_dir(dir(tasks_dir))
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let day = name.strip_suffix(".md")?.to_owned();
            (is_date(&day) && day.as_str() < date).then_some(day)
        })
        .max()?;
    let items = read(&path(tasks_dir, &latest)).ok()?;
    Some((latest, items))
}

fn is_date(s: &str) -> bool {
    s.len() == 10
        && s.bytes().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                b == b'-'
            } else {
                b.is_ascii_digit()
            }
        })
}

/// Local calendar date (`YYYY-MM-DD`) of epoch seconds `secs` at UTC offset `offset`.
pub fn local_date(secs: u64, offset: i64) -> String {
    let local = (secs as i64 + offset).max(0) as u64;
    crate::time::format_rfc3339(local)[..10].to_owned()
}

/// `2026-09-25` → `Fri 25 Sep`.
pub fn day_label(date: &str) -> String {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let parts: Vec<i64> = date.split('-').filter_map(|p| p.parse().ok()).collect();
    let [y, m, d] = parts[..] else {
        return date.to_owned();
    };
    if !(1..=12).contains(&m) {
        return date.to_owned();
    }
    let days = crate::time::days_from_civil(y, m, d);
    let weekday = DAYS[(days + 4).rem_euclid(7) as usize];
    format!("{weekday} {d} {}", MONTHS[(m - 1) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_round_trip() {
        let a = PlanItem::parse("- [x] PROJ-42 · Write the parser (1h)").unwrap();
        assert_eq!(a.task.as_deref(), Some("PROJ-42"));
        assert_eq!(a.title, "Write the parser");
        assert_eq!(a.estimate_min, 60);
        assert!(a.done);
        assert_eq!(a.render(), "- [x] PROJ-42 · Write the parser (1h)");
        let free = PlanItem::parse("- [ ] Email the vendor · samples (15m)").unwrap();
        assert_eq!(free.task, None, "a phrase before · is not a task id");
        assert_eq!(free.title, "Email the vendor · samples");
        assert_eq!(PlanItem::parse("- [ ] no estimate"), None);
    }

    #[test]
    fn free_items_from_typed_text() {
        let a = PlanItem::free("Email the vendor 15m", 30).unwrap();
        assert_eq!((a.title.as_str(), a.estimate_min), ("Email the vendor", 15));
        let b = PlanItem::free("Fill in timesheet (10m)", 30).unwrap();
        assert_eq!(
            (b.title.as_str(), b.estimate_min),
            ("Fill in timesheet", 10)
        );
        let c = PlanItem::free("Call Alice back", 30).unwrap();
        assert_eq!((c.title.as_str(), c.estimate_min), ("Call Alice back", 30));
        let d = PlanItem::free("Buy 2 boards", 30).unwrap();
        assert_eq!(d.title, "Buy 2 boards", "a bare number is not an estimate");
        assert_eq!(PlanItem::free("  ", 30), None);
    }

    #[test]
    fn write_keeps_notes_and_previous_finds_the_last_plan() {
        let tmp = tempfile::tempdir().unwrap();
        let tasks = tmp.path();
        let p = path(tasks, "2026-09-24");
        let items = vec![
            PlanItem::free("One (10m)", 5).unwrap(),
            PlanItem {
                task: Some("T-1".into()),
                title: "Two".into(),
                estimate_min: 30,
                done: true,
            },
        ];
        write(&p, "2026-09-24", &items).unwrap();
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("# Plan 2026-09-24\n"), "{text}");
        fs::write(&p, text.replace("## Notes\n", "## Notes\nmy own words\n")).unwrap();
        write(&p, "2026-09-24", &items[1..]).unwrap();
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.contains("my own words"), "{text}");
        assert_eq!(read(&p).unwrap(), items[1..].to_vec());
        write(&path(tasks, "2026-09-20"), "2026-09-20", &items).unwrap();
        fs::write(dir(tasks).join("notes.md"), "x").unwrap();
        let (day, prev) = previous(tasks, "2026-09-25").unwrap();
        assert_eq!(day, "2026-09-24");
        assert_eq!(prev.len(), 1);
        assert_eq!(previous(tasks, "2026-09-20"), None);
        assert!(read(&path(tasks, "2030-01-01")).unwrap().is_empty());
    }

    #[test]
    fn dates() {
        assert_eq!(local_date(1_790_000_000, 0), "2026-09-21");
        assert_eq!(local_date(1_790_000_000, 10 * 3600), "2026-09-22");
        assert_eq!(day_label("2026-09-25"), "Fri 25 Sep");
        assert_eq!(day_label("1970-01-01"), "Thu 1 Jan");
        assert_eq!(day_label("bogus"), "bogus");
    }
}
