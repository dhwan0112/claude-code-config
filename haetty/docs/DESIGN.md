# Design: terminal + Claude Code adapter + orchestrator

Product name **haetty** (working name until 2026-11-05: `ccterm`, also the
name of the crates and of the repository directory until then).
This document fixes the outer design (what the user sees) and the inner
design (processes, crates, data flow) together, so that each screen element
has exactly one producer and one rendering path. `docs/HANDOFF.md` holds the
state of the code; this file holds the target.

## 1. Principles

1. One executable, Windows and Linux, no runtime besides system libraries.
2. Rust everywhere except the VT core (libghostty-vt, Zig, behind FFI in
   `haetty-core`) and Microsoft's ConPTY binaries on Windows. No Python, no
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
  with two or more tabs. The status line exists only in Claude Code tabs and
  shows only when it has something to show (a reserved row stays blank until
  then, §3.10); a shell tab has no status line and gets the whole height. Status panel, diff pane and worker list are hidden
  by default and opened by key, palette or click; open/closed state and sizes
  persist per user.
- Events never change the layout. An edit, a finished worker or a pending
  permission shows an indicator in the status line or on the tab
  ("3 files changed · Ctrl+Shift+D"; plain Ctrl+D belongs to Claude Code, §3.10); only the user opens panes. Reason: every
  resize makes Claude Code's fullscreen renderer redraw the whole screen.
- No information Claude Code already shows (model, effort, mode). The status
  line and panel show context use, rate limits, cost and workers.

### 2.2 Visual rules
- Colours come from one theme (Ghostty's theme format, §3.14); the default
  is **IBM 5153 CGA** (the IBM 5153 colour monitor's CGA colours: `#cecece`
  on `#141414`, colour 3 brown). The terminal's 16 colours, background,
  foreground and cursor are the theme's.
- haetty's own cells (tab bar, status row, status panel, command palette,
  separators, hints) use neutral greys mixed from the theme's background
  and foreground (background, separator, dim, normal, strong), plus one
  accent family used only for state, taken from the theme: waiting = colour
  3, permission/question = colour 9 (also usage ≥ 80 % / ≥ 95 %); the diff
  view tints added/removed lines with colours 2/1. Another theme changes
  all of them. Idle screen shows no accent colour.
- One font, one size everywhere; hierarchy by weight and dimming only. State
  glyphs (● ✓ ✗ ? -) instead of icons.
- Space instead of boxes; where a separator is needed, a 1 px low-contrast
  line. No shadows, gradients or rounded boxes inside the window.
- Tab bar: on the terminal's own background (no band); the active tab's
  text in the foreground colour with a thin line of that colour under its
  whole share (the font's stroke: 1 px at 100 %, 2 px at 200 %), other tabs'
  text dim; no accent colour for the active tab; between the tab bar and
  the terminal no line, only space.
- Integrated decorations (`decorations = integrated`; the default on
  Wayland, §3.12): the tab-bar row is the title bar and stays with one tab,
  then showing the window title in the dim colour. Window buttons (minimize
  `–`, maximize `□` / restore `■`, close `×`) are single-colour glyphs on
  the cell grid, dim, the normal text colour under the pointer — close
  included, no accent — on the sides GNOME's `button-layout` names (right
  when it cannot be read). No shadow and no rounded corners are drawn: the
  window is a plain rectangle; what the compositor adds is its own.
- Padding around the grid: at least 10 px left/right and 6 px top/bottom
  (logical px, times the display scale); leftover space spread evenly
  (`window-padding-balance`).
- Key hints appear next to an element when it first shows, not in a resident
  list; a command palette (Ctrl+Shift+P) lists all actions.
- Underlines in the program's text are drawn as it asks: single, double,
  curly, dotted or dashed (SGR 4:x, 21), in its underline colour (SGR 58)
  or else the text's (E5, the user's DESIGN §2 change, 2026-11-08).
- Hyperlinks (OSC 8) have no mark of their own: plain text. Under the
  pointer the link's cells are underlined and the status row's place (the
  bottom row; the top screen row while the pointer is on the bottom one)
  shows the target in the dim colour, scheme and host always whole.
  Ctrl+click opens it with the operating system's default program — `http`,
  `https` and `file` only; another scheme is not opened, and a note after
  the target (which stays in view) says so. A link's text and its target
  can differ, so the target is always shown before a click can open it
  (E5, as above).
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

- One executable. `haetty` (no subcommand, or `gui`) is a GUI **client**;
  `haetty daemon` owns the sessions. The GUI connects to the daemon's
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
- `haetty-headless` keeps running sessions in-process (tests, scripts).

Robustness rules (stage S4, 2026-11-06):
- The engine never blocks on the child: bytes for the child go through one
  queue to a PTY writer thread; query replies are dropped while more than
  64 KiB wait, input while more than 64 MiB wait. (A program flooding
  terminal queries without reading its input had blocked the engine in
  `write`, and with it every `snapshot()` — the daemon's main loop too.)
- Grid sizes are clamped to 1..=4096 columns and 1..=2048 rows (a 65535 x
  65535 resize from any Control client ended the engine thread).
- A dropped session stops without processing queued output first.
- Frontend commands (snapshots, input, resizes, shutdown) have their own
  unbounded channel, read before the bounded output channel and again
  after at most 16 output chunks: a busy session answers a snapshot at
  once, and no sender waits for it (the daemon's main loop asks for
  snapshots; with a slow engine a firehose session had held it up).
- A session that ends while bytes still wait for a child that does not
  read: the writer stops after its current 4 KiB chunk, and the child gets
  what closing the terminal would give it — SIGHUP and the terminal's
  queues flushed (`PtyControl::hangup`; Windows: closing the pseudo console
  does it) — because the blocked `write` holds the terminal open and Linux
  does not wake it when the child's side closes. Without this, such a
  session kept its child and four threads (found after S4 by
  `tests/threads.rs`).
- A malformed connection (bad length, broken postcard, unknown message,
  wrong magic or version) ends only that connection.
- The daemon dies → its sessions' programs end with it (no orphans: SIGHUP
  from the closed PTY on Linux, the pseudo console closing on Windows); the
  GUI notices (`Client::is_lost`), keeps its window and a hint, and leaves
  the tab sets as they were, so the next start reopens the tabs.
- Builds: libghostty-vt-sys picks Zig's mode from its build script's
  `DEBUG` and `OPT_LEVEL`; Cargo.toml turns debug info off for that one
  package in `release` and `dev`, so every profile builds the optimized
  engine — dist ReleaseSmall, release and dev/test ReleaseFast (before:
  release and dev/test Zig Debug, 200-300x slower; T3, proven per profile by
  `scripts/zig-mode.sh`). Zig's safety checks: a separate fuzz run with
  `LIBGHOSTTY_VT_SYS_OPTIMIZE=ReleaseSafe`.

### 3.2 Crates

| Crate | Responsibility |
|---|---|
| `haetty-pty` | PTY / ConPTY |
| `haetty-core` | VT engine thread, snapshots, input encoding (libghostty-vt behind FFI) |
| `haetty-proto` | product name constants, paths, wire types (postcard), framing, local socket / named pipe transport, client for the session API |
| `haetty-daemon` | session owner: sessions, subscriptions and frame diffs, roles, state file, event log |
| `haetty-cc` | Claude Code adapter: `--settings` builder, hook and status-line parsing, edit snapshots (milestone 3) |
| `haetty-gui` (bin `haetty`) | window, renderer, tabs, palette, panels; subcommand dispatch (`gui`, `daemon`, `status`, `hook`, `statusline`, `install-desktop`) |
| `haetty-headless` | in-process step runner |

Rendering and UI stay in `haetty-gui` until a second consumer needs them
(the earlier plan had `haetty-render` / `haetty-ui` crates; splitting now
would only move files). The orchestrator (`haetty-orch`) is milestone 4.

Replacements for `ccx` pieces: `panel.py` → status panel in `haetty-gui`;
`diffview.py` + `delta` + `apply_edit.py` → diff pane in `haetty-gui` with a
Rust diff (e.g. `similar`) and the pre-edit snapshot logic; `hook.sh`,
`post.sh`, `statusline-tee.sh`, `savetabs.sh` → `haetty-cc` + daemon state;
`ccx_settings.py` → `haetty-cc` settings builder; `tmux.conf` key bindings →
`haetty-gui` keymap.

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
| Binary: `haetty`, `dist` profile (stripped) | ≤ 12 MB Linux, ≤ 10 MB Windows (incl. the embedded ConPTY) |
| Idle CPU, Claude Code idle | ~0 % |
| Anonymous memory of haetty — GUI plus daemon — with one session (120x36, 10k lines of scrollback at 120 columns), Claude Code idle | ≤ 25 MB |
| Anonymous memory per additional session or tab (daemon + GUI), Claude Code idle, its scrollback filled to the default limit | ≤ 16 MB |
| Key press → frame | ≤ 16 ms at 120x36 on a laptop CPU |

Anonymous memory is Linux `RssAnon` (`/proc/PID/status`) and Windows
private working set: heap, stacks and anonymous mappings of haetty itself.
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

Measured 2026-11-06 (T3) on optimized builds — `dist`, what the installers
ship (libghostty-vt in Zig ReleaseSmall), and `release` (ReleaseFast);
`scripts/e2e/measure.sh` under Xvfb on Linux (WSL2), 120x36 unless noted.
The Zig mode of every profile is shown by `scripts/zig-mode.sh` (the
`zig build -Doptimize=…` line itself). The earlier figures ("before") were
taken with `release` builds whose engine was Zig **Debug** (see below).

