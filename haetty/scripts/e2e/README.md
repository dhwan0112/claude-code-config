# End-to-end checks

Scripts that drive the real `haetty` GUI and daemon. They need a release build (`cargo build --release`), Linux,
and an X display of their own (`xvfb.sh`), plus `xdotool`, `xclip`, `python3`, `netpbm` (`pnmtopng`), `git`.
Everything is written only in a test run directory: set `HAETTY_TEST_TMP` (e.g. `/tmp/haetty-test`; never a
folder in your home) — `scripts/test-guard.sh` refuses anything else. Each script runs a `HAETTY_INSTANCE=dev`
daemon on its own socket and stops it by PID; a stable haetty you use is never touched.

```sh
HAETTY_TEST_TMP=/tmp/haetty-test scripts/e2e/run-all.sh          # everything (builds first)
HAETTY_TEST_TMP=/tmp/haetty-test scripts/e2e/xvfb.sh bash scripts/e2e/tabs.sh /tmp/haetty-test/frames
```

| Script | What it checks |
|---|---|
| `regression.sh TAG` | keys, Shift+Enter, paste, wheel, focus through the GUI (probe); OSC 52; resize; Ctrl+C twice ends Claude Code (when `claude` is on PATH; isolated home, dummy key); IME preedit (ibus-hangul, when installed); tabs and palette; tab sets; reattach after a crashed window; a Hangul/emoji frame dump |
| `design-2-3.sh` | DESIGN.md §2.3: no accent colour when idle, layout unchanged by events (`E2E_ARGS`, `E2E_BANDS` for integrated decorations) |
| `themes.sh` | theme change while running (palette reload, focus reload, reattached tabs) |
| `titlebar.sh` | the integrated title row: buttons, hover, move/maximize/resize calls, close |
| `tabs.sh` | rename, `repo:branch` names, closed tabs with `--resume`, diff views, detach/attach |
| `orchestrator.sh` | tasks, worker tabs in worktrees, reports and states, message gating, worktree removal |
| `daemon-crash.sh` | the GUI when its daemon is killed: window, hint, tab sets kept, no orphans, restore |
| `look.sh`, `dump-sample.sh` | frame dumps for looking at by hand |
| `firehose.sh` | daemon answer time while a tab runs `yes` |
| `record-claude.sh DIR` | records Claude Code's terminal output for the VT fuzz corpus (see below) |
| `windows/*.ps1` | Windows: `daemon-crash.ps1`, `soak.ps1` (copy with `scripts/test-guard.ps1`, see each header) |

`run-all.sh` and `scripts/gate.sh` use nothing outside the repository.

## Xvfb without root

`xvfb.sh` uses `Xvfb` from PATH when `/tmp/.X11-unix` is writable. Without root (e.g. WSL without the package),
unpack the Debian/Ubuntu packages into a folder and point `HAETTY_XROOT` at it:

```sh
mkdir -p ~/xroot/debs && cd ~/xroot/debs
apt-get download xvfb x11-xkb-utils xkb-data xserver-common libxfont2 libfontenc1 libxkbfile1 xfonts-base
for d in *.deb; do dpkg -x "$d" ..; done
export HAETTY_XROOT=~/xroot
```

Then `xvfb.sh` runs Xvfb inside a private mount namespace (`unshare -rm`: `xkbcomp` overlaid on `/usr/bin`,
a writable `/tmp/.X11-unix`); it needs unprivileged user namespaces.

## The VT fuzz corpus

`cargo test -p haetty-core --test vtfuzz` (part of the gate) fuzzes the engine with generated escape-sequence
streams, random bytes and mutations of a built-in Claude-Code-like stream — no files needed. The long run
(`HAETTY_VT_FUZZ=full cargo test --release -p haetty-core --test vtfuzz`) also mutates recorded Claude Code
output when `HAETTY_VT_CORPUS` names a folder of `*.raw` files. The corpus stays outside the repository
(recorded output of another program). To make one:

```sh
HAETTY_TEST_TMP=/tmp/haetty-test scripts/e2e/record-claude.sh /path/outside/the/repo
```

It starts Claude Code under `haetty-headless --raw-log` with an isolated `HOME` and `CLAUDE_CONFIG_DIR` in the
test run directory (onboarding done, the project trusted), a dummy `sk-ant-api03-AAAA…` key, and
`CLAUDE_CODE_*` / `CLAUDECODE` / `CLAUDE_*SESSION*` removed; it types text and opens menus but submits nothing, so
no API call is made. It records the fullscreen and the classic renderer, then replaces the dummy key and the run
path in the files with filler of the same length.
