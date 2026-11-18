# Changelog

## Unreleased

## 0.4.0 (2026-11-19, release candidate, not tagged)

Stage PH's harness screen: `claude` typed in a shell tab becomes a Claude Code tab (the proxy), the sidebar with the
agents of a conversation, All agents and `haetty agents` (`docs/history/2026-11-17-ph-harness.md`). The tag
`haetty-v0.4.0` goes on this commit under the P1c rule once the Linux candidate check passes
(`docs/history/2026-11-19-pm.md`); the person tags, builds the release and installs it.

**Windows:** unverified for this commit; its build and clippy pass for `x86_64-pc-windows-gnu` (the gate). Known
problems: the claude proxy in shell tabs does not work with release builds (`haetty.exe` is a GUI-subsystem program,
so the proxy has no console: no `--settings` is added and the real claude opens in a console of its own; Windows
record `checks/windows-2026-11-17`, D3); the settings panel cannot install an update (the version folder's rename is
refused while the panel reads the folder, and the junction `current` is listed as a version; the fixes are on
`windows/2026-11-17-fixes`, not merged). Both wait for stage PW.

### Added

- `claude` typed in a shell tab becomes a Claude Code tab: a proxy folder first on every tab's `PATH` runs your real
  `claude` with your arguments and the same observing `--settings` a Claude Code tab gets (not for `-p`, `--bg`,
  subcommands such as `mcp`/`update`, output that is not a terminal); the tab is a shell tab again when it ends; a
  restored tab set resumes its conversation in the shell (`restore-claude`, default `resume`). Fixes the hand-started
  claude restored as a bare shell. Stage PH, `docs/history/2026-11-17-ph-harness.md`. **Windows:** unverified.
- The sidebar (replaces the status panel; DESIGN §2 change, approved): on the right from 121 columns (`sidebar`,
  `sidebar-width`, `sidebar-min-cols`, Sidebar group), over the terminal in a narrower window (Ctrl+Shift+S); Lead,
  Agents (every subagent with its parent, model, current tool, background/worktree/fork, question, STATUS), Files,
  Workers, Log; an agent's record view. Same for every tab: no resize on events.
- All agents (palette, Ctrl+Shift+A) and `haetty agents [--json] [SESSION]`.
- Protocol 15: the agent table, changed files, effort, `GetClaude` / `GetSessionLog` (read-only).

### Changed

- The status panel's `ui` keys (`status-panel`, `status-panel-width`) are no longer read; the sidebar shows by default.
- Since PH the terminal itself (engine, soaks, look polish, release tools) gets defect fixes only.

## 0.3.3 (2026-11-17, tagged 2026-11-17)

The settings button and panel (stage PS) and signed releases with the in-app update (stage PU), with the person's
release key (`assets/update/haetty.pub`, `29c490a`). 0.3.2 has neither, so 0.3.3 is installed by hand
(`tools/install-stable.sh haetty-v0.3.3`); the in-app update works from 0.3.3 on, once a release is published
(SELF-HOSTING "Releases"). The tag `haetty-v0.3.3` goes on the release commit under the P1c rule (the Linux candidate
check with the short soak, `tools/check-range.sh haetty-v0.3.2..` with 0 failing; `29c490a` is KNOWN, approved
2026-11-17). Tagged on `ca70269` after that check passed (`docs/history/2026-11-17-v0.3.3.md`).

**Windows:** unverified for this commit. Its build and clippy pass for `x86_64-pc-windows-gnu` (the gate). Not run on
Windows: the settings panel, the updater and the installer layout (the Windows check list in `docs/dev-commands.md`);
the Windows reference frames still need making on the PC.
Known problem found later (Windows record `checks/windows-2026-11-17`, B3): on Windows 0.3.3 cannot install an
update from the settings panel ("access denied" on the version folder's rename; `prune` after a rollback can remove the
junction `current`). Update a Windows install with `tools/install-stable.ps1` until stage PW merges the fixes.

