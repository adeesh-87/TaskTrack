//! The audit (`Esc U`): fetch your tickets and Gerrit changes, match them to
//! tasks (rules, then the agent for what is left), let you review the
//! proposal in the Audit view, then create and update the tasks.

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ai::{self, AgentCall, PromptKind};
use crate::hooks::HookEvent;
use crate::tasks::audit::{
    self, change_key, AuditInput, AuditItem, KnownTask, NewTask, Overrides, Proposal, TaskUpdate,
};
use crate::tasks::context::{self as ctxfile, render_new, TaskMeta};
use crate::tasks::record::{summary_of, title_of};
use crate::tasks::sections::section_lines;
use crate::tasks::sources::{self, GerritStatus, Ticket};
use crate::tasks::GerritRef;
use crate::time::{local_offset_secs, now_rfc3339, now_secs};

use super::hooks::AfterHook;
use super::{App, AppEvent, Choice, JobEvent, Mode, Pending, Popup};

/// Reports folder inside the tasks folder.
const REPORTS: &str = ".pahiri/audit";

/// The Audit view's state.
#[derive(Debug, Clone)]
pub struct AuditView {
    /// First day looked at (`YYYY-MM-DD`).
    pub since: String,
    /// What was fetched and the tasks it was matched against.
    pub input: AuditInput,
    /// Decisions by the agent and by you.
    pub overrides: Overrides,
    /// The proposal (new tasks, updates, then what needs you).
    pub items: Vec<AuditItem>,
    /// Proposals you unticked (by key).
    pub rejected: HashSet<String>,
    /// New task ids you changed: proposed id → your id.
    pub renames: BTreeMap<String, String>,
    /// Selected item.
    pub selected: usize,
    /// Fetch problems and what the agent did.
    pub notes: Vec<String>,
    /// You changed something (Esc asks first).
    pub touched: bool,
}

impl AuditView {
    fn new(since: String, input: AuditInput, notes: Vec<String>) -> Self {
        let mut v = Self {
            since,
            input,
            overrides: Overrides::default(),
            items: Vec::new(),
            rejected: HashSet::new(),
            renames: BTreeMap::new(),
            selected: 0,
            notes,
            touched: false,
        };
        v.recompute();
        v
    }

    /// Compute the proposal again from the input and the decisions so far.
    pub fn recompute(&mut self) {
        let mut items = audit::propose(&self.input, &self.overrides);
        let rank = |p: &Proposal| match p {
            Proposal::Create(_) => 0,
            Proposal::Update(_) => 1,
            Proposal::Orphan(_) => 2,
        };
        items.sort_by(|a, b| {
            rank(&a.proposal)
                .cmp(&rank(&b.proposal))
                .then_with(|| a.proposal.key().cmp(&b.proposal.key()))
        });
        self.items = items;
        self.selected = self.selected.min(self.items.len().saturating_sub(1));
    }

    /// Whether `item` will be applied.
    pub fn accepted(&self, item: &AuditItem) -> bool {
        !matches!(item.proposal, Proposal::Orphan(_))
            && !self.rejected.contains(&item.proposal.key())
    }

    /// The id a new task gets (yours when renamed).
    pub fn id_of(&self, n: &NewTask) -> String {
        self.renames
            .get(&n.id)
            .cloned()
            .unwrap_or_else(|| n.id.clone())
    }

