//! A configurable lexer for C-like languages (C, C++, Rust, Python, Bash, `CMake`).

use super::{find, is_ident, is_ident_start, starts_with, Builder, Kind, Span, State};

/// Per-language configuration.
pub(crate) struct Spec {
    line_comments: &'static [&'static str],
    /// `#` comments only count at a word boundary (shell: `$#` is not a comment).
    comment_needs_boundary: bool,
    block_comment: Option<(&'static str, &'static str)>,
    keywords: &'static [&'static str],
    types: &'static [&'static str],
    constants: &'static [&'static str],
    /// `#directive` at line start.
    preprocessor: bool,
    /// `#[...]` / `#![...]` attributes and `'a` lifetimes (Rust).
    rust_extras: bool,
    /// `'x'` is a char literal (C-like) rather than a string (shell).
    char_literals: bool,
    /// `'...'` is a string.
    single_quote_strings: bool,
    /// Python triple-quoted strings.
    triple_strings: bool,
    /// `@decorator`.
    decorators: bool,
    /// `$VAR`, `${VAR}`, `$(...)`.
    dollar_variables: bool,
    /// `name!` is a macro.
    bang_macros: bool,
    /// Identifiers followed by `(` are functions.
    calls: bool,
    /// Capitalised identifiers are types.
    capitalised_types: bool,
    /// `ALL_CAPS` identifiers are constants.
    caps_constants: bool,
    /// Backticks are strings (shell).
    backtick_strings: bool,
}

pub(crate) const C: Spec = Spec {
    line_comments: &["//"],
    comment_needs_boundary: false,
    block_comment: Some(("/*", "*/")),
    keywords: &[
        "auto",
        "break",
        "case",
        "const",
        "continue",
        "default",
        "do",
        "else",
        "enum",
        "extern",
        "for",
        "goto",
        "if",
        "inline",
        "register",
        "restrict",
        "return",
        "sizeof",
        "static",
        "struct",
        "switch",
        "typedef",
        "union",
        "volatile",
        "while",
        "_Alignas",
        "_Alignof",
        "_Atomic",
        "_Generic",
        "_Noreturn",
        "_Static_assert",
        "_Thread_local",
        "__attribute__",
        "__asm__",
        "asm",
    ],
    types: &[
        "char",
        "double",
        "float",
        "int",
        "long",
        "short",
        "signed",
        "unsigned",
        "void",
        "_Bool",
        "bool",
        "size_t",
        "ssize_t",
        "int8_t",
        "int16_t",
        "int32_t",
        "int64_t",
        "uint8_t",
        "uint16_t",
        "uint32_t",
        "uint64_t",
        "uintptr_t",
        "intptr_t",
        "ptrdiff_t",
        "FILE",
        "wchar_t",
    ],
    constants: &["NULL", "true", "false", "EOF"],
    preprocessor: true,
    rust_extras: false,
    char_literals: true,
    single_quote_strings: false,
    triple_strings: false,
    decorators: false,
    dollar_variables: false,
    bang_macros: false,
    calls: true,
    capitalised_types: false,
    caps_constants: true,
    backtick_strings: false,
};

