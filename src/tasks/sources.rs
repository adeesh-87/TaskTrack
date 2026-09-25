//! Tickets produced by external task-source scripts.

use std::path::Path;
use std::process::Command;

use serde::Deserialize;

/// A ticket as reported by a task source script.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct Ticket {
    /// Ticket id, used as the task id (e.g. `PROJ-123`).
    #[serde(alias = "key")]
    pub id: String,
    /// One-line title.
    #[serde(default, alias = "summary")]
    pub title: String,
    /// Link to the ticket.
    #[serde(default, alias = "link")]
    pub url: String,
    /// Longer description.
    #[serde(default, alias = "body")]
    pub description: String,
    /// Workflow status (`In Progress`, `Done`, …) — used by the audit.
    #[serde(default, alias = "state", deserialize_with = "text")]
    pub status: String,
    /// Finished, when the script knows better than the status name.
    #[serde(default)]
    pub done: Option<bool>,
    /// When the ticket was created (any common date format).
    #[serde(default, deserialize_with = "opt_text")]
    pub created: Option<String>,
    /// When work on it started.
    #[serde(default, deserialize_with = "opt_text")]
    pub started: Option<String>,
    /// When it was resolved.
    #[serde(
        default,
        alias = "resolved",
        alias = "resolutiondate",
        deserialize_with = "opt_text"
    )]
    pub finished: Option<String>,
}

/// A string, or a number / bool written as text (`null` → empty).
fn text<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(opt_text(d)?.unwrap_or_default())
}

/// Like [`text`], `None` for `null` and empty strings.
fn opt_text<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(s).filter(|s| !s.trim().is_empty()),
        other => Some(other.to_string()),
    })
}

/// A number given as a number or as text.
fn opt_number<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    Ok(opt_text(d)?.and_then(|s| s.trim().parse().ok()))
}

impl Ticket {
    /// Task id derived from the ticket: spaces and other unsafe characters
    /// become `-` so it can be a folder and a git branch.
    pub fn task_id(&self) -> String {
        let mut out = String::new();
        for c in self.id.trim().chars() {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                out.push(c);
            } else if !out.ends_with('-') {
                out.push('-');
            }
        }
        out.trim_matches(|c| c == '-' || c == '.').to_owned()
    }
}

/// Parse script output: a JSON array, JSON lines, or tab separated lines.
pub fn parse_tickets(output: &str) -> Result<Vec<Ticket>, String> {
    let text = output.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    if text.starts_with('[') {
        return serde_json::from_str::<Vec<Ticket>>(text)
            .map_err(|e| format!("invalid JSON array: {e}"))
            .and_then(non_empty_ids);
    }
    if text.starts_with('{') {
        let mut tickets = Vec::new();
        for (n, line) in text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .enumerate()
        {
            let t: Ticket = serde_json::from_str(line)
                .map_err(|e| format!("invalid JSON on line {}: {e}", n + 1))?;
            tickets.push(t);
        }
        return non_empty_ids(tickets);
    }
    let tickets = text
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|line| {
            let mut cols = line.split('\t').map(str::trim);
            Ticket {
                id: cols.next().unwrap_or_default().to_owned(),
                title: cols.next().unwrap_or_default().to_owned(),
                url: cols.next().unwrap_or_default().to_owned(),
                description: cols.next().unwrap_or_default().replace("\\n", "\n"),
                ..Ticket::default()
            }
        })
        .collect();
    non_empty_ids(tickets)
}

fn non_empty_ids(tickets: Vec<Ticket>) -> Result<Vec<Ticket>, String> {
    if let Some(t) = tickets.iter().find(|t| t.task_id().is_empty()) {
        return Err(format!("ticket without a usable id: {t:?}"));
    }
    Ok(tickets)
}

/// Review status of one Gerrit change, as printed by the Gerrit status command.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct GerritStatus {
    /// `Change-Id` (`I` + 40 hex) — required.
    #[serde(alias = "id")]
    pub change_id: String,
    /// Change number.
    #[serde(default, alias = "_number", deserialize_with = "opt_number")]
    pub number: Option<u64>,
    /// `NEW`, `MERGED`, `ABANDONED`, …
    #[serde(default)]
    pub status: String,
    /// Link to the change.
    #[serde(default)]
    pub url: Option<String>,
    /// Votes in any short form, e.g. `CR+2 V+1`.
    #[serde(default)]
    pub labels: String,
    /// Subject (replaces the commit subject when given).
    #[serde(default)]
    pub subject: Option<String>,
    /// Gerrit project (for "my changes" in the audit).
    #[serde(default, deserialize_with = "opt_text")]
    pub project: Option<String>,
    /// Target branch.
    #[serde(default, deserialize_with = "opt_text")]
    pub branch: Option<String>,
    /// Topic (often the ticket id).
    #[serde(default, deserialize_with = "opt_text")]
    pub topic: Option<String>,
    /// When the change was uploaded.
    #[serde(default, alias = "createdOn", deserialize_with = "opt_text")]
    pub created: Option<String>,
    /// Last update.
    #[serde(default, alias = "lastUpdated", deserialize_with = "opt_text")]
    pub updated: Option<String>,
    /// When it was merged.
    #[serde(default, alias = "submitted", deserialize_with = "opt_text")]
    pub merged: Option<String>,
}

