//! A read-only view of one task, assembled from its folder and `CONTEXT.md`.
//!
//! Used by the "next up" dashboard and by `pahiri report`, which the review
//! skill reads instead of opening every task file.

use std::fs;
use std::path::Path;

use serde::Serialize;

use super::checkpoints::{self, Checkpoint};
use super::context::{TaskMeta, LOG_HEADING, OUTCOME_HEADING};
use super::sections::section_lines;

/// Everything worth knowing about a task at a glance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskRecord {
    /// Task id (folder name).
    pub id: String,
    /// Title from the first `# ` heading (without the `id: ` prefix).
    pub title: String,
    /// Board column.
    pub column: String,
    /// Ticket link.
    pub link: Option<String>,
    /// When the task was created.
    pub created: Option<String>,
    /// When work started.
    pub started: Option<String>,
    /// When it was finished.
    pub finished: Option<String>,
    /// Minutes recorded by the timer.
    pub time_spent_min: u64,
    /// Whether the context is marked ready.
    pub context_ready: bool,
    /// Attached workspaces.
    pub workspaces: Vec<String>,
    /// Gerrit changes as `workspace change-id url :: subject`.
    pub gerrit: Vec<String>,
    /// The same changes, with their fields (for the Home view).
    pub changes: Vec<ChangeRecord>,
    /// One line about what the task is: the goal from `## Context`, else the
    /// first line of `## Description`.
    pub summary: String,
    /// Checkpoints.
    pub checkpoints: Vec<CheckpointRecord>,
    /// `## Outcome` lines.
    pub outcome: Vec<String>,
    /// `## Log` lines.
    pub log: Vec<String>,
}

/// A Gerrit change of a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangeRecord {
    /// Workspace (or project) name.
    pub workspace: String,
    /// `Change-Id`.
    pub change_id: String,
    /// Review link.
    pub url: Option<String>,
    /// Commit subject.
    pub subject: String,
    /// Status, e.g. `MERGED #1234 CR+2`.
    pub status: Option<String>,
}

/// Longest summary kept.
const SUMMARY_CHARS: usize = 200;

/// The first line of prose in `text` (skipping headings, markers, empty and
/// bullet-only lines), without list markers, cut to [`SUMMARY_CHARS`].
fn first_prose(text: &str) -> Option<String> {
    let line = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("<!--"))
        .map(|l| {
            l.trim_start_matches(|c: char| matches!(c, '-' | '*' | '+' | '>') || c.is_whitespace())
                .trim()
        })
        .find(|l| !l.is_empty())?;
    let mut out: String = line.chars().take(SUMMARY_CHARS).collect();
    if line.chars().count() > SUMMARY_CHARS {
        out.push('…');
    }
    Some(out)
}

/// One line about the task: the `### Goal` of the AI context (else its first
/// line), else the first line of `## Description`.
pub fn summary_of(markdown: &str) -> String {
    let context = super::sections::extract(
        markdown,
        super::context::CONTEXT_BEGIN,
        super::context::CONTEXT_END,
    );
    let goal = context.and_then(|c| {
        let after = c
            .split("\n### ")
            .find(|part| part.trim_start_matches("### ").starts_with("Goal"))?;
        first_prose(after.split_once('\n').map_or("", |(_, rest)| rest))
    });
    goal.or_else(|| context.and_then(first_prose))
        .or_else(|| first_prose(&section_lines(markdown, "## Description").join("\n")))
        .unwrap_or_default()
}

/// Serializable checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckpointRecord {
    /// Title.
    pub title: String,
    /// Estimated minutes.
    pub estimate_min: u64,
    /// Spent minutes.
    pub spent_min: u64,
    /// Done.
    pub done: bool,
}

impl From<&Checkpoint> for CheckpointRecord {
    fn from(c: &Checkpoint) -> Self {
        Self {
            title: c.title.clone(),
            estimate_min: c.estimate_min,
            spent_min: c.spent_min,
            done: c.done,
        }
    }
}

fn strip_item(line: &str) -> String {
    line.trim_start_matches("- ").to_owned()
}

/// Title of a context file: the first `# ` heading, minus a leading `id: `.
pub fn title_of(markdown: &str, id: &str) -> String {
    let heading = markdown
        .lines()
        .find_map(|l| l.strip_prefix("# "))
        .unwrap_or("")
        .trim();
    heading
        .strip_prefix(id)
        .map_or(heading, |r| r.trim_start_matches(':').trim())
        .to_owned()
}

