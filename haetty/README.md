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
haetty events [--follow] [--json] [--since CURSOR]   (observable tabs only)
haetty screen [--json] SESSION                       (observable tabs only)
haetty install-desktop
haetty completions bash|zsh|fish|powershell
haetty hook EVENT        (run by Claude Code)
haetty statusline        (run by Claude Code)
```

- `haetty` runs your shell (`$SHELL`, else `/bin/sh`; Windows `%COMSPEC%`,
  else `cmd.exe`); `haetty claude` runs a program directly. `haetty gui ...`
  is the same as `haetty ...`.
- Subcommands must be the first word. The names `gui`, `daemon`, `status`,
  `events`, `screen`, `install-desktop`, `completions`, `hook`, `statusline` are subcommands, and `run` and
  `headless` are reserved (they end with "not implemented yet", exit code 2).
  To run a program with one of these names, put it after `--`:
  `haetty -- status`.
- `haetty --help` lists the options, every setting and the subcommands;
  `haetty SUBCOMMAND --help` explains one. Both come from the same tables as
  the shell completions:
  - bash: `eval "$(haetty completions bash)"` in `~/.bashrc`;
  - zsh: `haetty completions zsh > "${fpath[1]}/_haetty"` (or `eval` it after
    `compinit`);
  - fish: `haetty completions fish > ~/.config/fish/completions/haetty.fish`;
  - PowerShell: `haetty completions powershell | Out-String |
    Invoke-Expression` in `$PROFILE` (Windows PowerShell 5.1 completes no
    bare `-` or `--`: type the first letter; PowerShell 7 does).
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
- `haetty events` and `haetty screen` are for programs that watch haetty
  without being able to touch it, and see only tabs you allowed — see
  [Observing a tab](#observing-a-tab-read-only).
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

### Title bar

On Wayland (GNOME's default session) haetty draws its own title bar
(`decorations = auto` → integrated): the tab row, in the terminal's colours,
which stays with one tab and then shows the window title. The window
buttons follow your desktop's button layout (GNOME's `button-layout`, on
Xfce xfwm4's `button_layout`; right side when it cannot be read). Drag empty title space to move the window,
double-click it to maximize or restore, drag a window edge to resize. On
Windows 11 and X11 the system title bar is used and coloured to match;
`decorations = integrated` forces haetty's on X11 too, `native` the system's
everywhere.

### Tabs and keys

Each tab is a daemon session. The tab bar (one row, on the terminal's own
background; the active tab in the text colour with a thin line under it,
the others dim) appears from two tabs on; click a tab to select it. A new tab runs the program the window was
started with, in the active tab's folder. Tab names: your own (Ctrl+Shift+R;
an empty name brings the automatic one back), else Claude Code's session
name, else for Claude Code tabs `repo:branch` of the tab's folder, else the
title (without Claude Code's leading state glyph),
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
| Diff view (Claude Code tabs): last edit, `m` → vs HEAD → working tree | Ctrl+Shift+D | `diff_view` |
| Rename tab | Ctrl+Shift+R | `rename_tab` |
| Reopen a closed tab (last 20) | — (palette) | `reopen_closed_tab` |
| Detach tab (keeps running) | — (palette) | `detach_tab` |
| Attach detached sessions | — (palette) | `attach_detached` |
| New task (a worker in a git worktree) | — (palette) | `new_task` |
| Message the worker | — (palette) | `message_worker` |
| Remove a task's worktree | — (palette) | `remove_task_worktree` |
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

<!-- config-table:start: generated from config::SETTINGS (crates/haetty-gui/src/config.rs); HAETTY_UPDATE_README=1 cargo test -p haetty-gui config_table -->
| Key | Values | Default |
|---|---|---|
| `window-padding` | minimum padding around the grid on all sides, logical px (0–200) | — |
| `window-padding-x`, `window-padding-y` | the same, left/right or top/bottom only | `10`, `6` |
| `window-padding-balance` | `true` / `false` — spread leftover space evenly on all sides | `true` |
| `window-padding-color` | `background` / `extend` / `extend-always` (as in Ghostty) | `extend` |
| `decorations` | `auto` / `native` / `integrated` / `none` — auto: the system's title bar where it takes the theme's colour (Windows 11, X11), haetty's own (integrated) on Wayland; integrated is Linux only (Windows: native) | `auto` |
| `scrollback-limit` | bytes of scrollback per session, as in Ghostty (plain bytes, `_` allowed; rows of 120 columns take ~1.1 KB, the default keeps ~11 000 of them) | `12000000` |
| `restore-tabs` | `true` / `false` — reopen the previous run's tabs at start | `true` |
| `engine` | `ghostty` / `native` — the terminal engine of new tabs: libghostty-vt, or haetty's own (DESIGN.md §10; a build without libghostty always runs native) | `ghostty` |
| `claude-integration` | `true` / `false` — hooks and status line in Claude Code tabs | `true` |
| `worker-command` | the orchestrator's worker: a program and its arguments, space-separated (DESIGN.md §8.1) | `claude` |
| `notifications` | `true` / `false` — desktop notification (Linux `notify-send`) when Claude Code waits and the window is unfocused | `true` |
| `observe-new-tabs` | `true` / `false` — new tabs start observable by `haetty screen` / `haetty events` ([Observing a tab](#observing-a-tab-read-only)) | `false` |
| `token-status` | `true` / `false` — the status panel shows the tokens of the active tab and of each worker, read from the Claude Code transcripts ([`haetty tokens`](docs/tokens/README.md)) | `false` |
| `clear-hint` | a percentage, 0 for off: a worker tab shows a dim `/clear` once its context has passed that share of `context-window`. A hint only: nothing is sent ([Orchestrator](#orchestrator-tasks-and-workers)) | `60` |
| `context-window` | the context window of the workers' model, in tokens, for `clear-hint` (at least 1000) | `200000` |
| `saving-mode` | `true` / `false` — saving mode: Claude Code sessions haetty starts get `BASH_MAX_OUTPUT_LENGTH=12000` and `MAX_MCP_OUTPUT_TOKENS=12000`, the official limits on Bash output read back and on MCP tool output ([the values and sources](docs/tokens/README.md#saving-mode)); the status row shows a dim `◆ saving` | `false` |
| `keybind` | `CHORD=ACTION` or `CHORD=unbind` (file only, may repeat) | see [Tabs and keys](#tabs-and-keys) |
| `theme` | a built-in theme, a theme file's name in the themes folder, or a path (see [Themes](#themes)) | `IBM 5153 CGA` |
| `palette` | `N=COLOR`, N 0–15 (may repeat) — on top of the theme | the theme's |
| `minimum-contrast` | 1–21, as in Ghostty: text whose WCAG contrast with its background is lower turns white (dark background) or black (light) | `1` (off) |
| `background`, `foreground`, `cursor-color`, `cursor-text`, `selection-background`, `selection-foreground` | a colour (`#rrggbb`, `rrggbb`, an X11 name) — on top of the theme | the theme's |
<!-- config-table:end -->

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

