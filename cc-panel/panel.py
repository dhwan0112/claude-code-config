#!/usr/bin/env python3
"""ccx status panel (right pane): session, context, limits, orchestration. Standard library only.

Reads ~/.cache/cc-panel/$CC_PANEL_ID.json (status line input, written by statusline-tee.sh) and
$CC_PANEL_ID.orch (orchestration records, written by hook.sh) once a second, and redraws in place
with cursor movement. On a pane size change it clears the screen first, so no stale lines remain.
When the pane is too short, the lowest-priority rows are folded first (key help, cache, lines, cost/updated,
limits); session, model, context and the orchestration section stay.
"""
import json
import os
import shutil
import signal
import sys
import time
import unicodedata

ID = os.environ.get("CC_PANEL_ID", "")
D = os.path.expanduser("~/.cache/cc-panel")
JSON = os.path.join(D, f"{ID}.json")
ORCH = os.path.join(D, f"{ID}.orch")

R = "\033[0m"
DIM = "\033[2m"
BOLD = "\033[1m"
CYAN = "\033[36m"
GREEN = "\033[32m"
YELLOW = "\033[33m"
RED = "\033[31m"

# Row priorities: 0 is never folded, higher numbers fold first
P_MAIN, P_LIMITS, P_COST, P_LINES, P_CACHE, P_KEYS = 0, 1, 2, 3, 4, 5
# Key help shown at the bottom of the panel (folded first when the pane is short); the tab bar stays free for tabs
KEYS = (  # (key, description, short description for narrow panes)
    ("C-a c", "new tab", "new tab"), ("C-a C", "new tab in a folder…", "tab (dir)"), ("C-a S", "new tab (safe)", "safe tab"),
    ("C-a n/p", "next/previous tab", "next/prev"), ("C-a z", "zoom pane", "zoom"), ("C-a d", "fold/unfold diff pane", "fold diff"),
    ("C-a w", "save pane sizes", "save size"), ("C-a ←↑↓→", "resize panes", "resize"), ("C-a hjkl", "move between panes", "focus"),
    ("C-a ,", "rename tab + session", "rename"), ("drag tab", "move it along the bar", "move tab"),
    ("C-a D", "detach", "detach"), ("C-a C-a", "send C-a", "send C-a"),
)
MAX_DONE = 6  # done workers shown
DESC_LEN = 40  # at most this many columns of the task description for a running worker (less in narrow panes)

# Rate limit windows: always shown (with "-" when the JSON has none), then any other key the JSON gives
LIMIT_LABELS = {
    "five_hour": "5h",
    "seven_day": "7d all",
    "seven_day_opus": "7d opus",
    "seven_day_sonnet": "7d sonnet",
    "seven_day_oauth_apps": "7d apps",
}
ALWAYS_LIMITS = ("five_hour", "seven_day")


def width_of(s):
    return sum(2 if unicodedata.east_asian_width(c) in "WF" else 1 for c in s)


def cut(s, w):
    """Truncate s to display width w (with … when cut)."""
    if width_of(s) <= w:
        return s
    out, used = "", 0
    for c in s:
        cw = 2 if unicodedata.east_asian_width(c) in "WF" else 1
        if used + cw > w - 1:
            break
        out += c
        used += cw
    return out + "…"


def load_json():
    try:
        with open(JSON, encoding="utf-8") as f:
            return json.load(f), os.path.getmtime(JSON)
    except (OSError, ValueError):
        return None, None


def get(d, *keys, default=None):
    for k in keys:
        if not isinstance(d, dict) or k not in d or d[k] is None:
            return default
        d = d[k]
    return d


def human(n):
    if n is None:
        return "-"
    n = int(n)
    if n >= 1_000_000:
        return f"{n / 1_000_000:.1f}M".replace(".0M", "M")
    if n >= 1000:
        return f"{n // 1000}k"
    return str(n)


def level_color(pct):
    return RED if pct >= 80 else YELLOW if pct >= 50 else GREEN


def dur(seconds, running):
    s = max(0, int(seconds))
    if running:
        if s < 60:
            return f"{s}s"
        if s < 3600:
            return f"{s // 60}m{s % 60:02d}s"
        return f"{s // 3600}h{(s % 3600) // 60:02d}m"
    if s < 60:
        return f"{s}s"
    if s < 3600:
        return f"{round(s / 60)}m"
    return f"{s // 3600}h{(s % 3600) // 60:02d}m"


# ---------------------------------------------------------------- orchestration records

