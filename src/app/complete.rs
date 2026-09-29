//! Ctrl+Space: complete the word before the cursor from the words of the
//! open files (nearest first). Until clangd is wired in, this is the
//! "any word I have seen" completion.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::editor::Buffer;

use super::App;

/// Most suggestions listed.
const MAX_ITEMS: usize = 50;
/// Shortest word offered.
const MIN_WORD: usize = 3;

/// The suggestion list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// Where the word being completed starts (row, char column).
    pub start: (usize, usize),
    /// Suggestions, best first.
    pub items: Vec<String>,
    /// Selected suggestion.
    pub selected: usize,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The word before the cursor: its start column and text.
fn prefix(ed: &Buffer) -> (usize, String) {
    let (row, col) = ed.cursor();
    let line: Vec<char> = ed
        .lines()
        .get(row)
        .map_or_else(Vec::new, |l| l.chars().collect());
    let col = col.min(line.len());
    let start = line[..col]
        .iter()
        .rposition(|c| !is_word(*c))
        .map_or(0, |i| i + 1);
    (start, line[start..col].iter().collect())
}

/// Words of `text` lines with their distance from `row`, into `best`.
fn collect(lines: &[String], row: Option<usize>, far: usize, best: &mut HashMap<String, usize>) {
    for (i, line) in lines.iter().enumerate() {
        let dist = row.map_or(far, |r| r.abs_diff(i));
        let mut word = String::new();
        for c in line.chars().chain(std::iter::once(' ')) {
            if is_word(c) {
                word.push(c);
                continue;
            }
            if word.chars().count() >= MIN_WORD && !word.starts_with(|c: char| c.is_ascii_digit()) {
                let d = best.entry(std::mem::take(&mut word)).or_insert(dist);
                *d = (*d).min(dist);
            }
            word.clear();
        }
    }
}

/// Suggestions for `prefix`: words starting with it (ignoring case), those
/// matching its case first, then nearest to `row` of `current`.
pub fn suggest(prefix: &str, current: &Buffer, row: usize, others: &[&Buffer]) -> Vec<String> {
    let mut best = HashMap::new();
    collect(current.lines(), Some(row), 0, &mut best);
    for b in others {
        collect(b.lines(), None, usize::MAX / 2, &mut best);
    }
    let lower = prefix.to_lowercase();
    let mut items: Vec<(bool, usize, String)> = best
        .into_iter()
        .filter(|(w, _)| w != prefix && w.to_lowercase().starts_with(&lower))
        .map(|(w, d)| (!w.starts_with(prefix), d, w))
        .collect();
    items.sort();
    items
        .into_iter()
        .take(MAX_ITEMS)
        .map(|(_, _, w)| w)
        .collect()
}

impl App {
    /// The suggestion list, if showing.
    pub fn completion(&self) -> Option<&Completion> {
        self.completion.as_ref()
    }

    /// Ctrl+Space: list the words that complete the one before the cursor
    /// (one match is inserted right away).
    pub(super) fn open_completion(&mut self) {
        let Some(ctx) = self.active_context() else {
            return;
        };
        let Some(ed) = ctx.editor.as_ref() else {
            return;
        };
        let (start, word) = prefix(ed);
        let others: Vec<&Buffer> = ctx.recent.iter().collect();
        let row = ed.cursor().0;
        let items = suggest(&word, ed, row, &others);
        match items.len() {
            0 => self.set_status(if word.is_empty() {
                "type the start of a word, then Ctrl+Space".to_owned()
            } else {
                format!("no words start with {word}")
            }),
            1 => {
                self.completion = Some(Completion {
                    start: (row, start),
                    items,
                    selected: 0,
                });
                self.accept_completion();
            }
            _ => {
                self.completion = Some(Completion {
                    start: (row, start),
                    items,
                    selected: 0,
                });
            }
        }
    }

    /// Replace the word before the cursor with the selected suggestion.
    fn accept_completion(&mut self) {
        let Some(c) = self.completion.take() else {
            return;
        };
        let Some(word) = c.items.get(c.selected) else {
            return;
        };
        if let Some(ed) = self.active_context_mut().and_then(|x| x.editor.as_mut()) {
            let cursor = ed.cursor();
            ed.select_range(c.start, cursor);
            ed.insert_str(word);
        }
    }

    /// Keys while the list shows. Returns whether the key was used; typing
    /// goes to the editor and narrows the list.
    pub(super) fn handle_completion_key(&mut self, key: KeyEvent) -> bool {
        let Some(c) = self.completion.as_mut() else {
            return false;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let n = c.items.len();
        match key.code {
            KeyCode::Esc => {
                self.completion = None;
                return true;
            }
            KeyCode::Up => c.selected = (c.selected + n - 1) % n.max(1),
            KeyCode::Char('p') if ctrl => c.selected = (c.selected + n - 1) % n.max(1),
            KeyCode::Down => c.selected = (c.selected + 1) % n.max(1),
            KeyCode::Char('n' | ' ') if ctrl => c.selected = (c.selected + 1) % n.max(1),
            KeyCode::Enter | KeyCode::Tab => self.accept_completion(),
            // Typing a word character (or deleting one) narrows the list.
            KeyCode::Char(ch) if !ctrl && is_word(ch) => {
                self.refilter_after(key);
                return true;
            }
            KeyCode::Backspace if key.modifiers.is_empty() => {
                self.refilter_after(key);
                return true;
            }
            _ => {
                self.completion = None;
                return false;
            }
        }
        true
    }

    /// Let the editor take `key`, then list the words for the new prefix.
    fn refilter_after(&mut self, key: KeyEvent) {
        let start = self.completion.as_ref().map(|c| c.start);
        self.completion = None;
        self.handle_editor_key(key);
        let Some(start) = start else { return };
        let Some(ctx) = self.active_context() else {
            return;
        };
        let Some(ed) = ctx.editor.as_ref() else {
            return;
        };
        let (col, word) = prefix(ed);
        if (ed.cursor().0, col) != start || word.is_empty() {
            return;
        }
        let others: Vec<&Buffer> = ctx.recent.iter().collect();
        let items = suggest(&word, ed, start.0, &others);
        if !items.is_empty() {
            self.completion = Some(Completion {
                start,
                items,
                selected: 0,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn nearest_words_matching_case_first() {
        let cur = Buffer::from_text(
            Path::new("/a.c"),
            "int socket_open;\nint Socket_close;\n\nsoc\nint socket_read;\n",
            false,
        );
        let other = Buffer::from_text(Path::new("/b.c"), "socket_zap socket_open so1\n", false);
        let items = suggest("soc", &cur, 3, &[&other]);
        assert_eq!(
            items,
            ["socket_read", "socket_open", "socket_zap", "Socket_close"],
            "same case first, then by distance, other files last"
        );
        assert!(suggest("x", &cur, 0, &[]).is_empty());
        let (start, word) = {
            let mut b = cur.clone();
            b.goto(4, 4);
            prefix(&b)
        };
        assert_eq!((start, word.as_str()), (0, "soc"));
    }
}
