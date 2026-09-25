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
                    tmux: false,
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
    assert!(matches!(h.app.mode(), Mode::Settings(f) if f.selected() == 2));
    h.mouse(wheel(5, 3, true));
    assert!(matches!(h.app.mode(), Mode::Settings(f) if f.selected() == 3));
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
    assert!(matches!(h.app.mode(), Mode::Settings(f) if f.first_run()));
    h.press(key(KeyCode::Esc));
    assert!(matches!(h.app.mode(), Mode::Settings(_)));
    h.press(ctrl('s'));
    assert!(matches!(h.app.mode(), Mode::Settings(f) if !f.errors().is_empty()));
    h.press(key(KeyCode::Enter));
    let path = h.tasks.display().to_string();
    h.type_str(&path);
    h.press(key(KeyCode::Enter));
    h.press(ctrl('s'));
    assert!(matches!(h.app.mode(), Mode::Settings(f) if f.errors().is_empty() && !f.first_run()));
    assert!(h.app.store().is_some());
    h.press(key(KeyCode::Esc));
    assert!(matches!(h.app.mode(), Mode::Home));
    assert!(h.app.config_path().is_file());
}

#[test]
fn list_navigation_and_moving_tasks() {
    let mut h = Harness::new(true);
    assert!(matches!(h.app.mode(), Mode::Home));
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
    assert!(matches!(h.app.mode(), Mode::Settings(_)));
    h.press(key(KeyCode::Esc));
    assert!(matches!(h.app.mode(), Mode::Home));
    // Inside a task: Esc → c → Esc comes back to the task; Esc → T returns to the list.
    h.press(key(KeyCode::Enter));
    assert!(matches!(h.app.mode(), Mode::Task));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('c')));
    assert!(matches!(h.app.mode(), Mode::Settings(_)));
    h.press(key(KeyCode::Esc));
    assert!(matches!(h.app.mode(), Mode::Task));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('T')));
    assert!(matches!(h.app.mode(), Mode::Home));
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char(':')));
    h.type_str("home");
    h.press(key(KeyCode::Enter));
    assert!(matches!(h.app.mode(), Mode::Home));
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
                file: None,
            },
            TaskSource {
                name: "broken".into(),
                command: "echo nope >&2; exit 2".into(),
                file: None,
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
    assert!(matches!(h.app.mode(), Mode::Home));
    assert!(!h.tasks.join("beta").exists());
}

impl Harness {
    /// Let the next tick look for outside changes (normally every ~1 s).
    fn watch(&mut self) {
        self.app.watched = Instant::now().checked_sub(Duration::from_secs(5)).unwrap();
        self.app.handle(AppEvent::Tick);
    }

    /// Quit and start a new app with the same config and state folder.
    fn restart(&mut self) {
        self.app.shutdown();
        let (tx, rx) = EventSender::channel();
        let cfg = self.app.config().clone();
        self.app = App::new(
            Some(cfg),
            self.root.join("config.toml"),
            self.root.join("state"),
            tx,
        );
        self.rx = rx;
    }

    fn choice_index(&self, prefix: &str) -> Option<char> {
        match self.app.popup() {
            Some(Popup::Choose { choices, .. }) => choices
                .iter()
                .position(|c| c.label.starts_with(prefix))
                .map(|i| char::from(b'1' + i as u8)),
            _ => None,
        }
    }

    fn choose(&mut self, prefix: &str) {
        let c = self
            .choice_index(prefix)
            .unwrap_or_else(|| panic!("no choice {prefix:?} in {:?}", self.app.popup()));
        self.press(key(KeyCode::Char(c)));
    }
}

#[test]
fn timer_menu_alarm_without_popup_and_bookkeeping() {
    let mut h = Harness::new(true);
    fs::write(h.tasks.join("alpha/CONTEXT.md"), PLAN).unwrap();
    h.app.refresh_next_up();
    assert_eq!(h.app.next_up().len(), 2);
    // m opens the timer menu; starting leaves the task in its column.
    h.press(key(KeyCode::Char('m')));
    assert!(
        popup_title(&h.app).starts_with("Timer · alpha"),
        "{:?}",
        h.app.popup()
    );
    h.choose("start: Read the spec");
    let t = h.app.timer().expect("timer");
    assert_eq!(t.checkpoint.as_deref(), Some("Read the spec"));
    assert_eq!(t.checkpoint_index, Some(0));
    assert_eq!(t.budget, Duration::from_secs(20 * 60));
    assert_eq!(
        h.app.store().unwrap().board().locate("alpha").map(|l| l.0),
        Some(0)
    );
    assert!(h.context_md("alpha").contains("- started: "));

    // 7 minutes pass; pausing books whole minutes to the checkpoint, task and ledger.
    h.app
        .timer
        .as_mut()
        .unwrap()
        .backdate(Duration::from_secs(7 * 60 + 20));
    h.press(key(KeyCode::Char('m')));
    h.choose("pause");
    assert!(!h.app.timer().unwrap().is_running());
    let md = h.context_md("alpha");
    assert!(md.contains("- [ ] Read the spec (20m; spent 7m)"), "{md}");
    assert!(md.contains("- time_spent: 7m"), "{md}");
    let ledger = fs::read_to_string(h.tasks.join("timelog.tsv")).unwrap();
    assert!(ledger.contains("\talpha\t7\tRead the spec"), "{ledger}");
    h.press(key(KeyCode::Char('m')));
    h.choose("resume");
    assert!(h.app.timer().unwrap().is_running());

    // Time runs out: flash, bell, blinking chip — and no popup taking the keys.
    h.app
        .timer
        .as_mut()
        .unwrap()
        .backdate(Duration::from_secs(13 * 60));
    h.app.handle(AppEvent::Tick);
    assert!(h.app.popup().is_none(), "{:?}", h.app.popup());
    assert!(h.app.take_bell());
    assert!(h.app.flash_on());
    assert!(h.app.timer().unwrap().alarming());
    assert!(h.app.tick_interval() < Duration::from_millis(250));
    // Opening the menu (Esc m or a click on the chip) acknowledges it.
    h.app.ui.timer_chip = Rect::new(100, 40, 20, 1);
    h.mouse(click(105, 40));
    assert!(!h.app.timer().unwrap().alarming());
    h.choose("done → next: Write the code");
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
    assert_eq!(h.app.timer().unwrap().checkpoint_index, Some(1));

    // v ticks the last one and stops.
    h.press(key(KeyCode::Char('v')));
    assert!(h.app.timer().is_none());
    assert!(h.context_md("alpha").contains("- [x] Write the code (1h)"));

    // Finishing records the date without prompting; O records an outcome.
    h.press(key(KeyCode::Char(']')));
    h.press(key(KeyCode::Char(']')));
    assert!(h.app.popup().is_none());
    assert!(h.context_md("alpha").contains("- finished: "));
    h.press(key(KeyCode::Char('O')));
    h.type_str("Shipped the parser");
    h.press(key(KeyCode::Enter));
    assert!(h.context_md("alpha").contains("Shipped the parser"));
    h.press(key(KeyCode::Char('[')));
    assert!(!h.context_md("alpha").contains("- finished: "));
    h.app.refresh_next_up();
    assert!(h.app.today().today_min >= 20, "{:?}", h.app.today());
}

