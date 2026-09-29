//! Ctrl+P (quick open: fuzzy or regex over the files of the task folder and
//! its code workspaces) and Ctrl+O (open a path, completing it as you type).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Matcher, Utf32Str};
use regex::RegexBuilder;

use crate::config::Config;

use super::event::{AppEvent, JobEvent};
use super::popup::{Pending, Popup};
use super::{App, Focus};

/// Most files the quick-open index holds.
pub const MAX_INDEX: usize = 300_000;
/// Most rows a search shows.
const MAX_SHOWN: usize = 200;
/// Characters typed before quick open searches.
pub const MIN_QUERY: usize = 2;
/// An index older than this is rebuilt (in the background) on Ctrl+P.
const STALE: Duration = Duration::from_secs(60);

/// The files under the roots, for quick open.
#[derive(Debug, Default)]
pub struct FileIndex {
    /// (label, folder); the label prefixes paths shown from that root.
    pub roots: Vec<(String, PathBuf)>,
    /// (root, path relative to it with `/`).
    pub files: Vec<(usize, String)>,
    /// Stopped at [`MAX_INDEX`].
    pub truncated: bool,
    /// A rebuild is running.
    pub building: bool,
    /// When the files were listed.
    pub built: Option<Instant>,
    cancel: Option<Arc<AtomicBool>>,
}

impl FileIndex {
    /// The path shown for file `i` (`label/rel`, or `rel` for an unlabelled root).
    fn shown(&self, i: usize) -> String {
        let (root, rel) = &self.files[i];
        match self.roots.get(*root) {
            Some((label, _)) if !label.is_empty() => format!("{label}/{rel}"),
            _ => rel.clone(),
        }
    }

    fn full(&self, i: usize) -> PathBuf {
        let (root, rel) = &self.files[i];
        self.roots[*root].1.join(rel)
    }
}

/// List the files under `roots` (`.gitignore`d and hidden ones left out).
pub fn walk(
    roots: &[(String, PathBuf)],
    cap: usize,
    cancel: &AtomicBool,
) -> (Vec<(usize, String)>, bool) {
    let mut files = Vec::new();
    for (i, (_, root)) in roots.iter().enumerate() {
        let walker = ignore::WalkBuilder::new(root)
            .hidden(true)
            .git_ignore(true)
            .git_exclude(true)
            .require_git(false)
            .follow_links(false)
            .build();
        for entry in walker.flatten() {
            if cancel.load(Ordering::Relaxed) {
                return (files, true);
            }
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(root) else {
                continue;
            };
            files.push((i, rel.to_string_lossy().replace('\\', "/")));
            if files.len() >= cap {
                return (files, true);
            }
        }
    }
    (files, false)
}

/// Which finder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinderKind {
    /// Ctrl+P: search the index.
    Files,
    /// Ctrl+O: complete a path.
    Path,
}

/// One row of the finder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Text shown.
    pub label: String,
    /// Chars of `label` that matched (highlighted).
    pub marks: Vec<usize>,
    /// The file or folder.
    pub path: PathBuf,
    /// A folder.
    pub dir: bool,
}

/// The Ctrl+P / Ctrl+O popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finder {
    /// Which finder.
    pub kind: FinderKind,
    /// Typed text.
    pub query: String,
    /// Selected row.
    pub selected: usize,
    /// Rows.
    pub items: Vec<Item>,
    /// A line about the rows (mode, counts, errors).
    pub note: String,
    /// Relative paths start here (Ctrl+O).
    pub base: PathBuf,
}

/// Whether a quick-open query is a regular expression (else fuzzy).
pub fn is_regex(query: &str) -> bool {
    query.contains(['\\', '(', ')', '[', ']', '{', '}', '|', '*', '+', '?'])
}

/// A trailing `:line` on a quick-open query.
fn split_line(query: &str) -> (&str, Option<usize>) {
    match query.rsplit_once(':') {
        Some((q, n)) if !q.is_empty() => match n.parse::<usize>() {
            Ok(n) => (q, Some(n)),
            Err(_) if n.is_empty() => (q, None),
            Err(_) => (query, None),
        },
        _ => (query, None),
    }
}

