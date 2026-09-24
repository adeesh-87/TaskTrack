//! A deliberately small text buffer: enough to view and edit one file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        if lines.is_empty() || text.ends_with('\n') {
            lines.push(String::new());
        }
        Self {
            path: path.to_path_buf(),
            lines,
            cursor: (0, 0),
            scroll: 0,
            dirty: false,
            read_only,
            revision: 0,
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
        let mut text = self.text();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        fs::write(&self.path, text)?;
        self.dirty = false;
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
        for c in s.chars() {
            match c {
                '\n' => self.insert_newline(),
                '\r' => {}
                c => self.insert_char(c),
            }
        }
    }

    /// Split the line at the cursor.
    pub fn insert_newline(&mut self) {
        if self.read_only {
            return;
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
        if self.read_only {
            return;
        }
        let (row, col) = self.cursor;
        if col > 0 {
            let idx = Self::byte_index(&self.lines[row], col - 1);
            self.lines[row].remove(idx);
            self.cursor.1 -= 1;
            self.dirty = true;
            self.revision += 1;
            self.revision += 1;
        } else if row > 0 {
            let line = self.lines.remove(row);
            let prev_len = self.line_len(row - 1);
            self.lines[row - 1].push_str(&line);
            self.cursor = (row - 1, prev_len);
            self.dirty = true;
            self.revision += 1;
            self.revision += 1;
        }
    }

    /// Delete the character under the cursor (joining lines at the end).
    pub fn delete(&mut self) {
        if self.read_only {
            return;
        }
        let (row, col) = self.cursor;
        if col < self.line_len(row) {
            let idx = Self::byte_index(&self.lines[row], col);
            self.lines[row].remove(idx);
            self.dirty = true;
            self.revision += 1;
            self.revision += 1;
        } else if row + 1 < self.lines.len() {
            let next = self.lines.remove(row + 1);
            self.lines[row].push_str(&next);
            self.dirty = true;
            self.revision += 1;
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