#[test]
fn idle_pause_and_restart_restore_the_timer() {
    let mut h = Harness::build(true, |cfg| cfg.timer.idle_minutes = 1);
    fs::write(h.tasks.join("alpha/CONTEXT.md"), PLAN).unwrap();
    h.press(key(KeyCode::Char('m')));
    h.choose("start");
    h.app.last_input = Instant::now().checked_sub(Duration::from_secs(90)).unwrap();
    h.app.handle(AppEvent::Tick);
    let t = h.app.timer().unwrap();
    assert!(!t.is_running());
    assert!(matches!(t.away, Some((_, timer::Away::Idle))));
    h.press(key(KeyCode::Char('m')));
    assert!(h.choice_index("resume, counting").is_some());
    h.choose("resume without");
    assert!(h.app.timer().unwrap().is_running());
    assert!(h.app.timer().unwrap().elapsed(Instant::now()) < Duration::from_secs(5));

    // Quit with the timer running: it comes back paused, offering the closed time.
    h.app
        .timer
        .as_mut()
        .unwrap()
        .backdate(Duration::from_secs(3 * 60));
    h.restart();
    let t = h.app.timer().expect("restored timer");
    assert_eq!(t.task_id, "alpha");
    assert!(!t.is_running());
    assert!(t.elapsed(Instant::now()) >= Duration::from_secs(180));
    assert!(h.context_md("alpha").contains("spent 3m"));
}

#[test]
fn focus_block_without_checkpoints_and_checkpoint_popup() {
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('m')));
    assert!(h.choice_index("start a focus block (25 min)").is_some());
    h.choose("focus block of");
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
    let session = fs::read_to_string(h.root.join("state/session.json")).unwrap();
    assert!(session.contains("\"task_id\": \"alpha\""), "{session}");

    // The checkpoint popup ticks items and starts the timer on the selected one.
    fs::write(h.tasks.join("alpha/CONTEXT.md"), PLAN).unwrap();
    h.watch();
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
    h.watch();
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
        cfg.gerrit_status_command = r#"for id in "$@"; do printf '{"change_id":"%s","number":7,"status":"NEW","labels":"CR+2"}\n' "$id"; done"#.into();
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
            "- gerrit: fw {id} https://review.example.com/q/{id} [NEW #7 CR+2] :: Fix it"
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
            file: None,
        }];
    });
    h.press(key(KeyCode::Char(',')));
    let Mode::Settings(form) = &mut h.app.mode else {
        panic!()
    };
    let idx = form
        .rows()
        .iter()
        .position(|r| matches!(r, config_form::Row::Item { field, .. } if form.fields()[*field].key == config_form::FieldKey::TaskSources))
        .unwrap();
    form.select(idx);
    h.press(key(KeyCode::Char('?')));
    assert!(matches!(h.app.popup(), Some(Popup::Help { tabs, tab, .. })
        if tabs[*tab].1.iter().any(|l| l.contains("JSON array"))));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('t')));
    assert!(h.pump_until(|app| matches!(app.popup(), Some(Popup::Doc { .. }))));
    assert!(
        matches!(h.app.popup(), Some(Popup::Doc { lines, .. }) if lines.iter().any(|l| l.contains("J-1") && l.contains("One")))
    );
    assert!(!h.tasks.join("J-1").exists());
}

// ----- hooks, help, list, editor merge, keys, shells ------------------------------

#[test]
fn task_enter_hook_runs_before_the_task_view_and_others_get_env() {
    let mut h = Harness::build(true, |cfg| {
        cfg.hooks.insert(
            "task_enter".into(),
            r#"printf '%s|%s|%s|%s\n' "$PAHIRI_HOOK" "$PAHIRI_TASK" "$PAHIRI_COLUMN" "$PAHIRI_PREV_TASK" > enter.txt; "$PAHIRI_BIN" --version >/dev/null 2>&1; printf '\nhook note\n' >> CONTEXT.md; echo entered"#
                .into(),
        );
        cfg.hooks.insert(
            "task_move".into(),
            r#"echo "$PAHIRI_FROM_COLUMN>$PAHIRI_TO_COLUMN:$PAHIRI_FINISHED" > "$PAHIRI_TASKS_DIR/moved.txt""#.into(),
        );
        cfg.hooks.insert("timer_start".into(), "exit 3".into());
    });
    h.press(key(KeyCode::Enter));
    // The view is not shown until the hook finished.
    assert!(matches!(h.app.mode(), Mode::Home));
    assert!(matches!(
        h.app.popup(),
        Some(Popup::Log { done: false, .. })
    ));
    assert!(h.pump_until(|app| matches!(app.mode(), Mode::Task)));
    assert!(h.app.popup().is_none(), "{:?}", h.app.popup());
    assert_eq!(
        fs::read_to_string(h.tasks.join("alpha/enter.txt")).unwrap(),
        "task_enter|alpha|Planned|\n"
    );
    // What the hook wrote is what the task view shows.
    assert!(h.ctx().tree.nodes().iter().any(|n| n.name == "enter.txt"));
    assert!(h.context_md("alpha").contains("hook note"));
    assert_eq!(h.app.status(), Some("task_enter: entered"));

    // Background hooks: task_move gets the columns; a failing hook only sets a status.
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char(']')));
    let moved = h.tasks.join("moved.txt");
    assert!(h.pump_until(|_| h_file(&moved).is_some()));
    assert_eq!(
        h_file(&h.tasks.join("moved.txt")).unwrap(),
        "Planned>Doing:0\n"
    );
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('m')));
    h.choose("start");
    assert!(h.pump_until(|app| app.hook_runs.iter().any(|r| r.event == "timer_start")));
    let run = h
        .app
        .hook_runs
        .iter()
        .find(|r| r.event == "timer_start")
        .unwrap();
    assert!(!run.ok);
    // The help page lists configured hooks and recent runs.
    h.press(key(KeyCode::F(1)));
    let Some(Popup::Help { tabs, .. }) = h.app.popup() else {
        panic!("{:?}", h.app.popup());
    };
    let hooks_tab = &tabs[HelpTopic::Hooks.index()].1;
    assert!(hooks_tab.iter().any(|l| l.contains("● task_enter")));
    assert!(hooks_tab
        .iter()
        .any(|l| l.contains("timer_start") && l.contains("FAILED")));
    assert!(hooks_tab.iter().any(|l| l.contains("$PAHIRI_FROM_COLUMN")));
    h.press(key(KeyCode::Right));
    assert!(matches!(h.app.popup(), Some(Popup::Help { tab: 1, .. })));
    h.press(key(KeyCode::Char('x')));
    assert!(h.app.popup().is_none());
    h.app.shutdown();
}

fn h_file(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok()
}

