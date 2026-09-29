//! A small Language Server Protocol client: JSON-RPC over a server's stdin
//! and stdout, one reader thread per server. What to ask and what the
//! answers mean is up to the app (`src/app/lsp.rs`); this module only
//! starts servers, frames messages and converts positions and paths.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

pub mod ctags;

/// How servers count columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// Chars (what the editor uses).
    Utf32,
    /// UTF-16 code units (the LSP default).
    Utf16,
    /// Bytes.
    Utf8,
}

impl Encoding {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "utf-32" => Some(Self::Utf32),
            "utf-16" => Some(Self::Utf16),
            "utf-8" => Some(Self::Utf8),
            _ => None,
        }
    }
}

/// Column `char_col` of `line` as the server counts it.
pub fn to_lsp_col(line: &str, char_col: usize, enc: Encoding) -> u32 {
    let n: usize = match enc {
        Encoding::Utf32 => char_col,
        Encoding::Utf16 => line.chars().take(char_col).map(char::len_utf16).sum(),
        Encoding::Utf8 => line.chars().take(char_col).map(char::len_utf8).sum(),
    };
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// A server's column `col` of `line` as a char column.
pub fn from_lsp_col(line: &str, col: u32, enc: Encoding) -> usize {
    let col = col as usize;
    match enc {
        Encoding::Utf32 => col.min(line.chars().count()),
        Encoding::Utf16 | Encoding::Utf8 => {
            let mut units = 0;
            for (i, c) in line.chars().enumerate() {
                if units >= col {
                    return i;
                }
                units += if enc == Encoding::Utf16 {
                    c.len_utf16()
                } else {
                    c.len_utf8()
                };
            }
            line.chars().count()
        }
    }
}

/// `file://` URI of an absolute path.
pub fn path_to_uri(path: &Path) -> String {
    let mut out = String::from("file://");
    for b in path.to_string_lossy().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                use std::fmt::Write as _;
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// The path of a `file://` URI.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(b) = u8::from_str_radix(hex, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    Some(PathBuf::from(String::from_utf8_lossy(&out).into_owned()))
}

/// The LSP language id of a file, from its name.
pub fn language_id(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "c" => "c",
        "h" | "cc" | "cpp" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "ipp" | "inl" | "tcc" => "cpp",
        "m" => "objective-c",
        "rs" => "rust",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" => "typescript",
        "lua" => "lua",
        "zig" => "zig",
        "sh" | "bash" => "shellscript",
        _ => return None,
    })
}

/// Files that mark a project's root for its language server.
const ROOT_MARKERS: [&str; 7] = [
    "compile_commands.json",
    "compile_flags.txt",
    ".clangd",
    "Cargo.toml",
    "go.mod",
    "pyproject.toml",
    "package.json",
];

/// The folder a server for `file` should run in: the nearest one with a
/// compile database (or `build/compile_commands.json`) or another project
/// marker, else the nearest git checkout, else `fallback`.
pub fn find_root(file: &Path, fallback: &Path) -> PathBuf {
    let dirs: Vec<&Path> = file.ancestors().skip(1).collect();
    for d in &dirs {
        if ROOT_MARKERS.iter().any(|m| d.join(m).exists())
            || d.join("build/compile_commands.json").exists()
        {
            return d.to_path_buf();
        }
    }
    dirs.iter()
        .find(|d| d.join(".git").exists())
        .map_or_else(|| fallback.to_path_buf(), |d| d.to_path_buf())
}

/// Read one framed message (`None` at the end of the stream).
pub fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            if length.is_some() {
                break;
            }
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                length = v.trim().parse::<usize>().ok();
            }
        }
    }
    let mut body = vec![0; length.unwrap_or(0)];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Frame and write one message.
pub fn write_message(out: &mut impl Write, msg: &Value) -> io::Result<()> {
    let body = msg.to_string();
    write!(out, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    out.flush()
}

/// What a server sent.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// The answer to one of our requests.
    Response {
        /// Our request id.
        id: u64,
        /// Its result, or the error message.
        result: Result<Value, String>,
    },
    /// A notification (diagnostics, progress, …).
    Notification {
        /// Method.
        method: String,
        /// Parameters.
        params: Value,
    },
    /// A request the server makes of us (it needs an answer).
    Request {
        /// Its id, echoed in the answer.
        id: Value,
        /// Method.
        method: String,
        /// Parameters.
        params: Value,
    },
}

