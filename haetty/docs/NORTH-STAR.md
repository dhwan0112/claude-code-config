# North star

What haetty is for, set by the person on 2026-11-12. Every stage says in its report, in one line, which of the four
points it moves forward. The stage list (P0–P9) is in [HANDOFF.md](HANDOFF.md), "Next session".

## The four points

1. **UI and UX after OpenCode's full TUI IDE.** What the person liked most there: session management, the ease of
   connecting other tools, the sidebar, and the side-by-side diff seen right next to the chat.
2. **SSH and Tailscale made easy.**
3. **Optimised for Claude Code**, above all for many jobs at once (orchestration).
4. **Working with other tools**, Rakazo and others.

## Conditions that stay

- Windows and Linux both (`CLAUDE.md`, "Hard requirements").
- Light; standalone (one executable per platform); Rust.
- A plain look without extras (DESIGN §2).
- Claude Code's behaviour is never changed behind the person's back.
- No credentials handled: no Claude tokens, no SSH keys, passwords or passphrases.

## How the person's decisions are taken

- A rule written in the stage text is followed as written; where a choice is open, the option that serves the four
  points best is taken and the reason is recorded.
- The person is asked only for a protected-area change outside the scope they approved, and for large choices of the
  look.
- What can only be checked on Windows does not block a stage: it is marked "Windows unverified" and added to the
  Windows check list in [dev-commands.md](dev-commands.md).

## Where each point stands (2026-11-12)

Drawn from HANDOFF, the README, DESIGN and the history records up to `cd2e0e7`.

| Point | Done | Not done | Next step |
|---|---|---|---|
| 1. OpenCode-like UI | Tabs and tab sets; closed tabs reopen with `--resume` (one at a time); detach and attach; `repo:branch` tab names and renames; palette (Ctrl+Shift+P); status row (context, 5h/7d, cost) and a status panel on the right (Ctrl+Shift+S, opening it resizes the program); a diff view drawn over the tab (Ctrl+Shift+D: last edit, against `HEAD`, working tree; side by side from 110 columns) | A sidebar shown from the start; a session list across tabs and projects (only "reopen a closed tab" now); naming, pinning, forking sessions; a tool-connection screen; changed-file list per conversation; diff opened from a file list, moving between files and hunks | P3: sidebar and side-by-side diff overlay (research OpenCode first); P4 session browser; P5 connections |
| 2. SSH and Tailscale | Nothing in haetty: a shell tab can run `ssh` by hand. DESIGN §6 names Tailscale only as the reach of a later MCP adapter | Host list from `~/.ssh/config` and `tailscale status`; "SSH: connect"; truecolor over SSH; reconnect on drop; remote daemon | P6: hosts and connect through the system OpenSSH; P7: remote daemon design |
| 3. Claude Code, many jobs | Hooks, status line and transcripts observed (no credentials); attention marks and the input hold during a permission prompt (K1; the turn's end releases it, a click answers: P1b); orchestrator: tasks, workers in worktrees, STATUS reports, worker marks (S3); `haetty run`, `log`, `wait`, `tokens`; saving mode; observe-only interface (V3) | A coordinating session that hands stages to workers and reads only reports, tried for real (OPERATING "Not done yet"); a key to the worker that waited longest; a worker list in a sidebar | P2: run two small stages through workers in haetty, compare with `haetty tokens` |
| 4. Other tools (Rakazo …) | Observe-only interface: `haetty screen`, `haetty events`, per-tab observing permission, observe role checked by the daemon (V3, DESIGN §6.1) | An MCP server; registering haetty in Claude Code; an HTTPS bridge for tools that only take HTTPS (Rakazo); any input from another tool (delegation) | P5: `haetty mcp` (stdio, read-only) and `haetty mcp install`; P8: delegation design |
| Conditions | Linux and Windows builds in every gate; one executable; ~0 % idle CPU and a few MB per process; stable/dev instances; night runs | A tag since v0.2.0: v0.3.0 and v0.3.1 were held by the Windows night run's focus check (an environment problem) | P1a (2026-11-12): tag rule without the Windows night as a blocker, done; P1c: v0.3.2 |

## Put on hold

These do not move any of the four points directly, so they wait (HANDOFF lists them): closing the remaining
differences between the native engine and the newest Ghostty (F2), switching the default engine, splitting the big
files (H2), and checking the default engine's slow memory growth.
