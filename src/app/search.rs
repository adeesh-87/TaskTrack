//! Search in files (Ctrl+Shift+F): a regex or text search over the task
//! folder and its attached code workspaces and builds, with include and
//! exclude globs. It runs in the background (the `ignore` crate walks the
//! folders the way ripgrep does, skipping `.gitignore`d files); results are
//! grouped by file, Enter opens one and F4 / Shift+F4 step through them.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use regex::Regex;

use super::event::{AppEvent, EventSender, JobEvent};
use super::find::build_regex;
use super::popup::Popup;
use super::{App, Focus};

/// Most matches one search keeps.
pub const MAX_MATCHES: usize = 20_000;
/// Most matches kept per file.
const MAX_PER_FILE: usize = 1_000;
/// Files larger than this are skipped.
const MAX_FILE: u64 = 8 * 1024 * 1024;
/// Chars of a matching line kept.
const MAX_LINE: usize = 300;

/// One match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// Line (0-based).
    pub line: usize,
    /// First char of the match.
    pub start: usize,
    /// Char after the match.
    pub end: usize,
    /// The line (cut at [`MAX_LINE`] chars).
    pub text: String,
}

/// The matches in one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHits {
    /// The file.
    pub path: PathBuf,
    /// Shown as (root label / relative path).
    pub label: String,
    /// Matches, in order.
    pub hits: Vec<SearchHit>,
}

/// A folder searched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRoot {
    /// Shown name.
    pub label: String,
    /// Folder.
    pub path: PathBuf,
    /// Searched.
    pub on: bool,
}

/// Which part of the panel has the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// Search text.
    Query,
    /// Globs of files to search.
    Include,
    /// Globs of files and folders to skip.
    Exclude,
    /// The folder switches.
    Roots,
    /// The result list.
    Results,
}

impl Field {
    const ORDER: [Field; 5] = [
        Field::Query,
        Field::Include,
        Field::Exclude,
        Field::Roots,
        Field::Results,
    ];

    fn step(self, forward: bool) -> Self {
        let i = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
        let n = Self::ORDER.len();
        Self::ORDER[if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        }]
    }
}

/// The search panel's state (kept while it is closed, for F4).
#[derive(Debug)]
pub struct SearchPanel {
    /// Search text.
    pub query: String,
    /// Regular expression (else literal text).
    pub regex: bool,
    /// Match case.
    pub case: bool,
    /// Whole words.
    pub word: bool,
    /// Comma-separated globs of files to search (empty: all).
    pub include: String,
    /// Comma-separated globs to skip.
    pub exclude: String,
    /// Folders, with their switches.
    pub roots: Vec<SearchRoot>,
    /// Root selected in the Roots field.
    pub root_cursor: usize,
    /// Only under this folder (Ctrl+F in the file tree), if set.
    pub scope: Option<PathBuf>,
    /// Part with the keys.
    pub field: Field,
    /// Results, sorted by label.
    pub files: Vec<FileHits>,
    /// Selected row of [`Self::rows`].
    pub selected: usize,
    /// First row shown.
    pub scroll: usize,
    /// The match F4 moved to last (file, hit).
    pub current: Option<(usize, usize)>,
    /// A line about the search (counts, errors).
    pub note: String,
    /// A search is running.
    pub running: bool,
    /// Stopped at [`MAX_MATCHES`].
    pub truncated: bool,
    id: u64,
    cancel: Option<Arc<AtomicBool>>,
    rows: Vec<(usize, Option<usize>)>,
    rows_dirty: bool,
}

impl SearchPanel {
    fn new(roots: Vec<SearchRoot>) -> Self {
        Self {
            query: String::new(),
            regex: false,
            case: false,
            word: false,
            include: String::new(),
            exclude: String::new(),
            roots,
            root_cursor: 0,
            scope: None,
            field: Field::Query,
            files: Vec::new(),
            selected: 0,
            scroll: 0,
            current: None,
            note: String::new(),
            running: false,
            truncated: false,
            id: 0,
            cancel: None,
            rows: Vec::new(),
            rows_dirty: false,
        }
    }

    /// Result rows: (file, `None`) for a file header, (file, `Some(hit)`) for a match.
    pub fn rows(&mut self) -> &[(usize, Option<usize>)] {
        if self.rows_dirty {
            self.rows.clear();
            for (f, file) in self.files.iter().enumerate() {
                self.rows.push((f, None));
                self.rows.extend((0..file.hits.len()).map(|h| (f, Some(h))));
            }
            self.rows_dirty = false;
            self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        }
        &self.rows
    }