    /// (new tasks, updates, unmatched, ticked).
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        let mut c = (0, 0, 0, 0);
        for i in &self.items {
            match i.proposal {
                Proposal::Create(_) => c.0 += 1,
                Proposal::Update(_) => c.1 += 1,
                Proposal::Orphan(_) => c.2 += 1,
            }
            if self.accepted(i) {
                c.3 += 1;
            }
        }
        c
    }

    /// What applying `item` does, line by line (the Details pane).
    pub fn describe(&self, item: &AuditItem, columns: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        let date = |d: &Option<String>| {
            d.as_deref()
                .map_or("-", |s| s.get(..10).unwrap_or(s))
                .to_owned()
        };
        match &item.proposal {
            Proposal::Create(n) => {
                out.push(format!(
                    "Create task {} in {}",
                    self.id_of(n),
                    column_name(n.column, columns)
                ));
                if !n.title.is_empty() {
                    out.push(format!("title: {}", n.title));
                }
                out.push(format!("because: {}", n.reason));
                if let Some((source, t)) = &n.ticket {
                    out.push(format!("ticket: {} ({source}) {}", t.id, t.status));
                    if !t.url.is_empty() {
                        out.push(format!("link: {}", t.url));
                    }
                    if !t.description.trim().is_empty() {
                        out.push("## Description from the ticket".into());
                    }
                }
                out.push(format!(
                    "dates: created {} · started {} · finished {}",
                    date(&n.dates.created),
                    date(&n.dates.started),
                    date(&n.dates.finished)
                ));
                for c in &n.changes {
                    out.push(format!("CR {}", change_line(c)));
                }
            }
            Proposal::Update(u) => {
                out.push(format!("Update task {}", u.id));
                if let Some((source, t)) = &u.ticket {
                    out.push(format!("ticket: {} ({source}) {}", t.id, t.status));
                }
                for w in &u.what {
                    out.push(format!("· {w}"));
                }
            }
            Proposal::Orphan(o) => {
                out.push(format!("CR {}", change_line(&o.change)));
                if let Some(u) = &o.change.url {
                    out.push(u.clone());
                }
                out.push(o.reason.clone());
                out.push(String::new());
                out.push("Enter: attach it to a task, make a new task, or leave it.".into());
            }
        }
        if item.by_ai {
            out.push(String::new());
            out.push("(suggested by the agent)".into());
        }
        out
    }
}

/// Name of column `c`.
fn column_name(c: usize, names: &[String]) -> String {
    names.get(c).cloned().unwrap_or_else(|| c.to_string())
}

/// `#1234 MERGED fw · Fix the parser`.
pub fn change_line(c: &GerritStatus) -> String {
    let mut s = format!("#{} {}", change_key(c), c.status);
    if let Some(p) = &c.project {
        let _ = write!(s, " {p}");
    }
    if let Some(sub) = &c.subject {
        let _ = write!(s, " · {sub}");
    }
    s
}

fn gerrit_ref(c: &GerritStatus) -> GerritRef {
    GerritRef {
        workspace: c
            .project
            .as_deref()
            .map(|p| p.replace(char::is_whitespace, "-"))
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| "gerrit".into()),
        change_id: c.change_id.clone(),
        url: c.url.clone(),
        subject: c.subject.clone().unwrap_or_default(),
        status: Some(c.summary()).filter(|s| !s.is_empty()),
    }
}

/// Add changes to a task's `- gerrit:` lines, or refresh the ones it has.
fn upsert_changes(meta: &mut TaskMeta, changes: &[GerritStatus]) {
    for c in changes {
        let fresh = gerrit_ref(c);
        match meta.gerrit.iter_mut().find(|g| g.change_id == c.change_id) {
            Some(g) => {
                g.status = fresh.status;
                if g.url.is_none() {
                    g.url = fresh.url;
                }
                if g.subject.is_empty() {
                    g.subject = fresh.subject;
                }
            }
            None => meta.gerrit.push(fresh),
        }
    }
}

