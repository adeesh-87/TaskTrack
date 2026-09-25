#!/bin/sh
# pahiri audit: prints your Gerrit changes since $PAHIRI_AUDIT_SINCE as JSON
# lines, in the format pahiri expects (F1 → Audit).
#
#   [audit]
#   gerrit_command = "~/src/pahiri/examples/gerrit-mine.sh"
#
# With audit.gerrit_file set, pahiri sets PAHIRI_OUTPUT_FILE and the changes go
# there instead of stdout.
#
# Needs ssh access to Gerrit and jq. Environment:
#   GERRIT_SSH    e.g. me@review.example.com   (required)
#   GERRIT_PORT   default 29418
#   GERRIT_QUERY  default: owner:self after:<since>
set -eu
: "${GERRIT_SSH:?set GERRIT_SSH, e.g. me@review.example.com}"
PORT="${GERRIT_PORT:-29418}"
SINCE="${PAHIRI_AUDIT_SINCE:-1970-01-01}"
QUERY="${GERRIT_QUERY:-owner:self after:$SINCE}"

# Collect into a temporary file; a failed query (set -e stops the script)
# leaves the last good output file as it was.
out=$(mktemp)
trap 'rm -f "$out"' EXIT
# ssh query pages at 500 results; --start continues.
start=0
while :; do
  page=$(ssh -o BatchMode=yes -p "$PORT" "$GERRIT_SSH" \
    gerrit query --format=JSON --current-patch-set --start "$start" "$QUERY")
  printf '%s\n' "$page" | jq -c 'select(.type != "stats") | {
    change_id: .id,
    number: .number,
    status: .status,
    url: .url,
    subject: .subject,
    project: .project,
    branch: .branch,
    topic: .topic,
    created: .createdOn,
    updated: .lastUpdated,
    merged: (if .status == "MERGED" then .lastUpdated else null end),
    labels: ([.currentPatchSet.approvals[]? |
      ((.type | sub("Code-Review"; "CR") | sub("Verified"; "V")) +
       (if (.value | tonumber) > 0 then "+" else "" end) + .value)] | join(" "))
  }' >> "$out"
  more=$(printf '%s\n' "$page" | jq -r 'select(.type == "stats") | .moreChanges // false')
  [ "$more" = true ] || break
  start=$((start + $(printf '%s\n' "$page" | jq -r 'select(.type == "stats") | .rowCount')))
done
if [ -n "${PAHIRI_OUTPUT_FILE:-}" ]; then
  mv "$out" "$PAHIRI_OUTPUT_FILE"
else
  cat "$out"
fi
