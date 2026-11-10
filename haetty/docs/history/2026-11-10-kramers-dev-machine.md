# 2026-11-10 — kramers becomes the development machine (stage kramers-env)

Until 2026-11-09 the development machine was the Windows PC (WSL Ubuntu for the Linux side); kramers only ran the
person's checks. From 2026-11-10 kramers develops and the Windows PC only checks (the person's decision).

## What was missing on kramers and what was done

| Tool | State on 2026-11-10 | Done |
|---|---|---|
| Rust | 1.98.1; its clippy warns about `crates/haetty-gui/tests/wait.rs:264` (`drop_non_drop`), so the gate failed | `rustup update stable` → 1.99.1 (the version the gate passed with before) |
| rustup target `x86_64-pc-windows-gnu` | missing | `rustup target add` |
| Zig 0.15.2 | at `~/opt/zig/0.15.2`, not on PATH | link `~/.local/bin/zig` (the place the scripts expect) |
| Zig 0.17.0 | missing | ziglang.org tarball, sha256 `1cbe9df9…2026` as in `tools/ghostty-ref/README.md`, to `~/.local/zig-0.17.0` |
| Ghostty source at the pin `e035beb` | missing | shallow fetch of the pin to `~/m4/ghostty-ref/ghostty` (145 MB) |
| mingw-w64 | missing (needs root) | `sudo apt install mingw-w64` (13.0.0, gcc 13.2) |
| Xvfb, xdotool, dbus-x11 | missing (need root) | `sudo apt install xvfb xdotool dbus-x11` |
| python3 3.14, flock, unshare, git, rsync, ImageMagick, xclip, xprop, ibus-hangul | present | - |

## Machine-dependent values in one place

`tools/machine.sh` (defaults) and `tools/machines/<hostname>.env`: `HAETTY_REPO`, `HAETTY_WTROOT`, `HAETTY_WORK`
(was `~/m4` in the scripts), `HAETTY_ZIG_BIN`, `HAETTY_ZIG_REF`, `HAETTY_DROPBOX`, `HAETTY_NIGHT_DIR`. Changed to
read it: `tools/stage/e.sh`, `push.sh`, `stage.sh`, `scripts/gate.sh`, `tools/check-range.sh`,
`tools/overnight-soak-linux.sh`, `tools/ghostty-ref/build.sh`. Old-machine values removed:
`/mnt/c/Users/denny/Dropbox/…` (push.sh), `/home/denny/archive/…` (`.claude/settings.json`, now `~/archive/…`),
`~/m4/current-wt` and `$HOME/projects/…` written out in the stage scripts, `wslpath` on every path of the night
script (now only for Windows paths), `/tmp/haetty-overnight` for the night build (a tmpfs on kramers).
- `stage.sh start` now wants only `haetty/` clean on main (the scripts commit nothing else); `push.sh` holds the
  Dropbox mirror back while the main checkout has uncommitted files, because the mirror copies the working tree.
- The PowerShell scripts are unchanged: they run on the Windows PC, whose WSL checkout is still what they expect.
- `tools/overnight-soak-linux.sh night` is new and has not run (first run with the next candidate).

## Measurements on kramers

- First gate run, from an empty `target/` (Rust 1.98.1, before mingw-w64 was there): the Linux release build took
  **3 min 17 s**, the native-only build 1 min 58 s; the whole run 797 s; memory (`scripts/mem-peak.sh`): the job's
  process tree peaked at **4135 MB**, the system went from 6199 MB to 9070 MB used (+2871 MB) of 29 GB. It failed
  three steps: the Windows build (no mingw-w64 yet), clippy on Linux (1.98.1, above), and one test (below).
- **`orchestrator` failed on kramers, every time**: "the message did not arrive whole: \"1\"". The cause is the
  test's fake Claude Code, `stty raw -echo; exec cat >> FILE`: Ubuntu 26.04 ships uutils coreutils 0.8.0, and its
  `cat` forwards the first read from a raw terminal and then nothing more (reproduced without haetty in a plain
  pty; GNU cat and `dd bs=1` pass everything on). The daemon's state was right (`asking: false` after the Stop) and
  no product code is involved. The fixture is now a python3 reader (`tty.setraw`, read, append); 3 of 3 runs pass.
  The test's assertions are unchanged (it is on the permission-prompt path: flagged for the person's review).
  Other fixtures use `stty raw` with `cat FILE` or `exec cat` without a redirect and pass.
- **The gate's own check was wrong under `pipefail`**: `echo "$out" | grep -q PATTERN` — when grep matches early
  it exits, echo dies of SIGPIPE (141) and the pipeline counts as failed. On kramers the test step said FAILED
  with every test passing (twice); the clippy check has the same shape the other way round, so a warning could
  have passed as clean (not seen, but possible on any machine). Both now read a here-string
  (`bash -c 'set -o pipefail; x=$(seq 1 200000); echo "$x" | grep -q "^1$"; echo $?'` prints 141; the here-string
  form prints 0). `tools/check-range.sh` has no pipefail and is not affected.
- The passing gate, check-range and the stage scripts on kramers: see the end of this file.

The earlier "Next session" lines of HANDOFF (F6 to K4) are in `2026-11-10-windows-handover.md` §8, moved there by
the Windows PC's last session; that commit (`36ee748`) arrived while this stage ran, and this stage was replayed on it.
