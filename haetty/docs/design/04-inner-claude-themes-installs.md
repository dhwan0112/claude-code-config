# haetty design: inner design: Claude Code, themes, stable installs (3.13-3.15)

> Part of the design. Index: [`../DESIGN.md`](../DESIGN.md) (section
> numbers are the same there). Text below is moved verbatim from the former `docs/DESIGN.md`.

### 3.13 Claude Code integration (milestone 3)

- Which tabs: the program's file name is `claude` (`.exe`, `.cmd`), and
  `claude-integration` is on (config or `--claude-integration false`; the
  GUI passes it in `CreateSpec`). The daemon then adds
  `--settings <run dir>/settings-<session>.json` (0600 in the 0700 runtime
  directory; Windows `%LOCALAPPDATA%\haetty\run`), removed when the session
  ends or is closed, and at daemon start for sessions a killed daemon left.
  The GUI's tab spec never contains it, so tab sets do not save it.
- What is injected: `hooks` and `statusLine`, nothing else. Not taken from
  ccx (intentionally): its `permissions` defaults (bypassPermissions unless
  `--safe`, 39 `ask` rules for destructive git/gh), `tui`,
  `autoCompactWindow`, `CLAUDE_CODE_NO_FLICKER`/`TMUX_TRUECOLOR` — they
  change Claude Code's behaviour, which a terminal must not.