/// Sort a message into response / notification / request.
pub fn classify(mut msg: Value) -> Option<Incoming> {
    let method = msg.get("method").and_then(Value::as_str).map(str::to_owned);
    let params = msg.get_mut("params").map_or(Value::Null, Value::take);
    match (method, msg.get("id").cloned()) {
        (Some(method), Some(id)) => Some(Incoming::Request { id, method, params }),
        (Some(method), None) => Some(Incoming::Notification { method, params }),
        (None, Some(id)) => {
            let id = id.as_u64()?;
            let result = match msg.get_mut("error") {
                Some(e) => Err(e
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("error")
                    .to_owned()),
                None => Ok(msg.get_mut("result").map_or(Value::Null, Value::take)),
            };
            Some(Incoming::Response { id, result })
        }
        (None, None) => None,
    }
}

/// What a server's threads report.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerEvent {
    /// A message arrived.
    Message(Value),
    /// The server ended; the last lines it wrote to stderr.
    Exited(String),
}

/// A running language server.
pub struct Server {
    /// Command line.
    pub command: Vec<String>,
    /// Folder it runs for.
    pub root: PathBuf,
    /// Answered `initialize`.
    pub ready: bool,
    /// How it counts columns.
    pub encoding: Encoding,
    /// What it can do (`initialize` result's `capabilities`).
    pub capabilities: Value,
    stdin: ChildStdin,
    child: Child,
    next_id: u64,
    /// Messages held until `initialize` is answered.
    queued: Vec<Value>,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("command", &self.command)
            .field("root", &self.root)
            .field("ready", &self.ready)
            .finish_non_exhaustive()
    }
}

