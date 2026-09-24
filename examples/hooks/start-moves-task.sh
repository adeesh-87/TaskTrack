#!/bin/sh
# timer_start hook: starting the timer on a task in the first column moves it
# to the second one (e.g. Planned → Doing).
#   [hooks]
#   timer_start = "~/src/pahiri/examples/hooks/start-moves-task.sh"
[ "$PAHIRI_COLUMN_INDEX" = "0" ] || exit 0
"$PAHIRI_BIN" task move --to 1
