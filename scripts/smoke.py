#!/usr/bin/env python3
"""Drive the pahiri binary headlessly and print the rendered screens.

Usage: python3 scripts/smoke.py [path/to/pahiri]

Requires `pip install pexpect pyte`. Creates a throw-away tasks folder, a git
workspace, a fake Jira script and a config, then walks through the main
flows (board → palette → task from ticket → attach → prepare → shell with
`cd code` → full-screen program → zoom → settings → quit) and prints each
screen. Exit status is non-zero if a screen does not show what it should.
"""
import os
import subprocess
import sys
import tempfile
import time

import pexpect
import pyte

BIN = sys.argv[1] if len(sys.argv) > 1 else "./target/debug/pahiri"
COLS, ROWS = 120, 36

work = tempfile.mkdtemp(prefix="pahiri-smoke-")
tasks = os.path.join(work, "tasks")
os.makedirs(os.path.join(tasks, "demo-task", "scripts"))
open(os.path.join(tasks, "demo-task", "CONTEXT.md"), "w").write("# demo-task\n\nSome context.\n")
open(os.path.join(tasks, "demo-task", "scripts", "run.sh"), "w").write("echo hi\n")
open(os.path.join(tasks, "status.md"), "w").write("## Doing\n- demo-task\n")

repo = os.path.join(work, "firmware")
os.makedirs(repo)
for args in (["init", "-q", "-b", "main"], ["config", "user.email", "s@x"], ["config", "user.name", "s"]):
    subprocess.run(["git", "-C", repo, *args], check=True)
open(os.path.join(repo, "README"), "w").write("fw\n")
subprocess.run(["git", "-C", repo, "add", "-A"], check=True)
subprocess.run(["git", "-C", repo, "commit", "-q", "-m", "init"], check=True)

jira = os.path.join(work, "jira.sh")
open(jira, "w").write(
    "#!/bin/sh\nprintf '%s' '[{\"id\":\"PROJ-42\",\"title\":\"Fix the flux capacitor\","
    "\"url\":\"https://jira.example.com/browse/PROJ-42\",\"description\":\"It does not flux.\"}]'\n"
)
os.chmod(jira, 0o755)

# A fake one-shot agent: answers the context prompt, then the checkpoint prompt.
agent = os.path.join(work, "agent.sh")
open(agent, "w").write(
    "#!/bin/sh\nprompt=$(cat)\n"
    "case \"$prompt\" in\n"
    "  *'checklist only'*) printf -- '- [ ] Tiny warm-up (1m; spent 1m)\\n- [ ] Flux the capacitor (45m)\\n' ;;\n"
    "  *'MAP C:'*) printf 'NEW C:501 -> dts-cleanup | Clean up the dts\\n' ;;\n"
    "  *) printf '### Goal\\n- make it flux\\n\\nCONTEXT_READY: yes\\n' ;;\n"
    "esac\n"
)
os.chmod(agent, 0o755)

# The audit's "my changes": one for PROJ-42 (by topic), one nothing claims.
mine = os.path.join(work, "gerrit-mine.sh")
open(mine, "w").write(
    "#!/bin/sh\n"
    "echo '{\"change_id\":\"I5000000000000000000000000000000000000000\",\"number\":500,\"status\":\"NEW\","
    "\"project\":\"fw\",\"topic\":\"PROJ-42\",\"subject\":\"Flux fix\",\"created\":\"2026-07-01 10:00:00\"}'\n"
    "echo '{\"change_id\":\"I5010000000000000000000000000000000000000\",\"number\":501,\"status\":\"MERGED\","
    "\"project\":\"fw\",\"subject\":\"Clean dts\",\"updated\":\"2026-07-05 10:00:00\"}'\n"
)
os.chmod(mine, 0o755)

cfg = os.path.join(work, "config.toml")
open(cfg, "w").write(
    f'tasks_dir = "{tasks}"\n'
    f'[shell]\nprogram = "bash"\nargs = ["--norc", "-i"]\n'
    f'[agent]\ncommand = "{agent}"\nargs = []\n'
    f'[timer]\nbell = false\n'
    f'[[workspaces]]\nname = "fw"\npath = "{repo}"\n'
    f'[[task_sources]]\nname = "jira"\ncommand = "{jira}"\n'
    f'[audit]\ngerrit_command = "{mine}"\n'
)
state = os.path.join(work, "state")

