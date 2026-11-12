# Self-hosting: work in stable, try changes in dev

haetty is developed inside haetty. The **stable** instance (the installed
version, which hosts your Claude Code sessions) is never touched by
development. New builds run only as the **dev** instance. This page follows
the rules in `CLAUDE.md` (Working rules: self-hosting, processes, heavy jobs,
commits).

## The two instances

`HAETTY_INSTANCE=dev` gives everything its own name; nothing is shared:

| | stable | dev |
|---|---|---|
| Binary | Linux `~/.local/bin/haetty` → `~/.local/opt/haetty/<version>/`; Windows `%LOCALAPPDATA%\Programs\haetty\current\haetty.exe` | the working tree's build (Linux `target/<profile>/haetty`; Windows copied to `%LOCALAPPDATA%\haetty-dev\bin\haetty.exe`) |
| Daemon endpoint | `$XDG_RUNTIME_DIR/haetty/daemon.sock`; `\\.\pipe\haetty-<SID>` | `$XDG_RUNTIME_DIR/haetty-dev/daemon.sock`; `\\.\pipe\haetty-dev-<SID>` |
| State (tabs, tasks, logs) | `~/.local/state/haetty`; `%LOCALAPPDATA%\haetty` | `~/.local/state/haetty-dev`; `%LOCALAPPDATA%\haetty-dev` |
| Config | `~/.config/haetty`; `%APPDATA%\haetty` | `~/.config/haetty-dev`; `%APPDATA%\haetty-dev` |
| App id (dock icon, desktop file) | `io.github.dhwan0112.haetty` | `io.github.dhwan0112.haetty.dev` |
| Window title | `…` | `[dev] …` |

`haetty --version` names the instance (`instance stable` / `instance dev`).
`haetty status` talks to the stable daemon. `HAETTY_INSTANCE=dev <dev
binary> status` talks to the dev one; `dev-run` prints the dev binary's
path.

## A day

1. **Work in stable.** Open the installed haetty. Claude Code sessions run in
   its tabs. Each session works in its own git worktree on its own branch
   (`CLAUDE.md`).
2. **Try a change in dev.** In a stable tab, in the worktree:
   - Linux: `tools/dev-run.sh` (`--debug` builds faster, `--no-build` starts
     the last build);
   - Windows: `pwsh -ExecutionPolicy Bypass -File
     \\wsl.localhost\<distro>\…\haetty\tools\dev-run.ps1` (`-Debug`,
     `-NoBuild`; Bypass because scripts on `\\wsl.localhost` are unsigned).

   It builds the tree in a heavy-job slot and prints the dev and stable
   endpoint, state, config, app id and title side by side. It starts a
   `[dev]` window only when nothing points at the stable instance (an
   override such as `HAETTY_SOCKET`, `HAETTY_STATE_DIR`, `HAETTY_CONFIG_DIR`,
   `HAETTY_LOG_FILE` or their old `CCTERM_*` names, a path in the dev config,
   or the installed binary). `--check` / `-Check` only prints and checks.
3. **Use the dev window** for what changed: tabs, Claude Code in a dev tab,
   the palette. Its tabs and settings are its own: change
   `~/.config/haetty-dev/config`, never the stable one, for dev experiments.
4. **End the dev instance.** Close the dev window. If its daemon should go
   too: `HAETTY_INSTANCE=dev <dev binary> status --json` gives
   `daemon_pid`; stop that PID. Never stop anything by name (`pkill`, `taskkill /IM`,
   `Stop-Process -Name`): that could hit the stable daemon that hosts your
   sessions.
5. **Heavy jobs:** at most two at a time, Linux and Windows together. The
   build, gate, check-range, e2e and soak scripts take a slot themselves
   (`tools/heavy.sh --status` shows who holds them).
6. **Before a push:** the gate on each commit (`scripts/gate.sh`) and
   `tools/check-range.sh origin/main..HEAD` ending in "0 failing"
   (`CLAUDE.md`, Commits).

## Windows build without WSL

The Windows build that the installer ships is cross-compiled in WSL (mingw, with libghostty and Zig). A build with
the native engine alone needs none of that:

1. **Toolchain**: Rust for MSVC (`rustup default stable-x86_64-pc-windows-msvc`), and Visual Studio 2022 Build Tools
   with the C++ tools (MSVC and the Windows SDK). Find the install with
   `"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -requires
   Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`.
