#!/bin/sh
# task_move hook: when a task lands in the last column, remind yourself to
# record a one-line outcome (Esc O, or `pahiri task outcome "…"`).
#   [hooks]
#   task_move = "~/src/pahiri/examples/hooks/finish-reminder.sh"
[ "$PAHIRI_FINISHED" = "1" ] || exit 0
grep -q '^## Outcome' "$PAHIRI_CONTEXT_FILE" && exit 0
"$PAHIRI_BIN" task log "finished — record an outcome with Esc O"
echo "$PAHIRI_TASK finished: record an outcome (Esc O)"
