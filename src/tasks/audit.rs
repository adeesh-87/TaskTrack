//! The audit: your tickets and Gerrit changes, matched to the tasks you have,
//! as a list of proposals (create, update, or "needs you") to review.
//!
//! Matching is deterministic first — a ticket by task id or link, a change by
//! a recorded `Change-Id`, its topic / branch, or a ticket key in its subject.
//! What is left can be decided by the agent ([`Overrides`] from
//! [`parse_ai_answer`]) or by you; both only add overrides and the proposal is
//! computed again, so nothing is applied until you say so.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::sources::{GerritStatus, Ticket};
use crate::time::normalize;

/// Statuses that mean "finished" when a script does not say `done`.
pub const DONE_STATUSES: &[&str] = &[
    "done",
    "closed",
    "resolved",
    "fixed",
    "complete",
    "completed",
    "verified",
    "released",
    "merged",
];
/// Statuses that mean "being worked on".
pub const PROGRESS_STATUSES: &[&str] = &[
    "in progress",
    "in review",
    "review",
    "in development",
    "implementing",
    "doing",
    "started",
    "code review",
    "testing",
];

/// What pahiri knows about an existing task.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnownTask {
    /// Folder name.
    pub id: String,
    /// Title.
    pub title: String,
    /// One line about it.
    pub summary: String,
    /// Board column index.
    pub column: usize,
    /// Ticket link.
    pub link: Option<String>,
    /// Ticket source.
    pub source: Option<String>,
    /// Task branch.
    pub branch: Option<String>,
    /// Recorded changes: `Change-Id` and status.
    pub changes: Vec<(String, Option<String>)>,
    /// Dates (RFC 3339).
    pub created: Option<String>,
    /// Started.
    pub started: Option<String>,
    /// Finished.
    pub finished: Option<String>,
    /// Whether `## Description` has text.
    pub has_description: bool,
}

/// Everything the audit looks at.
#[derive(Debug, Clone, Default)]
pub struct AuditInput {
    /// Tickets with the name of their source.
    pub tickets: Vec<(String, Ticket)>,
    /// Your Gerrit changes.
    pub changes: Vec<GerritStatus>,
    /// Existing tasks.
    pub tasks: Vec<KnownTask>,
    /// Number of board columns.
    pub columns: usize,
    /// More statuses meaning done (lower case).
    pub extra_done: Vec<String>,
    /// More statuses meaning in progress (lower case).
    pub extra_progress: Vec<String>,
}

impl AuditInput {
    fn is_done_status(&self, status: &str) -> bool {
        let s = status.trim().to_lowercase();
        DONE_STATUSES.contains(&s.as_str()) || self.extra_done.contains(&s)
    }

    fn is_progress_status(&self, status: &str) -> bool {
        let s = status.trim().to_lowercase();
        PROGRESS_STATUSES.contains(&s.as_str()) || self.extra_progress.contains(&s)
    }

    fn ticket_done(&self, t: &Ticket) -> bool {
        t.done.unwrap_or_else(|| self.is_done_status(&t.status))
    }
}

/// Dates to record (RFC 3339 UTC).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dates {
    /// Created.
    pub created: Option<String>,
    /// Started.
    pub started: Option<String>,
    /// Finished.
    pub finished: Option<String>,
}

/// A task to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTask {
    /// Folder name.
    pub id: String,
    /// Title.
    pub title: String,
    /// The ticket, with its source name.
    pub ticket: Option<(String, Ticket)>,
    /// Changes that belong to it.
    pub changes: Vec<GerritStatus>,
    /// Board column to create it in.
    pub column: usize,
    /// Dates to record.
    pub dates: Dates,
    /// Why.
    pub reason: String,
}

/// Changes to an existing task.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskUpdate {
    /// Task.
    pub id: String,
    /// Ticket linked to it (for link, source and description).
    pub ticket: Option<(String, Ticket)>,
    /// Changes to add, or whose status changed.
    pub changes: Vec<GerritStatus>,
    /// Move to this column.
    pub move_to: Option<usize>,
    /// Dates to set.
    pub dates: Dates,
    /// Record this link.
    pub link: Option<String>,
    /// Fill the empty `## Description`.
    pub description: Option<String>,
    /// What changes, in words.
    pub what: Vec<String>,
}

