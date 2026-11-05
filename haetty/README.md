# haetty

A terminal built for running Claude Code on Linux and Windows, as a single
executable per platform. `haetty` opens a window (the GUI client); the
sessions (programs in pseudo-terminals) live in a per-user daemon,
`haetty daemon`, which the window starts when needed. The window has tabs,
a command palette and a config file, and in Claude Code tabs it adds
attention marks, a status row, a status panel and a diff view of the last
edit, fed by Claude Code's hooks and status line. Terminal emulation is
libghostty-vt; the window draws on the CPU (winit + softbuffer + swash).
`haetty-headless` runs programs without a window for testing.

```sh
haetty                     # your shell ($SHELL / %COMSPEC%)
haetty claude              # or any program with arguments
haetty --font "Iosevka" --font-size 12 --cols 140 --rows 40 -- claude --resume
```

## Formerly ccterm

haetty was called `ccterm` until 2026-11-05. On its first start, haetty
copies the old config file (`~/.config/ccterm/config`, Windows
`%APPDATA%\ccterm\config`) and saved tab sets (`~/.local/state/ccterm/tabs`,
Windows `%LOCALAPPDATA%\ccterm\tabs`) when it has none of its own; the
originals stay. A `ccterm daemon` that is still running is left alone (its
sessions keep running until its last ccterm window closes); haetty says so
in a hint and its log. The old `CCTERM_*` environment variables are still
read when the `HAETTY_*` one is unset, with a log line asking to rename
them.

## Usage

### Starting

```text
haetty [gui] [--font FAMILY] [--font-size PT] [--cols N] [--rows N] [--KEY VALUE]... [--] [PROGRAM [ARGS...]]
haetty daemon
haetty status [--json | --screen ID]
haetty install-desktop
haetty hook EVENT        (run by Claude Code)
haetty statusline        (run by Claude Code)
```

- `haetty` runs your shell (`$SHELL`, else `/bin/sh`; Windows `%COMSPEC%`,
  else `cmd.exe`); `haetty claude` runs a program directly. `haetty gui ...`
  is the same as `haetty ...`.
- Subcommands must be the first word. The names `gui`, `daemon`, `status`,
  `install-desktop`, `hook`, `statusline` are subcommands, and `run` and
  `headless` are reserved (they end with "not implemented yet", exit code 2).
  To run a program with one of these names, put it after `--`:
  `haetty -- status`.
