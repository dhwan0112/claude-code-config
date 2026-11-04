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

### 2.1 Window layout (default "sidebar" layout)

```
┌──────────────────────────────────────────────────────────────┬───────────┐
│ ● main  ◐ worker-1  ? worker-2  + │            title / window controls   │  tab bar (pixel chrome)
├──────────────────────────────────────────────────────────────┬───────────┤
│                                                              │ SESSION   │
│  Claude Code (fullscreen renderer)                           │ model     │
│                                                              │ context ▮▮│
│                                                              │ limits    │
│                                                              │ WORKERS   │
│                                                              │ ● w1 12s  │
│                                                              │ ✓ w2 ok   │
├──────────────────────────────────────────────────────────────┤ KEYS      │
│  diff: last edit (side by side ≥ 110 cols, else stacked)     │           │
└──────────────────────────────────────────────────────────────┴───────────┘
```

- Status panel right (default 22 % of width, min 36 cols), diff pane below
  the terminal (default 22 % of height, min 6 rows), both resizable by drag
  and collapsible; sizes persist. This mirrors `ccx`, whose layout the user
  already works in.
- Tab states (from hooks, not from screen scraping): normal; amber when the
  tab's Claude finished a turn or waits at the prompt while not visible; red
  with `?` while a permission prompt or question is pending. Cleared as in
  `ccx`.
- Orchestrator view = the WORKERS section: one row per worker with state
  glyph (`●` running, `✓` verified, `✗` blocked, `?` needs review, `-` no
  report), elapsed time, description; click switches to the worker's tab.

### 2.2 Visual language

- One theme struct feeds terminal default colours, panel colours, tab bar,
  window decorations (Windows 11 caption/border colours) and padding.
- Padding balanced on all sides; padding colour `extend` by default on the
  alternate screen (no visible frame around Claude Code's fullscreen UI).
- Palette: dark ink background, light foreground; accents not Anthropic
  orange (avoid confusion with Claude branding). Attention colours amber and
  red as above.
- Fonts: user monospace font first; fallback chain for Hangul/CJK (Windows
  Malgun Gothic, Linux Noto Sans CJK / Nanum); box drawing and block
  elements drawn by code, not from the font, so borders always join.
- Icon: original artwork (no resemblance to Claude Code's pixel mascot);
  one SVG source, pre-rendered PNG sizes and a multi-size `.ico` committed.
- Decorations: `native` (default; Windows 11 caption and border coloured to
  the theme, dark-theme hint elsewhere), `integrated` (own tab bar with
  window controls, drag via `drag_window`, resize via `drag_resize_window`),
  `none`.

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
| Stripped binary | ≤ 12 MB Linux, ≤ 10 MB Windows (incl. ConPTY) |
| Idle CPU, Claude Code idle | ~0 % |
| RSS per session, 10k scrollback | ≤ 25 MB |
| Key press → frame | ≤ 16 ms at 120x36 on a laptop CPU |

## 4. VT engine decision (measured 2026-11-04)

Replay of a real Claude Code 2.1.318 session (startup, Hangul typing,
Shift+Enter, fullscreen renderer; 2.5 KB of output) at 120x36:
libghostty-vt and alacritty_terminal 0.26 produced identical screens, equal
to the live screen. A 16.9 MB synthetic stream (24-bit colour, Hangul, CJK,
emoji, box drawing, synchronized updates) x3 at 120x36 with 10k scrollback,
one vCPU: libghostty-vt ~31–34 MB/s, max RSS ~20 MB; alacritty_terminal
~50–73 MB/s, max RSS ~47 MB; identical final screens. Building libghostty-vt
for `x86_64_v3` instead of `baseline` did not change throughput measurably.
Minimal stripped binaries (engine + replay): 2.0 MB vs 0.6 MB.

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
