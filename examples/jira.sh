#!/bin/sh
# pahiri task source for Jira: prints your open tickets as JSON lines.
#
#   [[task_sources]]
#   name = "jira"
#   command = "~/bin/jira.sh"
#
# Needs curl and jq. Environment:
#   JIRA_URL          https://jira.example.com            (required)
#   JIRA_TOKEN        personal access token / API token   (required)
#   JIRA_EMAIL        set for Jira Cloud (basic auth email:token); unset → Bearer token
#   JIRA_JQL          default: assignee = currentUser() AND resolution = Unresolved ORDER BY updated DESC
#   JIRA_SEARCH_PATH  default: rest/api/2/search  (Jira Cloud may need rest/api/3/search/jql)
set -eu

: "${JIRA_URL:?set JIRA_URL}"
: "${JIRA_TOKEN:?set JIRA_TOKEN}"
JQL="${JIRA_JQL:-assignee = currentUser() AND resolution = Unresolved ORDER BY updated DESC}"
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
    --data-urlencode "fields=summary,description" \
    --data-urlencode "maxResults=100"
}

search |
jq -c --arg base "$BASE" '.issues[] | {
  id: .key,
  title: .fields.summary,
  url: "\($base)/browse/\(.key)",
  description: (.fields.description // "" | if type == "string" then . else tostring end)
}'
