# The inline diff's replay set (stage PD)

`crates/haetty-gui/tests/data/inline-diff/run*/` holds real Claude Code output with the edit tools' hook input of the
same moment: `screen.raw` and `timing` (`script -T`), `hooks.jsonl` (PreToolUse and PostToolUse of Edit, Write and
NotebookEdit, reduced to the fields the inline diff uses), `meta` (size, renderer, Claude Code version). The test
`inline_diff::tests::the_recorded_real_runs_match_on_both_engines` replays them chunk by chunk and checks, at every
moment the output paused, how many block shapes there are and how many match; `replay_report` (ignored) prints the
shapes that did not match with the first row that differs. Build a new set when Claude Code's version changes.

Each run is one real Claude Code session (the person's login, `--model haiku`): count it against the stage's allowed
real runs. Content comes from the test repository only (`mkrepo.py`).

1. `HAETTY_TEST_TMP=/tmp/haetty-test scripts/e2e/xvfb.sh bash scripts/record-edits/host.sh run1 120 40 &` (a dev
   instance window over a fresh test repository whose `.claude/settings.json` logs the edit tools' hook input).
2. From bash: `. scripts/record-edits/drive.sh run1`, then in the tab:
   `typ "script -q -f -O \$W/raw.log -T \$W/timing -c 'claude --model haiku --permission-mode default'"` (add
   `CLAUDE_CODE_NO_FLICKER=1` before `claude` for the full-screen renderer). Trust the folder (`key Down; key Return`),
   paste `prompt1.txt` (`xclip -selection clipboard < scripts/record-edits/prompt1.txt; key ctrl+shift+v; key
   Return`), answer the permission questions with `key 1` or `key 2`, watch with `scr`. End with `typ /exit`.
3. `mkdir SRC; cp $W/raw.log $W/timing $W/hooks.jsonl SRC/`, then
   `CLAUDE_VERSION=2.1.330 python3 -I scripts/record-edits/pack.py SRC crates/haetty-gui/tests/data/inline-diff/runN
   COLS ROWS default|fullscreen` (the plan name in the banner becomes filler; it refuses a home path).
4. `touch $W/stop`. Add the run to the replay test with its counts; look at `replay_report`'s unmatched shapes before
   accepting a lower count.