screen = pyte.Screen(COLS, ROWS)
stream = pyte.ByteStream(screen)
env = {**os.environ, "TERM": "xterm-256color", "XDG_STATE_HOME": state}
child = pexpect.spawn(BIN, ["--config", cfg], dimensions=(ROWS, COLS), env=env, timeout=5)
# pahiri enables mouse reporting; make pyte ignore those private-mode requests quietly.
failures = []


def pump(seconds):
    end = time.time() + seconds
    while time.time() < end:
        try:
            stream.feed(child.read_nonblocking(65536, timeout=0.1))
        except pexpect.TIMEOUT:
            pass
        except pexpect.EOF:
            break


def text():
    return "\n".join(line.rstrip() for line in screen.display)


def dump(label, expect=None):
    print(f"===== {label} =====")
    print(text())
    print()
    if expect and expect not in text():
        failures.append(f"{label}: expected {expect!r}")


def send(keys, seconds=0.6):
    child.send(keys)
    pump(seconds)


def wait_for(needle, seconds=10):
    end = time.time() + seconds
    while time.time() < end:
        pump(0.2)
        if needle in text():
            return True
    return False


wait_for("DOING 1")
dump("task list", "DOING 1")
send("\x1b")                                   # palette
dump("palette", "new task")
send("n")                                      # new task chooser
dump("new task chooser", "from jira")
send("2")
wait_for("PROJ-42")
dump("tickets", "Fix the flux capacitor")
send("\r", 1.0)
dump("task created", "PROJ-42")
send("\r", 1.0)                                # open PROJ-42
send("\x1b"); send("o", 0.8)                   # open CONTEXT.md
dump("context of ticket task", "Link: https://jira.example.com/browse/PROJ-42")


def fg_at(needle):
    """Foreground colour of the first char of `needle` on screen."""
    for y, line in enumerate(screen.display):
        x = line.find(needle)
        if x >= 0:
            return screen.buffer[y][x].fg
    return None


heading_fg = fg_at("# PROJ-42")
plain_fg = fg_at("It does not flux")
print("heading colour:", heading_fg, "plain colour:", plain_fg)
if heading_fg == plain_fg:
    failures.append("markdown heading not highlighted")
# Editor selection: Ctrl+Home, Shift+↓ selects and highlights; Ctrl+C copies, not quits.
send("\x1b[1;5H", 0.3)
send("\x1b[1;2B", 0.5)
dump("keyboard selection", "2 lines selected")
sel_bg = None
for y, line in enumerate(screen.display):
    x = line.find("# PROJ-42")
    if x >= 0:
        sel_bg = screen.buffer[y][x].bg
if sel_bg in (None, "default"):
    failures.append("selection is not highlighted")
send("\x03", 0.5)
dump("ctrl+c copies", "copied")
send("\x1b[C", 0.3)                            # → drops the selection
# Mouse drag from the heading down one row selects.
for y, line in enumerate(screen.display):
    x = line.find("# PROJ-42")
    if x >= 0:
        child.send(f"\x1b[<0;{x + 3};{y + 1}M\x1b[<32;{x + 3};{y + 2}M\x1b[<0;{x + 3};{y + 2}m")
        break
pump(0.5)
dump("mouse drag selection", "lines selected")
send("\x1b[C", 0.3)
# Mouse: click on the CONTEXT.md row of the file tree (col 3, row 3) selects it and
# focuses the files pane; a second click opens it (already open → focus editor).
child.send("\x1b[<0;3;3M\x1b[<0;3;3m"); pump(0.5)
dump("after mouse click on files", "files")
send("\x1b"); send("a", 0.8)                   # attach
dump("attach", "[ ] code")
send(" "); send("\r", 0.8)
dump("attached", "code: fw")
send("\x1b"); send("p", 0.8)                   # prepare
wait_for("done:")
dump("prepared", "created branch 'PROJ-42'")
send("\r", 0.8)
send("\x1b"); send("s", 1.5)                   # new shell
send("cd code && git branch --show-current && pwd\r", 1.5)
wait_for("/firmware")
dump("shell cd code", "PROJ-42")
send("cd task && less CONTEXT.md\r")
wait_for("## Description")
dump("less inside pane", "CONTEXT.md")
send("q", 0.8)
send("\x02z", 0.8)                             # leader z: zoom
send("vim -u NONE CONTEXT.md\r")
wait_for('"CONTEXT.md"')
dump("vim zoomed", "[zoomed]")
send(":q\r", 1.0)
send("\x02z", 0.8)
send("\x02\x1b", 0.8)                          # leader Esc: palette from the shell
dump("palette from shell", "commands")
send("T", 0.8)
dump("back to list", "PLANNED")
send("\x1b"); send("c", 0.8)                   # config
dump("settings", "Code workspaces")
send("\x1b", 0.5)
dump("home: today pane", "No plan for today yet")