/// A change nothing claimed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Orphan {
    /// The change.
    pub change: GerritStatus,
    /// Why it is here (or what the agent said).
    pub reason: String,
}

/// One proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Proposal {
    /// Create a task.
    Create(NewTask),
    /// Update a task.
    Update(TaskUpdate),
    /// A change that needs you: attach it, make a task, or leave it.
    Orphan(Orphan),
}

impl Proposal {
    /// Stable key (survives recomputing the proposal).
    pub fn key(&self) -> String {
        match self {
            Self::Create(n) => format!("new:{}", n.id),
            Self::Update(u) => format!("update:{}", u.id),
            Self::Orphan(o) => format!("orphan:{}", change_key(&o.change)),
        }
    }
}

/// A proposal and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditItem {
    /// What to do.
    pub proposal: Proposal,
    /// Suggested by the agent (not by a rule).
    pub by_ai: bool,
}

/// Decisions on top of the rules, by the agent or by you.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    /// Ticket id → existing task.
    pub ticket_to_task: BTreeMap<String, String>,
    /// Change key → existing task.
    pub change_to_task: BTreeMap<String, String>,
    /// Change key → new task (id, title).
    pub change_to_new: BTreeMap<String, (String, String)>,
    /// Change key → leave it (reason).
    pub change_skip: BTreeMap<String, String>,
    /// Keys decided by the agent (`T:…` / `C:…`).
    pub by_ai: HashSet<String>,
}

impl Overrides {
    /// Add `other` on top (its decisions win).
    pub fn merge(&mut self, other: Overrides) {
        for key in other.ticket_to_task.keys() {
            self.by_ai.remove(&format!("T:{key}"));
        }
        for key in other
            .change_to_task
            .keys()
            .chain(other.change_to_new.keys())
            .chain(other.change_skip.keys())
        {
            self.change_to_task.remove(key);
            self.change_to_new.remove(key);
            self.change_skip.remove(key);
            self.by_ai.remove(&format!("C:{key}"));
        }
        self.ticket_to_task.extend(other.ticket_to_task);
        self.change_to_task.extend(other.change_to_task);
        self.change_to_new.extend(other.change_to_new);
        self.change_skip.extend(other.change_skip);
        self.by_ai.extend(other.by_ai);
    }
}

/// How a change is named in prompts and overrides: its number, else its `Change-Id`.
pub fn change_key(c: &GerritStatus) -> String {
    c.number
        .map_or_else(|| c.change_id.clone(), |n| n.to_string())
}

fn change_merged(c: &GerritStatus) -> bool {
    c.status.eq_ignore_ascii_case("MERGED")
}

fn change_closed(c: &GerritStatus) -> bool {
    change_merged(c) || c.status.eq_ignore_ascii_case("ABANDONED")
}

/// Ticket-key-like words (`PROJ-42`) in `text`, upper-cased.
pub fn ticket_keys(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .filter_map(|w| {
            let (head, num) = w.rsplit_once('-')?;
            let ok = head.len() >= 2
                && head.starts_with(|c: char| c.is_ascii_alphabetic())
                && head.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !num.is_empty()
                && num.chars().all(|c| c.is_ascii_digit());
            ok.then(|| w.to_uppercase())
        })
        .collect()
}

/// Where a ticket or change goes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Target {
    Existing(String),
    New(String),
}

#[derive(Default)]
struct Group {
    ticket: Option<(String, Ticket)>,
    changes: Vec<GerritStatus>,
    title: Option<String>,
    by_ai: bool,
}

