#!/usr/bin/env python3
"""ccx right-top pane: context and token panel. Standard library only.

Reads ~/.cache/cc-panel/$CC_PANEL_ID.json (status line input, written by statusline-tee.sh)
and $CC_PANEL_ID.agents (running subagents, written by hook.sh) once a second, and redraws
in place with cursor movement (no full-screen clear).
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
AGENTS = os.path.join(D, f"{ID}.agents")

R = "\033[0m"
DIM = "\033[2m"
BOLD = "\033[1m"
CYAN = "\033[36m"
GREEN = "\033[32m"
YELLOW = "\033[33m"
RED = "\033[31m"


def width_of(s):
    return sum(2 if unicodedata.east_asian_width(c) in "WF" else 1 for c in s)


def load_json():
    try:
        with open(JSON, encoding="utf-8") as f:
            return json.load(f), os.path.getmtime(JSON)
    except (OSError, ValueError):
        return None, None


def load_agents():
    try:
        with open(AGENTS, encoding="utf-8") as f:
            rows = [ln.rstrip("\n").split("\t") for ln in f if ln.strip()]
        return [r[1] if len(r) > 1 else "?" for r in rows]
    except OSError:
        return []


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


def rows(data, mtime, agents, width, compact=False):
    """Return a list of (label, value, color) rows. compact=True shortens values for narrow panes."""
    if data is None:
        return [("status", "대기 중 (첫 응답 후 표시)", DIM), ("workers", fmt_workers(agents), "")]
    out = []
    name = data.get("session_name") or (data.get("session_id") or "-")[:8]
    out.append(("session", name, BOLD))
    model = get(data, "model", "display_name") or get(data, "model", "id") or "-"
    effort = get(data, "effort", "level")
    out.append(("model", f"{model} · {effort}" if effort else model, CYAN))

    pct = get(data, "context_window", "used_percentage")
    size = get(data, "context_window", "context_window_size")
    cu = get(data, "context_window", "current_usage")
    used = None
    if isinstance(cu, dict):
        used = sum(cu.get(k) or 0 for k in ("input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"))
    elif get(data, "context_window", "total_input_tokens") is not None:
        used = get(data, "context_window", "total_input_tokens")
    if pct is None:
        out.append(("context", "-", ""))
    else:
        p = int(pct)
        barw = max(5, min(12, width - 26)) if compact else max(5, min(20, width - 34))
        fill = round(p * barw / 100)
        bar = "█" * fill + "░" * (barw - fill)
        out.append(("context", (bar, f" {p}% {human(used)}/{human(size)}"), level_color(p)))

    limits = []
    for key, label in (("five_hour", "5h"), ("seven_day", "7d")):
        lp = get(data, "rate_limits", key, "used_percentage")
        if lp is None:
            continue
        reset = get(data, "rate_limits", key, "resets_at")
        when = time.strftime("%m-%d %H:%M", time.localtime(reset)) if reset else "-"
        limits.append((label, int(lp), when))
    if limits:
        for label, lp, when in limits:
            out.append((label, f"{lp}% · {when}" if compact else f"{lp}% · resets {when}", level_color(lp)))
    else:
        out.append(("7d", "-", DIM))

    cost = get(data, "cost", "total_cost_usd")
    if cost is None:
        out.append(("cost", "-", ""))
    elif data.get("rate_limits"):
        out.append(("cost", f"~${cost:.2f} API est." if compact else f"~${cost:.2f} (API 환산 추정, 구독)", DIM))
    else:
        out.append(("cost", f"${cost:.2f} est." if compact else f"${cost:.2f} (추정)", ""))

    pc = data.get("prompt_cache") or {}
    if pc:
        state = "warm" if pc.get("warm") else "cold"
        hit = pc.get("hit_ratio")
        hit_s = f"{hit * 100:.0f}%" if isinstance(hit, (int, float)) else "-"
        if compact:
            value = f"{state} {pc.get('ttl', '-')} hit {hit_s} m{pc.get('misses', '-')}"
        else:
            value = f"{state} · ttl {pc.get('ttl', '-')} · hit {hit_s} · miss {pc.get('misses', '-')}"
        out.append(("cache", value, GREEN if pc.get("warm") else YELLOW))
    else:
        out.append(("cache", "-", ""))

    la = get(data, "cost", "total_lines_added")
    lr = get(data, "cost", "total_lines_removed")
    out.append(("lines", f"+{la if la is not None else '-'} -{lr if lr is not None else '-'}", ""))
    out.append(("workers", fmt_workers(agents), YELLOW if agents else ""))
    age = int(time.time() - mtime) if mtime else None
    out.append(("updated", f"{age}s ago" if age is not None else "-", DIM))
    return out


def fmt_workers(agents):
    if not agents:
        return "0"
    return f"{len(agents)} ({', '.join(agents)})"


def wrap(text, width):
    """Wrap by display width; returns list of chunks."""
    chunks, cur, w = [], "", 0
    for c in text:
        cw = 2 if unicodedata.east_asian_width(c) in "WF" else 1
        if w + cw > width and cur:
            chunks.append(cur)
            cur, w = "", 0
        cur += c
        w += cw
    chunks.append(cur)
    return chunks


# Narrower than this, the panel uses short labels and values
COMPACT_BELOW = 56
SHORT = {"session": "sess", "context": "ctx", "updated": "upd"}


def render(width, height):
    data, mtime = load_json()
    agents = load_agents()
    compact = width < COMPACT_BELOW
    lab_w = 7 if compact else 9
    val_w = max(10, width - lab_w - 2)
    lines = []
    for label, value, color in rows(data, mtime, agents, width, compact):
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
    lines = lines[:height]
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
    try:
        while True:
            size = shutil.get_terminal_size((60, 14))
            frame = render(size.columns, size.lines)
            if frame != last:
                sys.stdout.write(frame)
                sys.stdout.flush()
                last = frame
            time.sleep(1)
    except KeyboardInterrupt:
        bye()


if __name__ == "__main__":
    main()
