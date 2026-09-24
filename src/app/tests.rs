//! Behavioural tests driving the app with synthetic events.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::config::{ShellConfig, TaskSource, VendorBuild, Workspace};

use super::*;

struct Harness {
    app: App,
    rx: Receiver<AppEvent>,
    _dir: tempfile::TempDir,
    root: PathBuf,
    tasks: PathBuf,
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn alt(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)
}

fn git(path: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn make_repo(path: &Path) {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q", "-b", "main"]);
    git(path, &["config", "user.email", "t@example.com"]);
    git(path, &["config", "user.name", "t"]);
    fs::write(path.join("a.txt"), "a").unwrap();
    git(path, &["add", "-A"]);
    git(path, &["commit", "-q", "-m", "init"]);
}

impl Harness {
    fn build(with_config: bool, customise: impl FnOnce(&mut Config)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let tasks = root.join("tasks");
        fs::create_dir_all(tasks.join("alpha/scripts")).unwrap();
        fs::write(tasks.join("alpha/CONTEXT.md"), "# alpha\n").unwrap();
        fs::write(tasks.join("alpha/scripts/run.sh"), "echo hi\n").unwrap();
        fs::create_dir_all(tasks.join("beta")).unwrap();
        fs::write(tasks.join("status.md"), "## Doing\n- beta\n").unwrap();
        let config_path = root.join("config.toml");
        let config = with_config.then(|| {
            let mut cfg = Config {
                tasks_dir: tasks.clone(),
                shell: ShellConfig {
                    program: "bash".into(),
                    args: vec!["--norc".into(), "-i".into()],
                },
                ..Config::default()
            };
            customise(&mut cfg);
            cfg
        });
        let (tx, rx) = EventSender::channel();
        let state = root.join("state");
        let app = App::new(config, config_path, state, tx);
        Self {
            app,
            rx,
            _dir: dir,
            root,
            tasks,
        }
    }

    fn new(with_config: bool) -> Self {
        Self::build(with_config, |_| {})
    }

    fn press(&mut self, k: KeyEvent) {
        self.app.handle(AppEvent::Input(Event::Key(k)));
    }

    fn type_str(&mut self, s: &str) {
        for c in s.chars() {
            self.press(key(KeyCode::Char(c)));
        }
    }

    fn ctx(&self) -> &TaskContext {
        self.app.active_context().expect("active task")
    }

    fn context_md(&self, id: &str) -> String {
        fs::read_to_string(self.tasks.join(id).join("CONTEXT.md")).unwrap()
    }

    /// Pump background events until `pred` holds or the timeout elapses.
    fn pump_until(&mut self, pred: impl Fn(&App) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if pred(&self.app) {
                return true;
            }
            if let Ok(ev) = self.rx.recv_timeout(Duration::from_millis(50)) {
                self.app.handle(ev);
            }
        }
        pred(&self.app)
    }
}

