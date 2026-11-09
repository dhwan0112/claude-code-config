# K1: the attention mark and the input hold are separate (2026-11-09)

Kramers 2026-11-09, item 12 (see [docs/checks/2026-11-09-kramers.md](../checks/2026-11-09-kramers.md)): the red `?`
stayed after the answer. After "Yes" it stayed until `PostToolUse`, which comes only when the command ends (13 s in the
run); after Esc it stayed for good, because Claude Code sends no event for a rejection. One field, `Attention::Ask`,
did three jobs: the tab mark, the refusal of program input in the daemon, and the orchestrator's hold in `orch::ready`.

## Hook order with real Claude Code 2.1.319 (`--permission-mode` as noted; the events are the hook log, ms offsets)

| run | case | hook order |
|---|---|---|
| A, D, H | Write or Bash, Yes (`1`) | `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `Stop` |
| F | Write, option 2 "allow all edits this session" | `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `Stop` |
| E | Write, option 3 "tell Claude what to do differently", text typed | `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `UserPromptSubmit` (the text), `Stop` |
| C, G | Write, Esc (rejected) | `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, then nothing; the screen shows "User rejected write"; the next typed prompt gives `UserPromptSubmit`, `Stop` |
| I | Write, dialog open 75 s with no key, then Esc | `UserPromptSubmit`, `PreToolUse`, `PermissionRequest` (4925 ms), `Notification permission_prompt` (10877 ms); nothing after Esc in 90 s |
| H | Write, dialog open about 20 s, then `1` | `PermissionRequest` (5737 ms), `Notification permission_prompt` (11766 ms), `PostToolUse` (25837 ms), `Stop` (28921 ms) |
| J | acceptEdits: a Write inside the folder, then a Write outside it | inside: no `PermissionRequest`; outside: `PreToolUse`, `PermissionRequest`, `PostToolUse`, `Stop` |
| K | auto mode: a Write inside the folder | no `PermissionRequest` |
| L | default mode with `--allowedTools Write`: a Write | no `PermissionRequest` |
| D2 | auto mode: a shell command the permission check may refuse (it pipes from an unresolvable host) | `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop`; no dialog, no `PermissionDenied` (the denial path was not reached) |

Findings:
- `PermissionRequest` came only with a dialog in every case run. A denial by auto mode was not reproduced.
- `Notification permission_prompt` comes about 6 s after the request, and only while the dialog is open. No such
  notification followed an answer given within 3 s (runs A, C, D, E, F, G).
- `idle_prompt` did not come during a 75 s open dialog, nor in the 90 s after Esc. It cannot release the hold.
- After Esc no event comes at all: the turn ends on the screen ("Brewed for 6s · done") with no `Stop`, and `busy`
  stays set until the next prompt.
- Tool events of subagents carry `agent_id` (the hooks page: "the input carries the `agent_id` and `agent_type`
  common input fields"), and `Stop` is converted to `SubagentStop` inside a subagent. `Stop` is the main agent's
  end of turn; it cannot come while the main agent's dialog is open.

## Rules (what the code does)

- **Human input.** A key a person sends (origin Human): a `Key` that is a press or repeat of a non-modifier key, a
  `Paste`, or a `Write`. Wheel, mouse, focus, resize, and modifier-only presses are not input.
- **The mark (`attention = Ask`)** is set by `PermissionRequest` and by `Notification` with a question type. It is
  cleared at once by a human input while the input is held, and by `UserPromptSubmit`, `Stop`, `StopFailure`,
  `SessionStart` or `SessionEnd` that started after the question. `PostToolUse`, `PostToolUseFailure`,
  `PermissionDenied` and `PreToolUse` do not clear it: they can come from a subagent or a parallel call while the
  dialog is open.
- **The input hold (`ClaudeInfo.asking`)** is set by a question. It opens (1) at `UserPromptSubmit` that started after
  the question, or (2) at `Stop` / `StopFailure` that started after the question when a person's input came after it.
  Tool events never open it. After an Esc it stays closed until the person's next prompt (the brief's rule; the
  idle signal is not evidence, see above).
- **Stale questions** (a question that started before the latest prompt, turn end, or the person's answer) change
  nothing. A question reminder after an answer (the 6 s notification of the same dialog) changes nothing.
- **Program input** is refused by the daemon while the hold is on (`Denied`), and `orch::ready` returns
  `Hold::Asking` with a reason that names the release: the turn's end, or the person's next prompt in that tab.
  After a rejection the orchestrator does not resend; the GUI's send shows that reason.

## Limits and open choices

- **Stop alone** would also be safe (a turn cannot end while its dialog is open). The brief's rule asks for a human
  input as well; this is kept. If the person wants Stop alone, it is a one-line change in `Claude::hook`.
- **Auto-mode denial** (`PermissionDenied`) was not reproduced. If it comes with a `PermissionRequest` and no dialog, the
  hold stays until the person prompts, which stalls an unattended worker. This needs a probe that makes auto mode deny.
- **K2 is reopened**: the child inherits the daemon's environment, which the GUI-side filter does not see (see the
  K2 record).