### Added

- Settings: one schema for every key (`config::SETTINGS`: type, range, default, group, when it applies); the parser,
  `haetty --help` and README's table come from it. New keys `font-family` (the config file's `--font`), `font-size`
  and `settings-button`. Existing config files read as before (a test pins every older key, the `window-padding`
  family and the refused values). Stage PS, `docs/history/2026-11-16-ps-settings.md`.
- The settings button and the settings panel (DESIGN §2.4, approved; README "Settings"): a dim ⚙ in a Claude Code
  tab's status row (other tabs: over the bottom-right cell while the pointer is near, gone on a key; never under
  mouse reports), Ctrl+, (`settings`) and the palette's "Settings". Groups (Appearance, Font, Window, Terminal, Claude
  Code, Advanced), ↑/↓, Tab, Space, ←/→, Enter, `/` search, `r` default, Esc; mouse clicks and the wheel; a theme
  list that previews and Esc that restores. Saving changes only the key's line (comments, order, unknown keys,
  LF/CRLF kept; temporary file and rename; links kept; read-only refused; one `config.bak` per run; a file changed
  elsewhere is read again). Applied at once: theme and colours, `minimum-contrast`, `font-family`, `font-size`, the
  padding, `settings-button`, `notifications`, `token-status`, `clear-hint`, `context-window` (a font or padding
  change resizes the sessions once, 150 ms after the last step); the rest shows "next start". No new dependency.
  **Windows:** unverified (build and clippy pass; the Windows check list in `docs/dev-commands.md`).

- Releases and the in-app update (stage PU; README "Updates", SELF-HOSTING "Releases", DESIGN §2.4, §3.17):
  `tools/release.sh` builds a tag's assets reproducibly, writes `manifest.json` and leaves signing (minisign, your
  key) and the draft upload to you; haetty checks once a day (one GET; User-Agent `haetty/<version>` only), verifies
  the signature, version, size and SHA-256, installs a new version folder (or replaces a single executable) from the
  settings panel's Updates group, restarts with the tabs back, rolls back by itself when the new version does not
  stay up, and on request. `haetty update --check`. New dependencies: ureq 3 (rustls/ring with the system trust store
  on Linux, Schannel on Windows) and minisign-verify. `install-stable.sh` uses the same layout code
  (`haetty install-layout`). **Windows:** unverified (build, clippy and tests pass; Windows check list).

### Changed

- Ctrl+, is now haetty's (the settings); before, it reached the program. `keybind = ctrl+,=unbind` gives it back.
- "Reload config" (and a config file changed while the window was in the background) now applies every setting that
  applies while running, not only the colours. The Linux reference frames are made with kramers' fonts (the old
  ones were made with fewer fallback fonts and were skipped there).

### Fixed

- End-to-end checks on kramers (stage PS, S7): `orchestrator.sh` answered three of the New task wizard's seven
  questions since G5 (4 of 21 → 21 of 21); `soak.sh` had the same sequence and counted attempts as done, so the
  soaks made no task since G5 — a task now counts only when it was made and removed; regression `ime`: ibus-hangul
  1.5.5 starts in Latin mode, the test's own ibus now gets a private GSettings keyfile with Hangul mode (and no
  longer writes into the real `~/.config/ibus`) (29 of 30 → 30 of 30). The Windows twins got the same wizard keys
  (not run).
- The release-key test of `haetty-update` expected an empty key slot and failed once the person's key was in
  (`29c490a`); it now checks that the slot holds readable minisign keys (`6296de2`).

## 0.3.2 (2026-11-12, tagged 2026-11-16)

