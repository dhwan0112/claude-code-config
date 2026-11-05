# Design: terminal + Claude Code adapter + orchestrator

Working name `ccterm`; product name undecided (candidates: haetty, trinitty).
This document fixes the outer design (what the user sees) and the inner
design (processes, crates, data flow) together, so that each screen element
has exactly one producer and one rendering path. `docs/HANDOFF.md` holds the
state of the code; this file holds the target.

## 1. Principles

1. One executable, Windows and Linux, no runtime besides system libraries.
2. Rust everywhere except the VT core (libghostty-vt, Zig, behind FFI in
   `ccterm-core`) and Microsoft's ConPTY binaries on Windows. No Python, no
   shell scripts, no jq/flock/delta at run time.
3. Everything on screen is a cell grid, drawn by one renderer with one glyph
   cache: terminal panes (from libghostty-vt snapshots) and our own panels
   (status, diff, worker list) alike. Pixel-level chrome is limited to the
   tab bar, separators, scrollbars, cursor and IME preedit.
4. Event-driven: no polling loops, no timers while idle except cursor blink
   when focused. Target idle CPU ~0 %.
5. Never read, store or forward Claude credentials. Run the user's installed
   `claude`; observe it through hooks, the status line and transcript files.

## 2. Outer design

Principle: nothing on screen without a job, and every function one keystroke
away. Functions stay; what changes is when they are visible.

### 2.1 Layout
- Default view: the terminal (Claude Code) and one status line. Tab bar only
  with two or more tabs. Status panel, diff pane and worker list are hidden
  by default and opened by key, palette or click; open/closed state and sizes
  persist per user.
- Events never change the layout. An edit, a finished worker or a pending
  permission shows an indicator in the status line or on the tab
  ("3 files changed · Ctrl+Shift+D"; plain Ctrl+D belongs to Claude Code, §3.10); only the user opens panes. Reason: every
  resize makes Claude Code's fullscreen renderer redraw the whole screen.
- No information Claude Code already shows (model, effort, mode). The status
  line and panel show context use, rate limits, cost and workers.

### 2.2 Visual rules
- Neutral greys (background, separator, dim, normal, strong) plus one accent
  family used only for state: waiting (amber), permission/question (red),
  error. Idle screen shows no accent colour.
- One font, one size everywhere; hierarchy by weight and dimming only. State
  glyphs (● ✓ ✗ ? -) instead of icons.
- Space instead of boxes; where a separator is needed, a 1 px low-contrast
  line. No shadows, gradients or rounded boxes inside the window.
- Key hints appear next to an element when it first shows, not in a resident
  list; a command palette (Ctrl+Shift+P) lists all actions.
- No sounds, pop-ups or animations. Notifications: tab marker, and an OS
  notification only when the window is unfocused.
- Numbers: fixed width, right-aligned, consistent units, no needless decimals.

### 2.3 Checks (report after implementation)
- Layout changes without user input: 0.
- Accent colours on an idle screen: 0.
- With one tab: no element besides the terminal and the status line.


## 3. Inner design

### 3.1 Processes

```
              local socket (Unix socket / Windows named pipe)
GUI client(s) ◀──────────────────────────────────────────▶ daemon
                                                            ├─ sessions: PTY + VT engine thread each
claude (in PTY) ──hook command──▶ `<bin> hook <event>` ───▶ ├─ Claude Code adapter (hooks, status line, transcripts)
                                                            └─ orchestrator (tasks, workers, reports)
```

- One executable. `ccterm` (no subcommand, or `gui`) is a GUI **client**;
  `ccterm daemon` owns the sessions. The GUI connects to the daemon's
  socket and, when nothing answers, starts `<own exe> daemon` detached and
  retries for up to 2 s. Subcommands are dispatched in `main` before any
  font, winit or softbuffer code runs, so `daemon`, `hook`, `statusline`
  and `status` never initialise GUI libraries.
