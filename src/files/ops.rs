//! File operations: probing, create, rename and delete.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// What we learned about a file before opening it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileProbe {
    /// Size in bytes.
    pub size: u64,
    /// Whether the first chunk looks like binary data.
    pub is_binary: bool,
}

impl FileProbe {
    /// Whether opening should be confirmed first.
    pub fn needs_warning(&self, large_file_kb: u64) -> bool {
        self.is_binary || self.size > large_file_kb.saturating_mul(1024)
    }
}

/// Inspect a file without reading it entirely.
pub fn probe(path: &Path) -> io::Result<FileProbe> {
    let size = fs::metadata(path)?.len();
    let mut head = vec![0u8; 8192];
    let mut file = fs::File::open(path)?;
    let n = file.read(&mut head)?;
    head.truncate(n);
    Ok(FileProbe {
        size,
        is_binary: looks_binary(&head),
    })
}

/// Heuristic: NUL bytes or invalid UTF-8 in the sample means binary.
pub fn looks_binary(sample: &[u8]) -> bool {
    if sample.contains(&0) {
        return true;
    }
    match std::str::from_utf8(sample) {
        Ok(_) => false,
        // A truncated multi-byte sequence at the very end is fine.
        Err(e) => e.error_len().is_some(),
    }
}

fn validate_name(name: &str) -> io::Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid name {name:?}"),
        ));
    }
    Ok(())
}

fn ensure_absent(path: &Path) -> io::Result<()> {
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", path.display()),
        ));
    }
    Ok(())
}

/// Create an empty file `name` inside `dir`.
pub fn create_file(dir: &Path, name: &str) -> io::Result<PathBuf> {
    validate_name(name)?;
    let path = dir.join(name);
    ensure_absent(&path)?;
    fs::write(&path, "")?;
    Ok(path)
}

/// Create a folder `name` inside `dir`.
pub fn create_dir(dir: &Path, name: &str) -> io::Result<PathBuf> {
    validate_name(name)?;
    let path = dir.join(name);
    ensure_absent(&path)?;
    fs::create_dir(&path)?;
    Ok(path)
}

/// Rename `path` to `new_name` within the same folder.
pub fn rename(path: &Path, new_name: &str) -> io::Result<PathBuf> {
    validate_name(new_name)?;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "cannot rename root"))?;
    let target = parent.join(new_name);
    if target == path {
        return Ok(target);
    }
    ensure_absent(&target)?;
    fs::rename(path, &target)?;
    Ok(target)
}

/// Delete a file or a folder (recursively). Refuses to delete `root` itself or
/// anything outside it.
pub fn delete(root: &Path, path: &Path) -> io::Result<()> {
    if path == root || !path.starts_with(root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to delete outside the task folder",
        ));
    }
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_detection() {
        assert!(!looks_binary(b"hello world\n"));
        assert!(looks_binary(b"\x00\x01\x02"));
        assert!(looks_binary(&[0xff, 0xfe, 0x41]));
        // Truncated UTF-8 tail should not count as binary.
        let mut s = "héllo".as_bytes().to_vec();
        s.push(0xC3);
        assert!(!looks_binary(&s));
    }

    #[test]
    fn probe_reports_size_and_kind() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "abc").unwrap();
        let probe = probe(&p).unwrap();
        assert_eq!(
            probe,
            FileProbe {
                size: 3,
                is_binary: false
            }
        );
        assert!(!probe.needs_warning(1));
        assert!(FileProbe {
            size: 2048,
            is_binary: false
        }
        .needs_warning(1));
        assert!(FileProbe {
            size: 1,
            is_binary: true
        }
        .needs_warning(1));
    }

    #[test]
    fn create_rename_delete() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let f = create_file(root, "note.md").unwrap();
        assert!(f.is_file());
        assert!(create_file(root, "note.md").is_err());
        assert!(create_file(root, "../x").is_err());
        let d = create_dir(root, "sub").unwrap();
        assert!(d.is_dir());
        let renamed = rename(&f, "renamed.md").unwrap();
        assert!(renamed.is_file() && !f.exists());
        assert!(rename(&renamed, "sub").is_err());
        delete(root, &renamed).unwrap();
        delete(root, &d).unwrap();
        assert!(!renamed.exists() && !d.exists());
        assert!(delete(root, root).is_err());
        assert!(delete(root, Path::new("/etc/passwd")).is_err());
    }
}