impl TaskRecord {
    /// Build a record from the context file text.
    pub fn from_markdown(id: &str, column: &str, markdown: &str) -> Self {
        let meta = TaskMeta::parse(markdown);
        let items = checkpoints::parse_section(markdown);
        Self {
            id: id.to_owned(),
            title: title_of(markdown, id),
            column: column.to_owned(),
            link: meta.link,
            created: meta.created,
            started: meta.started,
            finished: meta.finished,
            time_spent_min: meta.time_spent_min,
            context_ready: meta.context_ready,
            workspaces: meta.workspaces,
            gerrit: meta
                .gerrit
                .iter()
                .map(|g| {
                    let mut s = format!("{} {}", g.workspace, g.change_id);
                    if let Some(u) = &g.url {
                        s = format!("{s} {u}");
                    }
                    if !g.subject.is_empty() {
                        s = format!("{s} :: {}", g.subject);
                    }
                    s
                })
                .collect(),
            changes: meta
                .gerrit
                .iter()
                .map(|g| ChangeRecord {
                    workspace: g.workspace.clone(),
                    change_id: g.change_id.clone(),
                    url: g.url.clone(),
                    subject: g.subject.clone(),
                    status: g.status.clone(),
                })
                .collect(),
            summary: summary_of(markdown),
            checkpoints: items.iter().map(CheckpointRecord::from).collect(),
            outcome: section_lines(markdown, OUTCOME_HEADING)
                .into_iter()
                .map(strip_item)
                .collect(),
            log: section_lines(markdown, LOG_HEADING)
                .into_iter()
                .map(strip_item)
                .collect(),
        }
    }

    /// Read a record from disk (a missing context file gives an empty record).
    pub fn load(id: &str, column: &str, context_path: &Path) -> Self {
        let text = fs::read_to_string(context_path).unwrap_or_default();
        Self::from_markdown(id, column, &text)
    }

    /// The next open checkpoint.
    pub fn next_checkpoint(&self) -> Option<&CheckpointRecord> {
        self.checkpoints.iter().find(|c| !c.done)
    }

    /// Whether the task's span (first date → finished, or last date while
    /// unfinished) overlaps `[from, to]` (inclusive `YYYY-MM-DD` bounds).
    pub fn active_between(&self, from: Option<&str>, to: Option<&str>) -> bool {
        let day = |s: &str| s.get(..10).unwrap_or(s).to_owned();
        let mut dates: Vec<String> = [&self.created, &self.started, &self.finished]
            .into_iter()
            .flatten()
            .map(|s| day(s))
            .chain(self.log.iter().map(|l| day(l)))
            .filter(|d| d.len() == 10 && d.as_bytes()[4] == b'-')
            .collect();
        dates.sort();
        let (Some(first), Some(last)) = (dates.first(), dates.last()) else {
            return from.is_none() && to.is_none();
        };
        let end = self.finished.as_deref().map_or_else(|| last.clone(), day);
        to.map_or(true, |t| first.as_str() <= t) && from.map_or(true, |f| end.as_str() >= f)
    }

