//! Merging a `CONTEXT.md` that pahiri changed on disk while you edited it.
//!
//! Three versions: `base` (what the editor loaded), `ours` (the editor
//! buffer) and `theirs` (the file on disk now). Your text always wins where
//! you changed it; pahiri's changes are kept where you did not:
//!
//! * managed block (`- key: value` lines): per field,
//! * `## Context` and `## Checkpoints` sections: whole section when you did
//!   not touch it, per checkpoint (by title) when you did,
//! * `## Log` and `## Outcome`: new lines from disk are appended.

use super::checkpoints::{self, Checkpoint};
use super::context::{
    TaskMeta, BEGIN, CONTEXT_BEGIN, CONTEXT_END, CONTEXT_HEADING, END, LOG_HEADING, OUTCOME_HEADING,
};
use super::sections::{append_item, extract, section_lines, upsert};

fn pick<T: PartialEq + Clone>(base: &T, ours: &T, theirs: &T) -> T {
    if ours == base {
        theirs.clone()
    } else {
        ours.clone()
    }
}

fn merge_meta(base: &str, ours: &str, theirs: &str) -> TaskMeta {
    let (b, o, t) = (
        TaskMeta::parse(base),
        TaskMeta::parse(ours),
        TaskMeta::parse(theirs),
    );
    TaskMeta {
        source: pick(&b.source, &o.source, &t.source),
        link: pick(&b.link, &o.link, &t.link),
        workspaces: pick(&b.workspaces, &o.workspaces, &t.workspaces),
        builds: pick(&b.builds, &o.builds, &t.builds),
        branch: pick(&b.branch, &o.branch, &t.branch),
        prepared: pick(&b.prepared, &o.prepared, &t.prepared),
        gerrit: pick(&b.gerrit, &o.gerrit, &t.gerrit),
        created: pick(&b.created, &o.created, &t.created),
        started: pick(&b.started, &o.started, &t.started),
        finished: pick(&b.finished, &o.finished, &t.finished),
        time_spent_min: pick(&b.time_spent_min, &o.time_spent_min, &t.time_spent_min),
        context_ready: pick(&b.context_ready, &o.context_ready, &t.context_ready),
    }
}

fn merge_checkpoints(
    base: &[Checkpoint],
    ours: &[Checkpoint],
    theirs: &[Checkpoint],
) -> Vec<Checkpoint> {
    let find = |list: &[Checkpoint], title: &str| list.iter().find(|c| c.title == title).cloned();
    let mut out: Vec<Checkpoint> = ours
        .iter()
        .map(|o| match (find(base, &o.title), find(theirs, &o.title)) {
            (Some(b), Some(t)) if b == *o => t,
            _ => o.clone(),
        })
        .collect();
    for t in theirs {
        let new_on_disk = find(base, &t.title).is_none() && find(ours, &t.title).is_none();
        if new_on_disk {
            out.push(t.clone());
        }
    }
    out
}