impl GerritStatus {
    /// `NEW #1234 CR+2 V+1`.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.status.is_empty() {
            parts.push(self.status.clone());
        }
        if let Some(n) = self.number {
            parts.push(format!("#{n}"));
        }
        if !self.labels.trim().is_empty() {
            parts.push(self.labels.trim().to_owned());
        }
        parts.join(" ")
    }
}

/// Parse the Gerrit status command's output: a JSON array or JSON lines.
pub fn parse_gerrit_status(output: &str) -> Result<Vec<GerritStatus>, String> {
    let text = output.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let list: Vec<GerritStatus> = if text.starts_with('[') {
        serde_json::from_str(text).map_err(|e| format!("invalid JSON array: {e}"))?
    } else {
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .enumerate()
            .map(|(n, l)| {
                serde_json::from_str(l).map_err(|e| format!("invalid JSON on line {}: {e}", n + 1))
            })
            .collect::<Result<_, _>>()?
    };
    if let Some(bad) = list.iter().find(|s| s.change_id.trim().is_empty()) {
        return Err(format!("entry without change_id: {bad:?}"));
    }
    Ok(list)
}

/// Output of a source that may write a file: run `command` (if any) with
/// `$PAHIRI_OUTPUT_FILE` set to `file`, then read `file` — or, without a
/// file, take the command's stdout. Returns the text and, for a file, a note
/// like `read ~/jira.json (3 h old)`.
pub fn run_or_read(
    command: &str,
    file: Option<&Path>,
    env: &[(String, String)],
) -> Result<(String, Option<String>), String> {
    let mut env = env.to_vec();
    if let Some(f) = file {
        env.push(("PAHIRI_OUTPUT_FILE".into(), f.display().to_string()));
    }
    let stdout = if command.trim().is_empty() {
        String::new()
    } else {
        run_script(command, &env)?
    };
    let Some(f) = file else {
        return Ok((stdout, None));
    };
    let text = std::fs::read_to_string(f).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!(
                "no file {}: the command should write it (to $PAHIRI_OUTPUT_FILE), or run your export first",
                f.display()
            )
        } else {
            format!("cannot read {}: {e}", f.display())
        }
    })?;
    let age = std::fs::metadata(f)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map_or_else(String::new, |d| format!(", {} old", age(d.as_secs())));
    Ok((text, Some(format!("read {}{age}", f.display()))))
}

/// `40 s`, `12 min`, `3 h`, `4 days`.
fn age(secs: u64) -> String {
    match secs {
        s if s < 60 => format!("{s} s"),
        s if s < 3600 => format!("{} min", s / 60),
        s if s < 86_400 => format!("{} h", s / 3600),
        s => format!("{} days", s / 86_400),
    }
}

/// Tickets of a task source: its command's stdout, or its file.
pub fn fetch_source(
    command: &str,
    file: Option<&Path>,
    env: &[(String, String)],
) -> Result<(Vec<Ticket>, Option<String>), String> {
    let (text, note) = run_or_read(command, file, env)?;
    let tickets = parse_tickets(&text).map_err(|e| match file {
        Some(f) => format!("{}: {e}", f.display()),
        None => e,
    })?;
    Ok((tickets, note))
}

