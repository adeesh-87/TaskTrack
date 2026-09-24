//! Log file highlighting: timestamps, levels, tags, key=value pairs.

use super::{Builder, Kind, Span};

fn level_of(token: &str) -> Option<Kind> {
    let t = token
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_ascii_uppercase();
    match t.as_str() {
        "ERROR" | "ERR" | "FATAL" | "CRITICAL" | "CRIT" | "PANIC" | "FAIL" | "FAILED"
        | "FAILURE" | "EXCEPTION" | "SEVERE" | "EMERG" | "ALERT" => Some(Kind::Error),
        "WARN" | "WARNING" => Some(Kind::Warn),
        "INFO" | "NOTICE" | "OK" | "SUCCESS" | "PASS" | "PASSED" => Some(Kind::Info),
        "DEBUG" | "TRACE" | "VERBOSE" | "DBG" => Some(Kind::Debug),
        _ => None,
    }
}

fn looks_like_time(token: &str) -> bool {
    let digits = token.chars().filter(char::is_ascii_digit).count();
    let seps = token
        .chars()
        .filter(|c| matches!(c, ':' | '-' | '.' | '/' | 'T' | 'Z' | ',' | '+'))
        .count();
    digits >= 4
        && seps >= 1
        && token.chars().all(|c| {
            c.is_ascii_digit()
                || matches!(c, ':' | '-' | '.' | '/' | 'T' | 'Z' | ',' | '+' | '[' | ']')
        })
}

fn looks_like_number(token: &str) -> bool {
    let t = token.trim_matches(|c: char| matches!(c, ',' | ';' | ')' | '(' | ']' | '['));
    !t.is_empty()
        && (t
            .strip_prefix("0x")
            .or_else(|| t.strip_prefix("0X"))
            .is_some_and(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit()))
            || t.chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
                && t.chars().any(|c| c.is_ascii_digit()))
}

pub(crate) fn lex(chars: &[char]) -> Vec<Span> {
    let mut b = Builder::default();
    let n = chars.len();
    let mut i = 0;
    let mut line_level: Option<Kind> = None;

    // dmesg style "[   12.345678]" prefix.
    if n > 2 && chars[0] == '[' {
        if let Some(close) = chars.iter().position(|c| *c == ']') {
            let inner: String = chars[1..close].iter().collect();
            if looks_like_time(inner.trim())
                || inner.trim().chars().all(|c| c.is_ascii_digit() || c == '.')
                    && inner.trim().chars().any(|c| c.is_ascii_digit())
            {
                b.push(0, close + 1, Kind::Timestamp);
                i = close + 1;
            }
        }
    }

    while i < n {
        let c = chars[i];
        if c.is_whitespace() {
            b.push(i, i + 1, Kind::Text);
            i += 1;
            continue;
        }
        // Quoted strings.
        if c == '"' || c == '\'' {
            let mut j = i + 1;
            while j < n && chars[j] != c {
                j += 1;
            }
            let end = (j + 1).min(n);
            b.push(i, end, Kind::String);
            i = end;
            continue;
        }
        // Bracketed tags: [main], <kernel>, (pid=12).
        if matches!(c, '[' | '<') {
            let closer = if c == '[' { ']' } else { '>' };
            if let Some(p) = chars[i..].iter().position(|ch| *ch == closer) {
                let end = i + p + 1;
                let inner: String = chars[i + 1..end - 1].iter().collect();
                let kind = level_of(&inner).unwrap_or(if looks_like_time(&inner) {
                    Kind::Timestamp
                } else {
                    Kind::Label
                });
                if matches!(kind, Kind::Error | Kind::Warn | Kind::Info | Kind::Debug)
                    && line_level.is_none()
                {
                    line_level = Some(kind);
                }
                b.push(i, end, kind);
                i = end;
                continue;
            }
        }
        // A whitespace-delimited token.
        let mut j = i;
        while j < n && !chars[j].is_whitespace() {
            j += 1;
        }
        let token: String = chars[i..j].iter().collect();
        if let Some(level) = level_of(&token) {
            if line_level.is_none() {
                line_level = Some(level);
            }
            b.push(i, j, level);
        } else if looks_like_time(&token) {
            b.push(i, j, Kind::Timestamp);
        } else if let Some(eq) = token.find('=').filter(|&p| {
            p > 0
                && token[..p]
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-')
        }) {
            b.push(i, i + eq, Kind::Variable);
            b.push(i + eq, i + eq + 1, Kind::Operator);
            let value = &token[eq + 1..];
            let kind = if looks_like_number(value) {
                Kind::Number
            } else if value.starts_with('"') || value.starts_with('\'') {
                Kind::String
            } else {
                line_level.unwrap_or(Kind::Text)
            };
            b.push(i + eq + 1, j, kind);
        } else if looks_like_number(&token) {
            b.push(i, j, Kind::Number);
        } else {
            b.push(i, j, line_level.unwrap_or(Kind::Text));
        }
        i = j;
    }
    b.finish()
}

#[cfg(test)]
mod tests {
    use super::super::{kinds, Kind, Language};

    #[test]
    fn levels_timestamps_and_tags() {
        let k = kinds(
            Language::Log,
            "2026-09-24T10:00:01.123Z [worker-1] ERROR request failed code=500 id=\"abc\"",
        );
        assert_eq!(k[0], ("2026-09-24T10:00:01.123Z".into(), Kind::Timestamp));
        assert!(k.contains(&("[worker-1]".into(), Kind::Label)));
        assert!(k.contains(&("ERROR".into(), Kind::Error)));
        assert!(k.contains(&("request".into(), Kind::Error)), "{k:?}");
        assert!(k.contains(&("code".into(), Kind::Variable)));
        assert!(k.contains(&("500".into(), Kind::Number)));
        assert!(k.contains(&("\"abc\"".into(), Kind::String)));
    }

    #[test]
    fn dmesg_and_bracketed_levels() {
        let k = kinds(Language::Log, "[   12.345678] usb 1-1: new device");
        assert_eq!(k[0], ("[   12.345678]".into(), Kind::Timestamp));
        let k = kinds(Language::Log, "10:22:01 [WARN] disk 91% full");
        assert!(k.contains(&("10:22:01".into(), Kind::Timestamp)));
        assert!(k.contains(&("[WARN]".into(), Kind::Warn)));
        assert!(k.contains(&("disk".into(), Kind::Warn)));
        let k = kinds(Language::Log, "INFO: started 0x1f");
        assert!(k.contains(&("INFO:".into(), Kind::Info)));
        assert!(k.contains(&("0x1f".into(), Kind::Number)));
        let k = kinds(Language::Log, "DEBUG ok");
        assert_eq!(k[0].1, Kind::Debug);
    }
}
