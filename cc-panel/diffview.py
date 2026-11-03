#!/usr/bin/env python3
"""ccx right-bottom pane: side-by-side diff with delta. Standard library only (plus the delta binary).

Views (key m or Tab cycles):
  edit  last edit only: snapshot taken by the PreToolUse hook vs the file now (default)
  head  last edited file vs git HEAD
  tree  whole working tree vs HEAD, untracked files included (shows Bash-made changes too)
Keys: m/Tab view · j/k line · Space/b page · g/G top/bottom · r refresh
"""
import os
import select
import shutil
import signal
import subprocess
import sys
import termios
import time
import tty

ID = os.environ.get("CC_PANEL_ID", "")
START_DIR = os.environ.get("CC_PANEL_DIR", os.getcwd())
D = os.path.expanduser("~/.cache/cc-panel")
LAST = os.path.join(D, f"{ID}.last")
SNAP = os.path.join(D, ID, "snap")
DELTA = shutil.which("delta") or os.path.expanduser("~/.local/bin/delta")
MODES = ["edit", "head", "tree"]
# Below this pane width the diff is drawn top/bottom (unified) instead of side by side
SBS_MIN = int(os.environ.get("CCX_SBS_MIN", "110") or 110)
LABEL = {"edit": "last edit", "head": "vs HEAD", "tree": "working tree"}

R = "\033[0m"
INV = "\033[7m"
DIM = "\033[2m"


def run(cmd, cwd=None, inp=None):
    try:
        p = subprocess.run(cmd, cwd=cwd, input=inp, capture_output=True, text=True, timeout=10)
        return p.returncode, p.stdout
    except (OSError, subprocess.TimeoutExpired):
        return 127, ""


def git_top(path):
    d = path if os.path.isdir(path) else os.path.dirname(path) or "."
    if not os.path.isdir(d):
        return None
    rc, out = run(["git", "-C", d, "rev-parse", "--show-toplevel"])
    return out.strip() if rc == 0 and out.strip() else None


def read_last():
    try:
        with open(LAST, encoding="utf-8") as f:
            parts = f.readline().rstrip("\n").split("\t")
        path, ts, key, agent = (parts + ["", "", "", ""])[:4]
        return {"path": path, "ts": ts, "key": key, "agent": agent or "main"}
    except OSError:
        return None


def rel(path, top):
    return os.path.relpath(path, top) if top else path


def diff_edit(last):
    """Return (unified_diff_text, note)."""
    meta = os.path.join(SNAP, f"{last['key']}.meta")
    try:
        with open(meta, encoding="utf-8") as f:
            status = f.readline().rstrip("\n").split("\t")[1]
    except (OSError, IndexError):
        return "", "no snapshot for this edit (kept: last 20)"
    path = last["path"]
    name = rel(path, git_top(path))
    if status == "copied":
        _, out = run(["diff", "-u", "--label", f"a/{name}", "--label", f"b/{name}",
                      os.path.join(SNAP, f"{last['key']}.snap"), path])
        return out, ""
    if status == "new":
        _, out = run(["diff", "-u", "--label", "/dev/null", "--label", f"b/{name}", "/dev/null", path])
        return out, "new file"
    if status == "toolarge":
        return "", "file over 2 MB: no snapshot"
    return "", "snapshot failed"


def untracked_diffs(top, paths):
    out = []
    for p in paths:
        _, d = run(["git", "-C", top, "diff", "--no-index", "--no-color", "--", "/dev/null", p])
        out.append(d)
    return "".join(out)


def diff_head(path):
    top = git_top(path)
    if not top:
        return "", "outside a git repository: no HEAD to compare"
    r = rel(path, top)
    rc, _ = run(["git", "-C", top, "ls-files", "--error-unmatch", "--", r])
    if rc != 0:
        return untracked_diffs(top, [r]), "untracked file"
    rc, _ = run(["git", "-C", top, "rev-parse", "--verify", "-q", "HEAD"])
    base = "HEAD" if rc == 0 else "4b825dc642cb6eb9a060e54bf8d69288fbee4904"  # empty tree
    _, out = run(["git", "-C", top, "diff", "--no-color", "--no-ext-diff", base, "--", r])
    return out, ""


def diff_tree(last):
    top = git_top(last["path"]) if last else None
    top = top or git_top(START_DIR)
    if not top:
        return "", "outside a git repository", None
    rc, _ = run(["git", "-C", top, "rev-parse", "--verify", "-q", "HEAD"])
    base = "HEAD" if rc == 0 else "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
    _, out = run(["git", "-C", top, "diff", "--no-color", "--no-ext-diff", base])
    _, others = run(["git", "-C", top, "ls-files", "--others", "--exclude-standard"])
    files = [f for f in others.splitlines() if f][:50]
    return out + untracked_diffs(top, files), "", top