- The daemon outlives the GUI. Linux: `setsid()` in the child before exec
  (new session, no controlling terminal), stdin/stdout/stderr on
  `/dev/null`, log to the state directory. Windows: `DETACHED_PROCESS |
  CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB` (no console; out of
  a job object the GUI may be in, e.g. a terminal's or a launcher's); if the
  job forbids breakaway, `CreateProcess` fails with access denied and the
  spawn is retried without the flag (logged; the daemon then dies with the
  job).
- The daemon exits when it has had no session and no client for 10 s.
  Two GUIs starting at once may both spawn a daemon: binding the socket
  decides, the loser exits quietly.
- Hooks and the status line are the same executable (`<bin> hook <event>`,
  `<bin> statusline`), reading stdin JSON and forwarding it over the socket.
  Paths in hook commands are quoted and use forward slashes (Windows runs hook
  commands through bash).
- `ccterm-headless` keeps running sessions in-process (tests, scripts).

### 3.2 Crates

| Crate | Responsibility |
|---|---|
| `ccterm-pty` | PTY / ConPTY |
| `ccterm-core` | VT engine thread, snapshots, input encoding (libghostty-vt behind FFI) |
| `ccterm-proto` | product name constants, paths, wire types (postcard), framing, local socket / named pipe transport, client for the session API |
| `ccterm-daemon` | session owner: sessions, subscriptions and frame diffs, roles, state file, event log |
| `ccterm-cc` | Claude Code adapter: `--settings` builder, hook and status-line parsing, edit snapshots (milestone 3) |
| `ccterm-gui` (bin `ccterm`) | window, renderer, tabs, palette, panels; subcommand dispatch (`gui`, `daemon`, `status`, `hook`, `statusline`, `install-desktop`) |
| `ccterm-headless` | in-process step runner |

Rendering and UI stay in `ccterm-gui` until a second consumer needs them
(the earlier plan had `ccterm-render` / `ccterm-ui` crates; splitting now
would only move files). The orchestrator (`ccterm-orch`) is milestone 4.

Replacements for `ccx` pieces: `panel.py` → status panel in `ccterm-gui`;
`diffview.py` + `delta` + `apply_edit.py` → diff pane in `ccterm-gui` with a
Rust diff (e.g. `similar`) and the pre-edit snapshot logic; `hook.sh`,
`post.sh`, `statusline-tee.sh`, `savetabs.sh` → `ccterm-cc` + daemon state;
`ccx_settings.py` → `ccterm-cc` settings builder; `tmux.conf` key bindings →
`ccterm-gui` keymap.

### 3.3 Rendering pipeline

`Wakeup` → snapshot (or row diff) → layout assigns each pane a rectangle →
each pane yields a cell grid → renderer rasterizes changed cells into a CPU
back buffer → copy damaged rectangles to the softbuffer buffer → present with
damage. The back buffer is owned by the renderer (softbuffer buffers may not
hold the previous frame). Renderer behind a trait so an OpenGL backend can be
added if profiling asks for it.

### 3.4 Budgets (to be measured, not assumed)

| Item | Target |
|---|---|
| Binary: `ccterm`, `dist` profile (stripped) | ≤ 12 MB Linux, ≤ 10 MB Windows (incl. the embedded ConPTY) |
| Idle CPU, Claude Code idle | ~0 % |
| Anonymous memory of ccterm — GUI plus daemon — with one session (120x36, 10k lines of scrollback at 120 columns), Claude Code idle | ≤ 25 MB |
| Anonymous memory per additional session or tab (daemon + GUI), Claude Code idle, its scrollback filled to the default limit | ≤ 16 MB |
| Key press → frame | ≤ 16 ms at 120x36 on a laptop CPU |

