#!/usr/bin/env python3
"""The verdict of a Linux soak series (scripts/e2e/soak.sh). The same rule is in
scripts/e2e/windows/soak-verdict.ps1; scripts/e2e/soak-cases holds synthetic series both must judge alike.

usage: soak-verdict.py SERIES.csv DUR ALIVE(yes|no)

Rule (F0, 2026-11-08):
- threads, fds, descendants, sessions: the last sample equals the first; task worktrees 0 at the end.
- Full, per session: a session is full from the first sample from which its scrollback rows no longer grow by
  more than max(100 rows, 1 %) over the sample it is at (samples where it is alive). A session that never grew
  by more than 100 rows does not count. Baseline = the latest full time over the sessions that grew: from then
  on every session's scrollback has stopped growing. A session still growing at the last sample: no baseline.
- From the baseline on, the window must be at least half the run, at most 1800 s (1800 s for the hour, 300 s for
  the short soak of 600 s, P1c), and the GUI's and the daemon's anonymous memory at
  the last sample at most 4 MB above their values at the baseline sample.
- Operations: the counts of the run are done (200/50/20/10 at 3600 s, scaled as soak.sh scales them); the
  window and the daemon are alive at the end.
A series without the sessions_rows column predates this rule: not judged, FAIL.
"""
import csv
import sys

TOL_ROWS = 100
TOL_FRAC = 0.01
WINDOW_MAX_S = 1800
BAR_KB = 4096
OPS = [("tabs_done", 200), ("detach_done", 50), ("tasks_done", 20), ("themes_done", 10)]


def ops_expect(dur):
    f = min(1.0, dur / 3600)
    return {k: max(1, round(c * f)) for k, c in OPS}


def load(path):
    with open(path, newline="") as f:
        rd = csv.DictReader(f)
        cols = rd.fieldnames or []
        rows = []
        for r in rd:
            o = {}
            for k, v in r.items():
                if k == "sessions_rows":
                    o[k] = {}
                    for part in filter(None, (v or "").split(";")):
                        sid, _, n = part.rpartition(":")
                        o[k][sid] = int(n)
                else:
                    o[k] = int(v)
            rows.append(o)
    return cols, rows


def baseline(rows):
    """(t_b, note): t_b in seconds, or None when a session is still growing at the last sample."""
    ids = []
    for r in rows:
        for sid in r["sessions_rows"]:
            if sid not in ids:
                ids.append(sid)
    t_b, notes = 0, []
    for sid in ids:
        idx = [i for i, r in enumerate(rows) if sid in r["sessions_rows"]]
        vals = [rows[i]["sessions_rows"][sid] for i in idx]
        if max(vals) - vals[0] <= TOL_ROWS:
            continue
        fill = None
        for k, i in enumerate(idx):
            tol = max(TOL_ROWS, TOL_FRAC * vals[k])
            if all(v <= vals[k] + tol for v in vals[k:]):
                fill = k
                break
        if fill == len(idx) - 1 and len(idx) > 1:
            return None, f"{sid} still growing at the last sample ({vals[-1]} rows)"
        t_full = rows[idx[fill]]["t_s"]
        notes.append(f"{sid} full at {t_full} s ({vals[-1]} rows)")
        t_b = max(t_b, t_full)
    return t_b, "; ".join(notes) if notes else "no session grew by more than 100 rows"


def verdict(cols, rows, dur, alive):
    out, ok = [], True

    def check(name, cond, detail):
        nonlocal ok
        ok = ok and cond
        out.append(("PASS " if cond else "FAIL ") + name + ": " + detail)

    if not rows:
        check("series", False, "no samples")
    elif "sessions_rows" not in cols:
        check("scrollback", False, "no sessions_rows column: this series predates the rule, not judged")
    else:
        first, last = rows[0], rows[-1]
        for k in ["gui_threads", "gui_fds", "daemon_threads", "daemon_fds", "daemon_descendants", "sessions"]:
            check(k, last[k] == first[k], f"{first[k]} -> {last[k]}")
        check("worktrees", last["worktrees"] == 0, str(last["worktrees"]))
        for k, want in ops_expect(dur).items():
            check(k, last[k] == want, f"{last[k]} of {want}")
        check("alive", alive == "yes", f"alive at the end: {alive}")
        tb, note = baseline(rows)
        if tb is None:
            check("scrollback_full", False, note)
        else:
            window, need = last["t_s"] - tb, min(WINDOW_MAX_S, dur // 2)
            out.append("INFO baseline: " + note + f"; baseline {tb} s")
            check("window", window >= need, f"{window} s after the baseline (need {need})")
            base = next(r for r in rows if r["t_s"] == tb)
            check("gui_anon", last["gui_anon_kb"] <= base["gui_anon_kb"] + BAR_KB,
                  f"{base['gui_anon_kb']} kB at {tb} s -> {last['gui_anon_kb']} kB")
            check("daemon_anon", last["daemon_anon_kb"] <= base["daemon_anon_kb"] + BAR_KB,
                  f"{base['daemon_anon_kb']} kB at {tb} s -> {last['daemon_anon_kb']} kB")
    out.append("VERDICT " + ("PASS" if ok else "FAIL"))
    return out, ok


def main(argv):
    cols, rows = load(argv[1])
    out, ok = verdict(cols, rows, int(argv[2]), argv[3])
    print("\n".join(out))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