def counts(udiff):
    a = r = 0
    for ln in udiff.splitlines():
        if ln.startswith("+") and not ln.startswith("+++"):
            a += 1
        elif ln.startswith("-") and not ln.startswith("---"):
            r += 1
    return a, r


def delta(udiff, width, files=False, side=True):
    if not udiff.strip():
        return []
    style = ["--file-style=bold yellow", "--file-decoration-style=yellow ul"] if files else ["--file-style=omit"]
    layout = ["--side-by-side"] if side else []
    rc, out = run([DELTA, *layout, "--line-numbers", "--dark", "--true-color=always",
                   "--paging=never", f"--width={width}", "--hunk-header-style=omit", *style],
                  inp=udiff)
    if rc != 0 or not out:
        return udiff.splitlines()
    return out.rstrip("\n").split("\n")


class View:
    def __init__(self):
        self.mode = 0
        self.scroll = 0
        self.lines = []
        self.header = ""
        self.sig = None
        self.frame = None

    def compute(self, width):
        last = read_last()
        mode = MODES[self.mode]
        who = last["agent"] if last else "-"
        if mode == "tree":
            udiff, note, top = diff_tree(last)
            target = top or "-"
        elif not last:
            udiff, note, target = "", "waiting for the first edit", "-"
        elif mode == "edit":
            udiff, note = diff_edit(last)
            target = last["path"]
        else:
            udiff, note = diff_head(last["path"])
            target = last["path"]
        sig = (mode, width, udiff, note, target, who)
        if sig == self.sig:
            return
        if self.sig is None or self.sig[0] != mode or self.sig[4] != target or self.sig[2] != udiff:
            self.scroll = 0
        self.sig = sig
        a, r = counts(udiff)
        by = "" if mode == "tree" else f" · by {who}"
        tail = f" · {note}" if note else ""
        side = width >= SBS_MIN
        narrow = "" if side else f" · top/bottom (pane < {SBS_MIN} cols)"
        self.header = f"[{LABEL[mode]}] {target} · +{a} -{r}{by}{narrow}{tail}"
        body = delta(udiff, width, files=(mode == "tree"), side=side)
        if not body:
            body = [f"{DIM}(no changes){R}" if not note else f"{DIM}{note}{R}"]
        self.lines = body

    def draw(self, width, height):
        hint = " m:view j/k space/b"
        head = self.header
        room = width - len(hint)
        if len(head) > room:
            head = "…" + head[-(room - 1):] if room > 1 else ""
        top = INV + head.ljust(room) + hint + R
        h = max(1, height - 1)
        self.scroll = max(0, min(self.scroll, max(0, len(self.lines) - h)))
        body = self.lines[self.scroll:self.scroll + h]
        frame = "\033[H" + top + "\033[K"
        for ln in body:
            frame += "\r\n" + ln + R + "\033[K"
        frame += "\033[J"
        if frame != self.frame:
            sys.stdout.write(frame)
            sys.stdout.flush()
            self.frame = frame

    def key(self, ch, height):
        h = max(1, height - 1)
        if ch in ("m", "\t"):
            self.mode = (self.mode + 1) % len(MODES)
            self.sig = None
        elif ch == "j":
            self.scroll += 1
        elif ch == "k":
            self.scroll -= 1
        elif ch == " ":
            self.scroll += h
        elif ch == "b":
            self.scroll -= h
        elif ch == "g":
            self.scroll = 0
        elif ch == "G":
            self.scroll = len(self.lines)
        elif ch == "r":
            self.sig = None


def main():
    if not ID:
        print("CC_PANEL_ID is not set; run this through ccx.")
        time.sleep(3600)
        return
    fd = sys.stdin.fileno()
    old = termios.tcgetattr(fd) if os.isatty(fd) else None
    if old:
        tty.setcbreak(fd)
    sys.stdout.write("\033[?25l")

    def bye(*_):
        if old:
            termios.tcsetattr(fd, termios.TCSADRAIN, old)
        sys.stdout.write("\033[?25h" + R)
        sys.stdout.flush()
        sys.exit(0)

    signal.signal(signal.SIGTERM, bye)
    signal.signal(signal.SIGHUP, bye)
    v = View()
    next_check = 0.0
    try:
        while True:
            size = shutil.get_terminal_size((100, 40))
            now = time.time()
            if now >= next_check:
                v.compute(size.columns)
                next_check = now + 1.0
            v.draw(size.columns, size.lines)
            r, _, _ = select.select([sys.stdin], [], [], 0.25) if old else ([], [], time.sleep(0.25))
            if r:
                ch = os.read(fd, 1).decode(errors="ignore")
                v.key(ch, size.lines)
                if ch in ("m", "\t", "r"):
                    next_check = 0.0
    except KeyboardInterrupt:
        bye()


if __name__ == "__main__":
    main()
