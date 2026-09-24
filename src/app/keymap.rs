//! Help texts shown by the help popups.

pub const HELP: &str = "\
Esc opens the command palette (anywhere except inside a shell; there use
leader Esc). Press a shortcut letter, or : to type and filter, then Enter.

Doing the work
  k checkpoints · m timer start/pause · v checkpoint done → next · M stop timer
  i AI: write context · r context ready · b AI: break into checkpoints
  l coding agent in a new shell · g Gerrit changes · E edit prompt templates

Task list
  ↑/↓ or j/k move · Enter open · [ / ] move between categories · n new task
  d delete (to .trash) · m timer · the right side shows what to do next

Panes (task view)
  Ctrl+Tab / Ctrl+Shift+Tab   next / previous pane (files → editor → shells → terminal)
  Alt+] / Alt+[               same, for terminals without the kitty keyboard protocol
  Ctrl+1..9 / Alt+1..9        select shell N and focus the terminal

Files
  Enter open file / toggle folder · ←/→ collapse / expand · a new file · A new folder
  r rename · d delete (asks first) · . hidden files · t new shell · R refresh

Shells pane
  Enter focus · n new · x close · ← hide pane

Editor
  Ctrl+S save · Ctrl+W close · Esc palette

Leader (default ctrl+b) — in the terminal, and in every other task pane
  a coding agent   m timer        v checkpoint done
  q leave shell    z zoom         n new shell       x close shell
  Esc palette      [ / ] scroll   leader leader sends the leader key

In the shell: cd task · cd code [name] · cd build [name]
From scripts / agents: pahiri task ready | log \"…\" | next · pahiri report";

/// Shown with `?` on the task-sources setting.
pub const TASK_SOURCE_HELP: &str = "\
A task source is any script that prints your tickets on stdout.

Setting:   name = command          e.g.   jira = ~/bin/jira-mine.sh
           The command runs through `sh -c` in your home folder with
           $PAHIRI_TASKS_DIR set. Exit non-zero to report an error
           (the last lines of stderr are shown).

Output, pick one:

1. JSON array
   [{\"id\": \"PROJ-123\", \"title\": \"Fix login\",
     \"url\": \"https://jira.example.com/browse/PROJ-123\",
     \"description\": \"Users cannot log in after …\"}]

2. JSON lines — one such object per line.

3. Tab separated — one ticket per line, # starts a comment:
   PROJ-123<TAB>Fix login<TAB>https://…/PROJ-123<TAB>line one\\nline two

Fields: id (required; becomes the task id and git branch — spaces and
other unsafe characters turn into -), title, url, description.
Aliases: key = id, summary = title, link = url, body = description.

Examples ship in examples/ (jira.sh uses curl + jq with $JIRA_URL and
$JIRA_TOKEN; template.py is a skeleton for anything else, e.g. Orbit).

On this page: t on a source runs it now and shows what pahiri parsed.";

/// Shown with `?` on the agent settings.
pub const AGENT_HELP: &str = "\
AI agent (one-shot): writes the task context and the checkpoints.

Agent command / arguments: the program and its flags. An argument
containing {prompt} receives the prompt; without one the prompt is
written to stdin. Examples:
  claude    -p {prompt}
  codex     exec {prompt}
  ollama    run llama3            (prompt on stdin)
It runs in the first attached code workspace (else the task folder)
with PAHIRI_TASK, PAHIRI_TASK_DIR, PAHIRI_CODE_DIR(S) and
PAHIRI_CONTEXT_FILE set. stdout is the answer; Esc cancels.

Prompt templates (Esc E inside a task opens them) are Markdown with
placeholders: {{task}} {{task_dir}} {{context_file}} {{context}}
{{workspaces}} {{builds}} {{branch}} {{next_checkpoint}} {{max_words}}
pahiri always appends a fixed OUTPUT FORMAT section after your
template, so the parts it parses cannot be edited away:
  context      ≤ word limit (max 1000), last line CONTEXT_READY: yes|no
  checkpoints  lines of  - [ ] step (40m)

Coding agent: an interactive program started in a new task shell with
leader a (or Esc l). Its startup prompt is a template like the above.

Context ready: checkpoints need a context that is good enough. The
agent's CONTEXT_READY line flips it, you flip it with Esc r, and
scripts/agents can run `pahiri task ready [--off]` — all three use the
same switch in CONTEXT.md (`- context_ready: true`).";