- `--font`, `--font-size` (points, 4–96), `--cols` and `--rows` (default
  120x36) exist on the command line only. Every other option is a setting
  (`--KEY VALUE` or `--KEY=VALUE`, see [Config file](#config-file)).
- The daemon starts on demand: when no daemon answers, the window starts
  `haetty daemon` detached and connects to it. You do not start it yourself.
  It exits when it has had no session and no window for 10 s, and logs to
  `daemon.log` in the state directory.
- Sessions belong to the daemon. If the window crashes or is killed, its
  sessions keep running (`haetty status` shows them with `attached` 0), and
  the next haetty window takes them back as tabs, as they are — the
  programs are not restarted. Palette: "Attach detached sessions" does the
  same at any time. Closing the window normally ends its sessions (the tab
  set `last` keeps them for the next start, see [Tab sets](#tab-sets)).
- `haetty` without a program opens no new tab when it took sessions back or
  reopened tabs; `haetty PROGRAM` always opens `PROGRAM` as well.
- `haetty status` lists the daemon's sessions; `--json` prints the same as
  JSON; `--screen ID` prints one session's screen as text (IDs come from
  `haetty status`). It is read-only and never starts a daemon: without one
  it prints `no daemon running` (with `--json`:
  `{"daemon_pid":null,"sessions":[]}`).
- If a daemon of another version is still running (after an upgrade), the
  window says so and does not connect. That daemon exits by itself 10 s after
  its last window closes; ending the `haetty daemon` process also ends its
  sessions.

### In the window

- Paste: Ctrl+Shift+V, Shift+Insert or the Paste key; on Windows also
  Ctrl+V. Plain Ctrl+V reaches the application on Linux (Claude Code pastes
  images with it).
- Copy: applications copy with OSC 52 — in Claude Code's fullscreen renderer
  (`CLAUDE_CODE_NO_FLICKER=1`) select with the mouse.
- Wheel: sent to applications that track the mouse, otherwise scrolls the
  scrollback.
- IME: composition is drawn underlined at the cursor; the candidate window
  follows the cursor.
- Fonts: `--font FAMILY` (default Cascadia Mono / Consolas on Windows,
  DejaVu Sans Mono / Noto Sans Mono on Linux), fallback for Hangul/CJK
  (Malgun Gothic, Noto Sans CJK, Nanum) and symbols, then any installed
  font. Box drawing and block characters are drawn, not taken from fonts.
- Window chrome: on Windows 11 the title bar and border take the terminal
  background and the title text its foreground; Windows 10 and Linux get
  the dark theme request (light if the background is light).

### Tabs and keys

Each tab is a daemon session. The tab bar (one row) appears from two tabs
on; click a tab to select it. A new tab runs the program the window was
started with, in the active tab's folder. Tab names come from Claude Code's
session name, else the title (without Claude Code's leading state glyph),
else the program name. When a tab's program exits, the tab stays, dimmed
and marked `[exited N]`, until you close it. With one tab, the window closes
when its program exits and `haetty` returns the program's exit code.

Application shortcuts use Ctrl+Shift (plus Ctrl+Tab and Alt+digit); plain
Ctrl+letter keys go to the program.

| Action | Default | `keybind` action |
|---|---|---|
| New tab | Ctrl+Shift+T | `new_tab` |
| Close tab (press twice) | Ctrl+Shift+W | `close_tab` |
| Next tab | Ctrl+Tab, Ctrl+PageDown | `next_tab` |
| Previous tab | Ctrl+Shift+Tab, Ctrl+PageUp | `previous_tab` |
| Go to tab N | Alt+1 … Alt+9 | `goto_tab:N` (1–9) |
| Command palette | Ctrl+Shift+P | `command_palette` |
| Paste | Ctrl+Shift+V, Shift+Insert, Paste key; Windows also Ctrl+V | `paste` |
| Diff of the last edit (Claude Code tabs) | Ctrl+Shift+D | `diff_view` |
| Status panel (Claude Code tabs) | Ctrl+Shift+S | `status_panel` |
| Status panel wider / narrower | — (palette) | `status_panel_wider` / `status_panel_narrower` |
| Save tabs as the set "saved" | — (palette) | `save_tabs` |
| Open saved tabs | — (palette) | `open_saved_tabs` |
| Open the previous run's tabs | — (palette) | `open_previous_tabs` |

- Close: while the program runs, the first Ctrl+Shift+W shows a hint over
  the bottom row ("Ctrl+Shift+W again closes this tab"); the same chord
  again within 3 s closes the tab. There is no dialog. An exited tab closes
  on the first press.
- Command palette (Ctrl+Shift+P): lists every action with its shortcut.
  Type to filter, arrow keys to move, Enter to run, Esc to close.
- Alt+digit is also bash/readline's numeric argument; rebind it if you need
  that.
- Rebinding, in the config file only (not on the command line):

  ```text
  keybind = ctrl+shift+n=new_tab
  keybind = ctrl+shift+t=unbind
  ```

  `keybind = CHORD=ACTION` adds a binding and `CHORD=unbind` removes one; a
  new binding does not remove the default chord, so unbind that separately.
  A chord is modifiers and a key joined with `+`: `ctrl` (`control`),
  `shift`, `alt` (`opt`, `option`), `super` (`cmd`, `win`), then a character
  (`t`, `1`, `-`) or a key name (`tab`, `pagedown`, `pageup`, `insert`,
  `enter`, `paste`, `f5`); case does not matter.

### Config file

| OS | Path |
|---|---|
| Linux | `$XDG_CONFIG_HOME/haetty/config` (default `~/.config/haetty/config`) |
| Windows | `%APPDATA%\haetty\config` |

One `key = value` per line, as in Ghostty. Lines starting with `#` are
comments, values may be in double quotes, booleans are `true` / `yes` / `1`
or `false` / `no` / `0`. Bad lines are skipped and logged with their line
number. The same keys work on the command line as `--KEY VALUE` or
`--KEY=VALUE`, and the command line wins over the file.

```text
# ~/.config/haetty/config
window-padding = 6
restore-tabs = false
keybind = ctrl+shift+n=new_tab
```

| Key | Values | Default |
|---|---|---|
| `window-padding` | minimum padding around the grid, logical px (0–200) | `4` |
| `window-padding-balance` | `true` / `false` — spread leftover space evenly on all sides | `true` |
| `window-padding-color` | `background` / `extend` / `extend-always` (as in Ghostty) | `extend` |
| `decorations` | `native` / `integrated` (not implemented yet; falls back to `native`) / `none` | `native` |
| `scrollback-limit` | bytes of scrollback per session, as in Ghostty (plain bytes, `_` allowed; rows of 120 columns take ~1.1 KB, the default keeps ~11 000 of them) | `12000000` |
| `restore-tabs` | `true` / `false` — reopen the previous run's tabs at start | `true` |
| `claude-integration` | `true` / `false` — hooks and status line in Claude Code tabs | `true` |
| `notifications` | `true` / `false` — desktop notification (Linux `notify-send`) when Claude Code waits and the window is unfocused | `true` |
| `keybind` | `CHORD=ACTION` or `CHORD=unbind` (file only, may repeat) | see [Tabs and keys](#tabs-and-keys) |
| `theme` | a built-in theme, a theme file's name in the themes folder, or a path (see [Themes](#themes)) | `haetty classic` |
| `palette` | `N=COLOR`, N 0–15 (may repeat) — on top of the theme | the theme's |
| `background`, `foreground`, `cursor-color`, `cursor-text`, `selection-background`, `selection-foreground` | a colour (`#rrggbb`, `rrggbb`, an X11 name) — on top of the theme | the theme's |

### Themes

Themes use Ghostty's theme file format, so Ghostty theme files work as
they are:

```text
palette = 0=#141414
palette = 1=#d03333
…
background = #141414
foreground = #cecece
cursor-color = #cecece
cursor-text = #141414
selection-background = #4e4e4e
selection-foreground = #141414
```

- Built in: `IBM 5153 CGA`, `IBM 5153 CGA (Black)` (the IBM 5153 colour
  monitor's CGA colours; sources and licence in
  [Third-party components](#third-party-components)) and `haetty classic`
  (haetty's earlier colours).
- Your own: put the file into the themes folder — Linux
  `~/.config/haetty/themes/` (`$XDG_CONFIG_HOME/haetty/themes/`), Windows
  `%APPDATA%\haetty\themes\` — and use its file name: `theme = My Theme`.
  A file there wins over a built-in theme of the same name. A value with
  `/` or `\` is a path (relative to the config folder).
- Colour keys in the config file apply after the theme, wherever they
  stand: `theme = IBM 5153 CGA` plus `palette = 4=#3a3aff` changes only
  blue. Keys a theme file leaves out keep `haetty classic`'s values.
- Only colours 0–15 are themed; 16–255 stay xterm's colour cube and grey
  ramp. Programs asking for the colours (OSC 4, 10, 11, 12) get the theme's
  values.
- A theme applies to tabs opened after haetty starts with it; tabs that
  were still running in the daemon keep their colours.

### Tab sets

A tab set is a file `tabs/<NAME>.tsv` in the state directory (Linux
`$XDG_STATE_HOME/haetty`, default `~/.local/state/haetty`; Windows
`%LOCALAPPDATA%\haetty`). Per tab it stores the name, folder, program,
arguments, Claude Code session id and whether it was active; never
environment variables.

| Set | Written | Opened |
|---|---|---|
| `last` | whenever the open tabs change | — |
| `previous` | at start, from `last` | automatically at start; palette "Open the previous run's tabs" |
| `saved` | palette "Save tabs" | palette "Open saved tabs" |

- At start, haetty first takes back the daemon's running sessions that no
  window shows, then `last` becomes `previous` and only its tabs that the
  daemon does not have are reopened (Claude Code tabs are matched by
  conversation, other tabs by program, arguments and folder).
  `restore-tabs = false` (or `--restore-tabs false`) turns reopening off.
- Tabs whose folder no longer exists are skipped (logged). A Claude Code
  conversation that is running in any tab or window is never opened (or
  resumed) a second time.
- A reopened tab is a new session: the program is started again. For
  Claude Code tabs see [Resume](#resume).

### Claude Code integration

Applies to tabs whose program is `claude` (`claude.exe`, `claude.cmd`) while
`claude-integration` is on (default). `claude-integration = false` (or
`--claude-integration false`) turns it off: Claude Code then runs exactly
as in any terminal.

**What is injected.** The daemon adds `--settings <file>` to the command
line, pointing to a per-session file in a private runtime directory (mode
0600; Windows `%LOCALAPPDATA%\haetty\run`), removed when the session ends.
The file contains only `hooks` and `statusLine`: haetty does not change
Claude Code's permission mode (no `--dangerously-skip-permissions`, no
`defaultMode`, no allow/ask/deny rules) or any other behaviour — your own
settings (`~/.claude/settings.json`, project and local settings, managed
policy, command-line flags) apply exactly as in any other terminal — and it
never touches those files. The hooks only report: they print nothing and
never answer a permission prompt. If you pass your own `--settings` (file or inline
JSON), it is merged into that file: your hooks are kept after haetty's, your
other keys are kept, and your `statusLine` is dropped with a log warning
(haetty's status row uses it). The hooks run `haetty hook EVENT` directly
(no shell); the status line runs `haetty statusline` through Claude Code's
shell. Both forward the JSON to the daemon and print nothing.

**Side effect.** Claude Code documents that with a custom status line it
hides most of its footer hints ("esc to interrupt", "? for shortcuts").
haetty's status row shows "esc to interrupt" itself while Claude Code
works. `haetty statusline` prints nothing, so Claude Code draws no status
line of its own.

**Attention marks.** On the tab bar, on tabs other than the active one:

| Mark | Meaning | Goes away |
|---|---|---|
| amber `●` | Claude Code stopped or is idle, waiting for you | when you select the tab, or focus the window on it |
| red `?` | a permission prompt or a question | only when Claude Code moves on (a prompt is sent, the tool runs or is denied, it stops), not by looking |

**Status row.** The bottom row of a Claude Code tab, reserved from the
start so that Claude Code is not reflowed when the first report arrives (it
shows `-` until then; the window adds that row, so `--rows N` still gives
Claude Code N rows):

```text
ctx  42%  5h  12%  7d  30%  $  1.23  3 files changed · Ctrl+Shift+D
```

Context use, 5-hour and 7-day limits, cost and files changed, in greys;
amber from 80 %, red from 95 %. "esc to interrupt" appears at the right
while Claude Code works.

**Status panel (Ctrl+Shift+S).** A column on the right, hidden by default;
opening it resizes the program, and it is not shown when fewer than 40
columns would be left for the program. It shows what Claude Code's own
screen lacks: session id and name, context of size, limits with reset times
(UTC), cost (an API estimate with subscription limits), lines, prompt cache,
files and edits, the last edit, subagents with durations, last update.
Model and effort are left out (Claude Code shows them). Width (24–80
columns, palette "Status panel wider/narrower") and open state are kept in
`ui` in the state directory.

**Diff view (Ctrl+Shift+D).** The last edit (Write, Edit, MultiEdit) of the
active tab, drawn over its screen; the program is not resized and keeps
running. Side by side from 110 columns, unified below; header
`[last edit] path · +A -R · by agent`. Keys: j/k, Space/b, g/G, wheel to
scroll; Esc or q (or Ctrl+Shift+D again) to close. Files that look like
credentials or keys (`.ssh`, `.aws`, `.env*`, `*.pem`, `*.key`, `*token*`,
`*secret*`, `.credentials.json`, …) are never read, and files over 2 MiB
or not UTF-8 are not copied; for those the diff view says why instead of
showing a diff.

<a id="resume"></a>**Resume.** Tab sets store each Claude Code tab's
session id. A restored Claude Code tab gets `--resume ID` when Claude Code
still has that transcript (under `CLAUDE_CONFIG_DIR` or `~/.claude`),
replacing earlier `--resume` / `--continue` arguments; otherwise it starts
a new conversation.

**Notifications.** When the window is unfocused and a tab starts waiting or
asking: the taskbar flashes (Windows) or the urgency hint is set (X11), and
on Linux `notify-send` shows a notification when it is installed. The text
names the tab and says that Claude Code is waiting or asks something; never
session content. `notifications = false` turns off the `notify-send`
notification; the taskbar/urgency hint stays.

### Desktop integration

- Linux: `haetty install-desktop` writes, for the current user (under
  `$XDG_DATA_HOME`, default `~/.local/share`),
  `applications/io.github.dhwan0112.haetty.desktop` (pointing to the
  running executable, with `StartupWMClass=io.github.dhwan0112.haetty`) and
  the icon in the hicolor theme (`io.github.dhwan0112.haetty`). The Wayland
  `app_id` and X11 `WM_CLASS` are that app id, so docks match the window to
  the entry. Run it again after moving the executable.
- Windows: nothing to install; `haetty.exe` carries its icon (pin it to
  Start or the taskbar). `haetty install-desktop` says so.

The icon is in `assets/icon/` (the design `haetty.svg` and the PNG, ICO
and RGBA files generated from it; `assets/icon/README.md` says how).

### Environment variables

| Variable | Effect |
|---|---|
| `HAETTY_LOG` | `off` / `error` / `warn` / `info` / `debug` / `trace` (default `warn`; `debug` never records typed text) |
| `HAETTY_LOG_FILE` | log to this file instead of stderr (the Windows build has no console) |
| `HAETTY_INSTANCE` | run a second, separate instance (e.g. a development build) with `HAETTY_INSTANCE=dev`: own daemon, sessions, settings, tab sets. Directories become `haetty-dev`, the app id and desktop file `io.github.dhwan0112.haetty.dev`, window titles start with `[dev]`. Letters, digits and `-`, up to 16 characters; unset or empty is the normal instance. |
| `HAETTY_FONT_DIR` | an extra font directory |
| `HAETTY_CONPTY_DLL` | Windows: use this `conpty.dll` (see [Windows notes](#windows-notes)) |
| `HAETTY_DUMP_FRAME` | write every frame to this `.ppm` file (debugging) |
| `HAETTY_DEBUG_PADDING` | `1` paints everything outside the grid red, to tell haetty's padding from a window manager's frame |
| `HAETTY_SOCKET`, `HAETTY_STATE_DIR`, `HAETTY_CONFIG_DIR` | for tests: override the daemon socket / pipe, the state directory and the config directory |

`HAETTY_SESSION_ID` (and `HAETTY_INSTANCE`) are set by the daemon in every
session's child, so that `haetty hook` and `haetty statusline` reach the
right daemon and tab.

### Known limits

- `decorations = integrated` (own title bar with tabs) is not implemented;
  it falls back to `native`.
- Tab sets are not per window: with several haetty windows open, the last
  one to write wins the `last` set.
- The diff view shows the last edit only (no diff against `HEAD`, no file
  tree); NotebookEdit edits record the path only.
- Windows: a hook process takes about 64 ms on the test machine (process
  start alone is about 52 ms), so the 50 ms target is not met there. Only
  the edit hook (PreToolUse for Write/Edit/MultiEdit/NotebookEdit) makes
  Claude Code wait for it.
- Windows: if Claude Code runs its status line through PowerShell (no Git
  Bash) and haetty is installed in a path with spaces, the double-quoted
  status-line command does not run in PowerShell.
- Windows: no toast notifications; only the taskbar flash.
- Status panel reset times are in UTC.
- `scrollback-limit` takes plain bytes; suffixes such as `10MB` are not
  parsed.
- No own text selection yet when the application does not track the mouse
  (plain shell); copying is the application's job (OSC 52).
- Not yet checked by hand (`docs/HANDOFF.md` §6.1): tabs and the palette
  with a real keyboard, `install-desktop` on GNOME/KDE, the attention marks
  and diff view with a real Claude Code account, notifications, and the
  PowerShell status-line case above; also the Microsoft IME, a real mouse
  wheel and HiDPI monitors.

## How it works

```
              local socket (Unix socket / Windows named pipe)
GUI client(s) ◀──────────────────────────────────────────▶ daemon
                                                            ├─ sessions: PTY + VT engine thread each
claude (in PTY) ──hook command──▶ `haetty hook <event>` ──▶ └─ Claude Code adapter (hooks, status line, transcripts)
```

The socket is `$XDG_RUNTIME_DIR/haetty/daemon.sock` on Linux (else
`/tmp/haetty-<uid>/daemon.sock`; in a private 0700 directory) and the named pipe `\\.\pipe\haetty-<user SID>` on Windows
(only that user may connect, no remote clients). There is no network
listener. Details: `docs/DESIGN.md` §3.

### Crates

| Crate | Role |
|---|---|
| `haetty-pty` | Pseudo-terminals. Unix: `portable-pty`. Windows: the ConPTY API called directly, preferring Microsoft's out-of-band `conpty.dll` + `OpenConsole.exe` (embedded, feature `embedded-conpty`) over the in-box console host. |
| `haetty-core` | `Session`: child in a PTY, a libghostty-vt `Terminal` on its own engine thread, replies to terminal queries, key/mouse/paste/focus encoding, synchronized-output handling, OSC notification/progress events, snapshots. |
| `haetty-proto` | Product name and instance, paths, wire types (postcard), framing, local socket / named pipe transport, the daemon client. |
| `haetty-daemon` | Session owner: sessions, subscriptions and row-diff frames, roles, event log. |
| `haetty-cc` | Claude Code adapter: `--settings` builder, hook and status-line reporters, per-session state, edit snapshots and diffs, resume. |
| `haetty-headless` | CLI that runs a program in a session and drives it with a step script (type, keys, wheel, drag, resize, wait for text, dump the screen). |
| `haetty-gui` | The `haetty` binary: subcommand dispatch, the window (winit: X11 and Wayland loaded at run time, Win32), tabs, palette, panels, CPU rendering into softbuffer behind a `Renderer` trait, glyphs by swash, fonts found with fontdb, clipboard via copypasta. |

### Threads and data flow

Inside the daemon, per session:

```
PTY reader thread ──Output──▶ engine thread ──Wakeup/Title/Notification/Exited──▶ frontend
child waiter thread ─Exited─▶   (Terminal, RenderState, encoders, PTY writer)
frontend ──Key/Mouse/Paste/Resize/Snapshot──▶ engine ──Snapshot (plain data)──▶ frontend
```

libghostty-vt objects are `!Send`, so they never leave the engine thread.
Every byte to the child (query replies and user input) is written from that
thread, in order. A frontend receives `SessionEvent::Wakeup` at most once until
it takes a snapshot, and no wakeup is sent while the application holds
synchronized output (mode 2026), up to `sync_timeout` (500 ms default).
Over the socket the daemon sends a window the rows that changed since its
last frame, and the next frame only after the window asked for it.

## Building

Needs Rust 1.90+ and Zig 0.15.2 on `PATH` (libghostty-vt is built from
Ghostty's source by `libghostty-vt-sys`).

```sh
# optional: reuse a Ghostty checkout at the pinned commit instead of cloning
export GHOSTTY_SOURCE_DIR=/path/to/ghostty
cargo build --release
cargo test
```

Windows binaries can be cross-built from Linux with `rustup target add
x86_64-pc-windows-gnu` and mingw-w64 (`cargo build --release --target
x86_64-pc-windows-gnu`); the result depends only on system DLLs. The icon
resource is compiled with mingw's `windres` (`WINDRES` overrides the name);
without it, or for an MSVC target, `haetty.exe` gets no icon. On Linux the
binary links only libc/libm/libgcc_s; X11, Wayland and xkbcommon are loaded
at run time.

For shipping, `cargo build --profile dist [--target ...]` (opt-level "s",
thin LTO, stripped): `haetty` about 6.3 MB on Linux and 4.9 MB on Windows
(milestone 3).

## haetty-headless

```sh
haetty-headless --cols 120 --rows 36 --trace \
  -e "wait-idle 3000 60000" -e "modes" -e "type hello" -e "key shift+Enter" -e "print" \
  -- claude
```

Steps: `sleep MS`, `wait-text TEXT [MS]`, `wait-title TEXT [MS]`, `wait-idle MS [MAX]`, `wait-exit [MS]`,
`type TEXT`, `send TEXT` (raw bytes), `key SPEC` (`Enter`, `shift+Enter`,
`ctrl+c`, `ArrowUp`, `F5`, …), `paste TEXT`, `wheel up|down N COL ROW`,
`click COL ROW`, `drag C1 R1 C2 R2`, `focus in|out`, `resize COLS ROWS`, `expect TEXT`,
`expect-not TEXT`, `print`, `modes`, `save FILE`. `--raw-log FILE` keeps every
output byte; `--trace` prints replies to queries and events.

## Status

Milestones 1–3 of `docs/DESIGN.md` §5 are built: the window, the daemon
with tabs and tab sets, and the Claude Code integration. The orchestrator
(milestone 4) is not started. Evidence per phase is in `docs/HANDOFF.md`
§3 (milestone 1) and §6.4 (milestones 2–3); open manual checks in §6.1.

Milestone 1, verified on Linux (x86_64):

- `haetty-core`: unit tests (OSC scanner) and end-to-end tests with real
  children in real PTYs (query replies, key/mouse/paste/focus encoding,
  synchronized output hold and timeout, OSC title/notification/progress/bell,
  resize, cell colours and wide characters, exit reporting, environment).
- Claude Code 2.1.318: onboarding screens drawn and navigated with arrow keys;
  main screen in the fullscreen renderer (alternate screen, SGR mouse
  tracking, kitty keyboard flags 5, bracketed paste, focus events), 24-bit
  colour output, Hangul input, Shift+Enter for a second input line, Ctrl+C
  twice to exit with status 0.
- `haetty` window with Claude Code under X11 and native Wayland: rendering,
  typing (ASCII, Hangul through ibus-hangul/XIM), Shift+Enter, wheel,
  resize, selection copy to the clipboard and paste (X11), idle CPU ~0%.

Windows 11 (native, x86_64): the PTY checks of `scripts/windows-smoke.ps1`
pass (embedded ConPTY 1.25); its Claude Code checks need a prepared Claude
Code config (see `docs/HANDOFF.md` §3) and pass with one, run through
`haetty-headless` (fullscreen renderer, kitty keyboard flags 5 included).
The `haetty` window runs Claude Code (typing, Shift+Enter, Ctrl+V paste,
exit code passed on), and the GUI unit tests pass there.

Milestones 2–3: daemon integration tests (a killed client's session keeps
running and can be listed, read-only and report roles refused, settings
injection and merge, attention rules), GUI and adapter unit tests, and
regression runs on Linux (Xvfb, real Claude Code with a dummy key) and
Windows 11. Key press to frame through the daemon: median about 2 ms on
both. Idle with Claude Code idle: GUI and daemon about 0 % CPU, about 6 MB
and 5 MB anonymous memory on Linux.

## Windows notes

ConPTY sits between the child and the terminal. The in-box host re-renders
output and has dropped mouse-mode requests (older builds), APC strings and
kitty keyboard negotiation. haetty therefore loads, in order: a `conpty.dll`
named by `HAETTY_CONPTY_DLL` or placed next to the executable; the copy
embedded in the binary, written once to
`%LOCALAPPDATA%\haetty\conpty\<version>\` together with `OpenConsole.exe`
and Microsoft's licence; kernel32. With the out-of-band host it passes
`PSEUDOCONSOLE_GLYPH_WIDTH_GRAPHEMES` (0x8); the in-box host gets no flags,
because 0x8 meant `PSEUDOCONSOLE_PASSTHROUGH_MODE` in older declarations.
npm's `claude.cmd` is started through `cmd.exe /d /s /c`.

The daemon is started without a console and outside the window's job
object where the job allows it; otherwise it ends with that job (logged).

## Third-party components

All of them, with the licence texts that are not in the code's own
packages: `THIRD-PARTY-NOTICES.md`.

- Colour themes "IBM 5153 CGA" and "IBM 5153 CGA (Black)" (embedded,
  `assets/themes/`): by Jesse Miller (https://jmiller.com/), after
  int10h.org's "IBM 5153's True CGA Palette and Color Output" (June 2022,
  https://int10h.org/blog/2022/06/ibm-5153-color-true-cga-palette/); the
  Ghostty files of mbadolato/iTerm2-Color-Schemes
  (https://github.com/mbadolato/iTerm2-Color-Schemes) at commit `38bd77c`,
  copied unchanged; MIT. When haetty loads them it fills in one value: a
  selection background missing or equal to the foreground becomes
  `#4e4e4e` (`assets/themes/README.md`). Licence:

```text
MIT License

Copyright (c) 2011 to Present Mark Badolato

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

This license covers the iTerm-Color-Schemes repository collection of themes. 

The copyright/license for each individual theme belongs to the author of that theme.
```


- libghostty-vt (Ghostty, MIT) via `libghostty-vt` / `libghostty-vt-sys`
  (MIT OR Apache-2.0).
- `portable-pty` (MIT) on Unix.
- winit, softbuffer (Apache-2.0 / MIT), swash, fontdb, copypasta, memmap2,
  unicode-width (MIT / Apache-2.0).
- postcard, serde, crossbeam-channel, log, libc, windows-sys
  (MIT / Apache-2.0); similar (Apache-2.0).
- Microsoft.Windows.Console.ConPTY `conpty.dll` / `OpenConsole.exe` (MIT,
  Microsoft) via `conpty-bin-vendored` (MIT) on Windows.

## Licence

MIT, see `LICENSE`.
