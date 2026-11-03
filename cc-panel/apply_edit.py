#!/usr/bin/env python3
"""Rebuild the file as one Write/Edit call left it, for the ccx diff pane. Standard library only.

Reads the PostToolUse hook JSON on stdin. Takes the snapshot hook.sh made before the call
(~/.cache/cc-panel/$CC_PANEL_ID/snap/<key>.snap, status in <key>.meta) and applies the tool input:
  Edit   old_string -> new_string (first occurrence, or every one with replace_all)
  Write  tool_input.content
and writes <key>.after next to the snapshot. Nothing is written (the pane then compares with the
current file) when: old_string is not in the snapshot, the snapshot is not UTF-8, the tool is
NotebookEdit, there is no snapshot (over 2 MB or copy failed), or the result is over 2 MB.
.after files whose .meta was pruned (hook.sh keeps the last 20) are removed, so the same 20 are kept.
Never prints; exit status is always 0.
"""
import json
import os
import re
import sys

LIMIT = 2097152
D = os.path.expanduser("~/.cache/cc-panel")


def build(tool, ti, status, snap):
    if status not in ("copied", "new"):
        return None
    if status == "copied":
        try:
            with open(snap, "rb") as f:
                base = f.read().decode("utf-8")
        except (OSError, UnicodeDecodeError):
            return None
    else:
        base = ""
    if tool == "Write":
        content = ti.get("content")
        return content if isinstance(content, str) else None
    if tool == "Edit":
        old, new = ti.get("old_string"), ti.get("new_string")
        if not isinstance(old, str) or not isinstance(new, str):
            return None
        if old == "":
            return new if base == "" else None
        if old not in base:
            return None
        return base.replace(old, new) if ti.get("replace_all") else base.replace(old, new, 1)
    return None


def prune(snapdir):
    try:
        names = os.listdir(snapdir)
    except OSError:
        return
    metas = {n[:-5] for n in names if n.endswith(".meta")}
    for n in names:
        if n.endswith(".after") and n[:-6] not in metas:
            try:
                os.unlink(os.path.join(snapdir, n))
            except OSError:
                pass


def main():
    pid = os.environ.get("CC_PANEL_ID", "")
    if not pid or not re.fullmatch(r"[A-Za-z0-9_-]+", pid):
        return
    try:
        data = json.load(sys.stdin)
    except ValueError:
        return
    tool = data.get("tool_name")
    ti = data.get("tool_input") or {}
    tuid = data.get("tool_use_id") or ""
    if not tuid or not isinstance(ti, dict):
        return
    key = re.sub(r"[^A-Za-z0-9_-]", "_", tuid)  # same as hook.sh
    snapdir = os.path.join(D, pid, "snap")
    try:
        with open(os.path.join(snapdir, f"{key}.meta"), encoding="utf-8") as f:
            status = f.readline().rstrip("\n").split("\t")[1]
    except (OSError, IndexError):
        status = ""
    after = build(tool, ti, status, os.path.join(snapdir, f"{key}.snap"))
    if after is not None:
        raw = after.encode("utf-8")
        if len(raw) <= LIMIT:
            tmp = os.path.join(snapdir, f".{key}.after.{os.getpid()}")
            try:
                with open(tmp, "wb") as f:
                    f.write(raw)
                os.replace(tmp, os.path.join(snapdir, f"{key}.after"))
            except OSError:
                try:
                    os.unlink(tmp)
                except OSError:
                    pass
    prune(snapdir)


if __name__ == "__main__":
    try:
        main()
    except Exception:  # a hook must never fail the tool call
        pass
    sys.exit(0)
