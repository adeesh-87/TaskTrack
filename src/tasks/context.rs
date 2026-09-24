//! The pahiri-managed block inside a task's `CONTEXT.md`.
//!
//! Everything pahiri records about a task (ticket source and link, attached
//! workspaces and builds, the task branch) lives between two HTML comment
//! markers so the rest of the file stays free-form:
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

/// Initial `CONTEXT.md` for a new task, optionally seeded from a ticket.
pub fn render_new(id: &str, source: Option<&str>, ticket: Option<&Ticket>) -> String {
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
        let md = "# T\n\n## Attachments\n<!-- pahiri:begin -->\n- source: jira\n- link: http://x\n- workspace: a\n- workspace: b\n- build: y\n- branch: T\n<!-- pahiri:end -->\n";
        let meta = TaskMeta::parse(md);
        assert_eq!(meta.source.as_deref(), Some("jira"));
        assert_eq!(meta.workspaces, vec!["a", "b"]);
        assert_eq!(meta.builds, vec!["y"]);
        assert_eq!(meta.branch.as_deref(), Some("T"));
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
        };
        let md = render_new("PROJ-1", Some("jira"), Some(&t));
        assert!(md.starts_with("# PROJ-1: Fix it\n\nSource: jira\nLink: https://j/PROJ-1\n\n## Description\n\nSteps\n1. a\n"));
        let meta = TaskMeta::parse(&md);
        assert_eq!(meta.link.as_deref(), Some("https://j/PROJ-1"));
        assert_eq!(meta.source.as_deref(), Some("jira"));
        let plain = render_new("custom", None, None);
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
    }
}
