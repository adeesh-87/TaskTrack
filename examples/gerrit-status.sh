#!/bin/sh
# pahiri Gerrit status command: prints one JSON line per Change-Id given as an
# argument, in the format pahiri expects (see F1 → Gerrit).
#
#   gerrit_status_command = "~/src/pahiri/examples/gerrit-status.sh"
#
# Needs ssh access to Gerrit and jq. Environment:
#   GERRIT_SSH   e.g. me@review.example.com   (required)
#   GERRIT_PORT  default 29418
set -eu
: "${GERRIT_SSH:?set GERRIT_SSH, e.g. me@review.example.com}"
PORT="${GERRIT_PORT:-29418}"

for id in "$@"; do
  ssh -o BatchMode=yes -p "$PORT" "$GERRIT_SSH" \
    gerrit query --format=JSON --current-patch-set "change:$id" |
  jq -c 'select(.type != "stats") | {
    change_id: .id,
    number: .number,
    status: .status,
    url: .url,
    subject: .subject,
    labels: ([.currentPatchSet.approvals[]? |
      ((.type | sub("Code-Review"; "CR") | sub("Verified"; "V")) +
       (if (.value | tonumber) > 0 then "+" else "" end) + .value)] | join(" "))
  }'
done