/// Merge `theirs` (disk) into `ours` (editor) relative to `base`.
pub fn merge(base: &str, ours: &str, theirs: &str) -> String {
    if theirs == base {
        return ours.to_owned();
    }
    let mut out = ours.to_owned();

    // Managed block.
    let meta = merge_meta(base, ours, theirs);
    if meta != TaskMeta::parse(ours) || (!ours.contains(BEGIN) && theirs.contains(BEGIN)) {
        out = meta.apply(&out);
    }

    // Generated context.
    let (b, o, t) = (
        extract(base, CONTEXT_BEGIN, CONTEXT_END),
        extract(ours, CONTEXT_BEGIN, CONTEXT_END),
        extract(theirs, CONTEXT_BEGIN, CONTEXT_END),
    );
    if o == b && t != b {
        if let Some(t) = t {
            out = upsert(
                &out,
                CONTEXT_HEADING,
                CONTEXT_BEGIN,
                CONTEXT_END,
                t.trim_matches('\n'),
                &[
                    "## Notes",
                    checkpoints::HEADING,
                    LOG_HEADING,
                    "## Attachments",
                ],
            );
        }
    }

    // Checkpoints.
    let (b, o, t) = (
        checkpoints::parse_section(base),
        checkpoints::parse_section(ours),
        checkpoints::parse_section(theirs),
    );
    if t != b {
        let merged = if o == b {
            t
        } else {
            merge_checkpoints(&b, &o, &t)
        };
        if merged != o
            || (!ours.contains(checkpoints::BEGIN) && theirs.contains(checkpoints::BEGIN))
        {
            out = checkpoints::apply(&out, &merged);
        }
    }

    // Append-only lists.
    for heading in [OUTCOME_HEADING, LOG_HEADING] {
        let base_lines = section_lines(base, heading);
        let our_lines = section_lines(&out, heading);
        let new: Vec<String> = section_lines(theirs, heading)
            .into_iter()
            .filter(|l| !base_lines.contains(l) && !our_lines.contains(l))
            .map(|l| l.trim_start_matches("- ").to_owned())
            .collect();
        for line in new {
            out = append_item(&out, heading, &line, &["## Attachments"]);
        }
    }
    if !out.contains(END) && theirs.contains(END) {
        out = meta.apply(&out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "# T\n\n## Notes\n\nhello\n\n## Checkpoints\n<!-- pahiri:checkpoints -->\n- [ ] A (10m)\n- [ ] B (20m)\n<!-- /pahiri:checkpoints -->\n\n## Log\n- 2026-01-01 10:00 first\n\n## Attachments\n<!-- pahiri:begin -->\n- branch: T\n- time_spent: 5m\n<!-- pahiri:end -->\n";

    #[test]
    fn keeps_user_prose_and_takes_pahiri_updates() {
        let ours = BASE.replace("hello", "hello world");
        let theirs = BASE
            .replace("- [ ] A (10m)", "- [x] A (10m; spent 12m)")
            .replace("- time_spent: 5m", "- time_spent: 17m")
            .replace(
                "- 2026-01-01 10:00 first\n",
                "- 2026-01-01 10:00 first\n- 2026-01-01 11:00 ✓ A\n",
            );
        let m = merge(BASE, &ours, &theirs);
        assert!(m.contains("hello world"), "{m}");
        assert!(m.contains("- [x] A (10m; spent 12m)"), "{m}");
        assert!(m.contains("- time_spent: 17m"), "{m}");
        assert!(
            m.contains("- 2026-01-01 10:00 first\n- 2026-01-01 11:00 ✓ A\n"),
            "{m}"
        );
        // Nothing changed on disk: the buffer wins as is.
        assert_eq!(merge(BASE, &ours, BASE), ours);
    }

    #[test]
    fn user_edits_to_managed_parts_win_per_field_and_item() {
        let ours = BASE
            .replace("- branch: T", "- branch: mine")
            .replace("- [ ] B (20m)", "- [ ] B renamed (25m)");
        let theirs = BASE
            .replace("- time_spent: 5m", "- time_spent: 9m")
            .replace("- [ ] A (10m)", "- [ ] A (10m; spent 4m)")
            .replace(
                "<!-- /pahiri:checkpoints -->",
                "- [ ] C (5m)\n<!-- /pahiri:checkpoints -->",
            );
        let m = merge(BASE, &ours, &theirs);
        assert!(m.contains("- branch: mine"), "{m}");
        assert!(m.contains("- time_spent: 9m"), "{m}");
        assert!(
            m.contains("- [ ] A (10m; spent 4m)\n- [ ] B renamed (25m)\n- [ ] C (5m)"),
            "{m}"
        );
    }

    #[test]
    fn sections_added_on_disk_are_added() {
        let theirs = upsert(
            BASE,
            CONTEXT_HEADING,
            CONTEXT_BEGIN,
            CONTEXT_END,
            "### Goal\n- g",
            &["## Notes"],
        );
        let ours = BASE.replace("hello", "hi");
        let m = merge(BASE, &ours, &theirs);
        assert!(m.contains("### Goal\n- g"), "{m}");
        assert!(m.contains("hi\n"));
    }
}