/// Run a command through `sh -c` with `env`; stdout, or the exit status and
/// the tail of stderr.
pub fn run_script(command: &str, env: &[(String, String)]) -> Result<String, String> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let output = cmd
        .output()
        .map_err(|e| format!("could not run {command:?}: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{command:?} exited with {}\n{}",
            output.status,
            stderr
                .trim()
                .lines()
                .rev()
                .take(8)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    Ok(stdout.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn sources_can_write_a_file_for_pahiri_to_read() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("jira.json");
        // The command writes the file; its stdout is ignored.
        let (tickets, note) = fetch_source(
            r#"echo noise; printf '[{"id":"P-1","title":"From the file"}]' > "$PAHIRI_OUTPUT_FILE""#,
            Some(&file),
            &[],
        )
        .unwrap();
        assert_eq!(tickets[0].title, "From the file");
        let note = note.unwrap();
        assert!(
            note.starts_with(&format!("read {}", file.display())),
            "{note}"
        );
        assert!(note.contains(" s old"), "{note}");
        // No command: just the file (kept fresh by cron, say).
        assert_eq!(fetch_source("", Some(&file), &[]).unwrap().0.len(), 1);
        // Without a file, stdout as before.
        let (t, note) = fetch_source("printf 'P-2\\tTwo'", None, &[]).unwrap();
        assert_eq!((t[0].id.as_str(), note), ("P-2", None));
        // Problems name the file.
        let missing = dir.path().join("nope.json");
        let e = fetch_source("true", Some(&missing), &[]).unwrap_err();
        assert!(
            e.starts_with(&format!("no file {}", missing.display())),
            "{e}"
        );
        fs::write(&file, "[{broken").unwrap();
        let e = fetch_source("", Some(&file), &[]).unwrap_err();
        assert!(e.starts_with(&file.display().to_string()), "{e}");
        // A failing command stops before reading.
        assert!(fetch_source("exit 3", Some(&file), &[])
            .unwrap_err()
            .contains("exited"));
    }

    #[test]
    fn parses_my_changes_for_the_audit() {
        let out = r#"{"change_id":"I0123456789012345678901234567890123456789","number":12345,"status":"MERGED","url":"https://r/c/12345","subject":"Fix it","project":"fw","branch":"main","topic":"PROJ-1","created":1751446800,"updated":1752076800,"merged":1752076800,"labels":"CR+2"}"#;
        let c = &parse_gerrit_status(out).unwrap()[0];
        assert_eq!(c.number, Some(12345));
        assert_eq!(c.project.as_deref(), Some("fw"));
        assert_eq!(c.topic.as_deref(), Some("PROJ-1"));
        assert_eq!(c.created.as_deref(), Some("1751446800"));
        assert_eq!(
            crate::time::normalize(c.merged.as_deref().unwrap()).unwrap(),
            "2025-07-09T16:00:00Z"
        );
        assert_eq!(c.summary(), "MERGED #12345 CR+2");
        // REST style: _number as text, submitted, no project.
        let rest = r#"{"change_id":"I1","_number":"7","status":"NEW","submitted":null,"created":"2026-01-02 03:04:05.000000000"}"#;
        let c = &parse_gerrit_status(rest).unwrap()[0];
        assert_eq!((c.number, c.merged.as_ref()), (Some(7), None));
        // Tickets with audit fields, dates as numbers.
        let t = &parse_tickets(r#"{"id":"P-1","status":"Done","created":1790000000,"resolutiondate":"2026-09-22","done":false}"#).unwrap()[0];
        assert_eq!(t.created.as_deref(), Some("1790000000"));
        assert_eq!(t.finished.as_deref(), Some("2026-09-22"));
        assert_eq!(t.done, Some(false));
    }

    #[test]
    fn parses_json_array_with_aliases() {
        let out = r#"[{"key":"PROJ-1","summary":"One","link":"http://a"},{"id":"PROJ-2","title":"Two","url":"http://b","description":"d"}]"#;
        let t = parse_tickets(out).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].id, "PROJ-1");
        assert_eq!(t[0].title, "One");
        assert_eq!(t[0].url, "http://a");
        assert_eq!(t[1].description, "d");
    }

    #[test]
    fn parses_json_lines_and_tsv() {
        let out = "{\"id\":\"A-1\",\"title\":\"x\"}\n{\"id\":\"A-2\"}\n";
        assert_eq!(parse_tickets(out).unwrap().len(), 2);
        let tsv = "# comment\nORB-7\tDo thing\thttps://orbit/ORB-7\tline1\\nline2\n";
        let t = parse_tickets(tsv).unwrap();
        assert_eq!(t[0].id, "ORB-7");
        assert_eq!(t[0].description, "line1\nline2");
        assert!(parse_tickets("").unwrap().is_empty());
        assert!(parse_tickets("[{\"title\":\"no id\"}]").is_err());
        assert!(parse_tickets("[oops").is_err());
    }

    #[test]
    fn task_id_is_branch_safe() {
        assert_eq!(
            Ticket {
                id: " PROJ 12 / x ".into(),
                ..Ticket::default()
            }
            .task_id(),
            "PROJ-12-x"
        );
        assert_eq!(
            Ticket {
                id: "ORB-7".into(),
                ..Ticket::default()
            }
            .task_id(),
            "ORB-7"
        );
    }

    #[test]
    fn gerrit_status_output() {
        let out = r#"{"change_id":"I1","number":12,"status":"NEW","labels":"CR+2 V+1","url":"https://g/c/p/+/12"}
{"id":"I2","status":"MERGED"}"#;
        let s = parse_gerrit_status(out).unwrap();
        assert_eq!(s[0].summary(), "NEW #12 CR+2 V+1");
        assert_eq!(s[1].summary(), "MERGED");
        assert_eq!(parse_gerrit_status("[]").unwrap().len(), 0);
        assert!(parse_gerrit_status("{\"status\":\"NEW\"}").is_err());
        assert!(parse_gerrit_status("nope").is_err());
    }

    #[test]
    fn fetch_runs_a_script() {
        let t = fetch_source("printf 'X-1\\tTitle\\thttp://x\\n'", None, &[])
            .unwrap()
            .0;
        assert_eq!(t[0].id, "X-1");
        let err = fetch_source("echo boom >&2; exit 3", None, &[]).unwrap_err();
        assert!(err.contains("boom"), "{err}");
    }
}