| Item | Target | dist (shipped) | release | before (Zig Debug engine) |
|---|---|---|---|---|
| `dist` binary | ≤ 12 MB / ≤ 10 MB | Linux 6 656 016 B, Windows 5 210 624 B (`8d66d29`, 0.2.0) | — | 0.1.0: 6 437 584 / 5 014 016 |
| Key → frame through the daemon (`examples/latency.rs`, 300 keys) | ≤ 16 ms | median 0.96 ms, p95 1.19, max 1.50 | median 0.88, p95 1.05 | median 1.95, p95 2.33 (Linux); 1.65 (Windows, P1) |
| Throughput through a session, `cat` in `haetty-headless`, 80x24 | — | plain lines 24 MiB/s, long wrapped lines 55 MiB/s, SGR-heavy 53 MiB/s | 34 / 72 / 66 MiB/s | 20 000 short lines in ~21 s (~5 KB/s); 3.3 M `REP` characters in 50 s |
| Anonymous memory per session, default scrollback filled (`examples/mem.rs`) | — | 11.6–11.8 MB each (4 sessions: 47.4 MB) | 11.6–11.9 MB | — |
| GUI + daemon, one session, default scrollback filled | ≤ 25 MB | 6.0 + 13.2 = 19.2 MB | 5.9 + 13.2 = 19.1 MB | 6.3 + 14.3 = 20.5 MB |
| Per additional session, scrollback filled | ≤ 16 MB | +13.5 MB (2nd: GUI +1.2, daemon +12.4), 4 tabs 58.6 MB = +13.1 each | +13.8 / +13.2 | +14.0 (2nd), +13.4 (3rd) |
| GUI + daemon, one Claude Code tab, little scrollback | — | 5.6 + 1.6 MB, 9 daemon threads | 5.6 + 1.8 MB | 5.9 + 5.1 MB |
| Idle CPU over 30 s (all cases above) | ~0 % | GUI 0 ticks, daemon 0–1 tick | 0 / 0–1 | GUI 0, daemon 1 tick |
| GUI draw per incremental frame while typing | — | median 0.16 ms, p95 0.29 | median 0.10, p95 0.23 | median 0.09 |
| Claude Code's wait for a **synchronous** haetty hook (edit-tool PreToolUse, PermissionRequest): from Claude Code starting the process to its exit | ≤ 250 ms always (enforced by the hook's own deadline); typical ≤ 10 ms Linux, ≤ 100 ms Windows | not re-measured (no engine on that path) | — | Linux median 4.4 ms, max 5.1; Windows median 69 ms (PermissionRequest) / 76 ms (PreToolUse), max 92 ms — process start alone 57 ms there |
| Asynchronous hooks and the status line | not waited for by Claude Code; each ends within 1 s | not re-measured | — | Linux ~3 ms, Windows ~64 ms |
| 1 hour, 4 tabs (two writing — 500 lines every 10 s, a line every 0.2 s — two idle), stage S4 | no growth of fds / handles / threads; memory flat once scrollback is full | replaced by the T4 soak v2 (HANDOFF.md) | — | at `e2ec468` (Zig Debug engine, no PTY writer thread): Linux GUI 7.2 MB, daemon 10.6 → 30.2 MB then flat; Windows GUI 12.4–13.2 MB, daemon 15.3 → 46.7 MB then flat; no fd/handle/thread growth |
| Daemon answer (`haetty status`) while one tab runs `yes` | — | — | avg 6 ms, max 8 ms (idle 5 / 7), `8d66d29` | — |

Key → frame and memory hardly moved: they are bound by the round trip and
the scrollback bytes, not by the engine's speed. Throughput is where the
Debug engine was 200–300x slower; `dist` (installed builds) always had the
optimized engine. Before T3 the gate's `cargo test` also ran the Debug
engine; now `dev`/`test` build it in ReleaseFast (Cargo.toml), and a fuzz
run with Zig's safety checks is a separate command
(`LIBGHOSTTY_VT_SYS_OPTIMIZE=ReleaseSafe`, README "Testing").

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
- `observe`: read-only (`haetty status`, later CLI over SSH and MCP per §6).
  Any input or session-changing message is answered with `Denied` and has no
  effect.
- `report`: hook and status-line processes; may only send `Hook` and
  `StatusLine` — and only they may send them (a Control or Observe client
  sending a report is answered with `Denied`).
- Every input message carries `origin` (`Human` = typed or clicked by the
  person, from an OS input event; `Program` = made up by haetty or a
  program: palette actions, restored tabs, scripts, automation). The client
  library has no default: every input method takes the origin. While a
  session waits for a Claude Code permission answer or a question (known
  from hook events), the daemon rejects input whose origin is not `Human`,
  whatever the role. Remote parties never answer permission prompts (§6).
- What roles are and are not: a **cooperative separation between processes
  of the same user**, not a security boundary. Any process of that user can
  open the socket (0700 directory / pipe ACL for the user's SID) and claim
  any role and any origin; the roles keep haetty's own parts honest (a hook
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
- Reattach (review C8): a window that is gone (crashed, killed) leaves its
  sessions in the daemon. At start the GUI asks the daemon first
  (`SessionInfo.attached` = Control clients attached, `claude_session`):
  running sessions with no window become tabs again as they are; exited
  ones nobody shows are closed. Tab-set restore then applies only to saved
  tabs the daemon does not have (`tabsets::not_live`: Claude Code tabs by
  conversation id, others by program, arguments without resume requests,
  folder; each live session matches one saved tab), and a conversation
  running anywhere in the daemon is never resumed again (checked against
  the daemon's list at that moment, not the tabs' Claude Code state, which
  arrives later). `haetty` without a program opens no extra tab when
  anything came back; `haetty PROGRAM` still opens `PROGRAM`. Palette:
  "Attach detached sessions". Sessions of another open window
  (`attached` > 0) are never taken.
- Tab sets (`tabs/<name>.tsv` in the state directory, after ccx's
  `savetabs.sh`): per tab name, folder (OSC 7, else the start folder),
  program, arguments, Claude Code session id, active flag; never
  environment variables. `last` follows the open tabs (rewritten only when
  the content changes); at start it becomes `previous` and is reopened
  after the requested tab; `restore-tabs = false` (config or
  `--restore-tabs false`) turns that off; palette: save as `saved`, open
  `saved`, open `previous`. A seventh column `custom` (1: the person named
  the tab; files without it read as 0) keeps a renamed tab's name across
  restores; automatic names are stored for display only and recomputed.
  Closed tabs (2026-11-06, ccx's `closed.tsv`): a tab closed on purpose —
  Ctrl+Shift+W, an exited tab closed, the last tab — goes first into the
  set `closed` (20 at most, one entry per conversation); closing the window
  adds nothing (that is `last`). Palette "Reopen a closed tab" lists name,
  folder, program and "resume"; Enter opens it like a tab-set entry (the
  same rules: missing folder skipped, a running conversation never opened
  twice, `--resume` when the transcript exists) and removes it from the
  list. A conversation counts as running when a live session reports its id
  *or* was started with `--resume ID` (Claude Code reports its id a moment
  after starting; found by the stage S2 test reopening twice in a row).
- Tab names (first that applies): the name the person gave (Rename tab,
  Ctrl+Shift+R or the palette; up to 64 characters, control characters
  removed; empty puts the automatic name back; nothing is sent to the
  program and the text is not logged); Claude Code's session name;
  Claude Code tabs only: `repo:branch` of the tab's folder (`gitview.rs`,
  read from `.git/HEAD` — linked worktrees and submodules followed, a
  detached HEAD as 7 hex digits — no process, again when the folder changes
  or after 3 s); a restored name while the program has no title; the title
  without Claude Code's state glyph; the program. Decision: `repo:branch`
  only for Claude Code tabs — a shell's title (user, host, folder, the
  running command) says more than the branch.
- Detach (palette "Detach tab"): the active tab leaves the window, its
  session keeps running unattached in the daemon (not in `last`, not in
  `closed`); "Attach detached sessions" shows it again. The only tab is not
  detached (the window would close); a hint says so. Decision: restore *after* the requested tab
  rather than instead of it, so `haetty PROGRAM` always runs `PROGRAM` and
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

The product name, **haetty** (decided 2026-11-05; working name `ccterm`
before), lives in one constant module, `haetty_proto::product`: executable
name used when spawning the daemon and in hook commands (taken from
`current_exe()` at run time, falling back to the constant), app id
`io.github.dhwan0112.haetty` (Wayland `app_id`, X11 `WM_CLASS` instance and
class, `.desktop` file and hicolor icon name), socket, pipe, state and
config directory names (`haetty`), window title, the environment prefix
`HAETTY_`. Renaming also touches the `[[bin]]` names (`haetty`,
`haetty-headless`; Cargo needs literals) and `TERM_PROGRAM` / XTVERSION in
`haetty-core` (below this crate, so not taken from the module).

Former name: `product::env` reads `HAETTY_<NAME>` and falls back to
`CCTERM_<NAME>`, logging once per variable that the old name was used (also
for variables read before the logger existed). On start (GUI and daemon)
`migrate::run` copies the old config file and `tabs/*.tsv` when haetty has
none (originals stay; nothing is overwritten; skipped when the directories
are overridden) and checks the old socket/pipe: a running `ccterm daemon`
is reported in the log and a hint, never touched — the names do not
overlap, so both daemons run side by side. Checked by
`crates/haetty-gui/tests/migrate.rs` with the real binary.

Instances (added on request after P4): next to the name constants,
`product::instance()` reads `HAETTY_INSTANCE` (lower-case ASCII letters,
digits, `-`, ≤ 16; unset or empty = stable). A named instance gets its own
socket/pipe (`haetty-dev`), runtime, state and config directories
(`~/.local/state/haetty-dev`, …), app id (`io.github.dhwan0112.haetty.dev`,
so its desktop file and dock icon are separate) and window title prefix (`[dev] `). The daemon
sets `HAETTY_INSTANCE` in every session's child, so hooks of a dev session
report to the dev daemon. Purpose: run a development build beside the
stable one (self-hosting) without touching its sessions, tab sets or
settings. Checked by `crates/haetty-gui/tests/instances.rs` with two real
daemons.

### 3.10 Layout rules derived from §2

