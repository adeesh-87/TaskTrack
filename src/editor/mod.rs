//! A deliberately small text buffer: enough to view and edit one file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Most undo steps kept.
const UNDO_LIMIT: usize = 500;

/// What the last edit was, for grouping typed characters into one undo step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Type,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    lines: Vec<String>,
    cursor: (usize, usize),
}

/// A text buffer with a cursor and a scroll offset.
#[derive(Debug, Clone)]
pub struct Buffer {
    path: PathBuf,
    lines: Vec<String>,
    /// Cursor as (line index, char index).
    cursor: (usize, usize),
    /// First visible line.
    scroll: usize,
    dirty: bool,
    read_only: bool,
    /// Incremented on every content change (used to invalidate caches).
    revision: u64,
    /// Text as last loaded from or saved to disk.
    base: String,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_edit: Option<EditKind>,
    /// Inside `insert_str`: the whole paste is one undo step.
    batching: bool,
    /// Selection anchor: the selection runs from here to the cursor.
    anchor: Option<(usize, usize)>,
    /// The file had Windows line endings; they are written back.
    crlf: bool,
    /// Modification time of the file when last read or written.
    disk_mtime: Option<std::time::SystemTime>,
    /// Indentation the file uses, when it shows one: a tab or N spaces.
    indent: Option<String>,
}

fn file_mtime(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The indentation a file uses: a tab (Makefiles always), or the most common
/// step of leading spaces (2, 3, 4 or 8); `None` when it does not show.
pub fn detect_indent(lines: &[String], path: &Path) -> Option<String> {
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    if matches!(name.as_str(), "Makefile" | "makefile" | "GNUmakefile")
        || path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("mk"))
    {
        return Some("\t".into());
    }
    let (mut tabs, mut spaced) = (0usize, 0usize);
    let mut steps = [0usize; 9];
    let mut prev = 0usize;
    for line in lines.iter().filter(|l| !l.trim().is_empty()).take(4000) {
        if line.starts_with('\t') {
            tabs += 1;
            continue;
        }
        let n = line.chars().take_while(|c| *c == ' ').count();
        if n > 0 {
            spaced += 1;
        }
        if n > prev && n - prev < steps.len() {
            steps[n - prev] += 1;
        }
        prev = n;
    }
    if tabs > spaced {
        return Some("\t".into());
    }
    [2usize, 3, 4, 8]
        .into_iter()
        .filter(|w| steps[*w] > 0)
        .max_by_key(|w| (steps[*w], *w))
        .map(|w| " ".repeat(w))
}

/// Character classes for word-wise movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Space,
    Word,
    Punct,
}

