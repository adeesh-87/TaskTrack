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
use crate::tasks::context::{append_log, set_context_ready};
use crate::tasks::record::TaskRecord;
use crate::tasks::store::{discover, is_valid_id};
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
