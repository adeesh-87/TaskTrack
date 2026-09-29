//! A lazily expanded, flattened directory tree suitable for list rendering.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// One visible row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// Absolute path.
    pub path: PathBuf,
    /// Display name (file name).
    pub name: String,
    /// Nesting depth, 0 for direct children of the root.
    pub depth: usize,
    /// Whether this node is a directory.
    pub is_dir: bool,
    /// Whether the directory is currently expanded.
    pub expanded: bool,
    /// An extra root (an attached workspace or build), shown by its label.
    pub root: bool,
}

/// The tree state: root, expanded folders, flattened rows and the cursor.
#[derive(Debug)]
pub struct FileTree {
    root: PathBuf,
    show_hidden: bool,
    expanded: BTreeSet<PathBuf>,
    nodes: Vec<Node>,
    selected: usize,
    /// Modification times of the root and every expanded folder at the last refresh.
    dir_mtimes: Vec<(PathBuf, Option<SystemTime>)>,
    /// More folders listed after the root's entries (label, path): the task's
    /// attached code workspaces and builds.
    extra: Vec<(String, PathBuf)>,
}

fn mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl FileTree {
    /// Build a tree rooted at `root` with the top level expanded.
    pub fn new(root: &Path, show_hidden: bool) -> io::Result<Self> {
        let mut tree = Self {
            root: root.to_path_buf(),
            show_hidden,
            expanded: BTreeSet::new(),
            nodes: Vec::new(),
            selected: 0,
            dir_mtimes: Vec::new(),
            extra: Vec::new(),
        };
        tree.refresh()?;
        Ok(tree)
    }

    /// Root folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Show these folders (label, path) after the root's entries.
    pub fn set_extra_roots(&mut self, roots: Vec<(String, PathBuf)>) -> io::Result<()> {
        if roots == self.extra {
            return Ok(());
        }
        self.extra = roots;
        self.refresh()
    }

    /// Every root: the task folder, then the extra ones.
    pub fn roots(&self) -> Vec<(String, PathBuf)> {
        let task = self
            .root
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        std::iter::once((task, self.root.clone()))
            .chain(self.extra.iter().cloned())
            .collect()
    }

    /// The root `path` lives in (the deepest one), for file operations.
    pub fn root_of(&self, path: &Path) -> &Path {
        self.extra
            .iter()
            .map(|(_, p)| p.as_path())
            .filter(|p| path.starts_with(p))
            .max_by_key(|p| p.as_os_str().len())
            .unwrap_or(&self.root)
    }

    /// Visible rows.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Index of the selected row.
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// The selected row, if any.
    pub fn selected(&self) -> Option<&Node> {
        self.nodes.get(self.selected)
    }

    /// Whether dot-files are shown.
    pub fn show_hidden(&self) -> bool {
        self.show_hidden
    }

    /// Toggle showing dot-files.
    pub fn toggle_hidden(&mut self) -> io::Result<()> {
        self.show_hidden = !self.show_hidden;
        self.refresh()
    }

    /// Folder that new entries should be created in: the selected directory,
    /// or the parent of the selected file, or the root.
    pub fn target_dir(&self) -> PathBuf {
        match self.selected() {
            Some(n) if n.is_dir => n.path.clone(),
            Some(n) => n
                .path
                .parent()
                .map_or_else(|| self.root.clone(), Path::to_path_buf),
            None => self.root.clone(),
        }
    }

    /// Re-read the filesystem, keeping expansion state and selection where possible.
    pub fn refresh(&mut self) -> io::Result<()> {
        let previous = self.selected().map(|n| n.path.clone());
        let mut nodes = Vec::new();
        let root = self.root.clone();
        self.walk(&root, 0, &mut nodes)?;
        for (label, path) in &self.extra {
            let expanded = self.expanded.contains(path);
            nodes.push(Node {
                path: path.clone(),
                name: label.clone(),
                depth: 0,
                is_dir: true,
                expanded,
                root: true,
            });
            if expanded {
                let _ = self.walk(path, 1, &mut nodes);
            }
        }
        self.nodes = nodes;
        self.dir_mtimes = std::iter::once(root.clone())
            .chain(self.expanded.iter().cloned())
            .map(|p| {
                let m = mtime(&p);
                (p, m)
            })
            .collect();
        self.selected = previous
            .and_then(|p| self.nodes.iter().position(|n| n.path == p))
            .unwrap_or(0)
            .min(self.nodes.len().saturating_sub(1));
        Ok(())
    }

    /// Whether a shown folder changed on disk since the last refresh.
    pub fn changed_on_disk(&self) -> bool {
        self.dir_mtimes.iter().any(|(p, m)| mtime(p) != *m)
    }

    fn walk(&self, dir: &Path, depth: usize, out: &mut Vec<Node>) -> io::Result<()> {
        let mut entries: Vec<(String, PathBuf, bool)> = fs::read_dir(dir)?
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                if !self.show_hidden && name.starts_with('.') {
                    return None;
                }
                let is_dir =
                    e.file_type().map(|t| t.is_dir()).unwrap_or(false) || (e.path().is_dir());
                Some((name, e.path(), is_dir))
            })
            .collect();
        entries.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
        });
        for (name, path, is_dir) in entries {
            let expanded = is_dir && self.expanded.contains(&path);
            out.push(Node {
                path: path.clone(),
                name,
                depth,
                is_dir,
                expanded,
                root: false,
            });
            if expanded {
                // A folder we cannot read simply renders as empty.
                let _ = self.walk(&path, depth + 1, out);
            }
        }
        Ok(())
    }

    /// Move the cursor up.
    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Move the cursor down.
    pub fn select_next(&mut self) {
        if !self.nodes.is_empty() {
            self.selected = (self.selected + 1).min(self.nodes.len() - 1);
        }
    }

    /// Jump to the first row.
    pub fn select_first(&mut self) {
        self.selected = 0;
    }

    /// Jump to the last row.
    pub fn select_last(&mut self) {
        self.selected = self.nodes.len().saturating_sub(1);
    }

    /// Select row `index` (clamped).
    pub fn select_index(&mut self, index: usize) {
        self.selected = index.min(self.nodes.len().saturating_sub(1));
    }

    /// Select the row for `path`, if visible.
    pub fn select_path(&mut self, path: &Path) {
        if let Some(i) = self.nodes.iter().position(|n| n.path == path) {
            self.selected = i;
        }
    }

    /// Expand the folders down to `path` and select it. Returns whether it
    /// is in the tree.
    pub fn reveal(&mut self, path: &Path) -> io::Result<bool> {
        let root = self.root_of(path).to_path_buf();
        if !path.starts_with(&root) {
            return Ok(false);
        }
        let mut dir = path.parent();
        while let Some(d) = dir.filter(|d| d.starts_with(&root) && *d != self.root) {
            self.expanded.insert(d.to_path_buf());
            dir = d.parent();
        }
        self.refresh()?;
        self.select_path(path);
        Ok(self.selected().is_some_and(|n| n.path == path))
    }

    /// Expand or collapse the selected directory. Returns `true` if it was a directory.
    pub fn toggle_selected(&mut self) -> io::Result<bool> {
        let Some(node) = self.selected() else {
            return Ok(false);
        };
        if !node.is_dir {
            return Ok(false);
        }
        let path = node.path.clone();
        if !self.expanded.remove(&path) {
            self.expanded.insert(path);
        }
        self.refresh()?;
        Ok(true)
    }

    /// Expand the selected directory (no-op for files or already-expanded folders).
    pub fn expand_selected(&mut self) -> io::Result<()> {
        let target = self
            .selected()
            .filter(|n| n.is_dir && !n.expanded)
            .map(|n| n.path.clone());
        if let Some(path) = target {
            self.expanded.insert(path);
            self.refresh()?;
        }
        Ok(())
    }

    /// Collapse the selected directory, or jump to the parent when on a file or
    /// an already-collapsed folder.
    pub fn collapse_selected(&mut self) -> io::Result<()> {
        let Some(n) = self.selected() else {
            return Ok(());
        };
        let (path, collapse) = (n.path.clone(), n.is_dir && n.expanded);
        if collapse {
            self.expanded.remove(&path);
            self.refresh()?;
        } else if let Some(parent) = path.parent() {
            self.select_path(parent);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("scripts/nested")).unwrap();
        fs::write(dir.path().join("CONTEXT.md"), "ctx").unwrap();
        fs::write(dir.path().join("scripts/run.sh"), "echo").unwrap();
        fs::write(dir.path().join("scripts/nested/deep.txt"), "deep").unwrap();
        fs::write(dir.path().join(".secret"), "hidden").unwrap();
        dir
    }

    #[test]
    fn lists_top_level_with_dirs_first() {
        let dir = fixture();
        let tree = FileTree::new(dir.path(), false).unwrap();
        let names: Vec<_> = tree.nodes().iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["scripts", "CONTEXT.md"]);
    }

    #[test]
    fn hidden_files_toggle() {
        let dir = fixture();
        let mut tree = FileTree::new(dir.path(), false).unwrap();
        assert!(!tree.nodes().iter().any(|n| n.name == ".secret"));
        tree.toggle_hidden().unwrap();
        assert!(tree.nodes().iter().any(|n| n.name == ".secret"));
    }

    #[test]
    fn expand_and_collapse() {
        let dir = fixture();
        let mut tree = FileTree::new(dir.path(), false).unwrap();
        assert!(tree.toggle_selected().unwrap());
        let names: Vec<_> = tree.nodes().iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["scripts", "nested", "run.sh", "CONTEXT.md"]);
        assert_eq!(tree.nodes()[1].depth, 1);
        tree.select_next();
        tree.expand_selected().unwrap();
        assert!(tree.nodes().iter().any(|n| n.name == "deep.txt"));
        tree.select_next();
        tree.collapse_selected().unwrap(); // on deep.txt -> jump to parent
        assert_eq!(tree.selected().unwrap().name, "nested");
        tree.collapse_selected().unwrap(); // collapse nested
        assert!(!tree.nodes().iter().any(|n| n.name == "deep.txt"));
        tree.select_first();
        assert!(tree.toggle_selected().unwrap());
        assert_eq!(tree.nodes().len(), 2);
    }

    #[test]
    fn target_dir_follows_selection() {
        let dir = fixture();
        let mut tree = FileTree::new(dir.path(), false).unwrap();
        assert_eq!(tree.target_dir(), dir.path().join("scripts"));
        tree.select_last();
        assert_eq!(tree.target_dir(), dir.path());
    }

    #[test]
    fn extra_roots_follow_the_task_folder() {
        let dir = fixture();
        let code = tempfile::tempdir().unwrap();
        fs::create_dir_all(code.path().join("src")).unwrap();
        fs::write(code.path().join("src/main.c"), "int main;").unwrap();
        let mut tree = FileTree::new(dir.path(), false).unwrap();
        tree.set_extra_roots(vec![("fw · code".into(), code.path().to_path_buf())])
            .unwrap();
        let names: Vec<_> = tree.nodes().iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["scripts", "CONTEXT.md", "fw · code"]);
        assert!(tree.nodes()[2].root);
        tree.select_last();
        assert!(tree.toggle_selected().unwrap());
        tree.select_next();
        tree.expand_selected().unwrap();
        tree.select_next();
        let main = tree.selected().unwrap().path.clone();
        assert!(main.ends_with("src/main.c"));
        assert_eq!(tree.root_of(&main), code.path());
        assert_eq!(tree.root_of(&dir.path().join("CONTEXT.md")), dir.path());
        assert_eq!(tree.roots().len(), 2);
        // Collapsing on a file inside the root walks up to its folder.
        tree.collapse_selected().unwrap();
        assert_eq!(tree.selected().unwrap().name, "src");
        // Revealing a file expands the folders down to it.
        tree.select_last();
        tree.collapse_selected().unwrap();
        tree.select_first();
        tree.select_last();
        tree.collapse_selected().unwrap();
        assert!(tree.reveal(&main).unwrap());
        assert_eq!(tree.selected().unwrap().path, main);
        assert!(!tree.reveal(Path::new("/elsewhere/x")).unwrap());
    }

    #[test]
    fn refresh_keeps_selection_and_clamps() {
        let dir = fixture();
        let mut tree = FileTree::new(dir.path(), false).unwrap();
        tree.select_last();
        fs::remove_file(dir.path().join("CONTEXT.md")).unwrap();
        tree.refresh().unwrap();
        assert_eq!(tree.selected_index(), 0);
    }
}
