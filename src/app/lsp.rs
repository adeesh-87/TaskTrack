//! Code intelligence for the editor: language servers (clangd by default)
//! and a ctags index where no server runs.
//!
//! Servers start on their own for open files whose language has one
//! (`[lsp]` in the config), one per (command, project root). Open files are
//! kept in sync on every tick; diagnostics mark the gutter. F12 definition,
//! Shift+F12 references, Ctrl+K hover, Ctrl+Space completion, Ctrl+T
//! symbols, Ctrl+Shift+O the file's outline, Alt+← / Alt+→ jump back /
//! forward, F8 / Shift+F8 next / previous problem. Without a server (or
//! when it finds nothing) definitions and symbols come from ctags.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::lsp::ctags::{self, Tag};
use crate::lsp::{self as proto, Diagnostic, Encoding, Incoming, Server, ServerEvent};

use super::complete::{self, Completion};
use super::event::{AppEvent, JobEvent};
use super::finder::{filter_items, Finder, FinderKind, Item, MIN_QUERY};
use super::popup::Popup;
use super::App;

/// Most places kept to jump back to.
const MAX_JUMPS: usize = 100;
/// A ctags index older than this is rebuilt when used.
const TAGS_STALE: Duration = Duration::from_secs(10 * 60);
/// Most symbols listed.
const MAX_SYMBOLS: usize = 300;

/// A place: file, line, char column.
type Spot = (PathBuf, (usize, usize));

/// What an answer is for.
#[derive(Debug, Clone)]
enum Ask {
    Definition {
        word: String,
        path: PathBuf,
    },
    References {
        word: String,
    },
    Hover {
        word: String,
    },
    Completion {
        path: PathBuf,
        revision: u64,
        cursor: (usize, usize),
        start: (usize, usize),
    },
    Symbols {
        generation: u64,
    },
    Outline {
        path: PathBuf,
    },
}

/// An open file the server knows.
#[derive(Debug, Clone, Copy)]
struct Doc {
    server: usize,
    version: i64,
    revision: u64,
}

/// Language servers, their documents and answers; the ctags index.
#[derive(Debug, Default)]
pub struct LspState {
    servers: Vec<Option<Server>>,
    by_key: HashMap<(Vec<String>, PathBuf), usize>,
    /// Commands that could not start or died (not retried): why.
    failed: HashMap<Vec<String>, String>,
    docs: HashMap<PathBuf, Doc>,
    diagnostics: HashMap<PathBuf, Vec<Diagnostic>>,
    pending: HashMap<(usize, u64), Ask>,
    /// Work a server reports (e.g. clangd's background indexing).
    progress: HashMap<usize, String>,
    back: Vec<Spot>,
    forward: Vec<Spot>,
    symbol_generation: u64,
    tags: TagIndex,
}

/// The ctags index over the task's (or `pahiri edit`'s) folders.
#[derive(Debug, Default)]
struct TagIndex {
    roots: Vec<(String, PathBuf)>,
    tags: Vec<Tag>,
    building: bool,
    built: Option<Instant>,
    error: Option<String>,
}

/// The editor's view of the open file's problems.
#[derive(Debug, Default, Clone)]
pub struct DiagnosticView {
    /// Worst severity per line (1 error … 4 hint).
    pub lines: HashMap<usize, u8>,
    /// Char ranges to underline: (line, start, end, severity).
    pub ranges: Vec<(usize, usize, usize, u8)>,
    /// The problem at the cursor's line.
    pub at_cursor: Option<(u8, String)>,
    /// Errors and warnings in the file.
    pub counts: (usize, usize),
}