fn click(col: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: col,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn wheel(col: u16, row: u16, down: bool) -> MouseEvent {
    let kind = if down {
        MouseEventKind::ScrollDown
    } else {
        MouseEventKind::ScrollUp
    };
    MouseEvent {
        kind,
        column: col,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

impl Harness {
    fn mouse(&mut self, m: MouseEvent) {
        self.app.handle(AppEvent::Input(Event::Mouse(m)));
    }
}

#[test]
fn mouse_in_task_list_selects_and_double_click_opens() {
    let mut h = Harness::new(true);
    // Pretend the list was drawn at the top-left; rows: PLANNED, alpha, DOING, beta.
    h.app.ui.task_list = Rect::new(0, 0, 22, 20);
    h.mouse(click(3, 4));
    assert_eq!(selected_task(&h.app).as_deref(), Some("beta"));
    h.mouse(click(3, 1)); // header row: selection unchanged
    assert_eq!(selected_task(&h.app).as_deref(), Some("beta"));
    h.mouse(wheel(3, 3, false));
    assert_eq!(selected_task(&h.app).as_deref(), Some("alpha"));
    h.mouse(click(3, 4));
    h.mouse(click(3, 4));
    assert!(matches!(h.app.mode(), Mode::Task));
    assert_eq!(h.app.active_task(), Some("beta"));
    h.mouse(click(50, 50)); // outside everything: ignored
}

#[test]
fn mouse_in_task_view_focuses_selects_and_scrolls() {
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Enter));
    h.app.ui.tree = Rect::new(0, 1, 22, 10);
    h.app.ui.shells = Rect::new(0, 11, 22, 8);
    h.app.ui.editor = Rect::new(24, 1, 60, 10);
    h.app.ui.editor_gutter = 3;
    // Double-click the folder expands it; then double-click the file opens it.
    h.mouse(click(3, 2));
    h.mouse(click(3, 2));
    assert!(h.ctx().tree.nodes().iter().any(|n| n.name == "run.sh"));
    h.mouse(click(3, 3));
    assert_eq!(h.ctx().tree.selected().unwrap().name, "run.sh");
    h.mouse(click(3, 3));
    assert!(h.ctx().editor.as_ref().unwrap().path().ends_with("run.sh"));
    assert_eq!(h.ctx().focus, Focus::Editor);
    // Click inside the editor places the cursor.
    h.mouse(click(24 + 3 + 4, 1));
    assert_eq!(h.ctx().editor.as_ref().unwrap().cursor(), (0, 4));
    h.mouse(click(24 + 3 + 40, 1));
    assert_eq!(h.ctx().editor.as_ref().unwrap().cursor(), (0, 7));
    // Wheel over the tree moves its selection and does not steal focus.
    h.mouse(wheel(3, 3, false));
    assert_eq!(h.ctx().tree.selected().unwrap().name, "scripts");
    assert_eq!(h.ctx().focus, Focus::Editor);
    // Click on the shells pane focuses it; a shell then appears in the terminal rect.
    h.mouse(click(3, 12));
    assert_eq!(h.ctx().focus, Focus::Shells);
    h.press(key(KeyCode::Char('n')));
    assert_eq!(h.ctx().focus, Focus::Terminal);
    h.app.ui.terminal = Rect::new(24, 12, 60, 8);
    h.mouse(click(3, 2));
    assert_eq!(h.ctx().focus, Focus::Tree);
    h.mouse(click(30, 14));
    assert_eq!(h.ctx().focus, Focus::Terminal);
    // Wheel over the terminal without a mouse-aware program scrolls the pane, harmlessly.
    h.mouse(wheel(30, 14, true));
    h.app.shutdown();
}

#[test]
fn mouse_on_settings_page_selects_rows() {
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Char(',')));
    h.app.ui.config_rows = Rect::new(1, 1, 80, 20);
    h.mouse(click(5, 3));
    assert!(matches!(h.app.mode(), Mode::Config(f) if f.selected() == 2));
    h.mouse(wheel(5, 3, true));
    assert!(matches!(h.app.mode(), Mode::Config(f) if f.selected() == 3));
}

fn selected_task(app: &App) -> Option<String> {
    match app.rows().get(app.list_selected()) {
        Some(ListRow::Task(_, id)) => Some(id.clone()),
        _ => None,
    }
}

#[test]
fn first_run_opens_config_and_saving_opens_board() {
    let mut h = Harness::new(false);
    assert!(matches!(h.app.mode(), Mode::Config(f) if f.first_run()));
    h.press(key(KeyCode::Esc));
    assert!(matches!(h.app.mode(), Mode::Config(_)));
    h.press(ctrl('s'));
    assert!(matches!(h.app.mode(), Mode::Config(f) if !f.errors().is_empty()));
    h.press(key(KeyCode::Enter));
    let path = h.tasks.display().to_string();
    h.type_str(&path);
    h.press(key(KeyCode::Enter));
    h.press(ctrl('s'));
    assert!(matches!(h.app.mode(), Mode::Config(f) if f.errors().is_empty() && !f.first_run()));
    assert!(h.app.store().is_some());
    h.press(key(KeyCode::Esc));
    assert!(matches!(h.app.mode(), Mode::TaskList));
    assert!(h.app.config_path().is_file());
}

#[test]
fn list_navigation_and_moving_tasks() {
    let mut h = Harness::new(true);
    assert!(matches!(h.app.mode(), Mode::TaskList));
    assert_eq!(selected_task(&h.app).as_deref(), Some("alpha"));
    h.press(key(KeyCode::Down));
    assert_eq!(selected_task(&h.app).as_deref(), Some("beta"));
    h.press(key(KeyCode::Char(']')));
    assert_eq!(h.app.store().unwrap().board().locate("beta"), Some((2, 0)));
    assert_eq!(selected_task(&h.app).as_deref(), Some("beta"));
    let status = fs::read_to_string(h.tasks.join("status.md")).unwrap();
    assert!(status.contains("## Done\n- beta"));
    // Moving through the palette works from inside a task too.
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('[')));
    assert_eq!(h.app.store().unwrap().board().locate("beta"), Some((1, 0)));
}

#[test]
fn escape_palette_commands() {
    let mut h = Harness::new(true);
    // From the list: Esc → c opens configuration, Esc leaves it again.
    h.press(key(KeyCode::Esc));
    assert!(matches!(h.app.popup(), Some(Popup::Palette(_))));
    h.press(key(KeyCode::Char('c')));
    assert!(matches!(h.app.mode(), Mode::Config(_)));
    h.press(key(KeyCode::Esc));
    assert!(matches!(h.app.mode(), Mode::TaskList));
    // Inside a task: Esc → c → Esc comes back to the task; Esc → T returns to the list.
    h.press(key(KeyCode::Enter));
    assert!(matches!(h.app.mode(), Mode::Task));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('c')));
    assert!(matches!(h.app.mode(), Mode::Config(_)));
    h.press(key(KeyCode::Esc));
    assert!(matches!(h.app.mode(), Mode::Task));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('T')));
    assert!(matches!(h.app.mode(), Mode::TaskList));
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char(':')));
    h.type_str("task list");
    h.press(key(KeyCode::Enter));
    assert!(matches!(h.app.mode(), Mode::TaskList));
    // Esc → q quits.
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('q')));
    assert!(h.app.should_quit());
}