- Built in: `IBM 5153 CGA` (the default) and `IBM 5153 CGA (Black)` — the
  IBM 5153 colour monitor's CGA colours, colour 3 ("yellow") is brown as on
  the real monitor; sources and licence in
  [Third-party components](#third-party-components) — and `haetty classic`
  (haetty's colours before themes: `theme = haetty classic`).
- haetty's own rows follow the theme: greys mixed from its background and
  foreground; the "waiting" mark and usage ≥ 80 % in colour 3, the "asks
  you" mark and usage ≥ 95 % in colour 9.
- Your own: put the file into the themes folder — Linux
  `~/.config/haetty/themes/` (`$XDG_CONFIG_HOME/haetty/themes/`), Windows
  `%APPDATA%\haetty\themes\` — and use its file name: `theme = My Theme`.
  A file there wins over a built-in theme of the same name (a
  `HAETTY_INSTANCE=dev` build reads `haetty-dev/themes/` instead, like its
  config file). A value with
  `/` or `\` is a path (relative to the config folder).
- Colour keys in the config file apply after the theme, wherever they
  stand: `theme = IBM 5153 CGA` plus `palette = 4=#3a3aff` changes only
  blue. Keys a theme file leaves out keep `haetty classic`'s values.
- Selected text keeps its own colour when that is easier to read on the
  selection background than the theme's `selection-foreground` (WCAG
  contrast; details in `assets/themes/README.md`).
- Only colours 0–15 are themed; 16–255 stay xterm's colour cube and grey
  ramp. Programs asking for the colours (OSC 4, 10, 11, 12) get the theme's
  values.
- `minimum-contrast = 3` (or 4.5) keeps dim colours readable: with IBM
  5153 CGA, blue (1.85:1 on its background) and bright black (2.21:1) are
  hard to read; at 3 they — and colour 0 — turn white, at 4.5 also red,
  magenta and bright blue. It changes only what programs draw; haetty's
  own rows (tab bar, status row, panel, palette, hints) keep their
  colours. The table and frame dumps are in
  `assets/themes/README.md` and `docs/theme/`.
- **Claude Code in the theme's colours:** Claude Code's own themes use
  fixed 24-bit colours, which no terminal theme changes. To have Claude
  Code's screen use the theme's palette too, choose its ANSI theme once:
  in Claude Code type `/theme` and pick **Dark mode (ANSI colors only)**
  (it is saved as `"theme": "dark-ansi"` in `~/.claude/settings.json`).
  Checked with Claude Code 2.1.318: with its default dark theme its start
  screen and `/theme` preview use only 24-bit colours (`38;2`/`48;2`, 63
  times, no palette numbers); with "Dark mode (ANSI colors only)" the same
  screens use only palette numbers (`37`, `91`–`96`, `40`; 40 times) and no
  24-bit colour — so IBM 5153 CGA's colours appear in Claude Code itself.
- Changing the theme while haetty runs: edit the config file (`theme =`,
  colour keys, `minimum-contrast`) and switch back to the haetty window —
  it rereads the file when it gets focus and the file changed — or choose
  "Reload config" in the palette (Ctrl+Shift+P). Every open tab, and any
  tab haetty reattaches later, takes the new colours at once; other
  settings still need a restart.
- **Claude Code keeps its own look until it restarts.** It decides on its
  light or dark appearance when it starts (from the terminal background
  with its "Auto" theme, else from its `/theme` setting), so changing the
  haetty theme under a running Claude Code does not switch Claude Code's
  own theme — after going from a dark to a light theme (or back), restart
  Claude Code or pick its theme again. With "Dark mode (ANSI colors
  only)" its colours are palette numbers and follow the change at once.

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
| `closed` | a tab closed on purpose (newest first, 20 at most) | palette "Reopen a closed tab" (one at a time) |

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
- A renamed tab keeps its name in tab sets; automatic names are worked out
  again.
