#!/usr/bin/env python3
"""Build the --settings JSON for a ccx session and merge in the user's own --settings value.

Usage: ccx_settings.py [USER_SETTINGS] [bypass]
  USER_SETTINGS is a JSON string or a file path ("" for none).
  "bypass" adds permissions.defaultMode=bypassPermissions (ccx's default; ccx --safe or CCX_SAFE=1 leaves
  it out, so the user's global defaultMode applies). A --permission-mode or --dangerously-skip-permissions
  flag on the claude command line still wins over any defaultMode.
Claude Code applies only the last --settings flag it gets, so ccx passes exactly one.
Merge rules: the user's keys override ccx's, except
  - "hooks": both sets are kept (ccx's hooks first, then the user's, per event);
  - "permissions": list keys (ask, allow, deny, ...) are concatenated, other keys (defaultMode, ...) the user's win;
  - "statusLine": ccx's is kept because the panel reads its data; a warning goes to stderr.
Permission rules from ~/.claude/settings.json still apply: list keys merge across settings scopes.
"""
import json
import os
import sys

P = os.environ.get("CCX_HOME") or os.path.expanduser("~/.claude/cc-panel")


# ccx-only "ask" rules: destructive git/gh commands still ask in bypass mode (ask rules apply in every mode).
# Each git rule also has a "git * <sub>" form for global options before the subcommand
# (git -C dir, git -c k=v, git --git-dir=..., git --no-pager). "*" matches any text, spaces included.
GIT_ASK = [
    "reset *--hard*",
    "clean *-*f*",                                 # -f, -fd, -xdf, --force, ...
    "checkout *.", "checkout *./", "checkout *:/",  # checkout [-- | <tree> --] . / ./ / :/
    "checkout -f*", "checkout * -f*", "checkout *--force*",
    "restore *.", "restore *./", "restore *:/",
    "branch *-D*", "branch *-d*-f*", "branch *-f*-d*", "branch *-df*", "branch *-fd*",  # also --delete --force
]
ASK = (
    [f"Bash(git {r})" for r in GIT_ASK]
    + [f"Bash(git * {r})" for r in GIT_ASK]
    + [
        "Bash(git * push*)",  # the global rules miss "git -C . push" with no arguments and "git -c k=v push"
        "Bash(gh repo delete *)", "Bash(gh * repo delete*)",
        "Bash(gh release delete *)", "Bash(gh * release delete*)",
        "Bash(gh api *DELETE*)", "Bash(gh api *delete*)",  # gh api -X DELETE repos/OWNER/REPO, ...
    ]
)


def hook(arg, matcher=None, background=False, script="hook.sh"):
    h = {"type": "command", "command": f'"{P}/{script}" {arg}'.rstrip(), "timeout": 5}
    if background:
        h["async"] = True  # does not hold up the tool; the diff pane polls every second anyway
    entry = {"hooks": [h]}
    if matcher:
        entry["matcher"] = matcher
    return [entry]


def base(bypass=False):
    edit = "^(Write|Edit|NotebookEdit)$"
    perms = {"ask": list(ASK)}
    if bypass:
        perms["defaultMode"] = "bypassPermissions"
    return {
        "tui": "default",
        "autoCompactWindow": 180000,  # compact at about 180k tokens in ccx sessions only; a user --settings value wins
        # statusline-tee.sh prints nothing (the status panel shows the data); it saves the JSON and renames the tab
        "statusLine": {"type": "command", "command": f'"{P}/statusline-tee.sh"', "refreshInterval": 3},
        "permissions": perms,
        "hooks": {
            # Agent tool calls and subagent start/stop feed the orchestration section of the status panel
            "PreToolUse": hook("pre", edit) + hook("agentpre", "^Agent$") + hook("attn clear", background=True),
            # post.sh rebuilds the file as this edit left it (.after), then runs "hook.sh post"
            "PostToolUse": hook("", edit, background=True, script="post.sh") + hook("agentpost", "^Agent$", background=True),
            "SubagentStart": hook("substart"),
            "SubagentStop": hook("substop"),
            # Tab attention (hook.sh attn): a turn that ended or an idle prompt marks the tab until it is looked at; a permission
            # prompt or a question marks it "ask" until the user answers (next tool call) or sends a prompt
            "Stop": hook("attn wait", background=True),
            "Notification": hook("attn ask", "permission_prompt|elicitation_dialog|elicitation_url_dialog|agent_needs_input", background=True)
                + hook("attn wait", "idle_prompt", background=True),
            "UserPromptSubmit": hook("attn clear", background=True),
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
        elif k == "permissions" and isinstance(v, dict):
            perms = dict(ours.get("permissions", {}))
            for pk, pv in v.items():
                if isinstance(pv, list) and isinstance(perms.get(pk), list):
                    perms[pk] = perms[pk] + [x for x in pv if x not in perms[pk]]
                else:
                    perms[pk] = pv
            out["permissions"] = perms
        elif k == "statusLine":
            print("ccx: ignoring statusLine from --settings (the panel needs its own)", file=sys.stderr)
        else:
            out[k] = v
    return out


def main():
    try:
        user = load_user(sys.argv[1] if len(sys.argv) > 1 else "")
        bypass = len(sys.argv) > 2 and sys.argv[2] == "bypass"
    except (OSError, ValueError) as e:
        print(f"ccx: cannot read --settings value: {e}", file=sys.stderr)
        return 2
    json.dump(merge(base(bypass), user), sys.stdout, ensure_ascii=False)
    return 0


if __name__ == "__main__":
    sys.exit(main())