fn class_of(c: char) -> CharClass {
    if c.is_whitespace() {
        CharClass::Space
    } else if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

impl Buffer {
    /// Load `path` into a buffer. Binary content is shown read-only as a hex dump.
    pub fn open(path: &Path) -> io::Result<Self> {
        let bytes = fs::read(path)?;
        let (text, read_only) = match String::from_utf8(bytes) {
            Ok(t) => (t, false),
            Err(e) => (hex_dump(e.as_bytes()), true),
        };
        Ok(Self::from_text(path, &text, read_only))
    }

    /// Build a buffer from text (used by tests and for read-only views).
    pub fn from_text(path: &Path, text: &str, read_only: bool) -> Self {
        let crlf = text.contains("\r\n");
        let normalized;
        let text = if crlf {
            normalized = text.replace("\r\n", "\n");
            normalized.as_str()
        } else {
            text
        };
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        if lines.is_empty() || text.ends_with('\n') {
            lines.push(String::new());
        }
        let indent = detect_indent(&lines, path);
        Self {
            path: path.to_path_buf(),
            lines,
            cursor: (0, 0),
            scroll: 0,
            dirty: false,
            read_only,
            revision: 0,
            base: text.to_owned(),
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: None,
            batching: false,
            anchor: None,
            crlf,
            disk_mtime: file_mtime(path),
            indent,
        }
    }

    /// The indentation to insert: the file's own, else `default_width` spaces.
    pub fn indent_unit(&self, default_width: usize) -> String {
        self.indent
            .clone()
            .unwrap_or_else(|| " ".repeat(default_width.max(1)))
    }

    /// Whether the file changed on disk since it was read or saved.
    pub fn changed_on_disk(&self) -> bool {
        self.disk_mtime.is_some() && file_mtime(&self.path) != self.disk_mtime
    }

    /// Take the current state on disk as seen (without reloading).
    pub fn mark_disk_seen(&mut self) {
        self.disk_mtime = file_mtime(&self.path);
    }

    /// Read the file again (keeping the cursor; undo can go back).
    pub fn reload(&mut self) -> io::Result<()> {
        let text = fs::read_to_string(&self.path)?;
        self.replace_saved(&text);
        self.mark_disk_seen();
        Ok(())
    }

    /// Text as last loaded or saved (the merge base for [`crate::tasks::merge`]).
    pub fn base_text(&self) -> &str {
        &self.base
    }

    /// Replace the whole content with text that is now on disk (after a merged
    /// save). Keeps the cursor where possible; undo history is kept.
    pub fn replace_saved(&mut self, text: &str) {
        self.push_undo(EditKind::Other);
        let normalized = text.replace("\r\n", "\n");
        let text = normalized.as_str();
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        if lines.is_empty() || text.ends_with('\n') {
            lines.push(String::new());
        }
        self.lines = lines;
        text.clone_into(&mut self.base);
        self.dirty = false;
        self.revision += 1;
        self.anchor = None;
        self.set_cursor(self.cursor.0, self.cursor.1);
    }

    fn push_undo(&mut self, kind: EditKind) {
        if self.batching {
            return;
        }
        let group = kind == EditKind::Type && self.last_edit == Some(EditKind::Type);
        self.last_edit = Some(kind);
        self.redo.clear();
        if group {
            return;
        }
        self.undo.push(Snapshot {
            lines: self.lines.clone(),
            cursor: self.cursor,
        });
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
    }

    fn restore(&mut self, snap: Snapshot) {
        self.lines = snap.lines;
        self.cursor = snap.cursor;
        self.revision += 1;
        self.dirty = self.text().trim_end_matches('\n') != self.base.trim_end_matches('\n');
        self.last_edit = None;
        self.anchor = None;
    }

    /// Undo the last edit. Returns whether there was one.
    pub fn undo(&mut self) -> bool {
        let Some(snap) = self.undo.pop() else {
            return false;
        };
        self.redo.push(Snapshot {
            lines: self.lines.clone(),
            cursor: self.cursor,
        });
        self.restore(snap);
        true
    }

    /// Redo an undone edit. Returns whether there was one.
    pub fn redo(&mut self) -> bool {
        let Some(snap) = self.redo.pop() else {
            return false;
        };
        self.undo.push(Snapshot {
            lines: self.lines.clone(),
            cursor: self.cursor,
        });
        self.restore(snap);
        true
    }

    /// Set the first visible line.
    pub fn set_scroll(&mut self, scroll: usize) {
        self.scroll = scroll.min(self.lines.len().saturating_sub(1));
    }

    fn text_for_disk(&self) -> String {
        let mut text = self.text();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text
    }

    /// `text` with the file's own line endings.
    fn with_eol(&self, text: String) -> String {
        if self.crlf {
            text.replace('\n', "\r\n")
        } else {
            text
        }
    }

    /// File backing this buffer.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// All lines.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Cursor position as (line, char column).
    pub fn cursor(&self) -> (usize, usize) {
        self.cursor
    }

    /// First visible line.
    pub fn scroll(&self) -> usize {
        self.scroll
    }

    /// Whether there are unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Whether editing is disabled.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Content revision; changes whenever the text changes.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Place the cursor (clamped to the buffer).
    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor.0 = row.min(self.lines.len() - 1);
        self.cursor.1 = col.min(self.line_len(self.cursor.0));
    }

    /// Start extending a selection from the cursor (`extend`), or drop the
    /// selection. Call before a movement: Shift+arrow is `select(true)` + move.
    pub fn select(&mut self, extend: bool) {
        if !extend {
            self.anchor = None;
        } else if self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        }
    }

    /// Put the selection anchor at the cursor (a mouse press: dragging extends it).
    pub fn set_anchor(&mut self) {
        self.anchor = Some(self.cursor);
    }

    /// Select `start..end` (char positions), leaving the cursor at `end`.
    pub fn select_range(&mut self, start: (usize, usize), end: (usize, usize)) {
        self.set_cursor(start.0, start.1);
        self.anchor = Some(self.cursor);
        self.set_cursor(end.0, end.1);
    }

    /// Select the whole buffer.
    pub fn select_all(&mut self) {
        let last = self.lines.len() - 1;
        self.select_range((0, 0), (last, self.line_len(last)));
    }

    /// Select the word (or run of spaces / punctuation) under the cursor.
    pub fn select_word(&mut self) {
        let (row, col) = self.cursor;
        let chars: Vec<char> = self.lines[row].chars().collect();
        let Some(&c) = chars.get(col).or_else(|| chars.get(col.wrapping_sub(1))) else {
            return;
        };
        let class = class_of(c);
        let at = col.min(chars.len() - 1);
        let mut start = at;
        while start > 0 && class_of(chars[start - 1]) == class {
            start -= 1;
        }
        let mut end = at;
        while end < chars.len() && class_of(chars[end]) == class {
            end += 1;
        }
        self.select_range((row, start), (row, end));
    }

    /// Select the cursor's whole line, including its line break.
    pub fn select_line(&mut self) {
        let row = self.cursor.0;
        if row + 1 < self.lines.len() {
            self.select_range((row, 0), (row + 1, 0));
        } else {
            self.select_range((row, 0), (row, self.line_len(row)));
        }
    }

    /// The selection as ordered (start, end) char positions; `None` when empty.
    pub fn selection(&self) -> Option<((usize, usize), (usize, usize))> {
        let anchor = self.anchor?;
        let row = anchor.0.min(self.lines.len() - 1);
        let anchor = (row, anchor.1.min(self.line_len(row)));
        match anchor.cmp(&self.cursor) {
            std::cmp::Ordering::Less => Some((anchor, self.cursor)),
            std::cmp::Ordering::Greater => Some((self.cursor, anchor)),
            std::cmp::Ordering::Equal => None,
        }
    }

    /// Text in `start..end` (char positions), lines joined with `\n`.
    fn text_between(&self, start: (usize, usize), end: (usize, usize)) -> String {
        if start.0 == end.0 {
            return self.lines[start.0]
                .chars()
                .skip(start.1)
                .take(end.1 - start.1)
                .collect();
        }
        let mut out: String = self.lines[start.0].chars().skip(start.1).collect();
        for line in &self.lines[start.0 + 1..end.0] {
            out.push('\n');
            out.push_str(line);
        }
        out.push('\n');
        out.extend(self.lines[end.0].chars().take(end.1));
        out
    }

    /// The selected text, if any.
    pub fn selected_text(&self) -> Option<String> {
        self.selection().map(|(a, b)| self.text_between(a, b))
    }

    /// Remove `start..end` and put the cursor at `start` (no undo step).
    fn remove_range(&mut self, start: (usize, usize), end: (usize, usize)) {
        let tail_idx = Self::byte_index(&self.lines[end.0], end.1);
        let tail = self.lines[end.0][tail_idx..].to_owned();
        let head_idx = Self::byte_index(&self.lines[start.0], start.1);
        self.lines[start.0].truncate(head_idx);
        self.lines[start.0].push_str(&tail);
        self.lines.drain(start.0 + 1..=end.0);
        self.cursor = start;
        self.anchor = None;
        self.dirty = true;
        self.revision += 1;
    }

    /// Delete the selection as one undo step. Returns whether there was one.
    /// Always drops the anchor.
    fn delete_selection_step(&mut self) -> bool {
        let Some((a, b)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        self.push_undo(EditKind::Other);
        self.remove_range(a, b);
        true
    }

    /// Cut: return the selected text and delete it.
    pub fn cut(&mut self) -> Option<String> {
        let text = self.selected_text()?;
        if self.read_only {
            return None;
        }
        self.delete_selection_step();
        Some(text)
    }

    /// Position one word to the left of `pos` (crossing to the previous line end).
    fn word_left_of(&self, (row, col): (usize, usize)) -> (usize, usize) {
        if col == 0 {
            return if row > 0 {
                (row - 1, self.line_len(row - 1))
            } else {
                (0, 0)
            };
        }
        let chars: Vec<char> = self.lines[row].chars().collect();
        let mut i = col.min(chars.len());
        while i > 0 && class_of(chars[i - 1]) == CharClass::Space {
            i -= 1;
        }
        if i > 0 {
            let class = class_of(chars[i - 1]);
            while i > 0 && class_of(chars[i - 1]) == class {
                i -= 1;
            }
        }
        (row, i)
    }

    /// Position one word to the right of `pos` (crossing to the next line start).
    fn word_right_of(&self, (row, col): (usize, usize)) -> (usize, usize) {
        let chars: Vec<char> = self.lines[row].chars().collect();
        if col >= chars.len() {
            return if row + 1 < self.lines.len() {
                (row + 1, 0)
            } else {
                (row, chars.len())
            };
        }
        let mut i = col;
        let class = class_of(chars[i]);
        if class != CharClass::Space {
            while i < chars.len() && class_of(chars[i]) == class {
                i += 1;
            }
        }
        while i < chars.len() && class_of(chars[i]) == CharClass::Space {
            i += 1;
        }
        (row, i)
    }

    /// Move the cursor to the start of the previous word.
    pub fn word_left(&mut self) {
        self.cursor = self.word_left_of(self.cursor);
    }

    /// Move the cursor to the start of the next word.
    pub fn word_right(&mut self) {
        self.cursor = self.word_right_of(self.cursor);
    }

    /// Delete from the previous word start to the cursor (or the selection).
    pub fn delete_word_left(&mut self) {
        if self.read_only || self.delete_selection_step() {
            return;
        }
        let start = self.word_left_of(self.cursor);
        if start != self.cursor {
            self.push_undo(EditKind::Other);
            self.remove_range(start, self.cursor);
        }
    }

    /// Delete from the cursor to the next word start (or the selection).
    pub fn delete_word_right(&mut self) {
        if self.read_only || self.delete_selection_step() {
            return;
        }
        let end = self.word_right_of(self.cursor);
        if end != self.cursor {
            self.push_undo(EditKind::Other);
            self.remove_range(self.cursor, end);
        }
    }

    /// Scroll the viewport by `delta` lines, dragging the cursor along so it stays visible.
    pub fn scroll_by(&mut self, delta: i32, height: usize) {
        let height = height.max(1);
        let max_scroll = self.lines.len().saturating_sub(height);
        let next = (self.scroll as i64 + i64::from(delta)).clamp(0, max_scroll as i64) as usize;
        self.scroll = next;
        if self.cursor.0 < self.scroll {
            self.cursor.0 = self.scroll;
            self.clamp_col();
        } else if self.cursor.0 >= self.scroll + height {
            self.cursor.0 = self.scroll + height - 1;
            self.clamp_col();
        }
    }

    /// Buffer contents joined with newlines.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Write the buffer back to its file.
    pub fn save(&mut self) -> io::Result<()> {
        if self.read_only {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "buffer is read-only",
            ));
        }
        let text = self.text_for_disk();
        fs::write(&self.path, self.with_eol(text.clone()))?;
        self.base = text;
        self.dirty = false;
        self.last_edit = None;
        self.mark_disk_seen();
        Ok(())
    }

    /// Keep the cursor inside the viewport of `height` rows.
    pub fn ensure_visible(&mut self, height: usize) {
        let height = height.max(1);
        if self.cursor.0 < self.scroll {
            self.scroll = self.cursor.0;
        } else if self.cursor.0 >= self.scroll + height {
            self.scroll = self.cursor.0 + 1 - height;
        }
    }

    fn line_len(&self, row: usize) -> usize {
        self.lines.get(row).map_or(0, |l| l.chars().count())
    }

    fn clamp_col(&mut self) {
        let len = self.line_len(self.cursor.0);
        if self.cursor.1 > len {
            self.cursor.1 = len;
        }
    }

    fn byte_index(line: &str, col: usize) -> usize {
        line.char_indices().nth(col).map_or(line.len(), |(i, _)| i)
    }

    /// Move the cursor left (wrapping to the previous line end).
    pub fn move_left(&mut self) {
        if self.cursor.1 > 0 {
            self.cursor.1 -= 1;
        } else if self.cursor.0 > 0 {
            self.cursor.0 -= 1;
            self.cursor.1 = self.line_len(self.cursor.0);
        }
    }

    /// Move the cursor right (wrapping to the next line start).
    pub fn move_right(&mut self) {
        if self.cursor.1 < self.line_len(self.cursor.0) {
            self.cursor.1 += 1;
        } else if self.cursor.0 + 1 < self.lines.len() {
            self.cursor.0 += 1;
            self.cursor.1 = 0;
        }
    }

    /// Move the cursor up.
    pub fn move_up(&mut self) {
        if self.cursor.0 > 0 {
            self.cursor.0 -= 1;
            self.clamp_col();
        }
    }

    /// Move the cursor down.
    pub fn move_down(&mut self) {
        if self.cursor.0 + 1 < self.lines.len() {
            self.cursor.0 += 1;
            self.clamp_col();
        }
    }

    /// Move by a page.
    pub fn page_up(&mut self, height: usize) {
        self.cursor.0 = self.cursor.0.saturating_sub(height.max(1));
        self.scroll = self.scroll.saturating_sub(height.max(1));
        self.clamp_col();
    }

    /// Move by a page.
    pub fn page_down(&mut self, height: usize) {
        self.cursor.0 = (self.cursor.0 + height.max(1)).min(self.lines.len() - 1);
        self.clamp_col();
    }

    /// Jump to line start.
    pub fn home(&mut self) {
        self.cursor.1 = 0;
    }

    /// Jump to line end.
    pub fn end(&mut self) {
        self.cursor.1 = self.line_len(self.cursor.0);
    }

    /// Jump to the top of the buffer.
    pub fn top(&mut self) {
        self.cursor = (0, 0);
    }

    /// Jump to the bottom of the buffer.
    pub fn bottom(&mut self) {
        self.cursor.0 = self.lines.len() - 1;
        self.end();
    }

    /// Insert a character at the cursor.
    pub fn insert_char(&mut self, c: char) {
        if self.read_only {
            return;
        }
        let kind = if c.is_whitespace() {
            EditKind::Other
        } else {
            EditKind::Type
        };
        if self.delete_selection_step() {
            // Typing over a selection undoes together with the deletion.
            self.last_edit = Some(kind);
        } else {
            self.push_undo(kind);
        }
        let (row, col) = self.cursor;
        let line = &mut self.lines[row];
        let idx = Self::byte_index(line, col);
        line.insert(idx, c);
        self.cursor.1 += 1;
        self.dirty = true;
        self.revision += 1;
    }

    /// Insert a string (e.g. a paste) at the cursor, honouring newlines.
    pub fn insert_str(&mut self, s: &str) {
        if self.read_only {
            return;
        }
        self.push_undo(EditKind::Other);
        self.batching = true;
        self.delete_selection_step();
        for c in s.chars() {
            match c {
                '\n' => self.insert_newline(),
                '\r' => {}
                c => self.insert_char(c),
            }
        }
        self.batching = false;
        self.last_edit = Some(EditKind::Other);
    }

    /// Enter in code: split the line and keep its indentation, one more level
    /// after an opening bracket; between `{` and `}` the closing one moves to
    /// its own line.
    pub fn insert_newline_indented(&mut self, unit: &str) {
        if self.read_only {
            return;
        }
        if !self.delete_selection_step() {
            self.push_undo(EditKind::Other);
        }
        let (row, col) = self.cursor;
        let line = &self.lines[row];
        let idx = Self::byte_index(line, col);
        let lead: String = line[..idx]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let opens = matches!(line[..idx].trim_end().chars().last(), Some('{' | '(' | '['));
        let rest = self.lines[row].split_off(idx);
        let rest = rest.trim_start().to_owned();
        let closes = opens && matches!(rest.chars().next(), Some('}' | ')' | ']'));
        let inner = if opens {
            format!("{lead}{unit}")
        } else {
            lead.clone()
        };
        let cursor_col = inner.chars().count();
        if closes {
            self.lines.insert(row + 1, inner);
            self.lines.insert(row + 2, format!("{lead}{rest}"));
        } else {
            self.lines.insert(row + 1, format!("{inner}{rest}"));
        }
        self.cursor = (row + 1, cursor_col);
        self.dirty = true;
        self.revision += 1;
    }

    /// Tab: indent the selected lines, or insert indentation at the cursor
    /// (spaces up to the next stop).
    pub fn indent(&mut self, unit: &str) {
        if self.read_only {
            return;
        }
        match self.selection() {
            Some((a, b)) if a.0 != b.0 => self.shift_lines(a, b, true, unit),
            _ if unit == "\t" => self.insert_char('\t'),
            _ => {
                let w = unit.len().max(1);
                let (row, col) = self.cursor;
                let disp = display_col(&self.lines[row], col, w);
                self.insert_str(&" ".repeat(w - disp % w));
            }
        }
    }

    /// Shift+Tab: remove one level of indentation from the selected lines (or
    /// the cursor's line).
    pub fn outdent(&mut self, unit: &str) {
        if self.read_only {
            return;
        }
        let (a, b) = self.selection().unwrap_or((self.cursor, self.cursor));
        self.shift_lines(a, b, false, unit);
    }

    /// Indent or outdent lines `a.0..=b.0` (not `b`'s line when the selection
    /// ends at its start), keeping cursor and selection on the same text.
    fn shift_lines(&mut self, a: (usize, usize), b: (usize, usize), indent: bool, unit: &str) {
        let last = if b.0 > a.0 && b.1 == 0 { b.0 - 1 } else { b.0 };
        let width = if unit == "\t" { 1 } else { unit.len().max(1) };
        let mut deltas = Vec::new();
        for row in a.0..=last {
            let line = &self.lines[row];
            let delta: i64 = if indent {
                if line.is_empty() {
                    0
                } else {
                    unit.chars().count() as i64
                }
            } else if line.starts_with('\t') {
                -1
            } else {
                -(line.chars().take(width).take_while(|c| *c == ' ').count() as i64)
            };
            deltas.push((row, delta));
        }
        if deltas.iter().all(|(_, d)| *d == 0) {
            return;
        }
        self.push_undo(EditKind::Other);
        for &(row, delta) in &deltas {
            let line = &mut self.lines[row];
            if delta > 0 {
                line.insert_str(0, unit);
            } else if delta < 0 {
                line.drain(..(-delta) as usize);
            }
        }
        // Positions move with their text; a line start stays at the start, so
        // a selection from column 0 grows to take the new indentation in.
        let shift = |(r, c): (usize, usize)| {
            let d = deltas
                .iter()
                .find(|(row, _)| *row == r)
                .map_or(0, |(_, d)| *d);
            if d > 0 && c == 0 {
                (r, 0)
            } else {
                (r, (c as i64 + d).max(0) as usize)
            }
        };
        let (cursor, anchor) = (shift(self.cursor), self.anchor.map(shift));
        self.cursor = cursor;
        self.anchor = anchor;
        self.set_cursor(self.cursor.0, self.cursor.1);
        self.dirty = true;
        self.revision += 1;
    }

    /// Replace char ranges `(row, start, end, text)` on single lines, all as
    /// one undo step. Ranges must not overlap. Returns how many were replaced.
    pub fn replace_ranges(&mut self, edits: &[(usize, usize, usize, String)]) -> usize {
        if self.read_only || edits.is_empty() {
            return 0;
        }
        self.push_undo(EditKind::Other);
        let mut sorted: Vec<&(usize, usize, usize, String)> = edits.iter().collect();
        sorted.sort_by(|a, b| (b.0, b.1).cmp(&(a.0, a.1)));
        let mut n = 0;
        for (row, start, end, text) in sorted {
            let Some(line) = self.lines.get_mut(*row) else {
                continue;
            };
            let s = Self::byte_index(line, *start);
            let e = Self::byte_index(line, *end);
            line.replace_range(s..e, text);
            n += 1;
        }
        self.anchor = None;
        self.dirty = true;
        self.revision += 1;
        self.set_cursor(self.cursor.0, self.cursor.1);
        n
    }

    /// Put the cursor on `line` (1-based) and `col` (1-based), clamped.
    pub fn goto(&mut self, line: usize, col: usize) {
        self.anchor = None;
        self.set_cursor(line.saturating_sub(1), col.saturating_sub(1));
    }

    /// Split the line at the cursor.
    pub fn insert_newline(&mut self) {
        if self.read_only {
            return;
        }
        if !self.delete_selection_step() {
            self.push_undo(EditKind::Other);
        }
        let (row, col) = self.cursor;
        let idx = Self::byte_index(&self.lines[row], col);
        let rest = self.lines[row].split_off(idx);
        self.lines.insert(row + 1, rest);
        self.cursor = (row + 1, 0);
        self.dirty = true;
        self.revision += 1;
    }

    /// Delete the character before the cursor (joining lines at column 0).
    pub fn backspace(&mut self) {
        if self.read_only || self.delete_selection_step() || self.cursor == (0, 0) {
            return;
        }
        self.push_undo(EditKind::Other);
        let (row, col) = self.cursor;
        if col > 0 {
            let idx = Self::byte_index(&self.lines[row], col - 1);
            self.lines[row].remove(idx);
            self.cursor.1 -= 1;
            self.dirty = true;
            self.revision += 1;
        } else if row > 0 {
            let line = self.lines.remove(row);
            let prev_len = self.line_len(row - 1);
            self.lines[row - 1].push_str(&line);
            self.cursor = (row - 1, prev_len);
            self.dirty = true;
            self.revision += 1;
        }
    }

    /// Delete the character under the cursor (joining lines at the end).
    pub fn delete(&mut self) {
        if self.read_only || self.delete_selection_step() {
            return;
        }
        self.push_undo(EditKind::Other);
        let (row, col) = self.cursor;
        if col < self.line_len(row) {
            let idx = Self::byte_index(&self.lines[row], col);
            self.lines[row].remove(idx);
            self.dirty = true;
            self.revision += 1;
        } else if row + 1 < self.lines.len() {
            let next = self.lines.remove(row + 1);
            self.lines[row].push_str(&next);
            self.dirty = true;
            self.revision += 1;
        }
    }
}