- Palette "Detach tab" takes the active tab out of the window and leaves
  its program running; "Attach detached sessions" brings it back (also in
  another window). Closing the window instead ends its tabs.

### Claude Code integration

**tmux started by your shell.** If your shell starts tmux on its own (an
`exec tmux` in `~/.bashrc` / `~/.zshrc`), Claude Code in a haetty tab runs
inside tmux and loses what haetty gives it: it sees `TMUX` and lowers its
colours to 256, and tmux does not support synchronized output (checked with
tmux 3.4: haetty answers the mode-2026 query, inside tmux it gets no answer),
so Claude Code's redraws can flicker. tmux also replaces `TERM_PROGRAM` with
`tmux` inside, so test it before tmux starts — skip tmux in haetty:

```sh
# ~/.bashrc or ~/.zshrc
if [ -z "$TMUX" ] && [ "$TERM_PROGRAM" != haetty ] && command -v tmux >/dev/null; then
  exec tmux new-session -A -s main
fi
```

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
| `●` (theme colour 3: brown in IBM 5153 CGA, amber in others) | Claude Code stopped or is idle, waiting for you | when you select the tab, or focus the window on it |
| `?` (theme colour 9: red) | a permission prompt or a question | only when Claude Code moves on (a prompt is sent, the tool runs or is denied, it stops), not by looking |