pub(crate) const CPP: Spec = Spec {
    keywords: &[
        "alignas",
        "alignof",
        "and",
        "and_eq",
        "asm",
        "auto",
        "bitand",
        "bitor",
        "break",
        "case",
        "catch",
        "class",
        "compl",
        "concept",
        "const",
        "consteval",
        "constexpr",
        "constinit",
        "const_cast",
        "continue",
        "co_await",
        "co_return",
        "co_yield",
        "decltype",
        "default",
        "delete",
        "do",
        "dynamic_cast",
        "else",
        "enum",
        "explicit",
        "export",
        "extern",
        "final",
        "for",
        "friend",
        "goto",
        "if",
        "inline",
        "mutable",
        "namespace",
        "new",
        "noexcept",
        "not",
        "not_eq",
        "operator",
        "or",
        "or_eq",
        "override",
        "private",
        "protected",
        "public",
        "register",
        "reinterpret_cast",
        "requires",
        "return",
        "sizeof",
        "static",
        "static_assert",
        "static_cast",
        "struct",
        "switch",
        "template",
        "this",
        "thread_local",
        "throw",
        "try",
        "typedef",
        "typeid",
        "typename",
        "union",
        "using",
        "virtual",
        "volatile",
        "while",
        "xor",
        "xor_eq",
    ],
    types: &[
        "bool",
        "char",
        "char8_t",
        "char16_t",
        "char32_t",
        "double",
        "float",
        "int",
        "long",
        "short",
        "signed",
        "unsigned",
        "void",
        "wchar_t",
        "size_t",
        "ssize_t",
        "int8_t",
        "int16_t",
        "int32_t",
        "int64_t",
        "uint8_t",
        "uint16_t",
        "uint32_t",
        "uint64_t",
        "uintptr_t",
        "intptr_t",
        "ptrdiff_t",
        "string",
        "vector",
        "map",
        "unordered_map",
        "set",
        "unordered_set",
        "shared_ptr",
        "unique_ptr",
        "weak_ptr",
        "optional",
        "variant",
        "array",
        "deque",
        "list",
        "pair",
        "tuple",
        "string_view",
    ],
    constants: &["NULL", "nullptr", "true", "false"],
    ..C
};

pub(crate) const RUST: Spec = Spec {
    line_comments: &["//"],
    comment_needs_boundary: false,
    block_comment: Some(("/*", "*/")),
    keywords: &[
        "as",
        "async",
        "await",
        "break",
        "const",
        "continue",
        "crate",
        "dyn",
        "else",
        "enum",
        "extern",
        "fn",
        "for",
        "if",
        "impl",
        "in",
        "let",
        "loop",
        "match",
        "mod",
        "move",
        "mut",
        "pub",
        "ref",
        "return",
        "self",
        "Self",
        "static",
        "struct",
        "super",
        "trait",
        "type",
        "unsafe",
        "use",
        "where",
        "while",
        "union",
        "macro_rules",
    ],
    types: &[
        "bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32",
        "i64", "i128", "isize", "f32", "f64", "String", "Vec", "Option", "Result", "Box", "Rc",
        "Arc", "HashMap", "HashSet", "BTreeMap", "BTreeSet", "PathBuf", "Path",
    ],
    constants: &["true", "false", "None", "Some", "Ok", "Err"],
    preprocessor: false,
    rust_extras: true,
    char_literals: true,
    single_quote_strings: false,
    triple_strings: false,
    decorators: false,
    dollar_variables: false,
    bang_macros: true,
    calls: true,
    capitalised_types: true,
    caps_constants: true,
    backtick_strings: false,
};

pub(crate) const PYTHON: Spec = Spec {
    line_comments: &["#"],
    comment_needs_boundary: false,
    block_comment: None,
    keywords: &[
        "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
        "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is",
        "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with",
        "yield", "match", "case",
    ],
    types: &[
        "int",
        "float",
        "str",
        "bytes",
        "bool",
        "list",
        "dict",
        "set",
        "tuple",
        "object",
        "type",
        "bytearray",
        "frozenset",
        "complex",
        "range",
        "enumerate",
        "zip",
        "map",
        "filter",
        "iter",
        "next",
        "len",
        "print",
        "open",
        "isinstance",
        "super",
        "property",
        "staticmethod",
        "classmethod",
        "Exception",
        "ValueError",
        "TypeError",
        "KeyError",
        "RuntimeError",
        "self",
        "cls",
    ],
    constants: &["True", "False", "None", "Ellipsis", "NotImplemented"],
    preprocessor: false,
    rust_extras: false,
    char_literals: false,
    single_quote_strings: true,
    triple_strings: true,
    decorators: true,
    dollar_variables: false,
    bang_macros: false,
    calls: true,
    capitalised_types: true,
    caps_constants: true,
    backtick_strings: false,
};