impl App {
    /// `Esc U`: fetch tickets and changes in the background, then match them.
    pub(super) fn start_audit(&mut self) {
        if self.store.is_none() {
            self.set_status("set up the tasks folder first");
            return;
        }
        let sources: Vec<(String, String, Option<std::path::PathBuf>)> = self
            .config
            .task_sources
            .iter()
            .filter(|s| !s.command.trim().is_empty() || s.file_path().is_some())
            .map(|s| (s.name.clone(), s.command.clone(), s.file_path()))
            .collect();
        let gerrit = self.config.audit.gerrit_command.trim().to_owned();
        let gerrit_file = self.config.audit.gerrit_file_path();
        if sources.is_empty() && gerrit.is_empty() && gerrit_file.is_none() {
            self.popup = Some(Popup::message(
                "Nothing to audit yet",
                "The audit needs your tickets and/or your Gerrit changes:\n\n\
                 · Task sources (Esc c) — the scripts that list your tickets. With\n  \
                 PAHIRI_AUDIT=1 they should include finished ones since\n  \
                 $PAHIRI_AUDIT_SINCE, with status and dates.\n\
                 · Audit: Gerrit command (Esc c) — prints your changes.\n\n\
                 Formats and examples: F1 → Audit.",
            ));
            return;
        }
        let since = crate::tasks::plan::local_date(
            now_secs().saturating_sub(self.config.audit.since_days * 86_400),
            local_offset_secs(),
        );
        let env = vec![
            ("PAHIRI_AUDIT".to_owned(), "1".to_owned()),
            ("PAHIRI_AUDIT_SINCE".to_owned(), since.clone()),
            (
                "PAHIRI_TASKS_DIR".to_owned(),
                self.config.tasks_dir.display().to_string(),
            ),
        ];
        self.popup = Some(Popup::log(format!(
            "Audit · since {}",
            crate::tasks::plan::day_label(&since)
        )));
        let events = self.events.clone();
        let spawned = std::thread::Builder::new()
            .name("audit".into())
            .spawn(move || {
                let log = |l: String| events.send(AppEvent::Job(JobEvent::Log(l)));
                let mut tickets = Vec::new();
                let mut notes = Vec::new();
                for (name, command, file) in &sources {
                    log(format!("tickets · {name} …"));
                    match sources::fetch_source(command, file.as_deref(), &env) {
                        Ok((list, note)) => {
                            log(format!(
                                "  {} ticket(s){}",
                                list.len(),
                                note.map_or_else(String::new, |n| format!(" · {n}"))
                            ));
                            tickets.extend(list.into_iter().map(|t| (name.clone(), t)));
                        }
                        Err(e) => {
                            let first = e.lines().last().unwrap_or("failed").to_owned();
                            log(format!("  FAILED: {first}"));
                            notes.push(format!("{name}: {first}"));
                        }
                    }
                }
                let mut changes = Vec::new();
                if !gerrit.is_empty() || gerrit_file.is_some() {
                    log("changes · Gerrit …".into());
                    match sources::run_or_read(&gerrit, gerrit_file.as_deref(), &env)
                        .and_then(|(o, note)| sources::parse_gerrit_status(&o).map(|l| (l, note)))
                    {
                        Ok((list, note)) => {
                            log(format!(
                                "  {} change(s){}",
                                list.len(),
                                note.map_or_else(String::new, |n| format!(" · {n}"))
                            ));
                            changes = list;
                        }
                        Err(e) => {
                            let first = e.lines().last().unwrap_or("failed").to_owned();
                            log(format!("  FAILED: {first}"));
                            notes.push(format!("Gerrit: {first}"));
                        }
                    }
                }
                events.send(AppEvent::Job(JobEvent::AuditFetched {
                    since,
                    tickets,
                    changes,
                    notes,
                }));
            });
        if let Err(e) = spawned {
            self.error(format!("could not start the audit: {e}"));
        }
    }

    /// What pahiri knows about every task on the board.
    fn known_tasks(&self) -> Vec<KnownTask> {
        let Some(store) = &self.store else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (ci, col) in store.board().columns.iter().enumerate() {
            for id in &col.tasks {
                let text = fs::read_to_string(store.context_path(id)).unwrap_or_default();
                let meta = TaskMeta::parse(&text);
                out.push(KnownTask {
                    id: id.clone(),
                    title: title_of(&text, id),
                    summary: summary_of(&text),
                    column: ci,
                    link: meta.link,
                    source: meta.source,
                    branch: meta.branch,
                    changes: meta
                        .gerrit
                        .iter()
                        .map(|g| (g.change_id.clone(), g.status.clone()))
                        .collect(),
                    created: meta.created,
                    started: meta.started,
                    finished: meta.finished,
                    has_description: !section_lines(&text, "## Description").is_empty(),
                });
            }
        }
        out
    }

    fn column_names(&self) -> Vec<String> {
        self.store.as_ref().map_or_else(Vec::new, |s| {
            s.board().columns.iter().map(|c| c.name.clone()).collect()
        })
    }

