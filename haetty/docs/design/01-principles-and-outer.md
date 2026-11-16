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
- Settings button (§2.4): it adds no layout change (a shell tab's button is drawn over a cell, the session's size
  stays); on an idle screen it is grey (`scripts/e2e/design-2-3.sh`: its Claude Code tab shows the button).

### 2.4 Settings button and settings panel
(Stage PS, the person's DESIGN §2 change, approved 2026-11-16.)
- **The button.** A single-colour `⚙` (U+2699 with VS15; `≡` when the fonts have no single-colour ⚙, §3.16) in the
  dim colour, the normal text colour under the pointer; no accent, no blinking, no animation. Its cell is haetty's
  own (`HAETTY_UI`: `minimum-contrast` does not change it).
  - Claude Code tabs: always in the status row's last column, also while the row is still blank before Claude Code's
    first report. The row's items end two columns earlier; a long row is cut, the button never moves.
  - Other tabs: no row is reserved. The button is drawn over the last cell of the window only while the pointer is
    near the bottom-right corner (the last 6 columns of the bottom 2 rows) and goes when the pointer leaves or a key
    is typed. Never while the program has asked for mouse reports, nor over the palette, the diff view or the panel.
    Drawing it changes nothing else: the session's size, the bytes the program gets and its screen stay as they are.
  - `settings-button = auto | always | never`: `always` shows it in other tabs whenever the pointer is in the window;
    `never` removes it (Ctrl+, and the palette's "Settings" stay).
- **The panel** opens only from the person: a click on the button, Ctrl+, or the palette's "Settings" — never from
  an event. It covers the screen area like the diff view (the tab bar, the status row and the status panel stay),
  in the panel greys of §2.2: groups on the left (Appearance, Font, Window, Terminal, Claude Code, Advanced), the
  settings on the right; below 64 columns the groups become one line above the list. The selected row has the
  palette's selection grey. Each row: the setting, its value, a dim `•` when it differs from the default, a dim
  "next start" when it applies only then, or the reason in one line when a change was refused. The bottom line
  shows the selected setting's description and its key in the config file. The list scrolls; a window too small
  for it shows no panel.
- While the panel is open no key, text, paste or mouse event reaches the program. A tab waiting for a permission
  answer keeps its Ask mark and its input hold (§3.7) unchanged while the panel opens and closes.
- **Updates** (stage PU, the person's DESIGN §2 change, approved 2026-11-16): while a newer release is out (or one
  is installed and waiting for a restart), the button is in the normal text colour instead of dim — not an accent, no
  blinking, no layout change, no pop-up or sound. The panel then lists the group **Updates** first and opens on it
  (otherwise it is last): the version and commit; the new version and its date; the release notes (Enter: a scrolled
  list); Download and install; the progress; Restart now and Later; Roll back to the version before; `update-check`;
  Check now; the last check's time and result; why it cannot be installed here, when so. Restart now and Roll back
  take a second press, after the bottom line said how many tabs close and that programs in shell tabs end; with a
  permission question or a working worker open, the first press selects Later. Only the person's keys and clicks in
  the panel run these (never a program's input, an IPC message or a command line). The palette's "Updates" opens the
  group (a shell-only window shows no button).
- Not in the panel: what changes how Claude Code works (permission modes, models, hooks, `saving-mode`,
  `worker-command`); `claude-integration` (haetty's observing hooks and status line) is the one Claude Code entry.
