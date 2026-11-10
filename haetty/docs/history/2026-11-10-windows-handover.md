# Handover from the Windows PC, 2026-11-10

Written when development moved to kramers (from 2026-11-10). This PC is the Windows verification machine: its night
results reach kramers as `checks/windows-<date>` branches (docs only). This file lists everything that exists only on
this PC, so that nothing is lost when the PC is cleared. HANDOFF "Next session" has the short version.

## 0. Status at the handover

- `haetty-v0.3.0` (`b1262d2`): not tagged. Night run 2026-11-10: LINUX PASS, WINDOWS FAIL in both engines (focus check;
  cause not established). Record: branch `checks/windows-2026-11-10`, file `haetty/docs/checks/2026-11-10-windows.md`
  (commit `8321081`).
- `1163a0a` (K4, the v0.3.1 candidate): night run 2026-11-11 02:00 (`haetty-overnight-f6`, native engine only). The tag
  `haetty-v0.3.1` is pre-approved if both platforms pass (HANDOFF "Next session").
- No stage is in progress (`~/m4/current-wt` is absent).

## 1. Scheduled tasks (Windows Task Scheduler, root folder)

| Task | Commit | Trigger | State | Action |
|---|---|---|---|---|
| `haetty-overnight-f5` | `b1262d2` (v0.3.0 candidate) | 2026-11-10 02:00 | ran: native 02:00:04 to 03:04:24, ghostty 03:04:25 to 04:07:02; last result 0; one-time, spent | runs `f5-night.ps1`; can be deleted (not deleted) |
| `haetty-overnight-f6` | `1163a0a` (K4, v0.3.1 candidate) | 2026-11-11 02:00 | ready; keep (as asked) | runs `f6-night.ps1`: `overnight-soak.ps1 -Commit 1163a0a -Engine native -GuiE2E` |

- Both run PowerShell 7.6.9 (WindowsApps) with `-NoProfile -ExecutionPolicy Bypass -File`, LogonType Interactive, run level
  Limited. `haetty-overnight-f6` has StartWhenAvailable off, so a run missed while the PC is off is skipped.
- They need: the WSL distro `Ubuntu` (stopped now; the run starts it); `tools\overnight-soak.ps1` in the main checkout
  (the UNC path is in HANDOFF §6.1 item 30; do not move or rename it); the wrappers `C:\Users\Public\haetty-overnight\f5-night.ps1`
  and `f6-night.ps1` (only on this PC, not in the repo).
- The soak waits up to 240 minutes for a desktop with no input for 120 s (`-IdleWaitMinutes`), and prevents sleep and
  screen lock while it runs.

## 2. Results and logs (`C:\Users\Public\haetty-overnight\`)