    /// Tickets and changes arrived: match them, then ask the agent about the rest.
    pub(super) fn audit_fetched(
        &mut self,
        since: String,
        tickets: Vec<(String, Ticket)>,
        changes: Vec<GerritStatus>,
        notes: Vec<String>,
    ) {
        let lower = |v: &[String]| v.iter().map(|s| s.trim().to_lowercase()).collect();
        let input = AuditInput {
            tickets,
            changes,
            tasks: self.known_tasks(),
            columns: self.column_names().len(),
            extra_done: lower(&self.config.audit.done_statuses),
            extra_progress: lower(&self.config.audit.progress_statuses),
        };
        let view = AuditView::new(since, input, notes);
        let (new, upd, orphans, _) = view.counts();
        self.log_line(format!(
            "rules: {new} new task(s), {upd} update(s), {orphans} unmatched change(s)"
        ));
        let for_agent = orphans
            + view
                .items
                .iter()
                .filter(|i| matches!(&i.proposal, Proposal::Create(n) if n.ticket.is_some()))
                .count();
        let agent = self.config.agent.command.trim().to_owned();
        if !self.config.audit.use_agent || agent.is_empty() || for_agent == 0 {
            self.open_audit_view(view);
            return;
        }
        let template_path = self
            .config
            .prompt_path(PromptKind::Audit, &self.config_path);
        let template = match ai::load_template(&template_path, PromptKind::Audit) {
            Ok(t) => t,
            Err(e) => {
                self.log_line(format!(
                    "cannot read {}: {e} · rules only",
                    template_path.display()
                ));
                self.open_audit_view(view);
                return;
            }
        };
        let vars = vec![
            ("since", crate::tasks::plan::day_label(&view.since)),
            ("items", audit::ai_request(&view.input, &view.items)),
        ];
        let prompt = ai::compose(PromptKind::Audit, &template, &vars, 0);
        let call = AgentCall {
            program: agent,
            args: self.config.agent.args.clone(),
            cwd: self.config.tasks_dir.clone(),
            env: Vec::new(),
            prompt,
            timeout: Duration::from_secs(self.config.agent.timeout_secs.max(1)),
        };
        self.log_line(format!(
            "asking the agent about {for_agent} item(s) ({} words, {}) · Esc cancels",
            ai::word_count(&call.prompt),
            template_path.display()
        ));
        let cancel = Arc::new(AtomicBool::new(false));
        self.job_cancel = Some(Arc::clone(&cancel));
        self.audit_pending = Some(view);
        let events = self.events.clone();
        let spawned = std::thread::Builder::new()
            .name("audit-agent".into())
            .spawn(move || {
                let result = ai::run(&call, &cancel, &mut |_| {});
                events.send(AppEvent::Job(JobEvent::AuditAgent { result }));
            });
        if let Err(e) = spawned {
            self.log_line(format!("could not start the agent: {e}"));
            if let Some(view) = self.audit_pending.take() {
                self.open_audit_view(view);
            }
        }
    }

    /// The agent answered (or failed): add its decisions and show the proposal.
    pub(super) fn audit_agent_done(&mut self, result: Result<String, String>) {
        self.job_cancel = None;
        let Some(mut view) = self.audit_pending.take() else {
            return;
        };
        match result {
            Ok(answer) => {
                let decided = audit::parse_ai_answer(&answer, &view.input.tasks);
                let n = decided.by_ai.len();
                view.overrides.merge(decided);
                view.recompute();
                self.log_line(format!("the agent decided {n} item(s)"));
                view.notes
                    .push(format!("the agent decided {n} item(s), marked AI"));
            }
            Err(e) => {
                let first = e.lines().last().unwrap_or("failed").to_owned();
                self.log_line(format!("agent FAILED: {first} · rules only"));
                view.notes
                    .push(format!("agent failed ({first}): rules only"));
            }
        }
        self.open_audit_view(view);
    }

    fn open_audit_view(&mut self, view: AuditView) {
        if view.items.is_empty() {
            self.log_line("");
            self.log_line("Everything is recorded: nothing to propose.");
            self.finish_log();
            return;
        }
        self.popup = None;
        self.mode = Mode::Audit(Box::new(view));
    }