Anonymous memory is Linux `RssAnon` (`/proc/PID/status`) and Windows
private working set: heap, stacks and anonymous mappings of ccterm itself.
Not counted: file-backed pages (shared libraries, memory-mapped fonts;
`RssFile`), shared memory with the display server (X11 MIT-SHM, Wayland
pool buffers; `RssShmem`) and the child process (Claude Code).
Scrollback is a byte limit (`scrollback-limit`, `SessionConfig::
scrollback_limit` = libghostty-vt's `max_scrollback`, as Ghostty's
`scrollback-limit`); "10k lines" means a limit large enough to keep 10 000
rows of 120 columns. The default, 12 000 000 bytes, keeps about 11 000 such
rows (~1.1 KB per row). Measured 2026-11-05 with the default filled and
Claude Code idle: Linux 19.2 MB `RssAnon`, Windows 22.6 MB private working
set — inside the budget (single process, before the daemon split).
Per session since the daemon (2026-11-05, Linux, Claude Code idle, little
scrollback): 1 / 4 / 8 tabs → daemon 5.4 / 17.8 / 34.3 MB, GUI 5.5 / 6.8 /
8.1 MB, i.e. about 4.1 MB per session in the daemon and 0.3 MB per tab in
the GUI; a full default scrollback adds up to about 11 MB to its session
(`examples/mem.rs`), hence the 16 MB per-session budget.

Measured at the end of milestone 3 (2026-11-05; Linux under Xvfb, Windows
11 where noted; Claude Code 2.1.318 idle with the integration on, status
line every 3 s):

| Item | Target | Measured |
|---|---|---|
| `dist` binary | ≤ 12 MB / ≤ 10 MB | Linux 6.3 MB, Windows 4.9 MB (see HANDOFF.md §6.4 for each phase) |
| Idle CPU, Claude Code idle, 30 s | ~0 % | GUI 0 ticks, daemon 1 tick (10 ms) |
| GUI + daemon, one session, default scrollback filled at 120 columns | ≤ 25 MB | 6.3 + 14.3 = 20.5 MB |
| Per additional session, scrollback filled | ≤ 16 MB | +14.0 MB (2nd: daemon +13.1, GUI +0.9), +13.4 MB (3rd) |
| GUI + daemon, one Claude Code tab, little scrollback | — | 5.9 + 5.1 MB |
| Key → frame through the daemon, 120x36 | ≤ 16 ms | median 1.95 ms, p95 2.33 ms (Linux); 1.65 ms (Windows, P1) |
| GUI draw per incremental frame, 120x36 | — | median 0.09 ms |
| `ccterm hook` process, no daemon / with daemon | ≤ 50 ms | Linux 2.9 / 3.2 ms; Windows 64 ms (process start alone 52 ms: not met there) |

### 3.5 Local socket

- Linux: a Unix socket `$XDG_RUNTIME_DIR/<app>/daemon.sock`; without
  `XDG_RUNTIME_DIR`, `/tmp/<app>-<uid>/daemon.sock`. The directory is
  created with mode 0700; an existing one is used only if it is a real
  directory (not a symlink) owned by the user with no group/other bits —
  otherwise the daemon refuses to start (the `/tmp` fallback is open to
  squatting). A stale socket (connect refused) is unlinked before binding.
- Windows: a named pipe `\\.\pipe\<app>-<user SID>` (the pipe namespace is
  global, so the SID keeps users apart). Created with a DACL that grants
  only the user's SID (SDDL `D:P(A;;GA;;;<SID>)`), `PIPE_REJECT_REMOTE_CLIENTS`,
  and `FILE_FLAG_FIRST_PIPE_INSTANCE` on the first instance so nobody can
  pre-create it. Overlapped I/O on both ends: with synchronous handles a
  blocking `ReadFile` in the reader thread would block `WriteFile` from the
  writer thread on the same handle.
- No network listener, ever (remote access is DESIGN §6's job: CLI over SSH,
  MCP adapter in a separate binary).

### 3.6 Wire format and session protocol