- A `--settings` the user gave (file, relative to the tab's folder, or
  inline JSON) is merged into one file: our hook entries first and the
  user's appended per event, every other key the user's, the user's
  `statusLine` dropped with a log warning (ours feeds haetty's status row).
  `~/.claude/settings.json` is never touched.
- Hooks use Claude Code's exec form (`command` = haetty's path with forward
  slashes, `args` = `["hook", EVENT]`): no shell on any platform (checked:
  their parent process is `claude`/`claude.exe`). Events: SessionStart,
  UserPromptSubmit, PreToolUse (synchronous with matcher
  `Write|Edit|MultiEdit|NotebookEdit`, and asynchronous for all tools as
  `PreToolUseAll`), PermissionRequest (synchronous: the question is in the
  daemon before Claude Code shows its dialog; no output, so the normal
  permission flow decides), PermissionDenied, PostToolUse,
  PostToolUseFailure, Notification, Stop, StopFailure, SubagentStart/Stop,
  SessionEnd; all asynchronous except those two; timeout 5 s.
  `statusLine` has no exec form (checked 2026-11-06 in the status line
  guide and the settings reference: only `type`, `command`, `padding`,
  `refreshInterval`, `hideVimModeIndicator`): `haetty statusline` runs
  through Claude Code's shell (`sh -c`; Windows Git Bash when installed,
  else PowerShell). The path must therefore be one every shell runs bare
  (`settings::shell_safe_exe`): as it is when it has no spaces or shell
  characters; on Windows otherwise its 8.3 short path
  (`C:/Users/KIMTES~1/…/haetty.exe`), and when the volume has no short
  names a copy in `%ProgramData%\haetty\<dir>-<SID>\` (refreshed when the
  executable changes; only the user's own processes run it, so another
  user replacing it would only break its status line). A quoted path runs
  in sh/Git Bash only — PowerShell rejects it (checked: parser error).
  `refreshInterval` 3 s. Side effect documented by Claude Code: with a
  custom status line it hides most footer hints ("esc to interrupt",
  "? for shortcuts"); haetty's status row shows "esc to interrupt" while
  Claude Code works.
- `haetty hook EVENT` / `haetty statusline` are dispatched first in `main`
  (no logging, config, fonts): no `HAETTY_SESSION_ID` → exit; dial the
  daemon (50 ms, never start one) → none: exit without reading stdin;
  else read stdin (≤ 16 MiB), for the edit PreToolUse copy the file
  (≤ 2 MiB, UTF-8; never credential/key/token-looking paths: `.ssh`, `.aws`,
  `.env*`, `*.pem`, `*.key`, `*token*`, `*secret*`, `.credentials.json`, …),
  send it as a `Report` client, wait for `Synced`, exit 0. Never stdout or
  stderr (UserPromptSubmit's stdout would reach the model), silent panic
  hook. Deadline for the whole process: 250 ms for the synchronous hooks
  (of which at most 150 ms for the file copy — past that the event is
  reported without it and the edit has no diff), 1 s for the others. Measured: Linux median
  2.7–3.3 ms with and without a daemon, max 4.3 ms; stdin left open: 8 ms
  without a daemon, 1.02 s (the deadline) with one. Windows (this machine):
  process start alone is ~52 ms (`hostname.exe`, `whoami.exe` medians), the
  hook 64 ms — the 50 ms target is not reachable there; the extra ~12 ms is
  loading the GUI binary's DLLs (the same with an immediate exit). Claude
  Code waits only for the synchronous ones (edit PreToolUse,
  PermissionRequest; §3.4).
  Without a daemon the hook exits without reading stdin; Claude Code passes
  hook input over a socket and ignores the resulting `EPIPE` (checked up to
  4 MB input, review B7), so stdin is not drained.
- State per session (daemon, `haetty_cc::state`): Claude Code session id,
  name, model, attention, busy, context %, rate limits, cost, lines,
  files changed, edits, last edit, subagents, prompt cache — derived
  fields only, sent to attached clients as `Claude` messages when they
  change (a status-line refresh with the same content sends nothing, so an
  idle Claude Code tab does not wake the GUI every 3 s). Edit texts stay in
  the daemon (last 20) and are sent only on `GetEdit`.
- Attention (after ccx): Stop/StopFailure and Notification `idle_prompt` →
  `Wait`; PermissionRequest and Notification `permission_prompt`,
  `elicitation_dialog`, `elicitation_url_dialog`, `agent_needs_input` →
  `Ask`. `Wait` goes when the tab is looked at, on UserPromptSubmit and
  PreToolUse. `Ask` is not cleared by looking or by PreToolUse; only by
  UserPromptSubmit, PostToolUse(Failure), PermissionDenied, Stop whose hook
  started after the one that set it (each report carries its start time;
  asynchronous hooks can arrive out of order) — the permission state fails
  closed. While `Ask`, the daemon accepts only `Human` input (§3.7).
- Edits (after ccx `apply_edit`): the synchronous PreToolUse copies the
  file; PostToolUse rebuilds the text after from the copy and the tool's
  input (Write: content; Edit: first or all occurrences; MultiEdit in
  order; NotebookEdit: path only), so the diff is exactly that edit. Diff:
  `similar` (+87 KB Linux dist; a diff crate as asked, no `delta`).
- JSON: an own parser/writer (`haetty_cc::json`, ~350 lines): standalone
  +54 KB Linux / +51 KB Windows against serde_json's +71 / +64 KB.
- The event log gets `attention` lines (event name and result) only.
- In the window (all hidden or quiet by default, nothing opens by itself):
  - Tab bar marks on *other* tabs: `●` in theme colour 3 (amber/brown) =
    Wait, `?` in colour 9 (red) = Ask (the
    only accent colours besides usage near a limit). Selecting a tab, or
    focusing the window on it, clears Wait (the GUI sends
    `ClearAttention`); Ask stays until Claude Code moves on.
  - Unfocused window and Wait/Ask: `request_user_attention` (taskbar
    flash on Windows, urgency hint on X11; always) and, on Linux,
    `notify-send` when installed (`notifications = false` turns only the
    notification off). The text names
    the tab and says "waiting"/"asks you"; never session content. Windows
    toast notifications are not done (they need WinRT registration).
  - Status row (bottom row of Claude Code tabs, reserved from creation so
    Claude Code is not reflowed when the first report arrives; blank until
    there is something to show):
    `ctx  42%  5h  12%  7d  30%  $  1.23  3 files changed · Ctrl+Shift+D`,
    percentages and cost in fixed widths, greys; colour 3 ≥ 80 %, colour 9
    ≥ 95 %;
    "esc to interrupt" at the right while Claude Code works.
  - Status panel (Ctrl+Shift+S, a right column, hidden by default; the
    program is resized when it opens, and it is not shown below 40 program
    columns): only what Claude Code's own screen lacks — session id and
    name, context of size, limits with reset times (UTC: local time would
    need a time-zone dependency), cost ("API estimate" with subscription
    limits), lines, prompt cache, files and edits, last edit, subagents
    with durations, last update. Width (palette: wider/narrower, 24–80) and
    open state are kept in `<state dir>/ui`. Model and effort are left out
    (Claude Code shows them).
  - Diff view (Ctrl+Shift+D): the last edit of the active tab over its
    screen area (the program is not resized and keeps running), side by
    side from 110 columns, unified below; header `[last edit] path · +A -R
    · by agent`; keys j/k, Space/b, g/G, Esc/q, wheel. Decision: an
    overlay rather than a split, so Claude Code is never reflowed by
    looking at a diff and the full width is available for side by side.
  - Diff views vs HEAD and working tree (2026-11-06, ccx's `m` cycle): `m`
    or Tab cycles last edit → the last edited file against `HEAD` → every
    changed and untracked file of its repository (else the tab's folder's)
    against `HEAD` (changes made by Bash show here); `r` reads again,
    keeping the scroll position. `gitview.rs` runs `git` (`status
    --porcelain=v1 -z --untracked-files=all --no-renames`, one `cat-file
    --batch` for all `HEAD` blobs) with `GIT_OPTIONAL_LOCKS=0` (never
    rewrites the index while Claude Code works), no pager, no external
    diff, `CREATE_NO_WINDOW` on Windows. Limits (`gitview::Limits`): 300
    files read (more are counted, the first 300 by path shown), 2 MiB per
    file side (as for edits), 16 MiB in all, 3 s wall time for every `git`
    call together — over it the child is killed and the view says what was
    cut; each file's line diff gives up exactness after 500 ms. Credential-
    like paths are listed with "never read"; binary and over-size files
    with the reason. Measured (release, WSL2, 1 500 tracked files of 400
    lines, 1 200 changed, 300 untracked): limit 300 → collect 104 ms, diff
    43 ms, 120 301 rows; limit 1 500 → 966 files read before 16 MiB,
    collect 333 ms, diff 72 ms. Synchronous on the GUI thread (a key
    press): within the 3 s limit it stays on demand only; a background
    thread is the alternative if real repositories need more.
  - Resume: tab sets store the Claude Code session id; a restored
    Claude Code tab gets `--resume ID` when Claude Code still has the
    transcript (`<CLAUDE_CONFIG_DIR or ~/.claude>/projects/<folder with
    non-alphanumerics as ->/ID.jsonl`, existence only), with earlier
    `--resume`/`--continue` arguments removed; a conversation already open
    in a tab is not opened again.

### 3.14 Themes

- Format: Ghostty's theme files (`palette = N=COLOR`, `background`,
  `foreground`, `cursor-color`, `cursor-text`, `selection-background`,
  `selection-foreground`), read by `haetty_core::theme` with the config
  file's own line parser (`theme::lines`, shared) and libghostty-vt's colour
  parser (Ghostty's syntax: hex, X11 names). Keys a file leaves out keep
  "haetty classic"'s values (`Theme::base`: Ghostty's default 16 colours,
  `#dcdcdc` on `#1e1e1e`).
