//! The pahiri-managed block inside a task's `CONTEXT.md`.
//!
//! Everything pahiri records about a task (ticket source and link, attached
//! workspaces and builds, the task branch, timestamps, Gerrit changes) lives
//! between two HTML comment markers so the rest of the file stays free-form:
//!
//! ```markdown
//! ## Attachments
//! <!-- pahiri:begin -->
//! - source: jira
//! - link: https://jira.example.com/browse/PROJ-123
//! - workspace: firmware
//! - build: yocto-2024
//! - branch: PROJ-123
//! - prepared: 2026-09-24T10:00:00Z
//! - gerrit: firmware I3f2a… https://gerrit/q/I3f2a… :: Fix the flux capacitor
//! - created: 2026-09-20T08:00:00Z
//! - started: 2026-09-21T09:30:00Z
//! - finished: 2026-09-24T17:00:00Z
//! - time_spent: 215m
//! - context_ready: true
//! <!-- pahiri:end -->
//! ```

use std::fs;
use std::io;
use std::path::Path;

use super::sources::Ticket;

/// Start marker of the managed block.
pub const BEGIN: &str = "<!-- pahiri:begin -->";
/// End marker of the managed block.
pub const END: &str = "<!-- pahiri:end -->";
const HEADING: &str = "## Attachments";

/// A Gerrit change found on a task branch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GerritRef {
    /// Workspace the commit lives in.
    pub workspace: String,
    /// The `Change-Id` trailer.
    pub change_id: String,
    /// Review URL (search URL when the change number is unknown).
    pub url: Option<String>,
    /// Commit subject.
    pub subject: String,
    /// Review status from the Gerrit status command, e.g. `NEW #1234 CR+2 V+1`.
    pub status: Option<String>,
}

impl GerritRef {
    /// Render as the value of a `- gerrit:` line.
    pub fn render(&self) -> String {
        let mut s = format!("{} {}", self.workspace, self.change_id);
        if let Some(u) = &self.url {
            s.push(' ');
            s.push_str(u);
        }
        if let Some(st) = &self.status {
            s.push_str(" [");
            s.push_str(&st.replace(['[', ']'], ""));
            s.push(']');
        }
        if !self.subject.is_empty() {
            s.push_str(" :: ");
            s.push_str(&self.subject);
        }
        s
    }

    fn parse(value: &str) -> Option<Self> {
        let (head, subject) = match value.split_once(" :: ") {
            Some((h, s)) => (h, s.trim().to_owned()),
            None => (value, String::new()),
        };
        let (head, status) = match (head.find(" ["), head.rfind(']')) {
            (Some(b), Some(e)) if e > b => (
                format!("{}{}", &head[..b], &head[e + 1..]),
                Some(head[b + 2..e].trim().to_owned()).filter(|s| !s.is_empty()),
            ),
            _ => (head.to_owned(), None),
        };
        let mut parts = head.split_whitespace();
        let workspace = parts.next()?.to_owned();
        let change_id = parts.next()?.to_owned();
        let url = parts.next().map(str::to_owned);
        Some(Self {
            workspace,
            change_id,
            url,
            subject,
            status,
        })
    }
}

/// Machine-readable task metadata.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskMeta {
    /// Ticket source name, e.g. `jira`.
    pub source: Option<String>,
    /// Link to the ticket.
    pub link: Option<String>,
    /// Attached workspace names.
    pub workspaces: Vec<String>,
    /// Attached build names.
    pub builds: Vec<String>,
    /// Branch the workspaces were switched to.
    pub branch: Option<String>,
    /// When `prepare` last ran (RFC 3339, UTC).
    pub prepared: Option<String>,
    /// Gerrit changes found on the task branches.
    pub gerrit: Vec<GerritRef>,
    /// When the task folder was created.
    pub created: Option<String>,
    /// When work started (first timer start or first move out of the first column).
    pub started: Option<String>,
    /// When the task was moved to the last column.
    pub finished: Option<String>,
    /// Minutes recorded by the checkpoint timer.
    pub time_spent_min: u64,
    /// Whether the context is good enough to break the task into checkpoints.
    pub context_ready: bool,
}