- `latest.txt` (the same file as `result-20261110-0304.txt`) and `result-20261110-0200.txt`: the 2026-11-10 runs.
- `result-20261109-0200.txt` (commit `bb61e92`, native): LINUX, WINDOWS, GUI-TABS and GUI-ORCHESTRATOR all PASS.
- `result-20261108-0219.txt` (commit `63822de`, v0.2.0): LINUX PASS, WINDOWS PASS, GUI-TABS FAIL (1 of 14), GUI-ORCHESTRATOR PASS.
- `progress.log` (every run since 2026-11-08); `linux.csv` and `windows.csv` (the last run only; earlier CSVs were overwritten).
- `bin\haetty.exe` (the Windows build of `b1262d2` used by the 2026-11-10 runs); `scripts\` (copies of the e2e scripts used by the last run).
- `f5-night.ps1` and `f6-night.ps1` (see §1); `f5-night-transcript.txt` (the PowerShell transcript; it contains the Windows
  account and the PC name, so remove them before copying it anywhere).
- Earlier trials, 2026-11-07: `haetty-overnight-trial` (Linux PASS; Windows NOT-RUN, desktop in use);
  `haetty-overnight-trial4` (Linux PASS; Windows PASS with idle 2337 s at its start, so no input during the run; GUI NOT-RUN,
  desktop in use); `haetty-overnight-trial2` and `-trial3` (2026-11-07, 09:00 to 09:45; their files are kept, no verdict recorded here).

## 3. Test debris and Windows state (not needed; not deleted)

- `C:\Users\Public\haetty-test\` (about 77 entries, mostly run folders), `haetty-test-f4\`, `haetty-test-f5\`,
  `haetty-smoke\` (binaries v3 to v7), `haetty-e5b\`, `haetty-e5d\`.
- `%TEMP%\haetty-f4-test\` and `%TEMP%\haetty-f5-inst\` (installs and tests of 2026-11-09).
- `%LOCALAPPDATA%\haetty\` (conpty, daemon.log, events.log, run, tabs; from the 2026-11-05 to 11-07 test runs). There is no
  Windows stable install here (`%LOCALAPPDATA%\Programs\haetty` is absent), no haetty entry in the Claude Code settings,
  and `haetty` is not on PATH.
- `%USERPROFILE%\ccterm-win\m3\` (2026-11-05; coexistence test data for ccterm and haetty).

## 4. Worktrees, branches and stashes

- `git worktree list` (WSL, `~/projects/claude-code-config`): only the main checkout before this session. The folder
  `~/projects/claude-code-config-wt/` is empty (the parent of the stage worktrees).
- Local branches: `main` only. No stash.
- Remote branches before this session: `origin/main`, `origin/checks/kramers-2026-11-06`, `origin/checks/kramers-2026-11-09`.
  This session adds `origin/checks/windows-2026-11-10` (docs only).
- Git-ignored local files (not pushed, not mirrored to Dropbox): `haetty/assets/icon/tools/__pycache__/`,
  `haetty/bench/engine-compare/Cargo.lock`, `haetty/bench/engine-compare/stress.bin`.

## 5. What is not pushed

- Before this session: `origin/main..main` was empty, and the local tags equal the remote tags (`20261103-Kramers-1`,
  `20261103-Pauli` to `-7`, `20261104-Kramers-1` to `-8`, `haetty-v0.1.0`, `haetty-v0.1.1`, `haetty-v0.2.0`).
- Only on this PC (never in git): the results and logs in §2, the debris in §3, the corpus and records in §6, and
  `~/archive/`, `~/m4/`, `~/m2/` and `~/m3/`.
- Pushed by this session: the checks branch (`8321081`) and the HANDOFF commit on `main`.

## 6. Archived copies, corpus and repo-external data (WSL unless marked)

- Archived old copy: `~/archive/ccterm-2026-11-09/` (7.3 GB, most of it `target/`; the old ccterm tree archived by the H
  stage, commit `1de5a31`). Keep. `~/m4/old-ccterm/ccterm` (91 MB) is another copy.
- Recorded Claude Code output, the corpus of `haetty-vtdiff --corpus ~/m4/vtcorpus` (`docs/vtdiff/`, DESIGN 12):
  `~/m4/vtcorpus/claude-default.raw` (3892 bytes) and `claude-fullscreen.raw` (4957 bytes). It cannot be re-recorded without
  a logged-in Claude Code. Personal-data scan (2026-11-10): no e-mail address, home path, name, token, UUID or URL. The text
  is Claude Code's screen and two typed lines ("hello world", "second line with and emoji"). Not in the repo.
- Historical measurements (cannot be re-recorded identically): `~/m4/f0-native-1h.csv` and `~/m4/f0-ghostty-1h.csv`
  (DESIGN 12); `~/m4/soak/` (94 MB: `soak-linux.csv`, `soak-win.csv`), `~/m4/soak2/` (247 MB), `~/m4/e5c-data/` (343 MB),
  and the measurement notes `~/m4/*.txt|*.md|*.tsv`. Scan: no hits in `soak/`, `soak2/`, `e5c-data/`, `e5e/` or the CSVs.
  Many top-level `.txt` files contain the user name in `/home/...` paths, so strip those before any move.
- The 2026-11-08 to 11-10 Windows results (§2) are historical too. The scan found the user name only in the transcript and
  in the two wrapper scripts; the result, progress and CSV files contain none.
- Regenerable: `~/m4/cargo-target-*` (16 build folders), `~/m4/target-ghostty-ref-f5/`, `~/m4/e5e/` (1443 files), the
  vtdiff outcome and report files under `~/m4/` (regenerate with the commands in `docs/vtdiff/*.md`), `~/m4/ghostty-ref/ghostty`
  (re-clone at the pins in §7), and `~/m4/zig-dl/zig17.tar.xz` (Zig 0.17.0; its sha256 `1cbe9df9…2026` matches the pin in
  `tools/ghostty-ref/README.md`, checked here).
- `~/m4/harness/` (521 small scripts, 2.6 MB; cited by `docs/history/2026-11-06-07-stages-t-u-v.md`): ad-hoc test scripts that
  the current tools do not use; they contain the user name in paths.
- `~/m2/` (299 MB): `xroot/` (the extracted root for Xvfb without sudo; `scripts/e2e/README.md` expects `HAETTY_XROOT=~/xroot`,
  this copy is at `~/m2/xroot`), `ucd/` (Unicode data; `tools/gen-grapheme-table.py` takes the UCD directory, and the Zig
  package cache has one at `~/.cache/zig/p/uucode-0.2.0-*/ucd`), `fonts/`, `debs/`, `pre-ime/`, `before/`, `xvfb.log`.
- `~/m3/before/` (83 MB): an old leftover.
- Toolchains: Rust (`~/.rustup`, `~/.cargo`); Zig 0.15.2 (`~/.local/bin/zig`, a link to `~/.local/opt/zig-x86_64-linux-0.15.2/zig`);
  Zig 0.17.0 (`~/.local/zig-0.17.0/zig`). Windows: PowerShell 7.6.9 (WindowsApps).

## 7. Zig and Ghostty: what kramers needs

- haetty's own build (`docs/dev-commands.md`, Environment): Rust 1.90 or newer, Zig 0.15.2 on PATH, and for libghostty-vt the
  Ghostty commit `a887df42c56f6de86c0fe6da9c4eeca37931e083` (`GHOSTTY_SOURCE_DIR` points at a checkout and skips the clone).
  The clone `~/m4/ghostty-ref/ghostty` on this PC contains that commit.
- The vtdiff reference `ghostty-main` (`tools/ghostty-ref/README.md`): Zig 0.17.0 and Ghostty `e035beb9437118b803830a5864ea2f4dd7094354`
  (main, 2026-11-08), as a clean checkout outside the repo (default `~/m4/ghostty-ref/ghostty`, or `$GHOSTTY_SRC`). Build:
  `tools/heavy.sh --label ghostty-ref -- bash tools/ghostty-ref/build.sh`. That script refuses any Zig other than 0.17.0.
- On kramers: install Zig 0.15.2 on PATH and Zig 0.17.0 under `~/.local/zig-0.17.0/` from ziglang.org (check the sha256 against
  the README pin), then clone Ghostty outside the repo and check out both commits. Zig 0.17 is for the `ghostty-main` helper
  only; the build itself still uses 0.15.2.

## 8. Progress records moved from HANDOFF "Next session" (2026-11-10)

Moved verbatim from `docs/HANDOFF.md` at commit `1163a0a` (the lines from the F6 bullet up to the "Stable" note).

- F6 done: drafts 01 and 02 fixed upstream (`bed20eb`, `82df0be`); 03 still present (re-run: 5 of 6 cases stale);
  none sent.
- G0 done (2026-11-09): DESIGN by topic in `docs/design/` (index `docs/DESIGN.md`, 65 lines); CLAUDE.md keeps its rules,
  commands and environment moved to `docs/dev-commands.md`; HANDOFF under 400 lines. Token estimates:
  `docs/history/2026-11-09-g0-handoff-trim.md`.
- G1 to G7 done (2026-11-09): `haetty tokens` and the token status line (G1, G1b; off by default), `haetty run` and
  `haetty log` (G2), `haetty wait` (G3: exit 0 when met, 124 on timeout, 3 when the tab is not observable; the event
  waits follow the daemon's stream), the guide `docs/tokens/claude-guide.md` and the CLAUDE.md section (G4; haetty
  adds the guide to no session), the orchestrator (G5: a task's model and effort are launch arguments, a briefing
  file per task, the report keeps its STATUS block only), the dim `/clear` mark on a worker tab past its context
  share (G5c: `clear-hint`, `context-window`; display only, nothing is sent), and the saving mode (G6: `saving-mode`,
  off by default; BASH_MAX_OUTPUT_LENGTH and MAX_MCP_OUTPUT_TOKENS of 12000 for Claude Code sessions, a dim `◆ saving`
  mark; sources in `docs/tokens/README.md`), and the G7 measurement (`docs/tokens/g7-measurement.md`, one run each).
  K0–K3b done (2026-11-09): kramers results in §6.1 and §6.0 (`docs/checks/`); K1 fixes kramers 12
  (`docs/history/2026-11-09-k1-attention.md`); K2b, K3a and K3b fix the other K-items; H done (`docs/OPERATING.md`,
  `tools/stage/`, `docs/tokens/partial-reads.md`). Next: the v0.3.0 judgement and tag after the night run of
  2026-11-10 (commit `b1262d2`, `haetty-overnight-f5`). K4 is the v0.3.1 candidate: the newest commit on main, its night
  run on 2026-11-11 02:00 after its gate.

## 9. HANDOFF lines superseded by the night run (2026-11-10)

Their text as it stood at commit 1163a0a. The status they gave is now in HANDOFF §1, "Next session" and §6.0.

```
- **Version** 0.3.0 (workspace; the candidate commit of stage F5, tag `haetty-v0.3.0` only after its night run). Tags: `haetty-v0.1.0` = `4d75fcd`,
  commit; the tag waits for its night run.
- F0 to F4 done (DESIGN 10.11 to 10.15). F5: candidate `b1262d2` on main; its night run is registered for 2026-11-10 02:00
  and had not run at 2026-11-09 21:31. The tag `haetty-v0.3.0` waits for both platforms to PASS, then the F5 record (DESIGN 10.16).
- F5: the candidate `b1262d2` (v0.3.0) has its night run registered for 2026-11-10 02:00 (both engines, `-GuiE2E`,
  as §6.1 item 30 with the commit). The tag `haetty-v0.3.0` waits for both PASS.
```