/// Compute the proposals.
pub fn propose(input: &AuditInput, overrides: &Overrides) -> Vec<AuditItem> {
    let by_id: HashMap<String, &KnownTask> = input
        .tasks
        .iter()
        .map(|t| (t.id.to_uppercase(), t))
        .collect();
    let by_link: HashMap<&str, &KnownTask> = input
        .tasks
        .iter()
        .filter_map(|t| t.link.as_deref().filter(|l| !l.is_empty()).map(|l| (l, t)))
        .collect();
    let by_change: HashMap<&str, &KnownTask> = input
        .tasks
        .iter()
        .flat_map(|t| t.changes.iter().map(move |(c, _)| (c.as_str(), t)))
        .collect();
    let by_branch: HashMap<String, &KnownTask> = input
        .tasks
        .iter()
        .filter_map(|t| t.branch.as_ref().map(|b| (b.to_uppercase(), t)))
        .collect();

    // Tickets.
    let mut groups: BTreeMap<Target, Group> = BTreeMap::new();
    let mut ticket_target: HashMap<String, Target> = HashMap::new();
    for (source, t) in &input.tickets {
        let tid = t.task_id();
        let forced = overrides.ticket_to_task.get(&t.id);
        let target = match forced {
            Some(task) => Target::Existing(task.clone()),
            None => by_id
                .get(&tid.to_uppercase())
                .or_else(|| by_link.get(t.url.as_str()).filter(|_| !t.url.is_empty()))
                .map_or_else(
                    || Target::New(tid.clone()),
                    |k| Target::Existing(k.id.clone()),
                ),
        };
        ticket_target.insert(t.id.to_uppercase(), target.clone());
        ticket_target.insert(tid.to_uppercase(), target.clone());
        let g = groups.entry(target).or_default();
        g.ticket = Some((source.clone(), t.clone()));
        g.by_ai |= forced.is_some() && overrides.by_ai.contains(&format!("T:{}", t.id));
    }

    // Changes.
    let mut orphans = Vec::new();
    let mut seen = HashSet::new();
    for c in &input.changes {
        if !seen.insert(c.change_id.clone()) {
            continue;
        }
        let key = change_key(c);
        let ai = overrides.by_ai.contains(&format!("C:{key}"));
        if let Some(reason) = overrides.change_skip.get(&key) {
            orphans.push((c.clone(), reason.clone(), ai));
            continue;
        }
        if let Some(task) = overrides.change_to_task.get(&key) {
            let g = groups.entry(Target::Existing(task.clone())).or_default();
            g.changes.push(c.clone());
            g.by_ai |= ai;
            continue;
        }
        if let Some((id, title)) = overrides.change_to_new.get(&key) {
            let g = groups.entry(Target::New(id.clone())).or_default();
            g.changes.push(c.clone());
            g.title.get_or_insert_with(|| title.clone());
            g.by_ai |= ai;
            continue;
        }
        let named = [c.topic.as_deref(), c.branch.as_deref()]
            .into_iter()
            .flatten()
            .map(str::to_uppercase)
            .chain(ticket_keys(c.subject.as_deref().unwrap_or_default()))
            .chain(c.topic.as_deref().map(ticket_keys).unwrap_or_default());
        let mut target = by_change
            .get(c.change_id.as_str())
            .map(|k| Target::Existing(k.id.clone()));
        for name in named {
            if target.is_some() {
                break;
            }
            target = by_id
                .get(&name)
                .or_else(|| by_branch.get(&name))
                .map(|k| Target::Existing(k.id.clone()))
                .or_else(|| ticket_target.get(&name).cloned());
        }
        match target {
            Some(t) => groups.entry(t).or_default().changes.push(c.clone()),
            None => orphans.push((
                c.clone(),
                "no task, ticket or ticket key in its subject / topic".to_owned(),
                false,
            )),
        }
    }

    let last = input.columns.saturating_sub(1);
    let progress_col = usize::from(input.columns >= 3);
    let mut items = Vec::new();
    for (target, g) in groups {
        let ticket = g.ticket.as_ref().map(|(_, t)| t);
        let done = match ticket {
            Some(t) => input.ticket_done(t),
            None => {
                !g.changes.is_empty()
                    && g.changes.iter().all(change_closed)
                    && g.changes.iter().any(change_merged)
            }
        };
        let progress = !done
            && (ticket.is_some_and(|t| input.is_progress_status(&t.status))
                || g.changes.iter().any(|c| !change_closed(c)));
        let dates = dates_of(ticket, &g.changes, done);
        match target {
            Target::New(id) => {
                let title = ticket
                    .map(|t| t.title.clone())
                    .filter(|t| !t.is_empty())
                    .or(g.title)
                    .or_else(|| g.changes.iter().find_map(|c| c.subject.clone()))
                    .unwrap_or_default();
                let reason = match &g.ticket {
                    Some((source, t)) => format!(
                        "ticket {} ({source}){}",
                        t.id,
                        if t.status.is_empty() {
                            String::new()
                        } else {
                            format!(", {}", t.status)
                        }
                    ),
                    None => format!("{} change(s) without a ticket", g.changes.len()),
                };
                items.push(AuditItem {
                    proposal: Proposal::Create(NewTask {
                        id,
                        title,
                        ticket: g.ticket,
                        changes: g.changes,
                        column: if done {
                            last
                        } else if progress {
                            progress_col
                        } else {
                            0
                        },
                        dates,
                        reason,
                    }),
                    by_ai: g.by_ai,
                });
            }
            Target::Existing(id) => {
                let Some(task) = by_id.get(&id.to_uppercase()) else {
                    continue;
                };
                let update = update_of(task, g.ticket, g.changes, dates, done, progress, input);
                if let Some(u) = update {
                    items.push(AuditItem {
                        proposal: Proposal::Update(u),
                        by_ai: g.by_ai,
                    });
                }
            }
        }
    }
    items.extend(
        orphans
            .into_iter()
            .map(|(change, reason, by_ai)| AuditItem {
                proposal: Proposal::Orphan(Orphan { change, reason }),
                by_ai,
            }),
    );
    items
}