/// Quick-open rows for `query`: fuzzy (fzf syntax: `'exact`, `^start`,
/// `end$`, `!not`, spaces separate terms) or, with regex characters, a
/// regular expression (case-insensitive unless it has capitals).
pub fn search_index(index: &FileIndex, query: &str) -> (Vec<Item>, String) {
    let (query, _) = split_line(query.trim());
    let total = index.files.len();
    let mut scored: Vec<(u32, usize, Vec<usize>)> = Vec::new();
    let mode;
    if is_regex(query) {
        mode = "regex";
        let re = match RegexBuilder::new(query)
            .case_insensitive(!query.chars().any(char::is_uppercase))
            .build()
        {
            Ok(re) => re,
            Err(e) => {
                let why = e.to_string();
                let why = why.lines().last().unwrap_or("invalid pattern");
                return (Vec::new(), format!("regex: {why}"));
            }
        };
        for i in 0..total {
            let shown = index.shown(i);
            if let Some(m) = re.find(&shown) {
                let start = shown[..m.start()].chars().count();
                let len = shown[m.start()..m.end()].chars().count();
                let short = u32::try_from(shown.len()).unwrap_or(u32::MAX);
                scored.push((u32::MAX - short, i, (start..start + len).collect()));
            }
        }
    } else {
        mode = "fuzzy";
        let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
        let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT.match_paths());
        let mut buf = Vec::new();
        for i in 0..total {
            let shown = index.shown(i);
            if let Some(score) = pattern.score(Utf32Str::new(&shown, &mut buf), &mut matcher) {
                scored.push((score, i, Vec::new()));
            }
        }
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| index.files[a.1].1.len().cmp(&index.files[b.1].1.len()))
        });
        scored.truncate(MAX_SHOWN);
        for (_, i, marks) in &mut scored {
            let shown = index.shown(*i);
            let mut idx = Vec::new();
            pattern.indices(Utf32Str::new(&shown, &mut buf), &mut matcher, &mut idx);
            idx.sort_unstable();
            idx.dedup();
            *marks = idx.into_iter().map(|n| n as usize).collect();
        }
    }
    if mode == "regex" {
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    }
    let found = scored.len();
    scored.truncate(MAX_SHOWN);
    let items = scored
        .into_iter()
        .map(|(_, i, marks)| Item {
            label: index.shown(i),
            marks,
            path: index.full(i),
            dir: false,
        })
        .collect();
    let plus = if index.truncated { "+" } else { "" };
    let building = if index.building {
        " · indexing …"
    } else {
        ""
    };
    (
        items,
        format!("{mode} · {found} of {total}{plus} files{building}"),
    )
}

/// Resolve a typed path: `~` expanded, relative ones under `base`.
pub fn resolve(typed: &str, base: &Path) -> PathBuf {
    let p = Config::expand_tilde(typed);
    if p.is_absolute() {
        p
    } else {
        base.join(p)
    }
}

/// Ctrl+O rows: the entries of the folder typed so far that start with (or
/// else contain) the last part, folders first.
pub fn list_path(typed: &str, base: &Path) -> (Vec<Item>, String) {
    let (dir_part, prefix) = match typed.rfind('/') {
        Some(i) => (&typed[..=i], &typed[i + 1..]),
        None => ("", typed),
    };
    let dir = if dir_part.is_empty() {
        base.to_path_buf()
    } else {
        resolve(dir_part, base)
    };
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) => return (Vec::new(), format!("{}: {e}", dir.display())),
    };
    let lower = prefix.to_lowercase();
    let mut starts = Vec::new();
    let mut contains = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') && !prefix.starts_with('.') {
            continue;
        }
        let is_dir = entry.path().is_dir();
        let low = name.to_lowercase();
        let bucket = if low.starts_with(&lower) {
            &mut starts
        } else if !lower.is_empty() && low.contains(&lower) {
            &mut contains
        } else {
            continue;
        };
        let at = low.find(&lower).unwrap_or(0);
        let at = name[..at.min(name.len())].chars().count();
        let marks = (at..at + prefix.chars().count()).collect();
        let label = if is_dir { format!("{name}/") } else { name };
        bucket.push(Item {
            label,
            marks,
            path: entry.path(),
            dir: is_dir,
        });
    }
    let order = |a: &Item, b: &Item| b.dir.cmp(&a.dir).then_with(|| a.label.cmp(&b.label));
    starts.sort_by(order);
    contains.sort_by(order);
    starts.extend(contains);
    let n = starts.len();
    starts.truncate(MAX_SHOWN * 5);
    (starts, format!("{} · {n} entries", dir.display()))
}

