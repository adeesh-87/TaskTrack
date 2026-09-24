---
name: plan-first
description: Before executing a complex or multi-step task, propose a short structured plan (goal, assumptions, ordered checkpoints with time estimates, risks, how to verify) and wait for approval. Use for any change touching several files, anything risky or ambiguous, or when asked to "plan", "break down" or "checkpoint" work.
---

# plan-first

Think first, get a yes, then execute one checkpoint at a time.

## When

- The task needs more than ~3 steps, touches several files, or is hard to undo.
- The request is ambiguous, or there are real alternatives.
- The user asks to plan, break down, estimate or checkpoint.

Skip it for one-line fixes and plain questions.

## The plan (keep it under ~250 words)

```markdown
### Goal
One sentence: what is true when this is done.

### Assumptions
- Things you are taking as given; each one the user can veto.

### Checkpoints
- [ ] <imperative step, max 12 words> (<estimate: 20m, 45m, 1h30m>)
- [ ] ...

### Risks
- <risk> → <mitigation>

### Verify
- The command / test / observation that proves it works.
```

Checkpoint rules — the same format pahiri parses, so a plan can be pasted
straight into a task's `## Checkpoints`:

- 3 to 12 items, in execution order, each 10m–2h (split bigger ones).
- Each ends in something visible: a file, a passing test, a pushed change.
- First step removes the most uncertainty. Last step verifies and hands over
  (push for review, update the ticket).
- Estimates are for the person doing it, not an expert.

## Then

1. Stop and ask: "Go ahead?" Do not start before a yes (or edits to the plan).
2. Execute one checkpoint at a time. After each: one line of what changed and
   how it was verified. In a pahiri task, also run
   `pahiri task log "✓ <checkpoint>: <result>"`.
3. If reality diverges from the plan, stop and propose the updated plan
   instead of silently improvising.

## In a pahiri task

`$PAHIRI_TASK` is set. `pahiri task next` prints the current checkpoint;
the task's `CONTEXT.md` has the goal, context and checkpoints. Plan within
the current checkpoint unless asked to re-plan the task.