    /// Compact Markdown for reports.
    pub fn render_markdown(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let _ = writeln!(
            out,
            "## {}{}",
            self.id,
            if self.title.is_empty() {
                String::new()
            } else {
                format!(" — {}", self.title)
            }
        );
        let date = |d: &Option<String>| {
            d.as_deref()
                .map_or("-".to_owned(), |s| s.get(..10).unwrap_or(s).to_owned())
        };
        let _ = writeln!(
            out,
            "- {} · created {} · started {} · finished {} · time {}",
            self.column,
            date(&self.created),
            date(&self.started),
            date(&self.finished),
            checkpoints::fmt_minutes(self.time_spent_min)
        );
        if let Some(l) = &self.link {
            let _ = writeln!(out, "- link: {l}");
        }
        if !self.workspaces.is_empty() {
            let _ = writeln!(out, "- code: {}", self.workspaces.join(", "));
        }
        for g in &self.gerrit {
            let _ = writeln!(out, "- gerrit: {g}");
        }
        if !self.checkpoints.is_empty() {
            let done = self.checkpoints.iter().filter(|c| c.done).count();
            let est: u64 = self.checkpoints.iter().map(|c| c.estimate_min).sum();
            let spent: u64 = self.checkpoints.iter().map(|c| c.spent_min).sum();
            let _ = writeln!(
                out,
                "- checkpoints: {done}/{} · estimated {} · spent {}",
                self.checkpoints.len(),
                checkpoints::fmt_minutes(est),
                checkpoints::fmt_minutes(spent)
            );
        }
        for o in &self.outcome {
            let _ = writeln!(out, "- outcome: {o}");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD: &str = "# PROJ-1: Fix login\n\n## Checkpoints\n<!-- pahiri:checkpoints -->\n- [x] A (10m; spent 12m)\n- [ ] B (20m)\n<!-- /pahiri:checkpoints -->\n\n## Outcome\n- 2026-03-10 12:00 Shipped; root cause was a stale token\n\n## Log\n- 2026-03-02 09:00 started\n\n## Attachments\n<!-- pahiri:begin -->\n- link: https://j/PROJ-1\n- workspace: fw\n- gerrit: fw I1 https://g/q/I1 :: Fix token\n- created: 2026-03-01T08:00:00Z\n- started: 2026-03-02T09:00:00Z\n- finished: 2026-03-10T12:00:00Z\n- time_spent: 95m\n<!-- pahiri:end -->\n";

    #[test]
    fn builds_record_and_renders() {
        let r = TaskRecord::from_markdown("PROJ-1", "Done", MD);
        assert_eq!(r.title, "Fix login");
        assert_eq!(r.checkpoints.len(), 2);
        assert_eq!(r.next_checkpoint().unwrap().title, "B");
        assert_eq!(
            r.outcome,
            vec!["2026-03-10 12:00 Shipped; root cause was a stale token"]
        );
        assert_eq!(r.log.len(), 1);
        assert_eq!(r.gerrit, vec!["fw I1 https://g/q/I1 :: Fix token"]);
        let md = r.render_markdown();
        assert!(md.starts_with("## PROJ-1 — Fix login\n- Done · created 2026-03-01 · started 2026-03-02 · finished 2026-03-10 · time 1h35m\n"), "{md}");
        assert!(md.contains("- checkpoints: 1/2 · estimated 30m · spent 12m\n"));
        assert!(md.contains("- outcome: 2026-03-10 12:00 Shipped"));
    }

    #[test]
    fn summary_prefers_the_goal_then_the_description() {
        let with_goal = "# T\n\n## Description\n\nThe ticket text.\n\n## Context\n<!-- pahiri:context -->\n### Background\n- old stuff\n\n### Goal\n- Make login survive token expiry\n<!-- /pahiri:context -->\n";
        assert_eq!(summary_of(with_goal), "Make login survive token expiry");
        let no_goal = "# T\n\n## Context\n<!-- pahiri:context -->\nSome words first.\n<!-- /pahiri:context -->\n";
        assert_eq!(summary_of(no_goal), "Some words first.");
        let description = "# T\n\n## Description\n\n> quoted *intro* line\nmore\n\n## Notes\n";
        assert_eq!(summary_of(description), "quoted *intro* line");
        assert_eq!(summary_of("# T\n\n## Notes\n"), "");
        let long = format!("# T\n\n## Description\n{}\n", "x".repeat(300));
        assert_eq!(summary_of(&long).chars().count(), SUMMARY_CHARS + 1);
        let r = TaskRecord::from_markdown("PROJ-1", "Done", MD);
        assert_eq!(r.changes[0].subject, "Fix token");
    }

    #[test]
    fn date_ranges() {
        let r = TaskRecord::from_markdown("PROJ-1", "Done", MD);
        assert!(r.active_between(Some("2026-03-05"), Some("2026-03-06")));
        assert!(r.active_between(Some("2026-01-01"), Some("2026-12-31")));
        assert!(!r.active_between(Some("2026-04-01"), None));
        assert!(!r.active_between(None, Some("2026-02-01")));
        assert!(r.active_between(None, None));
        let empty = TaskRecord::from_markdown("x", "Planned", "# x\n");
        assert!(empty.active_between(None, None));
        assert!(!empty.active_between(Some("2026-01-01"), None));
        assert_eq!(title_of("# x\n", "x"), "");
    }
}