/// `path` for the Ctrl+O field: `~/…` under the home folder, with a
/// trailing `/` for folders.
fn typed_path(path: &Path, dir: bool) -> String {
    let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf());
    let mut s = match home.as_deref().and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    };
    if dir && !s.ends_with('/') {
        s.push('/');
    }
    s
}

impl Finder {
    fn new(kind: FinderKind, query: String, base: PathBuf) -> Self {
        Self {
            kind,
            query,
            selected: 0,
            items: Vec::new(),
            note: String::new(),
            base,
        }
    }

    /// Title of the popup.
    pub fn title(&self) -> &'static str {
        match self.kind {
            FinderKind::Files => "Open file",
            FinderKind::Path => "Open path",
        }
    }

    /// Keys shown under the rows.
    pub fn hint(&self) -> &'static str {
        match self.kind {
            FinderKind::Files => {
                "↑/↓ · Enter open · name:line jumps · regex when it has ( [ * + ? | \\ · Esc"
            }
            FinderKind::Path => {
                "↑/↓ · Tab/Enter take the entry · Ctrl+Enter (Alt+Enter) open what is typed · Esc"
            }
        }
    }
}

impl App {
    /// The folders Ctrl+P searches: the task folder and its code workspaces.
    fn index_roots(&self) -> Vec<(String, PathBuf)> {
        let Some(ctx) = self.active_context() else {
            return Vec::new();
        };
        let mut roots = vec![(String::new(), ctx.tree.root().to_path_buf())];
        if let Some(fixed) = &ctx.fixed_roots {
            roots.extend(fixed.iter().cloned());
            return roots;
        }
        roots.extend(
            ctx.meta
                .workspaces
                .iter()
                .filter_map(|n| self.config.workspace(n))
                .filter(|w| w.path.is_dir())
                .map(|w| (w.name.clone(), w.path.clone())),
        );
        roots
    }

