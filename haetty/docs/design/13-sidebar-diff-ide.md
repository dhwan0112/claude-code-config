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

As built in stage PM (2026-11-19, workers of P2's real use plus the person's session; record
[history/2026-11-19-pm.md](../history/2026-11-19-pm.md)): items 1–6 as written, with these differences:

- Opening from the sidebar: a click on a file in the docked sidebar; Enter only in the sidebar over a narrow window
  (the docked sidebar takes no keys, they go to Claude Code). The palette's "Diff of the last edit" stays.
- `n`/`p` in the last-edit view: another file shows against `HEAD` (only the last edit has a snapshot); in the
  working-tree view they scroll to that file's title.
- `diff-after-edit` does not open the overlay in a turn that asked for a permission (`asking` stays set to the turn's
  end), nor over the palette, the settings panel or an agent's record.
- The header names the file relative to the tab's folder; in the working-tree view a file's first hunk starts at its
  title row, so `]` from the top goes to the second hunk.
- The sidebar marks the shown file (`›`, bold) while the overlay is open.

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

## 13.7 Stage PH: the harness screen as built (2026-11-17)

Stage PH (the person's text in [STAGES.md](../STAGES.md), "PH") took P3a in and grew the Agents section; record
`docs/history/2026-11-17-ph-harness.md` (H0 measurement, the ccx table). What differs from 13.3:

- **The proxy** (H1): every shell tab with the integration has `<run dir>/bin` first on `PATH` (Linux: `claude` →
  haetty; Windows: `claude.cmd` → `haetty claude-proxy`) and `HAETTY_CLAUDE_SETTINGS` (its settings file, written as a
  Claude Code tab's). `haetty-cc::proxy` runs the real claude with the same arguments plus `--settings` (merged with
  the user's), never for `-p`/`--bg`/`--version`/`--help`, a non-conversation subcommand, output that is not a
  terminal, or inside a proxied claude. The daemon makes the tab a Claude Code tab at its first report and a shell tab
  at `SessionEnd` (`ClaudeInfo.via_proxy`, `ended`); no status row is reserved for it. Tab sets keep its conversation;
  a restore types `claude --resume <id>` into the shell (`restore-claude`). A `claude` in the foreground without
  reports (Linux `/proc`: the terminal's `tpgid`) gets the sidebar line "claude is running without haetty's
  integration". The injected content is unchanged (protected area, approved).
- **Lead**: the session name, then model · effort (the stage text asks for them; §2.1's rule against what Claude Code
  shows was changed for this), context, limits, cost, lines, cache, tokens.
- **Agents** (replaces Subagents): from `haetty-cc::agents` — the hooks of H0 only (no new injected event): parent and
  depth from the parent's Agent tool call, model (`resolvedModel`), background (`async_launched`), fork (type `fork`),
  worktree (`tool_input.isolation`), current tool and target, calls, failed call, the agent's open question (shown
  only while the session's is), description, the STATUS of its last message; running first, 200 finished kept, 6
  shown. The record view (`agentview.rs`) reads the agent's transcript tail on a thread.
- **Log**: derived event-log lines (`agent_start`, `agent_end`, `edit` with the file name, `ask`, `turn_end`,
  `turn_failed`, `compact`), the latest 64 per session kept in the daemon's memory for `GetSessionLog`.
- **Queries** (protocol 14–15): `GetClaude` → `ClaudeState`, `GetSessionLog` → `SessionLog`, read-only on
  `ListSessions`' line of the role table; an observe-only client gets them only for observable sessions
  (`haetty agents`).
- **All agents** (H3): palette list of every tab's agents, the longest-waiting tab first (replaces P2's key to the
  longest-waiting worker).
- Measured on kramers: GUI idle CPU 0.0 % with the sidebar shown and no agents, 1.2 % with five agents running (a
  redraw of their elapsed times each second; only changed rows are drawn); the daemon 0.0 %. Stripped size +234 KB
  (Linux) / +239 KB (Windows) against v0.3.3, no new dependency.

## 13.8 Stage PI: shell integration (2026-11-20)

The person's `.zshrc` puts `~/.opencode/bin` and `~/.local/bin` before the proxy folder, so `claude` and `haetty`
typed in a tab were not the proxy's (PM's list, P1 and P2). The research behind the method (Ghostty, kitty) is in
`docs/history/2026-11-20-pi.md`. As built (protected area, approved: stage PI, I1):

- `shell-integration = auto | off` (default `auto`; panel group Claude Code); it needs the proxy, so
  `claude-integration = false` turns it off too. `CreateSpec.shell_integration` (protocol 17); the daemon decides in
  `proxy_env`, only for a program named `zsh` or `bash` with no arguments. Worker tabs and `haetty task new` run claude
  directly and are not shell tabs.
- zsh: the daemon writes `<run dir>/zsh/.zshenv` at start (`haetty-cc/src/shell/zshenv.zsh`, built into the
  executable) and gives the tab `ZDOTDIR=<run dir>/zsh`, plus `HAETTY_ZSH_ZDOTDIR` with the person's own value when
  they had one (an empty value counts as set). That file gives `ZDOTDIR` back (or unsets it), sources
  `${ZDOTDIR-$HOME}/.zshenv`, and in an interactive shell appends one function to `precmd_functions`; zsh then reads
  `.zprofile`, `.zshrc`, `.zlogin` from the person's `ZDOTDIR` in its own order. The hook compares the first `PATH`
  entry with `HAETTY_PROXY_DIR` and, when it differs, rebuilds `path` with the folder first and every other copy of it
  removed.
- bash: the tab gets `BASH_FUNC__haetty_path_first%%` (an exported function; bash imports it) and `PROMPT_COMMAND`
  with that name first (before a `PROMPT_COMMAND` the environment had). The function keeps `$?`. No `--posix`/`ENV`
  rewrite of bash's start (Ghostty's and kitty's way), because that means reading the person's startup files in
  haetty's order instead of bash's.
- "No shell at run time" (CLAUDE.md): haetty runs no shell script; the `.zshenv` and the bash function are startup
  settings that the person's own shell reads, like the `--settings` file Claude Code reads.
- Limits (README, "Claude Code integration"): `exec zsh` and nested shells inherit `PATH` but not the hook; a
  `.zshrc` that empties `precmd_functions` or a `.bashrc` that overwrites `PROMPT_COMMAND` removes it; a `ZDOTDIR` set
  in `/etc/zsh/zshenv` bypasses the folder; nothing inside `ssh`; Windows has no shell integration (the daemon makes
  no folder there).
- Checked by `tests/shell.rs` (a real daemon, real zsh and bash in isolated homes: the cases above, the startup files'
  order with and without, `which haetty`, `claude` through the proxy) and `scripts/e2e/sidebar.sh` (a zsh whose
  `.zshrc` puts `~/.local/bin` first). zsh to the first prompt: 36.21 → 36.45 ms median (200 runs each, isolated home).

## 13.9 Stage P3c: haetty as Claude Code's editor, as built (2026-11-20)

The prototype of 13.5, built in one session (record `docs/history/2026-11-20-p3c.md`). What differs from 13.5 or
was decided while building:

- **Where:** a server per session in the daemon (`haetty-daemon/src/ide.rs`), for a Claude Code tab **and** for a
  shell tab with the proxy (13.5 predates the proxy): the shell tab's port goes to the shell as `HAETTY_IDE_PORT`, and
  the proxy hands it to the real claude as `CLAUDE_CODE_SSE_PORT` only when it injects the integration; a Claude Code
  tab gets `CLAUDE_CODE_SSE_PORT` directly. The server lives as long as the tab, so a `claude` started again in the
  same shell tab connects to the same port. Only with `claude-integration` on; `ide-integration` takes effect for new
  tabs.
- **Protocol** (`haetty-cc/src/ide.rs`, unofficial, pinned to Claude Code 2.1.329 — its strings were checked in the
  installed CLI): the handshake needs `x-claude-code-ide-authorization` equal to the lock's token (compared without
  early exit) and answers `Sec-WebSocket-Protocol: mcp` when asked; text, ping/pong and close frames, continuations
  joined, at most 2 MiB per frame; `initialize` → `2024-11-05`; `tools/list` → `openDiff`, `close_tab`,
  `closeAllDiffTabs`, `getDiagnostics` (empty); `prompts/list` and `resources/list` empty; `ping`; unknown methods
  −32601. No new dependency: SHA-1 and base64 are written out (tested against the FIPS 180 and RFC 4648 vectors and
  RFC 6455's example key). The token is 16 bytes from `/dev/urandom`.
- **Lock file:** `<CLAUDE_CONFIG_DIR or ~/.claude>/ide/<port>.lock`, 0600 (the folder 0700 when haetty makes it),
  `{pid: daemon, workspaceFolders: [], ideName: "haetty", transport: "ws", authToken}`; the config folder is the
  session's own (`spec.env`, else the daemon's environment). Locks of haetty whose daemon PID is gone are removed when
  a server starts (Linux: `/proc`). Removed when the session exits or closes.