#[test]
fn list_filter_reorder_archive_and_outside_moves() {
    let mut h = Harness::new(true);
    fs::create_dir_all(h.tasks.join("gamma")).unwrap();
    fs::write(
        h.tasks.join("gamma/CONTEXT.md"),
        "# gamma: Flux capacitor\n",
    )
    .unwrap();
    h.watch();
    assert!(h.app.rows().contains(&ListRow::Task(0, "gamma".into())));
    // Filter by title words.
    h.press(key(KeyCode::Char('/')));
    h.type_str("flux");
    h.press(key(KeyCode::Enter));
    let tasks: Vec<ListRow> = h
        .app
        .rows()
        .into_iter()
        .filter(|r| matches!(r, ListRow::Task(..)))
        .collect();
    assert_eq!(tasks, vec![ListRow::Task(0, "gamma".into())]);
    assert_eq!(selected_task(&h.app).as_deref(), Some("gamma"));
    h.press(key(KeyCode::Esc));
    assert!(h.app.list_filter().0.is_empty());
    // J/K reorder within the column.
    assert_eq!(
        h.app.store().unwrap().board().columns[0].tasks,
        vec!["alpha", "gamma"]
    );
    h.press(key(KeyCode::Char('K')));
    assert_eq!(
        h.app.store().unwrap().board().columns[0].tasks,
        vec!["gamma", "alpha"]
    );
    // Another program moves a task: picked up without a restart.
    crate::cli::task_move(h.app.config(), "gamma", "Done").unwrap();
    h.watch();
    assert_eq!(
        h.app.store().unwrap().board().locate("gamma").map(|l| l.0),
        Some(2)
    );
    // Finished long ago → archived (hidden until A).
    crate::tasks::context::update_meta(&h.tasks.join("gamma/CONTEXT.md"), "gamma", |m| {
        m.finished = Some("2020-01-01T00:00:00Z".into());
    })
    .unwrap();
    h.app.refresh_next_up();
    assert!(!h.app.rows().contains(&ListRow::Task(2, "gamma".into())));
    assert_eq!(h.app.archived_count(2), 1);
    h.press(key(KeyCode::Char('A')));
    assert!(h.app.rows().contains(&ListRow::Task(2, "gamma".into())));
}

#[test]
fn saving_context_merges_changes_made_by_pahiri_meanwhile() {
    let mut h = Harness::new(true);
    fs::write(h.tasks.join("alpha/CONTEXT.md"), PLAN).unwrap();
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('o')));
    h.press(key(KeyCode::End));
    h.type_str(" edited");
    assert!(h.ctx().editor.as_ref().unwrap().is_dirty());
    // Meanwhile the timer books time and the CLI logs a line.
    crate::cli::task_log(h.app.config(), "alpha", "from outside").unwrap();
    let path = h.tasks.join("alpha/CONTEXT.md");
    crate::tasks::context::update_meta(&path, "alpha", |m| m.time_spent_min = 42).unwrap();
    h.press(ctrl('s'));
    let md = h.context_md("alpha");
    assert!(md.starts_with("# alpha edited\n"), "{md}");
    assert!(md.contains("from outside"), "{md}");
    assert!(md.contains("- time_spent: 42m"), "{md}");
    assert!(!h.ctx().editor.as_ref().unwrap().is_dirty());
    assert!(h.app.status().unwrap_or("").contains("merged"));
    // Undo in the editor.
    h.type_str("!");
    h.press(ctrl('z'));
    assert!(!h.ctx().editor.as_ref().unwrap().text().contains("edited!"));
    // Find.
    h.press(ctrl('f'));
    h.type_str("write the");
    h.press(key(KeyCode::Enter));
    let (row, _) = h.ctx().editor.as_ref().unwrap().cursor();
    assert!(h.ctx().editor.as_ref().unwrap().lines()[row].contains("Write the code"));
}

#[test]
fn existing_ticket_offers_open_or_refresh() {
    let script =
        r#"printf '%s' '[{"id":"alpha","title":"Alpha","description":"Fresh description"}]'"#;
    let mut h = Harness::build(true, |cfg| {
        cfg.task_sources = vec![TaskSource {
            name: "jira".into(),
            command: script.into(),
            file: None,
        }];
    });
    h.press(key(KeyCode::Char('n')));
    h.press(key(KeyCode::Char('2')));
    assert!(h.pump_until(|app| matches!(app.popup(), Some(Popup::Tickets { .. }))));
    h.press(key(KeyCode::Enter));
    assert!(popup_title(&h.app).contains("already exists"));
    h.choose("refresh its ## Description");
    assert!(h.pump_until(|app| matches!(app.mode(), Mode::Task)));
    assert!(h
        .context_md("alpha")
        .contains("## Description\n\nFresh description\n"));
}

#[test]
fn key_overrides_change_palette_and_leader() {
    let mut h = Harness::build(true, |cfg| {
        cfg.keys.insert("palette.timer".into(), "u".into());
        cfg.keys.insert("leader.help".into(), "H".into());
    });
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('u')));
    assert!(popup_title(&h.app).starts_with("Timer"));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Enter));
    assert!(h.pump_until(|app| matches!(app.mode(), Mode::Task)));
    h.press(ctrl('b'));
    h.press(KeyEvent::new(KeyCode::Char('H'), KeyModifiers::SHIFT));
    assert!(matches!(h.app.popup(), Some(Popup::Help { .. })));
}

#[test]
fn shells_are_restored_after_a_restart() {
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Char('t')));
    h.type_str("cd scripts");
    h.press(key(KeyCode::Enter));
    let scripts = fs::canonicalize(h.tasks.join("alpha/scripts")).unwrap();
    assert!(h.pump_until(|app| {
        app.active_context()
            .and_then(TaskContext::active_shell)
            .and_then(|s| s.session.cwd())
            .is_some_and(|c| c == scripts)
    }));
    h.restart();
    assert!(matches!(h.app.mode(), Mode::Home));
    h.press(key(KeyCode::Enter));
    assert_eq!(h.ctx().shells.len(), 1);
    assert!(h.pump_until(|app| {
        app.active_context()
            .and_then(TaskContext::active_shell)
            .and_then(|s| s.session.cwd())
            .is_some_and(|c| c == scripts)
    }));
    h.app.shutdown();
}

#[test]
fn tmux_backed_shells_survive_pahiri() {
    if Command::new("tmux").arg("-V").output().is_err() {
        return;
    }
    let socket = format!("pahiri-test-{}", std::process::id());
    std::env::set_var("PAHIRI_TMUX_SOCKET", &socket);
    let mut h = Harness::build(true, |cfg| cfg.shell.tmux = true);
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Char('t')));
    let name = h
        .ctx()
        .active_shell()
        .unwrap()
        .tmux
        .clone()
        .expect("tmux session");
    assert_eq!(name, "pahiri-alpha-1");
    let alive = |n: &str| {
        Command::new("tmux")
            .args(["-L", &socket, "has-session", "-t", n])
            .output()
            .is_ok_and(|o| o.status.success())
    };
    assert!(alive("pahiri-alpha-1"), "created before pahiri attaches");
    h.restart();
    assert!(alive(&name), "session survives pahiri exiting");
    h.press(key(KeyCode::Enter));
    assert_eq!(h.ctx().shells[0].tmux.as_deref(), Some(name.as_str()));
    assert!(h.pump_until(|app| {
        app.active_context()
            .and_then(TaskContext::active_shell)
            .is_some_and(|s| !s.session.screen().contents().trim().is_empty())
    }));
    // Closing the shell in pahiri ends the session.
    h.press(ctrl('b'));
    h.press(key(KeyCode::Char('x')));
    assert!(matches!(h.app.popup(), Some(Popup::Confirm { .. })));
    h.press(key(KeyCode::Char('y')));
    assert!(h.ctx().shells.is_empty());
    assert!(h.pump_until(|_| !alive(&name)));
    let _ = Command::new("tmux")
        .args(["-L", &socket, "kill-server"])
        .output();
    std::env::remove_var("PAHIRI_TMUX_SOCKET");
    h.app.shutdown();
}

