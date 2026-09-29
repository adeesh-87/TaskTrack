//! Universal Ctags as a fallback index: where no language server runs (or
//! it finds nothing), definitions and symbols come from a tags file built in
//! the background over the same files quick open lists (so `.gitignore` is
//! respected), kept in the state folder and reused until it is old.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// One tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// Symbol name.
    pub name: String,
    /// File it is in.
    pub path: PathBuf,
    /// Line (0-based).
    pub line: usize,
    /// Kind, e.g. `function`, `macro`, `struct`.
    pub kind: String,
    /// Enclosing scope, e.g. `struct:foo`.
    pub scope: String,
}

/// Run `ctags` (a command line such as `["ctags"]`) over `files` (paths
/// relative to `root`), writing the tags to `out`.
pub fn build(command: &[String], root: &Path, files: &[String], out: &Path) -> io::Result<()> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty ctags command"))?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = out.with_extension("tmp");
    let mut child = Command::new(program)
        .args(args)
        .args(["--fields=+nKS", "--sort=no", "-f"])
        .arg(&tmp)
        .args(["-L", "-"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        for f in files {
            // A broken pipe means ctags stopped early: its exit says why.
            if writeln!(stdin, "{f}").is_err() {
                break;
            }
        }
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!(
            "{program}: {}",
            err.lines().next().unwrap_or("failed")
        )));
    }
    std::fs::rename(&tmp, out)
}

/// Parse a tags file whose paths are relative to `root`.
pub fn parse(text: &str, root: &Path) -> Vec<Tag> {
    let mut tags = Vec::new();
    for line in text.lines() {
        if line.starts_with("!_TAG_") || line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let (Some(name), Some(file)) = (parts.next(), parts.next()) else {
            continue;
        };
        let mut kind = String::new();
        let mut scope = String::new();
        let mut number = None;
        // The address may contain tabs: fields come after `;"`.
        let rest: Vec<&str> = parts.collect();
        let fields_from = rest
            .iter()
            .position(|p| p.ends_with(";\""))
            .map_or(0, |i| i + 1);
        if fields_from == 0 {
            if let Some(n) = rest.first().and_then(|a| a.parse::<usize>().ok()) {
                number = Some(n);
            }
        }
        for field in &rest[fields_from..] {
            match field.split_once(':') {
                Some(("line", n)) => number = n.parse().ok(),
                Some(("kind", k)) => k.clone_into(&mut kind),
                Some((k, v)) if SCOPES.contains(&k) => scope = format!("{k}:{v}"),
                None if kind.is_empty() => (*field).clone_into(&mut kind),
                _ => {}
            }
        }
        let Some(n) = number else { continue };
        let path = if Path::new(file).is_absolute() {
            PathBuf::from(file)
        } else {
            root.join(file)
        };
        tags.push(Tag {
            name: name.to_owned(),
            path,
            line: n.saturating_sub(1),
            kind,
            scope,
        });
    }
    tags
}

const SCOPES: [&str; 8] = [
    "class",
    "struct",
    "union",
    "enum",
    "namespace",
    "function",
    "typedef",
    "module",
];

/// How much a kind looks like a definition (higher first).
fn kind_rank(kind: &str) -> u8 {
    match kind {
        "function" | "method" | "class" | "struct" | "union" | "enum" | "typedef" | "macro"
        | "namespace" => 3,
        "variable" | "enumerator" | "member" | "field" => 2,
        "prototype" | "externvar" => 1,
        _ => 0,
    }
}

/// Tags named `name`, definitions first, those in `near` (the current file)
/// before others.
pub fn lookup<'a>(tags: &'a [Tag], name: &str, near: &Path) -> Vec<&'a Tag> {
    let mut found: Vec<&Tag> = tags.iter().filter(|t| t.name == name).collect();
    found.sort_by_key(|t| {
        (
            std::cmp::Reverse(kind_rank(&t.kind)),
            t.path != near,
            t.path.clone(),
            t.line,
        )
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAGS: &str = "!_TAG_FILE_FORMAT\t2\t/extended format/\n\
        net_open\tsrc/net.c\t/^int net_open(int fd)$/;\"\tkind:function\tline:12\tsignature:(int fd)\n\
        net_open\tinclude/net.h\t/^int net_open(int fd);$/;\"\tkind:prototype\tline:3\n\
        MAX_FD\tinclude/net.h\t5;\"\tkind:macro\tline:5\n\
        fd\tsrc/net.c\t/^  int fd;$/;\"\tkind:member\tline:4\tstruct:conn\n\
        broken line\n";

    #[test]
    fn parses_and_ranks_definitions_first() {
        let root = Path::new("/w");
        let tags = parse(TAGS, root);
        assert_eq!(tags.len(), 4);
        assert_eq!(tags[0].path, PathBuf::from("/w/src/net.c"));
        assert_eq!((tags[0].line, tags[0].kind.as_str()), (11, "function"));
        assert_eq!(tags[3].scope, "struct:conn");
        let found = lookup(&tags, "net_open", Path::new("/w/include/net.h"));
        assert_eq!(
            found[0].kind, "function",
            "a definition beats a nearer prototype"
        );
        assert_eq!(found[1].kind, "prototype");
        assert!(lookup(&tags, "nope", root).is_empty());
    }

    #[test]
    fn builds_with_real_ctags() {
        if Command::new("ctags").arg("--version").output().is_err() {
            eprintln!("ctags not installed: skipped");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.c"),
            "#define LIMIT 3\nstruct conn { int fd; };\nint net_open(int fd) { return fd; }\n",
        )
        .unwrap();
        let out = dir.path().join("state/tags");
        build(&["ctags".into()], dir.path(), &["a.c".into()], &out).unwrap();
        let tags = parse(&std::fs::read_to_string(&out).unwrap(), dir.path());
        let f = lookup(&tags, "net_open", dir.path())[0];
        assert_eq!((f.line, f.kind.as_str()), (2, "function"));
        assert_eq!(lookup(&tags, "LIMIT", dir.path())[0].kind, "macro");
    }
}
