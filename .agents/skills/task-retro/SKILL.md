---
name: task-retro
description: Learn from past pahiri tasks for a period and write review material - what was done, impact, evidence, time spent, estimation accuracy and recurring learnings. Use for annual / mid-year / quarterly reviews, brag documents, "what did I do between X and Y", or "what did I learn".
---

# task-retro

Turn the task records of a period into review-ready material.

## Get the data (cheap first)

```sh
pahiri report --from 2026-01-01 --to 2026-06-30          # compact Markdown
pahiri report --from 2026-01-01 --to 2026-06-30 --json   # structured
```

Only open a task's `<tasks_dir>/<ID>/CONTEXT.md` when the report is not
enough (e.g. to understand what an outcome means). `pahiri --show-config`
prints `tasks_dir`.

## What a record contains

| field | where it comes from |
| ----- | ------------------- |
| created / started / finished | set by pahiri (task created, timer started or moved out of the first column, moved to the last column) |
| time_spent | minutes booked by the pahiri timer |
| checkpoints | `- [x] step (estimate; spent actual)` |
| outcome | one line typed when the task was finished (`## Outcome`) |
| log | timestamped lines (`## Log`): ✓ checkpoints, timer sessions, notes from `pahiri task log` |
| gerrit / link | review changes and the ticket |
| Context → "Review notes" | one line of impact written with the context |

Missing fields are normal (older tasks); say "not recorded", never invent.

## Output (default; follow the user's format if they give one)

```markdown
# <period> — work summary

## Highlights (3–5)
- <outcome, with impact and scope> — <ID>, <link / gerrit>

## By theme
### <theme, e.g. "Boot time", "Tooling">
- <ID> <title>: what I did → result. (<finished date>, <time spent>)

## How I worked
- Tasks finished: N · time booked: Xh · median task: Yd from start to finish
- Estimation: spent/estimated = R (per checkpoint; >1.3 means under-estimating)
- Where time went beyond estimates: <pattern>

## Learnings
- <pattern seen in ≥2 tasks> → <what to do differently>

## Evidence
- <ID>: <gerrit URLs / ticket links>
```

Rules: facts from the records only; quantify when the data allows; keep each
bullet to one line; group small tasks rather than listing them all.

## Keeping future reviews easy

Suggest (once, briefly) whichever of these is missing in many records:
typing an outcome when finishing, keeping the "Review notes" line in the
context, booking time with the timer.