fn with_mods(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, mods)
}

fn editor_text(h: &Harness) -> String {
    h.ctx().editor.as_ref().unwrap().text()
}

fn editor_selection(h: &Harness) -> Option<String> {
    h.ctx().editor.as_ref().unwrap().selected_text()
}

#[test]
fn editor_selects_moves_by_word_and_copies_cuts_pastes() {
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('o')));
    h.press(with_mods(KeyCode::End, KeyModifiers::CONTROL));
    h.type_str("fix the login bug");
    // Ctrl+← moves by word; Ctrl+Shift+← selects by word.
    h.press(with_mods(KeyCode::Left, KeyModifiers::CONTROL));
    h.press(with_mods(
        KeyCode::Left,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    assert_eq!(editor_selection(&h).as_deref(), Some("login "));
    h.press(with_mods(KeyCode::Right, KeyModifiers::SHIFT));
    assert_eq!(editor_selection(&h).as_deref(), Some("ogin "));
    // Ctrl+C copies (and does not quit), also to the terminal via OSC 52.
    h.press(ctrl('c'));
    assert!(!h.app.should_quit() && h.app.popup().is_none());
    assert_eq!(h.app.clipboard, "ogin ");
    assert!(h.app.status().unwrap_or("").contains("copied"));
    let out = String::from_utf8(h.app.take_terminal_output()).unwrap();
    assert!(out.starts_with("\x1b]52;c;"), "{out:?}");
    // → collapses the selection to its end; Ctrl+V pastes.
    h.press(key(KeyCode::Right));
    assert_eq!(editor_selection(&h), None);
    h.press(ctrl('v'));
    assert!(editor_text(&h).ends_with("fix the login ogin bug"));
    // Shift+Home then Ctrl+X cuts to the line start; Ctrl+Z brings it back.
    h.press(with_mods(KeyCode::Home, KeyModifiers::SHIFT));
    h.press(ctrl('x'));
    assert!(editor_text(&h).ends_with("\nbug"), "{}", editor_text(&h));
    assert_eq!(h.app.clipboard, "fix the login ogin ");
    h.press(ctrl('z'));
    assert!(editor_text(&h).ends_with("fix the login ogin bug"));
    // Ctrl+Backspace (Ctrl+H in most terminals) deletes a word.
    h.press(with_mods(KeyCode::End, KeyModifiers::NONE));
    h.press(ctrl('h'));
    assert!(editor_text(&h).ends_with("fix the login ogin "));
    // Typing replaces a selection; Ctrl+A selects everything.
    h.press(ctrl('a'));
    h.type_str("new");
    assert_eq!(editor_text(&h), "new");
    // Ctrl+C with nothing selected copies the line.
    h.press(ctrl('c'));
    assert_eq!(h.app.clipboard, "new");
}

#[test]
fn copy_and_paste_commands_reach_the_system_clipboard() {
    let mut h = Harness::build(true, |cfg| {
        cfg.copy_command = "cat > copied.txt".into();
        cfg.paste_command = "printf 'from system'".into();
    });
    let copied = h.tasks.join("copied.txt");
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('o')));
    h.press(ctrl('a'));
    h.press(ctrl('c'));
    assert!(
        h.app.take_terminal_output().is_empty(),
        "no OSC 52 with a command"
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while fs::read_to_string(&copied).unwrap_or_default().is_empty()
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(fs::read_to_string(&copied).unwrap(), "# alpha\n");
    h.press(ctrl('v'));
    assert_eq!(editor_text(&h), "from system");
}

#[test]
fn mouse_drag_and_multi_click_select_in_the_editor() {
    let mut h = Harness::new(true);
    fs::write(
        h.tasks.join("alpha/CONTEXT.md"),
        "# alpha\nfirst line here\nsecond line\n",
    )
    .unwrap();
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('o')));
    h.app.ui.editor = Rect::new(24, 1, 60, 10);
    h.app.ui.editor_gutter = 3;
    let at = |kind, col: u16, row: u16| MouseEvent {
        kind,
        column: 24 + 3 + col,
        row: 1 + row,
        modifiers: KeyModifiers::NONE,
    };
    let left = MouseButton::Left;
    // Press on "line" in row 1, drag into row 2, release.
    h.mouse(at(MouseEventKind::Down(left), 6, 1));
    assert_eq!(editor_selection(&h), None, "a plain click selects nothing");
    h.mouse(at(MouseEventKind::Drag(left), 6, 2));
    h.mouse(at(MouseEventKind::Up(left), 6, 2));
    assert_eq!(editor_selection(&h).as_deref(), Some("line here\nsecond"));
    // Dragging past the bottom of the pane keeps selecting (to the last line).
    h.mouse(at(MouseEventKind::Down(left), 0, 0));
    h.mouse(at(MouseEventKind::Drag(left), 3, 20));
    assert!(editor_selection(&h).unwrap().starts_with("# alpha\nfirst"));
    h.mouse(at(MouseEventKind::Up(left), 3, 20));
    // Double-click selects a word, a third click the line.
    h.mouse(at(MouseEventKind::Down(left), 1, 2));
    h.mouse(at(MouseEventKind::Down(left), 1, 2));
    assert_eq!(editor_selection(&h).as_deref(), Some("second"));
    h.mouse(at(MouseEventKind::Down(left), 1, 2));
    assert_eq!(editor_selection(&h).as_deref(), Some("second line\n"));
}

const BETA_CHECKPOINTS: &str = "# beta\n\n## Checkpoints\n<!-- pahiri:checkpoints -->\n- [x] Old step (10m)\n- [ ] Read spec (30m)\n- [ ] Write parser (2h)\n<!-- /pahiri:checkpoints -->\n";
const ALPHA_CHECKPOINTS: &str = "# alpha\n\n## Checkpoints\n<!-- pahiri:checkpoints -->\n- [ ] Draft (20m)\n<!-- /pahiri:checkpoints -->\n";

fn plan_view(app: &App) -> &PlanView {
    match app.mode() {
        Mode::Plan(v) => v,
        _ => panic!("not in the Plan view"),
    }
}

fn today_plan_file(h: &Harness) -> String {
    let date = App::today_date();
    fs::read_to_string(crate::tasks::plan::path(&h.tasks, &date)).unwrap_or_default()
}

