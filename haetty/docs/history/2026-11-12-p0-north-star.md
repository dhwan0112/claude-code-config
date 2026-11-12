# 2026-11-12: P0, the direction (NORTH-STAR) and the issue fixes of 2026-11-11

The person reset haetty's direction on 2026-11-12: four points and the conditions ([NORTH-STAR.md](../NORTH-STAR.md))
and the stages P0–P9 ([STAGES.md](../STAGES.md)). This record holds stage P0 and the HANDOFF text it replaced.

## Issue fixes of 2026-11-11 and their tests

- #1, `70f97a4` (repaint after X11 map and expose). New regression test `scripts/e2e/remap.sh` (Xvfb, in
  `run-all.sh`): the window is unmapped and mapped directly (Xvfb has no window manager; that is what minimize and
  restore do to it), its pixels are read from the X server with `import -window` and compared with the pixels before.
  On the fixed build: 3 passed. With `app.rs` of `70f97a4^`: both map rounds FAIL, 61570 px differ (then restored).
- #2, `cd2e0e7` (title-row buttons from xfwm4's `button_layout`). The parsing test was already in the commit:
  `titlebar::tests::button_layouts_parse_like_xfwm` (`O|HMC`, `CMH|O`, shade ignored, duplicates, `|`, no title mark,
  `is_xfce` on `XFCE`, `ubuntu:xfce`, `GNOME`, empty). No new test needed.
- DESIGN §2 (`docs/design/01-principles-and-outer.md`, integrated decorations): the button sides follow the desktop's
  window-button layout setting (GNOME `button-layout`, Xfce xfwm4's `button_layout`, otherwise right) — the person's
  wording, approved 2026-11-12, its own commit.

## What `decorations = auto` picks on Xfce (X11)

- Code (`config.rs`, `Decorations::effective`): `Auto` on any X11 → `Native`; only Wayland → `Integrated`. So on Xfce
  xfwm4 draws the title bar, and the Xfce button fix (#2) matters only with `decorations = integrated` set by hand
  (as in the issue).
- DESIGN §3.12's reason for `native` on X11: "the system can give its title bar the theme's colour … (the window
  manager draws it; mutter takes `_GTK_THEME_VARIANT=dark`)". That holds for mutter. xfwm4 4.18.0 has no reference to
  `_GTK_THEME_VARIANT` (`strings $(which xfwm4)` on an Xubuntu 24.04 / Xfce 4.18 machine finds `_GTK_FRAME_EXTENTS`
  and `_MOTIF_WM_HINTS`, not it): its title bar keeps xfwm4's theme colours, whatever the terminal's theme.
- So on Xfce the reason written in DESIGN does not hold, and by its own criterion `auto` would pick `integrated` there.
  Not changed: it changes the default look on Xfce, a choice for the person (HANDOFF §6.0). DESIGN §3.12 now says what
  `auto` does on Xfce.

## HANDOFF text replaced by the P0–P9 list (verbatim)

## Next session

Roles from 2026-11-10: **kramers (Linux) is the development machine; the Windows PC only checks** (it pulls, runs,
and pushes results as documents on `checks/windows-<date>`; `docs/dev-commands.md`, "Windows-only checks"). One
stage per session (`docs/OPERATING.md`); the person says "next stage" after `/clear`. Tools and paths:
`docs/dev-commands.md`, "Environment". Records: the takeover on kramers
[history/2026-11-10-kramers-dev-machine.md](history/2026-11-10-kramers-dev-machine.md); what exists only on the PC
and the earlier progress lines (F6, G0–G7, K0–K3b, H) [history/2026-11-10-windows-handover.md](history/2026-11-10-windows-handover.md).

Stages on kramers, in this order (1 done 2026-11-10):
1. Done: the development environment on kramers (`tools/machine.sh`; gate, check-range, stage scripts, cross-build).
2. **Next: K1 follow-up** (protected area; approved by the person for exactly this): a Stop or StopFailure that
   started after the question releases the input hold without the person's input; a mouse button press (not
   motion, not the wheel) counts as the person's input and clears the mark. Add both cases to the existing
   integration tests (`crates/haetty-gui/tests/permission.rs`; K1's record: `history/2026-11-09-k1-attention.md`).
   The deny path of auto mode is not examined: keep it as an open item in §6.0.
3. Windows results by themselves: `tools/overnight-soak.ps1` pushes a summary of its result to `checks/windows-<date>` when it ends (code changed here, the PC only pulls); the steps for judging that branch and tagging from kramers go
   into this file. Added by the person (2026-11-10): the Windows night run first checks that the haetty window really came to the foreground; if not, the result is `NOT-RUN (foreground)`, not FAIL, and keeps the foreground window's
   program name, the time without input and the way of starting (Task Scheduler or by hand). And a document with a procedure for each of the three untested hypotheses in the record on `checks/windows-2026-11-10` (0.3.0's window
   activation, the Task Scheduler session, another window taking the foreground after the Linux part), to decide whether the unattended scheduler start is why the window does not come forward.
4. The v0.3.2 candidate (2 and 3 in it) and its checks; the Linux night run on kramers (`overnight-soak-linux.sh
   night`, first run), the Windows night run on the PC. Tag conditions as before (both PASS, check-range 0
   failing; `NOT-RUN (foreground)` never stands in for WINDOWS PASS); the tag is approved in advance when they hold.
5. After the person installs the new stable: self-hosting inside haetty; a coordinating session hands stages to
   orchestrator workers and reads only their STATUS reports — tried with two small stages and compared with the
   one-session way by `haetty tokens` (the parts H3 and H4 left open).
6. Then: splitting the big files (H2), the differences left to the newest Ghostty (F2), the default-engine table.

Candidates and night runs:
- F5 (`b1262d2`, v0.3.0 candidate): the night run of 2026-11-10 gave LINUX PASS and WINDOWS FAIL in both engines
  (focus check; cause not established). **No `haetty-v0.3.0` tag.** Record: branch `checks/windows-2026-11-10`,
  file `haetty/docs/checks/2026-11-10-windows.md`. The cause is open (§6.0).
- K4 (`1163a0a`, the v0.3.1 candidate): its night run is registered on the PC for 2026-11-11 02:00
  (`haetty-overnight-f6`, native engine only). **On the Windows PC**, on the user's "야간 결과 올려":
  1. read `C:\Users\Public\haetty-overnight\result-20261111-0200.txt`;
  2. write `haetty/docs/checks/2026-11-11-windows.md` on branch `checks/windows-2026-11-11` (docs only), and push that branch only;
  3. tag `haetty-v0.3.1` on `1163a0a` (by hash, not main's HEAD) only if LINUX and WINDOWS PASS and
     `tools/check-range.sh haetty-v0.2.0..1163a0a` reports 0 failing (heavy: start it early, as recorded for v0.2.0).
     The user approved this tag on 2026-11-10; push only the tag.