    /// Number of matches.
    pub fn match_count(&self) -> usize {
        self.files.iter().map(|f| f.hits.len()).sum()
    }

    fn add(&mut self, file: FileHits) {
        let at = self.files.partition_point(|f| f.label < file.label);
        self.files.insert(at, file);
        self.rows_dirty = true;
        self.current = None;
    }

    /// The match `forward` / backward from the current one (wrapping).
    fn next_match(&self, forward: bool) -> Option<(usize, usize)> {
        if self.files.is_empty() {
            return None;
        }
        let Some((f, h)) = self.current else {
            return if forward {
                Some((0, 0))
            } else {
                let f = self.files.len() - 1;
                Some((f, self.files[f].hits.len() - 1))
            };
        };
        if forward {
            if h + 1 < self.files[f].hits.len() {
                Some((f, h + 1))
            } else {
                Some(((f + 1) % self.files.len(), 0))
            }
        } else if h > 0 {
            Some((f, h - 1))
        } else {
            let f = (f + self.files.len() - 1) % self.files.len();
            Some((f, self.files[f].hits.len() - 1))
        }
    }

    fn globs(text: &str) -> Vec<String> {
        text.split(',')
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .map(str::to_owned)
            .collect()
    }

    fn field_text(&mut self) -> Option<&mut String> {
        match self.field {
            Field::Query => Some(&mut self.query),
            Field::Include => Some(&mut self.include),
            Field::Exclude => Some(&mut self.exclude),
            Field::Roots | Field::Results => None,
        }
    }
}

/// What one search looks for and where.
pub struct SearchJob {
    /// Result id (older results are dropped).
    pub id: u64,
    /// Pattern.
    pub re: Regex,
    /// (label, folder to walk, the root it is in).
    pub roots: Vec<(String, PathBuf, PathBuf)>,
    /// Globs of files to search.
    pub include: Vec<String>,
    /// Globs to skip.
    pub exclude: Vec<String>,
    /// Set to stop.
    pub cancel: Arc<AtomicBool>,
}

/// The matches of `re` in `text`, one line at a time.
pub fn search_text(re: &Regex, text: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    for (line, l) in text.lines().enumerate() {
        for m in re.find_iter(l) {
            if m.start() == m.end() {
                continue;
            }
            let start = l[..m.start()].chars().count();
            let end = start + l[m.start()..m.end()].chars().count();
            hits.push(SearchHit {
                line,
                start,
                end,
                text: l.chars().take(MAX_LINE).collect(),
            });
            if hits.len() >= MAX_PER_FILE {
                return hits;
            }
        }
    }
    hits
}

/// Run a search, sending one [`JobEvent::SearchHits`] per matching file and
/// a [`JobEvent::SearchDone`] at the end.
pub fn run(job: &SearchJob, events: &EventSender) {
    let found = Arc::new(AtomicUsize::new(0));
    let searched = Arc::new(AtomicUsize::new(0));
    let mut error = None;
    for (label, walk_root, root) in &job.roots {
        let mut overrides = ignore::overrides::OverrideBuilder::new(walk_root);
        let globs = job
            .include
            .iter()
            .cloned()
            .chain(job.exclude.iter().map(|g| format!("!{g}")));
        for g in globs {
            if let Err(e) = overrides.add(&g) {
                error = Some(format!("glob {g}: {e}"));
            }
        }
        let overrides = match overrides.build() {
            Ok(o) => o,
            Err(e) => {
                error = Some(format!("globs: {e}"));
                continue;
            }
        };
        let walker = ignore::WalkBuilder::new(walk_root)
            .hidden(true)
            .git_ignore(true)
            .git_exclude(true)
            .require_git(false)
            .overrides(overrides)
            .build_parallel();
        walker.run(|| {
            let found = found.clone();
            let searched = searched.clone();
            let cancel = job.cancel.clone();
            let re = job.re.clone();
            let events = events.clone();
            let (label, root, id) = (label.clone(), root.clone(), job.id);
            Box::new(move |entry| {
                if cancel.load(Ordering::Relaxed) || found.load(Ordering::Relaxed) >= MAX_MATCHES {
                    return ignore::WalkState::Quit;
                }
                let Ok(entry) = entry else {
                    return ignore::WalkState::Continue;
                };
                if !entry.file_type().is_some_and(|t| t.is_file())
                    || entry.metadata().map_or(true, |m| m.len() > MAX_FILE)
                {
                    return ignore::WalkState::Continue;
                }
                let Ok(bytes) = std::fs::read(entry.path()) else {
                    return ignore::WalkState::Continue;
                };
                searched.fetch_add(1, Ordering::Relaxed);
                if bytes.iter().take(8192).any(|b| *b == 0) {
                    return ignore::WalkState::Continue;
                }
                let hits = search_text(&re, &String::from_utf8_lossy(&bytes));
                if hits.is_empty() {
                    return ignore::WalkState::Continue;
                }
                found.fetch_add(hits.len(), Ordering::Relaxed);
                let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
                let rel = rel.to_string_lossy().replace('\\', "/");
                let label = if label.is_empty() {
                    rel
                } else {
                    format!("{label}/{rel}")
                };
                events.send(AppEvent::Job(JobEvent::SearchHits {
                    id,
                    file: FileHits {
                        path: entry.path().to_path_buf(),
                        label,
                        hits,
                    },
                }));
                ignore::WalkState::Continue
            })
        });
    }
    events.send(AppEvent::Job(JobEvent::SearchDone {
        id: job.id,
        searched: searched.load(Ordering::Relaxed),
        truncated: found.load(Ordering::Relaxed) >= MAX_MATCHES,
        error,
    }));
}

