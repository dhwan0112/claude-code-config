# Stage PD's fake claude for inline-diff.sh: edit blocks in Claude Code 2.1.330's shape (a number column from column 6,
# the mark, the line backgrounds over 108 columns) and the edit tools' hooks through `haetty hook`, step by step on
# files in DIR; counts the bytes it receives into DIR/input. usage: inline-claude.py BIN DIR FILE
import json, os, subprocess, sys, termios, threading, time, tty

BIN, DIR, FILE = sys.argv[1:4]
DEL, INS = "\x1b[48;2;61;1;0m", "\x1b[48;2;2;40;0m"
base = {"session_id": "5b2c9e0d-1a3f-4c6b-9d8e-7f6a5b4c3d2e", "cwd": os.path.dirname(FILE)}
got = 0
tty.setraw(0)


def reader():
    global got
    while True:
        b = os.read(0, 4096)
        if not b:
            return
        got += len(b)
        open(os.path.join(DIR, "input"), "w").write(str(got))


threading.Thread(target=reader, daemon=True).start()
open(os.path.join(DIR, "input"), "w").write("0")


def out(s):
    sys.stdout.write(s.replace("\n", "\r\n"))
    sys.stdout.flush()


def wait(step):
    while not os.path.exists(os.path.join(DIR, step)):
        time.sleep(0.05)


def hook(event, j):
    subprocess.run([BIN, "hook", event], input=json.dumps({**base, **j}).encode(), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def block(path, start, lines):
    nw = len(str(start + len(lines)))
    o, n = start, start
    out(f"\n\x1b[32m●\x1b[0m \x1b[1mUpdate\x1b[0m({path})\n  ⎿  Added 1 line, removed 1 line\n")
    for l in lines:
        m, t = l[0], l[1:]
        num = n if m in " +" else o
        if m == " ":
            o += 1; n += 1
        elif m == "-":
            o += 1
        else:
            n += 1
        text = f"{num:>{nw}} {m}{t}"
        bg = {"-": DEL, "+": INS}.get(m, "")
        out(f"      {bg}{text.ljust(108)}\x1b[0m\n")


def edit(tid, old, new, lines, start):
    inp = {"file_path": FILE, "old_string": old, "new_string": new}
    hook("PreToolUse", {"tool_name": "Edit", "tool_use_id": tid, "tool_input": inp})
    text = open(FILE).read()
    open(FILE, "w").write(text.replace(old, new, 1))
    patch = [{"oldStart": start, "oldLines": 0, "newStart": start, "newLines": 0, "lines": lines}]
    hook("PostToolUse", {"tool_name": "Edit", "tool_use_id": tid, "tool_input": inp, "tool_response": {"structuredPatch": patch}})


hook("SessionStart", {"source": "startup", "model": "claude-haiku-5-5"})
hook("UserPromptSubmit", {})
out("fake claude ready\n")
L1 = [" SCALE = 10", " ", " ", "-def add(a, b):", "+def add_numbers(a, b):", "     # add two numbers", "     return a + b"]
wait("step1")
edit("t1", "def add(a, b):", "def add_numbers(a, b):", L1, 5)
block("a.py", 5, L1)
out("\n\x1b[37m●\x1b[0m Done with the first edit.\n")
wait("step2")
L2 = [" def add_numbers(a, b):", "-    # add two numbers", "+    # add two numbers, then return them", "     return a + b"]
block("a.py", 8, L2)
out("\n")
wait("step3")
edit("t2", "    # add two numbers\n", "    # add two numbers, then return them\n", L2, 8)
wait("stop")