2. **Source**: a git checkout on Windows at the commit to build. A copy without `.git` builds too, but `--version`
   then says `commit unknown`.
3. **Build**, in the checkout:
   `cargo build --profile dist --no-default-features -p haetty-gui -p haetty-headless`
   The binaries are `target\dist\haetty.exe` and `target\dist\haetty-headless.exe` (about 4.5 MB and 2.1 MB at
   `12b5546`).
4. **Check**: `target\dist\haetty.exe --version`; and
   `target\dist\haetty-headless.exe --engine native --cols 40 --rows 5 --timeout 20 --dump plain -- cmd /c echo ok`
   prints `ok`.

The installer (`tools\install-stable.ps1`) has no option for this build yet and installs the default build only. A
`ghostty` engine request in this build runs on the native engine and logs a warning.

## Dev → stable

Only you replace the stable binary. The steps:

1. **Candidate.** A commit on `main` whose own build passed everything:
   - the gate;
   - `tools/check-range.sh haetty-v<previous>..<commit>` with **0 failing**
     (KNOWN entries of `tools/known-gate-failures` do not count; new entries
     need your approval);
   - `scripts/e2e/run-all.sh`;
   - the installer self-tests (Linux);
   - kramers' Linux night run (`tools/overnight-soak-linux.sh night`,
     `docs/dev-commands.md`) with verdict PASS.

   The results go into `HANDOFF.md`. The Windows checks (unit tests, smoke,
   installer self-test, `tools/overnight-soak.ps1`) do not hold the tag
   (P1, 2026-11-12: the v0.3.1 candidate's Windows night passed once in three
   runs of one build, stopped twice by the desktop's focus check). The
   release's CHANGELOG section ends with a "Windows:" line: verified (with
   the `checks/windows-<date>` record), unverified, or known problems.
2. **Your approval** of that commit. A tag needs it, unless you approved a
   tag in advance on stated conditions.
3. **Tag.** An annotated `haetty-vX.Y.Z` on that commit (crate version
   X.Y.Z). Only the tag is pushed.
4. **You install:**
   - Linux: `tools/install-stable.sh haetty-vX.Y.Z`;
   - Windows: `pwsh -File tools\install-stable.ps1 haetty-vX.Y.Z`.

   Each builds in its own worktree (dist profile), installs read-only next
   to the older versions and makes it active. A stable haetty that is open
   keeps running its old version until all its windows are closed and its
   daemon has exited; then start it again.
5. **HANDOFF.** The "current stable" section is updated only when you say
   the new version is installed.

## Rollback

Older versions stay installed:

- Linux: `tools/install-stable.sh --list`, then `--activate <version>`;
- Windows: `install-stable.ps1 -List`, `-Activate <version>`.

The active link or junction moves back; close the stable windows (its daemon
exits with the last session) and start again.

## When stable haetty does not work: back to ccx

ccx (tmux; `cc-panel/README.md`) stays installed as the recovery path, and
nothing in haetty's development may remove or break it (`CLAUDE.md`).

1. Open any other terminal: GNOME Terminal or a TTY on Linux; Windows
   Terminal or `wsl.exe` on Windows.
2. Linux or WSL: `ccx` opens Claude Code in tmux. Your projects, worktrees and
   Claude Code sessions are files; they do not depend on haetty. In a ccx
   tab, `claude --resume` (or `/resume`) continues a session that ran in a
   haetty tab.
3. If the stable binary is at fault: roll back (above) from that terminal.
   If its daemon hangs, `haetty status` cannot answer. Find its PID
   without it: Linux `ss -xlpn | grep '/haetty/daemon.sock'` (the stable
   socket; not `haetty-dev`); Windows Task Manager → Details, the
   `haetty.exe` whose command line ends in `daemon` and whose path is the
   stable install. Stop that PID, then start haetty again.
4. If haetty's state is at fault (a broken tab set): move
   `~/.local/state/haetty/tabs/last.tsv` (Windows
   `%LOCALAPPDATA%\haetty\tabs\last.tsv`) aside before starting; do not
   delete it, it is needed to find the cause.
5. Tell the next development session what happened: the haetty log, the
   version (`haetty --version`), what you did. Record it in `HANDOFF.md`.