#[test]
fn create_custom_task_via_palette() {
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('n')));
    assert!(matches!(h.app.popup(), Some(Popup::Input { .. })));
    h.type_str("gamma");
    h.press(key(KeyCode::Enter));
    assert!(h.app.popup().is_none());
    assert!(h.tasks.join("gamma/CONTEXT.md").is_file());
    assert_eq!(selected_task(&h.app).as_deref(), Some("gamma"));
    // Ids with spaces are rejected.
    h.press(key(KeyCode::Char('n')));
    h.type_str("bad id");
    h.press(key(KeyCode::Enter));
    assert!(matches!(h.app.popup(), Some(Popup::Message { title, .. }) if title == "Error"));
}

#[test]
fn create_task_from_ticket_source() {
    let script = r#"printf '%s' '[{"id":"PROJ-7","title":"Fix login","url":"https://jira/PROJ-7","description":"Users cannot log in."},{"id":"ORB 12","title":"Orbit thing","url":"https://orbit/12","description":"Line 1\nLine 2"}]'"#;
    let mut h = Harness::build(true, |cfg| {
        cfg.task_sources = vec![
            TaskSource {
                name: "jira".into(),
                command: script.into(),
            },
            TaskSource {
                name: "broken".into(),
                command: "echo nope >&2; exit 2".into(),
            },
        ];
    });
    h.press(key(KeyCode::Char('n')));
    assert!(matches!(h.app.popup(), Some(Popup::Choose { choices, .. }) if choices.len() == 3));
    h.press(key(KeyCode::Char('2'))); // from jira
    assert!(matches!(
        h.app.popup(),
        Some(Popup::Log { done: false, .. })
    ));
    assert!(h.pump_until(|app| matches!(app.popup(), Some(Popup::Tickets { .. }))));
    // Filter to the orbit one and create it.
    h.type_str("orbit");
    h.press(key(KeyCode::Enter));
    assert!(h.app.popup().is_none(), "{:?}", h.app.popup());
    let md = h.context_md("ORB-12");
    assert!(
        md.starts_with("# ORB-12: Orbit thing\n\nSource: jira\nLink: https://orbit/12\n"),
        "{md}"
    );
    assert!(md.contains("## Description\n\nLine 1\nLine 2\n"));
    assert!(md.contains("- link: https://orbit/12"));
    assert_eq!(selected_task(&h.app).as_deref(), Some("ORB-12"));

    // A failing script reports its stderr.
    h.press(key(KeyCode::Char('n')));
    h.press(key(KeyCode::Char('3')));
    assert!(h.pump_until(|app| matches!(app.popup(), Some(Popup::Message { .. }))));
    assert!(matches!(h.app.popup(), Some(Popup::Message { body, .. }) if body.contains("nope")));
}