impl TaskMeta {
    /// Parse the managed block out of Markdown text (absent block → default).
    pub fn parse(markdown: &str) -> Self {
        let mut meta = Self::default();
        let mut inside = false;
        for raw in markdown.lines() {
            let line = raw.trim();
            if line == BEGIN {
                inside = true;
                continue;
            }
            if line == END {
                break;
            }
            if !inside {
                continue;
            }
            let Some(rest) = line.strip_prefix("- ") else {
                continue;
            };
            let Some((key, value)) = rest.split_once(':') else {
                continue;
            };
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            match key.trim() {
                "source" => meta.source = Some(value.to_owned()),
                "link" => meta.link = Some(value.to_owned()),
                "workspace" => meta.workspaces.push(value.to_owned()),
                "build" => meta.builds.push(value.to_owned()),
                "branch" => meta.branch = Some(value.to_owned()),
                "prepared" => meta.prepared = Some(value.to_owned()),
                "gerrit" => meta.gerrit.extend(GerritRef::parse(value)),
                "created" => meta.created = Some(value.to_owned()),
                "started" => meta.started = Some(value.to_owned()),
                "finished" => meta.finished = Some(value.to_owned()),
                "time_spent" => {
                    meta.time_spent_min = value.trim_end_matches('m').trim().parse().unwrap_or(0);
                }
                "context_ready" => meta.context_ready = matches!(value, "true" | "yes" | "1"),
                _ => {}
            }
        }
        meta
    }

    /// Render just the managed block (markers included).
    pub fn render_block(&self) -> String {
        let mut out = String::from(BEGIN);
        out.push('\n');
        let mut push = |k: &str, v: &str| {
            out.push_str("- ");
            out.push_str(k);
            out.push_str(": ");
            out.push_str(v);
            out.push('\n');
        };
        if let Some(s) = &self.source {
            push("source", s);
        }
        if let Some(l) = &self.link {
            push("link", l);
        }
        for w in &self.workspaces {
            push("workspace", w);
        }
        for b in &self.builds {
            push("build", b);
        }
        if let Some(b) = &self.branch {
            push("branch", b);
        }
        if let Some(p) = &self.prepared {
            push("prepared", p);
        }
        for g in &self.gerrit {
            push("gerrit", &g.render());
        }
        if let Some(c) = &self.created {
            push("created", c);
        }
        if let Some(s) = &self.started {
            push("started", s);
        }
        if let Some(f) = &self.finished {
            push("finished", f);
        }
        if self.time_spent_min > 0 {
            push("time_spent", &format!("{}m", self.time_spent_min));
        }
        if self.context_ready {
            push("context_ready", "true");
        }
        out.push_str(END);
        out.push('\n');
        out
    }

    /// Return `markdown` with the managed block replaced (or appended under an
    /// "Attachments" heading when there was none).
    pub fn apply(&self, markdown: &str) -> String {
        let block = self.render_block();
        let begin = markdown.find(BEGIN);
        let end = markdown.find(END);
        match (begin, end) {
            (Some(b), Some(e)) if e >= b => {
                let after = e + END.len();
                let after = if markdown[after..].starts_with('\n') {
                    after + 1
                } else {
                    after
                };
                format!("{}{}{}", &markdown[..b], block, &markdown[after..])
            }
            _ => {
                let mut out = markdown.to_owned();
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(HEADING);
                out.push('\n');
                out.push_str(&block);
                out
            }
        }
    }

    /// Whether anything is attached.
    pub fn has_attachments(&self) -> bool {
        !self.workspaces.is_empty() || !self.builds.is_empty()
    }
}

/// Read the metadata from a context file (missing file → default).
pub fn read_meta(path: &Path) -> io::Result<TaskMeta> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(TaskMeta::parse(&text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(TaskMeta::default()),
        Err(e) => Err(e),
    }
}

/// Write the metadata into a context file, creating it if needed.
pub fn write_meta(path: &Path, id: &str, meta: &TaskMeta) -> io::Result<()> {
    let existing = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => format!("# {id}\n"),
        Err(e) => return Err(e),
    };
    fs::write(path, meta.apply(&existing))
}

/// Read-modify-write the metadata of a context file.
pub fn update_meta(
    path: &Path,
    id: &str,
    edit: impl FnOnce(&mut TaskMeta),
) -> io::Result<TaskMeta> {
    let mut meta = read_meta(path)?;
    edit(&mut meta);
    write_meta(path, id, &meta)?;
    Ok(meta)
}

/// Heading of the timestamped log pahiri (and `pahiri task log`) appends to.
pub const LOG_HEADING: &str = "## Log";
/// Heading of the one-line outcomes recorded when a task is finished.
pub const OUTCOME_HEADING: &str = "## Outcome";
/// Heading of the generated context section.
pub const CONTEXT_HEADING: &str = "## Context";
/// Start marker of the generated context.
pub const CONTEXT_BEGIN: &str = "<!-- pahiri:context -->";
/// End marker of the generated context.
pub const CONTEXT_END: &str = "<!-- /pahiri:context -->";

fn read_or_new(path: &Path, id: &str) -> io::Result<String> {
    match fs::read_to_string(path) {
        Ok(t) => Ok(t),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(format!("# {id}\n")),
        Err(e) => Err(e),
    }
}

