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

cfg = os.path.join(work, "config.toml")
open(cfg, "w").write(
    f'tasks_dir = "{tasks}"\n'
    f'[shell]\nprogram = "bash"\nargs = ["--norc", "-i"]\n'
    f'[[workspaces]]\nname = "fw"\npath = "{repo}"\n'
    f'[[task_sources]]\nname = "jira"\ncommand = "{jira}"\n'
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
send("\x1b"); send("q", 0.5)                   # quit → confirm (shell running)
dump("quit confirm", "Quit pahiri?")
send("y", 1.0)
child.expect(pexpect.EOF, timeout=5)
print("exit status", child.exitstatus)
branch = subprocess.run(["git", "-C", repo, "branch", "--show-current"], capture_output=True, text=True).stdout.strip()
print("workspace branch:", branch)
if branch != "PROJ-42":
    failures.append(f"workspace branch is {branch!r}")
if failures:
    print("FAILURES:\n  " + "\n  ".join(failures))
    sys.exit(1)
print("smoke OK")