/// The word (letters, digits, `_`) at or before the char column.
pub fn word_at(line: &str, col: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut start = col.min(chars.len());
    if start == chars.len() || !is_word(chars[start]) {
        if start > 0 && is_word(chars[start - 1]) {
            start -= 1;
        } else {
            return String::new();
        }
    }
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let end = chars[start..]
        .iter()
        .position(|c| !is_word(*c))
        .map_or(chars.len(), |i| start + i);
    chars[start..end].iter().collect()
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

impl App {
    // ----- servers and documents --------------------------------------------

    /// The server for `path` (started when needed), if its language has one.
    fn server_for(&mut self, path: &Path) -> Option<usize> {
        let lang = proto::language_id(path)?;
        let cmd = self.config.lsp.server_for(lang)?;
        if self.lsp.failed.contains_key(&cmd) {
            return None;
        }
        let fallback = self.active_context()?.tree.root_of(path).to_path_buf();
        let root = proto::find_root(path, &fallback);
        let key = (cmd.clone(), root.clone());
        if let Some(&id) = self.lsp.by_key.get(&key) {
            return self.lsp.servers[id].as_ref().map(|_| id);
        }
        let id = self.lsp.servers.len();
        let events = self.events.clone();
        let report = move |event| {
            events.send(AppEvent::Job(JobEvent::Lsp { server: id, event }));
        };
        match Server::start(&cmd, &root, report) {
            Ok(server) => {
                tracing::info!("started {} in {}", cmd.join(" "), root.display());
                self.lsp.servers.push(Some(server));
                self.lsp.by_key.insert(key, id);
                self.lsp.progress.insert(id, "starting".into());
                Some(id)
            }
            Err(e) => {
                let why = format!("{}: {e}", cmd[0]);
                self.set_status(format!("{why} · definitions come from ctags"));
                self.lsp.failed.insert(cmd, why);
                None
            }
        }
    }

    /// Tell servers about opened, changed and closed files.
    pub(super) fn lsp_sync(&mut self) {
        if !self.config.lsp.enabled {
            return;
        }
        let Some(ctx) = self.active_context() else {
            return;
        };
        let open: Vec<(PathBuf, u64)> = ctx
            .editor
            .iter()
            .chain(&ctx.recent)
            .filter(|b| !b.is_read_only())
            .map(|b| (b.path().to_path_buf(), b.revision()))
            .collect();
        let open_set: HashSet<&PathBuf> = open.iter().map(|(p, _)| p).collect();
        let closed: Vec<PathBuf> = self
            .lsp
            .docs
            .keys()
            .filter(|p| !open_set.contains(p))
            .cloned()
            .collect();
        for path in closed {
            if let Some(doc) = self.lsp.docs.remove(&path) {
                if let Some(s) = self
                    .lsp
                    .servers
                    .get_mut(doc.server)
                    .and_then(Option::as_mut)
                {
                    s.notify(
                        "textDocument/didClose",
                        &json!({"textDocument": {"uri": proto::path_to_uri(&path)}}),
                    );
                }
            }
            self.lsp.diagnostics.remove(&path);
        }
        for (path, revision) in open {
            let known = self.lsp.docs.get(&path).copied();
            if known.is_some_and(|d| d.revision == revision) {
                continue;
            }
            let Some(server) = known.map(|d| d.server).or_else(|| self.server_for(&path)) else {
                continue;
            };
            let Some(text) = self.open_text(&path) else {
                continue;
            };
            let Some(s) = self.lsp.servers.get_mut(server).and_then(Option::as_mut) else {
                continue;
            };
            let uri = proto::path_to_uri(&path);
            let version = if let Some(d) = known {
                s.notify(
                    "textDocument/didChange",
                    &json!({"textDocument": {"uri": uri, "version": d.version + 1},
                            "contentChanges": [{"text": text}]}),
                );
                d.version + 1
            } else {
                let lang = proto::language_id(&path).unwrap_or("plaintext");
                s.notify(
                    "textDocument/didOpen",
                    &json!({"textDocument": {"uri": uri, "languageId": lang,
                            "version": 1, "text": text}}),
                );
                1
            };
            self.lsp.docs.insert(
                path,
                Doc {
                    server,
                    version,
                    revision,
                },
            );
        }
    }

    /// The text of an open file.
    fn open_text(&self, path: &Path) -> Option<String> {
        let ctx = self.active_context()?;
        ctx.editor
            .iter()
            .chain(&ctx.recent)
            .find(|b| b.path() == path)
            .map(crate::editor::Buffer::text)
    }

    /// Line `line` of `path`: from the open buffer, else from disk (cached in `cache`).
    fn line_of(
        &self,
        path: &Path,
        line: usize,
        cache: &mut HashMap<PathBuf, Vec<String>>,
    ) -> String {
        if let Some(ctx) = self.active_context() {
            if let Some(b) = ctx
                .editor
                .iter()
                .chain(&ctx.recent)
                .find(|b| b.path() == path)
            {
                return b.lines().get(line).cloned().unwrap_or_default();
            }
        }
        let lines = cache.entry(path.to_path_buf()).or_insert_with(|| {
            std::fs::read_to_string(path)
                .map(|t| t.lines().map(str::to_owned).collect())
                .unwrap_or_default()
        });
        lines.get(line).cloned().unwrap_or_default()
    }

    fn encoding_of(&self, server: usize) -> Encoding {
        self.lsp
            .servers
            .get(server)
            .and_then(Option::as_ref)
            .map_or(Encoding::Utf16, |s| s.encoding)
    }

    /// A server's message or exit.
    pub(super) fn lsp_event(&mut self, server: usize, event: ServerEvent) {
        match event {
            ServerEvent::Exited(tail) => self.lsp_exited(server, &tail),
            ServerEvent::Message(msg) => match proto::classify(msg) {
                Some(Incoming::Response { id, result }) => {
                    let starting = self
                        .lsp
                        .servers
                        .get(server)
                        .and_then(Option::as_ref)
                        .is_some_and(|s| !s.ready);
                    if starting && id == 1 {
                        match result {
                            Ok(r) => {
                                if let Some(s) = self.lsp.servers[server].as_mut() {
                                    s.initialized(&r);
                                }
                                self.lsp.progress.remove(&server);
                            }
                            Err(e) => self.lsp_exited(server, &e),
                        }
                    } else if let Some(ask) = self.lsp.pending.remove(&(server, id)) {
                        match result {
                            Ok(v) => self.lsp_answer(server, ask, &v),
                            Err(e) => self.set_status(format!("language server: {e}")),
                        }
                    }
                }
                Some(Incoming::Notification { method, params }) => {
                    self.lsp_notification(server, &method, &params);
                }
                Some(Incoming::Request { id, method, params }) => {
                    // Configuration gets one (empty) answer per item asked.
                    let result = if method == "workspace/configuration" {
                        let n = params
                            .get("items")
                            .and_then(Value::as_array)
                            .map_or(0, Vec::len);
                        Value::Array(vec![Value::Null; n])
                    } else {
                        Value::Null
                    };
                    if let Some(s) = self.lsp.servers.get_mut(server).and_then(Option::as_mut) {
                        s.reply(&id, &result);
                    }
                }
                None => {}
            },
        }
    }

    fn lsp_exited(&mut self, server: usize, why: &str) {
        let Some(s) = self.lsp.servers.get_mut(server).and_then(Option::take) else {
            return;
        };
        let name = s.command[0].clone();
        let why = if why.is_empty() {
            format!("{name} stopped")
        } else {
            format!("{name} stopped: {why}")
        };
        tracing::warn!("{why}");
        self.set_status(format!("{why} · ctags is used instead"));
        self.lsp.failed.insert(s.command.clone(), why);
        self.lsp.docs.retain(|_, d| d.server != server);
        self.lsp.pending.retain(|(id, _), _| *id != server);
        self.lsp.progress.remove(&server);
        let docs: HashSet<PathBuf> = self.lsp.docs.keys().cloned().collect();
        self.lsp.diagnostics.retain(|p, _| docs.contains(p));
        s.stop();
    }

    fn lsp_notification(&mut self, server: usize, method: &str, params: &Value) {
        match method {
            "textDocument/publishDiagnostics" => {
                if let Some((path, list)) = proto::parse_diagnostics(params) {
                    self.lsp.diagnostics.insert(path, list);
                }
            }
            "$/progress" => {
                let value = params.get("value").unwrap_or(&Value::Null);
                match value.get("kind").and_then(Value::as_str) {
                    Some("end") => {
                        self.lsp.progress.remove(&server);
                    }
                    Some(_) => {
                        let title = value.get("title").and_then(Value::as_str);
                        let pct = value.get("percentage").and_then(Value::as_u64);
                        let old = self.lsp.progress.get(&server).cloned().unwrap_or_default();
                        let what = title.map_or_else(
                            || old.split(' ').next().unwrap_or("working").to_owned(),
                            str::to_lowercase,
                        );
                        let text = match pct {
                            Some(p) => format!("{what} {p}%"),
                            None => what,
                        };
                        self.lsp.progress.insert(server, text);
                    }
                    None => {}
                }
            }
            "window/showMessage" => {
                let kind = params.get("type").and_then(Value::as_u64).unwrap_or(4);
                if kind <= 2 {
                    if let Some(m) = params.get("message").and_then(Value::as_str) {
                        self.set_status(m.lines().next().unwrap_or(m).to_owned());
                    }
                }
            }
            _ => {}
        }
    }

    /// The server, position parameters, path and word at the cursor, when
    /// the open file has a ready server that can do `capability`.
    fn lsp_here(&mut self, capability: &str) -> Option<(usize, Value, PathBuf)> {
        self.lsp_sync();
        let ctx = self.active_context()?;
        let ed = ctx.editor.as_ref()?;
        let path = ed.path().to_path_buf();
        let (row, col) = ed.cursor();
        let line = ed.lines().get(row).cloned().unwrap_or_default();
        let server = self.lsp.docs.get(&path)?.server;
        let s = self.lsp.servers.get(server)?.as_ref()?;
        if !s.ready {
            self.set_status(format!("{} is starting …", s.command[0]));
            return None;
        }
        if !s.can(capability) {
            return None;
        }
        let character = proto::to_lsp_col(&line, col, s.encoding);
        let params = json!({
            "textDocument": {"uri": proto::path_to_uri(&path)},
            "position": {"line": row, "character": character}
        });
        Some((server, params, path))
    }

    fn ask(&mut self, server: usize, method: &str, params: &Value, ask: Ask) {
        if let Some(s) = self.lsp.servers.get_mut(server).and_then(Option::as_mut) {
            let id = s.request(method, params);
            self.lsp.pending.insert((server, id), ask);
        }
    }

    /// The word at the editor's cursor and the file.
    fn cursor_word(&self) -> Option<(String, PathBuf)> {
        let ed = self.active_context()?.editor.as_ref()?;
        let (row, col) = ed.cursor();
        let word = word_at(ed.lines().get(row).map_or("", String::as_str), col);
        Some((word, ed.path().to_path_buf()))
    }

    // ----- requests ------------------------------------------------------------

    /// F12: go to the definition of the symbol at the cursor.
    pub(super) fn goto_definition(&mut self) {
        let Some((word, path)) = self.cursor_word() else {
            return;
        };
        if let Some((server, params, _)) = self.lsp_here("definitionProvider") {
            self.set_status(format!("looking up {word} …"));
            self.ask(
                server,
                "textDocument/definition",
                &params,
                Ask::Definition { word, path },
            );
            return;
        }
        self.tags_definition(&word, &path);
    }

    /// Shift+F12: every reference to the symbol at the cursor.
    pub(super) fn find_references(&mut self) {
        let Some((word, path)) = self.cursor_word() else {
            return;
        };
        if let Some((server, mut params, _)) = self.lsp_here("referencesProvider") {
            params["context"] = json!({"includeDeclaration": true});
            self.set_status(format!("finding references to {word} …"));
            self.ask(
                server,
                "textDocument/references",
                &params,
                Ask::References { word },
            );
            return;
        }
        // Without a server: every tag of that name, then a text search.
        let found = self.tags_named(&word, &path);
        if found.is_empty() {
            self.set_status(format!(
                "no language server for this file: Ctrl+Shift+F searches for {word}"
            ));
        } else {
            self.show_locations(format!("{word} (ctags)"), found);
        }
    }

    /// Ctrl+K: what the server knows about the symbol at the cursor.
    pub(super) fn hover(&mut self) {
        let Some((word, _)) = self.cursor_word() else {
            return;
        };
        match self.lsp_here("hoverProvider") {
            Some((server, params, _)) => {
                self.ask(server, "textDocument/hover", &params, Ask::Hover { word });
            }
            None => self.set_status("no language server for this file"),
        }
    }

    /// Ctrl+Space with a server: ask it. Returns whether it was asked.
    pub(super) fn lsp_complete(&mut self) -> bool {
        let Some((server, params, path)) = self.lsp_here("completionProvider") else {
            return false;
        };
        let Some(ed) = self.active_context().and_then(|c| c.editor.as_ref()) else {
            return false;
        };
        let cursor = ed.cursor();
        let (start, _) = complete::prefix(ed);
        let ask = Ask::Completion {
            path,
            revision: ed.revision(),
            cursor,
            start: (cursor.0, start),
        };
        self.ask(server, "textDocument/completion", &params, ask);
        true
    }

    /// Ctrl+T: search symbols of the workspace.
    pub(super) fn open_symbols(&mut self) {
        if self.active_context().is_none() {
            return;
        }
        let seed = self
            .cursor_word()
            .map(|(w, _)| w)
            .filter(|w| w.chars().count() >= MIN_QUERY)
            .unwrap_or_default();
        let mut f = Box::new(Finder::new_kind(FinderKind::Symbols, seed));
        self.update_finder(&mut f);
        self.popup = Some(Popup::Finder(f));
    }

    /// Rows for the Ctrl+T finder (asks the servers; their answers update it).
    pub(super) fn symbol_rows(&mut self, f: &Finder) -> (Vec<Item>, String) {
        let query = f.query.trim().to_owned();
        if query.chars().count() < MIN_QUERY {
            return (
                Vec::new(),
                format!("type {MIN_QUERY}+ characters of a symbol name"),
            );
        }
        self.lsp_sync();
        let servers: Vec<usize> = self
            .lsp
            .servers
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.as_ref()
                    .is_some_and(|s| s.ready && s.can("workspaceSymbolProvider"))
            })
            .map(|(i, _)| i)
            .collect();
        if servers.is_empty() {
            return self.tags_symbols(&query);
        }
        self.lsp.symbol_generation += 1;
        let generation = self.lsp.symbol_generation;
        for s in servers {
            self.ask(
                s,
                "workspace/symbol",
                &json!({"query": query}),
                Ask::Symbols { generation },
            );
        }
        (f.items.clone(), "asking the language server …".into())
    }

    /// Ctrl+Shift+O: the symbols of the open file.
    pub(super) fn open_outline(&mut self) {
        let Some(path) = self
            .active_context()
            .and_then(|c| c.editor.as_ref())
            .map(|e| e.path().to_path_buf())
        else {
            self.set_status("open a file first");
            return;
        };
        if let Some((server, params, _)) = self.lsp_here("documentSymbolProvider") {
            let params = json!({"textDocument": params["textDocument"].clone()});
            self.ask(
                server,
                "textDocument/documentSymbol",
                &params,
                Ask::Outline { path },
            );
            return;
        }
        let items = self.tags_outline(&path);
        self.show_outline(items, "ctags");
    }

    fn show_outline(&mut self, items: Vec<Item>, source: &str) {
        if items.is_empty() {
            self.set_status(format!("no symbols in this file ({source})"));
            return;
        }
        let mut f = Box::new(Finder::new_kind(FinderKind::Outline, String::new()));
        f.all = items;
        self.update_finder(&mut f);
        self.popup = Some(Popup::Finder(f));
    }

    // ----- answers -------------------------------------------------------------

    fn lsp_answer(&mut self, server: usize, ask: Ask, v: &Value) {
        let enc = self.encoding_of(server);
        match ask {
            Ask::Definition { word, path } => {
                let locs = self.char_locations(&proto::parse_locations(v), enc);
                if locs.is_empty() {
                    self.tags_definition(&word, &path);
                } else {
                    self.status = None;
                    self.show_locations(format!("definitions of {word}"), locs);
                }
            }
            Ask::References { word } => {
                let locs = self.char_locations(&proto::parse_locations(v), enc);
                if locs.is_empty() {
                    self.set_status(format!("no references to {word}"));
                } else {
                    self.status = None;
                    self.show_locations(format!("{} references to {word}", locs.len()), locs);
                }
            }
            Ask::Hover { word } => {
                let text = proto::hover_text(v);
                if text.is_empty() {
                    self.set_status(format!("nothing known about {word}"));
                } else {
                    self.popup = Some(Popup::Doc {
                        title: word,
                        lines: text.lines().map(str::to_owned).collect(),
                        scroll: 0,
                    });
                }
            }
            Ask::Completion {
                path,
                revision,
                cursor,
                start,
            } => {
                let still = self
                    .active_context()
                    .and_then(|c| c.editor.as_ref())
                    .is_some_and(|e| {
                        e.path() == path && e.revision() == revision && e.cursor() == cursor
                    });
                if !still {
                    return;
                }
                let prefix = self
                    .active_context()
                    .and_then(|c| c.editor.as_ref())
                    .map(|e| complete::prefix(e).1)
                    .unwrap_or_default();
                let c = Completion::from_server(start, proto::parse_completions(v), &prefix);
                match c.items.len() {
                    0 => self.word_completion(),
                    1 => {
                        self.completion = Some(c);
                        self.accept_completion();
                    }
                    _ => self.completion = Some(c),
                }
            }
            Ask::Symbols { generation } => {
                if generation != self.lsp.symbol_generation {
                    return;
                }
                let items: Vec<Item> = proto::parse_symbols(v, None)
                    .into_iter()
                    .take(MAX_SYMBOLS)
                    .map(|s| {
                        let detail = format!(
                            "{} · {}:{}",
                            s.kind,
                            self.short_path(&s.location.path),
                            s.location.line + 1
                        );
                        let col = s.location.col as usize;
                        Item::at(s.name, s.location.path, (s.location.line, col), detail)
                    })
                    .collect();
                if let Some(Popup::Finder(f)) = &mut self.popup {
                    if f.kind == FinderKind::Symbols {
                        f.note = format!("{} symbols (language server)", items.len());
                        f.items = items;
                        f.selected = 0;
                    }
                }
            }
            Ask::Outline { path } => {
                let mut cache = HashMap::new();
                let items: Vec<Item> = proto::parse_symbols(v, Some(&path))
                    .into_iter()
                    .map(|s| {
                        let line = self.line_of(&path, s.location.line, &mut cache);
                        let col = proto::from_lsp_col(&line, s.location.col, enc);
                        let detail = format!("{} · line {}", s.kind, s.location.line + 1);
                        Item::at(s.name, s.location.path, (s.location.line, col), detail)
                    })
                    .collect();
                self.show_outline(items, "language server");
            }
        }
    }

    /// Server locations with char columns.
    fn char_locations(&self, locs: &[proto::Location], enc: Encoding) -> Vec<Spot> {
        let mut cache = HashMap::new();
        locs.iter()
            .map(|l| {
                let line = self.line_of(&l.path, l.line, &mut cache);
                (
                    l.path.clone(),
                    (l.line, proto::from_lsp_col(&line, l.col, enc)),
                )
            })
            .collect()
    }

    /// Where a path is shown: relative to its root, with the root's name
    /// for attached folders.
    fn short_path(&self, path: &Path) -> String {
        let Some(ctx) = self.active_context() else {
            return path.display().to_string();
        };
        let root = ctx.tree.root_of(path);
        let Ok(rel) = path.strip_prefix(root) else {
            return path.display().to_string();
        };
        if root == ctx.tree.root() {
            return rel.display().to_string();
        }
        let label = ctx
            .tree
            .roots()
            .into_iter()
            .find(|(_, p)| p == root)
            .map(|(l, _)| l.split(" · ").next().unwrap_or_default().to_owned())
            .unwrap_or_default();
        format!("{label}/{}", rel.display())
    }

    /// Jump to the only place, or list them.
    fn show_locations(&mut self, heading: String, mut locs: Vec<Spot>) {
        locs.dedup();
        if let [(path, pos)] = locs.as_slice() {
            let (path, pos) = (path.clone(), *pos);
            self.jump_to(&path, pos);
            return;
        }
        let mut cache = HashMap::new();
        let items: Vec<Item> = locs
            .into_iter()
            .map(|(path, (line, col))| {
                let text = self.line_of(&path, line, &mut cache);
                let label = format!("{}:{}", self.short_path(&path), line + 1);
                Item::at(
                    label,
                    path,
                    (line, col),
                    text.trim().chars().take(100).collect(),
                )
            })
            .collect();
        let mut f = Box::new(Finder::new_kind(FinderKind::Locations, String::new()));
        f.heading = heading;
        f.all = items;
        self.update_finder(&mut f);
        self.popup = Some(Popup::Finder(f));
    }

    // ----- jumps -----------------------------------------------------------------

    fn here(&self) -> Option<Spot> {
        let ed = self.active_context()?.editor.as_ref()?;
        Some((ed.path().to_path_buf(), ed.cursor()))
    }

    /// Open `path` at (line, char column), remembering where we were.
    pub(super) fn jump_to(&mut self, path: &Path, pos: (usize, usize)) {
        if let Some(h) = self.here() {
            if h != (path.to_path_buf(), pos) {
                self.lsp.back.push(h);
                if self.lsp.back.len() > MAX_JUMPS {
                    self.lsp.back.remove(0);
                }
                self.lsp.forward.clear();
            }
        }
        self.go(path, pos);
    }

    /// Go to a finder row; a ctags row (column 0) lands on its name.
    pub(super) fn jump_to_item(&mut self, item: &Item) {
        let (line, mut col) = item.pos.unwrap_or((0, 0));
        if col == 0 {
            let name = item.label.rsplit("::").next().unwrap_or(&item.label);
            let text = self.line_of(&item.path, line, &mut HashMap::new());
            if let Some(b) = text.find(name) {
                col = text[..b].chars().count();
            }
        }
        self.jump_to(&item.path, (line, col));
    }

    fn go(&mut self, path: &Path, (line, col): (usize, usize)) {
        self.open_at(path, None);
        let height = self.editor_height;
        if let Some(ed) = self
            .active_context_mut()
            .and_then(|c| c.editor.as_mut())
            .filter(|e| e.path() == path)
        {
            ed.goto(line + 1, col + 1);
            let row = ed.cursor().0;
            if row < ed.scroll() || row >= ed.scroll() + height {
                ed.set_scroll(row.saturating_sub(height / 3));
            }
        }
    }

    /// Alt+← / Alt+→: back to where a jump started / forward again.
    pub(super) fn jump_back(&mut self, forward: bool) {
        let target = if forward {
            self.lsp.forward.pop()
        } else {
            self.lsp.back.pop()
        };
        let Some((path, pos)) = target else {
            self.set_status(if forward {
                "nothing to go forward to"
            } else {
                "nothing to go back to (F12 and Ctrl+T remember where you were)"
            });
            return;
        };
        if let Some(h) = self.here() {
            if forward {
                self.lsp.back.push(h);
            } else {
                self.lsp.forward.push(h);
            }
        }
        self.go(&path, pos);
    }

    // ----- diagnostics -------------------------------------------------------------

    /// Problems of the open file as the editor draws them.
    pub fn diagnostic_view(&self) -> DiagnosticView {
        let mut view = DiagnosticView::default();
        let Some(ed) = self.active_context().and_then(|c| c.editor.as_ref()) else {
            return view;
        };
        let Some(list) = self.lsp.diagnostics.get(ed.path()) else {
            return view;
        };
        let enc = self
            .lsp
            .docs
            .get(ed.path())
            .map_or(Encoding::Utf16, |d| self.encoding_of(d.server));
        let cursor_row = ed.cursor().0;
        for d in list {
            let worst = view.lines.entry(d.line).or_insert(d.severity);
            *worst = (*worst).min(d.severity);
            match d.severity {
                1 => view.counts.0 += 1,
                2 => view.counts.1 += 1,
                _ => {}
            }
            for line in d.line..=d.end_line.max(d.line) {
                let text = ed.lines().get(line).map_or("", String::as_str);
                let start = if line == d.line {
                    proto::from_lsp_col(text, d.col, enc)
                } else {
                    0
                };
                let mut end = if line == d.end_line {
                    proto::from_lsp_col(text, d.end_col, enc)
                } else {
                    text.chars().count()
                };
                if end <= start {
                    end = start + 1;
                }
                view.ranges.push((line, start, end, d.severity));
            }
            if (d.line..=d.end_line.max(d.line)).contains(&cursor_row)
                && view
                    .at_cursor
                    .as_ref()
                    .map_or(true, |(s, _)| d.severity < *s)
            {
                view.at_cursor = Some((d.severity, d.message.clone()));
            }
        }
        view
    }

    /// F8 / Shift+F8: the next / previous problem in the open file.
    pub(super) fn next_diagnostic(&mut self, forward: bool) {
        let view = self.diagnostic_view();
        let Some(here) = self.here() else { return };
        let mut spots: Vec<(usize, usize)> = view.ranges.iter().map(|r| (r.0, r.1)).collect();
        spots.sort_unstable();
        spots.dedup();
        let next = if forward {
            spots
                .iter()
                .find(|s| **s > here.1)
                .or_else(|| spots.first())
        } else {
            spots
                .iter()
                .rev()
                .find(|s| **s < here.1)
                .or_else(|| spots.last())
        };
        match next.copied() {
            Some(pos) => {
                self.go(&here.0, pos);
                if let Some((_, msg)) = self.diagnostic_view().at_cursor {
                    self.set_status(msg);
                }
            }
            None => self.set_status("no problems in this file"),
        }
    }

    /// What the editor's title says about the file's server, if any.
    pub fn lsp_label(&self, path: &Path) -> Option<String> {
        let lang = proto::language_id(path)?;
        let cmd = self.config.lsp.server_for(lang)?;
        let name = Path::new(&cmd[0])
            .file_name()
            .map_or_else(|| cmd[0].clone(), |n| n.to_string_lossy().into_owned());
        if self.lsp.failed.contains_key(&cmd) {
            return Some(format!("{name} ✗ · ctags"));
        }
        let Some(doc) = self.lsp.docs.get(path) else {
            return Some(name);
        };
        Some(match self.lsp.progress.get(&doc.server) {
            Some(p) => format!("{name} · {p}"),
            None => name,
        })
    }

    /// Whether the ctags index is built (tests).
    #[cfg(test)]
    pub(super) fn lsp_tags_ready(&self) -> bool {
        self.lsp.tags.built.is_some() && !self.lsp.tags.building
    }

    /// Whether the open file's server has answered `initialize` (tests).
    #[cfg(test)]
    pub(super) fn lsp_ready_for_active(&self) -> bool {
        self.active_context()
            .and_then(|c| c.editor.as_ref())
            .and_then(|e| self.lsp.docs.get(e.path()))
            .and_then(|d| self.lsp.servers.get(d.server))
            .and_then(Option::as_ref)
            .is_some_and(|s| s.ready)
    }

    /// Stop every server (on quit).
    pub(super) fn lsp_shutdown(&mut self) {
        for s in self.lsp.servers.iter_mut().filter_map(Option::take) {
            s.stop();
        }
    }

    // ----- ctags -------------------------------------------------------------------

    /// Start (re)building the ctags index when it is missing, old or for
    /// other folders. Returns whether one is usable now.
    fn tags_ready(&mut self) -> bool {
        let cmd = shell_words::split(&self.config.lsp.ctags).unwrap_or_default();
        if cmd.is_empty() {
            return false;
        }
        let roots = self.index_roots();
        let t = &mut self.lsp.tags;
        let same = t.roots == roots;
        let fresh = same && t.built.is_some_and(|b| b.elapsed() < TAGS_STALE);
        if !fresh && !t.building {
            t.building = true;
            if !same {
                t.tags.clear();
                t.built = None;
            }
            t.roots.clone_from(&roots);
            let state = self.state_dir.join("tags");
            let events = self.events.clone();
            let spawned = std::thread::Builder::new()
                .name("ctags".into())
                .spawn(move || {
                    let (tags, error) = build_tags(&cmd, &roots, &state);
                    events.send(AppEvent::Job(JobEvent::Tags { roots, tags, error }));
                });
            if spawned.is_err() {
                self.lsp.tags.building = false;
            }
        }
        same && self.lsp.tags.built.is_some()
    }

    /// The ctags index is ready.
    pub(super) fn tags_built(
        &mut self,
        roots: &[(String, PathBuf)],
        tags: Vec<Tag>,
        error: Option<String>,
    ) {
        let t = &mut self.lsp.tags;
        if roots != t.roots {
            return;
        }
        t.building = false;
        t.built = Some(Instant::now());
        t.tags = tags;
        if let Some(e) = &error {
            self.set_status(format!("ctags: {e}"));
        }
        self.lsp.tags.error = error;
        // A symbol search waiting for the index.
        if let Some(Popup::Finder(f)) = &self.popup {
            if f.kind == FinderKind::Symbols {
                let mut f = f.clone();
                self.update_finder(&mut f);
                self.popup = Some(Popup::Finder(f));
            }
        }
    }

    fn tags_status(&self) -> String {
        match &self.lsp.tags.error {
            Some(e) => format!("ctags: {e}"),
            None if self.config.lsp.ctags.trim().is_empty() => {
                "no language server for this file and ctags is off".into()
            }
            None => "ctags is indexing the folders … try again in a moment".into(),
        }
    }

    /// Tags named `name` as places (definitions first).
    fn tags_named(&mut self, name: &str, near: &Path) -> Vec<Spot> {
        if name.is_empty() || !self.tags_ready() {
            return Vec::new();
        }
        ctags::lookup(&self.lsp.tags.tags, name, near)
            .into_iter()
            .map(|t| (t.path.clone(), (t.line, 0)))
            .collect()
    }

    /// F12 without a server: the ctags definitions of `word`.
    fn tags_definition(&mut self, word: &str, path: &Path) {
        if word.is_empty() {
            self.set_status("no symbol at the cursor");
            return;
        }
        if !self.tags_ready() {
            let msg = self.tags_status();
            self.set_status(msg);
            return;
        }
        let found = ctags::lookup(&self.lsp.tags.tags, word, path);
        // Definitions only, when there are any.
        let best = found.first().map(|t| t.kind.clone());
        let spots: Vec<Spot> = found
            .iter()
            .filter(|t| {
                best.as_deref().map_or(true, |b| {
                    !matches!(b, "prototype" | "externvar") || t.kind == b
                })
            })
            .map(|t| (t.path.clone(), (t.line, 0)))
            .collect();
        if spots.is_empty() {
            self.set_status(format!("no definition of {word} found (ctags)"));
            return;
        }
        let spots = self.column_of(word, spots);
        self.show_locations(format!("definitions of {word} (ctags)"), spots);
    }

    /// Put the cursor on `word` in each line (ctags gives lines only).
    fn column_of(&self, word: &str, spots: Vec<Spot>) -> Vec<Spot> {
        let mut cache = HashMap::new();
        spots
            .into_iter()
            .map(|(p, (line, _))| {
                let text = self.line_of(&p, line, &mut cache);
                let col = text.find(word).map_or(0, |b| text[..b].chars().count());
                (p, (line, col))
            })
            .collect()
    }

    /// Ctrl+T rows from ctags.
    fn tags_symbols(&mut self, query: &str) -> (Vec<Item>, String) {
        if !self.tags_ready() {
            return (Vec::new(), self.tags_status());
        }
        // Only names with the query's first letter go to the fuzzy matcher.
        let first = query.chars().next().unwrap_or(' ').to_ascii_lowercase();
        let all: Vec<Item> = self
            .lsp
            .tags
            .tags
            .iter()
            .filter(|t| t.name.chars().any(|c| c.to_ascii_lowercase() == first))
            .map(|t| {
                let detail = format!("{} · {}:{}", t.kind, self.short_path(&t.path), t.line + 1);
                Item::at(t.name.clone(), t.path.clone(), (t.line, 0), detail)
            })
            .collect();
        let mut items = filter_items(&all, query);
        items.truncate(MAX_SYMBOLS);
        let n = self.lsp.tags.tags.len();
        (items, format!("ctags · {n} tags"))
    }

    /// The symbols of one file from ctags (run on it alone, quickly).
    fn tags_outline(&mut self, path: &Path) -> Vec<Item> {
        let cmd = shell_words::split(&self.config.lsp.ctags).unwrap_or_default();
        let Some((program, args)) = cmd.split_first() else {
            return Vec::new();
        };
        let out = std::process::Command::new(program)
            .args(args)
            .args(["--fields=+nKS", "--sort=no", "-f", "-"])
            .arg(path)
            .output();
        let Some(out) = out.ok().filter(|o| o.status.success()) else {
            self.set_status("no language server for this file and ctags did not run");
            return Vec::new();
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let mut tags = ctags::parse(&text, Path::new("/"));
        tags.sort_by_key(|t| t.line);
        let spots: Vec<Spot> = tags
            .iter()
            .map(|t| (path.to_path_buf(), (t.line, 0)))
            .collect();
        let mut cache = HashMap::new();
        tags.into_iter()
            .zip(spots)
            .map(|(t, (p, (line, _)))| {
                let text = self.line_of(&p, line, &mut cache);
                let col = text.find(&t.name).map_or(0, |b| text[..b].chars().count());
                let name = match t.scope.split_once(':') {
                    Some((_, s)) => format!("{s}::{}", t.name),
                    None => t.name,
                };
                Item::at(
                    name,
                    p,
                    (line, col),
                    format!("{} · line {}", t.kind, line + 1),
                )
            })
            .collect()
    }

    /// Esc I: rebuild the ctags index now.
    pub(super) fn rebuild_tags(&mut self) {
        self.lsp.tags.built = None;
        self.lsp.tags.roots.clear();
        self.tags_ready();
        self.set_status("ctags: indexing the folders in the background …");
    }
}