- `theme = VALUE` (config or `--theme`): a path when it contains `/` or `\`
  (relative to the config directory), else `<config dir>/themes/VALUE`, else
  a built-in theme (`assets/themes/`, embedded: "IBM 5153 CGA", "IBM 5153
  CGA (Black)", "haetty classic"); unknown → the default theme ("IBM 5153
  CGA", §2.2) and a log line. The colour keys in the config apply after the theme, in their
  order, wherever they stand.
- The two CGA files are copied unchanged (sources: `THIRD-PARTY-NOTICES.md`);
  at load, a selection background missing or equal to the foreground is
  filled in with `#4e4e4e` — built-in CGA files only.
- Where the colours go: default fg/bg, colours 0–15 and the cursor colour
  into the session's terminal (`SessionConfig::apply_theme` →
  `set_default_{fg,bg,cursor}_color`, `set_default_color_palette` with 16–255
  left as libghostty-vt's xterm cube and grey ramp), so OSC 4/10/11/12
  queries answer with them and snapshot cells carry the theme's RGB;
  `CreateSpec` carries them to the daemon (protocol version 5). Cursor text
  and selection colours are the painter's (`paint::Look`); a selected
  cell uses the theme's selection text colour only when its contrast with
  the selection background is not lower than the cell's own text colour's
  (`assets/themes/README.md`: CGA's `#141414` is 2.21:1 on `#4e4e4e`, the
  default text 5.29:1). The window's
  first frame and the Windows 11 title bar use the theme's background
  before a snapshot exists.
