//! Behavioural tests driving the app with synthetic events.

use std::fs;
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use super::*;

struct Harness {
    app: App,
    rx: Receiver<AppEvent>,
    _dir: tempfile::TempDir,
    tasks: PathBuf,
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

impl Harness {
    fn new(with_config: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let tasks = dir.path().join("tasks");
        fs::create_dir_all(tasks.join("alpha/scripts")).unwrap();
        fs::write(tasks.join("alpha/CONTEXT.md"), "# alpha\n").unwrap();
        fs::write(tasks.join("alpha/scripts/run.sh"), "echo hi\n").unwrap();
        fs::create_dir_all(tasks.join("beta")).unwrap();
        fs::write(tasks.join("status.md"), "## Doing\n- beta\n").unwrap();
        let config_path = dir.path().join("config.toml");
        let config = with_config.then(|| Config {
            tasks_dir: tasks.clone(),
            shell: crate::config::ShellConfig {
                program: "sh".into(),
                args: vec![],
            },
            ..Config::default()
        });
        let (tx, rx) = EventSender::channel();
        let app = App::new(config, config_path, tx);
        Self {
            app,
            rx,
            _dir: dir,
            tasks,
        }
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

    /// Pump PTY events until `pred` holds or the timeout elapses.
    fn pump_until(&mut self, pred: impl Fn(&App) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
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
    // Esc is refused until a valid config is saved.
    h.press(key(KeyCode::Esc));
    assert!(matches!(h.app.mode(), Mode::Config(_)));
    // Save without a folder: validation errors, still on config page.
    h.press(ctrl('s'));
    assert!(matches!(h.app.mode(), Mode::Config(f) if !f.errors().is_empty()));
    // Enter the tasks folder and save.
    h.press(key(KeyCode::Enter));
    let path = h.tasks.display().to_string();
    h.type_str(&path);
    h.press(key(KeyCode::Enter));
    h.press(ctrl('s'));
    assert!(matches!(h.app.mode(), Mode::Config(f) if f.errors().is_empty() && !f.first_run()));
    assert!(h.app.store().is_some());
    h.press(key(KeyCode::Esc));
    assert!(matches!(h.app.mode(), Mode::TaskList));
    assert!(Path::new(&h.app.config_path).is_file());
}

#[test]
fn list_navigation_and_moving_tasks() {
    let mut h = Harness::new(true);
    assert!(matches!(h.app.mode(), Mode::TaskList));
    // status.md put beta in Doing; alpha was discovered into Planned.
    assert_eq!(selected_task(&h.app).as_deref(), Some("alpha"));
    h.press(key(KeyCode::Down));
    assert_eq!(selected_task(&h.app).as_deref(), Some("beta"));
    h.press(key(KeyCode::Down));
    assert_eq!(selected_task(&h.app).as_deref(), Some("beta"));
    h.press(key(KeyCode::Char(']')));
    let board = h.app.store().unwrap().board().clone();
    assert_eq!(board.locate("beta"), Some((2, 0)));
    assert_eq!(selected_task(&h.app).as_deref(), Some("beta"));
    let status = fs::read_to_string(h.tasks.join("status.md")).unwrap();
    assert!(status.contains("## Done\n- beta"));
    h.press(key(KeyCode::Up));
    assert_eq!(selected_task(&h.app).as_deref(), Some("alpha"));
}

#[test]
fn create_task_via_popup() {
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Char('n')));
    assert!(matches!(h.app.popup(), Some(Popup::Input { .. })));
    h.type_str("gamma");
    h.press(key(KeyCode::Enter));
    assert!(h.app.popup().is_none());
    assert!(h.tasks.join("gamma/CONTEXT.md").is_file());
    assert_eq!(selected_task(&h.app).as_deref(), Some("gamma"));
}

#[test]
fn enter_task_browse_files_and_edit() {
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Enter));
    assert!(matches!(h.app.mode(), Mode::Task));
    assert_eq!(h.app.active_task(), Some("alpha"));
    assert_eq!(h.ctx().focus, Focus::Tree);
    let names: Vec<String> = h
        .ctx()
        .tree
        .nodes()
        .iter()
        .map(|n| n.name.clone())
        .collect();
    assert_eq!(names, vec!["scripts", "CONTEXT.md"]);

    // Enter on a folder toggles it, Enter on a file opens it.
    h.press(key(KeyCode::Enter));
    assert!(h.ctx().tree.nodes().iter().any(|n| n.name == "run.sh"));
    h.press(key(KeyCode::Down));
    h.press(key(KeyCode::Enter));
    assert_eq!(h.ctx().focus, Focus::Editor);
    assert!(h.ctx().editor.as_ref().unwrap().path().ends_with("run.sh"));

    // Type, save, verify on disk.
    h.press(ctrl('e')); // ignored (ctrl chars are not inserted)
    h.press(key(KeyCode::End));
    h.type_str("# edited");
    h.press(ctrl('s'));
    let content = fs::read_to_string(h.tasks.join("alpha/scripts/run.sh")).unwrap();
    assert_eq!(content, "echo hi# edited\n");
    assert!(!h.ctx().editor.as_ref().unwrap().is_dirty());

    // Close editor, Esc returns to the list.
    h.press(ctrl('w'));
    assert!(h.ctx().editor.is_none());
    assert_eq!(h.ctx().focus, Focus::Tree);
    h.press(key(KeyCode::Esc));
    assert!(matches!(h.app.mode(), Mode::TaskList));
}

