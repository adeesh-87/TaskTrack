//! Tickets produced by external task-source scripts.

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
    #[serde(default)]
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

/// Run a task source command through `sh -c` and parse its output.
pub fn fetch(command: &str, task_dir: Option<&std::path::Path>) -> Result<Vec<Ticket>, String> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command);
    if let Some(dir) = task_dir {
        cmd.env("PAHIRI_TASKS_DIR", dir);
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
    parse_tickets(&stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let t = fetch("printf 'X-1\\tTitle\\thttp://x\\n'", None).unwrap();
        assert_eq!(t[0].id, "X-1");
        let err = fetch("echo boom >&2; exit 3", None).unwrap_err();
        assert!(err.contains("boom"), "{err}");
    }
}