The first release after 0.2.0: it also carries the untagged candidates 0.3.0 and 0.3.1 (their sections below). The
tag `haetty-v0.3.2` goes on the release commit `622d629` under the release rule of P1c (the Linux candidate check
with the short soak, `tools/check-range.sh haetty-v0.2.0..` with 0 failing). Tagged 2026-11-16 after the person's decision: 26
old commits that fail with the leak P1a fixed are listed in `tools/known-gate-failures`, and two end-to-end scripts
that fail on kramers with 0.3.1 alike (`orchestrator.sh`, `ime`) do not hold the tag (`docs/history/2026-11-12-p1.md`,
"v0.3.2").

**Windows:** unverified for this commit. Its build and clippy pass for `x86_64-pc-windows-gnu` (the gate). Not run on
Windows: the changes since 0.3.1 below (the K1 follow-up and the health counts are platform-independent code; the
PTY fix of P1a is Linux-only code; #1's redraw on `Focused` / `Occluded(false)` is on the Windows check list).
Known problems from the 0.3.1 candidate (`1163a0a`, `checks/windows-2026-11-11`): the Windows night run passed once
in three runs, twice stopped at the desktop's focus check (cause open); the Windows orchestrator end-to-end run
failed (no task worktree).

### Added

- **The daemon records its own health** (P1c): once an hour an event-log line with its threads, descriptors
  (Windows: handles), anonymous memory (Windows: private bytes), sessions and clients; a `health_warning` line when
  threads or descriptors grew three hours in a row at the same sessions and clients. `haetty status --json` has
  `"health"`: the counts now and the largest since the start (protocol 12). The daemon no longer wakes every 500 ms:
  it waits for events, the hourly line and, when idle, its exit time.

### Changed

- **Release rule** (P1c): a tag needs the Linux candidate check with the short soak (`scripts/e2e/soak-short.sh`,
  10 min, the soak's operations and rule) and check-range with 0 failing. The one-hour soak is a stage's own check
  when it changes the PTY layer, the daemon's session lifetime or the engine's memory structure (`docs/OPERATING.md`).

### Fixed

- **An X11 window is painted whole after it is restored from minimized** (#1, `70f97a4`): a redraw the system asks for
  (Expose) and a map draw a full frame. Regression test: `scripts/e2e/remap.sh` (Xvfb; fails without the fix).
- **The integrated title row on Xfce shows the buttons xfwm4's `button_layout` names** (#2, `cd2e0e7`), not GNOME's
  unused `button-layout`. Unit test: `titlebar::tests::button_layouts_parse_like_xfwm`.
- **A closed session no longer leaks its PTY writer on Linux** when its child never read its input (P1a): the writer
  and its drop waited forever on a full input queue (portable-pty's writer types `"\n"` + EOF on drop). The master
  is non-blocking now and a closed session's reader and writer return; the child gets the terminal's hangup. Fixes the
  flaky `closed_sessions_leave_no_threads`. Tests: `haetty-pty` `unix::tests` (fail without the fix).
- **A turn's end releases the program-input hold without the person's key** (P1b, K1 follow-up; protected area,
  approved): a `Stop` or `StopFailure` that started after the permission question opens it — an unattended worker
  whose dialog was answered outside haetty no longer stays held until the next prompt. **A mouse button press answers
  a dialog** like a key (the mark goes); motion, releases and the wheel do not. A rejection (Esc, no event) still holds
  until the person's next prompt; the auto-mode denial path stays open. Tests: `tests/permission.rs`
  `turn_end_releases_the_hold_and_a_mouse_press_answers` (fails without either half), `haetty-cc` `state::tests`.

## 0.3.1 (candidate, 2026-11-09)

The tag `haetty-v0.3.1` is set on this release commit only after its night run passes on both platforms and both
engines, and the kramers re-check list in `docs/HANDOFF.md` section 6.1 is done.

### Fixed

- **The permission mark (`?`) clears when the person answers.** The program-input hold opens at the end of the turn, or
  at the person's next prompt after a rejection (Esc gives no event). Program input is refused while a permission dialog
  is open or was answered and its turn has not ended. The hook order was recorded with Claude Code 2.1.319
  (`docs/history/2026-11-09-k1-attention.md`).
- **Child markers of a Claude Code session are not passed on.** A session haetty starts does not inherit `CLAUDECODE`
  and the other child markers, also when the daemon was started from inside Claude Code: transcript saving stays on and
  `--resume` works (K2, K2b).
- **A killed daemon is reaped by its window**, not left as a zombie (K3a).
- **A test or dev instance keeps its per-session settings beside its own socket**, not in the shared runtime directory
  (K3a; the injected settings path, for review).
- **A resumed session shows the report of its transcript** after a restart: the worker mark no longer waits for the next
  turn (K3a).
- **A tab keeps its own name** across a detach and an attach, also when another window takes the session (K3b).

### Removed

- The former environment names `CCTERM_*` are no longer read; `HAETTY_*` is the only prefix. The file migration of the
  former name (config, tab sets, a running old daemon) stays for this version.

### Changed

- The stage scripts are in the repository (`tools/stage/`); `docs/OPERATING.md` describes one stage per session; the
  project settings deny reads of the build folders (`.claude/settings.json`).

## 0.3.0 (candidate, 2026-11-09)

The tag `haetty-v0.3.0` is set on this release commit only after its night run passes on both platforms and both
engines (`docs/DESIGN.md` section 10.16).

### Added

- **Native terminal engine.** haetty's own engine (`haetty-vt`) is chosen per tab with `engine = "native"` in the
  config. `ghostty` (libghostty-vt) stays the default.
- **Builds without libghostty.** `cargo build --no-default-features` needs no Zig; every tab then runs on the native
  engine. A `ghostty` tab in such a build runs on the native engine and logs a warning.
- **Observing a tab (read-only).** A tab can be made observable from the palette ("Observing: allow or stop for this
  tab") or for every new tab with `observe-new-tabs = true`. `haetty events` and `haetty screen` read an observable
  tab; they cannot write to it.
- **Underline kinds and colour, and OSC 8 hyperlinks.**
- **Generated help and completions.** The CLI help and the shell completions come from one table; the README's config
  table is generated from the code.

### Changed

- The default engine reads every snapshot from a new render state. A combining mark, a variation selector or a ZWJ that
  joins a character drawn in an earlier read is drawn at once (it stayed blank until the row changed). The copy costs
  about 90 µs per snapshot, against 86 µs before.

### Fixed

- The first window opens at its monitor's scale, and the grid is kept on every scale change.
- Windows: Hangul typed into "Rename tab" reached the shell; it now goes into the rename.
- Native engine: C0 controls 0x10 to 0x1F do nothing in the ground state; C1 bytes and the DECRQSS replies for SGR,
  DECSTBM, DECSCUSR and DECSLRM follow the newest Ghostty.

### Performance

- Native engine: runs of printable ASCII are written directly; SGR parameters are parsed in one pass. Session
  throughput of the native engine against libghostty (five runs each, quiet machine, `e5c-tput.sh`): lines 117 %,
  wrapped 132 %, SGR 80 %.
- Dist builds without libghostty are smaller: `haetty` 15 % (Linux) and 23 % (Windows, mingw) smaller;
  `haetty-headless` 49 % and 38 % smaller.

### Build and install

- Windows build without WSL (MSVC, native engine only), described in `docs/SELF-HOSTING.md`.
- The installer still builds the default build in WSL (libghostty and Zig); it has no native-only option yet.

### Known gaps

- The native engine still differs from the newest Ghostty in 744 of 2066 `haetty-vtdiff` runs: kitty graphics
  replies, XTWINOPS size reports, other replies, and some text cases (`docs/vtdiff/2026-11-09-f2.md`).
- Native key-to-frame latency is 0.13 ms above libghostty at the median (budget 16 ms).
- A `ghostty` tab in a build without libghostty records its fallback in the log only; `haetty-headless` shows nothing
  of it without `--trace`.
