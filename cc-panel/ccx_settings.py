#!/usr/bin/env python3
"""Build the --settings JSON for a ccx session and merge in the user's own --settings value.

Usage: ccx_settings.py [USER_SETTINGS]   (USER_SETTINGS is a JSON string or a file path)
Claude Code applies only the last --settings flag it gets, so ccx passes exactly one.
Merge rules: the user's keys override ccx's, except
  - "hooks": both sets are kept (ccx's hooks first, then the user's, per event);
  - "statusLine": ccx's is kept because the panel reads its data; a warning goes to stderr.
"""
import json
import os
import sys

P = os.path.expanduser("~/.claude/cc-panel")


def hook(arg, matcher=None, background=False):
    h = {"type": "command", "command": f'"{P}/hook.sh" {arg}', "timeout": 5}
    if background:
        h["async"] = True  # does not hold up the tool; the diff pane polls every second anyway
    entry = {"hooks": [h]}
    if matcher:
        entry["matcher"] = matcher
    return [entry]


def base():
    edit = "^(Write|Edit|NotebookEdit)$"
    return {
        "tui": "default",
        "statusLine": {"type": "command", "command": f'"{P}/statusline-tee.sh"', "refreshInterval": 3},
        "hooks": {
            "PreToolUse": hook("pre", edit),
            "PostToolUse": hook("post", edit, background=True),
            "SubagentStart": hook("substart"),
            "SubagentStop": hook("substop"),
        },
    }


def load_user(value):
    if not value:
        return {}
    text = value
    if not value.lstrip().startswith("{"):
        with open(os.path.expanduser(value), encoding="utf-8") as f:
            text = f.read()
    data = json.loads(text)
    if not isinstance(data, dict):
        raise ValueError("--settings must be a JSON object")
    return data


def merge(ours, user):
    out = dict(ours)
    for k, v in user.items():
        if k == "hooks" and isinstance(v, dict):
            hooks = {e: list(lst) for e, lst in ours.get("hooks", {}).items()}
            for event, entries in v.items():
                hooks.setdefault(event, [])
                hooks[event] = hooks[event] + list(entries)
            out["hooks"] = hooks
        elif k == "statusLine":
            print("ccx: ignoring statusLine from --settings (the panel needs its own)", file=sys.stderr)
        else:
            out[k] = v
    return out


def main():
    try:
        user = load_user(sys.argv[1] if len(sys.argv) > 1 else "")
    except (OSError, ValueError) as e:
        print(f"ccx: cannot read --settings value: {e}", file=sys.stderr)
        return 2
    json.dump(merge(base(), user), sys.stdout, ensure_ascii=False)
    return 0


if __name__ == "__main__":
    sys.exit(main())
