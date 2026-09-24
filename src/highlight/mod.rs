//! Dependency-free syntax highlighting for the editor.
//!
//! Each language is a small hand-written lexer producing [`Span`]s over one
//! line, plus a [`State`] carried to the next line for multi-line constructs
//! (block comments, triple-quoted strings, fenced code). This keeps startup
//! instant and the behaviour predictable; it is not a full parser.

mod clike;
mod log;
mod makefile;
mod markdown;

use std::path::Path;

/// Token classes, mapped to colours by the theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Plain text.
    Text,
    /// Language keyword.
    Keyword,
    /// Type name.
    Type,
    /// Constant / enum-like identifier.
    Constant,
    /// String or character literal.
    String,
    /// Comment.
    Comment,
    /// Numeric literal.
    Number,
    /// Preprocessor directive.
    Preprocessor,
    /// Function or macro name.
    Function,
    /// Variable reference such as `$VAR` or `${VAR}`.
    Variable,
    /// Attribute, decorator or lifetime.
    Attribute,
    /// Operator or punctuation.
    Operator,
    /// Heading (Markdown).
    Heading,
    /// Label: Makefile target, log tag, Markdown link text.
    Label,
    /// Log: error level.
    Error,
    /// Log: warning level.
    Warn,
    /// Log: info level.
    Info,
    /// Log: debug / trace level.
    Debug,
    /// Log: timestamp.
    Timestamp,
}

/// Lexer state carried across lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    /// Nothing open.
    #[default]
    Normal,
    /// Inside a block comment.
    BlockComment,
    /// Inside a triple-quoted string with this quote char.
    TripleString(char),
    /// Inside a Markdown fenced code block.
    Fenced,
}

/// A highlighted range in char indices of the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// First char index.
    pub start: usize,
    /// One past the last char index.
    pub end: usize,
    /// Token class.
    pub kind: Kind,
}

/// Supported languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    /// C.
    C,
    /// C++.
    Cpp,
    /// Rust.
    Rust,
    /// Bourne-style shells.
    Bash,
    /// Python.
    Python,
    /// Log files.
    Log,
    /// `CMake`.
    CMake,
    /// Makefiles.
    Makefile,
    /// Markdown.
    Markdown,
}

impl Language {
    /// Guess the language from the file name and its first line (shebang).
    pub fn detect(path: &Path, first_line: &str) -> Option<Self> {
        let name = path.file_name()?.to_string_lossy().to_lowercase();
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let by_name = match name.as_str() {
            "cmakelists.txt" => Some(Self::CMake),
            "makefile" | "gnumakefile" | "bsdmakefile" => Some(Self::Makefile),
            ".bashrc" | ".zshrc" | ".profile" | ".bash_profile" | ".zshenv" | ".zprofile" => {
                Some(Self::Bash)
            }
            "syslog" | "dmesg" | "messages" => Some(Self::Log),
            _ => None,
        };
        if by_name.is_some() {
            return by_name;
        }
        let by_ext = match ext.as_str() {
            "c" | "h" => Some(Self::C),
            "cpp" | "cc" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "h++" | "ipp" | "inl" | "cu" => {
                Some(Self::Cpp)
            }
            "rs" => Some(Self::Rust),
            "sh" | "bash" | "zsh" | "ksh" => Some(Self::Bash),
            "py" | "pyi" | "pyw" => Some(Self::Python),
            "log" | "out" | "err" | "trace" => Some(Self::Log),
            "cmake" => Some(Self::CMake),
            "mk" | "mak" | "make" => Some(Self::Makefile),
            "md" | "markdown" | "mdown" => Some(Self::Markdown),
            _ => None,
        };
        if by_ext.is_some() {
            return by_ext;
        }
        // Rotated logs: app.log.1, app.log.2.gz is binary anyway.
        if name.contains(".log") {
            return Some(Self::Log);
        }
        let shebang = first_line.strip_prefix("#!")?;
        if shebang.contains("python") {
            Some(Self::Python)
        } else if shebang.contains("sh") {
            Some(Self::Bash)
        } else {
            None
        }
    }

