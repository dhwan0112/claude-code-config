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