#[test]
fn plan_view_picks_suggests_orders_and_saves() {
    let mut h = Harness::build(true, |cfg| {
        cfg.planner.day_minutes = 100;
        cfg.hooks.insert(
            "plan_save".into(),
            "echo $PAHIRI_PLAN_ITEMS > saved.txt".into(),
        );
        fs::write(cfg.tasks_dir.join("beta/CONTEXT.md"), BETA_CHECKPOINTS).unwrap();
        fs::write(cfg.tasks_dir.join("alpha/CONTEXT.md"), ALPHA_CHECKPOINTS).unwrap();
    });
    h.press(key(KeyCode::Char('p')));
    let v = plan_view(&h.app);
    // Only Doing (the middle column) is offered; done checkpoints are not.
    assert!(
        v.rows.contains(&PlanRow::Section("DOING".into())),
        "{:?}",
        v.rows
    );
    assert!(!v
        .rows
        .iter()
        .any(|r| matches!(r, PlanRow::Task(id, _) if id == "alpha")));
    let titles: Vec<&str> = v
        .rows
        .iter()
        .filter_map(|r| match r {
            PlanRow::Item(i) => Some(i.title.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(titles, ["Read spec", "Write parser"]);
    // Suggest fills up to the capacity (100 min): the 2h step does not fit.
    h.press(key(KeyCode::Char('s')));
    let v = plan_view(&h.app);
    assert_eq!(v.picked.len(), 1);
    assert_eq!(v.picked[0].title, "Read spec");
    // A shows every open column; pick alpha's checkpoint with Space.
    h.press(key(KeyCode::Char('A')));
    let v = plan_view(&h.app);
    let draft = v
        .rows
        .iter()
        .position(|r| matches!(r, PlanRow::Item(i) if i.title == "Draft"))
        .unwrap();
    while plan_view(&h.app).cursor < draft {
        h.press(key(KeyCode::Down));
    }
    h.press(key(KeyCode::Char(' ')));
    assert_eq!(plan_view(&h.app).picked.len(), 2);
    // A free item, then order: Draft first.
    h.press(key(KeyCode::Char('a')));
    h.type_str("Email the vendor 15m");
    h.press(key(KeyCode::Enter));
    assert_eq!(plan_view(&h.app).focus, PlanFocus::Order);
    h.press(key(KeyCode::Up));
    h.press(key(KeyCode::Char('K')));
    let v = plan_view(&h.app);
    let order: Vec<&str> = v.picked.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(order, ["Draft", "Read spec", "Email the vendor"]);
    assert_eq!(v.planned_min(), 20 + 30 + 15);
    // Enter saves and shows the plan on Home.
    h.press(key(KeyCode::Enter));
    assert!(matches!(h.app.mode(), Mode::Home));
    assert_eq!(h.app.home_focus(), HomeFocus::Today);
    let file = today_plan_file(&h);
    assert!(
        file.contains(
            "- [ ] alpha · Draft (20m)\n- [ ] beta · Read spec (30m)\n- [ ] Email the vendor (15m)\n"
        ),
        "{file}"
    );
    let saved = h.tasks.join("saved.txt");
    assert!(h.pump_until(|_| fs::read_to_string(&saved).is_ok_and(|s| s.trim() == "3")));
    // Esc in the Plan view with changes asks first.
    h.press(key(KeyCode::Char('p')));
    h.press(key(KeyCode::Char('s')));
    h.press(key(KeyCode::Esc));
    assert!(popup_title(&h.app).contains("Discard"));
    h.press(key(KeyCode::Char('y')));
    assert!(matches!(h.app.mode(), Mode::Home));
    assert!(
        today_plan_file(&h).contains("alpha · Draft"),
        "discarding kept the file"
    );
}

#[test]
fn today_pane_runs_the_plan_and_the_timer_follows_it() {
    let mut h = Harness::build(true, |cfg| {
        fs::write(cfg.tasks_dir.join("beta/CONTEXT.md"), BETA_CHECKPOINTS).unwrap();
        fs::write(cfg.tasks_dir.join("alpha/CONTEXT.md"), ALPHA_CHECKPOINTS).unwrap();
        let date = App::today_date();
        let items: Vec<_> = [
            "- [ ] beta · Read spec (30m)",
            "- [ ] alpha · Draft (20m)",
            "- [ ] beta · Reply to review (10m)",
            "- [ ] Email the vendor (15m)",
        ]
        .iter()
        .map(|l| crate::tasks::plan::PlanItem::parse(l).unwrap())
        .collect();
        crate::tasks::plan::write(
            &crate::tasks::plan::path(&cfg.tasks_dir, &date),
            &date,
            &items,
        )
        .unwrap();
    });
    assert_eq!(h.app.day_plan().len(), 4);
    h.press(key(KeyCode::Tab));
    assert_eq!(h.app.home_focus(), HomeFocus::Today);
    // Enter starts the timer on the selected checkpoint.
    h.press(key(KeyCode::Enter));
    let t = h.app.timer().unwrap();
    assert_eq!((t.task_id.as_str(), t.what()), ("beta", "Read spec"));
    // Space on the timed item ticks it and moves on to the plan's next item,
    // which is in another task.
    h.press(key(KeyCode::Char(' ')));
    assert!(h.context_md("beta").contains("- [x] Read spec"));
    let t = h.app.timer().unwrap();
    assert_eq!((t.task_id.as_str(), t.what()), ("alpha", "Draft"));
    assert!(today_plan_file(&h).contains("- [x] beta · Read spec"));
    // Done → next from the timer menu follows the plan too: a task item that is
    // not a checkpoint gets a timer of its estimate and is ticked in the plan.
    h.press(key(KeyCode::Char('m')));
    h.choose("done → next: beta · Reply to review");
    let t = h.app.timer().unwrap();
    assert_eq!((t.task_id.as_str(), t.what()), ("beta", "Reply to review"));
    assert_eq!(t.budget, Duration::from_secs(10 * 60));
    h.press(key(KeyCode::Char('M')));
    // The free item: Enter explains, Space ticks it in the plan file.
    h.press(key(KeyCode::Char('G')));
    h.press(key(KeyCode::Enter));
    assert!(h.app.status().unwrap_or("").contains("free item"));
    h.press(key(KeyCode::Char(' ')));
    assert!(today_plan_file(&h).contains("- [x] Email the vendor (15m)"));
    // J/K reorder, x removes.
    h.press(key(KeyCode::Char('K')));
    let order: Vec<String> = h.app.day_plan().iter().map(|i| i.title.clone()).collect();
    assert_eq!(
        order,
        ["Read spec", "Draft", "Email the vendor", "Reply to review"]
    );
    h.press(key(KeyCode::Char('x')));
    assert_eq!(h.app.day_plan().len(), 3);
    assert!(!today_plan_file(&h).contains("Email the vendor"));
    // o opens the selected item's task.
    h.press(key(KeyCode::Char('g')));
    h.press(key(KeyCode::Char('o')));
    assert!(h.pump_until(|app| matches!(app.mode(), Mode::Task)));
    assert_eq!(h.app.active_task(), Some("beta"));
    h.app.shutdown();
}

#[test]
fn yesterday_carries_over_and_day_start_fires_once() {
    let mut h = Harness::build(true, |cfg| {
        cfg.hooks.insert(
            "day_start".into(),
            "echo \"$PAHIRI_CARRIED $PAHIRI_DATE\" >> days.txt".into(),
        );
        fs::write(cfg.tasks_dir.join("beta/CONTEXT.md"), BETA_CHECKPOINTS).unwrap();
        let yesterday = crate::tasks::plan::local_date(
            crate::time::now_secs() - 86_400,
            crate::time::local_offset_secs(),
        );
        let items: Vec<_> = [
            "- [ ] beta · Read spec (30m)",
            "- [ ] beta · Old step (10m)",
            "- [ ] gone · Something (10m)",
            "- [x] Call Alice (5m)",
        ]
        .iter()
        .map(|l| crate::tasks::plan::PlanItem::parse(l).unwrap())
        .collect();
        crate::tasks::plan::write(
            &crate::tasks::plan::path(&cfg.tasks_dir, &yesterday),
            &yesterday,
            &items,
        )
        .unwrap();
    });
    // Only the open item of an existing task carries over ("Old step" is ticked
    // in CONTEXT.md, "gone" is not a task, the call is done).
    let (_, n) = h.app.carry_hint().cloned().unwrap();
    assert_eq!(n, 1);
    let days = h.tasks.join("days.txt");
    let today = App::today_date();
    assert!(h.pump_until(|_| {
        fs::read_to_string(&days).is_ok_and(|s| s.trim() == format!("1 {today}"))
    }));
    h.restart();
    h.watch();
    std::thread::sleep(Duration::from_millis(300));
    h.pump_until(|app| app.hooks_running() == 0);
    assert_eq!(
        fs::read_to_string(&days).unwrap().lines().count(),
        1,
        "day_start fires once a day"
    );
    // The Plan view starts from the carried item; Enter saves it for today.
    h.press(key(KeyCode::Char('p')));
    let v = plan_view(&h.app);
    assert!(v.dirty);
    assert_eq!(v.picked.len(), 1);
    assert!(matches!(&v.rows[0], PlanRow::Section(s) if s.starts_with("CARRIED OVER")));
    h.press(key(KeyCode::Enter));
    assert!(today_plan_file(&h).contains("- [ ] beta · Read spec (30m)"));
    assert!(h.app.carry_hint().is_none());
}

#[test]
fn plan_changes_on_disk_show_up() {
    let mut h = Harness::new(true);
    assert!(h.app.day_plan().is_empty());
    let date = App::today_date();
    let path = crate::tasks::plan::path(&h.tasks, &date);
    crate::tasks::plan::write(
        &path,
        &date,
        &[crate::tasks::plan::PlanItem::free("From a script 10m", 30).unwrap()],
    )
    .unwrap();
    h.watch();
    assert_eq!(h.app.day_plan().len(), 1);
    assert_eq!(h.app.day_plan()[0].title, "From a script");
}

#[test]
fn mouse_and_timer_menu_work_on_the_plan() {
    let mut h = Harness::build(true, |cfg| {
        fs::write(cfg.tasks_dir.join("beta/CONTEXT.md"), BETA_CHECKPOINTS).unwrap();
        let date = App::today_date();
        let items: Vec<_> = [
            "- [ ] beta · Read spec (30m)",
            "- [ ] beta · Write parser (2h)",
        ]
        .iter()
        .map(|l| crate::tasks::plan::PlanItem::parse(l).unwrap())
        .collect();
        crate::tasks::plan::write(
            &crate::tasks::plan::path(&cfg.tasks_dir, &date),
            &date,
            &items,
        )
        .unwrap();
    });
    // Pretend the Today pane was drawn with the items on rows 3 and 4.
    h.app.ui.today = Rect::new(24, 1, 60, 20);
    h.app.ui.today_items = vec![(3, 0), (4, 1)];
    h.mouse(click(30, 4));
    assert_eq!(h.app.home_focus(), HomeFocus::Today);
    assert_eq!(h.app.today_selected(), 1);
    // The timer menu offers the selected plan item first.
    h.press(key(KeyCode::Char('m')));
    h.choose("start: beta · Write parser");
    assert_eq!(h.app.timer().unwrap().what(), "Write parser");
    h.press(key(KeyCode::Char('M')));
    // Double-click starts the timer on the clicked item.
    h.mouse(click(30, 3));
    h.mouse(click(30, 3));
    assert_eq!(h.app.timer().unwrap().what(), "Read spec");
    // A click on the board gives it the keys back.
    h.app.ui.task_list = Rect::new(0, 0, 22, 20);
    h.mouse(click(3, 4));
    assert_eq!(h.app.home_focus(), HomeFocus::Board);

    // Plan view: click selects, double-click picks / removes.
    h.press(key(KeyCode::Char('p')));
    h.app.ui.plan_pick = Rect::new(0, 2, 50, 20);
    h.app.ui.plan_order = Rect::new(50, 2, 50, 20);
    let write = plan_view(&h.app)
        .rows
        .iter()
        .position(|r| matches!(r, PlanRow::Item(i) if i.title == "Write parser"))
        .unwrap() as u16;
    h.mouse(click(0, 3));
    h.mouse(click(10, 3 + write));
    assert_eq!(plan_view(&h.app).cursor, write as usize);
    h.mouse(click(10, 3 + write));
    assert_eq!(
        plan_view(&h.app).picked.len(),
        1,
        "double-click unpicked it"
    );
    h.mouse(click(60, 3));
    h.mouse(click(60, 3));
    assert!(
        plan_view(&h.app).picked.is_empty(),
        "double-click on the right removes"
    );
    h.app.shutdown();
}

/// Set `path`'s modification time `secs` seconds ahead so a change is seen
/// even on coarse file-system clocks.
fn bump_mtime(path: &Path, secs: u64) {
    let f = fs::File::options().write(true).open(path).unwrap();
    f.set_modified(std::time::SystemTime::now() + Duration::from_secs(secs))
        .unwrap();
}

#[test]
fn config_changes_on_disk_are_reloaded() {
    let mut h = Harness::new(true);
    let path = h.root.join("config.toml");
    h.app.config().save(&path).unwrap();
    bump_mtime(&path, 1);
    h.watch();
    assert!(h.app.status().is_none(), "same settings: nothing to say");

    // A script adds a workspace and a build: pahiri picks them up.
    let fw = h.root.join("fw");
    let yocto = h.root.join("yocto");
    fs::create_dir_all(&fw).unwrap();
    fs::create_dir_all(&yocto).unwrap();
    crate::cli::config_add_workspace(&path, "fw", &fw, Some("main".into())).unwrap();
    crate::cli::config_add_build(&path, "yocto", &yocto).unwrap();
    bump_mtime(&path, 2);
    h.watch();
    assert_eq!(h.app.config().workspaces.len(), 1);
    assert_eq!(h.app.config().builds.len(), 1);
    assert_eq!(
        h.app.status(),
        Some("config reloaded from disk · workspaces 1 (+1) · builds 1 (+1)")
    );

    // Broken or invalid files keep the previous settings.
    let good = fs::read_to_string(&path).unwrap();
    fs::write(&path, "garbage = [").unwrap();
    bump_mtime(&path, 3);
    h.watch();
    assert!(h.app.status().unwrap().contains("cannot be read"));
    assert_eq!(h.app.config().workspaces.len(), 1);
    fs::write(&path, good.replace("name = \"fw\"", "name = \"f w\"")).unwrap();
    bump_mtime(&path, 4);
    h.watch();
    let status = h.app.status().unwrap().to_owned();
    assert!(status.contains("keeping the previous settings"), "{status}");
    assert_eq!(h.app.config().workspaces[0].name, "fw");
    // A missing folder is loaded, with a warning.
    let missing = good.replace(&fw.display().to_string(), "/no/such/dir");
    fs::write(&path, missing).unwrap();
    bump_mtime(&path, 5);
    h.press(key(KeyCode::Down));
    h.watch();
    let status = h.app.status().unwrap().to_owned();
    assert!(
        status.contains("folder is missing: /no/such/dir"),
        "{status}"
    );
    assert_eq!(
        h.app.config().workspaces[0].path,
        std::path::PathBuf::from("/no/such/dir")
    );

    // Unsaved edits on the Settings view win until you leave it.
    fs::write(&path, &good).unwrap();
    bump_mtime(&path, 6);
    h.watch();
    h.press(key(KeyCode::Char(',')));
    h.press(key(KeyCode::Enter));
    h.type_str("x");
    h.press(key(KeyCode::Enter));
    assert!(matches!(h.app.mode(), Mode::Settings(f) if f.dirty()));
    crate::cli::config_remove(&path, true, "yocto").unwrap();
    bump_mtime(&path, 7);
    h.watch();
    assert!(h.app.status().unwrap().contains("changed on disk"));
    assert_eq!(h.app.config().builds.len(), 1);
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('y')));
    assert!(matches!(h.app.mode(), Mode::Home));
    h.watch();
    assert!(
        h.app.config().builds.is_empty(),
        "reloaded after leaving Settings"
    );
}

#[test]
fn tasks_dir_override_survives_a_reload() {
    let mut h = Harness::new(true);
    let path = h.root.join("config.toml");
    let mut on_disk = h.app.config().clone();
    let elsewhere = h.root.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    on_disk.tasks_dir.clone_from(&elsewhere);
    on_disk.default_main_branch = "develop".into();
    on_disk.save(&path).unwrap();
    let tasks = h.tasks.clone();
    h.app.set_tasks_dir_override(tasks.clone());
    bump_mtime(&path, 1);
    h.watch();
    assert_eq!(h.app.config().default_main_branch, "develop");
    assert_eq!(h.app.config().tasks_dir, tasks);
}

#[test]
fn run_a_hook_now_from_the_palette() {
    let mut h = Harness::build(true, |cfg| {
        cfg.hooks.insert(
            "startup".into(),
            "echo \"manual=$PAHIRI_MANUAL task=$PAHIRI_TASK\" > \"$PAHIRI_TASKS_DIR/ran.txt\""
                .into(),
        );
    });
    let ran = h.tasks.join("ran.txt");
    assert!(h.pump_until(|_| fs::read_to_string(&ran).is_ok_and(|s| s.contains("manual= "))));
    fs::remove_file(&ran).unwrap();
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('!')));
    assert!(popup_title(&h.app).starts_with("Run a hook now"));
    h.choose("startup");
    assert!(h.pump_until(|_| {
        fs::read_to_string(&ran).is_ok_and(|s| s.trim() == "manual=1 task=alpha")
    }));
    // With no hooks configured it says so instead of opening an empty menu.
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('!')));
    assert!(h.app.popup().is_none());
    assert!(h.app.status().unwrap().contains("no hooks configured"));
}