- The grid asked for (`--cols` × `--rows`, plus the title row) is what the
  first session gets, at the scale of the monitor the window is on, not the
  primary monitor's (A7, 2026-11-07). Wayland reports the scale only after
  the surface is shown on an output, so a Wayland window first shows a
  blank frame and opens its tabs on the first `ScaleFactorChanged`, or
  300 ms after its first frame when none comes: no PTY is sized from a
  guess, and Claude Code does not draw once at a wrong size. On every scale
  change — the first one, or a move to a monitor with another scale — the
  grid stays and the window asks for the pixels that grid needs at the new
  scale (winit's `inner_size_writer`). Before, the window kept its pixels:
  `--cols 80 --rows 24` gave 64x19 at 125 % (kramers, 2026-11-06).

- A tab whose program exits stays, dimmed with its exit status, until the
  user closes it; with one tab, the window closes when its program exits
  (as in milestone 1). So an event never removes a tab or the tab bar.
- The status line row is part of a Claude Code tab's layout from the first
  frame (integration on): it stays blank on the terminal's background until
  Claude Code reports something (context, limits, cost, changed files, or
  work in progress), so its arrival does not resize the terminal. Window
  size requests add that row, so the terminal keeps the requested rows.
- Shell (non-Claude Code) tabs have no status row: their program gets that
  row too. Decision on tab switching (2026-11-06): every tab keeps its own
  grid size for its whole life — shell tabs the full area, Claude Code tabs
  one row less — and switching tabs only changes which one is shown, never
  a size. So switching never resizes a session (a resize would make Claude
  Code redraw its whole screen) and a Claude Code tab is never reflowed by
  its status row appearing. Alternatives not taken: one size for all tabs
  (a shell tab would lose a row to a status line it never has) and
  reserving the row only once content arrives (one full Claude Code redraw
  per tab). Checked (Linux, Xvfb): a shell tab 70x11 beside a Claude Code
  tab 70x10; 20 switches → the same sizes, no resize.
- The diff indicator says "N files changed · Ctrl+Shift+D": plain Ctrl+D
  reaches Claude Code (and exits it on an empty prompt), so app shortcuts
  use Ctrl+Shift.
- `statusLine` is a single value: injecting haetty's replaces the user's own
  status line in haetty tabs (as in ccx). `haetty statusline` prints
  nothing, so Claude Code draws no line of its own; the information goes to
  haetty's status line instead.
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
| New tab | Ctrl+Shift+T | Claude Code: Ctrl+T (task list) only; distinct with the kitty protocol, legacy terminals cannot tell them apart but haetty consumes the chord before the program |
| Close tab (press twice) | Ctrl+Shift+W | Claude Code: Ctrl+W (delete word); same remark |
| Next / previous tab | Ctrl+Tab / Ctrl+Shift+Tab, Ctrl+PageDown / Ctrl+PageUp | free in Claude Code; Shift+Tab alone stays with the program |
| Go to tab N | Alt+1 … Alt+9 | free in Claude Code; bash/readline uses Alt+digit as a numeric argument (rarely typed) — rebind if needed |
| Command palette | Ctrl+Shift+P | free (Claude Code: Ctrl+P = previous history line) |
| Paste | Ctrl+Shift+V, Shift+Insert, Paste key; Windows also Ctrl+V | Linux plain Ctrl+V stays with Claude Code (image paste) |
| Diff pane / status panel (milestone 3) | Ctrl+Shift+D / Ctrl+Shift+S | free (Claude Code: Ctrl+D exit, Ctrl+S stash) |
| Rename tab | Ctrl+Shift+R | free (Claude Code: Ctrl+R history search) |
| Reopen a closed tab, detach tab, attach detached sessions | palette only | — (no chord: rarely used; detach is easy to mistake for close) |
| New task, message the worker, remove a task's worktree (§8) | palette only | — (no chord: each makes or sends something; a chord pressed by mistake must not) |
| In the diff view: next view / read again | m or Tab / r | the view has the keyboard while open; nothing reaches the program |

Linux: GNOME/KDE terminals use the same Ctrl+Shift family inside their own
windows only; haetty's window gets the keys first. Windows: Windows
Terminal uses Ctrl+Shift+T/W and Ctrl+Tab the same way; Alt+digit is free.
Every binding can be changed in the config file (`keybind = CHORD=ACTION`,
`CHORD=unbind`). Irreversible actions (close) ask for the same chord again
within 3 s, with a short hint over the bottom row; no dialog.

### 3.12 Icon and desktop integration

The icon is the user's design (`assets/icon/haetty.svg`, 256×256: a
window-shaped body #0A0A0B with transparent margins, an orange #FF6A2C
gradient bar on top and an orange glow fading into the black from the top
left, a grey #B4B2AF `❯` and underscore cursor). Its colours, shapes and
proportions are not changed by code. Everything derived from it is
generated once and committed (`assets/icon/README.md`: resvg 0.49 and
small Python scripts), so a build needs no conversion tool:
- `png/haetty-N.png` for N = 16, 20, 24, 32, 40, 48, 64, 128, 256, 512;
  16 and 24 px were checked magnified (`assets/icon/check/`) and are the
  plain renders (the shapes hold; no hand-tuned pixels);
- `haetty.ico` with 16, 20, 24, 32, 40, 48, 64, 256 (PNG entries);
- `rgba/haetty-64.rgba`, the 64 px image as raw RGBA.

Uses (`crates/haetty-gui/src/icon.rs`, `build.rs`):
- Windows: `build.rs` links `haetty.ico` as icon resource 1 (`windres`), so
  Explorer, shortcuts and the taskbar show it; the window's small and big
  icons are loaded from that resource at the system's small/large icon
  sizes, so Windows picks the matching image from the `.ico`.
- Linux X11: the window icon (`_NET_WM_ICON`) is the embedded 64 px RGBA
  (no PNG decoder at run time). Wayland has no window icon in winit; the
  dock uses the desktop entry.
- `haetty install-desktop` (Linux) writes
  `~/.local/share/applications/io.github.dhwan0112.haetty.desktop`
  (`Icon=` and `StartupWMClass=` = the app id) and the icon as
  `icons/hicolor/{16…512}x{16…512}/apps/io.github.dhwan0112.haetty.png`
  plus `scalable/apps/io.github.dhwan0112.haetty.svg`; on Windows it has
  nothing to install.
- Wayland `app_id` = X11 `WM_CLASS` (instance and class) = the app id, so
  docks match the window to the desktop entry.
- Title bar colours, Linux (checked in the sources, 2026-11-06):
  - GNOME on Wayland gives Wayland clients no server-side decorations, so
    winit draws its own: sctk-adwaita 0.10.1 (winit 0.30.13
    `platform_impl/linux/wayland/window/state.rs`, `into_sctk_adwaita_config`).
    winit passes only a theme — `Theme::Dark`/`Light`/`None` →
    `FrameConfig::dark()`/`light()`/`auto()` — and the colours are constants
    in sctk-adwaita (`theme.rs`: dark header bar `#303030` active, `#242424`
    inactive; light `#ebebeb` / `#fafafa`). haetty asks for dark or light
    from the theme background (`chrome.rs`), so the bar is dark, but its
    colour cannot be the terminal's `#141414`: winit has no call for
    colours.
  - GNOME on X11: mutter draws the frame; `_GTK_THEME_VARIANT=dark` (set by
    haetty) selects its dark variant; the colour is mutter's.
  - The way to a title bar in the terminal's colours on Linux is
    `decorations = integrated` (haetty draws the bar and its buttons, with
    the tabs in it), done 2026-11-06 (`titlebar.rs`).
- `decorations = auto | native | integrated | none`, default `auto`
  (`Decorations::effective`): `native` where the system can give its title
  bar the theme's colour — Windows 11 (DWM caption/border colours,
  `chrome.rs`) and X11 (the window manager draws it; mutter takes
  `_GTK_THEME_VARIANT=dark`) — and `integrated` where it cannot: Wayland,
  where winit draws an Adwaita frame with fixed colours (above). On Windows
  `integrated` is not implemented (snap layouts would need the caption hit
  test) and means `native`. The window system is winit's (`is_wayland`).
- Integrated, how it works: the window has no system decorations
  (`with_decorations(false)`); the title row is row 0 of the window grid
  (`top()` = 1 with any number of tabs, so a second tab no longer adds a
  row), and the window asks for one more row so the program keeps the
  requested size. One layout function (`titlebar::layout`) places tabs and
  buttons for drawing and for hit tests. Mouse: a press on empty title
  space starts `drag_window` (move), two within 400 ms and 4 px toggle
  maximize, a press on a tab selects it, a button acts on release over the
  same button; within 6 logical px (× scale) of a window edge or corner the
  cursor becomes a resize cursor and a press starts `drag_resize_window` —
  not while maximized; a right click does nothing (no window menu yet).
  Button layout: `HAETTY_TITLE_BUTTONS` (GNOME syntax) if set, else the
  settings portal via `dbus-send` (as winit's Adwaita frame reads its own
  settings), else `gsettings`, each ≤ 150 ms at window creation, else
  right-side minimize/maximize/close. Taken from GNOME: `appmenu`, `menu`,
  `spacer` entries are ignored.
- Windows: the GUI process sets its AppUserModelID to the app id
  (`io.github.dhwan0112.haetty`, a dev instance `….haetty.dev`) before any
  window exists, so the taskbar groups haetty windows and a pinned icon
  stays the same whatever folder the executable runs from.

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
component, isolated in `haetty-core`; keep `Snapshot` engine-neutral so an
alacritty_terminal backend can be added and A/B-tested later if the Zig
toolchain becomes a burden. Re-run the comparison on Windows once the GUI
exists.

## 5. Milestones

1. GUI, one window one session (`docs/history/2026-11-04-05-milestone-1.md`), with this document's theme,
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

### 6.1 Observe-only interface (first step, 2026-11-07; no network)

The local base the later adapters (CLI over SSH, MCP) stand on. Written with
the code (stage V3); the role table change is a protected-area change,
approved for this scope.

- **Consent per session, off by default.** A session is observable only
  after the person allowed it: palette "Observing: allow or stop for this
  tab", or `observe-new-tabs = true` for new tabs. The flag lives in the
  daemon (`SessionInfo.observable`, `SetObservable`, Control only), so it
  survives detach and reattach; tab sets do not store it (a restored tab
  is a new session and asks again).
- **Display.** One dim glyph `◦` after the tab number (`ui::tab_bar`), in
  the dim grey on every tab, active or not — never an accent colour
  (§2.2); no hint stays on screen. With nothing observable the screen is
  unchanged (the reference frames do not move).
