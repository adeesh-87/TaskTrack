//! Non-interactive commands for scripts, agents and reviews.
//!
//! They work on the same files as the TUI (and through the same functions),
//! so a running pahiri picks up their changes on its next tick.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _, Result};

use crate::config::Config;
use crate::tasks::checkpoints::{self, fmt_minutes};
use crate::tasks::context::{
    append_log, append_under, record_move, set_context_ready, OUTCOME_HEADING,
};
use crate::tasks::plan::{self, PlanItem};
use crate::tasks::record::TaskRecord;
use crate::tasks::store::{discover, empty_trash, is_valid_id, TaskStore};
use crate::tasks::Board;
use crate::time::now_rfc3339;

/// Skills bundled with pahiri (`name`, `SKILL.md` text).
pub const SKILLS: &[(&str, &str)] = &[
    ("less", include_str!("../.agents/skills/less/SKILL.md")),
    (
        "plan-first",
        include_str!("../.agents/skills/plan-first/SKILL.md"),
    ),
    (
        "task-retro",
        include_str!("../.agents/skills/task-retro/SKILL.md"),
    ),
    (
        "standup",
        include_str!("../.agents/skills/standup/SKILL.md"),
    ),
    (
        "unstick",
        include_str!("../.agents/skills/unstick/SKILL.md"),
    ),
];

/// The task to act on: `explicit`, else `$PAHIRI_TASK`.
pub fn resolve_task(explicit: Option<String>) -> Result<String> {
    let id = explicit
        .or_else(|| std::env::var("PAHIRI_TASK").ok())
        .filter(|s| !s.trim().is_empty())
        .context("no task: pass --task ID or run inside a pahiri shell ($PAHIRI_TASK)")?;
    if !is_valid_id(&id) {
        bail!("invalid task id {id:?}");
    }
    Ok(id)
}

fn context_path(cfg: &Config, id: &str) -> Result<PathBuf> {
    let dir = cfg.tasks_dir.join(id);
    if !dir.is_dir() {
        bail!("no task folder {}", dir.display());
    }
    Ok(dir.join(&cfg.context_file))
}

/// `pahiri task ready [--off]`.
pub fn task_ready(cfg: &Config, id: &str, ready: bool) -> Result<String> {
    let path = context_path(cfg, id)?;
    set_context_ready(&path, id, ready).with_context(|| format!("updating {}", path.display()))?;
    Ok(format!(
        "{id}: context {}",
        if ready { "ready" } else { "not ready" }
    ))
}

/// `pahiri task log <message>`.
pub fn task_log(cfg: &Config, id: &str, message: &str) -> Result<String> {
    if message.trim().is_empty() {
        bail!("empty message");
    }
    let path = context_path(cfg, id)?;
    append_log(&path, id, &now_rfc3339(), message.trim())
        .with_context(|| format!("updating {}", path.display()))?;
    Ok(format!("{id}: logged"))
}

/// `pahiri task outcome <text>`.
pub fn task_outcome(cfg: &Config, id: &str, text: &str) -> Result<String> {
    if text.trim().is_empty() {
        bail!("empty outcome");
    }
    let path = context_path(cfg, id)?;
    append_under(&path, id, OUTCOME_HEADING, &now_rfc3339(), text.trim())
        .with_context(|| format!("updating {}", path.display()))?;
    Ok(format!("{id}: outcome recorded"))
}

/// `pahiri task move --to <column>` (name, case-insensitive, or 0-based index).
pub fn task_move(cfg: &Config, id: &str, to: &str) -> Result<String> {
    let path = context_path(cfg, id)?;
    let mut store = TaskStore::open(
        &cfg.tasks_dir,
        &cfg.status_file,
        &cfg.context_file,
        &cfg.categories,
    )?;
    let cats = store.categories().to_vec();
    let target = to
        .trim()
        .parse::<usize>()
        .ok()
        .filter(|i| *i < cats.len())
        .or_else(|| cats.iter().position(|c| c.eq_ignore_ascii_case(to.trim())))
        .with_context(|| format!("no column {to:?}; columns: {}", cats.join(", ")))?;
    let from = store
        .board()
        .locate(id)
        .map(|(c, _)| c)
        .with_context(|| format!("{id} is not on the board"))?;
    store.move_task(id, target)?;
    record_move(&path, id, from, target, cats.len(), &now_rfc3339())?;
    Ok(format!("{id}: {} → {}", cats[from], cats[target]))
}

