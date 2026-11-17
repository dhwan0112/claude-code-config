# haetty design: the sidebar, the diff overlay and the IDE prototype (section 13, stage P3)

Stage P3 (NORTH-STAR point 1; the person's text in [STAGES.md](../STAGES.md), "P3"). DESIGN §2 change approved for
that scope. This section holds the research (13.1, 13.2), the decisions taken from it (13.3–13.5) and the plan of the
implementation sessions (13.6). Written in the research session of 2026-11-17 (`docs/history/2026-11-17-p3-research.md`);
§2's own text changes in the implementation commit, together with the reference frames.

## 13.1 Research: OpenCode's sidebar and diffs

Read from OpenCode's source, release **v1.19.5** (npm `latest`, 2026-10-28, commit `810c0eba`; the TUI in
`packages/tui/src/`), with the unreleased V2 (`dev`, `df46f69`) and opencode.ai/docs for comparison. No screenshots:
every number is from the code. Files: `routes/session/index.tsx`, `routes/session/sidebar.tsx`,
`feature-plugins/sidebar/*.tsx`, `feature-plugins/system/diff-viewer.tsx`, `config/keybind.ts`.

**Sidebar**

- Right side, **42 columns** fixed (padding 2 left, 2 right, 1 top and bottom: about 38 for content). V2:
  `SESSION_SIDEBAR_WIDTH = 42`.
- Shown when the terminal is **wider than 120 columns** (`wide = width > 120`); always hidden in subagent sessions.
- No border: a different background (`backgroundPanel`). Section titles bold in the text colour, items dim
  (`textMuted`), one blank line between sections. Sections with more than two items fold by a mouse click (▼/▶,
  not kept).
- Order: **session title** (bold) → **Context** (`N tokens`, `N% used`, `$X.XX spent`) → **MCP** (`• name status`,
  only when there are servers) → **LSP** → **Todo** (only while one is open) → **Modified Files** (path cut from the
  left, `+a` / `-d` right-aligned in the diff colours; the rows open nothing) → footer pinned at the bottom:
  `~/parent/name:branch` and the version. V2 drops LSP and Modified Files (the changed files move to the diff viewer)
  and adds a usage-limit section.
- Narrow (≤ 120): hidden; the toggle (`ctrl+x b`, or the palette) shows it **over** the chat at the right edge (the
  chat keeps its layout) on a translucent dark scrim. Setting `auto | hide` (V1: kept in the TUI's state; V2: config
  `session.sidebar`).

**Diffs**

- In the chat (tool blocks): `diff_style = auto | stacked`; `auto` is side by side when the chat is wider than 120
  columns, else unified ("stacked").
- The diff viewer (`/diff` or the palette; no default key): covers the whole terminal; a file tree on the left
  (32 columns, `M`/`A`/`D`, ✓ reviewed), the patch on the right with a `path +a -d` header per file. Side by side
  when the patch pane has **at least 100 columns** (`MIN_SPLIT_WIDTH = 100`), else unified; `v` switches. Sources:
  working tree, main branch, last turn (`d`). **Not opened from the sidebar** in v1.19.5.
- Viewer keys: `esc`/`q` close; `j`/`k`, PageUp/PageDown scroll; **`]`/`[` next/previous hunk; `n`/`p`
  next/previous file**; `tab` tree or patch; `b` tree; `s` one file; `d` source; `v` split or unified; `m` reviewed;
  `?` help. V2 adds `alt+↓/↑` for files and `diffs.view = auto | split | unified`.

## 13.2 Research: Claude Code's editor (IDE) integration

From Claude Code 2.1.326 (current on 2026-11-17). **Official** = code.claude.com docs (VS Code and JetBrains pages,
"The built-in IDE MCP server"; env-vars; settings; CLI reference). **Unofficial** = coder/claudecode.nvim v0.4.0
(`PROTOCOL.md`, `lua/claudecode/`), manzaltu/claude-code-ide.el, and strings read from the shipped CLI package
(`@anthropic-ai/claude-code-linux-x64@2.1.326`, nothing run). The full notes with every URL: the history record.

