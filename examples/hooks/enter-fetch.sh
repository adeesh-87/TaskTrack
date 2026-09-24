#!/bin/sh
# task_enter hook (pahiri waits for it before showing the task): fetch the
# attached repos so branch information is fresh, and print where you are.
#   [hooks]
#   task_enter = "~/src/pahiri/examples/hooks/enter-fetch.sh"
IFS=';'
for entry in $PAHIRI_CODE_DIRS; do
  dir="${entry#*=}"
  [ -d "$dir" ] || continue
  git -C "$dir" fetch -q --all 2>/dev/null &
done
wait
echo "${PAHIRI_TASK}: next ${PAHIRI_NEXT_CHECKPOINT:-(no checkpoints yet)}"
