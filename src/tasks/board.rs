//! Parsing and rendering of the Markdown board file.

/// One column of the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// Category name as configured.
    pub name: String,
    /// Task ids in display order.
    pub tasks: Vec<String>,
}

/// The board: an ordered list of categories, each with its task ids.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Board {
    /// Columns in configured order.
    pub columns: Vec<Column>,
}

impl Board {
    /// An empty board with the given categories.
    pub fn new<S: AsRef<str>>(categories: &[S]) -> Self {
        Self {
            columns: categories
                .iter()
                .map(|c| Column {
                    name: c.as_ref().to_owned(),
                    tasks: Vec::new(),
                })
                .collect(),
        }
    }

    /// Parse Markdown text into a board with exactly the configured categories.
    ///
    /// Headings are matched case-insensitively. Tasks listed under unknown
    /// headings, or before any heading, are placed in the first category so
    /// nothing silently disappears.
    pub fn parse<S: AsRef<str>>(markdown: &str, categories: &[S]) -> Self {
        let mut board = Self::new(categories);
        if board.columns.is_empty() {
            return board;
        }
        let mut current: Option<usize> = None;
        let mut orphans: Vec<String> = Vec::new();
        for raw in markdown.lines() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(heading) = heading_text(line) {
                let lower = heading.to_lowercase();
                current = board
                    .columns
                    .iter()
                    .position(|c| c.name.to_lowercase() == lower);
                if current.is_none() && lower != "status" {
                    // Unknown heading: remember its items as orphans.
                    current = None;
                }
                continue;
            }
            let Some(item) = list_item_text(line) else {
                continue;
            };
            match current {
                Some(idx) => board.columns[idx].tasks.push(item),
                None => orphans.push(item),
            }
        }
        for orphan in orphans {
            if !board.contains(&orphan) {
                board.columns[0].tasks.push(orphan);
            }
        }
        board.dedup();
        board
    }

    /// Render the board back to Markdown.
    pub fn render(&self) -> String {
        let mut out = String::from("# Status\n");
        for col in &self.columns {
            out.push('\n');
            out.push_str("## ");
            out.push_str(&col.name);
            out.push('\n');
            for t in &col.tasks {
                out.push_str("- ");
                out.push_str(t);
                out.push('\n');
            }
        }
        out
    }

    /// Whether `task` appears anywhere on the board.
    pub fn contains(&self, task: &str) -> bool {
        self.columns
            .iter()
            .any(|c| c.tasks.iter().any(|t| t == task))
    }

    /// Column index and position of `task`, if present.
    pub fn locate(&self, task: &str) -> Option<(usize, usize)> {
        self.columns
            .iter()
            .enumerate()
            .find_map(|(ci, c)| c.tasks.iter().position(|t| t == task).map(|ti| (ci, ti)))
    }

    /// Make the board agree with the folders that actually exist:
    /// unknown tasks are appended to the first column and tasks whose folder is
    /// gone are dropped. Returns `true` if anything changed.
    pub fn reconcile(&mut self, existing: &[String]) -> bool {
        let mut changed = false;
        for col in &mut self.columns {
            let before = col.tasks.len();
            col.tasks.retain(|t| existing.contains(t));
            changed |= col.tasks.len() != before;
        }
        if self.columns.is_empty() {
            return changed;
        }
        for task in existing {
            if !self.contains(task) {
                changed = true;
                self.columns[0].tasks.push(task.clone());
            }
        }
        changed
    }

    /// Move `task` to column `to`, appending it at the end. Returns `false` if the
    /// task is unknown or `to` is out of range.
    pub fn move_to(&mut self, task: &str, to: usize) -> bool {
        if to >= self.columns.len() {
            return false;
        }
        let Some((ci, ti)) = self.locate(task) else {
            return false;
        };
        if ci == to {
            return true;
        }
        let t = self.columns[ci].tasks.remove(ti);
        self.columns[to].tasks.push(t);
        true
    }

    /// Move `task` up (`delta < 0`) or down within its column. Returns whether it moved.
    pub fn shift(&mut self, task: &str, delta: i32) -> bool {
        let Some((ci, ti)) = self.locate(task) else {
            return false;
        };
        let len = self.columns[ci].tasks.len() as i64;
        let target = ti as i64 + i64::from(delta);
        if target < 0 || target >= len || target == ti as i64 {
            return false;
        }
        let t = self.columns[ci].tasks.remove(ti);
        self.columns[ci].tasks.insert(target as usize, t);
        true
    }

    /// Add a new task to column `to` (defaults to the first column when out of range).
    pub fn add(&mut self, task: &str, to: usize) {
        if self.contains(task) || self.columns.is_empty() {
            return;
        }
        let idx = if to < self.columns.len() { to } else { 0 };
        self.columns[idx].tasks.push(task.to_owned());
    }

    /// Total number of tasks.
    pub fn len(&self) -> usize {
        self.columns.iter().map(|c| c.tasks.len()).sum()
    }

    /// Whether the board holds no tasks.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn dedup(&mut self) {
        let mut seen = std::collections::HashSet::new();
        for col in &mut self.columns {
            col.tasks.retain(|t| seen.insert(t.clone()));
        }
    }
}

