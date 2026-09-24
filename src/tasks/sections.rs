//! Helpers for pahiri-owned sections of `CONTEXT.md`.
//!
//! A section is a `## Heading` followed by content between two HTML comment
//! markers. pahiri only ever rewrites what is between its markers; everything
//! else in the file belongs to the user.

/// Content between `begin` and `end` markers, if both are present.
pub fn extract<'a>(markdown: &'a str, begin: &str, end: &str) -> Option<&'a str> {
    let b = markdown.find(begin)?;
    let e = markdown[b..].find(end)? + b;
    Some(&markdown[b + begin.len()..e])
}

/// Byte offset of the first `## <heading>` line among `headings`.
fn first_heading(markdown: &str, headings: &[&str]) -> Option<usize> {
    let mut offset = 0;
    for line in markdown.split_inclusive('\n') {
        let t = line.trim_end();
        if headings.contains(&t) {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

/// Replace the marked content, or insert a new `heading` + markers section
/// before the first of `before` (or at the end of the file).
pub fn upsert(
    markdown: &str,
    heading: &str,
    begin: &str,
    end: &str,
    body: &str,
    before: &[&str],
) -> String {
    let mut section = String::from(begin);
    section.push('\n');
    section.push_str(body.trim_end());
    if !body.trim().is_empty() {
        section.push('\n');
    }
    section.push_str(end);
    section.push('\n');

    if let Some(b) = markdown.find(begin) {
        if let Some(rel) = markdown[b..].find(end) {
            let mut after = b + rel + end.len();
            if markdown[after..].starts_with('\n') {
                after += 1;
            }
            return format!("{}{section}{}", &markdown[..b], &markdown[after..]);
        }
    }
    let block = format!("{heading}\n{section}");
    if let Some(at) = first_heading(markdown, before) {
        return format!("{}{block}\n{}", &markdown[..at], &markdown[at..]);
    }
    let mut out = markdown.to_owned();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(&block);
    out
}

/// Append a list item under `heading`: after the section's last line when the
/// heading exists, else as a new section before the first of `before` (or at the end).
pub fn append_item(markdown: &str, heading: &str, item: &str, before: &[&str]) -> String {
    let entry = format!("- {item}\n");
    if let Some(start) = first_heading(markdown, &[heading]) {
        let body_start = start
            + markdown[start..]
                .find('\n')
                .map_or(markdown.len() - start, |i| i + 1);
        // The section ends at the next heading or managed block.
        let mut end = markdown.len();
        let mut offset = body_start;
        for line in markdown[body_start..].split_inclusive('\n') {
            let t = line.trim_start();
            if t.starts_with("## ") || t.starts_with("# ") || t.starts_with("<!-- pahiri:") {
                end = offset;
                break;
            }
            offset += line.len();
        }
        // Insert after the last non-blank line of the section.
        let section = &markdown[body_start..end];
        let content_len = section.trim_end_matches(['\n', ' ']).len();
        let at = if content_len == 0 {
            body_start
        } else {
            body_start + content_len + 1
        };
        let mut out = String::with_capacity(markdown.len() + entry.len() + 1);
        out.push_str(&markdown[..at.min(markdown.len())]);
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&entry);
        out.push_str(&markdown[at.min(markdown.len())..]);
        return out;
    }
    let block = format!("{heading}\n{entry}");
    if let Some(at) = first_heading(markdown, before) {
        return format!("{}{block}\n{}", &markdown[..at], &markdown[at..]);
    }
    let mut out = markdown.to_owned();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(&block);
    out
}

/// Replace the body of a plain `heading` section (up to the next `#`/`##`
/// heading or pahiri marker), or insert it before the first of `before`.
pub fn replace_plain(markdown: &str, heading: &str, body: &str, before: &[&str]) -> String {
    let block_body = format!("\n{}\n\n", body.trim_end());
    if let Some(start) = first_heading(markdown, &[heading]) {
        let body_start = start
            + markdown[start..]
                .find('\n')
                .map_or(markdown.len() - start, |i| i + 1);
        let mut end = markdown.len();
        let mut offset = body_start;
        for line in markdown[body_start..].split_inclusive('\n') {
            let t = line.trim_start();
            if t.starts_with("## ") || t.starts_with("# ") || t.starts_with("<!-- pahiri:") {
                end = offset;
                break;
            }
            offset += line.len();
        }
        return format!(
            "{}{block_body}{}",
            &markdown[..body_start],
            &markdown[end..]
        );
    }
    let block = format!("{heading}\n{block_body}");
    if let Some(at) = first_heading(markdown, before) {
        return format!("{}{block}{}", &markdown[..at], &markdown[at..]);
    }
    let mut out = markdown.to_owned();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(&block);
    out
}

/// Lines of a plain `## heading` section (without the heading), trimmed, non-empty.
pub fn section_lines<'a>(markdown: &'a str, heading: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in markdown.lines() {
        let t = line.trim();
        if t == heading {
            inside = true;
            continue;
        }
        if inside {
            if t.starts_with("## ") || t.starts_with("# ") || t.starts_with("<!-- pahiri:") {
                break;
            }
            if !t.is_empty() {
                out.push(t);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: &str = "<!-- x -->";
    const E: &str = "<!-- /x -->";

    #[test]
    fn upsert_inserts_before_and_replaces() {
        let md = "# T\n\n## Notes\n\nhi\n\n## Attachments\nblock\n";
        let out = upsert(md, "## X", B, E, "body", &["## Notes", "## Attachments"]);
        assert_eq!(out, "# T\n\n## X\n<!-- x -->\nbody\n<!-- /x -->\n\n## Notes\n\nhi\n\n## Attachments\nblock\n");
        let again = upsert(&out, "## X", B, E, "new\nlines", &[]);
        assert!(
            again.contains("<!-- x -->\nnew\nlines\n<!-- /x -->\n\n## Notes"),
            "{again}"
        );
        assert_eq!(extract(&again, B, E), Some("\nnew\nlines\n"));
        let plain = upsert("# T", "## X", B, E, "b", &["## Nope"]);
        assert_eq!(plain, "# T\n\n## X\n<!-- x -->\nb\n<!-- /x -->\n");
    }

    #[test]
    fn replace_plain_sections() {
        let md = "# T\n\n## Description\n\nold\nlines\n\n## Notes\n\nmine\n";
        assert_eq!(
            replace_plain(md, "## Description", "new", &[]),
            "# T\n\n## Description\n\nnew\n\n## Notes\n\nmine\n"
        );
        assert_eq!(
            replace_plain("# T\n\n## Notes\n", "## Description", "d", &["## Notes"]),
            "# T\n\n## Description\n\nd\n\n## Notes\n"
        );
    }

    #[test]
    fn append_item_variants() {
        // New section before attachments.
        let md = "# T\n\n## Notes\n\nhi\n\n## Attachments\nblock\n";
        let out = append_item(md, "## Log", "a", &["## Attachments"]);
        assert_eq!(
            out,
            "# T\n\n## Notes\n\nhi\n\n## Log\n- a\n\n## Attachments\nblock\n"
        );
        // Existing section: appended after its last item, blank line kept.
        let out = append_item(&out, "## Log", "b", &["## Attachments"]);
        assert_eq!(
            out,
            "# T\n\n## Notes\n\nhi\n\n## Log\n- a\n- b\n\n## Attachments\nblock\n"
        );
        // Section at the end of the file.
        let out = append_item("# T\n\n## Log\n- a\n", "## Log", "b", &[]);
        assert_eq!(out, "# T\n\n## Log\n- a\n- b\n");
        // Empty section.
        let out = append_item("# T\n\n## Log\n\n## Next\n", "## Log", "a", &[]);
        assert_eq!(out, "# T\n\n## Log\n- a\n\n## Next\n");
        // No heading, no anchor: appended at the end.
        assert_eq!(
            append_item("# T\n", "## Log", "a", &[]),
            "# T\n\n## Log\n- a\n"
        );
        assert_eq!(
            section_lines("# T\n## Log\n- a\n\n- b\n## X\n- c\n", "## Log"),
            vec!["- a", "- b"]
        );
    }
}