/// Build (or reuse, when younger than [`TAGS_STALE`]) the tags of every
/// root; returns them all and the first error.
fn build_tags(
    cmd: &[String],
    roots: &[(String, PathBuf)],
    state: &Path,
) -> (Vec<Tag>, Option<String>) {
    let mut tags = Vec::new();
    let mut error = None;
    for (_, root) in roots {
        let out = state.join(format!("{:016x}.tags", fnv(&root.display().to_string())));
        let fresh = std::fs::metadata(&out)
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().is_ok_and(|e| e < TAGS_STALE));
        if !fresh {
            let one = vec![(String::new(), root.clone())];
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let (files, _) = super::finder::walk(&one, super::finder::MAX_INDEX, &cancel);
            let rel: Vec<String> = files.into_iter().map(|(_, f)| f).collect();
            if let Err(e) = ctags::build(cmd, root, &rel, &out) {
                error.get_or_insert(e.to_string());
                continue;
            }
        }
        if let Ok(text) = std::fs::read_to_string(&out) {
            tags.extend(ctags::parse(&text, root));
        }
    }
    (tags, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_at_the_cursor() {
        assert_eq!(word_at("int net_open(fd);", 6), "net_open");
        assert_eq!(
            word_at("int net_open(fd);", 12),
            "net_open",
            "just after the word"
        );
        assert_eq!(word_at("int net_open(fd);", 3), "int");
        assert_eq!(word_at("a + b", 2), "");
        assert_eq!(word_at("", 0), "");
    }
}