#[test]
fn a_missing_workspace_folder_does_not_stop_pahiri() {
    let h = Harness::build(true, |cfg| {
        cfg.workspaces.push(crate::config::Workspace {
            name: "gone".into(),
            path: cfg.tasks_dir.join("no-such-checkout"),
            main_branch: None,
        });
    });
    assert!(
        matches!(h.app.mode(), Mode::Home),
        "starts on Home, not Settings"
    );
    let status = h.app.status().unwrap_or_default();
    assert!(
        status.contains("workspace gone folder is missing"),
        "{status}"
    );
    assert!(status.contains("pahiri config prune"), "{status}");
}

#[test]
fn periodic_hook_runs_every_n_minutes() {
    let mut h = Harness::build(true, |cfg| {
        cfg.periodic_minutes = 10;
        cfg.hooks.insert(
            "periodic".into(),
            "echo run >> \"$PAHIRI_TASKS_DIR/periodic.txt\"".into(),
        );
    });
    let file = h.tasks.join("periodic.txt");
    h.watch();
    assert!(!file.exists(), "not before the interval");
    h.app.periodic_at = Instant::now()
        .checked_sub(Duration::from_secs(601))
        .unwrap();
    h.watch();
    assert!(h.pump_until(|app| !app.periodic_busy));
    h.watch();
    h.pump_until(|app| app.hooks_running() == 0);
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "run\n",
        "once per interval"
    );
}