| | Official | Unofficial (third parties, the shipped CLI) |
|---|---|---|
| Transport | The editor runs an MCP server named `ide`, plain `ws://`, VS Code on `127.0.0.1`, random port 10000–65535 | JSON-RPC 2.0, one message per text frame; the CLI asks for subprotocol `mcp`; editors answer MCP `2024-11-05` (the CLI takes 2024-10-07 … 2025-11-25) |
| Discovery | Lock file `~/.claude/ide/<port>.lock` (or `$CLAUDE_CONFIG_DIR/ide/`), mode 0600 in a 0700 folder; fields not documented | `{pid, workspaceFolders, ideName, transport: "ws", authToken}`; the port is the file name. `CLAUDE_CODE_SSE_PORT=<port>` in `claude`'s environment makes it connect at start, and a lock with that port counts as valid without the folder and parent-process checks. `ENABLE_IDE_INTEGRATION` is no longer read |
| Auth | A new random token per editor start, sent as `X-Claude-Code-Ide-Authorization` | The CLI sends the header only when the lock has `authToken`; a 128-bit hex token (claudecode.nvim) |
| Tools | Only `getDiagnostics` and `executeCode` reach the model; the rest is "internal RPC" (diffs, selections, saving), not named | `openDiff {old_file_path, new_file_path, new_file_contents, tab_name}`, `close_tab`, `closeAllDiffTabs` (each user turn), `getDiagnostics`, `openFile`, selections … |
| Settings | `diffTool` (`auto` default: Edit/Write diffs in the connected IDE; `terminal`), `autoConnectIde`, `CLAUDE_CODE_AUTO_CONNECT_IDE`, `--ide`, `/ide` | — |

`openDiff` (unofficial, from the shipped CLI): sent only for an Edit/Write permission prompt with `diffTool = auto`; the
call **blocks** while Claude Code's own permission dialog stays open — whichever is answered first wins, and the CLI
then sends `close_tab`. Answers: `FILE_SAVED` + the final text as a second item (accept; edits made in the diff are
kept; **the CLI writes the file**, the editor never does), `DIFF_REJECTED` (reject), **`TAB_CLOSED` = accept the
proposal unchanged**, anything else = not accepted (the dialog stays). An accepted text equal to the old file counts
as a rejection.

Risks: the tool names and answers are not documented and may change in any release; a server that restarts on a new
port leaves a running `claude` with a stale `CLAUDE_CODE_SSE_PORT`; CVE-2025-52882 (an unauthenticated editor socket)
is why the token exists.

## 13.3 Decisions: the sidebar

Taken by the rule "the option that serves the four points best, reason recorded" (NORTH-STAR). Proposed, not yet
built; the implementation commit carries the §2 text and the frames.