/// Append `- <timestamp> <line>` under `heading` (created before the attachments block).
pub fn append_under(
    path: &Path,
    id: &str,
    heading: &str,
    timestamp: &str,
    line: &str,
) -> io::Result<()> {
    let text = read_or_new(path, id)?;
    let item = format!("{} {line}", crate::time::short(timestamp));
    fs::write(
        path,
        super::sections::append_item(&text, heading, &item, &["## Attachments"]),
    )
}

/// Append a line to the task log.
pub fn append_log(path: &Path, id: &str, timestamp: &str, line: &str) -> io::Result<()> {
    append_under(path, id, LOG_HEADING, timestamp, line)
}

/// Replace (or add) the generated `## Context` section.
pub fn write_generated_context(path: &Path, id: &str, body: &str) -> io::Result<()> {
    let text = read_or_new(path, id)?;
    let out = super::sections::upsert(
        &text,
        CONTEXT_HEADING,
        CONTEXT_BEGIN,
        CONTEXT_END,
        body,
        &[
            "## Notes",
            super::checkpoints::HEADING,
            LOG_HEADING,
            "## Attachments",
        ],
    );
    fs::write(path, out)
}

/// Record `started` / `finished` for a move from column `from` to `to` of
/// `columns`. The TUI and `pahiri task move` both use this.
pub fn record_move(
    path: &Path,
    id: &str,
    from: usize,
    to: usize,
    columns: usize,
    now: &str,
) -> io::Result<TaskMeta> {
    let last = columns.saturating_sub(1);
    update_meta(path, id, |m| {
        if to > 0 && m.started.is_none() {
            m.started = Some(now.to_owned());
        }
        if columns > 1 && to == last {
            m.finished = Some(now.to_owned());
        } else if from == last {
            m.finished = None;
        }
    })
}

/// Replace the `## Description` section with a ticket's current description.
pub fn refresh_description(path: &Path, id: &str, description: &str) -> io::Result<()> {
    let text = read_or_new(path, id)?;
    let out = super::sections::replace_plain(
        &text,
        "## Description",
        description.trim(),
        &[
            "## Context",
            "## Notes",
            super::checkpoints::HEADING,
            "## Attachments",
        ],
    );
    fs::write(path, out)
}

/// Flip the "context is good enough to plan" switch. The settings page, the
/// palette and `pahiri task ready` all go through this one function.
pub fn set_context_ready(path: &Path, id: &str, ready: bool) -> io::Result<TaskMeta> {
    update_meta(path, id, |m| m.context_ready = ready)
}