#[test]
fn attach_and_prepare_workspaces() {
    let fw = tempfile::tempdir().unwrap();
    make_repo(fw.path());
    let build_dir = tempfile::tempdir().unwrap();
    let mut h = Harness::build(true, |cfg| {
        cfg.workspaces = vec![Workspace {
            name: "fw".into(),
            path: fw.path().to_path_buf(),
            main_branch: None,
        }];
        cfg.builds = vec![VendorBuild {
            name: "yocto".into(),
            path: build_dir.path().to_path_buf(),
        }];
    });
    // Prepare before attaching: refused.
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('p')));
    assert!(
        matches!(h.app.popup(), Some(Popup::Message { body, .. }) if body.contains("no code workspace"))
    );
    h.press(key(KeyCode::Enter));

    // Attach both.
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('a')));
    assert!(matches!(h.app.popup(), Some(Popup::MultiSelect { items, .. }) if items.len() == 2));
    h.press(key(KeyCode::Char(' ')));
    h.press(key(KeyCode::Down));
    h.press(key(KeyCode::Char(' ')));
    h.press(key(KeyCode::Enter));
    let md = h.context_md("alpha");
    assert!(md.contains("- workspace: fw\n- build: yocto\n"), "{md}");
    assert_eq!(h.ctx().meta.workspaces, vec!["fw"]);
    let env_file = h.ctx().env_file.clone().unwrap();
    let env = fs::read_to_string(&env_file).unwrap();
    assert!(env.contains(&format!("PAHIRI_CODE_DIR='{}'", fw.path().display())));
    assert!(env.contains("PAHIRI_BUILD_DIRS='yocto="));

    // Prepare on a clean main: creates the task branch.
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('p')));
    assert!(matches!(h.app.popup(), Some(Popup::Log { .. })));
    assert!(h.pump_until(|app| matches!(app.popup(), Some(Popup::Log { done: true, .. }))));
    assert_eq!(
        git(fw.path(), &["rev-parse", "--abbrev-ref", "HEAD"]),
        "alpha"
    );
    let md = h.context_md("alpha");
    assert!(md.contains("- branch: alpha\n- prepared: "), "{md}");
    assert!(h.ctx().attachment_summary().contains("branch: alpha"));
    h.press(key(KeyCode::Enter)); // close log

    // Dirty on main: warned, choose to commit there and continue into the other task.
    git(fw.path(), &["checkout", "-q", "main"]);
    fs::write(fw.path().join("wip.txt"), "wip").unwrap();
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('T')));
    h.press(key(KeyCode::Down));
    h.press(key(KeyCode::Enter)); // beta
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('a')));
    h.press(key(KeyCode::Char(' ')));
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('p')));
    assert!(
        matches!(h.app.popup(), Some(Popup::Choose { title, .. }) if title.starts_with("WARNING"))
    );
    h.press(key(KeyCode::Char('1')));
    assert!(h.pump_until(|app| matches!(app.popup(), Some(Popup::Log { done: true, .. }))));
    assert_eq!(
        git(fw.path(), &["rev-parse", "--abbrev-ref", "HEAD"]),
        "beta"
    );
    assert_eq!(
        git(fw.path(), &["log", "-1", "--format=%s", "main"]),
        "pahiri: state saved on main before switching to beta"
    );
    assert_eq!(git(fw.path(), &["status", "--porcelain"]), "");
}

#[test]
fn pane_cycling_and_shell_selection_keys() {
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Enter));
    assert_eq!(h.ctx().focus, Focus::Tree);
    h.press(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL));
    assert_eq!(h.ctx().focus, Focus::Shells);
    h.press(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL));
    assert_eq!(h.ctx().focus, Focus::Tree); // no editor, no shells yet
    h.press(alt(']'));
    assert_eq!(h.ctx().focus, Focus::Shells);
    h.press(KeyEvent::new(
        KeyCode::BackTab,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    assert_eq!(h.ctx().focus, Focus::Tree);

    // Two shells; Ctrl+1 / Alt+2 pick them from anywhere, even from the terminal.
    h.press(key(KeyCode::Char('t')));
    h.press(key(KeyCode::Esc)); // typed into the shell; the palette needs leader Esc
    h.press(ctrl('b'));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('s')));
    assert_eq!(h.ctx().shells.len(), 2);
    assert_eq!(h.ctx().focus, Focus::Terminal);
    h.press(ctrl('1'));
    assert_eq!(h.ctx().selected_shell, 0);
    h.press(alt('2'));
    assert_eq!(h.ctx().selected_shell, 1);
    h.press(ctrl('3'));
    assert_eq!(h.ctx().selected_shell, 1);
    // Ctrl+Tab from the terminal goes on to the files pane.
    h.press(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL));
    assert_eq!(h.ctx().focus, Focus::Tree);
    h.press(ctrl('2'));
    assert_eq!(h.ctx().focus, Focus::Terminal);
    h.app.shutdown();
}

#[test]
fn enter_task_browse_files_and_edit() {
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Enter));
    assert!(matches!(h.app.mode(), Mode::Task));
    assert_eq!(h.app.active_task(), Some("alpha"));
    let names: Vec<String> = h
        .ctx()
        .tree
        .nodes()
        .iter()
        .map(|n| n.name.clone())
        .collect();
    assert_eq!(names, vec!["scripts", "CONTEXT.md"]);
    h.press(key(KeyCode::Enter));
    assert!(h.ctx().tree.nodes().iter().any(|n| n.name == "run.sh"));
    h.press(key(KeyCode::Down));
    h.press(key(KeyCode::Enter));
    assert_eq!(h.ctx().focus, Focus::Editor);
    assert!(h.ctx().editor.as_ref().unwrap().path().ends_with("run.sh"));
    h.press(key(KeyCode::End));
    h.type_str("# edited");
    h.press(ctrl('s'));
    assert_eq!(
        fs::read_to_string(h.tasks.join("alpha/scripts/run.sh")).unwrap(),
        "echo hi# edited\n"
    );
    // Esc from the editor opens the palette; W closes the buffer.
    h.press(key(KeyCode::Esc));
    assert!(matches!(h.app.popup(), Some(Popup::Palette(_))));
    h.press(key(KeyCode::Char('W')));
    assert!(h.ctx().editor.is_none());
    // o opens CONTEXT.md.
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('o')));
    assert!(h
        .ctx()
        .editor
        .as_ref()
        .unwrap()
        .path()
        .ends_with("CONTEXT.md"));
}