/// Dates from a ticket, else from its changes.
fn dates_of(ticket: Option<&Ticket>, changes: &[GerritStatus], done: bool) -> Dates {
    let earliest = |f: fn(&GerritStatus) -> Option<&String>| {
        changes
            .iter()
            .filter_map(|c| f(c).and_then(|d| normalize(d)))
            .min()
    };
    let first_upload = earliest(|c| c.created.as_ref());
    let created = ticket
        .and_then(|t| t.created.as_deref())
        .and_then(normalize)
        .or_else(|| first_upload.clone());
    let started = ticket
        .and_then(|t| t.started.as_deref())
        .and_then(normalize)
        .or(first_upload);
    let finished = done
        .then(|| {
            ticket
                .and_then(|t| t.finished.as_deref())
                .and_then(normalize)
                .or_else(|| {
                    changes
                        .iter()
                        .filter(|c| change_merged(c))
                        .filter_map(|c| c.merged.as_deref().or(c.updated.as_deref()))
                        .filter_map(normalize)
                        .max()
                })
        })
        .flatten();
    Dates {
        created,
        started,
        finished,
    }
}

/// Earlier evidence wins for created / started; finished only fills a gap.
fn earlier(existing: Option<&String>, found: Option<String>) -> Option<String> {
    match (existing, found) {
        (None, Some(f)) => Some(f),
        (Some(e), Some(f)) if f < *e => Some(f),
        _ => None,
    }
}

fn update_of(
    task: &KnownTask,
    ticket: Option<(String, Ticket)>,
    changes: Vec<GerritStatus>,
    dates: Dates,
    done: bool,
    progress: bool,
    input: &AuditInput,
) -> Option<TaskUpdate> {
    let mut u = TaskUpdate {
        id: task.id.clone(),
        ..TaskUpdate::default()
    };
    for c in changes {
        let status = c.summary_status();
        match task.changes.iter().find(|(id, _)| *id == c.change_id) {
            None => {
                u.what.push(format!(
                    "+ CR {} {}",
                    status,
                    c.subject.as_deref().unwrap_or(&c.change_id)
                ));
                u.changes.push(c);
            }
            Some((_, old)) if old.as_deref().unwrap_or("") != status => {
                u.what.push(format!(
                    "CR {} → {status}",
                    c.subject.as_deref().unwrap_or(&c.change_id)
                ));
                u.changes.push(c);
            }
            Some(_) => {}
        }
    }
    if let Some((_, t)) = &ticket {
        if task.link.is_none() && !t.url.is_empty() {
            u.link = Some(t.url.clone());
            u.what.push(format!("link {}", t.url));
        }
        if !task.has_description && !t.description.trim().is_empty() {
            u.description = Some(t.description.clone());
            u.what.push("## Description from the ticket".into());
        }
    }
    let last = input.columns.saturating_sub(1);
    if done && input.columns > 1 && task.column != last {
        u.move_to = Some(last);
        u.what.push("→ last column (done)".into());
    } else if progress && task.column == 0 && input.columns >= 3 {
        u.move_to = Some(1);
        u.what.push("→ in progress".into());
    }
    u.dates = Dates {
        created: earlier(task.created.as_ref(), dates.created),
        started: earlier(task.started.as_ref(), dates.started),
        finished: if task.finished.is_none() && done {
            dates.finished
        } else {
            None
        },
    };
    for (name, d) in [
        ("created", &u.dates.created),
        ("started", &u.dates.started),
        ("finished", &u.dates.finished),
    ] {
        if let Some(d) = d {
            u.what.push(format!("{name} {}", &d[..10]));
        }
    }
    u.ticket = ticket;
    (!u.what.is_empty()).then_some(u)
}

