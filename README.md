# claude-code-config

Claude Code worker baseline shared between kramers and pauli: `cc-panel/` + `bin/ccx` (Claude Code workspace in tmux),
`agents/`, `skills/`, `templates/`, `statusline.sh`, `haetty/` (Rust terminal for Claude Code, successor of ccx, formerly `ccterm/`; `haetty/docs/HANDOFF.md`), and `MANIFEST.md` (change log with md5 lines). `hosts/<host>.md` are the
per-host logs. See `cc-panel/README.md` for ccx itself.

**Versioning.** Since 2026-11-04 this git repository is the baseline (public remote `github.com/dhwan0112/claude-code-config`).
The Dropbox folder `~/Dropbox/claude-code-config` is kept as a mirror for a host that has not switched yet:
`tools/mirror-dropbox.sh` copies the repository into `current/` and `hosts/` there. The earlier Dropbox `history/<date>-<Host>-N`
snapshots were imported as the first commits and are available as tags of the same name (`git tag`).

**Workflow.** Clone to `~/projects/claude-code-config`, make changes there (or copy a staged, tested tree in), regenerate the md5
lines in `MANIFEST.md` and add an `- Updated:` line, commit, push, then run `tools/mirror-dropbox.sh` while pauli still reads Dropbox.
Install on a host by copying `cc-panel/` to `~/.claude/cc-panel` and `bin/ccx` to `~/.local/bin/ccx` (write a new file and `mv`,
never edit the installed scripts in place while ccx tabs run). Commits carry the user's git identity and no AI attribution lines.