#[test]
fn file_operations_from_tree() {
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Down));
    h.press(key(KeyCode::Char('a')));
    h.type_str("notes.md");
    h.press(key(KeyCode::Enter));
    assert!(h.tasks.join("alpha/notes.md").is_file());
    h.press(key(KeyCode::Char('A')));
    h.type_str("data");
    h.press(key(KeyCode::Enter));
    assert!(h.tasks.join("alpha/data").is_dir());
    h.press(key(KeyCode::Char('r')));
    h.press(ctrl('u'));
    h.type_str("assets");
    h.press(key(KeyCode::Enter));
    assert!(h.tasks.join("alpha/assets").is_dir());
    h.press(key(KeyCode::Char('d')));
    assert!(matches!(h.app.popup(), Some(Popup::Confirm { .. })));
    h.press(key(KeyCode::Char('n')));
    assert!(h.tasks.join("alpha/assets").is_dir());
    h.press(key(KeyCode::Char('d')));
    h.press(key(KeyCode::Char('y')));
    assert!(!h.tasks.join("alpha/assets").exists());
}

#[test]
fn large_and_binary_files_warn_before_opening() {
    let mut h = Harness::new(true);
    fs::write(h.tasks.join("alpha/blob.bin"), [0u8, 159, 146, 150]).unwrap();
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Char('R')));
    let idx = h
        .ctx()
        .tree
        .nodes()
        .iter()
        .position(|n| n.name == "blob.bin")
        .unwrap();
    for _ in 0..idx {
        h.press(key(KeyCode::Down));
    }
    h.press(key(KeyCode::Enter));
    assert!(matches!(h.app.popup(), Some(Popup::Confirm { title, .. }) if title == "WARNING"));
    h.press(key(KeyCode::Esc));
    assert!(h.ctx().editor.is_none());
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Char('y')));
    assert!(h.ctx().editor.as_ref().unwrap().is_read_only());
}

#[test]
fn shells_run_in_task_folder_with_cd_wrapper_and_leader() {
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Char('t')));
    assert_eq!(h.ctx().focus, Focus::Terminal);
    h.type_str("cd /; cd task; printf 'cwd=%s task=%s' \"$PWD\" \"$PAHIRI_TASK\"");
    h.press(key(KeyCode::Enter));
    assert!(h.pump_until(|app| {
        app.active_context()
            .and_then(TaskContext::active_shell)
            .is_some_and(|s| s.session.screen().contents().contains("task=alpha"))
    }));
    let contents = h.ctx().active_shell().unwrap().session.screen().contents();
    let canon = fs::canonicalize(h.tasks.join("alpha")).unwrap();
    assert!(
        contents.contains(&format!("cwd={}", canon.display())),
        "{contents}"
    );

    // Leader Esc opens the palette from inside the terminal.
    h.press(ctrl('b'));
    h.press(key(KeyCode::Esc));
    assert!(matches!(h.app.popup(), Some(Popup::Palette(_))));
    h.press(key(KeyCode::Esc));
    assert_eq!(h.ctx().focus, Focus::Terminal);
    // Leader q leaves, leader z zooms.
    h.press(ctrl('b'));
    h.press(key(KeyCode::Char('q')));
    assert_eq!(h.ctx().focus, Focus::Shells);
    h.press(key(KeyCode::Enter));
    h.press(ctrl('b'));
    h.press(key(KeyCode::Char('z')));
    assert!(h.ctx().zoomed);
    // Exiting the shell marks it; a key press then removes it.
    h.type_str("exit");
    h.press(key(KeyCode::Enter));
    assert!(h.pump_until(|app| {
        app.active_context()
            .and_then(TaskContext::active_shell)
            .is_some_and(|s| s.session.has_exited())
    }));
    h.press(key(KeyCode::Enter));
    assert!(h.ctx().shells.is_empty());
    assert_eq!(h.ctx().focus, Focus::Shells);
    let _ = h.root.clone();
}

#[test]
fn switching_tasks_keeps_each_context() {
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Down));
    h.press(key(KeyCode::Enter));
    assert!(h.ctx().editor.is_some());
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('T')));
    h.press(key(KeyCode::Down));
    h.press(key(KeyCode::Enter));
    assert_eq!(h.app.active_task(), Some("beta"));
    assert!(h.ctx().editor.is_none());
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('T')));
    h.press(key(KeyCode::Up));
    h.press(key(KeyCode::Enter));
    assert!(h
        .ctx()
        .editor
        .as_ref()
        .unwrap()
        .path()
        .ends_with("CONTEXT.md"));
}