def load_orch():
    """Return a list of worker entries from <id>.orch, oldest first.

    Records (hook.sh): event, time, tool_use_id, agent_id, agent_type, description, status.
    One worker = one Agent tool call (tool_use_id). SubagentStart/Stop records (agent_id) are joined to it
    through the agentId in the PostToolUse response, or, while it runs, to the oldest running call of the
    same type that has no agent yet. Start/stop records with no matching call stand on their own.
    """
    try:
        with open(ORCH, encoding="utf-8") as f:
            recs = [ln.rstrip("\n").split("\t") for ln in f if ln.strip()]
    except OSError:
        return []
    entries = {}  # key -> entry
    order = []
    by_aid = {}

    def new(key, typ, desc, start):
        e = {"key": key, "type": typ or "?", "desc": desc, "start": start, "end": None,
             "status": None, "aid": None, "sstart": None, "sstop": None}
        entries[key] = e
        order.append(key)
        return e

    for r in recs:
        r = (r + [""] * 7)[:7]
        ev, ts, tuid, aid, typ, desc, status = r
        try:
            t = float(ts)
        except ValueError:
            continue
        if ev == "agentpre" and tuid:
            new(tuid, typ, desc, t)
        elif ev == "substart" and aid:
            if by_aid.get(aid) in entries:  # PostToolUse already named this agent (it can arrive before Start)
                e = entries[by_aid[aid]]
                e["sstart"] = t
                continue
            cand = [entries[k] for k in order
                    if entries[k]["aid"] is None and entries[k]["end"] is None and entries[k]["sstop"] is None
                    and entries[k]["key"] != entries[k].get("aid") and entries[k]["type"] == (typ or "?")
                    and not entries[k].get("standalone")]
            if cand:
                e = cand[0]
            else:
                e = new(aid, typ, "", t)
                e["standalone"] = True
            e["aid"] = aid
            e["sstart"] = t
            by_aid[aid] = e["key"]
        elif ev == "substop" and aid:
            key = by_aid.get(aid)
            e = entries.get(key) if key else None
            if e is None:
                e = new(aid, typ, "", t)
                e["standalone"] = True
                e["aid"] = aid
                by_aid[aid] = aid
            e["sstop"] = t
            if status and status != "-" and e["status"] in (None, "-"):
                e["status"] = status  # the Stop record carries the report; a PostToolUse "-" before it does not count
        elif ev == "agentpost" and tuid:
            e = entries.get(tuid) or new(tuid, typ, "", t)
            e["end"] = t
            if aid:
                other = by_aid.get(aid)
                if other and other != tuid and entries.get(other, {}).get("standalone"):
                    o = entries.pop(other)
                    order.remove(other)
                    e["sstart"], e["sstop"] = o["sstart"], o["sstop"]
                    if o["status"] and not e["status"]:
                        e["status"] = o["status"]
                e["aid"] = aid
                by_aid[aid] = tuid
            e["poststat"] = status or "-"
            if status and status != "-":
                e["status"] = status
            elif not e["status"]:
                e["status"] = "-"
    out = []
    for k in order:
        e = entries[k]
        # Start/stop records of agents with no type and no Agent call of their own (Claude Code's internal
        # agents, not workers) are not shown.
        if e.get("standalone") and e["type"] == "?":
            continue
        # Done when the subagent stopped, or when the Agent call returned and no subagent was ever linked to it.
        # A call that returned while its subagent still runs (background agent: PostToolUse right after
        # PreToolUse) stays "running" until SubagentStop.
        if e["sstop"] is not None:
            e["done"] = True
        elif e["end"] is None:
            e["done"] = False
        elif e["sstart"] is None:
            # No Start record: nothing to wait for, unless the call named an agent and returned within 2 s
            # of starting without a report (async_launched: its Start record is on its way).
            e["done"] = not (e["aid"] is not None and e["end"] - e["start"] <= 2 and e.get("poststat", "-") == "-")
        else:
            e["done"] = False  # started, not stopped: still running
        if e["done"] and not e["status"]:
            e["status"] = "-"
        e["finish"] = e["sstop"] if e["sstop"] is not None else e["end"]
        out.append(e)
    return out


def orch_rows(entries, width, compact, now):
    """Rows for the orchestration section: summary, running workers, last MAX_DONE done workers."""
    running = [e for e in entries if not e["done"]]
    done = [e for e in entries if e["done"]]
    counts = {}
    for e in done:
        counts[e["status"]] = counts.get(e["status"], 0) + 1
    parts = [f"{counts[s]} {s}" for s in ("verified", "blocked", "needs_review", "-") if counts.get(s)]
    narrow = width < 44  # 36-column panel: shorter lines (no duration on finished workers)
    summary = f"{len(running)} {'run' if narrow else 'running'} · {len(done)} done"
    if parts:
        summary += f" ({', '.join(parts)})"
    rows = [("workers", summary, YELLOW if running else "", P_MAIN)]
    val_w = max(10, width - (7 if compact else 9) - 2)  # value width of to_lines: one line per worker
    for e in running:
        head = f"● {e['type']}  {dur(now - e['start'], True)}"
        room = min(DESC_LEN, val_w - width_of(head) - 2)  # description cut to the room left, not wrapped
        desc = cut(e["desc"], room) if e["desc"] and room >= 6 else ""
        rows.append(("", f"{head}  {desc}".rstrip(), YELLOW, P_MAIN))
    marks = {"verified": ("✓", GREEN), "blocked": ("✗", RED), "needs_review": ("?", YELLOW), "-": ("-", DIM)}
    for e in done[-MAX_DONE:][::-1]:
        mark, color = marks.get(e["status"], ("-", DIM))
        took = "" if narrow else dur((e["finish"] or now) - e["start"], False) + "  "
        rows.append(("", f"{mark} {e['type']}  {took}{e['status']}", color, P_MAIN))
    return rows


