# haetty design: updating the stable instance without closing the tabs (section 9, design only)

> Part of the design. Index: [`../DESIGN.md`](../DESIGN.md) (section
> numbers are the same there). Text below is moved verbatim from the former `docs/DESIGN.md`.

## 9. Updating the stable instance without closing the tabs (design only, 2026-11-07)

Written in stage V4; nothing here is implemented. Today a new stable
version runs only after every stable window is closed and its daemon has
exited (§3.15). The sessions live in that daemon, so updating means ending
every program, Claude Code turns in progress included. Once haetty is
developed inside haetty, that becomes the main cost of an update.

What has to survive an update, per session: the program and its children
(a Claude Code turn may be mid-tool-call), the PTY, the screen and
scrollback, the Claude Code state (`ClaudeInfo`, attention, the injected
`--settings` file whose hook command names the old binary), the tab's place
in the window, and the observe consent (§6.1).

### 9.1 (a) Old and new daemon side by side

The old daemon keeps its sessions until they end; the new one takes new
sessions; the GUI shows the tabs of both.
- Needs per-version endpoints (`haetty/daemon-<protocol>.sock`, the pipe
  likewise): today both would want the same name. The hooks of old
  sessions run the old binary (its path is in their `--settings`), which
  dials its own version's endpoint, so old sessions keep reporting to the
  old daemon.
- The GUI must speak both protocol versions. postcard decodes enum
  variants by index and cannot skip unknown ones (§3.6), so a new GUI needs
  the old version's message types side by side — a frozen copy of
  `wire.rs` per supported old version — or a second, old GUI process for
  the old tabs, which is no longer one window.
- Risks: two daemons' state (tab sets, `last`, the closed list, tasks)
  written by both; `haetty status` and observers must ask both; the old
  daemon's bugs stay alive as long as its longest session; protocol
  compatibility becomes a permanent obligation.
- Size: large (versioned endpoints, a multi-version client, GUI routing
  per tab, status and observer merging). Protected areas: the endpoint and
  role code is untouched and the injected settings do not change.
  Moderate risk of subtle bugs, permanent cost.

### 9.2 (b) Hand the sessions to the new daemon

The new daemon starts on a temporary endpoint and the old one passes it
every session. The new daemon then takes the main endpoint, the old one
exits, and the GUI reconnects (it already survives a lost daemon:
`Client::is_lost`).
- **Linux: file descriptors.** The PTY master goes over the Unix socket
  with `SCM_RIGHTS` (plain `sendmsg`, no new dependency; `libc` is already
  used). The child stays where it is. It is not the new daemon's child, so
  `waitpid` cannot report its exit status: the new daemon watches it with
  `pidfd_open` + `poll` (Linux 5.3+) and knows *that* it ended, not the
  code. The old daemon must not exit before the handover. When it exits,
  its children are reparented to init and keep running (the PTY master is
  held by the new daemon), but exit codes of handed-over sessions become
  "unknown".