**Status row.** The bottom row of a Claude Code tab, reserved from the
start so that Claude Code is not reflowed when the first report arrives (it
stays blank until Claude Code reports something; the window adds that row,
so `--rows N` still gives Claude Code N rows). Shell tabs have no status row
and use the whole height; every tab keeps its size when you switch tabs:

```text
ctx  42%  5h  12%  7d  30%  $  1.23  3 files changed · Ctrl+Shift+D
```

Context use, 5-hour and 7-day limits, cost and files changed, in greys;
colour 3 (brown/amber) from 80 %, colour 9 (red) from 95 %. "esc to interrupt" appears at the right
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
scroll; `m` or Tab for the next view — that file against git `HEAD`, then
the whole working tree against `HEAD` with untracked files (so changes made
by Bash show too) — `r` to read again; Esc or q (or Ctrl+Shift+D again) to
close. The git views read at most 300 files, 2 MiB each and 16 MiB in all,
within 3 s, and say what they left out. Files that look like
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

### Orchestrator: tasks and workers

A *task* is one piece of work you hand to one Claude Code *worker*. haetty
gives the worker its own copy of the repository (a git worktree on a new
branch), starts Claude Code there in a new tab with the task as its first
prompt, and shows how the worker is doing. It does not split work, merge
results or answer questions for you (DESIGN.md §8).

**Before you start.** Open a tab whose folder is inside the repository you
want to work on, on the branch the work should start from. The worker runs
`worker-command` from the config file (default `claude`, i.e. the Claude
Code on your `PATH`); add arguments there if you always want them, e.g.
`worker-command = claude --model opus`. haetty adds no permission flags:
what the worker may do without asking is what your own Claude Code
settings say (`~/.claude/settings.json`, including bypass mode or not).

**1. Make a task.** Ctrl+Shift+P, type `new task`, Enter. Seven questions
follow, each ended with Enter (Esc cancels; Enter alone skips an optional one):

1. *Task title* — short; it names the tab and the branch, e.g.
   `Fix the CRLF parser`.
2. *Description* — one line with what "done" means, e.g. `Handle \r\n in
   parser.rs; add a test; cargo test must pass.` More can be sent later
   (step 4).
3. *Scope* — optional: what the worker may touch, e.g. `parser.rs only`; blank: the whole repository.
4. *Related files* — optional, comma-separated: where the worker starts, e.g. `src/parser.rs, tests/crlf.rs`.
5. *Model* — optional: passed to Claude Code as `--model`, e.g. `opus`; blank: Claude Code's default.
6. *Effort* — optional: passed as `--effort`, e.g. `high`; blank: Claude Code's default.
7. *Repository and base* — prefilled with the active tab's repository and
   its current branch, e.g. `/home/you/src/proj main`. Change the branch at
   the end of the line if needed.

haetty then:

- creates the branch `task/t1-fix-the-crlf-parser` from the base and a
  worktree for it **beside** the repository:
  `/home/you/src/proj-tasks/t1-fix-the-crlf-parser` (your repository's own
  folder and files are not touched). It refuses when that branch or folder
  already exists, the base is not a branch or commit, or the folder is not
  in a git repository — the hint at the bottom says which;
- opens a tab running the worker in that folder, with the task text as
  Claude Code's first prompt;
- asks the worker, in that prompt, to end its final message with a report
  (below).

Claude Code may first ask whether you trust the new folder, and in bypass
mode whether you accept it — **answer these yourself**; the task is
submitted only after that, by Claude Code itself. haetty never types into
these questions.

The answers are written to a briefing file in haetty's own state folder, not in the repository: the title, the scope, the related files and when the task is done. The worker's first prompt points to that file, and removing the task deletes it. A task's model and effort are passed to Claude Code as launch arguments only; no settings file changes. Claude Code may ask whether it may read the briefing outside the worktree: answer that yourself, because the orchestrator never answers permission prompts.

When a worker's context has passed `clear-hint` percent of `context-window` tokens, its tab shows a dim `/clear`: use `/clear` at a task boundary, not in the middle of a change. haetty only shows the mark; it sends nothing.

