# File and output formats

## `CONTEXT.md` (one per task)

Free Markdown owned by the user. pahiri only rewrites text between its markers
and appends list items under its headings:

```markdown
# PROJ-42: Fix login            ← title (first "# " heading)

## Description                   ← from the ticket, once
## Context                       ← AI-written (Esc i), replaced each run
<!-- pahiri:context -->
### Goal …
<!-- /pahiri:context -->

## Notes                         ← yours

## Checkpoints                   ← Esc b / by hand; timer books "spent"
<!-- pahiri:checkpoints -->
- [x] Read the spec (20m; spent 25m)
- [ ] Write the parser (1h)
<!-- /pahiri:checkpoints -->

## Outcome                       ← one line asked when the task is finished
- 2026-09-24 17:00 Shipped; root cause was a stale token

## Log                           ← timer sessions, ✓ checkpoints, `pahiri task log`
- 2026-09-24 10:05 ✓ Read the spec (spent 25m, estimated 20m)

## Attachments
<!-- pahiri:begin -->
- source: jira
- link: https://jira.example.com/browse/PROJ-42
- workspace: fw                  ← repeatable
- build: yocto                   ← repeatable
- branch: PROJ-42
- prepared: 2026-09-24T10:00:00Z
- gerrit: fw I0123…(41 chars) https://review.example.com/q/I0123… :: subject
- created: 2026-09-20T08:00:00Z
- started: 2026-09-21T09:30:00Z  ← first timer start / moved out of the first column
- finished: 2026-09-24T17:00:00Z ← moved to the last column (cleared when moved back)
- time_spent: 215m               ← minutes booked by the timer
- context_ready: true            ← gate for Esc b
<!-- pahiri:end -->
```

Checkpoint line grammar (tolerant): `- [ ]` or `- [x]`, optional `1.`,
title, then `(<estimate>[; spent <duration>])` at the end of the line.
Durations: `40m`, `1h`, `1h30m`, `1.5h`, `90 min`, `2 hours`, `45`.

## `timelog.tsv` (time ledger, in the tasks folder)

One line per booking: `2026-09-24T10:05:00Z<TAB>PROJ-42<TAB>7<TAB>Read the spec`
(UTC time, task, minutes, checkpoint or `focus block`). Append-only.

## Day plan (`<tasks>/.pahiri/plans/YYYY-MM-DD.md`)

```markdown
# Plan 2026-09-25

## Plan
<!-- pahiri:plan -->
- [x] PROJ-42 · Read the spec (20m)      ← a checkpoint of PROJ-42 (matched by title)
- [ ] PROJ-42 · Reply to review (20m)    ← a task item: not a checkpoint, timed on PROJ-42
- [ ] Email the vendor (15m)             ← a free item
<!-- /pahiri:plan -->

## Notes                                 ← yours
```

Order = plan order. A checkpoint's `[x]` mirrors its `CONTEXT.md`
(ticking either updates both); other items keep their own `[x]`. Only the
text between the markers is rewritten. `pahiri plan show|add` read and
append.

## `session.json` (state folder)

The timer (`task_id`, `checkpoint`, `budget_secs`, `elapsed_secs`,
`flushed_secs`, `running`, `saved_at`) and each task's shells (`cwd`,
`tmux` session name). Written on quit and every minute while timing.

## `status.md` (board)

`## <Category>` headings in configured order, `- <task-id>` items. Unknown
folders are added to the first category; missing folders are dropped.

## Task source scripts

Run through `sh -c`; print one of: JSON array, JSON lines, or TSV
`id<TAB>title<TAB>url<TAB>description` (`\n` escapes in the description).
Only `id` is required. Aliases: `key`, `summary`, `link`, `body`.
Non-zero exit = error (stderr tail is shown). Examples in `examples/`.

## Gerrit status command

Gets the Change-Ids as arguments (inline code using `$@` gets them as
positional parameters) and in `$PAHIRI_GERRIT_CHANGES`. Prints JSON lines
or an array: `{"change_id", "number", "status", "url", "labels", "subject"}`,
only `change_id` required. Stored as `[STATUS #number labels]` on the
`- gerrit:` line.

## Hooks

`[hooks]` `event = "command"`, run through `sh -c` in the task folder with
`PAHIRI_HOOK`, `PAHIRI_BIN`, `PAHIRI_CONFIG`, `PAHIRI_TASKS_DIR`,
`PAHIRI_TASK`, `PAHIRI_TASK_DIR`, `PAHIRI_CONTEXT_FILE`, `PAHIRI_TASK_TITLE`,
`PAHIRI_COLUMN(_INDEX)`, `PAHIRI_LINK`, `PAHIRI_BRANCH`,
`PAHIRI_CODE_DIR(S)`, `PAHIRI_BUILD_DIR(S)`, `PAHIRI_NEXT_CHECKPOINT`,
`PAHIRI_CONTEXT_READY`, plus per-event variables (`src/hooks.rs`
`extra_env`, or the help page). stdout's last line becomes the status line.

## One-shot agent (Esc i / Esc b)

Invocation: `<agent command> <args>`; an arg containing `{prompt}` gets the
prompt, otherwise the prompt is on stdin. cwd = first attached workspace
(else the task folder). Env: `PAHIRI_TASK`, `PAHIRI_TASK_DIR`,
`PAHIRI_CODE_DIR(S)`, `PAHIRI_BUILD_DIR(S)`, `PAHIRI_CONTEXT_FILE`.
stdout is the answer.

Prompt = user template (placeholders `{{task}} {{task_dir}} {{context_file}}
{{context}} {{workspaces}} {{builds}} {{branch}} {{next_checkpoint}}
{{max_words}}`) + a fixed format section pahiri appends:

- context: Markdown, ≤ word limit (max 1000), `###` headings, last line
  `CONTEXT_READY: yes` or `CONTEXT_READY: no - <what is missing>`.
- checkpoints: only lines `- [ ] <step> (<estimate>)`, 3–12 of them.

## CLI (for scripts and agents; default task `$PAHIRI_TASK`, config `$PAHIRI_CONFIG`)

```
pahiri task ready [--task ID] [--off]   flip context_ready
pahiri task log [--task ID] <message…>  append to ## Log
pahiri task next [--task ID]            current checkpoint and the one after
pahiri task move [--task ID] --to COL   move on the board, record dates
pahiri task outcome [--task ID] <text…> append to ## Outcome
pahiri trash empty [--older-than 30d]   delete old trashed tasks
pahiri report [--from D] [--to D] [--json]   tasks active in the range
pahiri plan show [--date D] [--json]    day plan with each item's state
pahiri plan add [--task ID] [--date D] <text [20m]>   add to the plan
pahiri config add-workspace NAME PATH [--main B] | add-build NAME PATH
pahiri config remove-workspace NAME | remove-build NAME | prune | list
pahiri install-skills <DIR> [--force]   write bundled skills to DIR/<name>/SKILL.md
```
