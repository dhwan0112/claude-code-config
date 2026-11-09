# haetty design: inner design: processes, crates, pipeline, socket, protocol, roles (3.1-3.7)

> Part of the design. Index: [`../DESIGN.md`](../DESIGN.md) (section
> numbers are the same there). Text below is moved verbatim from the former `docs/DESIGN.md`.

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