1. **When it shows.** Setting `sidebar = auto | hide` (default `auto`). `auto`: shown on the right whenever the
   window grid is at least **`sidebar-min-cols` = 121** columns (OpenCode's `> 120`), from the moment a tab opens.
   It is the same for every tab, shell tabs too, so switching tabs never changes a terminal's size, and only a resize
   by the person (or the person's toggle) adds or removes it. Reason: §2.1 "events never change the layout" and the
   stage text "a tab's terminal size never changes because of an event".
2. **Width.** `sidebar-width` = **42** columns including the separator column (OpenCode's 42), range 24–80 as today's
   panel; Ctrl+Shift+Alt+Left/Right (today's wider/narrower) change it. At 121 columns the terminal keeps 79.
3. **Toggle.** Ctrl+Shift+S (today's status panel key; the config name `status_panel` stays as an alias of
   `sidebar`). Wide window: hides or shows the docked sidebar (the person's input, so the terminal resize is allowed),
   kept in the ui state file. Narrow window: shows it **as an overlay** at the right edge over the terminal, which
   keeps its size; the same key or Esc hides it. No scrim (§2.2: no shadows), the overlay has the terminal's
   background and the 1 px line.
4. **Look.** No boxes or borders; the terminal's background; a 1 px low-contrast line between terminal and sidebar
   (today's separator glyph, drawn by `boxdraw`); section titles one step brighter (`strong`), items `normal` or
   `dim`; one blank row between sections; accent colours only for state (waiting = 3, permission or ≥ 80 % = 9). The
   `+a -d` counts are **not** tinted (an idle screen with changed files would otherwise show accent colours; §2.3).
5. **Status row.** While the sidebar is docked a Claude Code tab has no status row (the sidebar shows all of it), so
   its terminal gets that row back; hidden or narrow: today's one line. The first grid of a tab is chosen from the
   window at its opening, so no tab changes rows by itself.
6. **Contents, Claude Code tab** (top to bottom; an empty section is not drawn):
   - the session name (`strong`), the session id's first 8 characters (`dim`);
   - **Context**: `context 34% of 200k`, `5h limit 12% · 14:00`, `7d limit …`, `cost $1.23` (`(API estimate)` as
     today), cache, the tokens line (`token-status`);
   - **Files** `(n)`: one row per file changed in this conversation, the path cut from the left (`…/src/app.rs`) and
     `+a -d` right-aligned; the most recent first; a click (or Enter in the overlay's list) opens the diff overlay on
     that file;
   - **MCP**: empty until P5;
   - **Subagents**: running ● and done ✓, as in today's panel;
   - **Workers**: orchestrator tasks with their glyph, blockers, next (today's panel);
   - footer at the bottom: `~/parent/name:branch` (`dim` path, `normal` name), `updated 12s ago` (`dim`).
7. **Contents, shell tab**: the working directory (OSC 7, else the start folder), the git branch, a change summary
   (`3 files changed +40 -12` against `HEAD`, working tree with untracked files) and then the files as above. The
   summary is collected off the UI thread with `gitview`'s limits, when the tab's directory changes, on window focus
   and at most every 10 s while the tab is shown; never for hidden tabs. SSH tab: the host (P6).
8. **Data.** `ClaudeInfo` gains `files: Vec<FileStat {path, added, removed, edits, at_ms}>` (haetty-cc already keeps
   the set of paths; the counts are the sums of the edits' counts, which costs no extra memory — the overlay's
   "vs HEAD" view shows the net change). Protocol 12 → 13, the field `#[serde(default)]`.

## 13.4 Decisions: the diff overlay

1. Replaces today's diff view (`diffview.rs` grows into it; the three kinds stay: last edit, vs HEAD, working tree;
   `m`/Tab cycles).
2. Drawn **over the terminal area** without resizing it. With the sidebar docked the sidebar stays beside it and is
   the file list (the shown file marked); narrow, the overlay covers the window's terminal area and its header names
   the file (`2/7 src/app.rs`).
3. **Side by side** when the overlay is at least **`diff-split-cols` = 100** columns wide (OpenCode's
   `MIN_SPLIT_WIDTH`; today 110), else **top and bottom** (removed lines above the added ones in each hunk — the
   "stacked" form). Setting `diff-view = auto | split | stacked`.
4. **Opening**: Ctrl+Shift+D (on the last edited file), a click on a file in the sidebar, the palette. Setting
   `diff-after-edit` (default **off**): when on, an edit by Claude Code in the active tab opens the overlay on that
   file (an opt-in exception to §2.1, chosen by the person in the settings panel).
5. **Keys** (OpenCode's where they exist): `n`/`p` next/previous file, `]`/`[` next/previous hunk, `j`/`k` and the
   arrows, Space/PageDown, `b`/PageUp, `g`/`G`, `m`/Tab next view kind, `r` reread, `v` split or stacked, Esc or `q`
   close. Files in the list: the conversation's changed files (last edit, vs HEAD) or git's changed files (working
   tree).
6. The overlay never sends anything to the program; while it is open, keys go to it, as today.

## 13.5 Decisions: the IDE prototype (optional, off by default)

Setting `ide-integration = false`. Only when it is on:

1. **Where the server lives: the daemon**, one WebSocket server per Claude Code session on `127.0.0.1`, a random
   port in 10000–65535, kept for the session's whole life (sessions outlive the GUI, and a running `claude` cannot
   learn a new port). A new 128-bit token per session (`getrandom`, already in the tree).
2. **Discovery**: the session is started with `CLAUDE_CODE_SSE_PORT=<port>` (the port match makes Claude Code connect
   at start without the folder and parent checks); the lock file `<claude-config>/ide/<port>.lock` (0600, folder
   0700) with `workspaceFolders: []`, so that no other `claude` in the same folder picks it up; written at session
   start, removed at its end (and stale haetty locks — `ideName: "haetty"`, dead pid — removed at daemon start). The
   tests' isolated home holds it (`CLAUDE_CONFIG_DIR`).
3. **Protocol** (unofficial, pinned to what 2.1.326 does): WebSocket upgrade with the token header checked and
   subprotocol `mcp` echoed; `initialize` answered with `2024-11-05`; `tools/list` = `openDiff`, `close_tab`,
   `closeAllDiffTabs`, `getDiagnostics` (empty); pings answered. A hand-written WebSocket (handshake with a small
   SHA-1 and the `base64` already in the tree, text/ping/pong/close frames only) and haetty-cc's JSON: no new
   dependency.
4. **`openDiff`**: the daemon passes the proposal (path, old text read by the daemon with the edit-snapshot limits,
   new text) to the attached GUI as a new wire message (protocol 13); the GUI opens the overlay on it with a header
   `proposed edit · a accept · d reject · Esc keep asking`. **Accept and reject only by the person's keys** (key
   events in the window, never `haetty run`, the orchestrator, `Origin::Program` or an observing client); the answer
   goes back over a Control-role message the daemon checks. Esc closes the overlay **without answering** (never
   `TAB_CLOSED`, which Claude Code takes as accept); Claude Code's own dialog stays and works as without the
   integration. `close_tab` (answered in the terminal) closes the overlay. No GUI attached: the request just waits.
5. **Consistent with K1**: the request comes with a permission dialog, so the tab already has the permission mark and
   the input hold; an accept or reject key counts as the person's input like a click (P1b) and clears the mark; the
   hold is released by the turn's end as today.
6. **Fallback**: if the server cannot bind, the lock cannot be written, or the connection never comes, nothing is
   shown: the hook-based diff works as before.
7. README: the protocol is unofficial and may break with any Claude Code release; turning it on changes the editing
   flow (Edit/Write proposals also show in haetty and can be answered there).

Protected: the permission-prompt path (§3.7, §3.13) and the role checks — the scope above is what the stage text
approved ("accept and reject only by the person's keys, consistent with the permission-wait mark and the input-hold
rules"); its commit says "protected area, approved".

## 13.6 Plan of the implementation sessions

One per session (quota), each gated and pushed:

1. **P3a sidebar**: settings `sidebar`, `sidebar-min-cols`, `sidebar-width`; layout (the term grid without the
   status row while docked), the narrow overlay, drawing (the panel code becomes the sidebar), shell-tab git summary;
   `ClaudeInfo.files` (protocol 13); unit tests (layout never changes on events, widths, cut paths); Xvfb e2e; §2.1 /
   §2.2 / §2.3 text and the reference frames ("DESIGN §2 change, approved"); frame dumps `docs/look/p3-*`: wide
   Claude Code tab, narrow with the overlay, shell tab.
2. **P3b diff overlay**: files and hunks, `n`/`p`/`]`/`[`/`v`, the sidebar's file list as navigation, click to open,
   `diff-split-cols`, `diff-view`, `diff-after-edit`; frame dumps: split, stacked, narrow.
3. **P3c IDE prototype** (protected scope above): the daemon's WebSocket server, lock files, the wire messages, the
   overlay's accept/reject, one short real Claude Code run in an isolated home (quota), README.
