//! Find and replace in the open file (Ctrl+F / Ctrl+R): a bar at the bottom
//! of the editor with regex, match-case and whole-word switches; every match
//! is highlighted, F3 / Shift+F3 move between them.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use regex::{Regex, RegexBuilder};

use crate::editor::Buffer;

use super::{App, Focus};

/// Most matches kept for one search.
const MAX_HITS: usize = 100_000;

/// One match: a line and a char range on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    /// Line.
    pub row: usize,
    /// First char.
    pub start: usize,
    /// Char after the last one.
    pub end: usize,
}

/// Which field of the bar has the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindField {
    /// The search text.
    Query,
    /// The replacement.
    Replace,
}

/// The find bar.
#[derive(Debug)]
pub struct FindBar {
    /// Search text.
    pub query: String,
    /// Replacement (`$1` / `${name}` refer to groups in regex mode).
    pub replace: String,
    /// Regular expression (else literal text).
    pub regex: bool,
    /// Match case (else case-insensitive).
    pub case: bool,
    /// Whole words only.
    pub word: bool,
    /// Showing the replace field.
    pub replacing: bool,
    /// Field with the keys.
    pub field: FindField,
    /// Shown at the bottom of the editor.
    pub open: bool,
    /// Keys go to the bar.
    pub focused: bool,
    /// The query was filled in from the selection: typing replaces it.
    fresh: bool,
    /// Where the cursor was when the bar opened (typing searches from here).
    origin: (usize, usize),
    /// Why the pattern does not compile.
    pub error: Option<String>,
    compiled: Option<(String, Regex)>,
    hits: Option<(PathBuf, u64, String, Vec<Hit>)>,
}

impl FindBar {
    fn new(query: String, origin: (usize, usize)) -> Self {
        Self {
            fresh: !query.is_empty(),
            query,
            replace: String::new(),
            regex: false,
            case: false,
            word: false,
            replacing: false,
            field: FindField::Query,
            open: true,
            focused: true,
            origin,
            error: None,
            compiled: None,
            hits: None,
        }
    }

    /// A copy for drawing, without the cached matches.
    #[must_use]
    pub fn snapshot(&self) -> Self {
        Self {
            query: self.query.clone(),
            replace: self.replace.clone(),
            error: self.error.clone(),
            compiled: None,
            hits: None,
            ..*self
        }
    }

    fn key(&self) -> String {
        format!(
            "{}{}{}\u{0}{}",
            self.regex, self.case, self.word, self.query
        )
    }

    /// The compiled pattern (`None` for an empty or invalid query).
    pub fn pattern(&mut self) -> Option<&Regex> {
        if self.query.is_empty() {
            self.error = None;
            return None;
        }
        let key = self.key();
        if self.compiled.as_ref().map(|(k, _)| k) != Some(&key) {
            match build_regex(&self.query, self.regex, self.case, self.word) {
                Ok(re) => {
                    self.error = None;
                    self.compiled = Some((key, re));
                }
                Err(e) => {
                    self.error = Some(e);
                    self.compiled = None;
                }
            }
        }
        self.compiled.as_ref().map(|(_, re)| re)
    }

    /// Every match in `ed` (cached until the text or the query changes).
    pub fn hits(&mut self, ed: &Buffer) -> &[Hit] {
        let key = self.key();
        let fresh = self
            .hits
            .as_ref()
            .is_some_and(|(p, rev, k, _)| p == ed.path() && *rev == ed.revision() && *k == key);
        if !fresh {
            let list = self
                .pattern()
                .map_or_else(Vec::new, |re| find_all(re, ed.lines()));
            self.hits = Some((ed.path().to_path_buf(), ed.revision(), key, list));
        }
        self.hits.as_ref().map_or(&[], |(_, _, _, h)| h.as_slice())
    }

    /// The match after (or before) `from`, wrapping around; with `inclusive`
    /// a match starting at `from` counts. Returns its index and the match.
    pub fn step(
        &mut self,
        ed: &Buffer,
        from: (usize, usize),
        forward: bool,
        inclusive: bool,
    ) -> Option<(usize, Hit)> {
        let hits = self.hits(ed);
        if hits.is_empty() {
            return None;
        }
        let pos = |h: &Hit| (h.row, h.start);
        let i = if forward {
            hits.iter()
                .position(|h| {
                    if inclusive {
                        pos(h) >= from
                    } else {
                        pos(h) > from
                    }
                })
                .unwrap_or(0)
        } else {
            hits.iter()
                .rposition(|h| pos(h) < from)
                .unwrap_or(hits.len() - 1)
        };
        Some((i, hits[i]))
    }