# AI context → ready → checkpoints → timer → time's up (flash) → done.
send("\r", 1.0)                                # open PROJ-42 again (terminal focused)
send("\x02q", 0.5)                             # leader q: leave the shell
send("\x1b"); send("i", 0.5)
wait_for("context ready: yes")
dump("ai context", "wrote")
send("\r", 0.5)
send("\x1b"); send("b", 0.5)
wait_for("written to ## Checkpoints")
dump("ai checkpoints", "Flux the capacitor")
send("\r", 0.5)
send("\x1b"); send("m", 0.5)                   # timer menu
dump("timer menu", "start: Tiny warm-up")
send("1", 0.1)                                 # start it: no time left on it
flashed = False
end = time.time() + 3
while time.time() < end and not flashed:
    pump(0.05)
    flashed = any(screen.buffer[y][x].reverse for y in range(ROWS) for x in range(0, COLS, 7))
wait_for("TIME'S UP")
dump("time's up (no popup, chip blinks)", "TIME'S UP")
if not flashed:
    failures.append("screen did not flash")
if "done → next" in text():
    failures.append("time's up must not open a popup")
send("\x1b"); send("m", 0.5)                   # open the timer menu yourself
dump("timer menu after alarm", "done → next: Flux the capacitor")
send("1", 0.8)                                 # done → next checkpoint
dump("timer running", "Flux the capacitor")
if "⏱ PROJ-42" not in text():
    failures.append("timer chip does not show the running timer")
send("\x1bOP", 0.8)                            # F1: help page
dump("help page", "pahiri help")
send("\x1b[C", 0.5); send("\x1b[C", 0.5); send("\x1b[C", 0.5)   # → Hooks tab
dump("help: hooks", "PAHIRI_PREV_TASK")
send("q", 0.5)
send("\x1b"); send("M", 0.5)                   # stop and book
send("\x1b"); send("T", 0.8)
# Plan the day: p → A (all columns: PROJ-42 is still in Planned) → s suggest → Enter.
send("p", 0.8)
dump("plan view", "What can be planned")
send("A", 0.5)
send("s", 0.5)
dump("plan suggested", "Today, in order")
if "PROJ-42 · Flux the capacitor" not in text():
    failures.append("suggest did not pick the open checkpoint")
send("\r", 0.8)
dump("home with a plan", "Plan  0/1 done")
send("\r", 0.8)                                # Enter on the plan item: timer
if "⏱ PROJ-42" not in text():
    failures.append("Enter in the Today pane did not start the timer")
dump("timer from the plan", "▶")
send("M", 0.5)                                 # M works from the Today pane too
# The audit: Esc U → proposal (rules + the fake agent) → a → y.
send("\x1b"); send("U", 0.3)
wait_for("What it does")
dump("audit view", "dts-cleanup")
if "Flux fix" not in text():
    failures.append("audit did not propose the PROJ-42 change")
send("a", 0.5)
dump("audit confirm", "Apply the audit?")
send("y", 1.0)
dump("audit applied", "audit applied: 1 created, 1 updated")
send("\x1b"); send("q", 0.5)                   # quit → confirm (shell running)
dump("quit confirm", "Quit pahiri?")
send("y", 1.0)
child.expect(pexpect.EOF, timeout=5)
print("exit status", child.exitstatus)
ctx = open(os.path.join(tasks, "PROJ-42", "CONTEXT.md")).read()
dts = open(os.path.join(tasks, "dts-cleanup", "CONTEXT.md")).read()
if "- gerrit: fw I501" not in dts or "- finished: 2026-07-05" not in dts:
    failures.append("audit did not write dts-cleanup's CONTEXT.md")
plans = os.path.join(tasks, ".pahiri", "plans")
if not os.path.isdir(plans) or not os.listdir(plans):
    failures.append("no plan file written")
for needle in ("- [x] Tiny warm-up", "- context_ready: true", "## Log", "- started: "):
    if needle not in ctx:
        failures.append(f"CONTEXT.md lacks {needle!r}")
branch = subprocess.run(["git", "-C", repo, "branch", "--show-current"], capture_output=True, text=True).stdout.strip()
print("workspace branch:", branch)
if branch != "PROJ-42":
    failures.append(f"workspace branch is {branch!r}")
if failures:
    print("FAILURES:\n  " + "\n  ".join(failures))
    sys.exit(1)
print("smoke OK")
