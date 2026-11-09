# K2: child markers of a Claude Code session (2026-11-09)

The kramers check of 2026-11-09 ([docs/checks/2026-11-09-kramers.md](../checks/2026-11-09-kramers.md)) saw a tab started
from a shell inside Claude Code turn transcript saving off: "inherited CLAUDE_CODE_CHILD_SESSION marker".

## The variables (names only; read from a Claude Code session's environment on 2026-11-09, values never printed)

| name | in `CHILD_MARKERS` | note |
|---|---|---|
| `CLAUDECODE` | yes | marks a Claude Code process tree |
| `CLAUDE_CODE_CHILD_SESSION` | yes | the marker of the warning above |
| `CLAUDE_CODE_MESSAGING_SOCKET` | yes | the parent's messaging socket |
| `CLAUDE_CODE_MESSAGING_TOKEN` | yes | the parent's messaging token (a credential) |
| `CLAUDE_CODE_BRIDGE_SESSION_ID` | yes | the parent's bridge session |
| `CLAUDE_CODE_SESSION_ID` | yes | the parent's session id |
| `CLAUDE_CODE_SESSION_ATTENDED` | yes | the parent's session state |
| `CLAUDE_PID` | yes | the parent's process id |
| `CLAUDE_CODE_ENTRYPOINT` | no | Claude Code sets it for its own process |
| `CLAUDE_CODE_EXECPATH` | no | Claude Code sets it for its own process |
| `CLAUDE_EFFORT` | no | an effort setting, which may be the person's own |

The eight in the first block are the ones the filter removes. Only `CLAUDE_CODE_CHILD_SESSION` is shown to be needed
(the experiment below); the others are removed because they describe the parent session.

## Experiments

1. **Print mode** (`claude -p`, four runs with different variable sets): every run wrote a transcript. The marker does
   not affect print mode.
2. **Interactive Claude Code** in the headless runner (`haetty-headless`: a PTY with scripted input; one prompt, then
   `/exit`):
   - inherited environment: "Transcript saving is off — inherited CLAUDE_CODE_CHILD_SESSION marker"; 0 new transcripts;
   - the eight `CHILD_MARKERS` removed: no warning; 1 new transcript.
3. **Resume**: the transcript of run 2 (markers removed) was resumed interactively with `--resume <id>` in the same
   environment. The picker did not show; asked for the earlier answer, the session answered "ok"; the transcript grew
   from 45 to 55 lines.

Not tested: each of the eight names on its own; a tab created by the GUI (the dev instance). The GUI test is in the K2
check list.

## Where the filter runs

`config::child_env` (unit test `child_markers_are_not_passed_on_and_saving_mode_applies_on_top`) is called by
`App::session_env` for every new session. The GUI passes its environment as `CreateSpec.env`, and the child gets exactly
that, so the filter must sit there. `haetty-core` removes `TMUX` in the child's environment, but a value in
`CreateSpec.env` wins over that removal ("the caller's own settings win"), so it cannot do this job.