- `minimum-contrast` (1–21, default 1 = off; Ghostty's meaning and
  behaviour, `render::contrast`): a cell's text whose WCAG contrast with
  its background is below the value is drawn white or black, whichever
  contrasts more with that background; not for the text under a block
  cursor (cursor-text decides) nor box drawing, block elements, legacy
  computing and Powerline glyphs (Ghostty's `noMinContrast`). The default
  keeps the theme's colours as they are (IBM 5153 CGA: blue 1.85:1, bright
  black 2.21:1 on `#141414`; table in `assets/themes/README.md`). It
  applies to the program's cells only: `ui::Compose` marks every cell it
  did not take from the screen (tab bar, status row, panel, diff view,
  palette, hints) with `CellFlags::HAETTY_UI`, and the painter leaves
  those colours alone — otherwise, with CGA at 4.5, the `?` mark (4.02:1 on
  the bar) and the dim labels (3.55:1) would turn white against §2.2's
  accent and dimming rules.
- Windows 11: title bar, border and the padding take the theme's
  background (`chrome.rs`, from the first frame); checked by screen capture
  for each built-in theme and an override.
- Claude Code: its own themes draw in fixed 24-bit colours; its "Dark mode
  (ANSI colors only)" (`/theme`, saved as `"theme": "dark-ansi"` in the
  user's settings) uses palette numbers only, so it follows the theme.
  haetty does not set Claude Code's theme (§3.13: nothing but hooks and the
  status line is injected); the README tells the user.
- Changing the theme while haetty runs: the GUI reads the config file
  again when its window gets focus and the file's modification time
  changed, or on the palette action "Reload config"; a changed theme,
  colour key or `minimum-contrast` is applied at once — to haetty's own
  cells, the renderer (`Renderer::set_look`) and every open tab
  (`ClientMsg::SetTheme`, protocol version 6 → `SessionHandle::set_colors`:
  default colours, palette 0–15, cursor colour; colours a program set
  itself stay). A tab the GUI reattaches (crashed window, "Attach
  detached sessions") gets the current theme the same way, whatever it
  was started with. `SetTheme` is Control's only (role table in
  `Daemon::message`: Observe and Report are refused). Other settings
  still take effect at the next start.
- Programs see the new colours through queries made after the change;
  nothing is pushed to them. Claude Code picks its own light/dark look
  when it starts, so a running Claude Code keeps it; with its "Dark mode
  (ANSI colors only)" its palette-number colours follow at once.

### 3.15 Stable installs

- `tools/install-stable.sh` / `.ps1` build a commit or tag in a temporary
  `git worktree` with the `dist` profile (`HAETTY_BUILD_COMMIT` = that
  commit) and install it into a versioned folder that is never changed
  again (Linux `~/.local/opt/haetty/<version>/`, 0555; Windows
  `%LOCALAPPDATA%\Programs\haetty\<version>\`, read-only attribute).
  `<version>` = `<crate version>-<12-digit commit>`.
- One fixed path names the active version: Linux the symbolic link
  `~/.local/bin/haetty` (switched atomically), Windows the directory
  junction `…\haetty\current` (on the user `PATH`, behind the Start menu
  shortcut and taskbar pins). Rollback re-points it; old versions stay.
  Decision for Windows: a junction rather than a shortcut or a launcher
  executable — one path for the shell, the shortcut and pins, no second
  program to keep in step, no administrator rights.
- The installers stop nothing (CLAUDE.md: the stable instance is never
  signalled); a running stable daemon keeps its version until its windows
  are gone. `install-desktop` writes `Exec=` with the launching link
  (`install.rs::launcher_path`), so the menu entry follows the link.
- `haetty --version`: crate version, commit (`build.rs`: the installer's
  `HAETTY_BUILD_COMMIT`, else `git rev-parse`), UTC build date
  (`SOURCE_DATE_EPOCH` respected), instance.
- With `HAETTY_TEST_TMP`/`HAETTY_RUN` set (their self-tests) the installers
  refuse every path outside the test run directory before building.