    /// The replacement for match `hit` (groups expanded in regex mode).
    fn replacement(&mut self, ed: &Buffer, hit: Hit) -> Option<String> {
        let regex = self.regex;
        let replace = self.replace.clone();
        let re = self.pattern()?;
        if !regex {
            return Some(replace);
        }
        let line = ed.lines().get(hit.row)?;
        let byte = line
            .char_indices()
            .nth(hit.start)
            .map_or(line.len(), |(i, _)| i);
        let caps = re.captures_at(line, byte)?;
        let mut out = String::new();
        caps.expand(&replace, &mut out);
        Some(out)
    }
}

/// Compile a search: `query` as a regex or literal text, case-insensitive
/// unless `case`, whole words only with `word`. The error is one line.
pub fn build_regex(query: &str, regex: bool, case: bool, word: bool) -> Result<Regex, String> {
    let body = if regex {
        query.to_owned()
    } else {
        regex::escape(query)
    };
    let body = if word {
        format!(r"\b(?:{body})\b")
    } else {
        body
    };
    RegexBuilder::new(&body)
        .case_insensitive(!case)
        .build()
        .map_err(|e| {
            e.to_string()
                .lines()
                .last()
                .unwrap_or("invalid pattern")
                .to_owned()
        })
}

/// Delete the word before the end of the bar's field.
fn delete_word(find: &mut FindBar) {
    let field = match find.field {
        FindField::Query => &mut find.query,
        FindField::Replace => &mut find.replace,
    };
    if find.fresh && find.field == FindField::Query {
        field.clear();
    } else {
        let trimmed = field.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_');
        let keep = trimmed.trim_end_matches(|c: char| c.is_alphanumeric() || c == '_');
        let n = keep.len();
        field.truncate(n);
    }
    find.fresh = false;
}

/// Matches of `re` on every line (empty matches skipped), as char ranges.
pub fn find_all(re: &Regex, lines: &[String]) -> Vec<Hit> {
    let mut out = Vec::new();
    for (row, line) in lines.iter().enumerate() {
        for m in re.find_iter(line) {
            if m.start() == m.end() {
                continue;
            }
            let start = line[..m.start()].chars().count();
            let end = start + line[m.start()..m.end()].chars().count();
            out.push(Hit { row, start, end });
            if out.len() >= MAX_HITS {
                return out;
            }
        }
    }
    out
}

impl App {
    /// The find bar, if any.
    pub fn find_bar(&self) -> Option<&FindBar> {
        self.find.as_ref()
    }

    /// Matches on lines `first..last` of the open file, the selected match
    /// (its index and itself) and the number of matches.
    pub fn visible_hits(
        &mut self,
        first: usize,
        last: usize,
    ) -> (Vec<Hit>, Option<(usize, Hit)>, usize) {
        let Some(find) = self.find.as_mut().filter(|f| f.open) else {
            return (Vec::new(), None, 0);
        };
        let Some(ed) = self
            .active_task
            .as_ref()
            .and_then(|id| self.contexts.get(id))
            .and_then(|c| c.editor.as_ref())
        else {
            return (Vec::new(), None, 0);
        };
        let sel = ed.selection();
        let hits = find.hits(ed);
        let current = sel.and_then(|(a, b)| {
            hits.iter()
                .position(|h| (h.row, h.start) == a && (h.row, h.end) == b)
                .map(|i| (i, hits[i]))
        });
        let from = hits.partition_point(|h| h.row < first);
        let to = hits.partition_point(|h| h.row < last);
        (hits[from..to].to_vec(), current, hits.len())
    }

    /// Ctrl+F (`replace`: Ctrl+R): open the bar, seeded with the selection.
    pub(super) fn open_find(&mut self, replace: bool) {
        let Some(ed) = self.active_context().and_then(|c| c.editor.as_ref()) else {
            self.set_status("open a file first");
            return;
        };
        let selected = ed
            .selected_text()
            .filter(|t| !t.contains('\n') && !t.is_empty());
        let origin = ed.selection().map_or(ed.cursor(), |(a, _)| a);
        let bar = match self.find.take() {
            Some(mut bar) => {
                if let Some(t) = selected {
                    bar.query = t;
                    bar.fresh = true;
                }
                bar.open = true;
                bar.focused = true;
                bar.origin = origin;
                bar
            }
            None => FindBar::new(selected.unwrap_or_default(), origin),
        };
        self.find = Some(bar);
        if let Some(f) = &mut self.find {
            if replace {
                f.replacing = true;
                f.field = FindField::Replace;
            } else {
                f.field = FindField::Query;
            }
        }
    }

    /// F3 / Shift+F3 / Enter in the bar: select the next / previous match.
    pub(super) fn find_step(&mut self, forward: bool) {
        self.find_move(forward, false);
    }

