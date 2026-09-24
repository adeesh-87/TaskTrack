//! Checkpoints: the ordered sub-steps of a task with time estimates.
//!
//! They live in `CONTEXT.md` as a plain Markdown checklist between markers so
//! both people and agents can read and edit them:
//!
//! ```markdown
//! ## Checkpoints
//! <!-- pahiri:checkpoints -->
//! - [x] Read the DMA chapter of the datasheet (20m; spent 25m)
//! - [ ] Write the ISR skeleton (40m; spent 12m)
//! - [ ] Unit test the ring buffer (30m)
//! <!-- /pahiri:checkpoints -->
//! ```
//!
//! [`parse_list`] is deliberately tolerant: any line that is a task-list item
//! ending in a `(<duration>)` counts, whatever surrounds it. That is what lets
//! pahiri accept the raw output of an agent.

use std::fs;
use std::io;
use std::path::Path;

/// Start marker.
pub const BEGIN: &str = "<!-- pahiri:checkpoints -->";
/// End marker.
pub const END: &str = "<!-- /pahiri:checkpoints -->";
/// Heading of the checkpoint section.
pub const HEADING: &str = "## Checkpoints";

/// One checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    /// What to do, imperative.
    pub title: String,
    /// Estimated minutes.
    pub estimate_min: u64,
    /// Minutes recorded by the timer so far.
    pub spent_min: u64,
    /// Whether it is ticked.
    pub done: bool,
}

impl Checkpoint {
    /// Render as a checklist line.
    pub fn render(&self) -> String {
        let mark = if self.done { "x" } else { " " };
        if self.spent_min > 0 {
            format!(
                "- [{mark}] {} ({}; spent {})",
                self.title,
                fmt_minutes(self.estimate_min),
                fmt_minutes(self.spent_min)
            )
        } else {
            format!(
                "- [{mark}] {} ({})",
                self.title,
                fmt_minutes(self.estimate_min)
            )
        }
    }

    /// Parse one checklist line; `None` when it is not a checkpoint.
    pub fn parse(line: &str) -> Option<Self> {
        let t = line.trim();
        let rest = t
            .strip_prefix("- ")
            .or_else(|| t.strip_prefix("* "))
            .or_else(|| t.strip_prefix("+ "))?;
        let rest = rest.trim_start();
        // Optional "1." numbering before or after the checkbox.
        let rest = strip_number(rest);
        let (done, rest) = if let Some(r) = rest.strip_prefix("[ ]") {
            (false, r)
        } else if let Some(r) = rest
            .strip_prefix("[x]")
            .or_else(|| rest.strip_prefix("[X]"))
        {
            (true, r)
        } else {
            return None;
        };
        let rest = strip_number(rest.trim_start()).trim();
        let open = rest.rfind('(')?;
        if !rest.ends_with(')') {
            return None;
        }
        let title = rest[..open].trim().trim_end_matches([':', '-', '—']).trim();
        let inner = &rest[open + 1..rest.len() - 1];
        let mut estimate = None;
        let mut spent = 0;
        for part in inner.split([';', ',']) {
            let part = part.trim();
            if let Some(s) = part.strip_prefix("spent") {
                spent = parse_minutes(s.trim())?;
            } else if estimate.is_none() {
                estimate = Some(parse_minutes(part.trim_start_matches("est").trim())?);
            }
        }
        let estimate_min = estimate?;
        if title.is_empty() {
            return None;
        }
        Some(Self {
            title: title.to_owned(),
            estimate_min,
            spent_min: spent,
            done,
        })
    }
}

fn strip_number(s: &str) -> &str {
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 {
        let after = &s[digits..];
        if let Some(r) = after.strip_prefix('.').or_else(|| after.strip_prefix(')')) {
            return r.trim_start();
        }
    }
    s
}

/// Parse `40m`, `1h`, `1h30m`, `1.5h`, `90 min`, `2 hours`, `45` into minutes.
pub fn parse_minutes(s: &str) -> Option<u64> {
    let mut t = s.trim().to_lowercase();
    for (long, short) in [
        ("hours", "h"),
        ("hour", "h"),
        ("hrs", "h"),
        ("hr", "h"),
        ("minutes", "m"),
        ("minute", "m"),
        ("mins", "m"),
        ("min", "m"),
    ] {
        t = t.replace(long, short);
    }
    t.retain(|c| !c.is_whitespace());
    if t.is_empty() {
        return None;
    }
    let num = |x: &str| -> Option<f64> {
        (!x.is_empty() && x.bytes().all(|b| b.is_ascii_digit() || b == b'.'))
            .then(|| x.parse().ok())
            .flatten()
    };
    let minutes = if let Some((h, m)) = t.split_once('h') {
        let m = m.strip_suffix('m').unwrap_or(m);
        num(h)? * 60.0 + if m.is_empty() { 0.0 } else { num(m)? }
    } else {
        num(t.strip_suffix('m').unwrap_or(&t))?
    };
    Some(minutes.round() as u64)
}

/// `90` → `1h30m`, `40` → `40m`, `120` → `2h`.
pub fn fmt_minutes(min: u64) -> String {
    match (min / 60, min % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h{m}m"),
    }
}

/// Every checkpoint line found in `text`, in order (tolerant of surrounding prose).
pub fn parse_list(text: &str) -> Vec<Checkpoint> {
    text.lines().filter_map(Checkpoint::parse).collect()
}

/// The checkpoints stored between the markers of a context file.
pub fn parse_section(markdown: &str) -> Vec<Checkpoint> {
    super::sections::extract(markdown, BEGIN, END).map_or_else(Vec::new, parse_list)
}

