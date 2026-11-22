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
- Default view: the terminal and, from `sidebar-min-cols` (121) window columns on, the **sidebar** on the right
  (§13.3; stage PH, approved): `sidebar-width` (42) columns with its line, the same for every tab and shown from the
  moment a tab opens, so switching tabs or any event never resizes a terminal; only the person's window resize or
  Ctrl+Shift+S (hide/show, kept per user) does. Narrower (or `sidebar = hide`): no sidebar; Ctrl+Shift+S shows it over
  the terminal, which keeps its size. Tab bar only with two or more tabs. While the sidebar is docked a Claude Code tab
  has no status line (the sidebar shows all of it); without it, the status line exists only in Claude Code tabs and
  shows only when it has something to show (a reserved row stays blank until then, §3.10); a shell tab has no status
  line and gets the whole height. A `claude` started in a shell tab (the proxy, §13) never adds a status line. The
  diff overlay and an agent's record are hidden by default and opened by key, palette or click; folded sections and
  sizes persist per user. The diff overlay (stage PM, §13.4) covers the terminal area beside the docked sidebar, side by
  side from `diff-split-cols` (100) columns else top and bottom, and closes back to the exact screen it covered; its
  added and removed tints are the only colours it brings, and only while it is open.
- Events never change the layout. An edit, a finished worker or a pending
  permission shows an indicator in the status line or on the tab
  ("3 files changed · Ctrl+Shift+D"; plain Ctrl+D belongs to Claude Code, §3.10); only the user opens panes. Reason: every
  resize makes Claude Code's fullscreen renderer redraw the whole screen. One exception the person chooses:
  `diff-after-edit = true` (default off) opens the diff overlay on an edit; the overlay never resizes the terminal.
  A second one (stage P3c, approved scope of P3): with `ide-integration = true` (default off) an Edit or Write that
  Claude Code asks permission for opens the diff overlay on the proposal, because the person is being asked about it
  right then (the tab already has the permission mark). Its header says `proposed edit`, its footer `a accept · d
  reject · esc keep asking`; no accent colour beyond the diff tints. Only the person's `a` or `d` in the window
  answers; Esc closes it without an answer (Claude Code's own dialog still asks), and a proposal answered in the
  terminal closes it. It never opens over the palette, the settings panel or an agent's record.
- The status line and the sidebar show context use, rate limits, cost and workers; the sidebar's Lead also names the
  model and effort (stage PH: the harness view of many agents needs them per agent), never the mode. Sidebar sections:
  Lead, Agents (one line per subagent: state glyph, type, short model, current tool or calls, elapsed; nesting by
  indent and └; worktree and fork as dim tags; running first, the last 6 finished, "+ N earlier"), Files, Workers, Log
  (the tab's recent events), and a footer (folder:branch, last update); a shell tab shows its folder, branch and git
  changes instead of the Lead and Agents. Sections fold by a click on their title. The keyboard reaches the sidebar
  over a narrow window (Ctrl+Shift+S) and, docked, after Ctrl+Shift+E (stage PI, approved): ↑/↓ select a line, ←/→
  fold, Enter acts as a click, Esc gives the keyboard back; nothing typed reaches the program meanwhile. The only mark
  is a `›` before the selected line, whose text (a section title too) is drawn in the normal grey; no accent, no
  underline, nothing else changes (reference frame `sidebar-focus`).
- Text selection (stage PX, approved): only the person's mouse makes one, so an idle screen never has one. The
  selected cells take the theme's `selection-background` and the existing text-colour rule (`selection-foreground`
  only where it is not harder to read, 9625bbd); no other colour, no border. A rectangle (Alt+drag) shows as the
  rectangle it is, a normal selection as the stream of rows it covers, so the two differ by their shape alone. The
  grid, the layout and the bytes the program gets never change while selecting. A drag over the sidebar, the tab
  bar or the settings panel is not a selection; inside the diff overlay and an agent's record it selects their drawn
  text. Reference frames `selection`, `selection-rect`, `selection-diff`.

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
  line. No shadows, gradients or rounded boxes inside the window. The sidebar has the terminal's own background and
  a 1 px low-contrast line on its left; section titles one step brighter (bold), items in the normal or dim grey; its
  only colours are state: an agent waiting for a permission answer shows `?` in colour 9, usage ≥ 80 % / ≥ 95 % as in
  the status line. Nothing in it blinks or animates; running agents' elapsed times change once a second.
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
- With one tab: no element besides the terminal and the status line (or, from `sidebar-min-cols`, the sidebar).
- Sidebar (stage PH): the terminal's grid never changes with an event (`sidebar::docked_cols` takes only the window
  and the settings); greys only on an idle screen and while agents run, the one exception a waiting agent's `?`
  (`sidebar` unit tests; reference frames `claude-sidebar`, `workers`).
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

### 2.5 The program's cells, and the inline diff
(Stage PD, the person's DESIGN §2 exception, approved 2026-11-21.)
- The program's cells are drawn as the program sent them. One exception: Claude Code's edit blocks (the red and green
  lines under `Update(…)`, in the conversation and inside a permission question) may be redrawn side by side, before
  on the left and after on the right, with `inline-diff = split` (the default).
- **The condition.** A block is redrawn only when every visible row of it (line number, mark and text; a long line's
  next rows joined) matches one edit the hooks reported, line by line: PostToolUse's `structuredPatch`, or, while the
  tool waits, the hunks of the PreToolUse snapshot against the tool input. No match, or two edits that match, and the
  block shows as Claude Code drew it. A first or last row that does not match (cut by scrolling, covered by Claude
  Code's own label) is left out and the rest must match. Files are not read again; files that look secret are not
  handled; at most 1 ms per frame, over it the frame shows as it was.
- **What changes.** Only the cells inside the rows and columns the block took: the number of rows, the terminal's
  size, the bytes the program gets, the snapshot and what `haetty screen` and Observe clients read stay the same. One
  logical line is one screen row; a line wider than its half is cut with `…`; rows left over are blank. Changed lines
  pair on one row, a line on one side only leaves the other side empty; the changed words get a deeper tint. Colours:
  the diff overlay's tints of colours 2 and 1 and the dim grey, no new colour. A dim `⇄` at the right end of the
  block's header row says haetty redrew it; a click on it turns that block back to Claude Code's shape, a click on the
  file's name in the header opens the diff overlay on that file. In a permission question the option lines and the
  borders around them are never covered.
- From `inline-diff-min-cols` (default 110) columns of program area, a docked sidebar counted; below it, as drawn. A
  new file (Write) and a full deletion have nothing to split and stay as they are. A selection over a redrawn block
  copies the side the drag is on, whole lines, without numbers or the line.