pub(crate) const BASH: Spec = Spec {
    line_comments: &["#"],
    comment_needs_boundary: true,
    block_comment: None,
    keywords: &[
        "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac",
        "in", "function", "select", "time", "coproc", "return", "exit", "break", "continue",
        "local", "export", "declare", "typeset", "readonly", "unset", "shift", "source", "alias",
        "set", "eval", "exec", "trap", "let", "setopt", "unsetopt", "autoload",
    ],
    types: &[
        "echo", "printf", "cd", "pwd", "ls", "cp", "mv", "rm", "mkdir", "rmdir", "cat", "grep",
        "sed", "awk", "find", "xargs", "sort", "uniq", "head", "tail", "tr", "cut", "wc", "test",
        "read", "true", "false", "git", "make", "cmake", "cargo", "python3", "python", "sudo",
        "chmod", "chown", "ln", "touch", "tee", "curl", "wget", "tar", "ssh", "scp", "rsync",
        "kill", "sleep", "wait",
    ],
    constants: &[],
    preprocessor: false,
    rust_extras: false,
    char_literals: false,
    single_quote_strings: true,
    triple_strings: false,
    decorators: false,
    dollar_variables: true,
    bang_macros: false,
    calls: false,
    capitalised_types: false,
    caps_constants: false,
    backtick_strings: true,
};

pub(crate) const CMAKE: Spec = Spec {
    line_comments: &["#"],
    comment_needs_boundary: false,
    block_comment: Some(("#[[", "]]")),
    keywords: &[
        "if",
        "elseif",
        "else",
        "endif",
        "foreach",
        "endforeach",
        "while",
        "endwhile",
        "function",
        "endfunction",
        "macro",
        "endmacro",
        "return",
        "break",
        "continue",
        "block",
        "endblock",
    ],
    types: &[],
    constants: &[],
    preprocessor: false,
    rust_extras: false,
    char_literals: false,
    single_quote_strings: false,
    triple_strings: false,
    decorators: false,
    dollar_variables: true,
    bang_macros: false,
    calls: true,
    capitalised_types: false,
    caps_constants: true,
    backtick_strings: false,
};

fn scan_string(chars: &[char], start: usize, quote: char) -> usize {
    let mut i = start + 1;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2,
            c if c == quote => return i + 1,
            _ => i += 1,
        }
    }
    chars.len()
}

fn scan_raw_string(chars: &[char], start: usize) -> Option<usize> {
    // r"..." or r#"..."#
    let mut i = start + 1;
    let mut hashes = 0;
    while i < chars.len() && chars[i] == '#' {
        hashes += 1;
        i += 1;
    }
    if i >= chars.len() || chars[i] != '"' {
        return None;
    }
    i += 1;
    while i < chars.len() {
        if chars[i] == '"'
            && chars[i + 1..]
                .iter()
                .take(hashes)
                .filter(|c| **c == '#')
                .count()
                == hashes
        {
            return Some(i + 1 + hashes);
        }
        i += 1;
    }
    Some(chars.len())
}

fn word(chars: &[char], start: usize, end: usize) -> String {
    chars[start..end].iter().collect()
}