/// Replace (or add, before the log / attachments) the checkpoint section.
pub fn apply(markdown: &str, items: &[Checkpoint]) -> String {
    let body: String = items.iter().map(|c| c.render() + "\n").collect();
    super::sections::upsert(
        markdown,
        HEADING,
        BEGIN,
        END,
        &body,
        &[super::context::LOG_HEADING, "## Attachments"],
    )
}

/// Read the checkpoints of a context file (missing file → none).
pub fn read(path: &Path) -> io::Result<Vec<Checkpoint>> {
    match fs::read_to_string(path) {
        Ok(t) => Ok(parse_section(&t)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Write the checkpoints into a context file.
pub fn write(path: &Path, id: &str, items: &[Checkpoint]) -> io::Result<()> {
    let existing = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => format!("# {id}\n"),
        Err(e) => return Err(e),
    };
    fs::write(path, apply(&existing, items))
}

/// Index of the first unfinished checkpoint.
pub fn next_open(items: &[Checkpoint]) -> Option<usize> {
    items.iter().position(|c| !c.done)
}

/// Summary such as `2/5 done · 1h10m of 3h`.
pub fn summary(items: &[Checkpoint]) -> String {
    let done = items.iter().filter(|c| c.done).count();
    let est: u64 = items.iter().map(|c| c.estimate_min).sum();
    let spent: u64 = items.iter().map(|c| c.spent_min).sum();
    format!(
        "{done}/{} done · {} of {}",
        items.len(),
        fmt_minutes(spent),
        fmt_minutes(est)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_many_shapes() {
        for (line, title, est, spent, done) in [
            ("- [ ] Write the ISR (40m)", "Write the ISR", 40, 0, false),
            (
                "- [x] Read docs (1h30m; spent 2h)",
                "Read docs",
                90,
                120,
                true,
            ),
            ("* [ ] 3. Numbered (25 min)", "Numbered", 25, 0, false),
            ("- [ ] Test it — (1.5h)", "Test it", 90, 0, false),
            (
                "  - [X] indented (2 hours, spent 10m)",
                "indented",
                120,
                10,
                true,
            ),
            ("- 1) [ ] Weird (45)", "Weird", 45, 0, false),
        ] {
            let c = Checkpoint::parse(line).unwrap_or_else(|| panic!("{line}"));
            assert_eq!(c.title, title, "{line}");
            assert_eq!(c.estimate_min, est, "{line}");
            assert_eq!(c.spent_min, spent, "{line}");
            assert_eq!(c.done, done, "{line}");
        }
        assert!(Checkpoint::parse("- [ ] no estimate").is_none());
        assert!(Checkpoint::parse("- plain item (10m)").is_none());
        assert!(Checkpoint::parse("- [ ] (10m)").is_none());
        assert!(Checkpoint::parse("- [ ] bad unit (10 parsecs)").is_none());
    }

    #[test]
    fn minutes_roundtrip() {
        assert_eq!(fmt_minutes(40), "40m");
        assert_eq!(fmt_minutes(60), "1h");
        assert_eq!(fmt_minutes(90), "1h30m");
        for m in [1, 40, 60, 90, 125] {
            assert_eq!(parse_minutes(&fmt_minutes(m)), Some(m));
        }
        assert_eq!(parse_minutes("1.5h"), Some(90));
        assert_eq!(parse_minutes("2 hours"), Some(120));
        assert_eq!(parse_minutes("x"), None);
    }

    #[test]
    fn agent_output_is_tolerated() {
        let out = "Sure! Here is the breakdown:\n\n## Checkpoints\n- [ ] Read the spec (20m)\nsome chatter\n- [ ] Implement parser (1h)\n- [ ] Tests (30m)\n\nGood luck!";
        let items = parse_list(out);
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].estimate_min, 60);
    }

    #[test]
    fn section_apply_and_parse() {
        let items = vec![
            Checkpoint {
                title: "A".into(),
                estimate_min: 10,
                spent_min: 0,
                done: false,
            },
            Checkpoint {
                title: "B".into(),
                estimate_min: 20,
                spent_min: 5,
                done: true,
            },
        ];
        let md = "# T\n\n## Notes\n\nhi\n\n## Attachments\n<!-- pahiri:begin -->\n- branch: T\n<!-- pahiri:end -->\n";
        let out = apply(md, &items);
        assert!(out.contains("hi\n\n## Checkpoints\n<!-- pahiri:checkpoints -->\n- [ ] A (10m)\n- [x] B (20m; spent 5m)\n<!-- /pahiri:checkpoints -->\n\n## Attachments\n"), "{out}");
        assert_eq!(parse_section(&out), items);
        // Replacing keeps everything else.
        let again = apply(&out, &items[..1]);
        assert_eq!(parse_section(&again).len(), 1);
        assert!(again.contains("## Attachments"));
        assert!(again.contains("hi\n"));
        assert_eq!(next_open(&items), Some(0));
        assert_eq!(summary(&items), "1/2 done · 5m of 30m");
        // No attachments block: appended at the end.
        let plain = apply("# T\n", &items[..1]);
        assert!(plain.ends_with("## Checkpoints\n<!-- pahiri:checkpoints -->\n- [ ] A (10m)\n<!-- /pahiri:checkpoints -->\n"));
    }

    #[test]
    fn files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("CONTEXT.md");
        assert!(read(&p).unwrap().is_empty());
        let items = parse_list("- [ ] x (5m)");
        write(&p, "t", &items).unwrap();
        assert_eq!(read(&p).unwrap(), items);
    }
}