impl GerritStatus {
    /// Status as stored on a `- gerrit:` line: `MERGED #1234 CR+2`.
    pub fn summary_status(&self) -> String {
        self.summary()
    }
}

/// The part of the audit prompt that lists what the agent should decide.
pub fn ai_request(input: &AuditInput, items: &[AuditItem]) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("UNMATCHED CHANGES (key, project, branch/topic, status, subject)\n");
    let mut any = false;
    for item in items {
        if let Proposal::Orphan(o) = &item.proposal {
            any = true;
            let c = &o.change;
            let _ = writeln!(
                out,
                "- C:{} | {} | {}{} | {} | {}",
                change_key(c),
                c.project.as_deref().unwrap_or("-"),
                c.branch.as_deref().unwrap_or("-"),
                c.topic
                    .as_deref()
                    .map_or_else(String::new, |t| format!(" / {t}")),
                c.status,
                c.subject.as_deref().unwrap_or("")
            );
        }
    }
    if !any {
        out.push_str("(none)\n");
    }
    out.push_str("\nTICKETS WITHOUT A TASK (key, status, title — description)\n");
    let mut any = false;
    for item in items {
        if let Proposal::Create(n) = &item.proposal {
            if let Some((_, t)) = &n.ticket {
                any = true;
                let desc: String = t.description.chars().take(160).collect();
                let _ = writeln!(
                    out,
                    "- T:{} | {} | {} — {}",
                    t.id,
                    t.status,
                    t.title,
                    desc.replace('\n', " ")
                );
            }
        }
    }
    if !any {
        out.push_str("(none)\n");
    }
    out.push_str("\nEXISTING TASKS (id, title — summary)\n");
    for t in &input.tasks {
        let _ = writeln!(out, "- {} | {} — {}", t.id, t.title, t.summary);
    }
    out
}

/// Read the agent's answer (see `AUDIT_FORMAT` in `crate::ai`):
/// `MAP <key> -> <task>`, `NEW <key> [<key>…] -> <id> | <title>`, `SKIP <key> | <why>`.
/// Unknown tasks and malformed lines are ignored.
pub fn parse_ai_answer(answer: &str, tasks: &[KnownTask]) -> Overrides {
    let known: HashSet<String> = tasks.iter().map(|t| t.id.clone()).collect();
    let mut o = Overrides::default();
    for line in answer.lines() {
        let line = line.trim().trim_start_matches(['-', '*']).trim();
        let (head, note) = line
            .split_once('|')
            .map_or((line, ""), |(h, n)| (h.trim(), n.trim()));
        let mut words = head.split_whitespace();
        let verb = words.next().unwrap_or("").to_uppercase();
        let rest: Vec<&str> = words.collect();
        match verb.as_str() {
            "MAP" => {
                let [key, "->", task] = rest[..] else {
                    continue;
                };
                if !known.contains(task) {
                    continue;
                }
                if let Some(t) = key.strip_prefix("T:") {
                    o.ticket_to_task.insert(t.to_owned(), task.to_owned());
                    o.by_ai.insert(key.to_owned());
                } else if let Some(c) = key.strip_prefix("C:") {
                    o.change_to_task.insert(c.to_owned(), task.to_owned());
                    o.by_ai.insert(key.to_owned());
                }
            }
            "NEW" => {
                let Some(arrow) = rest.iter().position(|w| *w == "->") else {
                    continue;
                };
                let Some(id) = rest.get(arrow + 1).map(|s| sanitize_id(s)) else {
                    continue;
                };
                if id.is_empty() || known.contains(&id) {
                    continue;
                }
                for key in &rest[..arrow] {
                    if let Some(c) = key.strip_prefix("C:") {
                        o.change_to_new
                            .insert(c.to_owned(), (id.clone(), note.to_owned()));
                        o.by_ai.insert((*key).to_owned());
                    }
                }
            }
            "SKIP" => {
                for key in &rest {
                    if let Some(c) = key.strip_prefix("C:") {
                        let why = if note.is_empty() {
                            "the agent says skip"
                        } else {
                            note
                        };
                        o.change_skip.insert(c.to_owned(), format!("AI: {why}"));
                        o.by_ai.insert((*key).to_owned());
                    }
                }
            }
            _ => {}
        }
    }
    o
}