**2. Watch the state.** The worker's tab name starts with a mark:

| Mark | State | Meaning |
|---|---|---|
| `…` | running | the worker is in a turn (between your/its prompt and its final message) |
| `✓` | verified | the final message's report says `STATUS: verified` **and** lists evidence |
| `✗` | blocked | the report says `STATUS: blocked` (it could not finish; `BLOCKERS` says why) |
| `◇` | needs review | the report says `needs_review`, or `verified` without any evidence |
| `–` | no report | no turn finished yet, or the last final message had no valid report |

The state comes only from the latest turn's final message (Claude Code's
Stop hook); haetty never guesses. A red `?` on the tab is Claude Code's own
question mark, as in any Claude Code tab: it waits for you. The status
panel (Ctrl+Shift+S, in a Claude Code tab) has a **workers** section: one
line per task (mark, id, title, state) and, for the active worker, the
first line of its `BLOCKERS` and `NEXT_ACTION`.

**The report** the worker is asked for, at the end of its final message:

```text
STATUS: verified
ARTIFACT: src/parser.rs, tests/crlf.rs (commit 3f2a1c0 on task/t1-fix-the-crlf-parser)
EVIDENCE: cargo test -> 41 passed; cargo clippy -> 0 warnings
BLOCKERS: none
NEXT_ACTION: review the branch and merge it into main
```

`STATUS` is exactly one of `verified`, `blocked`, `needs_review`. The full
report is in `haetty status --json` under `workers` (`state`, `status`,
`artifact`, `evidence`, `blockers`, `next_action`, the worktree, the
branch and the session).

**3. Look at the work.** In the worker tab, Ctrl+Shift+D shows its last
edit; `m` switches to that file against `HEAD` and to the whole working
tree. Or open a shell in the worktree folder (Ctrl+Shift+T in the worker
tab starts a tab there) and use git as usual.