# ---------------------------------------------------------------- rows

def rows(data, mtime, entries, width, compact, now):
    """Return (label, value, color, priority) rows. compact=True shortens values for narrow panes."""
    out = []
    if data is None:
        out.append(("status", "대기 중 (첫 응답 후 표시)", DIM, P_MAIN))
        out += orch_rows(entries, width, compact, now)
        return out + key_rows(width, compact)
    name = data.get("session_name") or (data.get("session_id") or "-")[:8]
    out.append(("session", name, BOLD, P_MAIN))
    model = get(data, "model", "display_name") or get(data, "model", "id") or "-"
    effort = get(data, "effort", "level")
    out.append(("model", f"{model} · {effort}" if effort else model, CYAN, P_MAIN))

    pct = get(data, "context_window", "used_percentage")
    size = get(data, "context_window", "context_window_size")
    cu = get(data, "context_window", "current_usage")
    used = None
    if isinstance(cu, dict):
        used = sum(cu.get(k) or 0 for k in ("input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"))
    elif get(data, "context_window", "total_input_tokens") is not None:
        used = get(data, "context_window", "total_input_tokens")
    if pct is None:
        out.append(("context", "-", "", P_MAIN))
    else:
        p = int(pct)
        barw = max(5, min(12, width - 26)) if compact else max(5, min(20, width - 34))
        fill = round(p * barw / 100)
        bar = "█" * fill + "░" * (barw - fill)
        out.append(("context", (bar, f" {p}% {human(used)}/{human(size)}"), level_color(p), P_MAIN))

    out += orch_rows(entries, width, compact, now)

    # Rate limits: every window the JSON gives, as "name · used% · reset"; 5h and 7d always, "-" when absent
    rl = data.get("rate_limits") if isinstance(data.get("rate_limits"), dict) else {}
    keys = list(ALWAYS_LIMITS) + [k for k in rl if k not in ALWAYS_LIMITS]
    first = True
    for key in keys:
        win = rl.get(key) if isinstance(rl.get(key), dict) else {}
        lp = win.get("used_percentage")
        reset = win.get("resets_at")
        if isinstance(reset, (int, float)):
            when = time.strftime("%m-%d %H:%M", time.localtime(reset))
        else:
            when = "-"
        label = LIMIT_LABELS.get(key, key.replace("_", " "))
        if lp is None:
            value, color = f"{label} · - · {when}", DIM
        else:
            value, color = f"{label} · {int(lp)}% · {when}", level_color(int(lp))
        out.append(("limits" if first else "", value, color, P_LIMITS))
        first = False

    cost = get(data, "cost", "total_cost_usd")
    if cost is None:
        out.append(("cost", "-", "", P_COST))
    elif rl:
        out.append(("cost", f"~${cost:.2f} API est." if compact else f"~${cost:.2f} (API 환산 추정, 구독)", DIM, P_COST))
    else:
        out.append(("cost", f"${cost:.2f} est." if compact else f"${cost:.2f} (추정)", "", P_COST))

    pc = data.get("prompt_cache") or {}
    if pc:
        state = "warm" if pc.get("warm") else "cold"
        hit = pc.get("hit_ratio")
        hit_s = f"{hit * 100:.0f}%" if isinstance(hit, (int, float)) else "-"
        if compact:
            value = f"{state} {pc.get('ttl', '-')} hit {hit_s} m{pc.get('misses', '-')}"
        else:
            value = f"{state} · ttl {pc.get('ttl', '-')} · hit {hit_s} · miss {pc.get('misses', '-')}"
        out.append(("cache", value, GREEN if pc.get("warm") else YELLOW, P_CACHE))
    else:
        out.append(("cache", "-", "", P_CACHE))

    la = get(data, "cost", "total_lines_added")
    lr = get(data, "cost", "total_lines_removed")
    out.append(("lines", f"+{la if la is not None else '-'} -{lr if lr is not None else '-'}", "", P_LINES))
    age = int(now - mtime) if mtime else None
    out.append(("updated", f"{age}s ago" if age is not None else "-", DIM, P_COST))
    return out + key_rows(width, compact)