    fn find_move(&mut self, forward: bool, from_origin: bool) {
        let height = self.editor_height;
        let Some(find) = self.find.as_mut() else {
            self.open_find(false);
            return;
        };
        let Some(ed) = self
            .active_task
            .as_ref()
            .and_then(|id| self.contexts.get_mut(id))
            .and_then(|c| c.editor.as_mut())
        else {
            return;
        };
        let from = if from_origin {
            find.origin
        } else {
            ed.selection().map_or(ed.cursor(), |(a, _)| a)
        };
        let found = find.step(ed, from, forward, from_origin);
        let total = find.hits(ed).len();
        let status = match (found, &find.error) {
            (_, Some(e)) => format!("pattern: {e}"),
            (Some((i, h)), _) => {
                ed.select_range((h.row, h.start), (h.row, h.end));
                if h.row < ed.scroll() || h.row >= ed.scroll() + height {
                    ed.set_scroll(h.row.saturating_sub(height / 3));
                }
                format!("{}/{total}", i + 1)
            }
            (None, _) if find.query.is_empty() => String::new(),
            (None, _) => format!("no matches for {}", find.query),
        };
        if !status.is_empty() {
            self.set_status(status);
        }
    }

    /// Replace the selected match, then select the next one.
    fn replace_one(&mut self) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let Some(ed) = self
            .active_task
            .as_ref()
            .and_then(|id| self.contexts.get_mut(id))
            .and_then(|c| c.editor.as_mut())
        else {
            return;
        };
        let sel = ed.selection();
        let current = find
            .hits(ed)
            .iter()
            .copied()
            .find(|h| sel == Some(((h.row, h.start), (h.row, h.end))));
        if let Some(hit) = current {
            if let Some(text) = find.replacement(ed, hit) {
                ed.insert_str(&text);
            }
        }
        self.find_move(true, false);
    }

    /// Replace every match (one undo step).
    fn replace_all(&mut self) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let Some(ed) = self
            .active_task
            .as_ref()
            .and_then(|id| self.contexts.get_mut(id))
            .and_then(|c| c.editor.as_mut())
        else {
            return;
        };
        let hits: Vec<Hit> = find.hits(ed).to_vec();
        let edits: Vec<(usize, usize, usize, String)> = hits
            .iter()
            .filter_map(|h| find.replacement(ed, *h).map(|t| (h.row, h.start, h.end, t)))
            .collect();
        let n = ed.replace_ranges(&edits);
        self.set_status(format!("replaced {n} match(es) · Ctrl+Z undoes it"));
    }

    /// Keys while the bar has the focus. Returns whether the key was used
    /// (other Ctrl keys go on to the editor, e.g. Ctrl+S).
    pub(super) fn handle_find_key(&mut self, key: KeyEvent) -> bool {
        let Some(find) = self.find.as_mut().filter(|f| f.open) else {
            return false;
        };
        if !find.focused {
            // Esc in the editor closes the bar before it opens the menu.
            if key.code == KeyCode::Esc && key.modifiers.is_empty() {
                find.open = false;
                return true;
            }
            return false;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let mut research = false;
        match key.code {
            KeyCode::Esc => {
                find.open = false;
                find.focused = false;
            }
            KeyCode::Enter if alt || (ctrl && find.field == FindField::Replace) => {
                self.replace_all();
                return true;
            }
            KeyCode::Char('a') if alt => {
                self.replace_all();
                return true;
            }
            KeyCode::Enter if find.field == FindField::Replace => {
                self.replace_one();
                return true;
            }
            KeyCode::Enter | KeyCode::F(3) | KeyCode::Down => {
                let forward = !(shift || key.code == KeyCode::Up);
                self.find_step(forward);
                return true;
            }
            KeyCode::Up => {
                self.find_step(false);
                return true;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                if find.replacing {
                    find.field = match find.field {
                        FindField::Query => FindField::Replace,
                        FindField::Replace => FindField::Query,
                    };
                }
            }
            KeyCode::Char('c') if alt => {
                find.case = !find.case;
                research = true;
            }
            KeyCode::Char('r') if alt => {
                find.regex = !find.regex;
                research = true;
            }
            KeyCode::Char('w') if alt => {
                find.word = !find.word;
                research = true;
            }
            KeyCode::Char('f') if ctrl && !shift => find.field = FindField::Query,
            KeyCode::Char('r') if ctrl => {
                find.replacing = true;
                find.field = FindField::Replace;
            }
            // Ctrl+Backspace (Ctrl+H in most terminals) / Alt+Backspace: a word.
            KeyCode::Char('h') if ctrl => {
                research = find.field == FindField::Query;
                delete_word(find);
            }
            KeyCode::Backspace if ctrl || alt => {
                research = find.field == FindField::Query;
                delete_word(find);
            }
            KeyCode::Backspace => match find.field {
                FindField::Query => {
                    if find.fresh {
                        find.query.clear();
                    } else {
                        find.query.pop();
                    }
                    find.fresh = false;
                    research = true;
                }
                FindField::Replace => {
                    find.replace.pop();
                }
            },
            KeyCode::Char(c) if !ctrl && !alt => match find.field {
                FindField::Query => {
                    if find.fresh {
                        find.query.clear();
                        find.fresh = false;
                    }
                    find.query.push(c);
                    research = true;
                }
                FindField::Replace => find.replace.push(c),
            },
            _ if ctrl => {
                // Ctrl+S, Ctrl+Z, … act on the editor; the bar stays open.
                find.focused = false;
                return false;
            }
            _ => {}
        }
        if research {
            self.find_move(true, true);
        }
        true
    }

    /// Paste into the bar's field (one line).
    pub(super) fn find_paste(&mut self, text: &str) -> bool {
        let Some(find) = self.find.as_mut().filter(|f| f.focused && f.open) else {
            return false;
        };
        let line = text.lines().next().unwrap_or("");
        match find.field {
            FindField::Query => {
                if find.fresh {
                    find.query.clear();
                    find.fresh = false;
                }
                find.query.push_str(line);
                self.find_move(true, true);
            }
            FindField::Replace => find.replace.push_str(line),
        }
        true
    }

    /// Whether the editor is showing and the bar has the keys.
    pub(super) fn find_has_keys(&self) -> bool {
        self.find.as_ref().is_some_and(|f| f.open && f.focused)
            && self
                .active_context()
                .is_some_and(|c| c.focus == Focus::Editor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn bar(q: &str) -> FindBar {
        FindBar::new(q.into(), (0, 0))
    }

    #[test]
    fn literal_regex_case_and_word() {
        let ed = Buffer::from_text(Path::new("/x.c"), "int foo = FOO(foobar);\nfoo.x\n", false);
        let mut b = bar("foo");
        assert_eq!(b.hits(&ed).len(), 4, "case-insensitive by default");
        b.case = true;
        assert_eq!(b.hits(&ed).len(), 3);
        b.word = true;
        assert_eq!(
            b.hits(&ed),
            &[
                Hit {
                    row: 0,
                    start: 4,
                    end: 7
                },
                Hit {
                    row: 1,
                    start: 0,
                    end: 3
                }
            ]
        );
        let mut r = bar(r"f\w+");
        assert!(r.hits(&ed).is_empty(), "literal: backslash is text");
        r.regex = true;
        assert_eq!(r.hits(&ed).len(), 4);
        r.query = "(".into();
        assert!(r.hits(&ed).is_empty());
        assert!(r.error.is_some());
        let mut dot = bar("x.");
        assert!(dot.hits(&ed).is_empty(), "a dot is a dot unless regex");
        dot.query = ".x".into();
        assert_eq!(dot.hits(&ed).len(), 1);
    }

    #[test]
    fn stepping_wraps_and_counts_chars() {
        let ed = Buffer::from_text(Path::new("/x"), "é a\na a\n", false);
        let mut b = bar("a");
        assert_eq!(
            b.hits(&ed)[0],
            Hit {
                row: 0,
                start: 2,
                end: 3
            }
        );
        let (i, h) = b.step(&ed, (0, 2), true, false).unwrap();
        assert_eq!((i, h.row, h.start), (1, 1, 0));
        let (i, _) = b.step(&ed, (1, 2), true, false).unwrap();
        assert_eq!(i, 0, "wraps to the top");
        let (i, _) = b.step(&ed, (0, 0), false, false).unwrap();
        assert_eq!(i, 2, "backwards wraps to the bottom");
        let (i, _) = b.step(&ed, (0, 2), true, true).unwrap();
        assert_eq!(i, 0, "inclusive keeps a match at the start");
    }

    #[test]
    fn replacement_expands_groups() {
        let ed = Buffer::from_text(Path::new("/x"), "set_a(1); set_b(2);\n", false);
        let mut b = bar(r"set_(\w)\((\d)\)");
        b.regex = true;
        b.replace = "${1}=$2".into();
        let hits = b.hits(&ed).to_vec();
        assert_eq!(b.replacement(&ed, hits[1]).as_deref(), Some("b=2"));
        b.regex = false;
        b.query = "set_".into();
        b.replace = "$1".into();
        let h = b.hits(&ed)[0];
        assert_eq!(b.replacement(&ed, h).as_deref(), Some("$1"), "literal mode");
    }
}