- **Windows: ConPTY handles.** A pseudo console is three handles inside
  the HPCON: the OpenConsole process, the "reference" handle (keeps the
  ConDrv server open) and the signal pipe. The PTY input and output pipes
  and the client process handle come on top.
  - Checked in Microsoft's source (`src/winconpty/winconpty.cpp`,
    microsoft/terminal; the vendored `conpty.dll` 1.25 exports these
    names):
    - `ConptyPackPseudoConsole(hProcess, hRef, hSignal, &hpc)` only packs
      such loose handles into a new HPCON ("for an inbound ConPTY session
      … so resizing and closing just work"; Windows Terminal's
      default-terminal handoff uses it);
    - `ConptyReparentPseudoConsole(hpc, hwnd)` only writes
      `PTY_SIGNAL_REPARENT_WINDOW` with a new owner HWND for the pseudo
      window (what `GetConsoleWindow()` returns, GH#2988). It moves nothing
      between processes.
  - So the handover is: the old daemon `DuplicateHandle`s the five handles
    into the new daemon's process, and the new daemon calls
    `ConptyPackPseudoConsole` and goes on with resize and close as usual.
    This needs the side-loaded `conpty.dll` (the in-box `kernel32` has no
    Pack), which haetty ships already. Reparent is not needed (haetty's
    pseudo windows have no owner window today).
  - The client process handle gives the new daemon the exit code
    (`GetExitCodeProcess` works on any handle with query rights): unlike
    Linux, nothing is lost.
- **The screen state is the hard part.** The PTY carries no history: the
  terminal state (grid, scrollback, modes, cursor, charsets, tab stops,
  palette changes, kitty keyboard flags) lives only in the old daemon's
  engine. Options:
  1. **Serialize and replay.** libghostty-vt 0.2.2 has a formatter
     (`Formatter`, `FormatterOptions`: VT format, with palette, modes,
     scrolling region, tab stops, pwd, keyboard modes, cursor, style,
     hyperlinks, protection, kitty keyboard, charsets). The old daemon
     formats each session as VT bytes; the new daemon feeds them to a fresh
     engine before the PTY's next bytes.
     - To verify before relying on it: whether it includes the scrollback
       and both screens (primary and alternate).
     - Ordering, so nothing is lost: the old daemon stops reading the PTY
       first and formats second; the PTY buffers in the meantime.
     - The native engine (§10, planned) should have an exact state
       snapshot by design.
  2. **Ask for a redraw.** A resize (SIGWINCH / ConPTY resize) makes
     Claude Code's fullscreen renderer draw its whole screen again. That
     covers the visible screen of full-screen programs, not a shell's
     scrollback or a program that does not redraw.
  3. Both: replay, then a resize as a safety net for full-screen programs.
- Claude Code state: `ClaudeInfo` goes over as a message (plain data). The
  `--settings` file stays where it is. Its hook command still names the old
  binary; that binary stays (§3.15 keeps old versions, read-only), and its
  hooks reach the new daemon only while the endpoint name and the
  `Hook`/`StatusLine` messages stay compatible — or the new daemon also
  accepts the old protocol for Report clients only.
- Risks:
  - The handover window: both daemons alive, one session in neither. It
    must be short and atomic per session; a crash halfway must leave each
    session in exactly one daemon (one session at a time, acknowledged,
    rolled back on failure).
  - Lost exit codes on Linux.
  - The formatter's fidelity is not known yet.
- Size: medium to large (a handover protocol between daemons, fd and
  handle passing, replay, GUI reconnect, tests that update with sessions
  running, on both platforms).
- Protected areas: the handover is a new message family between two
  daemons of the same user. It needs its own role (a "Handover" client
  that may only send sessions, never input) — a change of the IPC role
  checks, to be reviewed. The permission-prompt path must survive intact:
  a session waiting for a permission answer keeps waiting, and only Human
  input answers it, in the new daemon as in the old.

### 9.3 (c) Close and resume, made smooth

Keep today's model, but make the update one action and lose as little as
possible.
- Palette "Update to the installed version". The GUI waits until no
  Claude Code tab is busy or asking (it knows `busy` and `attention`
  already). It then saves the tab set (`last`) with each tab's Claude Code
  session id, ends the sessions, and starts the new binary, which reopens
  the tabs. Claude Code tabs resume their conversations (`--resume`,
  already there); shells start again in their folders.
- What is lost: running shell programs and their scrollback, and a
  conversation's in-flight turn. Waiting for idle prevents the latter; a
  tab that never goes idle can be named and skipped.
- Optional: keep each tab's last screen as text (from the snapshot) and
  show it dimmed above the new prompt, so the person keeps the context.
- Risks: low; everything it uses exists. Size: small (one action, the
  wait, the restart). Protected areas: none (no input to sessions, no role
  change; `--resume` is the existing path).

### 9.4 Recommendation

1. **Now: (c).** Small, no protected area, and it removes most of the
   pain: updates happen when Claude Code is idle, and conversations come
   back.
2. **Later: (b), after the native engine (§10)** gives an exact state
   snapshot. On Windows, Pack and `DuplicateHandle` make the handover
   possible with what haetty ships; on Linux it costs only the exit codes
   of handed-over sessions. Before relying on the libghostty formatter,
   measure its fidelity with the differential tool of §10 (format, replay,
   compare cell by cell).
3. **Not (a).** Speaking several protocol versions for good costs more than
   a one-time handover, and old daemons would keep old bugs alive.