#[test]
fn quit_asks_when_shells_are_running() {
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Char('q')));
    assert!(h.app.should_quit());

    let mut h = Harness::new(true);
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Char('t')));
    h.press(ctrl('c')); // goes to the shell
    assert!(h.app.popup().is_none());
    h.press(ctrl('b'));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('q')));
    assert!(matches!(h.app.popup(), Some(Popup::Confirm { .. })));
    h.press(key(KeyCode::Char('n')));
    assert!(!h.app.should_quit());
    h.press(ctrl('b'));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('q')));
    h.press(key(KeyCode::Char('y')));
    assert!(h.app.should_quit());
    h.app.shutdown();
}

#[test]
fn helpers() {
    assert_eq!(human_size(10), "10 B");
    assert_eq!(human_size(2048), "2.0 KiB");
    assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
    assert_eq!(format_rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
    assert_eq!(format_rfc3339(1_709_164_800), "2024-02-29T00:00:00Z");
}

// ----- deleting, timer, checkpoints, agents, gerrit ---------------------------------

const PLAN: &str = "# alpha\n\n## Checkpoints\n<!-- pahiri:checkpoints -->\n- [ ] Read the spec (20m)\n- [ ] Write the code (1h)\n<!-- /pahiri:checkpoints -->\n";

fn popup_title(app: &App) -> String {
    app.popup()
        .map(|p| p.title().to_owned())
        .unwrap_or_default()
}

#[test]
fn delete_task_moves_it_to_trash() {
    let mut h = Harness::new(true);
    assert_eq!(selected_task(&h.app).as_deref(), Some("alpha"));
    h.press(key(KeyCode::Char('d')));
    assert!(popup_title(&h.app).starts_with("Delete task"));
    h.press(key(KeyCode::Char('n')));
    assert!(h.tasks.join("alpha").is_dir());
    h.press(key(KeyCode::Char('d')));
    h.press(key(KeyCode::Char('y')));
    assert!(!h.tasks.join("alpha").exists());
    assert!(fs::read_dir(h.tasks.join(".trash")).unwrap().count() == 1);
    assert_eq!(selected_task(&h.app).as_deref(), Some("beta"));
    assert!(h.app.store().unwrap().board().locate("alpha").is_none());

    // Inside a task with a running shell: refused; after closing it, deleted
    // and back on the list.
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Char('t')));
    h.press(ctrl('b'));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('D')));
    assert!(
        matches!(h.app.popup(), Some(Popup::Message { body, .. }) if body.contains("running shell"))
    );
    h.press(key(KeyCode::Enter));
    h.app.shutdown();
    let ctx = h.app.contexts.get_mut("beta").unwrap();
    ctx.shells.clear();
    ctx.focus = Focus::Tree;
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('D')));
    h.press(key(KeyCode::Char('y')));
    assert!(matches!(h.app.mode(), Mode::TaskList));
    assert!(!h.tasks.join("beta").exists());
}

#[test]
fn timer_runs_checkpoints_books_time_and_alarms() {
    let mut h = Harness::build(true, |cfg| {
        cfg.categories = vec!["Planned".into(), "Doing".into(), "Done".into()];
    });
    fs::write(h.tasks.join("alpha/CONTEXT.md"), PLAN).unwrap();
    h.app.refresh_next_up();
    assert_eq!(h.app.next_up().len(), 2);
    // Start on the first open checkpoint from the list; the task moves to Doing.
    h.press(key(KeyCode::Char('m')));
    let t = h.app.timer().expect("timer");
    assert_eq!(t.checkpoint.as_deref(), Some("Read the spec"));
    assert_eq!(t.budget, Duration::from_secs(20 * 60));
    assert_eq!(
        h.app.store().unwrap().board().locate("alpha").map(|l| l.0),
        Some(1)
    );
    assert!(h.context_md("alpha").contains("- started: "));

    // 7 minutes pass, pause books whole minutes to the checkpoint and the task.
    h.app
        .timer
        .as_mut()
        .unwrap()
        .backdate(Duration::from_secs(7 * 60 + 20));
    h.press(key(KeyCode::Char('m')));
    assert!(!h.app.timer().unwrap().is_running());
    let md = h.context_md("alpha");
    assert!(md.contains("- [ ] Read the spec (20m; spent 7m)"), "{md}");
    assert!(md.contains("- time_spent: 7m"), "{md}");
    h.press(key(KeyCode::Char('m')));
    assert!(h.app.timer().unwrap().is_running());

    // Time runs out: flash, bell, and the time's-up choice.
    h.app
        .timer
        .as_mut()
        .unwrap()
        .backdate(Duration::from_secs(13 * 60));
    h.app.handle(AppEvent::Tick);
    assert!(
        popup_title(&h.app).starts_with("Time's up"),
        "{:?}",
        h.app.popup()
    );
    assert!(h.app.take_bell());
    assert!(h.app.flash_on());
    assert!(h.app.tick_interval() < Duration::from_millis(250));
    // "done — start next".
    h.press(key(KeyCode::Char('1')));
    let md = h.context_md("alpha");
    assert!(md.contains("- [x] Read the spec (20m; spent 20m)"), "{md}");
    assert!(
        md.contains("✓ Read the spec (spent 20m, estimated 20m)"),
        "{md}"
    );
    assert_eq!(
        h.app.timer().unwrap().checkpoint.as_deref(),
        Some("Write the code")
    );

    // v ticks it and, being the last, stops; M with nothing running just says so.
    h.press(key(KeyCode::Char('v')));
    assert!(h.app.timer().is_none());
    assert!(h.context_md("alpha").contains("- [x] Write the code (1h)"));
    h.press(key(KeyCode::Char('M')));

    // Moving to the last column records `finished` and asks for an outcome.
    h.press(key(KeyCode::Char(']')));
    assert!(matches!(h.app.popup(), Some(Popup::Input { .. })));
    h.type_str("Shipped the parser");
    h.press(key(KeyCode::Enter));
    let md = h.context_md("alpha");
    assert!(md.contains("- finished: "), "{md}");
    assert!(md.contains("## Outcome\n- "), "{md}");
    assert!(md.contains("Shipped the parser"));
    // Moving back clears `finished`.
    h.press(key(KeyCode::Char('[')));
    assert!(!h.context_md("alpha").contains("- finished: "));
}