    /// Rebuild the quick-open index in the background when the roots
    /// changed or it is old.
    fn refresh_index(&mut self) {
        let roots = self.index_roots();
        let idx = &mut self.file_index;
        let fresh = idx.roots == roots && idx.built.is_some_and(|t| t.elapsed() < STALE);
        if fresh || (idx.building && idx.roots == roots) {
            return;
        }
        if idx.roots != roots {
            idx.files.clear();
            idx.truncated = false;
            idx.built = None;
        }
        if let Some(c) = idx.cancel.take() {
            c.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        idx.cancel = Some(cancel.clone());
        idx.roots.clone_from(&roots);
        idx.building = true;
        let events = self.events.clone();
        let spawned = std::thread::Builder::new()
            .name("file-index".into())
            .spawn(move || {
                let (files, truncated) = walk(&roots, MAX_INDEX, &cancel);
                if !cancel.load(Ordering::Relaxed) {
                    events.send(AppEvent::Job(JobEvent::FileIndex {
                        roots,
                        files,
                        truncated,
                    }));
                }
            });
        if let Err(e) = spawned {
            self.file_index.building = false;
            self.error(format!("could not index files: {e}"));
        }
    }

    /// The background index is ready.
    pub(super) fn index_built(
        &mut self,
        roots: &[(String, PathBuf)],
        files: Vec<(usize, String)>,
        truncated: bool,
    ) {
        if roots != self.file_index.roots {
            return;
        }
        let idx = &mut self.file_index;
        idx.files = files;
        idx.truncated = truncated;
        idx.building = false;
        idx.built = Some(Instant::now());
        idx.cancel = None;
        if let Some(Popup::Finder(f)) = &mut self.popup {
            if f.kind == FinderKind::Files {
                let mut f = f.clone();
                self.update_finder(&mut f);
                self.popup = Some(Popup::Finder(f));
            }
        }
    }

    /// Ctrl+P.
    pub(super) fn open_quick_open(&mut self) {
        if self.active_context().is_none() {
            return;
        }
        self.refresh_index();
        let mut f = Box::new(Finder::new(
            FinderKind::Files,
            String::new(),
            PathBuf::new(),
        ));
        self.update_finder(&mut f);
        self.popup = Some(Popup::Finder(f));
    }

    /// Ctrl+O: starts in the open file's folder.
    pub(super) fn open_path_prompt(&mut self) {
        let Some(ctx) = self.active_context() else {
            return;
        };
        let base = ctx.tree.root().to_path_buf();
        let start = ctx
            .editor
            .as_ref()
            .and_then(|e| e.path().parent())
            .map_or_else(|| base.clone(), Path::to_path_buf);
        let mut f = Box::new(Finder::new(
            FinderKind::Path,
            typed_path(&start, true),
            base,
        ));
        self.update_finder(&mut f);
        self.popup = Some(Popup::Finder(f));
    }

    /// Recompute the rows after the query changed.
    fn update_finder(&self, f: &mut Finder) {
        let (items, note) = match f.kind {
            FinderKind::Files if f.query.trim().chars().count() < MIN_QUERY => {
                let recent = self.recent_files();
                let n = self.file_index.files.len();
                let note = if self.file_index.building && n == 0 {
                    "recent files · indexing …".to_owned()
                } else {
                    format!("recent files · type {MIN_QUERY}+ characters to search {n} files")
                };
                (recent, note)
            }
            FinderKind::Files => search_index(&self.file_index, &f.query),
            FinderKind::Path => list_path(&f.query, &f.base),
        };
        f.items = items;
        f.note = note;
        f.selected = f.selected.min(f.items.len().saturating_sub(1));
    }

    /// Open files, most recent first (the shown one last).
    fn recent_files(&self) -> Vec<Item> {
        let Some(ctx) = self.active_context() else {
            return Vec::new();
        };
        let mut paths = ctx.open_paths();
        if paths.len() > 1 {
            paths.rotate_left(1);
        }
        paths
            .into_iter()
            .map(|p| {
                let root = ctx.tree.root_of(&p);
                let label = p
                    .strip_prefix(root)
                    .map_or_else(|_| p.display().to_string(), |r| r.display().to_string());
                Item {
                    label,
                    marks: Vec::new(),
                    path: p,
                    dir: false,
                }
            })
            .collect()
    }

    /// Keys of the Ctrl+P / Ctrl+O popup (it was taken out of `self.popup`).
    pub(super) fn handle_finder_key(&mut self, mut f: Box<Finder>, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let mut changed = false;
        match key.code {
            KeyCode::Esc => return,
            KeyCode::Up => f.selected = f.selected.saturating_sub(1),
            KeyCode::Down => {
                f.selected = (f.selected + 1).min(f.items.len().saturating_sub(1));
            }
            KeyCode::PageUp => f.selected = f.selected.saturating_sub(10),
            KeyCode::PageDown => {
                f.selected = (f.selected + 10).min(f.items.len().saturating_sub(1));
            }
            KeyCode::Char('p') if ctrl => {
                f.selected = f.selected.saturating_sub(1);
            }
            KeyCode::Char('n') if ctrl => {
                f.selected = (f.selected + 1).min(f.items.len().saturating_sub(1));
            }
            KeyCode::Enter if f.kind == FinderKind::Path && (ctrl || alt) => {
                self.open_typed_path(&f);
                return;
            }
            KeyCode::Enter if f.kind == FinderKind::Files => {
                let line = split_line(f.query.trim()).1;
                if let Some(item) = f.items.get(f.selected) {
                    let path = item.path.clone();
                    self.open_at(&path, line);
                    return;
                }
            }
            KeyCode::Enter | KeyCode::Tab if f.kind == FinderKind::Path => {
                if let Some(item) = f.items.get(f.selected).cloned() {
                    if !item.dir && key.code == KeyCode::Enter {
                        self.open_at(&item.path, None);
                        return;
                    }
                    let keep = f.query.rfind('/').map_or(0, |i| i + 1);
                    f.query.truncate(keep);
                    f.query.push_str(&item.label);
                    f.selected = 0;
                    changed = true;
                } else if key.code == KeyCode::Enter {
                    self.open_typed_path(&f);
                    return;
                }
            }
            KeyCode::Backspace if ctrl || alt => {
                delete_part(&mut f.query, f.kind);
                changed = true;
            }
            KeyCode::Char('h' | 'w') if ctrl => {
                delete_part(&mut f.query, f.kind);
                changed = true;
            }
            KeyCode::Char('u') if ctrl => {
                f.query.clear();
                changed = true;
            }
            KeyCode::Backspace => {
                f.query.pop();
                changed = true;
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                f.query.push(c);
                changed = true;
            }
            _ => {}
        }
        if changed {
            if f.kind == FinderKind::Files {
                f.selected = 0;
            }
            self.update_finder(&mut f);
        }
        self.popup = Some(Popup::Finder(f));
    }

    /// Paste into the finder's field.
    pub(super) fn finder_paste(&mut self, text: &str) -> bool {
        let Some(Popup::Finder(f)) = &mut self.popup else {
            return false;
        };
        f.query
            .push_str(text.lines().next().unwrap_or("").trim_end());
        let mut f = f.clone();
        self.update_finder(&mut f);
        self.popup = Some(Popup::Finder(f));
        true
    }

    /// Ctrl+Enter in Ctrl+O: open the typed file (offering to create it),
    /// or show a typed folder in the file tree.
    fn open_typed_path(&mut self, f: &Finder) {
        let path = resolve(f.query.trim(), &f.base);
        if path.is_dir() {
            let shown = self.active_context_mut().is_some_and(|ctx| {
                ctx.focus = Focus::Tree;
                ctx.tree.reveal(&path).unwrap_or(false)
            });
            if !shown {
                self.set_status(format!(
                    "{} is a folder outside the file tree",
                    path.display()
                ));
            }
        } else if path.exists() {
            self.open_at(&path, None);
        } else if f.query.trim().is_empty() || f.query.ends_with('/') {
            self.set_status("type a file name");
            self.popup = Some(Popup::Finder(Box::new(f.clone())));
        } else {
            self.popup = Some(Popup::confirm(
                "New file",
                format!("{} does not exist. Create it?", path.display()),
                Pending::CreatePath(path),
            ));
        }
    }

    /// Open a file (Ctrl+P / Ctrl+O), at a line when given.
    pub(super) fn open_at(&mut self, path: &Path, line: Option<usize>) {
        self.request_open_file(path);
        let Some(line) = line else { return };
        let height = self.editor_height;
        if let Some(ed) = self
            .active_context_mut()
            .and_then(|c| c.editor.as_mut())
            .filter(|e| e.path() == path)
        {
            ed.goto(line, 1);
            ed.set_scroll(ed.cursor().0.saturating_sub(height / 3));
        }
    }
}

/// Ctrl+Backspace: the last path part (Ctrl+O) or word (Ctrl+P).
fn delete_part(query: &mut String, kind: FinderKind) {
    let trimmed = match kind {
        FinderKind::Path => query.strip_suffix('/').unwrap_or(query),
        FinderKind::Files => query.trim_end(),
    };
    let cut = match kind {
        FinderKind::Path => trimmed.rfind('/').map_or(0, |i| i + 1),
        FinderKind::Files => trimmed.rfind([' ', '/']).map_or(0, |i| i + 1),
    };
    query.truncate(cut);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn index(files: &[&str]) -> FileIndex {
        FileIndex {
            roots: vec![
                (String::new(), PathBuf::from("/t")),
                ("ws".into(), PathBuf::from("/w")),
            ],
            files: files
                .iter()
                .map(|f| match f.strip_prefix("ws/") {
                    Some(rest) => (1, rest.to_owned()),
                    None => (0, (*f).to_owned()),
                })
                .collect(),
            ..FileIndex::default()
        }
    }

    #[test]
    fn fuzzy_regex_and_line_suffix() {
        let idx = index(&[
            "CONTEXT.md",
            "ws/src/net/socket.c",
            "ws/src/net/socket.h",
            "ws/docs/sockets.md",
            "ws/src/main.c",
        ]);
        let (items, note) = search_index(&idx, "sockc");
        assert!(note.starts_with("fuzzy · 1 of 5"), "{note}");
        assert_eq!(items[0].label, "ws/src/net/socket.c");
        assert_eq!(items[0].path, PathBuf::from("/w/src/net/socket.c"));
        assert_eq!(items[0].marks.len(), 5);
        let (items, _) = search_index(&idx, "socket.c:40");
        assert_eq!(
            items[0].label, "ws/src/net/socket.c",
            "`:line` is not searched"
        );
        let (items, note) = search_index(&idx, r"socket\.[ch]$");
        assert!(note.starts_with("regex · 2 of 5"), "{note}");
        assert_eq!(items[0].marks, (11..19).collect::<Vec<_>>());
        let (items, note) = search_index(&idx, "(");
        assert!(items.is_empty() && note.starts_with("regex:"), "{note}");
        assert_eq!(split_line("a.c:12"), ("a.c", Some(12)));
        assert_eq!(split_line("a.c:"), ("a.c", None));
        assert_eq!(split_line("a:b"), ("a:b", None));
    }

    #[test]
    fn walk_skips_ignored_and_hidden_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src/out")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".gitignore"), "out/\n").unwrap();
        fs::write(root.join("src/a.c"), "").unwrap();
        fs::write(root.join("src/out/gen.c"), "").unwrap();
        fs::write(root.join(".git/config"), "").unwrap();
        let roots = vec![(String::new(), root.to_path_buf())];
        let (files, truncated) = walk(&roots, 100, &AtomicBool::new(false));
        assert_eq!(files, vec![(0, "src/a.c".to_owned())]);
        assert!(!truncated);
        fs::write(root.join("b.c"), "").unwrap();
        let (files, truncated) = walk(&roots, 1, &AtomicBool::new(false));
        assert_eq!(files.len(), 1);
        assert!(truncated);
    }

    #[test]
    fn path_listing_completes_folders_first() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src/net")).unwrap();
        fs::write(root.join("src/netlink.c"), "").unwrap();
        fs::write(root.join("src/main.c"), "").unwrap();
        fs::write(root.join("src/.hidden"), "").unwrap();
        let (items, _) = list_path("src/", root);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["net/", "main.c", "netlink.c"]);
        let (items, _) = list_path("src/NE", root);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].marks, vec![0, 1]);
        let (items, _) = list_path("src/link", root);
        assert_eq!(items[0].label, "netlink.c", "contains, after starts-with");
        let (items, _) = list_path("src/.h", root);
        assert_eq!(items[0].label, ".hidden");
        let abs = format!("{}/src/ma", root.display());
        assert_eq!(list_path(&abs, Path::new("/")).0[0].label, "main.c");
        let (items, note) = list_path("nope/", root);
        assert!(items.is_empty() && note.contains("nope"));
        let mut q = "~/src/net/".to_owned();
        delete_part(&mut q, FinderKind::Path);
        assert_eq!(q, "~/src/");
        let mut q = "foo bar".to_owned();
        delete_part(&mut q, FinderKind::Files);
        assert_eq!(q, "foo ");
    }
}