/// Scripts for the audit: two tickets, four changes.
fn audit_scripts(cfg: &mut Config) {
    let tickets = r#"[
      {"id":"PROJ-1","title":"Fix the parser","url":"https://jira/browse/PROJ-1","description":"Parser breaks on tabs.","status":"Done","created":"2026-07-01T09:00:00.000+0000","finished":"2026-08-02T17:00:00.000+0000"},
      {"id":"beta","title":"Beta work","url":"https://jira/browse/beta","status":"Closed","finished":"2026-08-10"}
    ]"#;
    let changes = [
        r#"{"change_id":"I1111111111111111111111111111111111111111","number":11,"status":"MERGED","project":"fw","subject":"PROJ-1: handle tabs","created":"2026-07-02 10:00:00","merged":"2026-07-09 16:00:00"}"#,
        r#"{"change_id":"I1212121212121212121212121212121212121212","number":12,"status":"NEW","project":"fw","topic":"alpha","subject":"Alpha step one"}"#,
        r#"{"change_id":"I1313131313131313131313131313131313131313","number":13,"status":"NEW","project":"fw","subject":"Refactor the dts"}"#,
        r#"{"change_id":"I1414141414141414141414141414141414141414","number":14,"status":"MERGED","project":"fw","subject":"Bump version","updated":"2026-07-20 08:00:00"}"#,
    ]
    .join("\n");
    cfg.task_sources = vec![TaskSource {
        name: "jira".into(),
        command: format!(
            "[ \"$PAHIRI_AUDIT\" = 1 ] && [ -n \"$PAHIRI_AUDIT_SINCE\" ] && printf '%s' '{tickets}'"
        ),
        file: None,
    }];
    cfg.audit.gerrit_command = format!("printf '%s\\n' '{}'", changes.replace('\n', "' '"));
}

fn audit_view(app: &App) -> &AuditView {
    match app.mode() {
        Mode::Audit(v) => v,
        _ => panic!("not in the Audit view: {:?}", app.popup()),
    }
}