def key_rows(width, compact):
    """Key help: a blank row, then one row per key (label "keys" on the first). One row each, never wrapped: the
    description is the long form when it fits the row, else the short form (cut to the room left when even that is long)."""
    kw = max(width_of(k) for k, _, _ in KEYS)
    room = max(0, width - (7 if compact else 9) - 2 - kw - 2)  # value width of to_lines minus the key column
    rows = [("", "", "", P_KEYS)]
    for i, (k, what, short) in enumerate(KEYS):
        d = what if width_of(what) <= room else (cut(short, room) if room >= 4 else "")
        rows.append(("keys" if i == 0 else "", f"{k}{' ' * (kw - width_of(k))}  {d}".rstrip(), DIM, P_KEYS))
    return rows


def wrap(text, width):
    """Wrap by display width, at the last space when it lies in the second half of the line; returns chunks."""
    chunks, cur, w = [], "", 0
    for c in text:
        cw = 2 if unicodedata.east_asian_width(c) in "WF" else 1
        if w + cw > width and cur:
            sp = cur.rfind(" ")
            if sp > 0 and width_of(cur[:sp]) >= width // 2:
                chunks.append(cur[:sp].rstrip())
                cur = cur[sp + 1:]
            else:
                chunks.append(cur)
                cur = ""
            w = width_of(cur)
        cur += c
        w += cw
    chunks.append(cur)
    return chunks


# Narrower than this, the panel uses short labels and values
COMPACT_BELOW = 56
SHORT = {"session": "sess", "context": "ctx", "updated": "upd"}


def to_lines(rowlist, width, compact):
    lab_w = 7 if compact else 9
    val_w = max(10, width - lab_w - 2)
    lines = []
    for label, value, color, _ in rowlist:
        if compact:
            label = SHORT.get(label, label)
        if isinstance(value, tuple):  # context bar: colored bar + plain text
            bar, rest = value
            text = bar + rest
            parts = wrap(text, val_w)
            first = parts[0]
            colored = color + first[:len(bar)] + R + first[len(bar):] if len(first) >= len(bar) else color + first + R
            lines.append(f"{DIM}{label:<{lab_w}}{R} {colored}")
            lines += [f"{'':<{lab_w}} {p}" for p in parts[1:]]
            continue
        parts = wrap(str(value), val_w)
        lines.append(f"{DIM}{label:<{lab_w}}{R} {color}{parts[0]}{R}")
        lines += [f"{'':<{lab_w}} {color}{p}{R}" for p in parts[1:]]
    return lines


def fit(rowlist, width, height, compact):
    """Fold rows until they fit: lowest priority first, then the oldest done workers."""
    rowlist = list(rowlist)
    lines = to_lines(rowlist, width, compact)
    prio = P_KEYS
    while len(lines) > height and prio > P_MAIN:
        rowlist = [r for r in rowlist if r[3] != prio]
        prio -= 1
        lines = to_lines(rowlist, width, compact)
    while len(lines) > height:
        # drop the last done-worker row (they are listed newest first after the running ones)
        idx = [i for i, r in enumerate(rowlist) if r[0] == "" and r[1][:1] in "✓✗?-"]
        if not idx:
            break
        del rowlist[idx[-1]]
        lines = to_lines(rowlist, width, compact)
    return lines[:height]


def render(width, height):
    data, mtime = load_json()
    entries = load_orch()
    compact = width < COMPACT_BELOW
    now = time.time()
    lines = fit(rows(data, mtime, entries, width, compact, now), width, height, compact)
    return "\033[H" + "".join(ln + "\033[K\r\n" for ln in lines[:-1]) + (lines[-1] + "\033[K" if lines else "") + "\033[J"


def main():
    if not ID:
        print("CC_PANEL_ID is not set; run this through ccx.")
        time.sleep(3600)
        return
    sys.stdout.write("\033[?25l")  # hide cursor
    sys.stdout.flush()

    def bye(*_):
        sys.stdout.write("\033[?25h")
        sys.stdout.flush()
        sys.exit(0)

    signal.signal(signal.SIGTERM, bye)
    signal.signal(signal.SIGHUP, bye)
    last = None
    last_size = None
    try:
        while True:
            size = shutil.get_terminal_size((60, 14))
            frame = render(size.columns, size.lines)
            if size != last_size:
                # pane resized: clear everything (tmux reflowed the old lines), then draw fresh
                sys.stdout.write("\033[2J")
                last_size = size
                last = None
            if frame != last:
                sys.stdout.write(frame)
                sys.stdout.flush()
                last = frame
            time.sleep(1)
    except KeyboardInterrupt:
        bye()


if __name__ == "__main__":
    main()