/// Initial `CONTEXT.md` for a new task, optionally seeded from a ticket.
pub fn render_new(
    id: &str,
    source: Option<&str>,
    ticket: Option<&Ticket>,
    created: &str,
) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    match ticket {
        Some(t) if !t.title.is_empty() => {
            let _ = writeln!(out, "# {id}: {}\n", t.title);
        }
        _ => {
            let _ = writeln!(out, "# {id}\n");
        }
    }
    if let Some(t) = ticket {
        if let Some(s) = source {
            let _ = writeln!(out, "Source: {s}");
        }
        if !t.url.is_empty() {
            let _ = writeln!(out, "Link: {}", t.url);
        }
        out.push('\n');
        if !t.description.trim().is_empty() {
            out.push_str("## Description\n\n");
            out.push_str(t.description.trim());
            out.push_str("\n\n");
        }
    }
    let meta = TaskMeta {
        source: source.map(str::to_owned).filter(|_| ticket.is_some()),
        link: ticket.map(|t| t.url.clone()).filter(|u| !u.is_empty()),
        created: Some(created.to_owned()),
        ..TaskMeta::default()
    };
    out.push_str("## Notes\n\n");
    meta.apply(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_render_block() {
        let md = "# T\n\n## Attachments\n<!-- pahiri:begin -->\n- source: jira\n- link: http://x\n- workspace: a\n- workspace: b\n- build: y\n- branch: T\n- gerrit: a I123 https://g/q/I123 [NEW #7 CR+2] :: Fix it\n- created: 2026-01-01T00:00:00Z\n- time_spent: 42m\n- context_ready: true\n<!-- pahiri:end -->\n";
        let meta = TaskMeta::parse(md);
        assert_eq!(meta.source.as_deref(), Some("jira"));
        assert_eq!(meta.workspaces, vec!["a", "b"]);
        assert_eq!(meta.builds, vec!["y"]);
        assert_eq!(meta.branch.as_deref(), Some("T"));
        assert_eq!(meta.gerrit.len(), 1);
        assert_eq!(meta.gerrit[0].change_id, "I123");
        assert_eq!(meta.gerrit[0].url.as_deref(), Some("https://g/q/I123"));
        assert_eq!(meta.gerrit[0].subject, "Fix it");
        assert_eq!(meta.gerrit[0].status.as_deref(), Some("NEW #7 CR+2"));
        assert_eq!(meta.time_spent_min, 42);
        assert!(meta.context_ready);
        assert_eq!(TaskMeta::parse(&meta.apply("")), meta);
    }

    #[test]
    fn apply_replaces_in_place_and_keeps_prose() {
        let md = "# T\n\nprose before\n\n## Attachments\n<!-- pahiri:begin -->\n- workspace: old\n<!-- pahiri:end -->\n\nprose after\n";
        let meta = TaskMeta {
            workspaces: vec!["new".into()],
            ..TaskMeta::default()
        };
        let out = meta.apply(md);
        assert!(out.starts_with("# T\n\nprose before\n\n## Attachments\n<!-- pahiri:begin -->\n- workspace: new\n<!-- pahiri:end -->\n\nprose after\n"), "{out}");
        assert!(!out.contains("old"));
    }

    #[test]
    fn apply_appends_when_missing() {
        let meta = TaskMeta {
            builds: vec!["b".into()],
            ..TaskMeta::default()
        };
        let out = meta.apply("# T\nhello");
        assert_eq!(out, "# T\nhello\n\n## Attachments\n<!-- pahiri:begin -->\n- build: b\n<!-- pahiri:end -->\n");
    }

    #[test]
    fn new_context_from_ticket() {
        let t = Ticket {
            id: "PROJ-1".into(),
            title: "Fix it".into(),
            url: "https://j/PROJ-1".into(),
            description: "Steps\n1. a".into(),
            ..Ticket::default()
        };
        let md = render_new("PROJ-1", Some("jira"), Some(&t), "2026-01-01T00:00:00Z");
        assert!(md.starts_with("# PROJ-1: Fix it\n\nSource: jira\nLink: https://j/PROJ-1\n\n## Description\n\nSteps\n1. a\n"));
        let meta = TaskMeta::parse(&md);
        assert_eq!(meta.link.as_deref(), Some("https://j/PROJ-1"));
        assert_eq!(meta.source.as_deref(), Some("jira"));
        assert_eq!(meta.created.as_deref(), Some("2026-01-01T00:00:00Z"));
        let plain = render_new("custom", None, None, "2026-01-01T00:00:00Z");
        assert!(plain.starts_with("# custom\n"));
        assert!(TaskMeta::parse(&plain).link.is_none());
    }

    #[test]
    fn read_write_meta_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("CONTEXT.md");
        assert_eq!(read_meta(&p).unwrap(), TaskMeta::default());
        let meta = TaskMeta {
            branch: Some("x".into()),
            ..TaskMeta::default()
        };
        write_meta(&p, "x", &meta).unwrap();
        assert_eq!(read_meta(&p).unwrap(), meta);
        assert!(fs::read_to_string(&p).unwrap().starts_with("# x\n"));
        let updated = update_meta(&p, "x", |m| m.context_ready = true).unwrap();
        assert!(updated.context_ready);
        assert!(read_meta(&p).unwrap().context_ready);
    }

    #[test]
    fn log_context_and_ready() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("CONTEXT.md");
        write_meta(
            &p,
            "x",
            &TaskMeta {
                branch: Some("b".into()),
                ..TaskMeta::default()
            },
        )
        .unwrap();
        append_log(&p, "x", "2026-01-02T03:04:05Z", "first").unwrap();
        append_log(&p, "x", "2026-01-02T03:05:00Z", "second").unwrap();
        let text = fs::read_to_string(&p).unwrap();
        assert!(
            text.contains(&format!(
                "## Log\n- {} first\n- {} second\n\n## Attachments\n",
                crate::time::short("2026-01-02T03:04:05Z"),
                crate::time::short("2026-01-02T03:05:00Z")
            )),
            "{text}"
        );
        write_generated_context(&p, "x", "### Goal\n- do it").unwrap();
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.contains("## Context\n<!-- pahiri:context -->\n### Goal\n- do it\n<!-- /pahiri:context -->\n\n## Log"), "{text}");
        assert!(set_context_ready(&p, "x", true).unwrap().context_ready);
        let m = record_move(&p, "x", 0, 2, 3, "2026-01-03T00:00:00Z").unwrap();
        assert_eq!(m.started.as_deref(), Some("2026-01-03T00:00:00Z"));
        assert_eq!(m.finished.as_deref(), Some("2026-01-03T00:00:00Z"));
        let m = record_move(&p, "x", 2, 1, 3, "2026-01-04T00:00:00Z").unwrap();
        assert!(m.finished.is_none());
        assert_eq!(m.started.as_deref(), Some("2026-01-03T00:00:00Z"));
        refresh_description(&p, "x", "new text").unwrap();
        let text = fs::read_to_string(&p).unwrap();
        assert!(
            text.contains("## Description\n\nnew text\n\n## Context"),
            "{text}"
        );
        assert!(read_meta(&p).unwrap().context_ready);
        assert_eq!(read_meta(&p).unwrap().branch.as_deref(), Some("b"));
    }
}