**4. Send more instructions.** In the worker tab: Ctrl+Shift+P, `message
the worker`, type one line, Enter. It is sent as program input (one paste
and Enter) **only when the worker is ready**: it has finished a turn, it is
not working, and no question is waiting. Otherwise nothing is sent and the
hint says why (e.g. "the worker waits for your answer — the orchestrator
never answers"). You can always type into the worker tab yourself, as in
any Claude Code tab.

**5. Finish.** haetty merges nothing. Review the branch and merge it
yourself (e.g. `git merge task/t1-fix-the-crlf-parser` in your repository),
or leave it. To clean up: close the worker tab (Ctrl+Shift+W twice), then
Ctrl+Shift+P, `remove a task`, pick the task, Enter. haetty runs `git
worktree remove` **without** `--force`: a worktree with uncommitted or
untracked files is kept and the hint shows git's reason. The branch is never
deleted. While the worker tab is open, removing is refused.

**Safety boundary.** Everything the orchestrator sends to a worker is
marked as program input, never as yours. The daemon refuses program input
while Claude Code waits for a permission answer, and the orchestrator does
not even try until the worker has finished a turn and is idle. So the
orchestrator never answers a permission prompt, a folder-trust question or
any other dialog — those stay with you. It does not change Claude Code's
settings or trust records either.

**Restarting.** Worker tabs are saved in tab sets like other tabs; after a
restart the worker's conversation is resumed (`--resume`) without the task
being submitted again. Tasks are files in the `tasks` folder of the state
directory (`haetty status --json` lists them even when no window is open).

### Saving mode (opt-in)

With `saving-mode = true` (off by default), the Claude Code sessions haetty starts get two output limits in their
environment: `BASH_MAX_OUTPUT_LENGTH=12000` (Bash output read back into a result) and `MAX_MCP_OUTPUT_TOKENS=12000`.
The status row of a Claude Code tab shows a dim `◆ saving` while it is on. The official meaning of both variables,
the basis of the values in the token measurement, and the caveat that a `bashOutputMaxChars` setting overrides the
first are in `docs/tokens/README.md` (section "Saving mode").

### Quiet runs and run logs (`haetty run`, `haetty log`)

`haetty run -- COMMAND [ARGS...]` runs a command with an empty standard input and keeps its output in a run log under
the state folder (`runs/`). The terminal gets the command's own exit code, the time, the errors and warnings, and the
last ten lines. `haetty log` reads a log in parts: `--grep TEXT`, `--lines A:B`, `--tail N` (the last 50 lines by
default); without a file it lists the runs, newest first.

The logs are kept for 14 days and at most 50 of them; each run removes the older ones. **A log holds everything the
command printed and its command line:** a password or a token that a program prints, or takes as an argument, is in
the log. Delete the `runs/` folder when you no longer need the logs.

### Waiting (`haetty wait`)

`haetty wait` waits for one thing and prints one line: `pid PID` (a process ends); `file PATH --contains TEXT` (the file
has the text, counting from its start); `file PATH --changed` (the file appears, or its size or time changes);
`screen SESSION --contains TEXT`; and `event SESSION --stop` or `--permission` (an observable tab's turn ends, or it waits
for a permission answer). It exits 0 when the condition holds, 124 when `--timeout` (600 seconds by default) passes first,
and 3 when the tab is not observable: the same rule as `haetty screen` and `haetty events`. Waiting never answers a
permission prompt.

An event wait counts only what comes after it starts, so start it before the prompt is sent (`--permission` counts at once when the tab is asking now). `--stop` also counts a turn that ended with an error (`StopFailure`). A tab without Claude Code's integration is refused with exit 1, and a session that exits or is closed ends the wait with exit 1.

### Observing a tab (read-only)

Other programs — a script, later a remote adapter (DESIGN.md §6) — can
watch a tab without being able to type into it, and only a tab you allowed.

- Allow it: palette "Observing: allow or stop for this tab" (the same entry
  stops it), or `observe-new-tabs = true` for every new tab. An observable
  tab shows a dim `◦` after its number in the tab bar. The permission
  belongs to the running session: it survives detaching and reattaching,
  but not a restart from a tab set (allow it again).
- `haetty screen [--json] SESSION`: the screen text and the state — title,
  folder, program, size, cursor and, for Claude Code tabs, its attention
  mark, busy, turns and the last worker report's status. A tab that is not
  observable is refused (exit 3). Session ids come from `haetty status`.
- `haetty events [--json] [--since CURSOR] [--follow]`: haetty's event log
  (session started, exited, closed, attention changes, latency summaries,
  observe on/off) for observable tabs only, and only from when they were
  observable. One line per event (`--json`: one JSON object, with its
  `seq`); `--since` continues after a `seq`; `--follow` keeps printing new
  events until the daemon ends.
- Never in either: what you type, prompts, tool input, transcripts or
  credentials. `events` has no screen contents at all.
- Both connect as an observer: they cannot send keys, paste, resize, close
  a tab or allow observing. That is checked by the daemon and by the client
  type they are built on. There is no network listener.

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
| `HAETTY_TITLE_BUTTONS` | integrated title row: window buttons in GNOME's `button-layout` syntax (`appmenu:minimize,maximize,close`, `close,minimize,maximize:`), instead of the desktop's setting |
| `HAETTY_DEBUG_PADDING` | `1` paints everything outside the grid red, to tell haetty's padding from a window manager's frame |
| `HAETTY_SOCKET`, `HAETTY_STATE_DIR`, `HAETTY_CONFIG_DIR` | for tests: override the daemon socket / pipe, the state directory and the config directory |

`HAETTY_SESSION_ID` (and `HAETTY_INSTANCE`) are set by the daemon in every
session's child, so that `haetty hook` and `haetty statusline` reach the
right daemon and tab.

### Known limits

- If haetty's daemon dies (killed, crashed), its programs end with it (on
  Linux they get SIGHUP from the closed terminal, on Windows the pseudo
  console closes); the window stays with a hint, the tab sets are not
  touched, and the next start reopens the tabs (Claude Code conversations
  resumed).
- `decorations = integrated` is Linux only; on Windows it means `native`.
  The integrated title row has no window menu (right click does nothing).
- Tab sets are not per window: with several haetty windows open, the last
  one to write wins the `last` set.
