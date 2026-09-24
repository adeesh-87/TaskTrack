---
name: standup
description: Write a daily standup (yesterday / today / blockers) from pahiri task logs and checkpoints. Use when asked for a standup, daily update, status update or "what did I do yesterday".
---

# standup

```sh
pahiri report --from <last working day> --json
```

From each task in the output use: `log` lines dated since the last working
day (✓ checkpoints, timer sessions, notes), the next open checkpoint, and
log lines mentioning "blocked", "waiting" or "?".

## Output — at most 8 lines, no headers beyond these

```
Yesterday: <ID> <what got done, from ✓ lines>; <ID> …
Today: <ID> <next open checkpoint>; …
Blockers: <blocker — who can unblock> | none
```

Rules: past tense for yesterday, concrete nouns, no filler ("worked on"
only if nothing better is recorded). Mention Gerrit changes waiting for
review as blockers only if they are older than a day.
