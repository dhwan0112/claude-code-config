#!/usr/bin/env python3
"""Writes the synthetic soak series that the verdicts (soak-verdict.py, windows/soak-verdict.ps1) must judge, and
expect.txt with the verdict each one gets. Run from this folder: python3 make.py. The committed files are its output.

The hour: samples every 300 s over 3600 s (13 samples); the short soak (`short-*`, soak-short.sh): every 60 s over
600 s (11 samples), its scrollback filling within 3 min. expect.txt: name, verdict, the run's length in s. Sessions: b = the burst tab (fills at 2 439 rows, as the 800 kB limit
does for its coloured lines), s = the slow tab (fills at 4 761 rows, as it does for its ticks), c = a tab that fills
and closes, w1..w12 = one-line workers, idle sessions never grow.
"""
import os

HERE = os.path.dirname(os.path.abspath(__file__))
T = list(range(0, 3601, 300))
LIN = "t_s,gui_threads,gui_fds,gui_anon_kb,daemon_threads,daemon_fds,daemon_anon_kb,daemon_descendants,sessions,worktrees,tabs_done,detach_done,tasks_done,themes_done,scrollback_rows,sessions_rows"
WIN = "t_s,gui_threads,gui_handles,gui_private_ws_kb,daemon_threads,daemon_handles,daemon_private_ws_kb,daemon_descendants,sessions,worktrees,tabs_done,detach_done,tasks_done,themes_done,scrollback_rows,sessions_rows"
LEGACY = "t_s,gui_threads,gui_fds,gui_anon_kb,daemon_threads,daemon_fds,daemon_anon_kb,daemon_descendants,sessions,worktrees,tabs_done,detach_done,tasks_done,themes_done,scrollback_rows"


def short_sessions(t):
    """The short soak's tabs: b full at 60 s, s at 180 s (a limit that fills within 3 min, soak-short.sh)."""
    return {"b": 760 if t >= 60 else 0, "s": min(760, round(760 * t / 180))}


def sessions(t, extra=None):
    d = {"b": 2439 if t >= 300 else 0, "s": min(4761, round(4761 * t / 1200))}
    d.update(extra or {})
    return d


SHORT_OPS = (33, 8, 3, 2)


def case(name, expect, s_fn=sessions, daemon=lambda t: 20000 + 5 * min(t, 1200), ops=(200, 50, 20, 10),
         legacy=False, extra_t=None, dur=3600):
    rows = []
    for t in (T if dur == 3600 else list(range(0, dur + 1, 60))):
        ses = s_fn(t)
        if extra_t:
            ses.update(extra_t(t))
        rows.append(dict(
            t_s=t, gui_threads=7, gui_fds=17, gui_anon_kb=8000, daemon_threads=24, daemon_fds=21,
            daemon_anon_kb=daemon(t), daemon_descendants=6, sessions=4 + sum(1 for k in ses if k.startswith("w")),
            worktrees=0,
            tabs_done=ops[0] if t == dur else 0, detach_done=ops[1] if t == dur else 0,
            tasks_done=ops[2] if t == dur else 0, themes_done=ops[3] if t == dur else 0,
            scrollback_rows=max(ses.values()) if ses else 0,
            sessions_rows=";".join(f"{k}:{v}" for k, v in ses.items())))
    return name, expect, rows, legacy, dur


def win_row(r):
    o = dict(r)
    o["gui_handles"] = 287
    o["daemon_handles"] = 191
    o["gui_private_ws_kb"] = o.pop("gui_anon_kb")
    o["daemon_private_ws_kb"] = o.pop("daemon_anon_kb")
    o.pop("gui_fds")
    o.pop("daemon_fds")
    return o


def workers(t):
    return {f"w{i}": (1 if t >= 300 * i else 0) for i in range(1, 13)}


CASES = [
    case("full-early", "PASS"),
    case("full-late", "FAIL", s_fn=lambda t: {"b": sessions(t)["b"], "s": min(4761, round(4761 * t / 2100))}),
    case("never-full", "FAIL", s_fn=lambda t: {"b": sessions(t)["b"], "s": round(t * 1.4)}),
    case("grow-after-full", "FAIL", daemon=lambda t: 20000 + 5 * min(t, 1200) + (6000 if t >= 1500 else 0)),
    case("legacy", "FAIL", legacy=True),
    case("closed-session", "PASS", extra_t=lambda t: {"c": {0: 0, 300: 2400, 600: 4761, 900: 4761, 1200: 4761}.get(t)}
         if t <= 1200 else {}),
    case("workers", "PASS", extra_t=workers),
    case("sawtooth", "PASS", s_fn=lambda t: {"b": sessions(t)["b"],
                                             "s": {0: 0, 300: 300, 600: 507, 900: 300}.get(t, 507)}),
    case("ops-short", "FAIL", ops=(199, 50, 20, 10)),
    case("no-growth", "PASS", s_fn=lambda t: {"b": 0, "s": 0}, daemon=lambda t: 20000),
    # The short soak: 600 s, the judged window at least 300 s (half the run), the counts scaled (33/8/3/2).
    case("short-full-early", "PASS", s_fn=short_sessions, daemon=lambda t: 20000 + 10 * min(t, 180), ops=SHORT_OPS,
         dur=600),
    case("short-full-late", "FAIL", s_fn=lambda t: {"b": 760 if t >= 60 else 0, "s": min(760, round(760 * t / 360))},
         ops=SHORT_OPS, dur=600),
    case("short-grow-after-full", "FAIL", s_fn=short_sessions,
         daemon=lambda t: 20000 + 10 * min(t, 180) + (6000 if t >= 420 else 0), ops=SHORT_OPS, dur=600),
    case("short-ops-short", "FAIL", s_fn=short_sessions, ops=(33, 8, 2, 2), dur=600),
]


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", newline="\n") as f:
        f.write(text)


def main():
    expect = []
    for name, want, rows, legacy, dur in CASES:
        if legacy:
            lines = [LEGACY] + [",".join(str(r[k]) for k in LEGACY.split(",")) for r in rows]
            write(os.path.join(HERE, "linux", name + ".csv"), "\n".join(lines) + "\n")
            wcols = WIN.replace(",sessions_rows", "").split(",")
            wlines = [",".join(wcols)] + [",".join(str(win_row(r).get(k, "")) for k in wcols) for r in rows]
            write(os.path.join(HERE, "windows", name + ".csv"), "\n".join(wlines) + "\n")
        else:
            cols = LIN.split(",")
            lines = [LIN] + [",".join(str(r.get(k, "")) for k in cols) for r in rows]
            write(os.path.join(HERE, "linux", name + ".csv"), "\n".join(lines) + "\n")
            wrows = [win_row(r) for r in rows]
            wcols = WIN.split(",")
            wlines = [WIN] + [",".join(str(r.get(k, "")) for k in wcols) for r in wrows]
            write(os.path.join(HERE, "windows", name + ".csv"), "\n".join(wlines) + "\n")
        expect.append(f"{name} {want} {dur}")
    write(os.path.join(HERE, "expect.txt"), "\n".join(expect) + "\n")
    print(f"{len(CASES)} cases written")


if __name__ == "__main__":
    main()