- **What an observer sees.**
  - `GetScreen` (`haetty screen [--json] SESSION`): the visible screen
    text and the state — title, folder, program, size, cursor, and for
    Claude Code tabs the attention mark, busy, turns, name, model and the
    report's STATUS. Denied for a session that is not observable. The text
    travels as `Secret` (never logged).
  - `ReadEvents { cursor, follow }` (`haetty events [--follow] [--json]
    [--since CURSOR]`): `events.log` lines after the cursor, only events of
    a session written while it was observable. The daemon keeps per session
    the `seq` ranges of consent (from the "observe on" line to the "observe
    off" line). Nothing from before consent, after it was withdrawn, or from
    an earlier daemon run (those ranges are not kept across a restart; the
    sequence and the cursor continue). Daemon-wide lines (start, stop,
    friction) are never shown. With `follow` each new visible event is sent
    as it is written. The log holds derived fields only (§3.8): no typed
    text, prompts, tool input or screen contents.
- **Roles.** `ReadEvents` and `GetScreen` are Observe's alone (Control and
  Report are denied); `SetObservable` is Control's; every input or session
  message from Observe stays denied (§3.7, unchanged), and the permission
  rule for Program input is untouched.
- **Client type.** `haetty_proto::ObserverClient` is the observe-only
  client of §3.7's rule for remote adapters: it says `Hello` as Observe and
  can only build `ReadEvents`, `GetScreen` and `ListSessions`. It has no
  method taking a `ClientMsg`, no session handle and no input. A unit test
  checks its source for exactly those message names.
  `haetty events` / `haetty screen` use it. The older `haetty status
  --screen` (local scripts and tests) still reads any session through the
  full client's Observe `Attach`; adapters must not use it.
- Protocol version 8. Tests: `crates/haetty-gui/tests/observe.rs` (real
  daemon and CLI).

## 8. Orchestrator (milestone 4, first step)

(Numbered 8 as asked when it was commissioned; 7 is unused.) Written
2026-11-06 before the code (stage S3). Scope: one task → one worker; no
automatic decomposition, no automatic merging, no MCP / Rakazo (§6).

### 8.1 Task and worker