/// A usable task id: letters, digits, `-`, `_`, `.`.
pub fn sanitize_id(s: &str) -> String {
    Ticket {
        id: s.to_owned(),
        ..Ticket::default()
    }
    .task_id()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket(id: &str, status: &str) -> (String, Ticket) {
        (
            "jira".into(),
            Ticket {
                id: id.into(),
                title: format!("{id} title"),
                url: format!("https://jira/browse/{id}"),
                description: "What to do.".into(),
                status: status.into(),
                created: Some("2026-01-10T08:00:00.000+0000".into()),
                finished: (status == "Done").then(|| "2026-02-01 10:00:00".into()),
                ..Ticket::default()
            },
        )
    }

    fn change(n: u64, status: &str, subject: &str, topic: Option<&str>) -> GerritStatus {
        GerritStatus {
            change_id: format!("I{n:040}"),
            number: Some(n),
            status: status.into(),
            subject: Some(subject.into()),
            topic: topic.map(str::to_owned),
            project: Some("fw".into()),
            created: Some("2026-01-15 09:00:00.000000000".into()),
            updated: Some("2026-01-20 09:00:00.000000000".into()),
            ..GerritStatus::default()
        }
    }

    fn task(id: &str, column: usize) -> KnownTask {
        KnownTask {
            id: id.into(),
            title: format!("{id} title"),
            column,
            created: Some("2026-03-01T00:00:00Z".into()),
            ..KnownTask::default()
        }
    }

    fn input() -> AuditInput {
        AuditInput {
            tickets: vec![
                ticket("PROJ-1", "Done"),
                ticket("PROJ-2", "In Progress"),
                ticket("PROJ-3", "To Do"),
            ],
            changes: vec![
                change(11, "MERGED", "PROJ-1: fix the parser", None),
                change(12, "NEW", "Add tests", Some("PROJ-2")),
                change(13, "NEW", "Refactor the dts", None),
                change(14, "MERGED", "Bump version", None),
            ],
            tasks: vec![task("PROJ-2", 0), task("cleanup", 1)],
            columns: 3,
            ..AuditInput::default()
        }
    }

    fn find<'a>(items: &'a [AuditItem], key: &str) -> &'a AuditItem {
        items
            .iter()
            .find(|i| i.proposal.key() == key)
            .unwrap_or_else(|| panic!("no {key} in {items:#?}"))
    }

    #[test]
    fn rules_match_tickets_and_changes() {
        let items = propose(&input(), &Overrides::default());
        // PROJ-1: new, done, with its merged change, dates from the ticket.
        let Proposal::Create(n) = &find(&items, "new:PROJ-1").proposal else {
            panic!()
        };
        assert_eq!(n.column, 2);
        assert_eq!(n.changes.len(), 1);
        assert_eq!(n.dates.created.as_deref(), Some("2026-01-10T08:00:00Z"));
        assert_eq!(n.dates.started.as_deref(), Some("2026-01-15T09:00:00Z"));
        assert_eq!(n.dates.finished.as_deref(), Some("2026-02-01T10:00:00Z"));
        assert!(n.reason.contains("ticket PROJ-1 (jira), Done"));
        // PROJ-3: new, to do → first column.
        let Proposal::Create(n) = &find(&items, "new:PROJ-3").proposal else {
            panic!()
        };
        assert_eq!((n.column, n.dates.finished.as_ref()), (0, None));
        // PROJ-2 exists: gets the change by topic, the link, the description,
        // earlier dates, and moves to "in progress".
        let Proposal::Update(u) = &find(&items, "update:PROJ-2").proposal else {
            panic!()
        };
        assert_eq!(u.changes.len(), 1);
        assert_eq!(u.move_to, Some(1));
        assert!(u.link.is_some() && u.description.is_some());
        assert_eq!(u.dates.created.as_deref(), Some("2026-01-10T08:00:00Z"));
        assert_eq!(u.dates.finished, None);
        // Two changes nobody claimed.
        find(&items, "orphan:13");
        find(&items, "orphan:14");
        assert_eq!(items.len(), 5);
    }

    #[test]
    fn nothing_new_means_no_update() {
        let mut inp = input();
        inp.tickets.retain(|(_, t)| t.id == "PROJ-2");
        inp.changes.retain(|c| c.number == Some(12));
        inp.tasks[0] = KnownTask {
            link: Some("https://jira/browse/PROJ-2".into()),
            has_description: true,
            changes: vec![(inp.changes[0].change_id.clone(), Some("NEW #12".into()))],
            column: 1,
            created: Some("2026-01-01T00:00:00Z".into()),
            started: Some("2026-01-01T00:00:00Z".into()),
            ..inp.tasks[0].clone()
        };
        assert!(propose(&inp, &Overrides::default()).is_empty());
        // A status change is news.
        inp.changes[0].status = "MERGED".into();
        let items = propose(&inp, &Overrides::default());
        let Proposal::Update(u) = &items[0].proposal else {
            panic!()
        };
        assert_eq!(u.what, ["CR Add tests → MERGED #12"]);
    }

    #[test]
    fn the_agent_decides_what_is_left() {
        let inp = input();
        let answer = "Here you go:\n\
            MAP C:13 -> cleanup | dts work is the cleanup task\n\
            NEW C:14 -> release-bump | Version bumps\n\
            MAP T:PROJ-3 -> cleanup | same thing\n\
            MAP C:99 -> nowhere\n\
            SKIP C:12 | noise\n\
            garbage line";
        let o = parse_ai_answer(answer, &inp.tasks);
        assert_eq!(
            o.change_to_task.get("13").map(String::as_str),
            Some("cleanup")
        );
        assert_eq!(
            o.ticket_to_task.get("PROJ-3").map(String::as_str),
            Some("cleanup")
        );
        assert!(!o.change_to_task.contains_key("99"), "unknown task ignored");
        let items = propose(&inp, &o);
        let Proposal::Update(u) = &find(&items, "update:cleanup").proposal else {
            panic!()
        };
        assert!(find(&items, "update:cleanup").by_ai);
        assert_eq!(u.changes.len(), 1);
        assert_eq!(u.ticket.as_ref().unwrap().1.id, "PROJ-3");
        let Proposal::Create(n) = &find(&items, "new:release-bump").proposal else {
            panic!()
        };
        assert_eq!(n.title, "Version bumps");
        assert_eq!(n.column, 2, "merged only → done");
        assert_eq!(n.dates.finished.as_deref(), Some("2026-01-20T09:00:00Z"));
        let Proposal::Orphan(skip) = &find(&items, "orphan:12").proposal else {
            panic!()
        };
        assert_eq!(skip.reason, "AI: noise");
        assert!(!items.iter().any(|i| i.proposal.key() == "new:PROJ-3"));
        // Your own decision replaces the agent's.
        let mut mine = o.clone();
        mine.merge(Overrides {
            change_to_task: [("12".to_owned(), "PROJ-2".to_owned())].into(),
            ..Overrides::default()
        });
        assert!(!mine.change_skip.contains_key("12"));
        assert!(!mine.by_ai.contains("C:12"));
    }

    #[test]
    fn ticket_keys_and_ids() {
        assert_eq!(
            ticket_keys("PROJ-12: fix; see abc-3 and x-1, v1.2-3"),
            ["PROJ-12", "ABC-3"]
        );
        assert_eq!(sanitize_id("Release bump!"), "Release-bump");
    }
}