#[test]
fn file_operations_from_tree() {
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Enter));
    // New file in the task root (selection is on the "scripts" folder → target is that folder).
    h.press(key(KeyCode::Down)); // CONTEXT.md → target dir is root
    h.press(key(KeyCode::Char('a')));
    h.type_str("notes.md");
    h.press(key(KeyCode::Enter));
    assert!(h.tasks.join("alpha/notes.md").is_file());
    assert_eq!(h.ctx().tree.selected().unwrap().name, "notes.md");

    h.press(key(KeyCode::Char('A')));
    h.type_str("data");
    h.press(key(KeyCode::Enter));
    assert!(h.tasks.join("alpha/data").is_dir());

    // Rename the folder.
    h.press(key(KeyCode::Char('r')));
    h.press(ctrl('u'));
    h.type_str("assets");
    h.press(key(KeyCode::Enter));
    assert!(h.tasks.join("alpha/assets").is_dir());
    assert!(!h.tasks.join("alpha/data").exists());

    // Delete asks first; 'n' keeps, 'y' deletes.
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
    let ed = h.ctx().editor.as_ref().unwrap();
    assert!(ed.is_read_only());
}

#[test]
fn shells_run_in_task_folder_and_leader_leaves_focus() {
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Enter));
    h.press(key(KeyCode::Char('t')));
    assert_eq!(h.ctx().focus, Focus::Terminal);
    assert_eq!(h.ctx().shells.len(), 1);
    assert!(h.ctx().shell_pane_visible());

    // Everything typed goes to the shell.
    h.type_str("printf 'cwd=%s task=%s' \"$PWD\" \"$PAHIRI_TASK\"");
    h.press(key(KeyCode::Enter));
    let expected = format!("task={}", "alpha");
    assert!(h.pump_until(|app| {
        app.active_context()
            .and_then(TaskContext::active_shell)
            .is_some_and(|s| s.session.screen().contents().contains(&expected))
    }));
    let contents = h.ctx().active_shell().unwrap().session.screen().contents();
    assert!(
        contents.contains(&format!("cwd={}", h.tasks.join("alpha").display())),
        "{contents}"
    );

    // Leader + q leaves the terminal; leader twice sends the leader key.
    h.press(ctrl('b'));
    assert!(h.app.leader_pending());
    h.press(key(KeyCode::Char('q')));
    assert_eq!(h.ctx().focus, Focus::Shells);
    assert!(!h.app.leader_pending());

    // Back into the terminal; zoom toggles.
    h.press(key(KeyCode::Enter));
    assert_eq!(h.ctx().focus, Focus::Terminal);
    h.press(ctrl('b'));
    h.press(key(KeyCode::Char('z')));
    assert!(h.ctx().zoomed);
    h.press(ctrl('b'));
    h.press(key(KeyCode::Char('z')));
    assert!(!h.ctx().zoomed);

    // A second shell, then close it through the confirm popup.
    h.press(ctrl('b'));
    h.press(key(KeyCode::Char('n')));
    assert_eq!(h.ctx().shells.len(), 2);
    assert_eq!(h.ctx().selected_shell, 1);
    h.press(ctrl('b'));
    h.press(key(KeyCode::Char('x')));
    h.press(key(KeyCode::Char('y')));
    assert_eq!(h.ctx().shells.len(), 1);

    // Exiting the shell marks it; a key press then removes it.
    h.press(ctrl('b'));
    h.press(key(KeyCode::Char('q')));
    h.press(key(KeyCode::Enter));
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
}

#[test]
fn switching_tasks_keeps_each_context() {
    let mut h = Harness::new(true);
    h.press(key(KeyCode::Enter)); // alpha
    h.press(key(KeyCode::Down));
    h.press(key(KeyCode::Enter)); // open CONTEXT.md
    assert!(h.ctx().editor.is_some());
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Esc));
    assert!(matches!(h.app.mode(), Mode::TaskList));
    h.press(key(KeyCode::Down));
    h.press(key(KeyCode::Enter)); // beta
    assert_eq!(h.app.active_task(), Some("beta"));
    assert!(h.ctx().editor.is_none());
    h.press(key(KeyCode::Esc));
    h.press(key(KeyCode::Up));
    h.press(key(KeyCode::Enter)); // back to alpha
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
    // Ctrl+C inside the terminal goes to the shell; leader + Ctrl+C asks to quit.
    h.press(ctrl('c'));
    assert!(h.app.popup().is_none());
    h.press(ctrl('b'));
    h.press(ctrl('c'));
    assert!(matches!(h.app.popup(), Some(Popup::Confirm { .. })));
    h.press(key(KeyCode::Char('n')));
    assert!(!h.app.should_quit());
    h.press(ctrl('b'));
    h.press(key(KeyCode::Char('q')));
    h.press(ctrl('c'));
    assert!(matches!(h.app.popup(), Some(Popup::Confirm { .. })));
    h.press(key(KeyCode::Char('y')));
    assert!(h.app.should_quit());
    h.app.shutdown();
}

#[test]
fn human_size_formats() {
    assert_eq!(human_size(10), "10 B");
    assert_eq!(human_size(2048), "2.0 KiB");
    assert_eq!(human_size(3 * 1024 * 1024), "3.0 MiB");
}
