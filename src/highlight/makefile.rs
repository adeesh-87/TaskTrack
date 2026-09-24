//! Makefile highlighting: targets, variables, directives, recipes.

use super::{is_ident, Builder, Kind, Span};

const DIRECTIVES: &[&str] = &[
    "ifeq", "ifneq", "ifdef", "ifndef", "else", "endif", "include", "-include", "sinclude",
    "define", "endef", "export", "unexport", "override", "vpath", "undefine", "private",
];

fn lex_variables(chars: &[char], b: &mut Builder, from: usize, to: usize, default: Kind) {
    let mut i = from;
    while i < to {
        if chars[i] == '$' && i + 1 < to {
            let open = chars[i + 1];
            if open == '(' || open == '{' {
                let close = if open == '(' { ')' } else { '}' };
                let mut depth = 0;
                let mut j = i + 1;
                while j < to {
                    if chars[j] == open {
                        depth += 1;
                    } else if chars[j] == close {
                        depth -= 1;
                        if depth == 0 {
                            j += 1;
                            break;
                        }
                    }
                    j += 1;
                }
                b.push(i, j, Kind::Variable);
                i = j;
                continue;
            }
            b.push(i, i + 2, Kind::Variable);
            i += 2;
            continue;
        }
        if chars[i] == '#' {
            b.push(i, to, Kind::Comment);
            return;
        }
        b.push(i, i + 1, default);
        i += 1;
    }
}

pub(crate) fn lex(chars: &[char]) -> Vec<Span> {
    let mut b = Builder::default();
    let n = chars.len();
    if n == 0 {
        return b.finish();
    }
    // Recipe line.
    if chars[0] == '\t' {
        let mut i = 1;
        while i < n && matches!(chars[i], '@' | '-' | '+' | ' ') {
            if chars[i] != ' ' {
                b.push(i, i + 1, Kind::Operator);
            }
            i += 1;
        }
        lex_variables(chars, &mut b, i, n, Kind::Text);
        return b.finish();
    }
    let first = chars.iter().position(|c| !c.is_whitespace()).unwrap_or(n);
    if first >= n {
        return b.finish();
    }
    if chars[first] == '#' {
        b.push(first, n, Kind::Comment);
        return b.finish();
    }
    // Directive.
    let mut w = first;
    while w < n && (is_ident(chars[w]) || chars[w] == '-') {
        w += 1;
    }
    let word: String = chars[first..w].iter().collect();
    if DIRECTIVES.contains(&word.as_str()) {
        b.push(first, w, Kind::Keyword);
        lex_variables(chars, &mut b, w, n, Kind::Text);
        return b.finish();
    }
    // Assignment: NAME = / := / ?= / += / !=
    let assign = chars.iter().enumerate().skip(first).find(|(i, c)| {
        **c == '=' || (matches!(c, ':' | '?' | '+' | '!') && chars.get(i + 1) == Some(&'='))
    });
    let colon = chars.iter().position(|c| *c == ':');
    let is_assign = match (assign, colon) {
        (Some((a, _)), Some(c)) => a <= c,
        (Some(_), None) => true,
        _ => false,
    };
    if is_assign {
        let (a, _) = assign.expect("checked");
        let mut end_name = a;
        while end_name > first && chars[end_name - 1].is_whitespace() {
            end_name -= 1;
        }
        b.push(first, end_name, Kind::Variable);
        let op_len = if chars[a] == '=' { 1 } else { 2 };
        b.push(end_name, a, Kind::Text);
        b.push(a, a + op_len, Kind::Operator);
        lex_variables(chars, &mut b, a + op_len, n, Kind::Text);
        return b.finish();
    }
    // Target line.
    if let Some(c) = colon {
        let mut end_targets = c;
        while end_targets > first && chars[end_targets - 1].is_whitespace() {
            end_targets -= 1;
        }
        lex_variables(chars, &mut b, first, end_targets, Kind::Label);
        b.push(end_targets, c, Kind::Text);
        let op_end = if chars.get(c + 1) == Some(&':') {
            c + 2
        } else {
            c + 1
        };
        b.push(c, op_end, Kind::Operator);
        lex_variables(chars, &mut b, op_end, n, Kind::Text);
        return b.finish();
    }
    lex_variables(chars, &mut b, first, n, Kind::Text);
    b.finish()
}

#[cfg(test)]
mod tests {
    use super::super::{kinds, Kind, Language};

    #[test]
    fn targets_variables_and_recipes() {
        let k = kinds(Language::Makefile, "CC ?= gcc # compiler");
        assert_eq!(k[0], ("CC".into(), Kind::Variable));
        assert_eq!(k[1], ("?=".into(), Kind::Operator));
        assert!(k.contains(&("# compiler".into(), Kind::Comment)));
        let k = kinds(Language::Makefile, "build/%.o: src/%.c $(HEADERS)");
        assert_eq!(k[0], ("build/%.o".into(), Kind::Label));
        assert!(k.contains(&(":".into(), Kind::Operator)));
        assert!(k.contains(&("$(HEADERS)".into(), Kind::Variable)));
        let k = kinds(Language::Makefile, "\t@$(CC) -o $@ $< ${CFLAGS}");
        assert_eq!(k[0], ("@".into(), Kind::Operator));
        assert!(k.contains(&("$(CC)".into(), Kind::Variable)));
        assert!(k.contains(&("$@".into(), Kind::Variable)));
        assert!(k.contains(&("$<".into(), Kind::Variable)));
        assert!(k.contains(&("${CFLAGS}".into(), Kind::Variable)));
        let k = kinds(Language::Makefile, "ifeq ($(OS),Linux)");
        assert_eq!(k[0], ("ifeq".into(), Kind::Keyword));
        let k = kinds(Language::Makefile, ".PHONY: all clean");
        assert_eq!(k[0], (".PHONY".into(), Kind::Label));
    }
}