impl App {
    /// The search panel, if it was opened.
    pub fn search_panel(&mut self) -> Option<&mut SearchPanel> {
        self.search.as_mut()
    }

    /// The folders that can be searched: the file tree's roots; builds start
    /// switched off (they are big).
    fn search_roots(&self) -> Vec<SearchRoot> {
        let Some(ctx) = self.active_context() else {
            return Vec::new();
        };
        ctx.tree
            .roots()
            .into_iter()
            .enumerate()
            .map(|(i, (label, path))| SearchRoot {
                on: !label.ends_with(" · build"),
                label: if i == 0 { "task".into() } else { label },
                path,
            })
            .collect()
    }

    /// Ctrl+Shift+F (`scope`: Ctrl+F in the file tree, that folder only).
    pub(super) fn open_search(&mut self, scope: Option<PathBuf>) {
        if self.active_context().is_none() {
            return;
        }
        let roots = self.search_roots();
        let selected = self
            .active_context()
            .filter(|c| c.focus == Focus::Editor)
            .and_then(|c| c.editor.as_ref())
            .and_then(crate::editor::Buffer::selected_text)
            .filter(|t| !t.is_empty() && !t.contains('\n'));
        let panel = self
            .search
            .get_or_insert_with(|| SearchPanel::new(roots.clone()));
        // Keep the switches of folders still there; add new ones.
        let old = std::mem::take(&mut panel.roots);
        panel.roots = roots
            .into_iter()
            .map(|r| {
                let on = old.iter().find(|o| o.path == r.path).map_or(r.on, |o| o.on);
                SearchRoot { on, ..r }
            })
            .collect();
        panel.root_cursor = panel.root_cursor.min(panel.roots.len().saturating_sub(1));
        if scope.is_some() {
            panel.scope = scope;
        }
        if let Some(t) = selected {
            panel.query = t;
        }
        panel.field = Field::Query;
        self.popup = Some(Popup::Search);
    }

    /// Start the search with the panel's settings.
    fn start_search(&mut self) {
        let events = self.events.clone();
        let Some(p) = self.search.as_mut() else {
            return;
        };
        if let Some(c) = p.cancel.take() {
            c.store(true, Ordering::Relaxed);
        }
        p.files.clear();
        p.rows_dirty = true;
        p.selected = 0;
        p.scroll = 0;
        p.current = None;
        p.truncated = false;
        if p.query.is_empty() {
            p.running = false;
            p.note.clear();
            return;
        }
        let re = match build_regex(&p.query, p.regex, p.case, p.word) {
            Ok(re) => re,
            Err(e) => {
                p.running = false;
                p.note = format!("pattern: {e}");
                return;
            }
        };
        let roots: Vec<(String, PathBuf, PathBuf)> = p
            .roots
            .iter()
            .filter(|r| r.on)
            .filter_map(|r| {
                let label = if r.label == "task" {
                    String::new()
                } else {
                    r.label.split(" · ").next().unwrap_or(&r.label).to_owned()
                };
                match &p.scope {
                    Some(s) if s.starts_with(&r.path) => Some((label, s.clone(), r.path.clone())),
                    Some(_) => None,
                    None => Some((label, r.path.clone(), r.path.clone())),
                }
            })
            .collect();
        if roots.is_empty() {
            p.running = false;
            p.note = "no folder to search: switch one on (Tab to the folders, Space)".into();
            return;
        }
        p.id += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        p.cancel = Some(cancel.clone());
        p.running = true;
        p.note = "searching …".into();
        let job = SearchJob {
            id: p.id,
            re,
            roots,
            include: SearchPanel::globs(&p.include),
            exclude: SearchPanel::globs(&p.exclude),
            cancel,
        };
        let spawned = std::thread::Builder::new()
            .name("search".into())
            .spawn(move || run(&job, &events));
        if let Err(e) = spawned {
            p.running = false;
            p.note = format!("could not start the search: {e}");
        }
    }