#[test]
fn focus_block_without_checkpoints_and_checkpoint_popup() {
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('m')));
    assert!(matches!(h.app.popup(), Some(Popup::Input { value, .. }) if value == "25"));
    h.press(ctrl('u'));
    h.type_str("10");
    h.press(key(KeyCode::Enter));
    assert_eq!(h.app.timer().unwrap().budget, Duration::from_secs(600));
    h.app
        .timer
        .as_mut()
        .unwrap()
        .backdate(Duration::from_secs(4 * 60));
    h.app.shutdown(); // books the time on exit
    assert!(h.app.timer().is_none());
    let md = h.context_md("alpha");
    assert!(md.contains("⏱ 4m focus block"), "{md}");
    assert!(md.contains("- time_spent: 4m"));

    // The checkpoint popup ticks items and starts the timer on the selected one.
    fs::write(h.tasks.join("alpha/CONTEXT.md"), PLAN).unwrap();
    h.app.handle(AppEvent::Tick); // picks up the outside edit
    assert_eq!(h.ctx().checkpoints.len(), 2);
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('k')));
    assert!(matches!(h.app.popup(), Some(Popup::Checkpoints { .. })));
    h.press(key(KeyCode::Char(' ')));
    assert!(h.context_md("alpha").contains("- [x] Read the spec (20m)"));
    h.press(key(KeyCode::Down));
    h.press(key(KeyCode::Enter));
    assert_eq!(
        h.app.timer().unwrap().checkpoint.as_deref(),
        Some("Write the code")
    );
    assert!(h.ctx().plan_summary().contains("Write the code"));
}

fn fake_agent(cfg: &mut Config, script: &str) {
    cfg.agent.command = "sh".into();
    cfg.agent.args = vec![
        "-c".into(),
        format!("cat > \"$PAHIRI_TASK_DIR/prompt.txt\"; {script}"),
    ];
}

#[test]
fn agent_writes_context_and_checkpoints() {
    let mut h = Harness::build(true, |cfg| {
        fake_agent(
            cfg,
            r#"if grep -q 'checklist only' "$PAHIRI_TASK_DIR/prompt.txt"; then printf -- '- [ ] Reproduce (20m)\n- [ ] Fix (45m)\n'; else printf '## Goal\n- make it work\n\nCONTEXT_READY: yes\n'; fi"#,
        );
    });
    h.press(key(KeyCode::Enter));
    // Checkpoints need a ready context first.
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('b')));
    assert_eq!(popup_title(&h.app), "Context not ready yet");
    h.press(key(KeyCode::Enter));

    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('i')));
    assert!(h.pump_until(|app| matches!(app.popup(), Some(Popup::Log { done: true, .. }))));
    let md = h.context_md("alpha");
    assert!(md.contains("## Context\n<!-- pahiri:context -->\n### Goal\n- make it work\n<!-- /pahiri:context -->"), "{md}");
    assert!(md.contains("- context_ready: true"), "{md}");
    let prompt = fs::read_to_string(h.tasks.join("alpha/prompt.txt")).unwrap();
    assert!(prompt.contains("task alpha"), "{prompt}");
    assert!(prompt.contains("== OUTPUT FORMAT"), "{prompt}");
    assert!(h.ctx().meta.context_ready);
    h.press(key(KeyCode::Enter));

    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('b')));
    assert!(h.pump_until(|app| matches!(app.popup(), Some(Popup::Log { done: true, .. }))));
    assert!(h
        .context_md("alpha")
        .contains("- [ ] Reproduce (20m)\n- [ ] Fix (45m)\n"));
    assert_eq!(h.ctx().checkpoints.len(), 2);
    h.press(key(KeyCode::Enter));

    // With progress recorded, a new breakdown asks before replacing.
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('v')));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('b')));
    assert!(h.pump_until(|app| matches!(app.popup(), Some(Popup::Confirm { .. }))));
    h.press(key(KeyCode::Char('y')));
    assert!(h.context_md("alpha").contains("- [ ] Reproduce (20m)\n"));

    // Esc r toggles readiness; so does the CLI API.
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('r')));
    assert!(!h.ctx().meta.context_ready);
    crate::cli::task_ready(h.app.config(), "alpha", true).unwrap();
    h.app.handle(AppEvent::Tick);
    assert!(h.ctx().meta.context_ready);
}