/// `pahiri trash empty [--older-than 30d]`.
pub fn trash_empty(cfg: &Config, older_than: &str) -> Result<String> {
    let days: u64 = older_than
        .trim()
        .trim_end_matches('d')
        .parse()
        .with_context(|| format!("--older-than takes days, e.g. 30d, got {older_than:?}"))?;
    let removed = empty_trash(&cfg.tasks_dir, days, crate::time::now_secs())?;
    Ok(if removed.is_empty() {
        format!("nothing in the trash older than {days} days")
    } else {
        removed
            .iter()
            .map(|p| format!("deleted {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

/// `pahiri task next`: the current checkpoint and what is left.
pub fn task_next(cfg: &Config, id: &str) -> Result<String> {
    let path = context_path(cfg, id)?;
    let items = checkpoints::read(&path)?;
    Ok(match checkpoints::next_open(&items) {
        Some(i) => {
            let c = &items[i];
            let mut out = format!(
                "{id} · checkpoint {}/{}: {} ({}",
                i + 1,
                items.len(),
                c.title,
                fmt_minutes(c.estimate_min)
            );
            if c.spent_min > 0 {
                let _ = write!(out, ", spent {}", fmt_minutes(c.spent_min));
            }
            out.push(')');
            if let Some(n) = items[i + 1..].iter().find(|c| !c.done) {
                let _ = write!(
                    out,
                    "
then: {} ({})",
                    n.title,
                    fmt_minutes(n.estimate_min)
                );
            }
            out
        }
        None if items.is_empty() => format!("{id}: no checkpoints yet"),
        None => format!("{id}: all {} checkpoints done", items.len()),
    })
}

fn valid_date(d: &str) -> Result<()> {
    let ok = d.len() == 10
        && d.bytes().enumerate().all(|(i, b)| match i {
            4 | 7 => b == b'-',
            _ => b.is_ascii_digit(),
        });
    if !ok {
        bail!("dates are YYYY-MM-DD, got {d:?}");
    }
    Ok(())
}

/// Records of every task, with its board column.
pub fn records(cfg: &Config) -> Result<Vec<TaskRecord>> {
    let status = fs::read_to_string(cfg.status_path()).unwrap_or_default();
    let board = Board::parse(&status, &cfg.categories);
    let ids = discover(&cfg.tasks_dir)?;
    Ok(ids
        .iter()
        .map(|id| {
            let column = board.locate(id).map_or(
                cfg.categories.first().map_or("?", String::as_str),
                |(c, _)| board.columns[c].name.as_str(),
            );
            TaskRecord::load(id, column, &cfg.tasks_dir.join(id).join(&cfg.context_file))
        })
        .collect())
}

/// `pahiri report [--from] [--to] [--json]`.
pub fn report(cfg: &Config, from: Option<&str>, to: Option<&str>, json: bool) -> Result<String> {
    for d in [from, to].into_iter().flatten() {
        valid_date(d)?;
    }
    let mut recs: Vec<TaskRecord> = records(cfg)?
        .into_iter()
        .filter(|r| r.active_between(from, to))
        .collect();
    recs.sort_by(|a, b| {
        let key = |r: &TaskRecord| {
            r.started
                .clone()
                .or_else(|| r.created.clone())
                .unwrap_or_default()
        };
        key(a).cmp(&key(b)).then_with(|| a.id.cmp(&b.id))
    });
    if json {
        return Ok(serde_json::to_string_pretty(&recs)?);
    }
    let finished = recs.iter().filter(|r| r.finished.is_some()).count();
    let minutes: u64 = recs.iter().map(|r| r.time_spent_min).sum();
    let mut out = format!(
        "# Tasks {} … {}\n\n{} task(s) · {finished} finished · {} booked\n",
        from.unwrap_or("start"),
        to.unwrap_or("now"),
        recs.len(),
        fmt_minutes(minutes)
    );
    for r in &recs {
        out.push('\n');
        out.push_str(&r.render_markdown());
    }
    Ok(out)
}

/// The plan's day: `date`, else today (local time).
fn plan_day(date: Option<&str>) -> Result<String> {
    match date {
        Some(d) => {
            valid_date(d)?;
            Ok(d.to_owned())
        }
        None => Ok(plan::local_date(
            crate::time::now_secs(),
            crate::time::local_offset_secs(),
        )),
    }
}

/// One plan item with its live state, for `pahiri plan show --json`.
#[derive(Debug, serde::Serialize)]
struct PlanRecord {
    task: Option<String>,
    title: String,
    estimate_min: u64,
    spent_min: u64,
    done: bool,
}

/// `pahiri plan show [--date D] [--json]`: the day plan, with checkpoint
/// state read from each task's `CONTEXT.md`.
pub fn plan_show(cfg: &Config, date: Option<&str>, json: bool) -> Result<String> {
    let day = plan_day(date)?;
    let items = plan::read(&plan::path(&cfg.tasks_dir, &day))?;
    let recs = records(cfg)?;
    let rows: Vec<PlanRecord> = items
        .into_iter()
        .map(|i| {
            let cp = i.task.as_ref().and_then(|t| {
                recs.iter()
                    .find(|r| &r.id == t)
                    .and_then(|r| r.checkpoints.iter().find(|c| c.title == i.title))
            });
            PlanRecord {
                done: cp.map_or(i.done, |c| c.done),
                spent_min: cp.map_or(0, |c| c.spent_min),
                task: i.task,
                title: i.title,
                estimate_min: i.estimate_min,
            }
        })
        .collect();
    if json {
        return Ok(serde_json::to_string_pretty(&rows)?);
    }
    if rows.is_empty() {
        return Ok(format!("no plan for {day}"));
    }
    let done = rows.iter().filter(|r| r.done).count();
    let left: u64 = rows
        .iter()
        .filter(|r| !r.done)
        .map(|r| r.estimate_min.saturating_sub(r.spent_min))
        .sum();
    let mut out = format!(
        "Plan {} · {done}/{} done · {} left\n",
        plan::day_label(&day),
        rows.len(),
        fmt_minutes(left)
    );
    for r in &rows {
        let item = PlanItem {
            task: r.task.clone(),
            title: r.title.clone(),
            estimate_min: r.estimate_min,
            done: r.done,
        };
        out.push_str(&item.render());
        out.push('\n');
    }
    Ok(out.trim_end().to_owned())
}

/// `pahiri plan add [--task ID] [--date D] <text…>`: append an item such as
/// `Reply to review 20m` (a free item without `--task`).
pub fn plan_add(
    cfg: &Config,
    task: Option<String>,
    date: Option<&str>,
    text: &str,
) -> Result<String> {
    let day = plan_day(date)?;
    let Some(mut item) = PlanItem::free(text, 30) else {
        bail!("nothing to add");
    };
    if let Some(t) = &task {
        // A checkpoint of the task keeps its own estimate.
        let items = checkpoints::read(&context_path(cfg, t)?)?;
        if let Some(c) = items.iter().find(|c| c.title == item.title) {
            item.estimate_min = c.estimate_min;
        }
    }
    item.task = task;
    let path = plan::path(&cfg.tasks_dir, &day);
    let mut items = plan::read(&path)?;
    if items.iter().any(|i| i.same(&item)) {
        return Ok(format!("already planned: {}", item.render()));
    }
    items.push(item.clone());
    plan::write(&path, &day, &items)?;
    Ok(format!("added to {day}: {}", item.render()))
}

/// `pahiri install-skills <dir>`: write the bundled skills as `<dir>/<name>/SKILL.md`.
pub fn install_skills(dir: &Path, force: bool) -> Result<String> {
    let mut out = String::new();
    for (name, text) in SKILLS {
        let target = dir.join(name).join("SKILL.md");
        if target.exists() && !force {
            let _ = writeln!(
                out,
                "kept      {} (exists; --force overwrites)",
                target.display()
            );
            continue;
        }
        fs::create_dir_all(target.parent().unwrap_or(dir))?;
        fs::write(&target, text).with_context(|| format!("writing {}", target.display()))?;
        let _ = writeln!(out, "installed {}", target.display());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(dir: &Path) -> Config {
        Config {
            tasks_dir: dir.to_path_buf(),
            ..Config::default()
        }
    }

    #[test]
    fn plan_show_and_add() {
        let dir = tempfile::tempdir().unwrap();
        let c = cfg(dir.path());
        fs::create_dir_all(dir.path().join("T-1")).unwrap();
        fs::write(
            dir.path().join("T-1/CONTEXT.md"),
            "# T-1\n\n## Checkpoints\n<!-- pahiri:checkpoints -->\n- [x] A (10m; spent 12m)\n- [ ] B (20m; spent 5m)\n<!-- /pahiri:checkpoints -->\n",
        )
        .unwrap();
        let day = Some("2026-09-25");
        assert_eq!(plan_show(&c, day, false).unwrap(), "no plan for 2026-09-25");
        plan_add(&c, Some("T-1".into()), day, "A").unwrap();
        plan_add(&c, Some("T-1".into()), day, "B 20m").unwrap();
        assert!(plan_add(&c, Some("T-1".into()), day, "B 20m")
            .unwrap()
            .starts_with("already planned"));
        plan_add(&c, None, day, "Email the vendor 15m").unwrap();
        assert!(plan_add(&c, Some("nope".into()), day, "x").is_err());
        assert!(plan_add(&c, None, Some("25/09"), "x").is_err());
        assert_eq!(
            plan_show(&c, day, false).unwrap(),
            "Plan Fri 25 Sep · 1/3 done · 30m left\n\
             - [x] T-1 · A (10m)\n\
             - [ ] T-1 · B (20m)\n\
             - [ ] Email the vendor (15m)"
        );
        let json: serde_json::Value =
            serde_json::from_str(&plan_show(&c, day, true).unwrap()).unwrap();
        assert_eq!(json[1]["spent_min"], 5);
        assert_eq!(json[0]["done"], true);
        assert_eq!(json[2]["task"], serde_json::Value::Null);
    }

    #[test]
    fn task_commands_and_report() {
        let dir = tempfile::tempdir().unwrap();
        let c = cfg(dir.path());
        fs::create_dir_all(dir.path().join("T-1")).unwrap();
        fs::write(
            dir.path().join("T-1/CONTEXT.md"),
            "# T-1: Thing\n\n## Checkpoints\n<!-- pahiri:checkpoints -->\n- [x] A (10m)\n- [ ] B (20m; spent 5m)\n- [ ] C (1h)\n<!-- /pahiri:checkpoints -->\n",
        )
        .unwrap();
        fs::write(dir.path().join("status.md"), "## Doing\n- T-1\n").unwrap();
        assert!(task_ready(&c, "T-1", true).unwrap().contains("ready"));
        assert!(fs::read_to_string(dir.path().join("T-1/CONTEXT.md"))
            .unwrap()
            .contains("- context_ready: true"));
        task_log(&c, "T-1", "did a thing").unwrap();
        assert_eq!(
            task_next(&c, "T-1").unwrap(),
            "T-1 · checkpoint 2/3: B (20m, spent 5m)\nthen: C (1h)"
        );
        assert!(task_ready(&c, "nope", true).is_err());
        assert_eq!(task_move(&c, "T-1", "done").unwrap(), "T-1: Doing → Done");
        let md = fs::read_to_string(dir.path().join("T-1/CONTEXT.md")).unwrap();
        assert!(md.contains("- finished: "), "{md}");
        assert!(task_move(&c, "T-1", "nowhere").is_err());
        assert_eq!(task_move(&c, "T-1", "1").unwrap(), "T-1: Done → Doing");
        task_outcome(&c, "T-1", "shipped it").unwrap();
        assert!(fs::read_to_string(dir.path().join("T-1/CONTEXT.md"))
            .unwrap()
            .contains("## Outcome\n- "));
        assert!(trash_empty(&c, "30d").unwrap().contains("nothing"));
        assert!(trash_empty(&c, "x").is_err());
        assert!(task_log(&c, "T-1", " ").is_err());

        let md = report(&c, None, None, false).unwrap();
        assert!(md.contains("## T-1 — Thing\n- Doing ·"), "{md}");
        let today = &now_rfc3339()[..10];
        let json = report(&c, Some(today), None, true).unwrap();
        assert!(json.contains("\"id\": \"T-1\""), "{json}");
        assert!(json.contains("did a thing"));
        assert!(report(&c, Some("2026/01/01"), None, false).is_err());
        assert!(report(&c, None, Some("2000-01-01"), false)
            .unwrap()
            .contains("0 task(s)"));
    }

    #[test]
    fn resolves_task_from_env_or_flag() {
        assert_eq!(resolve_task(Some("X-1".into())).unwrap(), "X-1");
        assert!(resolve_task(Some("../x".into())).is_err());
    }

    #[test]
    fn installs_skills() {
        let dir = tempfile::tempdir().unwrap();
        let out = install_skills(dir.path(), false).unwrap();
        assert_eq!(out.matches("installed").count(), SKILLS.len());
        assert!(dir.path().join("less/SKILL.md").is_file());
        fs::write(dir.path().join("less/SKILL.md"), "mine").unwrap();
        assert!(install_skills(dir.path(), false).unwrap().contains("kept"));
        assert_eq!(
            fs::read_to_string(dir.path().join("less/SKILL.md")).unwrap(),
            "mine"
        );
        install_skills(dir.path(), true).unwrap();
        assert!(fs::read_to_string(dir.path().join("less/SKILL.md"))
            .unwrap()
            .starts_with("---\nname: less"));
        for (name, text) in SKILLS {
            assert!(text.starts_with(&format!("---\nname: {name}\n")), "{name}");
        }
    }
}
