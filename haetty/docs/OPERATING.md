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

## Not done yet

- A second session that runs one stage from this file alone (the two-stage test in a dev instance).