#[test]
fn agent_failures_and_cancel_are_reported() {
    let mut h = Harness::build(true, |cfg| fake_agent(cfg, "sleep 5"));
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('i')));
    h.press(key(KeyCode::Esc)); // cancel
    assert!(h.pump_until(|app| matches!(app.popup(), Some(Popup::Log { done: true, lines, .. }) if lines.iter().any(|l| l.contains("cancelled")))));
    h.press(key(KeyCode::Enter));
    h.app.config.agent.command = String::new();
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('i')));
    assert!(
        matches!(h.app.popup(), Some(Popup::Message { body, .. }) if body.contains("No AI agent"))
    );
}

#[test]
fn gerrit_changes_are_found_and_recorded() {
    let fw = tempfile::tempdir().unwrap();
    make_repo(fw.path());
    git(fw.path(), &["switch", "-q", "-c", "alpha"]);
    fs::write(fw.path().join("b.txt"), "b").unwrap();
    git(fw.path(), &["add", "-A"]);
    let id = format!("I{}", "b".repeat(40));
    git(
        fw.path(),
        &["commit", "-q", "-m", &format!("Fix it\n\nChange-Id: {id}")],
    );
    git(
        fw.path(),
        &[
            "remote",
            "add",
            "origin",
            "ssh://me@review.example.com:29418/fw",
        ],
    );
    let mut h = Harness::build(true, |cfg| {
        cfg.workspaces = vec![Workspace {
            name: "fw".into(),
            path: fw.path().to_path_buf(),
            main_branch: None,
        }];
    });
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('a')));
    h.press(key(KeyCode::Char(' ')));
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('g')));
    assert!(h.pump_until(|app| matches!(app.popup(), Some(Popup::Log { done: true, .. }))));
    let md = h.context_md("alpha");
    assert!(
        md.contains(&format!(
            "- gerrit: fw {id} https://review.example.com/q/{id} :: Fix it"
        )),
        "{md}"
    );
    assert_eq!(h.ctx().meta.gerrit.len(), 1);
}

#[test]
fn coding_agent_starts_in_a_new_shell() {
    let mut h = Harness::build(true, |cfg| {
        cfg.coding_agent.command = "printf".into();
        cfg.coding_agent.args = vec!["'agent:%s\\n'".into()];
        cfg.coding_agent.start_in_code = false;
    });
    // Leader works from the files pane too: leader a.
    h.press(key(KeyCode::Enter));
    h.press(ctrl('b'));
    h.press(key(KeyCode::Char('a')));
    assert_eq!(h.ctx().shells.len(), 1);
    assert!(h.pump_until(|app| {
        app.active_context()
            .and_then(TaskContext::active_shell)
            .is_some_and(|s| {
                s.session
                    .screen()
                    .contents()
                    .contains("helping with task alpha")
            })
    }));
    let prompt = h.root.join("state/prompts/alpha.coding-agent.md");
    assert!(fs::read_to_string(prompt).unwrap().contains("task alpha"));
    h.app.shutdown();
}

#[test]
fn settings_help_and_source_test_run() {
    let mut h = Harness::build(true, |cfg| {
        cfg.task_sources = vec![TaskSource {
            name: "jira".into(),
            command: "printf 'J-1\\tOne\\thttp://j/1\\n'".into(),
        }];
    });
    h.press(key(KeyCode::Char(',')));
    let Mode::Config(form) = &mut h.app.mode else {
        panic!()
    };
    let idx = form
        .rows()
        .iter()
        .position(|r| matches!(r, config_form::Row::Item { field, .. } if form.fields()[*field].key == config_form::FieldKey::TaskSources))
        .unwrap();
    form.select(idx);
    h.press(key(KeyCode::Char('?')));
    assert!(
        matches!(h.app.popup(), Some(Popup::Doc { lines, .. }) if lines.iter().any(|l| l.contains("JSON array")))
    );
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('t')));
    assert!(h.pump_until(|app| matches!(app.popup(), Some(Popup::Doc { .. }))));
    assert!(
        matches!(h.app.popup(), Some(Popup::Doc { lines, .. }) if lines.iter().any(|l| l.contains("J-1") && l.contains("One")))
    );
    assert!(!h.tasks.join("J-1").exists());
}