    /// Matches of a file arrived.
    pub(super) fn search_hits(&mut self, id: u64, file: FileHits) {
        if let Some(p) = self.search.as_mut().filter(|p| p.id == id) {
            p.add(file);
            p.note = format!(
                "{} matches in {} files · searching …",
                p.match_count(),
                p.files.len()
            );
        }
    }

    /// The search finished.
    pub(super) fn search_done(
        &mut self,
        id: u64,
        searched: usize,
        truncated: bool,
        error: Option<String>,
    ) {
        let Some(p) = self.search.as_mut().filter(|p| p.id == id) else {
            return;
        };
        p.running = false;
        p.truncated = truncated;
        p.cancel = None;
        let more = if truncated { "+ (stopped)" } else { "" };
        p.note = format!(
            "{}{more} matches in {} files · {searched} files searched",
            p.match_count(),
            p.files.len()
        );
        if let Some(e) = error {
            p.note = format!("{} · {e}", p.note);
        }
    }

    /// Keys while the panel shows (it is `Popup::Search`, already taken).
    pub(super) fn handle_search_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let Some(p) = self.search.as_mut() else {
            return;
        };
        let mut keep = true;
        match key.code {
            KeyCode::Esc => {
                keep = false;
            }
            KeyCode::Tab => p.field = p.field.step(true),
            KeyCode::BackTab => p.field = p.field.step(false),
            KeyCode::Char('c') if alt => p.case = !p.case,
            KeyCode::Char('r') if alt => p.regex = !p.regex,
            KeyCode::Char('w') if alt => p.word = !p.word,
            KeyCode::Char('x') if alt => p.scope = None,
            KeyCode::Char('c') if ctrl => {
                if let Some(c) = &p.cancel {
                    c.store(true, Ordering::Relaxed);
                }
            }
            _ if p.field == Field::Results => {
                let rows = p.rows().len();
                match key.code {
                    KeyCode::Up if p.selected == 0 => p.field = Field::Roots,
                    KeyCode::Up => p.selected -= 1,
                    KeyCode::Down => p.selected = (p.selected + 1).min(rows.saturating_sub(1)),
                    KeyCode::PageUp => p.selected = p.selected.saturating_sub(10),
                    KeyCode::PageDown => {
                        p.selected = (p.selected + 10).min(rows.saturating_sub(1));
                    }
                    KeyCode::Home => p.selected = 0,
                    KeyCode::End => p.selected = rows.saturating_sub(1),
                    KeyCode::Enter => {
                        let sel = p.selected;
                        let row = p.rows().get(sel).copied();
                        if let Some((f, h)) = row {
                            let h = h.unwrap_or(0);
                            p.current = Some((f, h));
                            self.open_search_hit(f, h);
                            return;
                        }
                    }
                    _ => {}
                }
            }
            _ if p.field == Field::Roots && key.code != KeyCode::Enter => match key.code {
                KeyCode::Left => p.root_cursor = p.root_cursor.saturating_sub(1),
                KeyCode::Right => {
                    p.root_cursor = (p.root_cursor + 1).min(p.roots.len().saturating_sub(1));
                }
                KeyCode::Char(' ') => {
                    if let Some(r) = p.roots.get_mut(p.root_cursor) {
                        r.on = !r.on;
                    }
                }
                KeyCode::Up => p.field = Field::Exclude,
                KeyCode::Down => p.field = Field::Results,
                _ => {}
            },
            KeyCode::Enter => {
                self.start_search();
                if let Some(p) = self.search.as_mut() {
                    if !p.files.is_empty() || p.running {
                        p.field = Field::Results;
                    }
                }
            }
            KeyCode::Up => p.field = p.field.step(false),
            KeyCode::Down => p.field = p.field.step(true),
            KeyCode::Backspace if ctrl || alt => {
                if let Some(t) = p.field_text() {
                    let keep_len = t
                        .trim_end()
                        .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
                        .map_or(0, |i| i + 1);
                    t.truncate(keep_len);
                }
            }
            KeyCode::Char('u') if ctrl => {
                if let Some(t) = p.field_text() {
                    t.clear();
                }
            }
            KeyCode::Backspace => {
                if let Some(t) = p.field_text() {
                    t.pop();
                }
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                if let Some(t) = p.field_text() {
                    t.push(c);
                }
            }
            _ => {}
        }
        if keep {
            self.popup = Some(Popup::Search);
        }
    }

    /// Paste into the panel's text field.
    pub(super) fn search_paste(&mut self, text: &str) -> bool {
        if !matches!(self.popup, Some(Popup::Search)) {
            return false;
        }
        if let Some(t) = self.search.as_mut().and_then(SearchPanel::field_text) {
            t.push_str(text.lines().next().unwrap_or(""));
        }
        true
    }

    /// Open match `h` of file `f` and select it.
    fn open_search_hit(&mut self, f: usize, h: usize) {
        let Some((path, hit)) = self.search.as_ref().and_then(|p| {
            let file = p.files.get(f)?;
            Some((file.path.clone(), file.hits.get(h)?.clone()))
        }) else {
            return;
        };
        self.open_at(&path, None);
        let height = self.editor_height;
        if let Some(ed) = self
            .active_context_mut()
            .and_then(|c| c.editor.as_mut())
            .filter(|e| e.path() == path)
        {
            ed.select_range((hit.line, hit.start), (hit.line, hit.end));
            if hit.line < ed.scroll() || hit.line >= ed.scroll() + height {
                ed.set_scroll(hit.line.saturating_sub(height / 3));
            }
        }
        if let Some(p) = self.search.as_ref() {
            let file = &p.files[f];
            self.set_status(format!(
                "{}:{} · match {}/{} in this file · F4 / Shift+F4 next / previous",
                file.label,
                hit.line + 1,
                h + 1,
                file.hits.len()
            ));
        }
    }

    /// F4 / Shift+F4: the next / previous search result.
    pub(super) fn search_step(&mut self, forward: bool) {
        let next = self.search.as_ref().and_then(|p| p.next_match(forward));
        match next {
            Some((f, h)) => {
                if let Some(p) = self.search.as_mut() {
                    p.current = Some((f, h));
                    // Keep the list in step for when the panel opens again.
                    let row = p.rows().iter().position(|r| *r == (f, Some(h)));
                    if let Some(row) = row {
                        p.selected = row;
                    }
                }
                self.open_search_hit(f, h);
            }
            None => self.set_status("no search results · Ctrl+Shift+F searches"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::mpsc;

    #[test]
    fn search_text_counts_chars_and_skips_empty_matches() {
        let re = Regex::new("é?x|^").unwrap();
        let hits = search_text(&re, "aéx x\n\nx");
        let spans: Vec<_> = hits.iter().map(|h| (h.line, h.start, h.end)).collect();
        assert_eq!(spans, [(0, 1, 3), (0, 4, 5), (2, 0, 1)]);
        assert_eq!(hits[0].text, "aéx x");
    }

    #[test]
    fn run_walks_roots_with_globs_and_ignores() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src/out")).unwrap();
        fs::write(root.join(".gitignore"), "out/\n").unwrap();
        fs::write(root.join("src/a.c"), "foo\nbar foo\n").unwrap();
        fs::write(root.join("src/a.h"), "foo\n").unwrap();
        fs::write(root.join("src/out/gen.c"), "foo\n").unwrap();
        fs::write(root.join("src/bin.c"), b"foo\0").unwrap();
        let (events, rx) = EventSender::channel();
        let job = |include: &[&str], exclude: &[&str]| SearchJob {
            id: 7,
            re: Regex::new("foo").unwrap(),
            roots: vec![("fw".into(), root.to_path_buf(), root.to_path_buf())],
            include: include.iter().map(|s| (*s).to_owned()).collect(),
            exclude: exclude.iter().map(|s| (*s).to_owned()).collect(),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        let collect = |rx: &mpsc::Receiver<AppEvent>| {
            let mut files = Vec::new();
            for ev in rx.try_iter() {
                match ev {
                    AppEvent::Job(JobEvent::SearchHits { file, .. }) => {
                        files.push((file.label, file.hits.len()));
                    }
                    AppEvent::Job(JobEvent::SearchDone { .. }) => {}
                    other => panic!("{other:?}"),
                }
            }
            files.sort();
            files
        };
        run(&job(&[], &[]), &events);
        assert_eq!(
            collect(&rx),
            [("fw/src/a.c".to_owned(), 2), ("fw/src/a.h".to_owned(), 1)]
        );
        run(&job(&["*.c"], &[]), &events);
        assert_eq!(collect(&rx), [("fw/src/a.c".to_owned(), 2)]);
        run(&job(&[], &["*.h"]), &events);
        assert_eq!(collect(&rx), [("fw/src/a.c".to_owned(), 2)]);
    }
}