- **Proposals:** `openDiff` → the daemon reads the old text with the edit snapshot's rules (credential-like, over
  2 MiB or binary: not shown, never answered — Claude Code's own dialog answers) and keeps it waiting; `DaemonMsg::Ide`
  (protocol 18) tells the session's Control subscribers the oldest waiting proposal (on attach and on every change).
  `ClientMsg::IdeAnswer` is Control only (the role table) and `Origin::Human` only (`Denied` otherwise); it counts as
  the person's input (K1: the mark goes, the hold waits for the turn's end). Accept → `FILE_SAVED` and the proposed
  text, reject → `DIFF_REJECTED` and the tab name; `TAB_CLOSED` (Claude Code's accept) is never sent for a proposal.
  `close_tab`, `closeAllDiffTabs` and a closed connection drop proposals.
- **GUI:** the active tab's new proposal opens the diff overlay (§2.1, approved) with `proposed edit  <path>  +a -d`
  and `a accept · d reject · esc keep asking`; Esc closes it and it does not open again for that proposal; the other
  views are off while it shows. Not over the palette, the settings panel or an agent's record (it opens at the next
  update after they close).
- **Not done:** Windows (no server: the prototype's random source is `/dev/urandom`; PW), `openFile`, selections and
  `at_mentioned`, diagnostics, editing the proposal before accepting (`FILE_SAVED` carries the proposal unchanged),
  more than one waiting proposal shown (the oldest first), WebSocket pings from the server.
- Checked by `tests/ide.rs` (a real daemon; this test as Claude Code: the token, the lock, the answers, a program's and
  an observer's answer refused, the mark and the hold, the proxy's port, off), `scripts/e2e/ide.sh` (Xvfb, 15 checks)
  and one real Claude Code 2.1.329 run (Haiku, a dev instance on a private Xvfb): `a` → Claude Code wrote the edit,
  `d` → "Rejected update to a.txt".