#[test]
fn audit_matches_asks_the_agent_and_applies() {
    let mut h = Harness::build(true, |cfg| {
        audit_scripts(cfg);
        cfg.agent.command = "sh".into();
        cfg.agent.args = vec![
            "-c".into(),
            "cat > /dev/null; printf 'MAP C:13 -> alpha | dts work\\nNEW C:14 -> release-bump | Version bumps\\n'"
                .into(),
        ];
    });
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('U')));
    assert!(h.pump_until(|app| matches!(app.mode(), Mode::Audit(_))));
    let v = audit_view(&h.app);
    let keys: Vec<String> = v.items.iter().map(|i| i.proposal.key()).collect();
    assert_eq!(
        keys,
        [
            "new:PROJ-1",
            "new:release-bump",
            "update:alpha",
            "update:beta"
        ],
        "{:#?}",
        v.items
    );
    assert!(v.items[1].by_ai && v.items[2].by_ai && !v.items[0].by_ai);
    assert_eq!(v.counts(), (2, 2, 0, 4));

    h.press(key(KeyCode::Char('a')));
    assert!(popup_title(&h.app).contains("Apply"));
    h.press(key(KeyCode::Char('y')));
    assert!(matches!(h.app.mode(), Mode::Home));
    let status = h.app.status().unwrap_or_default().to_owned();
    assert!(
        status.contains("audit applied: 2 created, 2 updated"),
        "{status}"
    );

    // New task from the ticket: done, dates from the ticket and its change.
    let p1 = h.context_md("PROJ-1");
    assert!(p1.starts_with("# PROJ-1: Fix the parser\n"), "{p1}");
    assert!(
        p1.contains("## Description\n\nParser breaks on tabs."),
        "{p1}"
    );
    for line in [
        "- source: jira",
        "- link: https://jira/browse/PROJ-1",
        "- gerrit: fw I1111111111111111111111111111111111111111 [MERGED #11] :: PROJ-1: handle tabs",
        "- created: 2026-07-01T09:00:00Z",
        "- started: 2026-07-02T10:00:00Z",
        "- finished: 2026-08-02T17:00:00Z",
    ] {
        assert!(p1.contains(line), "{line} missing in\n{p1}");
    }
    let board = fs::read_to_string(h.tasks.join("status.md")).unwrap();
    let done = board.split("## Done").nth(1).unwrap_or_default().to_owned();
    assert!(
        done.contains("- PROJ-1") && done.contains("- release-bump") && done.contains("- beta"),
        "{board}"
    );
    // The agent's new task, made from its change.
    let rb = h.context_md("release-bump");
    assert!(rb.starts_with("# release-bump: Version bumps\n"), "{rb}");
    assert!(rb.contains("- finished: 2026-07-20T08:00:00Z"), "{rb}");
    // alpha gets both changes (topic + the agent's MAP) and moves to Doing.
    let alpha = h.context_md("alpha");
    assert_eq!(alpha.matches("- gerrit: fw ").count(), 2, "{alpha}");
    assert!(alpha.contains("audit: + CR NEW #12"), "{alpha}");
    let doing = board
        .split("## Doing")
        .nth(1)
        .unwrap()
        .split("## Done")
        .next()
        .unwrap();
    assert!(doing.contains("- alpha"), "{board}");
    // beta: done with the ticket's date.
    assert!(h
        .context_md("beta")
        .contains("- finished: 2026-08-10T00:00:00Z"));
    // A report.
    let reports: Vec<_> = fs::read_dir(h.tasks.join(".pahiri/audit"))
        .unwrap()
        .collect();
    assert_eq!(reports.len(), 1);
    let report = fs::read_to_string(reports[0].as_ref().unwrap().path()).unwrap();
    assert!(
        report.contains("## Created\n\n- PROJ-1 — Fix the parser → Done"),
        "{report}"
    );
    // Running it again finds nothing new.
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('U')));
    assert!(h.pump_until(|app| matches!(app.popup(), Some(Popup::Log { done: true, .. }))));
    assert!(matches!(h.app.mode(), Mode::Home));
}

#[test]
fn audit_leaves_unmatched_changes_to_you() {
    let mut h = Harness::build(true, |cfg| {
        audit_scripts(cfg);
        cfg.audit.use_agent = false;
    });
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Char('U')));
    assert!(h.pump_until(|app| matches!(app.mode(), Mode::Audit(_))));
    assert_eq!(audit_view(&h.app).counts(), (1, 2, 2, 3));
    // Untick the beta update; attach #13 to alpha; make #14 a new task.
    let pos = |h: &Harness, key: &str| {
        audit_view(&h.app)
            .items
            .iter()
            .position(|i| i.proposal.key() == key)
            .unwrap()
    };
    let select = |h: &mut Harness, i: usize| {
        if let Mode::Audit(v) = &mut h.app.mode {
            v.selected = i;
        }
    };
    let i = pos(&h, "update:beta");
    select(&mut h, i);
    h.press(key(KeyCode::Char(' ')));
    let i = pos(&h, "orphan:13");
    select(&mut h, i);
    h.press(key(KeyCode::Enter));
    h.choose("attach it");
    h.choose("alpha");
    let i = pos(&h, "orphan:14");
    select(&mut h, i);
    h.press(key(KeyCode::Enter));
    h.choose("make a new task");
    if let Some(Popup::Input { value, .. }) = &mut h.app.popup {
        value.clear();
    }
    h.type_str("bumps");
    h.press(key(KeyCode::Enter));
    let v = audit_view(&h.app);
    assert_eq!(v.counts(), (2, 2, 0, 3), "{:#?}", v.items);
    let crate::tasks::audit::Proposal::Update(u) = &v.items[pos(&h, "update:alpha")].proposal
    else {
        panic!()
    };
    assert_eq!(u.changes.len(), 2);
    // Rename the new task from the ticket.
    let i = pos(&h, "new:PROJ-1");
    select(&mut h, i);
    h.press(key(KeyCode::Enter));
    h.choose("rename");
    if let Some(Popup::Input { value, .. }) = &mut h.app.popup {
        value.clear();
    }
    h.type_str("parser-fix");
    h.press(key(KeyCode::Enter));
    // Esc asks before throwing the decisions away.
    h.press(key(KeyCode::Esc));
    assert!(popup_title(&h.app).contains("Leave the audit"));
    h.press(key(KeyCode::Char('n')));
    h.press(key(KeyCode::Char('a')));
    h.press(key(KeyCode::Char('y')));
    assert!(h.tasks.join("parser-fix/CONTEXT.md").is_file());
    assert!(!h.tasks.join("PROJ-1").exists());
    let status = h.app.status().unwrap_or_default().to_owned();
    assert!(h.tasks.join("bumps").is_dir(), "{status}");
    assert!(h
        .context_md("bumps")
        .contains("#14 MERGED fw · Bump version"));
    let beta = fs::read_to_string(h.tasks.join("beta/CONTEXT.md")).unwrap_or_default();
    assert!(!beta.contains("- finished:"), "unticked");
}

#[test]
fn new_task_from_a_source_file() {
    let mut h = Harness::build(true, |cfg| {
        let file = cfg.tasks_dir.join("../jira.json");
        cfg.task_sources = vec![TaskSource {
            name: "jira".into(),
            command: r#"echo lots of noise; printf '[{"id":"BIG-1","title":"From the file"}]' > "$PAHIRI_OUTPUT_FILE""#.into(),
            file: Some(file),
        }];
    });
    h.press(key(KeyCode::Char('n')));
    h.choose("from jira");
    assert!(h.pump_until(|app| matches!(app.popup(), Some(Popup::Tickets { .. }))));
    let Some(Popup::Tickets { tickets, .. }) = h.app.popup() else {
        panic!()
    };
    assert_eq!(tickets[0].title, "From the file");
    assert!(h.app.status().unwrap_or_default().contains("jira.json"));
}
