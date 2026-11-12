# haetty design: principles and the outer design (sections 1-2)

> Part of the design. Index: [`../DESIGN.md`](../DESIGN.md) (section
> numbers are the same there). Text below is moved verbatim from the former `docs/DESIGN.md`.

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
  included, no accent — on the sides the desktop's window-button layout
  setting names (GNOME: `button-layout`; Xfce: xfwm4's `button_layout`;
  otherwise right). No shadow and no rounded corners are drawn: the
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