fn heading_text(line: &str) -> Option<&str> {
    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    if hashes == 0 {
        return None;
    }
    let rest = &line[hashes..];
    if !rest.starts_with(' ') && !rest.is_empty() {
        return None;
    }
    Some(rest.trim())
}

fn list_item_text(line: &str) -> Option<String> {
    let text = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))
        .unwrap_or(line);
    let text = text
        .strip_prefix("[ ] ")
        .or_else(|| text.strip_prefix("[x] "))
        .or_else(|| text.strip_prefix("[X] "))
        .unwrap_or(text);
    let text = text.trim().trim_matches('`');
    if text.is_empty() || text.starts_with("<!--") {
        None
    } else {
        Some(text.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATS: [&str; 3] = ["Planned", "Doing", "Done"];

    #[test]
    fn parses_headings_and_items() {
        let md = "# Status\n\n## Planned\n- a\n* b\n\n## doing\n- [ ] c\n\n## Done\n";
        let b = Board::parse(md, &CATS);
        assert_eq!(b.columns[0].tasks, vec!["a", "b"]);
        assert_eq!(b.columns[1].tasks, vec!["c"]);
        assert!(b.columns[2].tasks.is_empty());
    }

    #[test]
    fn bare_names_and_unknown_headings_are_kept() {
        let md = "## Planned\nalpha\n## Someday\n- beta\n";
        let b = Board::parse(md, &CATS);
        assert_eq!(b.columns[0].tasks, vec!["alpha", "beta"]);
    }

    #[test]
    fn render_roundtrips() {
        let mut b = Board::new(&CATS);
        b.add("x", 0);
        b.add("y", 1);
        let again = Board::parse(&b.render(), &CATS);
        assert_eq!(again, b);
    }

    #[test]
    fn reconcile_adds_and_removes() {
        let mut b = Board::parse("## Doing\n- gone\n- keep\n", &CATS);
        let changed = b.reconcile(&["keep".into(), "new".into()]);
        assert!(changed);
        assert_eq!(b.columns[1].tasks, vec!["keep"]);
        assert_eq!(b.columns[0].tasks, vec!["new"]);
        assert!(!b.reconcile(&["keep".into(), "new".into()]));
    }

    #[test]
    fn move_between_columns() {
        let mut b = Board::parse("## Planned\n- t\n", &CATS);
        assert!(b.move_to("t", 2));
        assert_eq!(b.locate("t"), Some((2, 0)));
        assert!(!b.move_to("t", 9));
        assert!(!b.move_to("nope", 0));
    }

    #[test]
    fn shift_within_column() {
        let mut b = Board::parse("## Planned\n- a\n- b\n- c\n", &CATS);
        assert!(b.shift("c", -1));
        assert_eq!(b.columns[0].tasks, vec!["a", "c", "b"]);
        assert!(!b.shift("a", -1));
        assert!(b.shift("a", 2));
        assert_eq!(b.columns[0].tasks, vec!["c", "b", "a"]);
        assert!(!b.shift("zz", 1));
    }

    #[test]
    fn duplicates_are_collapsed() {
        let b = Board::parse("## Planned\n- t\n## Done\n- t\n", &CATS);
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn empty_categories_yield_empty_board() {
        let b = Board::parse("## Planned\n- t\n", &Vec::<String>::new());
        assert!(b.columns.is_empty());
    }
}