    /// Keys of the Audit view.
    pub(super) fn handle_audit_key(&mut self, key: KeyEvent) {
        let Mode::Audit(view) = &mut self.mode else {
            return;
        };
        let n = view.items.len();
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char('q'), KeyModifiers::NONE) => {
                if view.touched {
                    self.popup = Some(Popup::confirm(
                        "Leave the audit?",
                        "Your decisions are not applied (a applies them).",
                        Pending::AuditDiscard,
                    ));
                } else {
                    self.mode = Mode::Home;
                }
            }
            (KeyCode::Down | KeyCode::Char('j'), _) => {
                view.selected = (view.selected + 1).min(n.saturating_sub(1));
            }
            (KeyCode::Up | KeyCode::Char('k'), _) => {
                view.selected = view.selected.saturating_sub(1);
            }
            (KeyCode::Home | KeyCode::Char('g'), _) => view.selected = 0,
            (KeyCode::End | KeyCode::Char('G'), _) => view.selected = n.saturating_sub(1),
            (KeyCode::Char(' '), _) => {
                let Some(item) = view.items.get(view.selected) else {
                    return;
                };
                if matches!(item.proposal, Proposal::Orphan(_)) {
                    self.audit_decide();
                    return;
                }
                let key = item.proposal.key();
                if !view.rejected.remove(&key) {
                    view.rejected.insert(key);
                }
                view.touched = true;
            }
            (KeyCode::Char('A'), _) => {
                let keys: Vec<String> = view
                    .items
                    .iter()
                    .filter(|i| !matches!(i.proposal, Proposal::Orphan(_)))
                    .map(|i| i.proposal.key())
                    .collect();
                if view.rejected.is_empty() {
                    view.rejected.extend(keys);
                } else {
                    view.rejected.clear();
                }
                view.touched = true;
            }
            (KeyCode::Enter, _) => self.audit_decide(),
            (KeyCode::Char('a'), _) => {
                let (_, _, _, ticked) = view.counts();
                if ticked == 0 {
                    self.set_status("nothing ticked · Space ticks a proposal");
                    return;
                }
                self.popup = Some(Popup::confirm(
                    "Apply the audit?",
                    format!(
                        "{ticked} proposal(s) will create and update tasks \
                         (CONTEXT.md, the board, dates). A report goes to <tasks>/{REPORTS}/."
                    ),
                    Pending::AuditApply,
                ));
            }
            (KeyCode::Char('?') | KeyCode::F(1), _) => {
                self.open_help(super::help::HelpTopic::Audit);
            }
            _ => {}
        }
    }

    /// Enter on a proposal: what to do with an unmatched change, or how to
    /// name / redirect a new task.
    fn audit_decide(&mut self) {
        let Mode::Audit(view) = &self.mode else {
            return;
        };
        let Some(item) = view.items.get(view.selected) else {
            return;
        };
        let choices = match &item.proposal {
            Proposal::Orphan(o) => {
                let key = change_key(&o.change);
                vec![
                    Choice {
                        label: "attach it to a task…".into(),
                        pending: Pending::AuditAttachAsk(key.clone()),
                    },
                    Choice {
                        label: "make a new task for it…".into(),
                        pending: Pending::AuditNewAsk(key.clone()),
                    },
                    Choice {
                        label: "leave it".into(),
                        pending: Pending::AuditLeave(key),
                    },
                ]
            }
            Proposal::Create(n) => {
                let mut c = vec![Choice {
                    label: format!("rename the new task ({})…", view.id_of(n)),
                    pending: Pending::AuditRenameAsk(n.id.clone()),
                }];
                if let Some((_, t)) = &n.ticket {
                    c.push(Choice {
                        label: format!("record ticket {} on an existing task instead…", t.id),
                        pending: Pending::AuditTicketAsk(t.id.clone()),
                    });
                }
                c
            }
            Proposal::Update(_) => {
                self.set_status("Space ticks / unticks this update");
                return;
            }
        };
        self.popup = Some(Popup::choose("Audit", choices));
    }

    /// Pick an existing task for `then(task)`.
    fn audit_pick_task(&mut self, title: String, then: impl Fn(String) -> Pending) {
        let Mode::Audit(view) = &self.mode else {
            return;
        };
        let choices = view
            .input
            .tasks
            .iter()
            .map(|t| Choice {
                label: if t.title.is_empty() {
                    t.id.clone()
                } else {
                    format!("{} · {}", t.id, t.title)
                },
                pending: then(t.id.clone()),
            })
            .collect::<Vec<_>>();
        if choices.is_empty() {
            self.set_status("no tasks on the board yet");
            return;
        }
        self.popup = Some(Popup::choose(title, choices));
    }

    /// A decision from the popups: record it and compute the proposal again.
    pub(super) fn audit_pending_action(&mut self, pending: Pending, input: Option<&str>) {
        match pending {
            Pending::AuditAttachAsk(key) => {
                self.audit_pick_task(format!("Attach change {key} to"), move |t| {
                    Pending::AuditAttach(key.clone(), t)
                });
            }
            Pending::AuditTicketAsk(ticket) => {
                self.audit_pick_task(format!("Record ticket {ticket} on"), move |t| {
                    Pending::AuditTicketTo(ticket.clone(), t)
                });
            }
            Pending::AuditNewAsk(key) => {
                let suggestion = match &self.mode {
                    Mode::Audit(v) => v
                        .input
                        .changes
                        .iter()
                        .find(|c| change_key(c) == key)
                        .and_then(|c| c.topic.clone())
                        .map_or_else(|| format!("CR-{key}"), |t| audit::sanitize_id(&t)),
                    _ => String::new(),
                };
                self.popup = Some(Popup::input(
                    format!("New task for change {key}"),
                    "task id (no spaces)",
                    suggestion,
                    Pending::AuditNew(key),
                ));
            }
            Pending::AuditRenameAsk(id) => {
                let current = match &self.mode {
                    Mode::Audit(v) => v.renames.get(&id).cloned().unwrap_or_else(|| id.clone()),
                    _ => id.clone(),
                };
                self.popup = Some(Popup::input(
                    format!("Rename {id}"),
                    "task id (no spaces)",
                    current,
                    Pending::AuditRename(id),
                ));
            }
            other => {
                let Mode::Audit(view) = &mut self.mode else {
                    return;
                };
                let mut o = Overrides::default();
                match other {
                    Pending::AuditAttach(key, task) => {
                        o.change_to_task.insert(key, task);
                    }
                    Pending::AuditTicketTo(ticket, task) => {
                        o.ticket_to_task.insert(ticket, task);
                    }
                    Pending::AuditLeave(key) => {
                        o.change_skip.insert(key, "you left it".into());
                    }
                    Pending::AuditNew(key) => {
                        let id = audit::sanitize_id(input.unwrap_or_default());
                        if id.is_empty() {
                            return;
                        }
                        let title = view
                            .input
                            .changes
                            .iter()
                            .find(|c| change_key(c) == key)
                            .and_then(|c| c.subject.clone())
                            .unwrap_or_default();
                        o.change_to_new.insert(key, (id, title));
                    }
                    Pending::AuditRename(id) => {
                        let new = audit::sanitize_id(input.unwrap_or_default());
                        if !new.is_empty() {
                            view.renames.insert(id, new);
                            view.touched = true;
                        }
                        return;
                    }
                    _ => return,
                }
                view.overrides.merge(o);
                view.recompute();
                view.touched = true;
            }
        }
    }

    /// `a`, confirmed: create and update the ticked tasks, write a report.
    pub(super) fn apply_audit(&mut self) {
        let Mode::Audit(view) = std::mem::replace(&mut self.mode, Mode::Home) else {
            return;
        };
        let columns = self.column_names();
        let now = now_rfc3339();
        let mut created = Vec::new();
        let mut updated = Vec::new();
        let mut problems = Vec::new();
        let mut report = format!(
            "# Audit {} (since {})\n\n",
            crate::time::short(&now),
            view.since
        );
        let mut created_md = String::new();
        let mut updated_md = String::new();
        let mut left_md = String::new();
        for item in &view.items {
            let ai = if item.by_ai { " (AI)" } else { "" };
            match &item.proposal {
                Proposal::Orphan(o) => {
                    let _ = writeln!(left_md, "- {} — {}", change_line(&o.change), o.reason);
                }
                _ if !view.accepted(item) => {}
                Proposal::Create(n) => {
                    let id = view.id_of(n);
                    match self.audit_create(&id, n, &columns, &now) {
                        Ok(()) => {
                            let _ = writeln!(
                                created_md,
                                "- {id} — {} → {}{ai} · {}",
                                n.title,
                                columns.get(n.column).map_or("?", String::as_str),
                                n.reason
                            );
                            created.push(id);
                        }
                        Err(e) => problems.push(format!("{id}: {e}")),
                    }
                }
                Proposal::Update(u) => match self.audit_update(u, &columns, &now) {
                    Ok(()) => {
                        let _ = writeln!(updated_md, "- {}{ai}: {}", u.id, u.what.join(" · "));
                        updated.push(u.id.clone());
                    }
                    Err(e) => problems.push(format!("{}: {e}", u.id)),
                },
            }
        }
        for (title, body) in [
            ("Created", &created_md),
            ("Updated", &updated_md),
            ("Left alone", &left_md),
        ] {
            if !body.is_empty() {
                let _ = write!(report, "## {title}\n\n{body}\n");
            }
        }
        if !problems.is_empty() {
            let _ = write!(report, "## Problems\n\n- {}\n", problems.join("\n- "));
        }
        let dir = self.config.tasks_dir.join(REPORTS);
        let report_path = dir.join(format!("{}.md", now.replace(':', "")));
        if let Err(e) = fs::create_dir_all(&dir).and_then(|()| fs::write(&report_path, &report)) {
            problems.push(format!("report: {e}"));
        }
        if let Some(store) = &mut self.store {
            let _ = store.refresh();
        }
        for id in created.iter().chain(&updated) {
            self.after_task_file_change(id);
        }
        self.refresh_next_up();
        self.fix_list_selection();
        let mut status = format!(
            "audit applied: {} created, {} updated · report {}",
            created.len(),
            updated.len(),
            report_path.display()
        );
        if let Some(p) = problems.first() {
            let _ = write!(status, " · {} problem(s): {p}", problems.len());
        }
        self.set_status(status);
        self.fire_hook(
            HookEvent::AuditApply,
            None,
            vec![
                ("PAHIRI_AUDIT_CREATED".into(), created.join(" ")),
                ("PAHIRI_AUDIT_UPDATED".into(), updated.join(" ")),
                (
                    "PAHIRI_AUDIT_REPORT".into(),
                    report_path.display().to_string(),
                ),
            ],
            AfterHook::Nothing,
        );
    }

    fn audit_create(
        &mut self,
        id: &str,
        n: &NewTask,
        columns: &[String],
        now: &str,
    ) -> Result<(), String> {
        let (source, ticket) = if let Some((s, t)) = &n.ticket {
            (s.clone(), t.clone())
        } else {
            // A task made from changes: they are its description.
            let mut description = String::from("Changes:\n");
            for c in &n.changes {
                let _ = writeln!(description, "- {}", change_line(c));
            }
            (
                "gerrit".to_owned(),
                Ticket {
                    id: id.to_owned(),
                    title: n.title.clone(),
                    description,
                    ..Ticket::default()
                },
            )
        };
        let created = n.dates.created.clone().unwrap_or_else(|| now.to_owned());
        let text = render_new(id, Some(&source), Some(&ticket), &created);
        let store = self.store.as_mut().ok_or("no tasks folder")?;
        let summary = store
            .create_task_with_context(id, n.column, &text)
            .map_err(|e| e.to_string())?;
        let path = summary.dir.join(&self.config.context_file);
        let last = columns.len().saturating_sub(1);
        ctxfile::update_meta(&path, id, |m| {
            m.started.clone_from(&n.dates.started);
            m.finished = n
                .dates
                .finished
                .clone()
                .or_else(|| (columns.len() > 1 && n.column == last).then(|| now.to_owned()));
            if m.started.is_none() && n.column > 0 {
                m.started = m.finished.clone().or_else(|| Some(now.to_owned()));
            }
            upsert_changes(m, &n.changes);
        })
        .and_then(|_| {
            ctxfile::append_log(&path, id, now, &format!("audit: created — {}", n.reason))
        })
        .map_err(|e| e.to_string())
    }

    fn audit_update(
        &mut self,
        u: &TaskUpdate,
        columns: &[String],
        now: &str,
    ) -> Result<(), String> {
        let path = self.context_path(&u.id).ok_or("no tasks folder")?;
        let last = columns.len().saturating_sub(1);
        if let Some(to) = u.move_to {
            let store = self.store.as_mut().ok_or("no tasks folder")?;
            store.move_task(&u.id, to).map_err(|e| e.to_string())?;
        }
        ctxfile::update_meta(&path, &u.id, |m| {
            if let Some(d) = &u.dates.created {
                m.created = Some(d.clone());
            }
            if let Some(d) = &u.dates.started {
                m.started = Some(d.clone());
            }
            if let Some(d) = &u.dates.finished {
                m.finished = Some(d.clone());
            }
            if let Some(to) = u.move_to {
                if to > 0 && m.started.is_none() {
                    m.started = Some(now.to_owned());
                }
                if columns.len() > 1 && to == last && m.finished.is_none() {
                    m.finished = Some(now.to_owned());
                }
            }
            if let Some(l) = &u.link {
                m.link = Some(l.clone());
            }
            if let Some((source, _)) = &u.ticket {
                if m.source.is_none() {
                    m.source = Some(source.clone());
                }
            }
            upsert_changes(m, &u.changes);
        })
        .map_err(|e| e.to_string())?;
        if let Some(d) = &u.description {
            ctxfile::refresh_description(&path, &u.id, d).map_err(|e| e.to_string())?;
        }
        ctxfile::append_log(&path, &u.id, now, &format!("audit: {}", u.what.join(" · ")))
            .map_err(|e| e.to_string())
    }
}
