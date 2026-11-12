# Operating a stage: one stage per session

How haetty's stages run (stage H3, 2026-11-09). A stage is one change, from the in-progress line in HANDOFF to the
pushed commit on main. Run each stage in its own session: a session that starts from HANDOFF reads less than one that
has carried many stages (the token method: [tokens/per-stage-method.md](tokens/per-stage-method.md)).

## The stage scripts

All in `tools/stage/` (they were in the session folders until H3, so a new session could not find them):

| script | what it does |
|---|---|
| `stage.sh start NAME "remaining work"` | on main: the in-progress line in HANDOFF (gated, pushed), then the worktree `$HAETTY_WTROOT/NAME` (`~/projects/claude-code-config-wt/NAME`) on the branch `haetty/NAME` |
| `c.sh MSGFILE` | the gate (`scripts/gate.sh`), then the commit of the haetty tree with that message; no commit if the gate fails |
| `push.sh` | check-range (`tools/check-range.sh`), the push of main, the Dropbox mirror (skipped, with a line, while the main checkout has uncommitted files) |
| `stage.sh finish` | fast-forward main to the stage branch, `push.sh`, remove the worktree and the branch |
| `e.sh`, `g.sh` | sourced helpers: the toolchain and the cargo target directory; the gate fallback |

Paths and tools that differ between machines come from `tools/machine.sh` (and `tools/machines/<hostname>.env`);
the table of tools is in `docs/dev-commands.md`, "Environment". The development machine is kramers (Linux); the
Windows PC only checks and sends results on `checks/windows-<date>` branches (`docs/dev-commands.md`).

Heavy commands (the gate, builds, long test runs) go through `tools/heavy.sh --label NAME -- COMMAND`; at most two run at
a time. A commit message file lives outside the repository (the session folder or `/tmp`).

## Start of a session

1. Read `docs/HANDOFF.md` (the top block, the in-progress line, "Next session"), `CLAUDE.md` and this file.
2. Read only the part of `docs/DESIGN.md` the stage needs (the index points to `docs/design/`).
3. Check the state before changing anything: `git fetch`, `git -C ~/projects/claude-code-config log --oneline -3`,
   `git worktree list`, and the logs the in-progress line names. If a stage is in progress, finish it or ask.

## One stage

1. `tools/stage/stage.sh start NAME "remaining work"` (through `tools/heavy.sh`). It refuses a dirty main.
2. Change files in the stage worktree (`~/projects/claude-code-config-wt/NAME/haetty`) only. Keep the changes small; the commit message says
   what was checked.
3. Check the changed crates on the worktree: fmt, clippy with warnings denied, the tests that cover the change.
4. `tools/stage/c.sh MSGFILE` (through `tools/heavy.sh`): the gate, then the commit.
5. `tools/stage/stage.sh finish` (through `tools/heavy.sh`): fast-forward main, check-range, push, Dropbox mirror, the
   worktree removed.
6. HANDOFF: the in-progress line goes; "Next session" says what is done and what is next; the file stays under 400 lines.

## An issue

A GitHub issue is handled as one stage of its own (`stage.sh start issue-N …`):

1. Record the reproduction conditions (version or commit, machine, desktop, X11 or Wayland, steps) and the environment
   the fix was checked in, in the commit message (and the issue, when it lacks them).
2. A regression test where possible — a unit test, an integration test, or an Xvfb script in `scripts/e2e/` added to
   `run-all.sh` — and check that it fails without the fix. Where no test is possible, the commit says why.
3. The commit message ends with `Fixes #N`.
4. One CHANGELOG line under the unreleased version, with `(#N)` and the commit.
5. What only Windows can show is marked "Windows unverified" and added to the Windows check list in
   `docs/dev-commands.md`; it does not hold the stage.

## Limits

- Protected areas change only with the person's approval, and the commit and the report flag them: the attention state
  and the input hold (`haetty-cc` state, the daemon's input gate), the injected settings path, the shared runtime
  directory, the stable install.
- Nothing heavy runs while a registered night run is on (HANDOFF names them).
- The stable haetty on the development machine (its instance, its daemon, `~/.local/opt/haetty`) gets no command
  from a stage: tests run in dev instances (`tools/dev-run.sh`) and test run directories only.
- `stage.sh start` wants `haetty/` clean on main; files changed elsewhere in the repository are left alone (and
  hold the Dropbox mirror back until they are committed or reverted).
- Real Claude Code runs are few and short (quota). A test that needs one says so in its record.
- Build folders and the archived old copy are not read: `.claude/settings.json` denies `target/` and `target-*/`.

## The one-hour soak (P1c)

Not a tag condition (the tag needs the candidate check with the short soak: `docs/SELF-HOSTING.md`). It stays as the
own check of a stage that changes one of these a lot, run before that stage's `finish`:

- **the PTY layer** — `crates/haetty-pty` (spawn, the reader and writer, close and hangup, the Windows backend);
- **the daemon's session lifetime** — creating, closing and reaping sessions, attach and detach, the per-session and
  per-client threads, the forwarding of a session's events (`crates/haetty-daemon`);
- **the engine's memory structure** — how rows, scrollback, pages and snapshots are kept or freed, the scrollback
  limit, a new libghostty-vt pin (`crates/haetty-core`, `crates/haetty-vt`).

"A lot" means the ownership or lifetime of threads, descriptors or buffers changes, not a fix inside one function.
Also run it when a `health_warning` line shows up in a daemon's event log. Neither a night nor an idle desktop is
needed: `tools/overnight-soak-linux.sh night COMMIT 60 [ENGINE]` any time; it waits for and holds both heavy slots
(about 70 min). Use the engine the change touches (`native` for `crates/haetty-vt`, else the default `ghostty`).
The verdict goes into the stage's history record; a FAIL holds the stage like a failing test.

## Not done yet

- A second session that runs one stage from this file alone (the two-stage test in a dev instance).
