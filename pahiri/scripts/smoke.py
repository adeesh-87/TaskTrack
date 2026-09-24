#!/usr/bin/env python3
"""Drive the pahiri binary headlessly and print the rendered screens.

Usage: python3 scripts/smoke.py [path/to/pahiri]

Requires `pip install pexpect pyte`. Creates a throw-away tasks folder and
config, walks through the main flows (board → task → editor → shell →
full-screen program → zoom → settings → quit) and prints each screen.
"""
import os
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
os.makedirs(os.path.join(tasks, "second"))
open(os.path.join(tasks, "demo-task", "CONTEXT.md"), "w").write("# demo-task\n\nSome context.\n")
open(os.path.join(tasks, "demo-task", "scripts", "run.sh"), "w").write("echo hi\n")
open(os.path.join(tasks, "status.md"), "w").write("## Doing\n- demo-task\n")
cfg = os.path.join(work, "config.toml")
open(cfg, "w").write(f'tasks_dir = "{tasks}"\n[shell]\nprogram = "bash"\nargs = ["--norc", "-i"]\n')

screen = pyte.Screen(COLS, ROWS)
stream = pyte.ByteStream(screen)
child = pexpect.spawn(
    BIN, ["--config", cfg], dimensions=(ROWS, COLS), env={**os.environ, "TERM": "xterm-256color"}, timeout=5
)


def pump(seconds):
    end = time.time() + seconds
    while time.time() < end:
        try:
            stream.feed(child.read_nonblocking(65536, timeout=0.1))
        except pexpect.TIMEOUT:
            pass
        except pexpect.EOF:
            break


def dump(label):
    print(f"===== {label} =====")
    for line in screen.display:
        print(line.rstrip())
    print()


def send(keys, seconds=0.6):
    child.send(keys)
    pump(seconds)


pump(1.0)
dump("task list")
send("j")
send("\r", 1.0)
dump("task view")
send("\r")
send("j", 0.3)
send("\r", 0.8)
dump("editor open")
send("\x1b", 0.3)
send("t", 1.5)
send("echo PAHIRI-SHELL-OK $PAHIRI_TASK\r", 1.0)
send("less CONTEXT.md\r", 1.5)
dump("less inside pane")
send("q", 0.8)
send("\x02z", 0.8)
send("vim -u NONE CONTEXT.md\r", 2.0)
dump("vim zoomed")
send(":q\r", 1.0)
send("\x02z", 0.8)
send("\x02q", 0.8)
dump("after leader q")
send("\x1b", 0.5)
send(",", 0.8)
dump("settings")
send("\x1b", 0.5)
send("q", 0.5)
dump("quit confirm")
send("y", 1.0)
child.expect(pexpect.EOF, timeout=5)
print("exit status", child.exitstatus)