impl Server {
    /// Start `command` in `root` and send `initialize` (its id is 1).
    /// `report` gets every message and the exit, from the reader thread.
    pub fn start(
        command: &[String],
        root: &Path,
        report: impl Fn(ServerEvent) + Send + 'static,
    ) -> io::Result<Self> {
        let (program, args) = command
            .split_first()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty command"))?;
        let mut child = Command::new(program)
            .args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("no stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("no stderr"))?;
        let tail = Arc::new(Mutex::new(VecDeque::new()));
        let err_tail = tail.clone();
        let name = program.clone();
        std::thread::Builder::new()
            .name("lsp-stderr".into())
            .spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    tracing::debug!(target: "lsp", "{name}: {line}");
                    if let Ok(mut t) = err_tail.lock() {
                        t.push_back(line);
                        if t.len() > 8 {
                            t.pop_front();
                        }
                    }
                }
            })?;
        std::thread::Builder::new()
            .name("lsp-reader".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                while let Ok(Some(msg)) = read_message(&mut reader) {
                    report(ServerEvent::Message(msg));
                }
                // Give stderr a moment to arrive before reporting it.
                std::thread::sleep(std::time::Duration::from_millis(50));
                let last = tail
                    .lock()
                    .map(|t| t.iter().cloned().collect::<Vec<_>>().join(" | "))
                    .unwrap_or_default();
                report(ServerEvent::Exited(last));
            })?;
        let mut server = Self {
            command: command.to_vec(),
            root: root.to_path_buf(),
            ready: false,
            encoding: Encoding::Utf16,
            capabilities: Value::Null,
            stdin,
            child,
            next_id: 1,
            queued: Vec::new(),
        };
        let init = initialize_params(root);
        server
            .write(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": init}))?;
        server.next_id = 2;
        Ok(server)
    }

    fn write(&mut self, msg: &Value) -> io::Result<()> {
        write_message(&mut self.stdin, msg)
    }

    fn send(&mut self, msg: Value) {
        if self.ready {
            if let Err(e) = self.write(&msg) {
                tracing::warn!("lsp write: {e}");
            }
        } else {
            self.queued.push(msg);
        }
    }

    /// Send a request; returns its id (the answer comes as a `Response`).
    pub fn request(&mut self, method: &str, params: &Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        id
    }

    /// Send a notification.
    pub fn notify(&mut self, method: &str, params: &Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    /// Answer a request of the server.
    pub fn reply(&mut self, id: &Value, result: &Value) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }

    /// `initialize` was answered: note what the server does and send what
    /// waited for it.
    pub fn initialized(&mut self, result: &Value) {
        self.capabilities = result.get("capabilities").cloned().unwrap_or(Value::Null);
        self.encoding = self
            .capabilities
            .get("positionEncoding")
            .or_else(|| result.get("offsetEncoding"))
            .and_then(Value::as_str)
            .and_then(Encoding::from_name)
            .unwrap_or(Encoding::Utf16);
        self.ready = true;
        let _ = self.write(&json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}));
        for msg in std::mem::take(&mut self.queued) {
            let _ = self.write(&msg);
        }
    }

    /// Whether the server says it can do `capability` (e.g. `definitionProvider`).
    pub fn can(&self, capability: &str) -> bool {
        self.capabilities
            .get(capability)
            .is_some_and(|v| !matches!(v, Value::Null | Value::Bool(false)))
    }

    /// Ask the server to end, then make sure it does.
    pub fn stop(mut self) {
        if self.ready {
            let _ = self.write(&json!({"jsonrpc": "2.0", "id": 0, "method": "shutdown"}));
            let _ = self.write(&json!({"jsonrpc": "2.0", "method": "exit"}));
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
        while std::time::Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// What we tell a server about ourselves.
fn initialize_params(root: &Path) -> Value {
    let uri = path_to_uri(root);
    let name = root
        .file_name()
        .map_or_else(|| "root".to_owned(), |n| n.to_string_lossy().into_owned());
    json!({
        "processId": std::process::id(),
        "clientInfo": {"name": "pahiri", "version": env!("CARGO_PKG_VERSION")},
        "rootUri": uri,
        "rootPath": root.display().to_string(),
        "workspaceFolders": [{"uri": uri, "name": name}],
        "capabilities": {
            "general": {"positionEncodings": ["utf-32", "utf-16"]},
            // clangd's older spelling of the same.
            "offsetEncoding": ["utf-32", "utf-16"],
            "textDocument": {
                "synchronization": {"didSave": true, "dynamicRegistration": false},
                "completion": {
                    "completionItem": {"snippetSupport": false, "documentationFormat": ["plaintext"]},
                    "contextSupport": false
                },
                "hover": {"contentFormat": ["plaintext", "markdown"]},
                "definition": {"linkSupport": true},
                "declaration": {"linkSupport": true},
                "references": {},
                "documentSymbol": {"hierarchicalDocumentSymbolSupport": true},
                "publishDiagnostics": {"relatedInformation": false}
            },
            "workspace": {"symbol": {}, "workspaceFolders": true, "configuration": false},
            "window": {"workDoneProgress": true}
        }
    })
}

/// A place in a file (char columns once converted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// The file.
    pub path: PathBuf,
    /// Line (0-based).
    pub line: usize,
    /// Column as the server counts it (convert with [`from_lsp_col`]).
    pub col: u32,
}

/// Locations in a `definition` / `references` answer: a `Location`, a list
/// of them, or a list of `LocationLink`s.
pub fn parse_locations(v: &Value) -> Vec<Location> {
    let one = |l: &Value| -> Option<Location> {
        let uri = l.get("uri").or_else(|| l.get("targetUri"))?.as_str()?;
        let range = l
            .get("targetSelectionRange")
            .or_else(|| l.get("range"))
            .or_else(|| l.get("targetRange"))?;
        let start = range.get("start")?;
        Some(Location {
            path: uri_to_path(uri)?,
            line: usize::try_from(start.get("line")?.as_u64()?).ok()?,
            col: u32::try_from(start.get("character")?.as_u64()?).ok()?,
        })
    };
    match v {
        Value::Array(items) => items.iter().filter_map(one).collect(),
        Value::Object(_) => one(v).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// The text of a `hover` answer (markdown fences dropped).
pub fn hover_text(v: &Value) -> String {
    fn piece(c: &Value) -> String {
        match c {
            Value::String(s) => s.clone(),
            Value::Object(o) => o
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            Value::Array(a) => a.iter().map(piece).collect::<Vec<_>>().join("\n\n"),
            _ => String::new(),
        }
    }
    let text = piece(v.get("contents").unwrap_or(&Value::Null));
    text.lines()
        .filter(|l| !l.trim_start().starts_with("```"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

/// Symbol kind names (LSP `SymbolKind`).
pub fn symbol_kind(kind: u64) -> &'static str {
    match kind {
        1 => "file",
        2 => "module",
        3 => "namespace",
        4 => "package",
        5 => "class",
        6 => "method",
        7 => "property",
        8 => "field",
        9 => "constructor",
        10 => "enum",
        11 => "interface",
        12 => "function",
        13 => "variable",
        14 => "constant",
        15 => "string",
        16 => "number",
        17 => "boolean",
        18 => "array",
        22 => "enum member",
        23 => "struct",
        24 => "event",
        25 => "operator",
        26 => "type parameter",
        _ => "symbol",
    }
}

/// A named symbol with where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// Name (with its container for nested ones, e.g. `Foo::bar`).
    pub name: String,
    /// Kind, e.g. `function`.
    pub kind: String,
    /// Where.
    pub location: Location,
}

/// Symbols of a `workspace/symbol` or `textDocument/documentSymbol` answer
/// (`path` for document symbols, which carry no URI); nested ones are
/// flattened with their parent's name.
pub fn parse_symbols(v: &Value, path: Option<&Path>) -> Vec<Symbol> {
    fn walk(v: &Value, path: Option<&Path>, parent: &str, out: &mut Vec<Symbol>) {
        let Some(items) = v.as_array() else { return };
        for s in items {
            let Some(name) = s.get("name").and_then(Value::as_str) else {
                continue;
            };
            let full = match s.get("containerName").and_then(Value::as_str) {
                Some(c) if !c.is_empty() && parent.is_empty() => format!("{c}::{name}"),
                _ if !parent.is_empty() => format!("{parent}::{name}"),
                _ => name.to_owned(),
            };
            let kind = symbol_kind(s.get("kind").and_then(Value::as_u64).unwrap_or(0));
            let loc = if let Some(l) = s.get("location") {
                parse_locations(l).into_iter().next()
            } else {
                let range = s.get("selectionRange").or_else(|| s.get("range"));
                range.and_then(|r| r.get("start")).and_then(|st| {
                    Some(Location {
                        path: path?.to_path_buf(),
                        line: usize::try_from(st.get("line")?.as_u64()?).ok()?,
                        col: u32::try_from(st.get("character")?.as_u64()?).ok()?,
                    })
                })
            };
            if let Some(location) = loc {
                out.push(Symbol {
                    name: full.clone(),
                    kind: kind.to_owned(),
                    location,
                });
            }
            if let Some(children) = s.get("children") {
                walk(children, path, &full, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(v, path, "", &mut out);
    out
}

/// One completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    /// Shown.
    pub label: String,
    /// Inserted.
    pub insert: String,
    /// Matched against what is typed.
    pub filter: String,
    /// Type or signature.
    pub detail: String,
    sort: String,
}

impl CompletionItem {
    /// A plain word (no server behind it).
    pub fn word(w: &str) -> Self {
        Self {
            label: w.to_owned(),
            insert: w.to_owned(),
            filter: w.to_owned(),
            detail: String::new(),
            sort: String::new(),
        }
    }
}

/// Items of a `completion` answer, in the server's order.
pub fn parse_completions(v: &Value) -> Vec<CompletionItem> {
    let items = match v {
        Value::Array(a) => a.as_slice(),
        Value::Object(o) => o
            .get("items")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice),
        _ => &[],
    };
    let mut out: Vec<CompletionItem> = items
        .iter()
        .filter_map(|i| {
            let label = i.get("label")?.as_str()?.trim().to_owned();
            let insert = i
                .get("textEdit")
                .and_then(|t| t.get("newText"))
                .or_else(|| i.get("insertText"))
                .and_then(Value::as_str)
                .unwrap_or(&label)
                .to_owned();
            let filter = i
                .get("filterText")
                .and_then(Value::as_str)
                .unwrap_or(&insert)
                .to_owned();
            let detail = i
                .get("detail")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_owned();
            let sort = i
                .get("sortText")
                .and_then(Value::as_str)
                .unwrap_or(&label)
                .to_owned();
            Some(CompletionItem {
                label,
                insert,
                filter,
                detail,
                sort,
            })
        })
        .collect();
    out.sort_by(|a, b| a.sort.cmp(&b.sort));
    out
}

/// A problem the server found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// First line.
    pub line: usize,
    /// Column on it (server units).
    pub col: u32,
    /// Last line.
    pub end_line: usize,
    /// Column after the end (server units).
    pub end_col: u32,
    /// 1 error, 2 warning, 3 information, 4 hint.
    pub severity: u8,
    /// What is wrong.
    pub message: String,
}

/// The diagnostics of a `publishDiagnostics` notification: (file, list).
pub fn parse_diagnostics(params: &Value) -> Option<(PathBuf, Vec<Diagnostic>)> {
    let path = uri_to_path(params.get("uri")?.as_str()?)?;
    let pos = |p: Option<&Value>, key: &str| -> u64 {
        p.and_then(|p| p.get(key))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    let list = params
        .get("diagnostics")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|d| {
                    let range = d.get("range");
                    let start = range.and_then(|r| r.get("start"));
                    let end = range.and_then(|r| r.get("end"));
                    let source = d.get("source").and_then(Value::as_str).unwrap_or_default();
                    let message = d
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_owned();
                    Diagnostic {
                        line: usize::try_from(pos(start, "line")).unwrap_or(0),
                        col: u32::try_from(pos(start, "character")).unwrap_or(0),
                        end_line: usize::try_from(pos(end, "line")).unwrap_or(0),
                        end_col: u32::try_from(pos(end, "character")).unwrap_or(0),
                        severity: d
                            .get("severity")
                            .and_then(Value::as_u64)
                            .and_then(|s| u8::try_from(s).ok())
                            .unwrap_or(1),
                        message: if source.is_empty() {
                            message
                        } else {
                            format!("{message} ({source})")
                        },
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    Some((path, list))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_round_trip_and_classify() {
        let mut buf = Vec::new();
        write_message(
            &mut buf,
            &json!({"jsonrpc": "2.0", "id": 3, "result": {"a": "é"}}),
        )
        .unwrap();
        write_message(
            &mut buf,
            &json!({"jsonrpc": "2.0", "method": "x", "params": [1]}),
        )
        .unwrap();
        write_message(
            &mut buf,
            &json!({"jsonrpc": "2.0", "id": "s1", "method": "y"}),
        )
        .unwrap();
        write_message(
            &mut buf,
            &json!({"jsonrpc": "2.0", "id": 4, "error": {"message": "no"}}),
        )
        .unwrap();
        let mut r = io::Cursor::new(buf);
        let mut got = Vec::new();
        while let Some(m) = read_message(&mut r).unwrap() {
            got.push(classify(m).unwrap());
        }
        assert_eq!(
            got,
            [
                Incoming::Response {
                    id: 3,
                    result: Ok(json!({"a": "é"}))
                },
                Incoming::Notification {
                    method: "x".into(),
                    params: json!([1])
                },
                Incoming::Request {
                    id: json!("s1"),
                    method: "y".into(),
                    params: Value::Null
                },
                Incoming::Response {
                    id: 4,
                    result: Err("no".into())
                },
            ]
        );
    }

    #[test]
    fn columns_uris_languages_and_roots() {
        let line = "a😀é b";
        assert_eq!(to_lsp_col(line, 3, Encoding::Utf16), 4);
        assert_eq!(to_lsp_col(line, 3, Encoding::Utf8), 7);
        assert_eq!(to_lsp_col(line, 3, Encoding::Utf32), 3);
        assert_eq!(from_lsp_col(line, 4, Encoding::Utf16), 3);
        assert_eq!(from_lsp_col(line, 7, Encoding::Utf8), 3);
        assert_eq!(from_lsp_col(line, 99, Encoding::Utf32), 5);
        let p = Path::new("/home/me/my src/a#b.c");
        let uri = path_to_uri(p);
        assert_eq!(uri, "file:///home/me/my%20src/a%23b.c");
        assert_eq!(uri_to_path(&uri).unwrap(), p);
        assert_eq!(language_id(Path::new("x/foo.H")), Some("cpp"));
        assert_eq!(language_id(Path::new("foo.c")), Some("c"));
        assert_eq!(language_id(Path::new("Makefile")), None);
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("lib/net")).unwrap();
        std::fs::create_dir_all(repo.join("build")).unwrap();
        let file = repo.join("lib/net/a.c");
        assert_eq!(find_root(&file, dir.path()), repo, "git checkout");
        std::fs::write(repo.join("build/compile_commands.json"), "[]").unwrap();
        std::fs::write(repo.join("lib/compile_flags.txt"), "").unwrap();
        assert_eq!(
            find_root(&file, dir.path()),
            repo.join("lib"),
            "nearest marker"
        );
        assert_eq!(
            find_root(&dir.path().join("x/y.c"), dir.path()),
            dir.path(),
            "fallback"
        );
    }

    #[test]
    fn answers_parse() {
        let loc = json!({"uri": "file:///a.c", "range": {"start": {"line": 3, "character": 4}, "end": {"line": 3, "character": 8}}});
        assert_eq!(parse_locations(&loc)[0].line, 3);
        let links = json!([{"targetUri": "file:///b.c", "targetRange": {"start": {"line": 1, "character": 0}},
            "targetSelectionRange": {"start": {"line": 2, "character": 5}}}]);
        assert_eq!(
            parse_locations(&links),
            [Location {
                path: "/b.c".into(),
                line: 2,
                col: 5
            }]
        );
        assert!(parse_locations(&Value::Null).is_empty());
        let hover = json!({"contents": {"kind": "markdown", "value": "### function `f`\n```cpp\nint f()\n```"}});
        assert_eq!(hover_text(&hover), "### function `f`\nint f()");
        let doc_syms = json!([{"name": "Foo", "kind": 5, "selectionRange": {"start": {"line": 1, "character": 6}},
            "children": [{"name": "bar", "kind": 6, "selectionRange": {"start": {"line": 2, "character": 7}}}]}]);
        let syms = parse_symbols(&doc_syms, Some(Path::new("/x.cpp")));
        let names: Vec<_> = syms
            .iter()
            .map(|s| (s.name.as_str(), s.kind.as_str()))
            .collect();
        assert_eq!(names, [("Foo", "class"), ("Foo::bar", "method")]);
        let ws = json!([{"name": "open", "kind": 12, "containerName": "net", "location": loc}]);
        assert_eq!(parse_symbols(&ws, None)[0].name, "net::open");
        let comp = json!({"isIncomplete": false, "items": [
            {"label": " printf(const char *, ...)", "insertText": "printf", "sortText": "2", "detail": "int"},
            {"label": "print_me", "textEdit": {"newText": "print_me", "range": {}}, "sortText": "1"}]});
        let items = parse_completions(&comp);
        assert_eq!(items[0].insert, "print_me");
        assert_eq!(items[1].label, "printf(const char *, ...)");
        assert_eq!(items[1].detail, "int");
        let diag = json!({"uri": "file:///a.c", "diagnostics": [{"range": {"start": {"line": 1, "character": 2}, "end": {"line": 1, "character": 5}},
            "severity": 2, "message": "unused variable 'x'\nmore", "source": "clang"}]});
        let (path, list) = parse_diagnostics(&diag).unwrap();
        assert_eq!(path, PathBuf::from("/a.c"));
        assert_eq!(list[0].message, "unused variable 'x' (clang)");
        assert_eq!((list[0].line, list[0].col, list[0].severity), (1, 2, 2));
    }
}
