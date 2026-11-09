# Token use of Claude Code sessions: `haetty tokens`

`haetty tokens [--top N] [PATH...]` reads Claude Code transcripts (the `.jsonl` files Claude Code writes under its
projects folder) and reports the token use of their API responses. It is read-only: it writes nothing, does not talk
to the daemon or to a running Claude Code, and changes nothing about how Claude Code runs.

With no PATH it reads the newest transcript under the projects folder (`%USERPROFILE%\.claude\projects` on Windows,
`$HOME/.claude/projects` elsewhere). A folder PATH is searched for `.jsonl` files, subfolders included, so the
subagents' transcripts are counted when their folder is given. Several transcripts give one report each and a merged
total.

## What is counted

- **Each API response once.** Claude Code writes one `assistant` record per content block of a response, and every
  record repeats the response's `usage`. Records are grouped by `message.id`. In the 2026-11-09 session, 990 records
  made up 412 responses.
- **The four counts of `usage`:** input tokens, output tokens, cache creation (shown as cache write) and cache read.
- **Attribution to the preceding tool.** A response is attributed to the tool call whose result came right before it,
  that is the output the response had to read. A response after a prompt, or after no tool result, is reported as
  `(no tool result)`.
- **Cache hit ratio** = cache read / (input + cache write + cache read): the share of the input side served from the
  cache.
- **Largest tool results.** The size in bytes of each tool result (a string's length, or the text parts of an array),
  with the tool and the identifier of its call: the command (Bash), the file path (Read, Edit, Write), the pattern
  (Grep, Glob) or the description (Agent). Identifiers are cut to 120 characters and control characters become
  spaces. Nothing else of the content is read or kept.

## Reading the report

- `responses` counts API responses; `tool calls` counts `tool_use` blocks.
- `by tool` adds up the tokens of the responses that followed a result of that tool. These sums show where the
  context the model had to read came from; they are not a bill.
- Sizes are bytes of text, not tokens. For English text, about 4 bytes make a token.

## Limits

- A transcript record that does not parse is counted as unreadable and skipped.
- The attribution is an approximation: a response's usage depends on its whole context, not only on the newest result.
- Only the record shapes Claude Code writes today are read; other records are skipped.

## Privacy

The output contains the commands and paths of your work and the token counts. Commands can contain secrets. The
command writes nothing to disk and sends nothing anywhere; keep that in mind before pasting its output into a shared
place.

## In the window (`token-status`)

`token-status = true` (off by default) adds the tokens to the status panel: the active tab's line at the top
(`tokens 2.08G (new 20.10M) · hit 99.3 %`), and each worker's line under its entry in WORKERS. `tokens` is the
total of everything counted (cache reads included); `new` is the input, the cache writes and the output, what was
processed fresh; `hit` is the cache's share of the input side. A background thread reads each transcript every
2 seconds, only what was appended, and keeps nothing else of it. A tab shows no line until its transcript has been
read.

## For a CLAUDE.md

`claude-guide.md` is the text to copy into your own CLAUDE.md, so that Claude Code uses `haetty run`, `haetty wait`
and the text of `haetty screen`. haetty does not add it to any session.

## Saving mode

`saving-mode = true` (off by default) makes haetty start each Claude Code session with two output limits in its
environment: `BASH_MAX_OUTPUT_LENGTH=12000` and `MAX_MCP_OUTPUT_TOKENS=12000`. A value the environment already has is
replaced for those sessions. The status row of a Claude Code tab shows a dim `◆ saving` while the mode is on.

### What the variables mean (official, checked 2026-11-09)

- `BASH_MAX_OUTPUT_LENGTH`: "Maximum number of characters of bash output that Claude Code reads back into a command's
  result (default: 30000; maximum: 150000)." Source: https://code.claude.com/docs/en/env-vars. The same row says:
  "If you set the `bashOutputMaxChars` setting, Claude Code ignores this variable." Saving mode does not set that
  setting; if you have it, its value is the limit.
- `MAX_MCP_OUTPUT_TOKENS`: "Claude Code displays a warning when MCP tool output exceeds 10,000 tokens and limits output
  to 25,000 tokens by default. To change the token limit, set the `MAX_MCP_OUTPUT_TOKENS` environment variable, for
  example `MAX_MCP_OUTPUT_TOKENS=50000`. The warning threshold is fixed. Unless the server raises a tool's own limit,
  successful text results longer than 50,000 characters are saved to a file regardless of this variable." Source:
  https://code.claude.com/docs/en/mcp (the Tips list). It limits MCP tool output only, not Bash output.

### Where the values come from (the G1 measurement, `measure-2026-11-09.md`)

- `BASH_MAX_OUTPUT_LENGTH = 12000`: the largest Bash results of the session were 28,596 and 24,093 bytes (the top-10
  list); 12,000 characters cuts them by about half. The official default is 30,000 characters.
- `MAX_MCP_OUTPUT_TOKENS = 12000`: no MCP tool result is in the measurement, so the value is not measured. The largest
  text result measured was 45,734 bytes, about 11,400 tokens at about four bytes a token (the report's own rule), so
  12,000 tokens keeps a result of that size whole. The official default is 25,000 tokens.

These are starting values from one session, not a tuned setting. The limits shorten what a tool returns into the
context; they change nothing else.

### Limits of the mode

- Claude Code reads its environment when it starts: a session that was already running keeps its limits.
- A shorter result may leave out what you need; `haetty run` keeps the full output in its log.
- How many tokens the mode saves in practice is measured in G7, not here.