fn is_all_caps(w: &str) -> bool {
    w.len() > 1
        && w.chars().any(|c| c.is_ascii_uppercase())
        && w.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

pub(crate) fn lex(chars: &[char], state: State, spec: &Spec) -> (Vec<Span>, State) {
    let mut b = Builder::default();
    let n = chars.len();
    let mut i = 0;
    let mut state = state;

    // Continue multi-line constructs.
    match state {
        State::BlockComment => {
            let end = spec.block_comment.and_then(|(_, e)| find(chars, 0, e));
            if let Some(e) = end {
                let close = spec.block_comment.map_or(0, |(_, c)| c.chars().count());
                b.push(0, e + close, Kind::Comment);
                i = e + close;
                state = State::Normal;
            } else {
                b.push(0, n, Kind::Comment);
                return (b.finish(), State::BlockComment);
            }
        }
        State::TripleString(q) => {
            let closer: String = [q, q, q].iter().collect();
            if let Some(e) = find(chars, 0, &closer) {
                b.push(0, e + 3, Kind::String);
                i = e + 3;
                state = State::Normal;
            } else {
                b.push(0, n, Kind::String);
                return (b.finish(), state);
            }
        }
        State::Normal | State::Fenced => {}
    }

    let first_non_ws = chars.iter().position(|c| !c.is_whitespace()).unwrap_or(n);
    let mut prev_significant: Option<char> = None;
    let mut prev_word = String::new();

    while i < n {
        let c = chars[i];
        if c.is_whitespace() {
            b.push(i, i + 1, Kind::Text);
            i += 1;
            continue;
        }

        // Block comment start.
        if let Some((open, close)) = spec.block_comment {
            if starts_with(chars, i, open) {
                let open_len = open.chars().count();
                if let Some(e) = find(chars, i + open_len, close) {
                    let end = e + close.chars().count();
                    b.push(i, end, Kind::Comment);
                    i = end;
                } else {
                    b.push(i, n, Kind::Comment);
                    return (b.finish(), State::BlockComment);
                }
                continue;
            }
        }

        // Line comment.
        let boundary_ok = !spec.comment_needs_boundary
            || i == 0
            || chars[i - 1].is_whitespace()
            || matches!(chars[i - 1], ';' | '(' | '{' | '|' | '&');
        if boundary_ok
            && spec
                .line_comments
                .iter()
                .any(|lc| starts_with(chars, i, lc))
        {
            let is_preproc = spec.preprocessor && c == '#' && i == first_non_ws;
            if !is_preproc {
                b.push(i, n, Kind::Comment);
                return (b.finish(), State::Normal);
            }
        }

        // Preprocessor directive.
        if spec.preprocessor && c == '#' && i == first_non_ws {
            let mut j = i + 1;
            while j < n && chars[j].is_whitespace() {
                j += 1;
            }
            while j < n && is_ident(chars[j]) {
                j += 1;
            }
            b.push(i, j, Kind::Preprocessor);
            i = j;
            // `#include <...>` header name.
            let mut k = i;
            while k < n && chars[k].is_whitespace() {
                k += 1;
            }
            if k < n && chars[k] == '<' {
                let end = chars[k..]
                    .iter()
                    .position(|c| *c == '>')
                    .map_or(n, |p| k + p + 1);
                b.push(i, k, Kind::Text);
                b.push(k, end, Kind::String);
                i = end;
            }
            continue;
        }

        // Rust attributes and lifetimes.
        if spec.rust_extras
            && c == '#'
            && (starts_with(chars, i, "#[") || starts_with(chars, i, "#!["))
        {
            let mut depth = 0i32;
            let mut j = i;
            while j < n {
                match chars[j] {
                    '[' => depth += 1,
                    ']' => {
                        depth -= 1;
                        if depth == 0 {
                            j += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            b.push(i, j, Kind::Attribute);
            i = j;
            continue;
        }

        // Decorators.
        if spec.decorators && c == '@' && i == first_non_ws {
            let mut j = i + 1;
            while j < n && (is_ident(chars[j]) || chars[j] == '.') {
                j += 1;
            }
            b.push(i, j, Kind::Attribute);
            i = j;
            continue;
        }

        // Triple-quoted strings.
        if spec.triple_strings
            && (c == '"' || c == '\'')
            && starts_with(chars, i, &format!("{c}{c}{c}"))
        {
            let closer = format!("{c}{c}{c}");
            if let Some(e) = find(chars, i + 3, &closer) {
                b.push(i, e + 3, Kind::String);
                i = e + 3;
            } else {
                b.push(i, n, Kind::String);
                return (b.finish(), State::TripleString(c));
            }
            continue;
        }

        // Raw strings (Rust) and prefixed strings (Python b"", f"", r"").
        if (spec.rust_extras || spec.triple_strings) && is_ident_start(c) {
            let mut j = i;
            while j < n && chars[j].is_ascii_alphabetic() && j - i < 2 {
                j += 1;
            }
            if j < n && (chars[j] == '"' || chars[j] == '#' && spec.rust_extras) {
                let prefix = word(chars, i, j).to_ascii_lowercase();
                if spec.rust_extras && prefix == "r" {
                    if let Some(end) = scan_raw_string(chars, i) {
                        b.push(i, end, Kind::String);
                        i = end;
                        continue;
                    }
                } else if spec.triple_strings
                    && chars[j] == '"'
                    && prefix.chars().all(|p| matches!(p, 'b' | 'f' | 'r' | 'u'))
                {
                    let end = scan_string(chars, j, '"');
                    b.push(i, end, Kind::String);
                    i = end;
                    continue;
                }
            }
        }

        // Strings.
        if c == '"' || (c == '`' && spec.backtick_strings) {
            let end = scan_string(chars, i, c);
            b.push(i, end, Kind::String);
            i = end;
            continue;
        }
        if c == '\'' {
            if spec.single_quote_strings {
                let end = scan_string(chars, i, '\'');
                b.push(i, end, Kind::String);
                i = end;
                continue;
            }
            if spec.char_literals {
                // 'a', '\n', '\x41' are chars; 'a (Rust) is a lifetime.
                let is_char = (i + 2 < n && chars[i + 1] != '\\' && chars[i + 2] == '\'')
                    || (i + 1 < n && chars[i + 1] == '\\');
                if is_char {
                    let end = scan_string(chars, i, '\'');
                    b.push(i, end, Kind::String);
                    i = end;
                    continue;
                }
                if spec.rust_extras {
                    let mut j = i + 1;
                    while j < n && is_ident(chars[j]) {
                        j += 1;
                    }
                    b.push(i, j, Kind::Attribute);
                    i = j;
                    continue;
                }
            }
        }

        // Dollar variables.
        if spec.dollar_variables && c == '$' {
            let mut j = i + 1;
            if j < n && chars[j] == '{' {
                let end = chars[j..]
                    .iter()
                    .position(|c| *c == '}')
                    .map_or(n, |p| j + p + 1);
                b.push(i, end, Kind::Variable);
                i = end;
                continue;
            }
            if j < n && chars[j] == '<' && !spec.backtick_strings {
                // CMake generator expression $<...>
                let end = chars[j..]
                    .iter()
                    .position(|c| *c == '>')
                    .map_or(n, |p| j + p + 1);
                b.push(i, end, Kind::Variable);
                i = end;
                continue;
            }
            if j < n
                && (is_ident(chars[j])
                    || matches!(chars[j], '@' | '#' | '?' | '!' | '*' | '$' | '-'))
            {
                if is_ident(chars[j]) {
                    while j < n && is_ident(chars[j]) {
                        j += 1;
                    }
                } else {
                    j += 1;
                }
                b.push(i, j, Kind::Variable);
                i = j;
                continue;
            }
            b.push(i, i + 1, Kind::Operator);
            i += 1;
            continue;
        }

        // Numbers.
        if c.is_ascii_digit()
            || (c == '.'
                && i + 1 < n
                && chars[i + 1].is_ascii_digit()
                && !matches!(prev_significant, Some(ch) if is_ident(ch)))
        {
            let mut j = i + 1;
            while j < n && (chars[j].is_ascii_alphanumeric() || chars[j] == '_' || chars[j] == '.')
            {
                j += 1;
            }
            b.push(i, j, Kind::Number);
            prev_significant = Some('0');
            i = j;
            continue;
        }

        // Identifiers.
        if is_ident_start(c) {
            let mut j = i + 1;
            while j < n && is_ident(chars[j]) {
                j += 1;
            }
            let w = word(chars, i, j);
            let mut k = j;
            while k < n && chars[k].is_whitespace() {
                k += 1;
            }
            let next = chars.get(k).copied();
            let is_const =
                spec.constants.contains(&w.as_str()) || (spec.caps_constants && is_all_caps(&w));
            let is_fn = matches!(
                prev_word.as_str(),
                "fn" | "def" | "function" | "macro_rules"
            ) || (spec.bang_macros && next == Some('!') && k == j)
                || (spec.calls && next == Some('('));
            let after_type_kw = matches!(
                prev_word.as_str(),
                "class" | "struct" | "enum" | "trait" | "union" | "namespace" | "impl" | "mod"
            ) && !spec.dollar_variables;
            let is_type = after_type_kw
                || spec.types.contains(&w.as_str())
                || (spec.capitalised_types && w.chars().next().is_some_and(char::is_uppercase));
            let kind = if spec.keywords.contains(&w.as_str()) {
                Kind::Keyword
            } else if is_const {
                Kind::Constant
            } else if is_fn {
                Kind::Function
            } else if is_type {
                Kind::Type
            } else {
                Kind::Text
            };
            b.push(i, j, kind);
            prev_significant = Some('a');
            prev_word = w;
            i = j;
            continue;
        }

        // Everything else: operators and punctuation.
        let kind = if "+-*/%=<>!&|^~?:.,;(){}[]".contains(c) {
            Kind::Operator
        } else {
            Kind::Text
        };
        b.push(i, i + 1, kind);
        prev_significant = Some(c);
        i += 1;
    }
    (b.finish(), state)
}

#[cfg(test)]
mod tests {
    use super::super::{kinds, Language, State};
    use super::*;

    #[test]
    fn c_tokens() {
        let k = kinds(Language::C, "#include <stdio.h>");
        assert_eq!(k[0], ("#include".into(), Kind::Preprocessor));
        assert_eq!(k[1], ("<stdio.h>".into(), Kind::String));
        let k = kinds(
            Language::C,
            "static int main(void) { return MAX_N + 0x1F; } // hi",
        );
        assert!(k.contains(&("static".into(), Kind::Keyword)));
        assert!(k.contains(&("int".into(), Kind::Type)));
        assert!(k.contains(&("main".into(), Kind::Function)));
        assert!(k.contains(&("MAX_N".into(), Kind::Constant)));
        assert!(k.contains(&("0x1F".into(), Kind::Number)));
        assert!(k.contains(&("// hi".into(), Kind::Comment)));
        let k = kinds(Language::C, "char c = '\\n'; char *s = \"a \\\" b\";");
        assert!(k.contains(&("'\\n'".into(), Kind::String)));
        assert!(k.contains(&("\"a \\\" b\"".into(), Kind::String)));
    }

    #[test]
    fn cpp_tokens() {
        let k = kinds(
            Language::Cpp,
            "std::vector<std::string> v; auto p = nullptr; /* c */ x",
        );
        assert!(k.contains(&("vector".into(), Kind::Type)));
        assert!(k.contains(&("nullptr".into(), Kind::Constant)));
        assert!(k.contains(&("/* c */".into(), Kind::Comment)));
    }

    #[test]
    fn rust_tokens() {
        let k = kinds(Language::Rust, "#[derive(Debug)] pub fn go<'a>(x: &'a str) -> Option<u8> { println!(\"{x}\"); Some(1) }");
        assert_eq!(k[0], ("#[derive(Debug)]".into(), Kind::Attribute));
        assert!(k.contains(&("pub".into(), Kind::Keyword)));
        assert!(k.contains(&("go".into(), Kind::Function)));
        assert!(k.contains(&("'a".into(), Kind::Attribute)));
        assert!(k.contains(&("Option".into(), Kind::Type)));
        assert!(k.contains(&("println".into(), Kind::Function)));
        assert!(k.contains(&("Some".into(), Kind::Constant)));
        assert!(k.contains(&("\"{x}\"".into(), Kind::String)));
        let k = kinds(
            Language::Rust,
            "let c = 'x'; let r = r#\"raw \" here\"#; let t = MyType;",
        );
        assert!(k.contains(&("'x'".into(), Kind::String)));
        assert!(k.contains(&("r#\"raw \" here\"#".into(), Kind::String)));
        assert!(k.contains(&("MyType".into(), Kind::Type)));
        let k = kinds(Language::Rust, "struct Point; impl Point { fn new() {} }");
        assert!(k.contains(&("Point".into(), Kind::Type)));
        assert!(k.contains(&("new".into(), Kind::Function)));
    }

    #[test]
    fn python_tokens() {
        let k = kinds(Language::Python, "@dataclass\nclass A:");
        assert_eq!(k[0], ("@dataclass".into(), Kind::Attribute));
        let k = kinds(Language::Python, "def f(self, n: int = 3) -> None:  # c");
        assert!(k.contains(&("def".into(), Kind::Keyword)));
        assert!(k.contains(&("f".into(), Kind::Function)));
        assert!(k.contains(&("int".into(), Kind::Type)));
        assert!(k.contains(&("None".into(), Kind::Constant)));
        assert!(k.contains(&("# c".into(), Kind::Comment)));
        let (spans, state) = Language::Python.highlight_line("s = \"\"\"open", State::Normal);
        assert_eq!(state, State::TripleString('"'));
        assert_eq!(spans.last().unwrap().kind, Kind::String);
        let (_, state) = Language::Python.highlight_line("still\"\"\" + f\"x{y}\"", state);
        assert_eq!(state, State::Normal);
        let k = kinds(Language::Python, "x = f\"hi {name}\" + b'raw'");
        assert!(k.contains(&("f\"hi {name}\"".into(), Kind::String)));
        assert!(k.contains(&("'raw'".into(), Kind::String)));
    }

    #[test]
    fn bash_tokens() {
        let k = kinds(
            Language::Bash,
            "if [ -z \"$HOME\" ]; then echo ${X:-1} $# `date`; fi # done",
        );
        assert!(k.contains(&("if".into(), Kind::Keyword)));
        assert!(k.contains(&("echo".into(), Kind::Type)));
        assert!(k.contains(&("${X:-1}".into(), Kind::Variable)));
        assert!(k.contains(&("$#".into(), Kind::Variable)));
        assert!(k.contains(&("`date`".into(), Kind::String)));
        assert!(k.contains(&("# done".into(), Kind::Comment)));
        assert!(k.contains(&("\"$HOME\"".into(), Kind::String)));
        assert!(!k
            .iter()
            .any(|(t, kind)| t == "# done" && *kind != Kind::Comment));
        let k = kinds(Language::Bash, "echo $#args");
        assert!(!k.iter().any(|(_, kind)| *kind == Kind::Comment));
    }

    #[test]
    fn cmake_tokens() {
        let k = kinds(Language::CMake, "if(BUILD_TESTS) target_link_libraries(app PUBLIC ${LIBS} $<TARGET_FILE:x>) endif() # c");
        assert!(k.contains(&("if".into(), Kind::Keyword)));
        assert!(k.contains(&("target_link_libraries".into(), Kind::Function)));
        assert!(k.contains(&("PUBLIC".into(), Kind::Constant)));
        assert!(k.contains(&("${LIBS}".into(), Kind::Variable)));
        assert!(k.contains(&("$<TARGET_FILE:x>".into(), Kind::Variable)));
        assert!(k.contains(&("# c".into(), Kind::Comment)));
    }

    #[test]
    fn block_comment_spans_lines() {
        let (spans, state) = Language::C.highlight_line("a /* open", State::Normal);
        assert_eq!(state, State::BlockComment);
        assert_eq!(spans.last().unwrap().kind, Kind::Comment);
        let (spans, state) = Language::C.highlight_line("end */ int x;", State::BlockComment);
        assert_eq!(state, State::Normal);
        assert_eq!(
            spans[0],
            Span {
                start: 0,
                end: 6,
                kind: Kind::Comment
            }
        );
        assert!(spans.iter().any(|s| s.kind == Kind::Type));
    }
}