- The diff view shows the last edit only (no diff against `HEAD`, no file
  tree); NotebookEdit edits record the path only.
- Windows: a hook process takes about 64 ms on the test machine (process
  start alone is about 52 ms), so the 50 ms target is not met there. Only
  the edit hook (PreToolUse for Write/Edit/MultiEdit/NotebookEdit) makes
  Claude Code wait for it.
- Windows: Claude Code runs the status line through PowerShell when Git
  Bash is missing; haetty then passes a path without spaces (8.3 short path
  or a copy, DESIGN.md §3.13). A machine without Git Bash was not
  available for testing (HANDOFF.md §6.1, check 23).
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
- `haetty wait` on Windows opens the process at each check, so a process ID reused in the meantime makes the wait run
  to its timeout (600 seconds by default).
- An event wait (`haetty wait event`) counts the events written after its subscription, which takes a few
  milliseconds after the command starts; an event in that gap is missed, and the wait then runs to its timeout.

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
child waiter thread ─Exited─▶   (Terminal, RenderState, encoders)
frontend ──Key/Mouse/Paste/Resize/Snapshot──▶ engine ──Snapshot (plain data)──▶ frontend
engine ──bytes for the child (in order)──▶ PTY writer thread
```

libghostty-vt objects are `!Send`, so they never leave the engine thread.
Every byte to the child (query replies and user input) leaves the engine in
order through one queue to the PTY writer thread, so the engine never waits
for a program that does not read its input: query replies beyond 64 KiB
waiting are dropped, input beyond 64 MiB is dropped with an error event.
Grid sizes are clamped to 4096 x 2048. A dropped session stops without
working through queued output. A frontend receives `SessionEvent::Wakeup` at most once until
it takes a snapshot, and no wakeup is sent while the application holds
synchronized output (mode 2026), up to `sync_timeout` (500 ms default).
Over the socket the daemon sends a window the rows that changed since its
last frame, and the next frame only after the window asked for it.

## Testing

`cargo test` runs the unit and integration tests; `scripts/gate.sh` the full
gate (both targets, clippy, tests, sizes). The end-to-end checks drive the
real GUI and daemon under Xvfb (and on Windows): see
[`scripts/e2e/README.md`](scripts/e2e/README.md). All of them write only in a
test directory (`HAETTY_TEST_TMP`) and use their own `HAETTY_INSTANCE=dev`
daemon.

Every profile builds libghostty-vt optimized (`scripts/zig-mode.sh` shows the
Zig mode per profile). To run the engine fuzzing with Zig's safety checks on:

```sh
LIBGHOSTTY_VT_SYS_OPTIMIZE=ReleaseSafe CARGO_TARGET_DIR=/tmp/safe-target \
  HAETTY_VT_FUZZ=full HAETTY_TEST_TMP=/tmp/haetty-test \
  tools/heavy.sh cargo test --release -p haetty-core --test vtfuzz
