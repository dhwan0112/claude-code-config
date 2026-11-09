# Using haetty's tools: a guide for Claude Code

haetty does not add this text to any session. To have Claude Code use the tools, copy the part below the line into
your own CLAUDE.md. The commands are the ones of `haetty --help` (each subcommand has its own `--help`).

---

## Long commands and their output

- Run builds, test suites and other long or noisy commands with `haetty run -- COMMAND [ARGS...]`. The full output
  goes to a run log. The terminal shows the exit code, the time taken, the lines that look like errors, the last
  lines (`--summary-lines N`, 10 by default) and the log path. The exit code is the command's own.
- Read a log in parts: `haetty log PATH --grep TEXT` (the lines that contain TEXT), `haetty log PATH --lines A:B`,
  `haetty log PATH --tail N`. Do not print a whole log.
- A log can contain secrets. Do not paste it into anything else.

## Waiting

- Do not poll with `sleep` loops. Use `haetty wait`, which prints one line and exits:
  - `haetty wait pid PID`: the process ends;
  - `haetty wait file PATH --contains TEXT`: the file has the text (text already there counts);
  - `haetty wait file PATH --changed`: the file appears, or its size or time changes;
  - `haetty wait screen SESSION --contains TEXT`: an observable tab shows the text;
  - `haetty wait event SESSION --stop`: an observable tab's turn ends (after the wait starts);
  - `haetty wait event SESSION --permission`: an observable tab asks for a permission answer.
- `--timeout SECONDS` sets the limit (600 by default). Exit codes: 0 the condition holds; 124 the timeout passed;
  3 the tab is not observable; 2 a bad command line.

## Looking at a screen

- Read a tab's screen as text first: `haetty screen SESSION` (or `--json`). Take an image only when the layout
  itself is the question.

## Tokens

- `haetty tokens --top 10 PATH` (a transcript `.jsonl` file, or a folder) shows the tokens of a session by tool and
  the largest results. It prints names, paths and sizes, never the content.
