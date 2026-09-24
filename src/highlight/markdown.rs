//! Markdown highlighting: headings, code, links, emphasis, lists, quotes.

use super::{find, starts_with, Builder, Kind, Span, State};

pub(crate) fn lex(chars: &[char], state: State) -> (Vec<Span>, State) {
    let mut b = Builder::default();
    let n = chars.len();
    let trimmed: String = chars.iter().collect::<String>().trim_start().to_owned();
    let indent = n - trimmed.chars().count();

    if state == State::BlockComment {
        return if let Some(e) = find(chars, 0, "-->") {
            b.push(0, e + 3, Kind::Comment);
            lex_inline(chars, &mut b, e + 3, n);
            (b.finish(), State::Normal)
        } else {
            b.push(0, n, Kind::Comment);
            (b.finish(), State::BlockComment)
        };
    }
    if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        b.push(0, n, Kind::Operator);
        let next = if state == State::Fenced {
            State::Normal
        } else {
            State::Fenced
        };
        return (b.finish(), next);
    }
    if state == State::Fenced {
        b.push(0, n, Kind::String);
        return (b.finish(), State::Fenced);
    }
    if trimmed.starts_with('#') {
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if hashes <= 6 && trimmed.chars().nth(hashes).map_or(true, |c| c == ' ') {
            b.push(0, n, Kind::Heading);
            return (b.finish(), State::Normal);
        }
    }
    if trimmed.starts_with('>') {
        b.push(0, n, Kind::Comment);
        return (b.finish(), State::Normal);
    }
    if trimmed.starts_with("<!--") {
        return if let Some(e) = find(chars, 0, "-->") {
            b.push(0, e + 3, Kind::Comment);
            lex_inline(chars, &mut b, e + 3, n);
            (b.finish(), State::Normal)
        } else {
            b.push(0, n, Kind::Comment);
            (b.finish(), State::BlockComment)
        };
    }
    let mut i = indent;
    // List bullets and numbers.
    if i < n && matches!(chars[i], '-' | '*' | '+') && chars.get(i + 1) == Some(&' ') {
        b.push(0, i + 1, Kind::Operator);
        i += 1;
        if starts_with(chars, i, " [ ] ")
            || starts_with(chars, i, " [x] ")
            || starts_with(chars, i, " [X] ")
        {
            b.push(i + 1, i + 4, Kind::Constant);
            i += 4;
        }
    } else {
        let mut j = i;
        while j < n && chars[j].is_ascii_digit() {
            j += 1;
        }
        if j > i && j < n && matches!(chars[j], '.' | ')') && chars.get(j + 1) == Some(&' ') {
            b.push(0, j + 1, Kind::Operator);
            i = j + 1;
        }
    }
    if trimmed
        .chars()
        .all(|c| matches!(c, '-' | '=' | '*' | '_' | ' '))
        && trimmed.chars().filter(|c| !c.is_whitespace()).count() >= 3
    {
        b.push(0, n, Kind::Operator);
        return (b.finish(), State::Normal);
    }
    lex_inline(chars, &mut b, i, n);
    (b.finish(), State::Normal)
}

fn lex_inline(chars: &[char], b: &mut Builder, from: usize, to: usize) {
    let mut i = from;
    while i < to {
        let c = chars[i];
        if c == '`' {
            if let Some(p) = chars[i + 1..to].iter().position(|ch| *ch == '`') {
                let end = i + 1 + p + 1;
                b.push(i, end, Kind::String);
                i = end;
                continue;
            }
        }
        if c == '[' {
            if let Some(p) = chars[i..to].iter().position(|ch| *ch == ']') {
                let close = i + p;
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(q) = chars[close + 1..to].iter().position(|ch| *ch == ')') {
                        let end = close + 1 + q + 1;
                        b.push(i, close + 1, Kind::Label);
                        b.push(close + 1, end, Kind::Variable);
                        i = end;
                        continue;
                    }
                }
            }
        }
        if (c == '*' || c == '_') && i + 1 < to && chars[i + 1] == c {
            let closer = [c, c];
            if let Some(p) = (i + 2..to.saturating_sub(1))
                .find(|&k| chars[k] == closer[0] && chars[k + 1] == closer[1])
            {
                b.push(i, p + 2, Kind::Constant);
                i = p + 2;
                continue;
            }
        }
        if starts_with(chars, i, "http://") || starts_with(chars, i, "https://") {
            let mut j = i;
            while j < to && !chars[j].is_whitespace() && !matches!(chars[j], ')' | '>' | '"') {
                j += 1;
            }
            b.push(i, j, Kind::Variable);
            i = j;
            continue;
        }
        b.push(i, i + 1, Kind::Text);
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::super::{kinds, Kind, Language, State};

    #[test]
    fn headings_lists_links_code() {
        assert_eq!(
            kinds(Language::Markdown, "## Title")[0],
            ("## Title".into(), Kind::Heading)
        );
        let k = kinds(
            Language::Markdown,
            "- [x] see [docs](https://x.y) and `code` **bold**",
        );
        assert_eq!(k[0], ("-".into(), Kind::Operator));
        assert!(k.contains(&("[x]".into(), Kind::Constant)));
        assert!(k.contains(&("[docs]".into(), Kind::Label)));
        assert!(k.contains(&("(https://x.y)".into(), Kind::Variable)));
        assert!(k.contains(&("`code`".into(), Kind::String)));
        assert!(k.contains(&("**bold**".into(), Kind::Constant)));
        let k = kinds(Language::Markdown, "Link: https://jira/PROJ-1 ok");
        assert!(k.contains(&("https://jira/PROJ-1".into(), Kind::Variable)));
        assert_eq!(kinds(Language::Markdown, "> quote")[0].1, Kind::Comment);
        assert_eq!(
            kinds(Language::Markdown, "1. first")[0],
            ("1.".into(), Kind::Operator)
        );
    }

    #[test]
    fn fences_and_html_comments_carry_state() {
        let (_, s) = Language::Markdown.highlight_line("```rust", State::Normal);
        assert_eq!(s, State::Fenced);
        let (spans, s) = Language::Markdown.highlight_line("let x = 1;", s);
        assert_eq!(spans[0].kind, Kind::String);
        assert_eq!(s, State::Fenced);
        let (_, s) = Language::Markdown.highlight_line("```", s);
        assert_eq!(s, State::Normal);
        let (spans, s) = Language::Markdown.highlight_line("<!-- pahiri:begin -->", State::Normal);
        assert_eq!(spans[0].kind, Kind::Comment);
        assert_eq!(s, State::Normal);
        let (_, s) = Language::Markdown.highlight_line("<!-- open", State::Normal);
        assert_eq!(s, State::BlockComment);
    }
}