```

`scripts/e2e/measure.sh [dist|release]` measures the DESIGN.md §3.4 budgets.

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

## Installing a stable haetty

A stable haetty is installed from a commit or tag of this repository with
`tools/install-stable.sh` (Linux) or `tools/install-stable.ps1` (Windows).
Each builds the given revision in its own temporary `git worktree` with the
`dist` profile (your checkout is not touched), installs it into a folder of
its own that is never changed afterwards, and points one fixed path at it.
Earlier versions stay installed, so going back is a matter of re-pointing.
Nothing running is stopped: windows of the previous version keep running
on it; the new version starts once all haetty windows are closed and the
old daemon has exited (about 10 s later).

`haetty --version` tells which build runs:

```text
haetty 0.1.0 (commit 0a1b2c3d4e5f, built 2026-11-06, instance stable)
```

(`HAETTY_INSTANCE=dev haetty --version` says `instance dev`; the date is
UTC.)

### Linux

```sh
cd ~/projects/claude-code-config/haetty
tools/install-stable.sh v0.1.0          # a tag or a commit
tools/install-stable.sh --list          # installed versions, * = active
tools/install-stable.sh --activate 0.1.0-0a1b2c3d4e5f   # roll back / forward
```

- Installs into `~/.local/opt/haetty/<version>/` (`<version>` =
  `<crate version>-<12-digit commit>`): `haetty`, licence files and an
  `INSTALLED` record, directory and files read-only (0555).
- `~/.local/bin/haetty` is a symbolic link to the active version
  (`~/.local/bin` must be on `PATH`); the switch is atomic.
- Runs `haetty install-desktop` through that link, so the menu entry's
  `Exec=` is `~/.local/bin/haetty` and follows updates and rollbacks.
- Prints `haetty --version` of the installed build and checks the commit.
- Needs the build tools of [Building](#building) (Rust, Zig 0.15.2); the
  cargo cache is `haetty/target/stable-install` (kept between installs, so
  the next install builds only what changed).
- Rolling back by hand: `ln -sfn ~/.local/opt/haetty/<version>/haetty
  ~/.local/bin/haetty`.
- Removing an old version: `chmod -R u+w ~/.local/opt/haetty/<version> &&
  rm -r ~/.local/opt/haetty/<version>` (never the active one).

### Windows

Run from PowerShell; the script lives in the repository inside WSL and
builds there (cross-build for `x86_64-pc-windows-gnu`, as in
[Building](#building) — native Windows has no Zig):

```powershell
pwsh -ExecutionPolicy Bypass -File \\wsl.localhost\Ubuntu\home\<you>\projects\claude-code-config\haetty\tools\install-stable.ps1 v0.1.0
... install-stable.ps1 -List
... install-stable.ps1 -Activate 0.1.0-0a1b2c3d4e5f     # roll back / forward
```

- Installs into `%LOCALAPPDATA%\Programs\haetty\<version>\`, every file
  with the read-only attribute.
- `%LOCALAPPDATA%\Programs\haetty\current` is a directory junction to the
  active version — the one fixed path: the installer adds it to the user
  `PATH` (`-NoPath` to skip) and creates the Start menu shortcut "haetty" →
  `current\haetty.exe` (`-NoShortcut` to skip). Pin haetty from the Start
  menu or from a running window: the pin keeps `current\haetty.exe`, so it
  follows updates and rollbacks (the AppUserModelID is the same in every
  version).
- Rolling back: `-Activate <version>` re-points the junction (by hand:
  `cmd /c rmdir %LOCALAPPDATA%\Programs\haetty\current` then
  `New-Item -ItemType Junction -Path …\current -Target …\<version>`;
  `rmdir` removes the junction only, never the version folder).
- Removing an old version: clear the read-only attribute
  (`attrib -R …\<version>\*`) and delete the folder.
- The read-only attribute keeps editors and casual overwrites off; a
  process of yours can still clear it. Note: after an update, open tabs of
  the old version keep their hooks on the old executable path only on
  Linux; on Windows they are expected to run `current\haetty.exe` (the new
  version; not verified),
  which an old daemon of another protocol version refuses — the status row
  of those old tabs stops updating until haetty is restarted.

### Protecting the stable build from Claude Code

At the end both scripts print (and change nothing) a suggestion for your
Claude Code settings with this machine's real paths: `permissions.deny`
rules against killing haetty by name and against Claude's file tools
writing into the install folders, and
`permissions.blockWritesOutsideWorkingDirectories`. Deny rules hold in every
permission mode, `bypassPermissions` included; they match command text only,
and `blockWritesOutsideWorkingDirectories` also removes Claude Code's
`EnterWorktree` tool. Copy what you want into `~/.claude/settings.json`
yourself.

### Testing the installers

`tools/test-install-stable.sh REF1 REF2` and `tools/test-install-stable.ps1
REF1 REF2` install two revisions into a prefix inside a test run directory
(`HAETTY_TEST_TMP`, as every test) and check layout, read-only files, the
link or junction, `--version`, the desktop entry or shortcut, rollback and
reinstall; with `HAETTY_TEST_TMP` set, the installers refuse any real
prefix before building.

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
Code config (see `docs/history/2026-11-04-05-milestone-1.md`, old HANDOFF §3) and pass with one, run through
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