/// Expand tabs to spaces for display.
pub fn expand_tabs(line: &str, tab_width: usize) -> String {
    let tab_width = tab_width.max(1);
    if !line.contains('\t') {
        return line.to_owned();
    }
    let mut out = String::new();
    let mut col = 0;
    for c in line.chars() {
        if c == '\t' {
            let n = tab_width - (col % tab_width);
            out.push_str(&" ".repeat(n));
            col += n;
        } else {
            out.push(c);
            col += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        }
    }
    out
}

/// Display column of char index `col` in `line` after tab expansion.
pub fn display_col(line: &str, col: usize, tab_width: usize) -> usize {
    let prefix: String = line.chars().take(col).collect();
    unicode_width::UnicodeWidthStr::width(expand_tabs(&prefix, tab_width).as_str())
}

/// Char index whose cell covers display column `disp` (clamped to the line end).
pub fn char_col_at(line: &str, disp: usize, tab_width: usize) -> usize {
    let tab_width = tab_width.max(1);
    let mut col = 0;
    for (i, c) in line.chars().enumerate() {
        let w = if c == '\t' {
            tab_width - (col % tab_width)
        } else {
            unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)
        };
        if disp < col + w {
            return i;
        }
        col += w;
    }
    line.chars().count()
}

fn hex_dump(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for (i, chunk) in bytes.chunks(16).enumerate() {
        let _ = write!(out, "{:08x}  ", i * 16);
        for b in chunk {
            let _ = write!(out, "{b:02x} ");
        }
        for _ in chunk.len()..16 {
            out.push_str("   ");
        }
        out.push(' ');
        for b in chunk {
            out.push(if b.is_ascii_graphic() || *b == b' ' {
                *b as char
            } else {
                '.'
            });
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(text: &str) -> Buffer {
        Buffer::from_text(Path::new("/tmp/x.txt"), text, false)
    }

    #[test]
    fn from_text_handles_trailing_newline() {
        assert_eq!(buf("a\nb\n").lines(), &["a", "b", ""]);
        assert_eq!(buf("a\nb").lines(), &["a", "b"]);
        assert_eq!(buf("").lines(), &[""]);
    }

    #[test]
    fn insert_and_newline() {
        let mut b = buf("héllo");
        b.end();
        b.insert_char('!');
        assert_eq!(b.text(), "héllo!");
        b.cursor = (0, 2);
        b.insert_newline();
        assert_eq!(b.lines(), &["hé", "llo!"]);
        assert_eq!(b.cursor(), (1, 0));
        assert!(b.is_dirty());
    }

    #[test]
    fn backspace_and_delete_join_lines() {
        let mut b = buf("ab\ncd");
        b.move_down();
        b.backspace();
        assert_eq!(b.text(), "abcd");
        assert_eq!(b.cursor(), (0, 2));
        b.insert_newline();
        b.move_up();
        b.end();
        b.delete();
        assert_eq!(b.text(), "abcd");
    }

    #[test]
    fn movement_wraps_and_clamps() {
        let mut b = buf("long line\nx");
        b.end();
        b.move_down();
        assert_eq!(b.cursor(), (1, 1));
        b.move_right();
        assert_eq!(b.cursor(), (1, 1));
        b.move_left();
        b.move_left();
        assert_eq!(b.cursor(), (0, 9));
        b.top();
        assert_eq!(b.cursor(), (0, 0));
        b.bottom();
        assert_eq!(b.cursor(), (1, 1));
    }

    #[test]
    fn scrolling_follows_cursor() {
        let mut b = buf(&(0..50)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n"));
        b.page_down(10);
        b.page_down(10);
        b.ensure_visible(10);
        assert_eq!(b.cursor().0, 20);
        assert_eq!(b.scroll(), 11);
        b.top();
        b.ensure_visible(10);
        assert_eq!(b.scroll(), 0);
    }

    #[test]
    fn display_helpers() {
        assert_eq!(expand_tabs("a\tb", 4), "a   b");
        assert_eq!(display_col("a\tb", 2, 4), 4);
        assert_eq!(display_col("你好", 1, 4), 2);
        assert_eq!(char_col_at("a\tb", 0, 4), 0);
        assert_eq!(char_col_at("a\tb", 2, 4), 1);
        assert_eq!(char_col_at("a\tb", 4, 4), 2);
        assert_eq!(char_col_at("a\tb", 40, 4), 3);
        assert_eq!(char_col_at("你好x", 3, 4), 1);
        assert_eq!(char_col_at("你好x", 4, 4), 2);
    }

    #[test]
    fn set_cursor_and_scroll_by() {
        let mut b = buf(&(0..30)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n"));
        b.set_cursor(99, 99);
        assert_eq!(b.cursor(), (29, 7));
        b.set_cursor(3, 2);
        b.scroll_by(10, 5);
        assert_eq!(b.scroll(), 10);
        assert_eq!(b.cursor().0, 10);
        b.scroll_by(-100, 5);
        assert_eq!(b.scroll(), 0);
        assert_eq!(b.cursor().0, 4);
        let r = b.revision();
        b.insert_char('x');
        assert_eq!(b.revision(), r + 1);
    }

    #[test]
    fn save_writes_file_and_clears_dirty() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.txt");
        fs::write(&p, "one\n").unwrap();
        let mut b = Buffer::open(&p).unwrap();
        b.insert_str("zero\n");
        b.save().unwrap();
        assert!(!b.is_dirty());
        assert_eq!(fs::read_to_string(&p).unwrap(), "zero\none\n");
    }

    #[test]
    fn undo_redo_groups_typing() {
        let mut b = buf("x");
        b.end();
        for c in "abc".chars() {
            b.insert_char(c);
        }
        b.insert_char(' ');
        for c in "de".chars() {
            b.insert_char(c);
        }
        assert_eq!(b.text(), "xabc de");
        assert!(b.undo());
        assert_eq!(b.text(), "xabc ");
        assert!(b.undo());
        assert_eq!(b.text(), "xabc");
        assert!(b.undo());
        assert_eq!(b.text(), "x");
        assert!(!b.is_dirty(), "back to the loaded text");
        assert!(!b.undo());
        assert!(b.redo());
        assert_eq!(b.text(), "xabc");
        b.insert_str("1\n2");
        assert_eq!(b.text(), "xabc1\n2");
        assert!(b.undo());
        assert_eq!(b.text(), "xabc");
        assert!(!b.redo() || b.text() == "xabc1\n2");
    }

    #[test]
    fn replace_saved_keeps_cursor_and_base() {
        let mut b = buf("a\nb\n");
        b.set_cursor(1, 1);
        b.insert_char('x');
        b.replace_saved("a\nbx\nc\n");
        assert!(!b.is_dirty());
        assert_eq!(b.base_text(), "a\nbx\nc\n");
        assert_eq!(b.cursor(), (1, 2));
    }

    #[test]
    fn word_movement_skips_words_spaces_and_punctuation() {
        let mut b = buf("let foo_bar = x.len();\nnext");
        b.word_right();
        assert_eq!(b.cursor(), (0, 4));
        b.word_right();
        assert_eq!(b.cursor(), (0, 12));
        b.word_right();
        assert_eq!(b.cursor(), (0, 14));
        b.word_right();
        assert_eq!(b.cursor(), (0, 15));
        b.end();
        b.word_right();
        assert_eq!(b.cursor(), (1, 0), "line end crosses to the next line");
        b.word_left();
        assert_eq!(b.cursor(), (0, 22));
        b.word_left();
        assert_eq!(b.cursor(), (0, 19));
        b.set_cursor(0, 7);
        b.word_left();
        assert_eq!(b.cursor(), (0, 4));
        b.word_left();
        b.word_left();
        assert_eq!(b.cursor(), (0, 0));
    }

    #[test]
    fn selection_extends_copies_and_is_replaced_by_typing() {
        let mut b = buf("hello world\nsecond line");
        b.select(true);
        b.word_right();
        assert_eq!(b.selected_text().as_deref(), Some("hello "));
        b.select(true);
        b.move_down();
        assert_eq!(b.selected_text().as_deref(), Some("hello world\nsecond"));
        b.select(false);
        assert_eq!(b.selection(), None);

        b.select_range((0, 6), (0, 11));
        b.insert_char('W');
        b.insert_char('!');
        assert_eq!(b.lines()[0], "hello W!");
        assert!(b.undo(), "typing over a selection is one undo step");
        assert_eq!(b.text(), "hello world\nsecond line");

        // A backwards selection (anchor after the cursor) works the same.
        b.set_cursor(1, 6);
        b.select(true);
        b.move_up();
        b.home();
        assert_eq!(b.selected_text().as_deref(), Some("hello world\nsecond"));
        b.backspace();
        assert_eq!(b.text(), " line");
        assert_eq!(b.cursor(), (0, 0));
    }

    #[test]
    fn cut_paste_select_all_and_word_deletes() {
        let mut b = buf("one two\nthree");
        b.select_all();
        assert_eq!(b.selected_text().as_deref(), Some("one two\nthree"));
        assert_eq!(b.cut().as_deref(), Some("one two\nthree"));
        assert_eq!(b.text(), "");
        b.insert_str("alpha beta\ngamma");
        b.select_range((0, 2), (1, 2));
        b.insert_str("XY");
        assert_eq!(b.text(), "alXYmma");
        assert!(b.undo());
        assert_eq!(b.text(), "alpha beta\ngamma");

        b.set_cursor(0, 10);
        b.delete_word_left();
        assert_eq!(b.lines()[0], "alpha ");
        b.home();
        b.delete_word_right();
        assert_eq!(b.lines()[0], "");
        b.delete_word_right();
        assert_eq!(b.text(), "gamma", "at a line end the line break goes");

        b.set_cursor(0, 2);
        b.select_word();
        assert_eq!(b.selected_text().as_deref(), Some("gamma"));
        b.select_line();
        assert_eq!(b.selected_text().as_deref(), Some("gamma"));
        assert_eq!(b.cut(), Some("gamma".into()));
        assert_eq!(b.cut(), None, "nothing selected");
    }

    #[test]
    fn read_only_selection_copies_but_does_not_cut() {
        let mut b = Buffer::from_text(Path::new("/tmp/x"), "keep me", true);
        b.select_all();
        assert_eq!(b.selected_text().as_deref(), Some("keep me"));
        assert_eq!(b.cut(), None);
        b.delete_word_left();
        assert_eq!(b.text(), "keep me");
    }

    #[test]
    fn indentation_is_detected_and_used() {
        let tabs = buf("int f() {\n\treturn 0;\n}\n");
        assert_eq!(tabs.indent_unit(4), "\t");
        let four = buf("def f():\n    if x:\n        y\n    z\n");
        assert_eq!(four.indent_unit(8), "    ");
        let two = buf("a:\n  b:\n    c\n  d\n");
        assert_eq!(two.indent_unit(8), "  ");
        assert_eq!(
            buf("plain\ntext\n").indent_unit(3),
            "   ",
            "the setting when unknown"
        );
        let make = Buffer::from_text(Path::new("/x/Makefile"), "all:\n", false);
        assert_eq!(make.indent_unit(4), "\t");
        let c = buf("/**\n * doc\n */\nint x;\n");
        assert_eq!(
            c.indent_unit(4),
            "    ",
            "doc comment stars are not indentation"
        );
    }

    #[test]
    fn enter_keeps_indentation_and_opens_blocks() {
        let mut b = buf("    if (x) {}");
        b.set_cursor(0, 12); // between { and }
        b.insert_newline_indented("    ");
        assert_eq!(b.lines(), &["    if (x) {", "        ", "    }"]);
        assert_eq!(b.cursor(), (1, 8));
        b.insert_str("y();");
        b.insert_newline_indented("    ");
        assert_eq!(b.lines()[2], "        ");
        assert!(b.undo());
        assert_eq!(b.lines()[1], "        y();");
        let mut t = buf("\tcall(a,");
        t.end();
        t.insert_newline_indented("\t");
        assert_eq!(t.lines(), &["\tcall(a,", "\t"]);
    }

    #[test]
    fn tab_and_shift_tab_indent_lines_or_insert() {
        let mut b = buf("ab\ncd\nef");
        b.set_cursor(0, 1);
        b.indent("    ");
        assert_eq!(b.lines()[0], "a   b", "spaces up to the next stop");
        b.select_range((0, 0), (1, 1));
        b.indent("    ");
        assert_eq!(b.lines(), &["    a   b", "    cd", "ef"]);
        assert_eq!(b.selected_text().as_deref(), Some("    a   b\n    c"));
        b.outdent("    ");
        b.outdent("    ");
        assert_eq!(b.lines(), &["a   b", "cd", "ef"]);
        // A selection ending at a line start leaves that line alone.
        b.select_range((1, 0), (2, 0));
        b.indent("\t");
        assert_eq!(b.lines(), &["a   b", "\tcd", "ef"]);
        assert!(b.undo());
        assert_eq!(b.lines()[1], "cd");
        b.select(false);
        b.goto(3, 2);
        assert_eq!(b.cursor(), (2, 1));
        b.goto(99, 99);
        assert_eq!(b.cursor(), (2, 2));
    }

    #[test]
    fn crlf_is_kept_and_outside_changes_are_seen() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("win.txt");
        fs::write(&p, "one\r\ntwo\r\n").unwrap();
        let mut b = Buffer::open(&p).unwrap();
        assert_eq!(b.lines(), &["one", "two", ""]);
        b.insert_str("zero ");
        b.save().unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "zero one\r\ntwo\r\n");
        assert!(!b.changed_on_disk());
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&p, "changed\n").unwrap();
        let f = fs::File::options().write(true).open(&p).unwrap();
        f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2))
            .unwrap();
        assert!(b.changed_on_disk());
        b.reload().unwrap();
        assert_eq!(b.text(), "changed\n");
        assert!(!b.changed_on_disk() && !b.is_dirty());
    }

    #[test]
    fn binary_opens_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bin");
        fs::write(&p, [0u8, 1, 2, 255]).unwrap();
        let mut b = Buffer::open(&p).unwrap();
        assert!(b.is_read_only());
        b.insert_char('x');
        assert!(!b.is_dirty());
        assert!(b.save().is_err());
        assert!(b.lines()[0].starts_with("00000000  00 01 02 ff"));
    }
}
