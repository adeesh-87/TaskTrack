//! Behavioural tests driving the app with synthetic events.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

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