    /// Highlight one line, returning its spans and the state for the next line.
    pub fn highlight_line(self, line: &str, state: State) -> (Vec<Span>, State) {
        let chars: Vec<char> = line.chars().collect();
        match self {
            Self::C => clike::lex(&chars, state, &clike::C),
            Self::Cpp => clike::lex(&chars, state, &clike::CPP),
            Self::Rust => clike::lex(&chars, state, &clike::RUST),
            Self::Bash => clike::lex(&chars, state, &clike::BASH),
            Self::Python => clike::lex(&chars, state, &clike::PYTHON),
            Self::CMake => clike::lex(&chars, state, &clike::CMAKE),
            Self::Log => (log::lex(&chars), State::Normal),
            Self::Makefile => (makefile::lex(&chars), State::Normal),
            Self::Markdown => markdown::lex(&chars, state),
        }
    }

    /// Compute the starting state of every line (index `i` is the state
    /// *before* line `i`). Used to highlight any window of a file.
    pub fn line_states(self, lines: &[String]) -> Vec<State> {
        let mut states = Vec::with_capacity(lines.len() + 1);
        let mut state = State::Normal;
        for line in lines {
            states.push(state);
            state = self.highlight_line(line, state).1;
        }
        states.push(state);
        states
    }
}

/// Helper used by the lexers to build spans without gaps.
#[derive(Debug, Default)]
pub(crate) struct Builder {
    spans: Vec<Span>,
}

impl Builder {
    pub(crate) fn push(&mut self, start: usize, end: usize, kind: Kind) {
        if end <= start {
            return;
        }
        if let Some(last) = self.spans.last_mut() {
            if last.kind == kind && last.end == start {
                last.end = end;
                return;
            }
        }
        self.spans.push(Span { start, end, kind });
    }

    pub(crate) fn finish(self) -> Vec<Span> {
        self.spans
    }
}

pub(crate) fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

pub(crate) fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Find `needle` in `chars` starting at `from`; returns the index of its first char.
pub(crate) fn find(chars: &[char], from: usize, needle: &str) -> Option<usize> {
    let n: Vec<char> = needle.chars().collect();
    if n.is_empty() || chars.len() < n.len() {
        return None;
    }
    (from..=chars.len() - n.len()).find(|&i| chars[i..i + n.len()] == n[..])
}

pub(crate) fn starts_with(chars: &[char], at: usize, needle: &str) -> bool {
    let n: Vec<char> = needle.chars().collect();
    at + n.len() <= chars.len() && chars[at..at + n.len()] == n[..]
}

#[cfg(test)]
pub(crate) fn kinds(lang: Language, line: &str) -> Vec<(String, Kind)> {
    let chars: Vec<char> = line.chars().collect();
    lang.highlight_line(line, State::Normal)
        .0
        .into_iter()
        .filter(|s| s.kind != Kind::Text)
        .map(|s| (chars[s.start..s.end].iter().collect(), s.kind))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_languages() {
        let d = |n: &str, first: &str| Language::detect(Path::new(n), first);
        assert_eq!(d("main.c", ""), Some(Language::C));
        assert_eq!(d("x.hpp", ""), Some(Language::Cpp));
        assert_eq!(d("lib.rs", ""), Some(Language::Rust));
        assert_eq!(d("run.sh", ""), Some(Language::Bash));
        assert_eq!(d("tool", "#!/usr/bin/env bash"), Some(Language::Bash));
        assert_eq!(d("tool", "#!/usr/bin/env python3"), Some(Language::Python));
        assert_eq!(d("CMakeLists.txt", ""), Some(Language::CMake));
        assert_eq!(d("Makefile", ""), Some(Language::Makefile));
        assert_eq!(d("rules.mk", ""), Some(Language::Makefile));
        assert_eq!(d("app.log.1", ""), Some(Language::Log));
        assert_eq!(d("CONTEXT.md", ""), Some(Language::Markdown));
        assert_eq!(d("data.bin", ""), None);
    }

    #[test]
    fn line_states_track_block_comments() {
        let lines: Vec<String> = ["int a; /* open", "still", "*/ int b;", "int c;"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let states = Language::C.line_states(&lines);
        assert_eq!(
            states,
            vec![
                State::Normal,
                State::BlockComment,
                State::BlockComment,
                State::Normal,
                State::Normal
            ]
        );
    }

    #[test]
    fn builder_merges_adjacent_spans() {
        let mut b = Builder::default();
        b.push(0, 2, Kind::Text);
        b.push(2, 4, Kind::Text);
        b.push(4, 5, Kind::Keyword);
        b.push(5, 5, Kind::Keyword);
        assert_eq!(
            b.finish(),
            vec![
                Span {
                    start: 0,
                    end: 4,
                    kind: Kind::Text
                },
                Span {
                    start: 4,
                    end: 5,
                    kind: Kind::Keyword
                }
            ]
        );
    }
}