- A task: id (`t1`, `t2`, … per instance), title, description (one line
  in the first step), repository (the git top level), base branch. Created
  in the palette ("New task": title, description, then repository and base,
  prefilled from the active tab's folder and its current branch).
- haetty makes the worktree with git commands only (`gitview.rs`'s
  time-limited runner): `git worktree add -b task/<id>-<slug> <path>
  <base>`, path `<repository's parent>/<repository>-tasks/<id>-<slug>` —
  beside the repository, never inside it, so the repository's content and
  ignore files are not touched. Refused when the branch or the path exists,
  the base is not a commit, or the folder is not a repository.
- The worker is Claude Code in a new tab in that worktree. Program: config
  `worker-command` (default `claude`); arguments: none of haetty's beyond
  the task prompt — no permission flags, so the permission mode is what
  the person's own Claude Code settings say (bypass included or not).
- The task text reaches Claude Code as its positional prompt argument
  (`claude "<prompt>"`), not as typed input: Claude Code itself submits it
  after its own start-up dialogs (folder trust, the one-time bypass
  acceptance), which only the person answers. Hooks from settings can run
  before the trust dialog is answered (Claude Code permissions docs, "What
  runs before you trust a folder"), so a hook report is not a sign that
  input would be safe. The prompt is not stored as the tab's arguments: tab
  sets, the closed list and re-attach strip it, so restoring a worker
  resumes its conversation without submitting the task again.
- The prompt asks for ccx's worker report at the end of the final message
  (`agents/worker-*.md`): `STATUS: verified | blocked | needs_review`,
  `ARTIFACT:`, `EVIDENCE:` (each check actually run), `BLOCKERS:`,
  `NEXT_ACTION:`.
- Tasks are files `tasks/<id>.task` in the state directory (key = value,
  escaped); the worker tab is the tab whose folder is the task's worktree.
- Cleanup only on the person's command (palette "Remove a task's
  worktree"): `git worktree remove <path>` without `--force` (a worktree
  with changes or untracked files is refused, with git's reason), then the
  task file goes; the branch stays (its commits are the result). Never
  `git branch -D`.

### 8.2 Worker reports and states

- Source: the Stop hook's `last_assistant_message` (Claude Code hooks
  docs: the transcript "may not yet include the current turn's most recent
  messages when a hook fires"), parsed in the daemon (`haetty-cc`, not in
  the hook process: `report.rs`'s rules are unchanged). Fallback when that
  field is missing: the transcript (`transcript_path` only if it is a
  `.jsonl` file in a `projects` folder; its last 256 KiB), the last
  main-chain (`isSidechain` false) assistant text whose timestamp is not
  before the turn's UserPromptSubmit. The report is cleared at
  UserPromptSubmit, so it always belongs to the latest turn.
- Grammar (ccx's `hook.sh`): `STATUS` on its own line, markdown marks
  (`#`, `*`, `>`, `-`, spaces) allowed around the key, value one of
  `verified`, `blocked`, `needs_review` (any case); the last such line
  counts. The other four keys take the rest of their line and the lines up
  to the next key, at most 2 000 characters each.
- States, never guessed:
  - running — the worker is between UserPromptSubmit and Stop (`busy`);
  - verified — STATUS verified with an EVIDENCE that is not empty or
    "none" (verified without evidence is needs review: it cannot be
    checked);
  - blocked, needs review — as reported;
  - no report — anything else: no turn finished yet, a final message
    without a valid STATUS, the program exited without one.
- Report texts are carried as `Secret` (never in logs, debug output or
  the event log); the event log gets the state only.
- Wire: `ClaudeInfo` and `SessionInfo` get `turns` (Stops seen), `report`
  and (`SessionInfo`) `busy`; protocol version 7.

### 8.3 Display

- Worker tab name: a state glyph and the task title (an own name replaces
  the title, not the glyph): `…` running, `✓` verified, `✗` blocked, `◇`
  needs review, `–` no report. Grey like the rest of the name: the existing
  attention marks (amber "turn finished", red `?` question) already call
  the person; a second colour channel would compete with them (§2.2).
- Status panel: a WORKERS section — one line per task (glyph, id, title,
  state) and, for the active worker, its BLOCKERS / NEXT_ACTION first
  lines.
- `haetty status --json`: `workers`, one object per task (id, title,
  repository, base, branch, worktree, session, state and the report's five
  fields).

### 8.4 Safety boundary (protected area, approved for this scope)

- Everything the orchestrator sends to a worker is `Origin::Program`;
  the function that sends (`haetty_cc::orch::send`) has no origin
  parameter, and the daemon refuses Program input while the session waits
  for a permission answer (§3.7, unchanged).
- The orchestrator sends only when the worker is ready: at least one turn
  finished (`turns` > 0: past every start-up dialog), not busy (no tool
  call can be asking), no question waiting. Otherwise it holds and says
  why. It sends a message as one paste plus one Enter, never key by key.
- The orchestrator never answers permission prompts or dialogs, never
  edits Claude Code's settings or trust records, and adds no permission
  flags; bypass follows the person's settings.
- A cargo integration test (`tests/orchestrator.rs`, real daemon, a fake
  `claude` recording every byte it reads) checks: before any turn → held,
  0 bytes; a permission question → held and a raw Program paste refused, 0
  bytes; after the answer and a Stop → delivered; busy → held; and that
  the orchestrator source contains no `Origin::Human`.

## 9. Updating the stable instance without closing the tabs (design only, 2026-11-07)

Written in stage V4; nothing here is implemented. Today a new stable
version runs only after every stable window is closed and its daemon has
exited (§3.15). The sessions live in that daemon, so updating means ending
every program, Claude Code turns in progress included. Once haetty is
developed inside haetty, that becomes the main cost of an update.

What has to survive an update, per session: the program and its children
(a Claude Code turn may be mid-tool-call), the PTY, the screen and
scrollback, the Claude Code state (`ClaudeInfo`, attention, the injected
`--settings` file whose hook command names the old binary), the tab's place
in the window, and the observe consent (§6.1).

### 9.1 (a) Old and new daemon side by side

The old daemon keeps its sessions until they end; the new one takes new
sessions; the GUI shows the tabs of both.
- Needs per-version endpoints (`haetty/daemon-<protocol>.sock`, the pipe
  likewise): today both would want the same name. The hooks of old
  sessions run the old binary (its path is in their `--settings`), which
  dials its own version's endpoint, so old sessions keep reporting to the
  old daemon.
- The GUI must speak both protocol versions. postcard decodes enum
  variants by index and cannot skip unknown ones (§3.6), so a new GUI needs
  the old version's message types side by side — a frozen copy of
  `wire.rs` per supported old version — or a second, old GUI process for
  the old tabs, which is no longer one window.
- Risks: two daemons' state (tab sets, `last`, the closed list, tasks)
  written by both; `haetty status` and observers must ask both; the old
  daemon's bugs stay alive as long as its longest session; protocol
  compatibility becomes a permanent obligation.
- Size: large (versioned endpoints, a multi-version client, GUI routing
  per tab, status and observer merging). Protected areas: the endpoint and
  role code is untouched and the injected settings do not change.
  Moderate risk of subtle bugs, permanent cost.

### 9.2 (b) Hand the sessions to the new daemon

The new daemon starts on a temporary endpoint and the old one passes it
every session. The new daemon then takes the main endpoint, the old one
exits, and the GUI reconnects (it already survives a lost daemon:
`Client::is_lost`).
- **Linux: file descriptors.** The PTY master goes over the Unix socket
  with `SCM_RIGHTS` (plain `sendmsg`, no new dependency; `libc` is already
  used). The child stays where it is. It is not the new daemon's child, so
  `waitpid` cannot report its exit status: the new daemon watches it with
  `pidfd_open` + `poll` (Linux 5.3+) and knows *that* it ended, not the
  code. The old daemon must not exit before the handover. When it exits,
  its children are reparented to init and keep running (the PTY master is
  held by the new daemon), but exit codes of handed-over sessions become
  "unknown".
- **Windows: ConPTY handles.** A pseudo console is three handles inside
  the HPCON: the OpenConsole process, the "reference" handle (keeps the
  ConDrv server open) and the signal pipe. The PTY input and output pipes
  and the client process handle come on top.
  - Checked in Microsoft's source (`src/winconpty/winconpty.cpp`,
    microsoft/terminal; the vendored `conpty.dll` 1.25 exports these
    names):
    - `ConptyPackPseudoConsole(hProcess, hRef, hSignal, &hpc)` only packs
      such loose handles into a new HPCON ("for an inbound ConPTY session
      … so resizing and closing just work"; Windows Terminal's
      default-terminal handoff uses it);
    - `ConptyReparentPseudoConsole(hpc, hwnd)` only writes
      `PTY_SIGNAL_REPARENT_WINDOW` with a new owner HWND for the pseudo
      window (what `GetConsoleWindow()` returns, GH#2988). It moves nothing
      between processes.
  - So the handover is: the old daemon `DuplicateHandle`s the five handles
    into the new daemon's process, and the new daemon calls
    `ConptyPackPseudoConsole` and goes on with resize and close as usual.
    This needs the side-loaded `conpty.dll` (the in-box `kernel32` has no
    Pack), which haetty ships already. Reparent is not needed (haetty's
    pseudo windows have no owner window today).
  - The client process handle gives the new daemon the exit code
    (`GetExitCodeProcess` works on any handle with query rights): unlike
    Linux, nothing is lost.
- **The screen state is the hard part.** The PTY carries no history: the
  terminal state (grid, scrollback, modes, cursor, charsets, tab stops,
  palette changes, kitty keyboard flags) lives only in the old daemon's
  engine. Options:
  1. **Serialize and replay.** libghostty-vt 0.2.2 has a formatter
     (`Formatter`, `FormatterOptions`: VT format, with palette, modes,
     scrolling region, tab stops, pwd, keyboard modes, cursor, style,
     hyperlinks, protection, kitty keyboard, charsets). The old daemon
     formats each session as VT bytes; the new daemon feeds them to a fresh
     engine before the PTY's next bytes.
     - To verify before relying on it: whether it includes the scrollback
       and both screens (primary and alternate).
     - Ordering, so nothing is lost: the old daemon stops reading the PTY
       first and formats second; the PTY buffers in the meantime.
     - The native engine (§10, planned) should have an exact state
       snapshot by design.
  2. **Ask for a redraw.** A resize (SIGWINCH / ConPTY resize) makes
     Claude Code's fullscreen renderer draw its whole screen again. That
     covers the visible screen of full-screen programs, not a shell's
     scrollback or a program that does not redraw.
  3. Both: replay, then a resize as a safety net for full-screen programs.
- Claude Code state: `ClaudeInfo` goes over as a message (plain data). The
  `--settings` file stays where it is. Its hook command still names the old
  binary; that binary stays (§3.15 keeps old versions, read-only), and its
  hooks reach the new daemon only while the endpoint name and the
  `Hook`/`StatusLine` messages stay compatible — or the new daemon also
  accepts the old protocol for Report clients only.
- Risks:
  - The handover window: both daemons alive, one session in neither. It
    must be short and atomic per session; a crash halfway must leave each
    session in exactly one daemon (one session at a time, acknowledged,
    rolled back on failure).
  - Lost exit codes on Linux.
  - The formatter's fidelity is not known yet.
- Size: medium to large (a handover protocol between daemons, fd and
  handle passing, replay, GUI reconnect, tests that update with sessions
  running, on both platforms).
- Protected areas: the handover is a new message family between two
  daemons of the same user. It needs its own role (a "Handover" client
  that may only send sessions, never input) — a change of the IPC role
  checks, to be reviewed. The permission-prompt path must survive intact:
  a session waiting for a permission answer keeps waiting, and only Human
  input answers it, in the new daemon as in the old.

### 9.3 (c) Close and resume, made smooth

Keep today's model, but make the update one action and lose as little as
possible.
- Palette "Update to the installed version". The GUI waits until no
  Claude Code tab is busy or asking (it knows `busy` and `attention`
  already). It then saves the tab set (`last`) with each tab's Claude Code
  session id, ends the sessions, and starts the new binary, which reopens
  the tabs. Claude Code tabs resume their conversations (`--resume`,
  already there); shells start again in their folders.
- What is lost: running shell programs and their scrollback, and a
  conversation's in-flight turn. Waiting for idle prevents the latter; a
  tab that never goes idle can be named and skipped.
- Optional: keep each tab's last screen as text (from the snapshot) and
  show it dimmed above the new prompt, so the person keeps the context.
- Risks: low; everything it uses exists. Size: small (one action, the
  wait, the restart). Protected areas: none (no input to sessions, no role
  change; `--resume` is the existing path).

### 9.4 Recommendation

1. **Now: (c).** Small, no protected area, and it removes most of the
   pain: updates happen when Claude Code is idle, and conversations come
   back.
2. **Later: (b), after the native engine (§10)** gives an exact state
   snapshot. On Windows, Pack and `DuplicateHandle` make the handover
   possible with what haetty ships; on Linux it costs only the exit codes
   of handed-over sessions. Before relying on the libghostty formatter,
   measure its fidelity with the differential tool of §10 (format, replay,
   compare cell by cell).
3. **Not (a).** Speaking several protocol versions for good costs more than
   a one-time handover, and old daemons would keep old bugs alive.

## 10. A terminal engine in Rust (design, stage E0, 2026-11-07)

Goal: an engine of our own in Rust beside libghostty-vt. A build with only
that engine needs no Zig, builds on Windows directly, and is all Rust.
**The default engine stays libghostty-vt** until the differential tests
(§10.5) show no difference from it. Stages: E0 this design, E1 the
differential tool (before the engine), E2 the core engine, E3 encoders and
the rest.

### 10.1 What haetty uses of libghostty-vt today (from the code)

Every use, from `crates/haetty-core` (no other crate calls libghostty-vt;
the GUI and the wire see its key and mouse types through `haetty_core`):

| Area | Where | What |
|---|---|---|
| Parser and screen operations | `engine.rs:341` `vt_write` | all of it: C0/C1, ESC, CSI, OSC, DCS, the screen, cursor, erase, insert and delete, scroll regions (DECSTBM, DECSLRM), origin mode, tab stops, charsets, the alternate screen, saved cursor, wrap state |
| Modes read back | `engine.rs:396, 402, 454, 470, 488, 758–766` | bracketed paste, focus events, colour-scheme reports (2031), synchronized output (2026), alternate screen (47/1047/1049), mouse tracking (any), SGR mouse, DECCKM, cursor visible, kitty keyboard flags |
| Replies to queries | `engine.rs:554–628` (callbacks) and libghostty itself | DA1 (VT220; 132 columns, selective erase, ANSI colour, clipboard), DA2 (VT220, firmware 1), DA3, XTVERSION (`haetty X.Y.Z`), DSR (5, 6), DECRQM, size reports (CSI 14/16/18 t, in-band mode 2048), colour scheme (CSI ? 996 n), OSC 4/10/11/12 queries, kitty keyboard query (CSI ? u) |
| Events | `engine.rs:562–590` | bell, title (OSC 0/2), working directory (OSC 7), clipboard write (OSC 52) |
| Not libghostty | `osc.rs`, `engine.rs:467–490`, the daemon | OSC 9 / 777 / 99 notifications and OSC 9;4 progress (our scanner), the synchronized-output hold timing, frame diffs per row (`haetty-daemon`, `frame_between`) |
| Colours | `engine.rs:532–544`, `theme.rs:97, 121, 149` | default fg/bg, palette 0–15, cursor colour; colour parsing (`#rgb`, `rgb:`, X11 names), xterm's 256-colour palette, `N=COLOR` |
| Scrollback and viewport | `engine.rs:203, 362, 391, 399, 431, 768` | `max_scrollback` in **bytes**, viewport top/bottom/delta, scrollbar (total, offset, length) |
| Resize | `engine.rs:224, 418` | cells and cell pixels; soft-wrapped lines reflow |
| Render state | `engine.rs:752–863` | per cell: grapheme text, resolved fg/bg, bold, italic, faint, blink, inverse, invisible, strikethrough, overline, underline (any kind), selected, wide/spacer. Cursor: visible, position, style (block, bar, underline, hollow), blinking. Title, pwd. The whole grid is copied each time: libghostty's per-row dirty state is **not** used (the daemon diffs rows). |
| Wide characters and graphemes | inside libghostty | East Asian width, emoji presentation, VS15/VS16 (wide only with mode 2027, §4 pitfalls), grapheme clustering (mode 2027) |
| Encoders | `engine.rs:663–750, 402, 457` | keys (`key::Encoder` from the terminal's modes: legacy, modifyOtherKeys, kitty flags), mouse (`mouse::Encoder`: X10, normal, button, any-motion; SGR and SGR-pixels; pixel geometry), paste (bracketed or not, unsafe bytes), focus in/out, colour-scheme report |
| Formatter | `engine.rs:866` | `dump` as plain text, VT or HTML (`haetty-headless`; DESIGN §9 handover candidate) |
| Selection | `engine.rs:843` | only `is_selected` is read; haetty never sets a selection (its own selection is a known gap, HANDOFF §6.2). |
| Hyperlinks | inside libghostty | OSC 8 parsed; nothing reaches the snapshot |
| Types | `types.rs:5–6, 46` | `Key` (about 180 variants, travelling as `u32`), `Mods`, `KittyKeyFlags`, key and mouse `Action`, mouse `Button`, `RgbColor` |

### 10.2 Build it, or take a small crate

Sizes are stripped `dist` builds of a small program using the crate, minus
the same program without it (measured 2026-11-07; Linux / Windows):

| Part | Decision | Size | Why |
|---|---|---|---|
| Parser (DEC/ANSI state machine) | **`vte` 0.15** | +9.5 / +8.7 KB (arrayvec, memchr) | Paul Williams' state machine, as Ghostty's; mature (Alacritty); `no_std`-able. Its limits (parameter count, OSC length) are checked against libghostty's by E1. |
| Screen, cursor, modes, scroll regions, tabs, charsets, alt screen | own | — | the engine itself; no small crate fits our snapshot and byte budget |
| Cell width | **own table, generated from libghostty** | ~10–15 KB (estimate, as unicode-width's tables) | `unicode-width` 0.2 (+13.5 / +12.8 KB) follows UAX 11 as the Rust ecosystem reads it; Ghostty has its own table (emoji presentation, private use, ambiguous). A dev-only generator in the E1 crate asks libghostty for the width of every code point and writes range tables, so widths match by construction; a test lists where `unicode-width` would differ. |
| Grapheme clusters (mode 2027) | **`unicode-segmentation`** to start | +29.5 / +28.7 KB | UAX 29 extended clusters; E1 compares it with libghostty. If they differ (Unicode version, Ghostty's own rules), own break tables generated like the width table. |
| Key, mouse, paste, focus encoders | own | — | small, protocol rules (xterm, kitty); compared byte for byte in E3 |
| Colour parsing, palette | own | X11 names ~10 KB | small; the xterm 256-colour formula; X11 names as a sorted table |
| Formatter | own, later | — | plain text first (`dump`); VT format when §9 needs it |

Budget: libghostty-vt's archive in the Linux `dist` build holds 1.01 MB of
code and 69 KB of data (`size -t`, ReleaseSmall, simdutf and highway
included); the native engine should stay well under that.

### 10.3 Memory

- Baseline (§3.4, libghostty): 11.6–11.8 MB per session with the default
  12 000 000-byte scrollback filled, ~1.1 KB per 120-column row; GUI +
  daemon with one filled session 19.2 MB.
- Target: the same scrollback limit costs no more than with libghostty.
- Cells: 8 bytes each (code point 21 bits, width 2 bits, style id 16 bits,
  flags), as Ghostty packs them. Styles in a per-screen table with
  reference counts. Grapheme extras (combining marks, ZWJ sequences) in a
  side map keyed by cell. Rows: a header (wrap, dirty, length) and their
  cells.
- **`scrollback-limit` stays bytes**: the engine counts what scrollback
  rows hold — 8 bytes per cell kept, the row header, grapheme extras, the
  share of styles — and drops the oldest rows beyond the limit.
  libghostty counts whole pages, so at the same limit the two keep
  different numbers of rows. That is a known difference of accounting, not
  of terminal behaviour: E1 compares scrollback with a limit neither
  reaches, and eviction is tested on its own.
- Trailing blank cells of scrollback rows are not stored (a row keeps its
  used length). Typical Claude Code output then needs less than ~1.1 KB per
  row, so the same bytes keep more history.

### 10.4 The engine boundary

- `haetty-core` gets a synchronous engine trait — feed bytes, take the
  replies and events, resize, snapshot, modes, scroll the viewport, set
  colours and the colour scheme, encode key / mouse / paste / focus, dump —
  with two implementations: `GhosttyEngine` (today's `engine.rs`, behind
  it) and `NativeEngine` (new crate `haetty-vt`, pure Rust). The engine
  thread, the PTY writer queue, the sync-output timing, the OSC scanner and
  the `Snapshot` format stay as they are, around the trait. E1's tool
  drives the same trait without threads.
- The key and mouse types become `haetty-core`'s own, **with libghostty's
  numbering** (the wire carries them as `u32`, §3.6); the ghostty engine
  converts. Then no other crate names libghostty.
- Cargo features of `haetty-core` (and passed through by the binaries):
  `ghostty` (default) and `native`. A build with `--no-default-features
  --features native` has no `libghostty-vt` in its dependency tree, so no
  Zig. The gate gets a step that builds it for both targets with `zig`
  taken off `PATH`. E3 also builds it on Windows itself (`cargo` on Windows,
  no WSL).
- Choice: the setting `engine = ghostty | native` (default `ghostty`) per
  **session**. It travels in `CreateSpec` (protocol 9), so one window can
  hold both for comparison, and an engine that is not compiled in is
  refused with a hint. Decided per session rather than per daemon: the
  daemon reads no config file, and A/B in one window is what E1–E3 need.

### 10.5 Differential tests (E1)

- A dev-only crate `haetty-vtdiff` (`publish = false`), the only place both
  engines are linked together, so the shipped native build never contains
  libghostty. Zig is needed only to build this crate.
- It feeds the same bytes to both engines.
  - Compares at fixed chunk boundaries, at randomly chosen split points (a
    parser state at a chunk edge), and at the end.
  - Compared per cell: text, width, fg, bg, flags; plus cursor (position,
    shape, visible), modes, kitty flags, title, pwd, scrollback line count,
    and the reply bytes.
  - Reports the first difference: input offset, row and column, field, both
    values, the last bytes fed.
- Inputs:
  - the recorded Claude Code corpus (`~/m4/vtcorpus`, `scripts/e2e/README.md`);
  - synthetic load;
  - vttest-like scenarios aimed at hidden state — scroll regions, insert and
    delete, the pending-wrap state at the last column, origin mode, tab
    stops, charsets, saved cursor, the alternate screen;
  - scenarios with resizes;
  - fuzz input (the vtfuzz generators).
- A report: match rate per category, and every remaining difference sorted
  into ours (a bug), interpretation (with the xterm ctlseqs / ECMA-48 / DEC
  STD 070 reference that supports each reading) or undecided.

### 10.6 The core engine (E2, 2026-11-07)

- `crates/haetty-vt` has the following:
  - screen grid and cursor with pending wrap;
  - scroll regions and origin mode;
  - primary and alternate screens (47, 1047, 1049), scrollback;
  - wide characters (a spacer in the last column when one does not fit);
  - combining marks and variation selectors;
  - SGR with the theme palette, OSC 4/10/11/12 and 104/110–112;
  - tab stops, G0–G3 with DEC special graphics (SI/SO, LS2/LS3, SS2/SS3);
  - the modes in `Modes`, including kitty flags per screen;
  - title, pwd, OSC 52 and the bell as events;
  - per-row change flags (`take_dirty`).
- It answers these queries, byte for byte as libghostty does:
  - DA1/2/3, DECID, DSR 5/6, XTVERSION;
  - DECRQM for private modes;
  - CSI 14/16/18 t, mode 2048 size reports;
  - OSC 4/10/11/12 queries (ending with BEL or ST, as asked);
  - kitty `CSI ? u`, `CSI ? 996 n`;
  - the kitty graphics support query (`ESC _ G a=q,i=N`).
- Widths come from `width_table.rs`, generated by
  `haetty-vtdiff --gen-width` from libghostty itself:
  - 375 zero-width ranges;
  - 126 wide ranges;
  - 185 code points a variation selector may follow (after any other, it
    is dropped).
- Behaviours taken over from libghostty, each found by `vtdiff --bisect`
  and checked with `--probe`:
  - Parameters are counted as libghostty counts them: `CSI 5;B` has one
    parameter, `CSI ;B` one, `CSI B` none. A sequence with more
    parameters than it takes is ignored whole (CUU…CUP, ED, EL, IL, DL,
    DCH, SU, SD, ECH, CBT, ICH, HPA/HPR/HPB, REP, VPA/VPR/VPB, TBC, DSR,
    XTWINOPS, DECSCUSR, DECRQM, DECSTBM).
  - An explicit 0 does nothing for IL, DL, DCH, SU, SD, CHT and CBT
    (elsewhere it means 1).
  - TBC needs its parameter (`CSI g` does nothing).
  - `CSI s` saves the cursor only without parameters (with them it would
    be DECSLRM).
  - A pending wrap survives SU, SD, SGR, HT, CBT, ED 3, unknown ED modes,
    DECSC/DECRC, and RI when it scrolls. It is cleared by:
    - CR, LF, BS (which also moves left);
    - cursor moves;
    - EL, ED 0/1/2, ECH, DCH, ICH;
    - RI when it moves up.
  - CUD from above the region's bottom stops at that bottom; CUU from
    below its top stops at that top.
  - SGR 38/48/58 with an unknown kind or too few values drops only the
    38, and the parameters after it count alone. Colour values wrap to a
    byte.
  - RIS clears the scrollback. ED 22 moves the rows down to the last used
    one into the scrollback, then clears the screen.
- Results (`vtdiff --fuzz 5000`, the corpus, 3 synthetic loads, 16
  scenarios, 2 resize scenarios, each cut two ways): 10 045 of 10 046 runs
  identical. The corpus has 0 differences. The random-input test in
  `haetty-vt` (300 rounds of random bytes, resizes down to 1×1, viewport
  scrolls) does not panic.
- One remaining difference, a libghostty bug the native engine does not
  copy. `CSI 0 g` on a column without a tab stop *sets* one in
  libghostty: `Tabstops.unset` toggles the bit (`^=`) instead of clearing
  it. ECMA-48 §8.3.154 (TBC, Ps = 0: "the character tabulation stop at the
  active presentation position is cleared") and xterm's ctlseqs ("Ps = 0
  ⇒ Clear Current Column") agree that it clears. Classified as a
  libghostty interpretation difference against the specification.
- E3 still has to do the following:
  - encoders (keys, mouse, paste, focus);
  - the byte-limited scrollback and the 8-byte cell layout (§10.3);
  - selection;
  - grapheme clustering under mode 2027.

### 10.7 E3: a wider harness, the engine as an option, encoders, memory (2026-11-07)

- vtdiff got wider before the engine went further:
  - the fuzz test's generators (escape-sequence grammar, Claude-Code-like
    stream, mutations with corpus bytes spliced in;
    `haetty-core/tests/common/vtgen.rs`, shared);
  - a second fuzz mix (private modes, non-ASCII text, SGR colon forms, OSC
    colours, DCS/APC, kitty keys, resizes inside the stream);
  - the scrollback contents compared page by page at the end of each run;
  - `--bisect` minimises inputs with resizes; `--probe` takes
    `<resize CxR>` and `VTDIFF_SPLIT`; `--dump`; `--encoders`;
    `--gen-keys`.
- The reference is read through a fresh render state
  (`GhosttyEngine::snapshot_fresh`). The incremental one misses a row
  whose only change is a combining mark, VS or ZWJ joining a cell already
  drawn: **libghostty does not mark that row dirty** (finding 3 below).
- Behaviours taken over from libghostty, found by vtdiff and checked
  against its source:
  - parsing:
    - C1 bytes inside escape, CSI, DCS and SOS/PM/APC act as their 7-bit
      forms; a raw C1 in ground state is U+FFFD, a UTF-8-encoded one is
      printed;
    - C0 bytes 0x10–0x1F print as cells in ground state;
    - a CSI with a colon outside SGR is void;
    - an APC ends at the ESC of its terminator, or at a C1 byte (and is
      dispatched); an OSC that is not valid UTF-8 is dropped;
  - SGR from the raw parameter bytes:
    - an empty field differs from 0 (`48:5:` is void, `38:2::R:G:B` is
      not);
    - sub-parameters void anything but `4:N` and the colours;
    - a single value left over after `38:5:N` counts as its own SGR;
  - resize as `PageList.resize`:
    - rows before columns when narrowing, after when widening;
    - fewer rows trim blank rows at the bottom (not the cursor's);
    - more rows pull the scrollback down only with the cursor on the last
      row;
    - reflow drops trailing empty cells of hard line ends, writes blank
      lines only before content, and keeps the rows below the cursor;
    - a cursor (or saved cursor) that ends in the scrollback goes to the
      top left;
    - no reflow while autowrap is off; the alternate screen never
      reflows;
    - a resize ends synchronized output;
  - wrap state:
    - IL, DL, SD and RI at the top (and SU on a region not from the
      first row) clear the soft wrap and the wrap-continuation of shifted
      rows, and turn their trailing spacer heads into plain cells;
    - DECALN clears every row's wrap, resets the region and the pen's
      attributes, and fills in the pen's colours;
    - overwriting a wide character in column 0 drops the spacer head of
      the row above;
    - the other half of an overwritten wide character is cleared in the
      pen's background;
  - mode 2027 grapheme clusters:
    - uucode's break rules (UAX #29 with GB9c and the emoji-modifier
      tailoring) on the Unicode 17 classes libghostty builds with
      (`grapheme_table.rs` from `tools/gen-grapheme-table.py`);
    - VS16 widens and VS15 narrows a cluster;
    - a part with width of its own widens it, wrapping to the next row
      when it no longer fits;
    - a zero-width character that starts a cluster is dropped;
  - screens and cursor:
    - the cursor shape goes with the cursor between screens but not
      through DECSC;
    - `?1049l` restores the cursor even off the alternate screen;
    - `?1049h` on it saves and erases;
    - leaving 47 or 1047 keeps the alternate screen's cursor;
    - DECRQM reports 47, 1047 and 1049 apart;
  - the rest:
    - DECRQM knows 3, 4, 8, 40, 45, 67, 1007, 1035, 1036, 1039, 1045 and
      1048, and needs a parameter;
    - DECSCUSR 0 is a steady block;
    - REP repeats the last printed character, as written, through the
      current charset;
    - characters above U+00FF print as a space under DEC special
      graphics;
    - kitty flags above 31 void a push or set, and popping 0 pops none;
    - HPR and VPR with an explicit 0 do nothing;
    - character protection (SPA/EPA ISO, DECSCA DEC) and the erasures
      that respect it;
    - colour specifications as `RGB.parse` (components truncated; `rgbi:`;
      9- and 12-digit hex; X11 names from `tools/gen-x11-colors.py`).
- Our bugs found on the way:
  - the reflow lost the cursor's line when there was scrollback;
  - spacer tails were trimmed as blanks;
  - RIS reset the byte scanner mid-input (E2);
  - the ghostty adapter reported `alt_screen` from mode flags that can
    stay set after 47/1047 leave, not from the screen shown (fixed:
    `active_screen`).
- **libghostty findings** (not copied; for the person to decide):
  1. TBC 0 toggles a tab stop (E2, §10.6).
  2. CHA, HPA and HPR under origin mode move the cursor down by the
     region's top. `setCursorPos` adds the origin offset to the row too.
     ECMA-48 §8.3.9 (CHA: "to character position n *in the active
     line*") and xterm (CHA, HPA move only horizontally) disagree. The
     native engine moves only horizontally.
  3. The render state misses rows changed only by a grapheme joining a
     drawn cell. This affects haetty today: a VS16, ZWJ or combining mark
     that arrives in a later PTY read than its base shows only when the
     row changes again. A fix on our side, outside E3 (the default engine
     is not to change): snapshot from a fresh render state when such a
     character was fed, or always (one extra copy of the visible rows).
  4. Mouse motion is never deduplicated in haetty's use: the adapter sets
     the encoder's options and size before each event, and libghostty
     clears its last cell on both. The native engine matches. Whether the
     GUI should deduplicate is a separate question.
- The engine is an option:
  - haetty-core owns its key and mouse types (`keys.rs`, from
    `vtdiff --gen-keys`, libghostty's numbering; a test round-trips every
    variant) and its colour parsing, so libghostty-vt is an optional
    dependency;
  - feature `ghostty` is the default, forwarded by every crate, and the
    workspace declares the haetty crates without default features;
  - `--no-default-features` builds with no libghostty in the graph and no
    Zig on PATH; the gate checks it ("build native-only (no zig)");
  - on Windows itself (MSVC, Rust 1.93.1, no Zig) it builds in 77 s and
    haetty-headless runs a session on the native engine;
  - `engine = ghostty | native` in the config file, `CreateSpec.engine`
    (protocol 9), `haetty-headless --engine`; a build without libghostty
    runs native with a warning.
- Colour parsing (themes, OSC) equals libghostty's: `vtdiff --colors`
  compares 3219 specifications and 97 palette entries (X11 names in
  several spellings, every hex/`rgb:`/`rgbi:` form, junk, every line of
  the built-in themes): all identical.
- Encoders (`haetty-core/src/encode.rs`): ports of libghostty's key
  (legacy, modifyOtherKeys, kitty), mouse, paste and focus encoders. `vtdiff
  --encoders` compares, byte for byte:
  - every key × 16 modifier sets × press/repeat/release × with and
    without text, under 6 legacy mode sets and all 31 kitty flag sets
    (397 824 cases);
  - mouse: 5 tracking modes × 5 formats × 11 buttons × 5 modifier sets ×
    6 positions (beyond 223 and 2015 columns, outside the window) × a
    press/motion/release sequence, at two widths (82 500 cases);
  - paste (bracketed or not, control bytes, an embedded `ESC [201~`) and
    focus.
  All 480 342 cases are identical.
- Memory (§10.3):
  - a cell is 16 bytes and `Copy`: a `u32` with the character or an index
    into a table of grapheme clusters (each kept once), the style inline,
    the width, the protected bit;
  - the scrollback keeps `scrollback-limit` bytes (a line costs its cells
    and its header);
  - a hard line end goes in without trailing blanks, so short lines cost
    little; the oldest lines leave first.
  This is twice libghostty's 8-byte cell for a full row and less for
  Claude Code's output. 8-byte cells (styles in a table) remain an option
  if the measurements call for it.
- Not in the native engine:
  - kitty graphics beyond the support query (transmit, display, delete:
    libghostty answers them; Claude Code sends only the query);
  - left/right margins (DECLRMM is kept for DECRQM, DECSLRM only homes the
    cursor; fuzz2 leaves mode 69 out);
  - the behaviour of the DECRQM-only modes (DECCOLM, reverse wrap…);
  - `dump` in VT and HTML form (`haetty-headless --dump vt|html`): the
    native engine gives plain text for all three.
- Selection needs nothing from the engine: haetty never sets one in
  libghostty (§10.1; its own selection is a known gap, HANDOFF §6.2).
- Results (`docs/vtdiff/2026-11-07-e3.md`, `--fuzz 200` per kind):
  - 1326 of 1648 runs identical (80.5 %); the corpus has 0 differences;
  - by kind: fuzz 400/400, mutated 394/400, fuzz2 317/400, grammar
    167/400 (random bytes, broken UTF-8), scenarios, resizes and
    synthetic loads all identical;
  - field match rates over checkpoints: 95 % or more each; scrollback
    contents 93 % of runs.
- What still differs, from minimal inputs of 30 differing runs:
  - libghostty findings 1 and 2 (TBC 0, CHA under origin mode);
  - undecided, needing more work on our side:
    - long runs of broken UTF-8 next to a wide character at the last
      column (vte's and libghostty's decoders split invalid sequences
      differently there);
    - SD with large counts followed by a resize;
    - kitty graphics transmit commands;
    - left/right margins (grammar sets mode 69).
- Measurements beside today's (Linux, `dist`, commit 2e2d572,
  `measure.sh`, `ENGINE=native` for the second column):

  | | libghostty (today) | native |
  |---|---|---|
  | key → frame, median / p95 | 0.99 / 1.25 ms | 0.78 / 0.94 ms |
  | throughput, plain lines | 24 MiB/s | 18 MiB/s |
  | throughput, long wrapped lines | 44 MiB/s | 31 MiB/s |
  | throughput, SGR-heavy | 53 MiB/s | 33 MiB/s |
  | memory per session, default scrollback filled (119-char lines) | 12.0 MB, 10 805 rows | 12.9 MB, 6 198 rows |
  | daemon with 4 filled tabs | 50.5 MB | 55.2 MB |
  | GUI draw per incremental frame | 0.14 ms | 0.15 ms |

  dist sizes, default build (both engines) / without libghostty:
  - Linux `haetty`: 6 654 576 / 5 851 648 B;
  - Linux `haetty-headless`: 1 855 424 / 978 968 B;
  - Windows `haetty.exe` (cross): 5 458 944 / 4 174 848 B;
  - Windows `haetty-headless.exe`: 3 244 032 / 1 960 448 B;
  - Windows `haetty.exe` built on Windows (MSVC), without libghostty:
    4 371 968 B.

  The default build carries both engines. New dependency: none besides
  vte 0.15 (E0: +9.5 KB); bitflags 2 was already in the tree.
- Before the default can change (the person's rule: 0 differences):
  - the person decides findings 1 and 2: copy libghostty or keep the
    specification (vtdiff cannot reach 0 with the specification);
  - the undecided differences above;
  - memory: 8-byte cells (styles in a table) to keep at least as many
    rows as libghostty in the same bytes (now 57 % of the rows, 7 % more
    memory);
  - throughput: now 65–75 % of libghostty's;
  - the e2e suite (`run-all.sh`) and a soak with `--engine native`;
  - VT and HTML dumps;
  - finding 3 (stale rows): fixed in the default engine (§10.8).

### 10.8 E4a: the default engine redraws rows libghostty leaves unmarked (2026-11-07)

- The defect (§10.7 finding 3):
  - libghostty's render state copies only the rows the terminal marked
    dirty;
  - a combining mark, VS16 or ZWJ that joins a cell already drawn does
    not mark its row;
  - when that character arrives in a later PTY read than its base, the
    daemon's snapshot keeps the old row until something else changes it.
  - Seen through the observer and the GUI frame: `cafe`, then U+0301 one
    second later, stays `cafe` until the cursor leaves the row.
  - `vtdiff --refresh` found a second unmarked change: the spacer at the
    last column (before a wide character that wrapped) turning back into
    an ordinary blank. It draws the same, but the cells differ.
- Fix: `GhosttyEngine` reads each snapshot from a new render state
  (`Refresh::Always` in `haetty-core/src/ghostty.rs`). The engine and the
  bytes fed are unchanged; only the copy into the snapshot changed.
- Choice, measured (`vtdiff --refresh`: a snapshot after every read, reads
  of 4096 bytes and of 1–64 bytes, corpus plus 200 inputs of each
  generator, 212 886 snapshots):

  | Refresh | New render states | Rows copied per snapshot | Time per snapshot | Stale snapshots (runs) |
  |---|---|---|---|---|
  | incremental (before) | 0 % | 4.91 | 86.2 µs | 1429 (240) |
  | after a possible joiner (`Joined`) | 10.7 % | 6.90 | 86.6 µs | 96 (22) |
  | always (chosen) | 100 % | 24.58 | 90.3 µs | 0 (0) |

  - The Claude Code corpus: 24 rows copied per snapshot instead of 4.02,
    84.3 µs instead of 87.8 µs per snapshot (within noise).
  - `Joined` scans the bytes for zero-width characters, and for anything
    above U+00FF under mode 2027. It copies fewer rows, but it misses the
    spacer case (all of its 96 stale snapshots).
  - Through the daemon (dist, `measure.sh`'s cases, two rounds each,
    incremental / joined / always):
    - key → frame median: 0.88–0.93 ms for all three;
    - throughput: within the noise of the 50 ms steps of headless's
      `wait-exit` (plain lines 24–30 MiB/s, SGR 53–67 MiB/s, a VS16/ZWJ
      mix 10–13 MiB/s).
  - GUI frame cost is unchanged: the daemon sends only rows whose
    contents changed (`haetty-proto` frame diff), so copying unchanged
    rows from libghostty sends nothing more.
- Tests:
  - `haetty-core/tests/ghostty_refresh.rs`: seven split cases (combining
    mark, VS16, VS16 on a narrow base, ZWJ, VS16 split inside its bytes,
    mode 2027 combining and ZWJ) equal a fresh render state. A second test
    shows that the incremental state alone leaves four of them stale.
  - `scripts/e2e/regression.sh` 5b:
    - headless: `café` and `❤️` with each joiner in its own read;
    - GUI: the frame dump with U+0301 a second after `cafe` equals, byte
      for byte, the frame where both come in one read.
    - Both checks failed with the incremental state and pass now.
  - vtdiff's reference was already read fresh (§10.7), so its results are
    unchanged. `run-all.sh` passes with the reference frames unchanged.

### 10.9 E4: KNOWN or FAIL, margins, 8-byte cells, kitty graphics, dumps (2026-11-07)

- The rule for switching the default (the user, E4): **no unexplained
  differences**.
  - A run is KNOWN only if an entry in `docs/vtdiff/known-vt-differences.txt`
    explains it. Every other differing run is FAIL.
  - The list holds only what the user approved: findings 1 and 2, approved
    in the E4 instructions.
  - Proposals go to `known-candidates.txt`. The reasoning for each is in
    `docs/vtdiff/known.md`.
  - KNOWN is decided by counterfactual, not by pattern alone (`docs/vtdiff/known.md`):
    1. the pattern occurs in the input;
    2. every field that differed is among the entry's fields;
    3. the run is identical with the entry's `haetty_vt::Compat` switch on,
       which makes haetty-vt do what libghostty does.
  - vtdiff's self-test adds an unrelated difference and expects FAIL.
- Findings 1 and 2 are already fixed on Ghostty `main` (`5813c94`;
  `docs/upstream/`). A libghostty-vt release built from a later commit would
  make both entries unnecessary.
- haetty-vt behaviours added in E4, as libghostty:
  - E4b, part 1:
    - a print over a wide character or its tail in the first two columns
      clears the row above's spacer head;
    - DCH and ECH split cell boundaries and reset the row's wrap;
    - CAN and SUB end an OSC without printing;
    - OSC 7 keeps semicolons;
    - XTSAVE / XTRESTORE;
    - 47/1047/1049 as `switchScreenMode`.
  - E4b, part 2:
    - left and right margins (DECLRMM, DECSLRM) in printing, wrapping, CR,
      CUF, HT, CBT, CUP/CHA/HPA under origin mode, IND, RI, SU/SD, IL/DL,
      ICH/DCH;
    - IL/DL/SU/SD reset wrap flags only on rows that moved.
- UTF-8: Unicode 17 §3.9 Tables 3-8 to 3-11 give the same U+FFFD
  substitution (maximal subparts) in both engines, whole and byte by byte.
  The decoders do not differ. The remaining broken-UTF-8 runs differ in
  other ways (below).
- Cells (E4c): 8 bytes. That is content, a 16-bit style number, width and
  protection.
  - The style table is per terminal and hashed with an FxHash-like hasher.
    The last style and the last erased form are cached.
  - When the table gets large it is rebuilt from the styles still in use,
    which bounds it (test: 200 000 colours).
- Dumps: `format.rs` ports libghostty's formatter (plain, VT, HTML,
  libghostty's HTML page boundaries included). vtdiff compares the dumps
  after every run.
- Kitty graphics: `kitty.rs` ports libghostty's command handling: parsing,
  base64, replies, quiet levels, chunks, ids and numbers, storage limits,
  placements and the cursor after them, deletes, screen events.
  - Images are kept as metadata and never drawn.
  - Files, temporary files and shared memory are never read; they answer
    "unsupported medium", as this libghostty build does.
  - `o=z` uses `miniz_oxide` 0.9.1 with `adler2` 2.0.1 (70.5 KB crate; +76.8
    KB in a stripped release binary).
  - PNG answers as this libghostty build, which has no PNG decoder.
- Results (`docs/vtdiff/2026-11-07-e4.md`: corpus, 200 inputs of each
  generator, 1 664 runs):
  - **FAIL (unexplained): 98 runs.** KNOWN 0, with the two approved entries
    and one candidate.
    - At the start of E4 there were 321, counted the same way. The inputs
      have since widened: margins and kitty commands in fuzz2, and the
      joiners.
    - Corpus 4/4, fuzz 400/400, mutated 400/400, scenarios, resizes,
      synthetic and joiners all identical.
    - fuzz2 362/400, grammar 340/400.
  - What the FAIL runs are (first differing field):
    - grammar, 60 runs: random bytes with broken UTF-8 and C1 controls.
      Their minimal inputs stay long (hundreds of steps), so the cause is
      context, not a single sequence. Undecided.
    - fuzz2, 38 runs: text 17, width 15, background 6. The minimal inputs
      examined involve wide characters at the last column and resizes
      after scrolling. Not each one has been traced.
    - kitty: a deflate match before the output start, placements through a
      reflow, and a huge `r=`.
  - Dumps after each run, on runs with identical screens:
    - plain 99.9 %;
    - VT 88.6 %;
    - HTML 70.2 %;
    - the rest is underline style and colour, OSC 8 hyperlinks (neither
      kept) and ED 22 on the alternate screen.
- Measured (dist, Linux; libghostty / native):

  | | libghostty | native | the user's bar |
  |---|---|---|---|
  | rows kept, 12 MB limit, 119-char lines (20 000 fed) | 11 065 | 12 195 | ≥ libghostty: met |
  | memory per filled session | 12.4 MB | 13.4 MB (1.10 vs 1.12 kB per row) | |
  | throughput, plain lines | 24–30 MiB/s | 18–22 MiB/s (67–81 %) | ≥ 50 %: met |
  | throughput, long wrapped lines | 55 MiB/s | 24–32 MiB/s (44–58 %) | ≥ 50 %: borderline (50 ms steps of the measurement) |
  | throughput, SGR-heavy | 59 MiB/s | 35–38 MiB/s (59–64 %) | ≥ 50 %: met |
  | key → frame median / p95 | 0.91 / 1.18 ms | 0.74 / 0.92 ms | ≤ libghostty: met |

  - The engine alone (`examples/feed.rs`, in-process): lines 34, wrapped
    71, SGR 60 MiB/s.
- Native beside libghostty, end to end, Linux, commit `6f3ceea`
  (`E2E_ENGINE=native scripts/e2e/run-all.sh`).
  - A check in the same setup confirmed which engine each run used. TBC 0
    lands the next tab at column 9 on native and column 5 on libghostty,
    for both GUI and headless sessions.

  | Check (script) | libghostty | native |
  |---|---|---|
  | regression list (`regression.sh`, incl. late joiners) | 29/29 | 29/29 |
  | DESIGN §2.3, system decorations (`design-2-3.sh`) | 5/5 | 5/5 |
  | DESIGN §2.3, integrated decorations | 5/5 | 5/5 |
  | themes (`themes.sh`) | 8/8 | 8/8 |
  | title row (`titlebar.sh`) | 13/13 | 13/13 |
  | tabs (`tabs.sh`) | 20/20 | 20/20 |
  | orchestrator (`orchestrator.sh`) | 21/21 | 21/21 |
  | GUI after a daemon crash (`daemon-crash.sh`) | 9/9 | 9/9 |
  | first window at 100–200 % (`scale.sh`) | 13/13 | 13/13 |
  | total | 123/123 | 123/123 |
- Soak, one hour on native (Linux, `soak.sh` through `E2E_ENGINE`'s
  wrappers, commit `6f3ceea`): VERDICT PASS.
  - Every operation was done: tabs 200/200, detach 50/50, tasks 20/20,
    themes 10/10.
  - These stayed level from the first sample to the last: GUI threads 7,
    GUI fds 17, daemon threads 24, daemon fds 21, descendants 6, sessions
    4.
  - GUI memory: 7 128 kB at 15 min, 7 228 kB at the end.
  - **Daemon memory: 53 400 kB at 45 min, 57 472 kB at the end.** That is
    +4 072 kB against a bar of +4 MB, and it was still rising by about
    1.4 MB per 5 minutes.
  - The bar was set for libghostty's scrollback, which fills in about 40
    minutes. With 8-byte cells the same byte limit holds more rows and
    fills later.
  - A longer native soak has to show where it levels off.
- Before the default can change (no unexplained differences):
  - the 98 FAIL runs: grammar 60, fuzz2 38;
  - kitty: a deflate match before the output start (miniz reads zeros),
    placements through a reflow;
  - underline style and colour (SGR 4:x, 21, 58) and OSC 8 hyperlinks:
    they show in dumps, and in rendering once the GUI draws them. They fit
    the style table (§10.9 cells);
  - the user's decision on the VPA/VPR candidate;
  - wrapped-line throughput just at the bar;
  - Windows: the native engine's e2e and soak have not run there.
