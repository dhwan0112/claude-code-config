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
  ("3 files changed · Ctrl+D"); only the user opens panes. Reason: every
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

- The daemon owns all sessions; closing the window does not stop work.
  Milestone 1 may run sessions in the GUI process, but the GUI talks only
  through `SessionHandle` / `SessionEvent` / `Snapshot`, so moving the
  boundary later does not touch GUI code.
- Hooks and the status line are the same executable (`<bin> hook ...`,
  `<bin> statusline`), reading stdin JSON and forwarding it over the socket.
  Paths in hook commands are quoted and use forward slashes (Windows runs hook
  commands through bash).

### 3.2 Crates (target)

| Crate | Responsibility | Language |
|---|---|---|
| `ccterm-pty` | PTY / ConPTY | Rust (+ Microsoft ConPTY binaries) |
| `ccterm-core` | VT engine thread, snapshots, input encoding | Rust + libghostty-vt (Zig, FFI) |
| `ccterm-render` | cell grids → pixels, glyph cache, damage, softbuffer present | Rust |
| `ccterm-ui` | layout, tabs, panels as cell grids, theme, input routing | Rust |
| `ccterm-cc` | Claude Code adapter: hook/status-line protocol, `--settings` injection, transcripts, session ids, rate limits | Rust |
| `ccterm-orch` | task queue, worker sessions, worktrees, report parsing | Rust |
| `ccterm-ipc` | daemon protocol (length-prefixed messages, versioned) | Rust |
| `ccterm` (bin) | subcommands: `gui` (default), `daemon`, `hook`, `statusline`, `run`, `headless`, `install-desktop` | Rust |

Replacements for `ccx` pieces: `panel.py` → `ccterm-ui` status panel;
`diffview.py` + `delta` + `apply_edit.py` → `ccterm-ui` diff pane with a
Rust diff (e.g. `similar`) and the pre-edit snapshot logic; `hook.sh`,
`post.sh`, `statusline-tee.sh`, `savetabs.sh` → `ccterm-cc` + daemon state;
`ccx_settings.py` → `ccterm-cc` settings builder; `tmux.conf` key bindings →
`ccterm-ui` keymap.

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
| Anonymous memory of the `ccterm` process with one session (120x36, 10k lines of scrollback at 120 columns), Claude Code idle | ≤ 25 MB |
| Anonymous memory per additional session (same scrollback) | measured, reported (no budget yet) |
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
set — inside the budget.

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
