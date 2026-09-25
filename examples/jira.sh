#!/bin/sh
# pahiri task source for Jira: prints your tickets as JSON lines.
#
#   [[task_sources]]
#   name = "jira"
#   command = "~/bin/jira.sh"
#
# Normally: your open tickets. For the audit (Esc U) pahiri sets PAHIRI_AUDIT=1
# and PAHIRI_AUDIT_SINCE=YYYY-MM-DD; then it lists every ticket assigned to you
# that changed since that day, finished ones included, with status and dates.
#
# With `file = …` on the source (or `@file …` on the settings page) pahiri sets
# PAHIRI_OUTPUT_FILE and the tickets go there instead of stdout.
#
# Needs curl and jq. Environment:
#   JIRA_URL          https://jira.example.com            (required)
#   JIRA_TOKEN        personal access token / API token   (required)
#   JIRA_EMAIL        set for Jira Cloud (basic auth email:token); unset → Bearer token
#   JIRA_JQL          default: assignee = currentUser() AND resolution = Unresolved ORDER BY updated DESC
#   JIRA_AUDIT_JQL    default: assignee was currentUser() AND updated >= <since> ORDER BY updated DESC
#   JIRA_SEARCH_PATH  default: rest/api/2/search  (Jira Cloud may need rest/api/3/search/jql)
set -eu

: "${JIRA_URL:?set JIRA_URL}"
: "${JIRA_TOKEN:?set JIRA_TOKEN}"
if [ "${PAHIRI_AUDIT:-}" = 1 ]; then
  SINCE="${PAHIRI_AUDIT_SINCE:-1970-01-01}"
  JQL="${JIRA_AUDIT_JQL:-assignee was currentUser() AND updated >= \"$SINCE\" ORDER BY updated DESC}"
else
  JQL="${JIRA_JQL:-assignee = currentUser() AND resolution = Unresolved ORDER BY updated DESC}"
fi
SEARCH="${JIRA_SEARCH_PATH:-rest/api/2/search}"
BASE="${JIRA_URL%/}"

search() {
  if [ -n "${JIRA_EMAIL:-}" ]; then
    set -- -u "$JIRA_EMAIL:$JIRA_TOKEN"
  else
    set -- -H "Authorization: Bearer $JIRA_TOKEN"
  fi
  curl -fsS "$@" -G "$BASE/$SEARCH" \
    --data-urlencode "jql=$JQL" \
    --data-urlencode "fields=summary,description,status,created,resolutiondate" \
    --data-urlencode "maxResults=200"
}

# status category "done" (green in Jira) marks finished tickets whatever the
# workflow calls its last status.
# Fetch into a temporary file first: a failed request (set -e stops here)
# leaves the last good output file as it was.
raw=$(mktemp)
trap 'rm -f "$raw" "$raw.out"' EXIT
search > "$raw"
jq -c --arg base "$BASE" '.issues[] | {
  id: .key,
  title: .fields.summary,
  url: "\($base)/browse/\(.key)",
  description: (.fields.description // "" | if type == "string" then . else tostring end),
  status: (.fields.status.name // ""),
  done: ((.fields.status.statusCategory.key // "") == "done"),
  created: .fields.created,
  finished: .fields.resolutiondate
}' "$raw" > "$raw.out"
if [ -n "${PAHIRI_OUTPUT_FILE:-}" ]; then
  mv "$raw.out" "$PAHIRI_OUTPUT_FILE"
else
  cat "$raw.out"
fi
