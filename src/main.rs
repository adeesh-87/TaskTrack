//! Binary entry point: argument parsing, logging, terminal setup and the event loop.

use std::io;
use std::path::PathBuf;
use std::sync::mpsc::RecvTimeoutError;

use anyhow::{Context as _, Result};
use clap::{Parser, Subcommand};
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use tracing::info;

use pahiri::app::{App, AppEvent, EventSender};
use pahiri::config::Config;

/// A terminal workspace for executing and managing tasks.
#[derive(Debug, Parser)]
#[command(name = "pahiri", version, about)]
struct Cli {
    /// Config file to use instead of the default location.
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,
    /// Override the tasks folder for this run (also seeds a first-run config).
    #[arg(short, long, value_name = "DIR")]
    tasks_dir: Option<PathBuf>,
    /// Log level for the log file (error, warn, info, debug, trace).
    #[arg(long, default_value = "info")]
    log_level: String,
    /// Print the resolved config and paths, then exit.
    #[arg(long)]
    show_config: bool,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[allow(clippy::doc_markdown)] // doc comments double as --help text
#[derive(Debug, Subcommand)]
enum Cmd {
    /// Task helpers for scripts and agents (default task: $PAHIRI_TASK).
    Task {
        #[command(subcommand)]
        cmd: TaskCmd,
    },
    /// Summarise tasks active in a date range (input for reviews and standups).
    Report {
        /// First day, YYYY-MM-DD.
        #[arg(long)]
        from: Option<String>,
        /// Last day, YYYY-MM-DD.
        #[arg(long)]
        to: Option<String>,
        /// JSON instead of Markdown.
        #[arg(long)]
        json: bool,
    },
    /// Change code workspaces and vendor builds in the config (for scripts and
    /// hooks; a running pahiri reloads the config when it changes).
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// The day plan (<tasks>/.pahiri/plans/<date>.md) for scripts and agents.
    Plan {
        #[command(subcommand)]
        cmd: PlanCmd,
    },
    /// Manage deleted tasks in <tasks>/.trash.
    Trash {
        #[command(subcommand)]
        cmd: TrashCmd,
    },
    /// Write the bundled agent skills to DIR/<name>/SKILL.md
    /// (e.g. ~/.claude/skills or <tasks>/.agents/skills).
    InstallSkills {
        /// Target folder.
        dir: PathBuf,
        /// Overwrite skills that already exist.
        #[arg(long)]
        force: bool,
    },
}

#[allow(clippy::doc_markdown)]
#[derive(Debug, Subcommand)]
enum TaskCmd {
    /// Mark the task context ready for planning (--off: not ready).
    Ready {
        /// Task id (default $PAHIRI_TASK).
        #[arg(long)]
        task: Option<String>,
        /// Mark it not ready.
        #[arg(long)]
        off: bool,
    },
    /// Append a timestamped line to the task's ## Log.
    Log {
        /// Task id (default $PAHIRI_TASK).
        #[arg(long)]
        task: Option<String>,
        /// The message.
        #[arg(required = true)]
        message: Vec<String>,
    },
    /// Move the task to another board column (records started / finished).
    Move {
        /// Task id (default $PAHIRI_TASK).
        #[arg(long)]
        task: Option<String>,
        /// Column name (any case) or 0-based index.
        #[arg(long)]
        to: String,
    },
    /// Append a line to the task's ## Outcome (for reviews).
    Outcome {
        /// Task id (default $PAHIRI_TASK).
        #[arg(long)]
        task: Option<String>,
        /// The outcome.
        #[arg(required = true)]
        text: Vec<String>,
    },
    /// Print the current checkpoint.
    Next {
        /// Task id (default $PAHIRI_TASK).
        #[arg(long)]
        task: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum PlanCmd {
    /// Print the plan with each item's state.
    Show {
        /// Day, YYYY-MM-DD (default today).
        #[arg(long)]
        date: Option<String>,
        /// JSON instead of Markdown.
        #[arg(long)]
        json: bool,
    },
    /// Add an item, e.g. `pahiri plan add --task PROJ-42 Reply to review 20m`.
    Add {
        /// Task the item belongs to (default: a free item).
        #[arg(long)]
        task: Option<String>,
        /// Day, YYYY-MM-DD (default today).
        #[arg(long)]
        date: Option<String>,
        /// What, optionally ending with an estimate (20m, 1h).
        #[arg(required = true)]
        text: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCmd {
    /// Add a code workspace, or update the path / branch of the one with this name.
    AddWorkspace {
        /// Short name (one word).
        name: String,
        /// The git checkout.
        path: PathBuf,
        /// Its main branch (default: what git reports for a new workspace).
        #[arg(long)]
        main: Option<String>,
    },
    /// Add a vendor build folder, or update the path of the one with this name.
    AddBuild {
        /// Short name (one word).
        name: String,
        /// The build folder.
        path: PathBuf,
    },
    /// Remove a code workspace.
    RemoveWorkspace {
        /// Its name.
        name: String,
    },
    /// Remove a vendor build.
    RemoveBuild {
        /// Its name.
        name: String,
    },
    /// Remove workspaces and builds whose folder no longer exists.
    Prune,
    /// Print workspaces and builds (kind, name, path, main branch; tab separated).
    List,
}

#[derive(Debug, Subcommand)]
enum TrashCmd {
    /// Delete trashed tasks older than the given age.
    Empty {
        /// Age in days, e.g. 30d.
        #[arg(long, default_value = "30d")]
        older_than: String,
    },
}

fn run_command(cmd: Cmd, config: Option<&Config>, config_path: &std::path::Path) -> Result<()> {
    use pahiri::cli;
    if let Cmd::Config { cmd } = &cmd {
        let out = match cmd {
            ConfigCmd::AddWorkspace { name, path, main } => {
                cli::config_add_workspace(config_path, name, path, main.clone())?
            }
            ConfigCmd::AddBuild { name, path } => cli::config_add_build(config_path, name, path)?,
            ConfigCmd::RemoveWorkspace { name } => cli::config_remove(config_path, false, name)?,
            ConfigCmd::RemoveBuild { name } => cli::config_remove(config_path, true, name)?,
            ConfigCmd::Prune => cli::config_prune(config_path)?,
            ConfigCmd::List => match config {
                Some(cfg) => cli::config_list(cfg),
                None => String::new(),
            },
        };
        println!("{out}");
        return Ok(());
    }
    if let Cmd::InstallSkills { dir, force } = &cmd {
        let dir = Config::expand_tilde(&dir.display().to_string());
        print!("{}", cli::install_skills(&dir, *force)?);
        return Ok(());
    }
    let cfg = config.with_context(|| {
        format!(
            "no config at {} yet: start pahiri once to create it",
            config_path.display()
        )
    })?;
    let out = match cmd {
        Cmd::Task { cmd } => match cmd {
            TaskCmd::Ready { task, off } => cli::task_ready(cfg, &cli::resolve_task(task)?, !off)?,
            TaskCmd::Log { task, message } => {
                cli::task_log(cfg, &cli::resolve_task(task)?, &message.join(" "))?
            }
            TaskCmd::Next { task } => cli::task_next(cfg, &cli::resolve_task(task)?)?,
            TaskCmd::Move { task, to } => cli::task_move(cfg, &cli::resolve_task(task)?, &to)?,
            TaskCmd::Outcome { task, text } => {
                cli::task_outcome(cfg, &cli::resolve_task(task)?, &text.join(" "))?
            }
        },
        Cmd::Report { from, to, json } => cli::report(cfg, from.as_deref(), to.as_deref(), json)?,
        Cmd::Plan { cmd } => match cmd {
            PlanCmd::Show { date, json } => cli::plan_show(cfg, date.as_deref(), json)?,
            PlanCmd::Add { task, date, text } => {
                cli::plan_add(cfg, task, date.as_deref(), &text.join(" "))?
            }
        },
        Cmd::Trash {
            cmd: TrashCmd::Empty { older_than },
        } => cli::trash_empty(cfg, &older_than)?,
        Cmd::InstallSkills { .. } | Cmd::Config { .. } => unreachable!("handled above"),
    };
    println!("{out}");
    Ok(())
}

fn init_logging(level: &str) -> Option<PathBuf> {
    let dir = Config::state_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("pahiri.log");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;
    let filter = tracing_subscriber::EnvFilter::try_new(level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(file)
        .with_ansi(false)
        .init();
    Some(path)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let log_path = init_logging(&cli.log_level);

    // Commands run from hooks and pahiri shells use the config of the pahiri
    // that started them ($PAHIRI_CONFIG) unless --config says otherwise.
    let from_env = cli
        .cmd
        .as_ref()
        .and_then(|_| std::env::var_os("PAHIRI_CONFIG"))
        .filter(|p| !p.is_empty())
        .map(PathBuf::from);
    let config_path = match cli.config.or(from_env) {
        Some(p) => p,
        None => Config::default_path().context("cannot determine a config directory")?,
    };
    let mut config = Config::load(&config_path)?;
    if let Some(dir) = &cli.tasks_dir {
        let mut cfg = config.take().unwrap_or_default();
        cfg.tasks_dir = Config::expand_tilde(&dir.display().to_string());
        config = Some(cfg);
    }

    if let Some(cmd) = cli.cmd {
        return run_command(cmd, config.as_ref(), &config_path);
    }

    if cli.show_config {
        println!("config file: {}", config_path.display());
        if let Some(p) = &log_path {
            println!("log file:    {}", p.display());
        }
        if let Some(d) = Config::state_dir() {
            println!("state dir:   {}", d.display());
        }
        match &config {
            Some(cfg) => print!("{}", toml::to_string_pretty(cfg)?),
            None => println!("(no config yet; pahiri will open the settings page)"),
        }
        return Ok(());
    }

    info!("starting pahiri, config at {}", config_path.display());
    // Shells, agents and hooks inherit it, so `pahiri task …` / `pahiri config …`
    // run inside them use this config. Set before any thread starts.
    std::env::set_var("PAHIRI_CONFIG", &config_path);
    let state_dir = Config::state_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("run");
    std::fs::create_dir_all(&state_dir)
        .with_context(|| format!("creating {}", state_dir.display()))?;
    let (tx, rx) = EventSender::channel();
    let tasks_dir_override = cli
        .tasks_dir
        .as_ref()
        .map(|d| Config::expand_tilde(&d.display().to_string()));
    let mut app = App::new(config, config_path, state_dir, tx.clone());
    if let Some(dir) = tasks_dir_override {
        app.set_tasks_dir_override(dir);
    }

    let mut terminal = ratatui::try_init().context("initialising terminal")?;
    let _ = execute!(io::stdout(), EnableBracketedPaste);
    let mouse = app.config().mouse;
    if mouse {
        let _ = execute!(io::stdout(), EnableMouseCapture);
    }
    // The kitty keyboard protocol lets the terminal report Ctrl+Tab, Ctrl+1 and
    // friends; without it the Alt+] / Alt+1 aliases still work everywhere.
    let enhanced = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        let _ = execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }
    info!(enhanced, "keyboard enhancement");
    spawn_input_thread(tx);

    let result = run(&mut terminal, &mut app, &rx);

    app.shutdown();
    if enhanced {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    if mouse {
        let _ = execute!(io::stdout(), DisableMouseCapture);
    }
    let _ = execute!(io::stdout(), DisableBracketedPaste);
    ratatui::restore();
    result
}

fn spawn_input_thread(tx: EventSender) {
    std::thread::Builder::new()
        .name("input".into())
        .spawn(move || {
            while let Ok(ev) = crossterm::event::read() {
                tx.send(AppEvent::Input(ev));
            }
        })
        .expect("spawn input thread");
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    rx: &std::sync::mpsc::Receiver<AppEvent>,
) -> Result<()> {
    loop {
        terminal
            .draw(|f| pahiri::ui::draw(f, app))
            .context("drawing frame")?;
        let out = app.take_terminal_output();
        if !out.is_empty() {
            use std::io::Write as _;
            let mut stdout = io::stdout();
            let _ = stdout.write_all(&out);
            let _ = stdout.flush();
        }
        if app.take_bell() {
            use std::io::Write as _;
            let mut out = io::stdout();
            let _ = out.write_all(b"\x07");
            let _ = out.flush();
        }
        match rx.recv_timeout(app.tick_interval()) {
            Ok(ev) => app.handle(ev),
            Err(RecvTimeoutError::Timeout) => app.handle(AppEvent::Tick),
            Err(RecvTimeoutError::Disconnected) => break,
        }
        // Coalesce whatever else is queued so bursts of PTY output render once.
        for _ in 0..512 {
            match rx.try_recv() {
                Ok(ev) => app.handle(ev),
                Err(_) => break,
            }
        }
        if app.should_quit() {
            break;
        }
    }
    Ok(())
}
