#!/bin/sh
# timer_expire hook: a desktop notification when a checkpoint's time is up.
#   [hooks]
#   timer_expire = "~/src/pahiri/examples/hooks/notify.sh"
msg="${PAHIRI_CHECKPOINT:-focus block}: time is up (${PAHIRI_TASK})"
if command -v notify-send >/dev/null 2>&1; then
  notify-send -u critical "pahiri" "$msg"
elif command -v osascript >/dev/null 2>&1; then
  osascript -e "display notification \"$msg\" with title \"pahiri\""
fi
echo "$msg"