- Frames: `u32` little-endian length, then a postcard-encoded message;
  frames over 32 MiB close the connection. Serialization measured with the
  same 11-message schema (dist profile, stripped, 2026-11-05): hand-written
  codec = baseline, **postcard +13.1 KB Linux / +13.3 KB Windows**, bincode
  +21.8 / +18.4 KB; encoded size of a two-row 120-column frame 3 328 /
  2 336 / 4 307 bytes. postcard is chosen: derive-based (no hand-written
  codec for ~40 message types), smallest frames (varints).
- Versioning: the client's first message is `Hello { magic: "CCTM",
  version, role, name }`, the daemon answers `Welcome { version }` or
  `Refused`. postcard encodes enum variants by index and a receiver
  cannot skip a variant it does not know (the decode fails and the
  connection drops), so **every** new variant or field bumps the version:
  an older daemon then refuses a newer client in `Hello`, and the GUI says
  that a daemon of another version runs and how it goes away (decided in
  P4, version 2; the first rule, "append only, same version", would have
  let a P3 daemon drop a P4 GUI on its first new message). Alternative not
  taken: feature bits in `Hello` (more code for a case that only occurs
  across an upgrade with sessions open).
- Text from users or programs (pastes, hook payloads, file contents) is
  wrapped in `Secret`, whose `Debug` shows the size only, so logging a
  message never logs its content.
- Schema: the `SessionHandle` methods (`key`, `mouse`, `wheel`, `paste`,
  `focus`, `resize`, `scroll`, `set_color_scheme`) and `SessionEvent`
  carried over one-to-one, plus `CreateSession`, `ListSessions`,
  `Attach`/`Detach`, `NextFrame`, `Close`, and later `Hook`, `StatusLine`,
  tab and tab-set messages. libghostty key/mouse enums travel as their
  `u32` values, `Mods` as `u16`.
- Screen transfer: on `Attach`, and whenever the grid size changes, a full
  frame; otherwise the rows that differ from what that client last
  received, with `seq`. The daemon keeps one baseline snapshot per
  (client, session). Backpressure: a client gets the next frame only after
  it sent `NextFrame`; while it is busy, the daemon keeps only the newest
  snapshot, so a flood of output costs the slow client intermediate frames,
  never memory. This is the same contract as `SessionEvent::Wakeup`
  ("not repeated until a snapshot was taken"), so the GUI code is
  unchanged: the client library turns a received frame into `Wakeup` and
  `snapshot()` into "apply, then `NextFrame`".
- The session's grid size is the last `Resize` from a control client.
- `CreateSession` carries the command, working directory and the
  requesting client's **environment** (PATH, DISPLAY, the user's own API
  key setup…); the environment lives only in the child process — never in
  the state file, tab sets or the event log.

### 3.7 Roles

- `control`: the local GUI. Input (`Key`, `Mouse`, `Wheel`, `Paste`,
  `Focus`, `Resize`, `Scroll`, raw writes), `CreateSession`, `Close`.
- `observe`: read-only (`ccterm status`, later CLI over SSH and MCP per §6).
  Any input or session-changing message is answered with `Denied` and has no
  effect.
- `report`: hook and status-line processes; may only send `Hook` and
  `StatusLine` — and only they may send them (a Control or Observe client
  sending a report is answered with `Denied`).
- Every input message carries `origin` (`Human` = typed or clicked by the
  person, from an OS input event; `Program` = made up by ccterm or a
  program: palette actions, restored tabs, scripts, automation). The client
  library has no default: every input method takes the origin. While a
  session waits for a Claude Code permission answer or a question (known
  from hook events), the daemon rejects input whose origin is not `Human`,
  whatever the role. Remote parties never answer permission prompts (§6).
- What roles are and are not: a **cooperative separation between processes
  of the same user**, not a security boundary. Any process of that user can
  open the socket (0700 directory / pipe ACL for the user's SID) and claim
  any role and any origin; the roles keep ccterm's own parts honest (a hook
  cannot type, `status` cannot close a session, a palette action cannot
  answer a permission prompt) and make mistakes visible (`Denied`). The
  boundary against other users is the socket's permissions; against the
  user's own processes there is none.
- Hence the rule for remote adapters (CLI over SSH, MCP, milestone 4+):
  they must be built on a client type that **cannot send Control messages
  at all** (an observe-only client API, no input or session methods), not
  on the full client with a role set to Observe; anything a remote party
  can make the adapter send is at most `Observe`.

### 3.8 Identity, state and event log

- Session and tab ids: `<unix ms, hex>-<counter, hex>` (sortable, unique per
  machine). A restored tab is a new session (new id): a tab set stores what
  to start, not the session; Claude Code session ids (UUIDs, from hooks) are
  stored with it so that Claude Code itself resumes (`--resume`).
- Tab sets (`tabs/<name>.tsv` in the state directory, after ccx's
  `savetabs.sh`): per tab name, folder (OSC 7, else the start folder),
  program, arguments, Claude Code session id, active flag; never
  environment variables. `last` follows the open tabs (rewritten only when
  the content changes); at start it becomes `previous` and is reopened
  after the requested tab; `restore-tabs = false` (config or
  `--restore-tabs false`) turns that off; palette: save as `saved`, open
  `saved`, open `previous`. Decision: restore *after* the requested tab
  rather than instead of it, so `ccterm PROGRAM` always runs `PROGRAM` and
  nothing is lost when a set is broken; tabs whose folder is gone are
  skipped with a log line. Alternatives not taken: reattaching the
  sessions a killed GUI left in the daemon (possible later: they are in
  `status`), a set per window (one window today; with several windows the
  last writer wins `last`), JSON (no parser in the binary; TSV with
  escapes is enough).
- State directory: Linux `$XDG_STATE_HOME/<app>` (default
  `~/.local/state/<app>`), Windows `%LOCALAPPDATA%\<app>`. Config:
  Linux `$XDG_CONFIG_HOME/<app>/config` (`~/.config/<app>/config`), Windows
  `%APPDATA%\<app>\config`.
- `events.log` in the state directory: append-only, one tab-separated line
  per event — `seq`, unix ms, kind, session id, derived fields. A reader
  continues from a `seq` cursor. **Only derived fields** are written: event
  names, tool names, file paths, Claude Code session ids, exit codes,
  titles. Never environment variables, prompts (`UserPromptSubmit` carries
  the typed prompt), `tool_input` contents, transcripts or credentials.
- Tab sets and the daemon's session list are files in the state directory
  (milestone 2, P3).
- Friction log (added on request after P4), in the same `events.log`:
  - `slow_input` — key or paste to the next screen change over the 16 ms
    budget (measured in the daemon; includes the program's own reaction,
    e.g. Claude Code redrawing), at most one line per session per 10 s;
  - `latency` — per session at close/exit, daemon stop, and every 10 min
    with ≥ 1000 samples: `n`, `p50_ms`, `p95_ms`, `max_ms`, `over_16ms`;
  - `panic` — daemon panics (message from our code, location, thread);
  - `error` — session errors and failed starts;
  - `gui_slow_draw` (a GUI frame over 16 ms, at most every 10 s) and
    `gui_panic`, sent by the GUI as `Friction` messages (Control role only,
    through the existing role rules).
  Never typed or shown text (a test pastes a marker and checks it is not
  in the log).

### 3.9 Product name

The working name (`ccterm`; candidates haetty, trinitty) lives in one
constant module, `ccterm_proto::product`: executable name used when
spawning the daemon and in hook commands (taken from `current_exe()` at run
time, falling back to the constant), app id (Wayland `app_id`, X11
`WM_CLASS`, `.desktop` file, Windows AppUserModelID), socket, pipe, state and
config directory names, window title prefix. Renaming = that module plus the
`[[bin]] name` in `crates/ccterm-gui/Cargo.toml` (Cargo needs a literal).

Instances (added on request after P4): next to the name constants,
`product::instance()` reads `CCTERM_INSTANCE` (lower-case ASCII letters,
digits, `-`, ≤ 16; unset or empty = stable). A named instance gets its own
socket/pipe (`ccterm-dev`), runtime, state and config directories
(`~/.local/state/ccterm-dev`, …), app id (`ccterm-dev`, so its desktop file
and dock icon are separate) and window title prefix (`[dev] `). The daemon
sets `CCTERM_INSTANCE` in every session's child, so hooks of a dev session
report to the dev daemon. Purpose: run a development build beside the
stable one (self-hosting) without touching its sessions, tab sets or
settings. Checked by `crates/ccterm-gui/tests/instances.rs` with two real
daemons.

### 3.10 Layout rules derived from §2

- A tab whose program exits stays, dimmed with its exit status, until the
  user closes it; with one tab, the window closes when its program exits
  (as in milestone 1). So an event never removes a tab or the tab bar.
- The status line row is part of the layout from the first frame when
  Claude Code integration is on (it shows `-` until the first status line
  JSON), so its arrival does not resize the terminal. Window size requests
  add that row, so the terminal keeps the requested rows.
- The diff indicator says "N files changed · Ctrl+Shift+D": plain Ctrl+D
  reaches Claude Code (and exits it on an empty prompt), so app shortcuts
  use Ctrl+Shift.
- `statusLine` is a single value: injecting ccterm's replaces the user's own
  status line in ccterm tabs (as in ccx). `ccterm statusline` prints
  nothing, so Claude Code draws no line of its own; the information goes to
  ccterm's status line instead.
- Settings are injected only into tabs whose program is `claude`.

### 3.11 Keys

Application shortcuts use Ctrl+Shift (plus Ctrl+Tab and Alt+digit), never
plain Ctrl+letter, which belongs to the program. Checked against Claude
Code's interactive-mode reference (code.claude.com/docs/en/interactive-mode,
2026-11-05): it binds Ctrl+C/D/G/L/O/R/V/B/T/S/Z/A/E/K/U/W/Y/P/N/_,
Ctrl+Shift+- (undo), Ctrl+Enter, Ctrl+X chords, Shift+Tab, Tab, Esc,
Alt+M/P/T/O/Y/B/F/D/V.

| Action | Default | Clash check |
|---|---|---|
| New tab | Ctrl+Shift+T | Claude Code: Ctrl+T (task list) only; distinct with the kitty protocol, legacy terminals cannot tell them apart but ccterm consumes the chord before the program |
| Close tab (press twice) | Ctrl+Shift+W | Claude Code: Ctrl+W (delete word); same remark |
| Next / previous tab | Ctrl+Tab / Ctrl+Shift+Tab, Ctrl+PageDown / Ctrl+PageUp | free in Claude Code; Shift+Tab alone stays with the program |
| Go to tab N | Alt+1 … Alt+9 | free in Claude Code; bash/readline uses Alt+digit as a numeric argument (rarely typed) — rebind if needed |
| Command palette | Ctrl+Shift+P | free (Claude Code: Ctrl+P = previous history line) |
| Paste | Ctrl+Shift+V, Shift+Insert, Paste key; Windows also Ctrl+V | Linux plain Ctrl+V stays with Claude Code (image paste) |
| Diff pane / status panel (milestone 3) | Ctrl+Shift+D / Ctrl+Shift+S | free (Claude Code: Ctrl+D exit, Ctrl+S stash) |

Linux: GNOME/KDE terminals use the same Ctrl+Shift family inside their own
windows only; ccterm's window gets the keys first. Windows: Windows
Terminal uses Ctrl+Shift+T/W and Ctrl+Tab the same way; Alt+digit is free.
Every binding can be changed in the config file (`keybind = CHORD=ACTION`,
`CHORD=unbind`). Irreversible actions (close) ask for the same chord again
within 3 s, with a short hint over the bottom row; no dialog.

### 3.12 Icon and desktop integration

The icon is a placeholder drawn in code (`ccterm-gui/src/icon.rs`, the
single point to replace): neutral greys, `>_` on a rounded square. It
feeds the window icon (X11 `_NET_WM_ICON`, Windows small and big icons),
`ccterm install-desktop` (Linux: `~/.local/share/applications/ccterm.desktop`
with `StartupWMClass`, hicolor 48/128/256 PNG and SVG; Windows: nothing to
install) and `build.rs` (Windows `.exe` icon resource via `windres`, sizes
16–64: PNGs are written uncompressed by our own writer, and a 256 px entry
would add 262 KB). Wayland `app_id` = X11 `WM_CLASS` = `APP_ID`
("ccterm"), so docks match the window to the desktop file.

### 3.13 Claude Code integration (milestone 3)

- Which tabs: the program's file name is `claude` (`.exe`, `.cmd`), and
  `claude-integration` is on (config or `--claude-integration false`; the
  GUI passes it in `CreateSpec`). The daemon then adds
  `--settings <run dir>/settings-<session>.json` (0600 in the 0700 runtime
  directory; Windows `%LOCALAPPDATA%\ccterm\run`), removed when the session
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
  `statusLine` dropped with a log warning (ours feeds ccterm's status row).
  `~/.claude/settings.json` is never touched.
- Hooks use Claude Code's exec form (`command` = ccterm's path with forward
  slashes, `args` = `["hook", EVENT]`): no shell on any platform (checked:
  their parent process is `claude`/`claude.exe`). Events: SessionStart,
  UserPromptSubmit, PreToolUse (synchronous with matcher
  `Write|Edit|MultiEdit|NotebookEdit`, and asynchronous for all tools as
  `PreToolUseAll`), PermissionRequest (synchronous: the question is in the
  daemon before Claude Code shows its dialog; no output, so the normal
  permission flow decides), PermissionDenied, PostToolUse,
  PostToolUseFailure, Notification, Stop, StopFailure, SubagentStart/Stop,
  SessionEnd; all asynchronous except those two; timeout 5 s.
  `statusLine` has no exec form: `ccterm statusline` through Claude Code's
  shell (`sh -c`; Windows Git Bash, else PowerShell), path bare when it has
  no shell characters (every one of those shells runs it), else
  double-quoted (sh/Git Bash only; PowerShell would need `&` — not handled),
  `refreshInterval` 3 s. Side effect documented by Claude Code: with a
  custom status line it hides most footer hints ("esc to interrupt",
  "? for shortcuts"); ccterm's status row shows "esc to interrupt" while
  Claude Code works.
- `ccterm hook EVENT` / `ccterm statusline` are dispatched first in `main`
  (no logging, config, fonts): no `CCTERM_SESSION_ID` → exit; dial the
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
  loading the GUI binary's DLLs (the same with an immediate exit). Only the
  edit PreToolUse makes Claude Code wait for it.
- State per session (daemon, `ccterm_cc::state`): Claude Code session id,
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
- JSON: an own parser/writer (`ccterm_cc::json`, ~350 lines): standalone
  +54 KB Linux / +51 KB Windows against serde_json's +71 / +64 KB.
- The event log gets `attention` lines (event name and result) only.
- In the window (all hidden or quiet by default, nothing opens by itself):
  - Tab bar marks on *other* tabs: amber `●` = Wait, red `?` = Ask (the
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
    Claude Code is not reflowed when the first report arrives):
    `ctx  42%  5h  12%  7d  30%  $  1.23  3 files changed · Ctrl+Shift+D`,
    percentages and cost in fixed widths, greys; amber ≥ 80 %, red ≥ 95 %;
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
  - Resume: tab sets store the Claude Code session id; a restored
    Claude Code tab gets `--resume ID` when Claude Code still has the
    transcript (`<CLAUDE_CONFIG_DIR or ~/.claude>/projects/<folder with
    non-alphanumerics as ->/ID.jsonl`, existence only), with earlier
    `--resume`/`--continue` arguments removed; a conversation already open
    in a tab is not opened again.

## 4. VT engine decision (measured 2026-11-04)

Replay of a real Claude Code 2.1.318 session (startup, Hangul typing,
Shift+Enter, fullscreen renderer; 2.5 KB of output) at 120x36:
libghostty-vt and alacritty_terminal 0.26 produced identical screens, equal
to the live screen. A 16.9 MB synthetic stream (24-bit colour, Hangul, CJK,
emoji, box drawing, synchronized updates) x3 at 120x36, one vCPU:
libghostty-vt ~31–34 MB/s, alacritty_terminal ~50–73 MB/s; identical final
screens. Building libghostty-vt for `x86_64_v3` instead of `baseline` did
not change throughput measurably. Minimal stripped binaries (engine +
replay): 2.0 MB vs 0.6 MB.

The memory figures of that run (max RSS ~20 MB vs ~47 MB) are void: the
benchmark gave libghostty-vt `max_scrollback = 10000`, which is bytes
(about 680 rows of history), and alacritty_terminal 10 000 lines. They are
no argument for either engine. Re-measured 2026-11-05 with about the same
history in both (`bench/engine-compare`, same stream x3 at 120x36, all
cores, three runs; max RSS includes the 16.9 MB input held in memory):

| Engine | Scrollback | Max RSS | Throughput |
|---|---|---|---|
| libghostty-vt 0.2.2 | 11 000 000 bytes (≈10 200 rows of 120 columns) | 31.5–31.6 MB | 32.5–37.1 MB/s |
| alacritty_terminal 0.26 | 10 000 lines | 47.6 MB | 110–120 MB/s |

Both max RSS figures include the same 16.9 MB input file held in memory;
without it they are roughly 31.6 − 16.9 ≈ 15 MB (libghostty-vt) and
47.6 − 16.9 ≈ 31 MB (alacritty_terminal): with about the same history
libghostty-vt uses about half the memory. This is an approximation — the
subtraction assumes the input buffer is fully resident in both and ignores
other differences (binary size 2.0 vs 0.6 MB, allocator behaviour, ~2 %
more rows kept by libghostty-vt). (The old condition, libghostty-vt with
10 000 bytes: 20.6 MB.) The final
screens of the two replays differ only in which 36 rows the harnesses
print (one row offset); the rows themselves match.

Claude Code's fullscreen renderer emits kilobytes per interaction, so
throughput is not the constraint for this workload; memory, correctness of
widths (emoji presentation, graphemes) and the built-in key/mouse encoders
and query replies are. Decision: keep libghostty-vt as the only non-Rust
component, isolated in `ccterm-core`; keep `Snapshot` engine-neutral so an
alacritty_terminal backend can be added and A/B-tested later if the Zig
toolchain becomes a burden. Re-run the comparison on Windows once the GUI
exists.

## 5. Milestones

1. GUI, one window one session (HANDOFF.md §6), with this document's theme,
   padding and decoration rules.
2. Daemon split, IPC, tabs, saved/restored tab sets, `install-desktop`.
3. Claude Code adapter: hook/status-line subcommands, status panel, diff
   pane, attention states.
4. Orchestrator: tasks, worker tabs, worktrees, worker reports.

## 6. External integration (very long term): Rakazo

Rakazo stays unmodified (no fork). Integration goes through surfaces it
already has:
1. CLI with stable JSON output (`<bin> status --json`, `<bin> run --task ... --json`),
   callable by Rakazo bots over SSH.
2. An MCP adapter (separate feature or binary, not in the main executable)
   speaking to the daemon socket, registered in Rakazo as an HTTPS MCP server
   reachable only over Tailscale, token-protected, read-only by default.
3. Optional push of events into Rakazo chats through its client API, only once
   that API is stable.

Constraints on current work: the daemon API is a typed, versioned
command/event schema shared by GUI, CLI and MCP; sessions, workers and tasks
have durable ids; orchestrator events go to an append-only log readable from a
cursor. Remote parties never approve Claude Code permission prompts; approvals
stay with a human.
