# Commands, environment and tools (moved from CLAUDE.md)

Moved verbatim from `CLAUDE.md` in stage G0; `CLAUDE.md` keeps the rules and points here.

## Environment

The development machine is **kramers** (Linux, from 2026-11-10; record: `docs/history/2026-11-10-kramers-dev-machine.md`).
The Windows PC only checks (see "Windows-only checks" below). Values that differ between machines are in one place:
`tools/machine.sh` (defaults) and `tools/machines/<hostname>.env`; the stage scripts, the gate, check-range, the
night run and `tools/ghostty-ref/build.sh` source it.

| Tool | Needed for | Where (default, `tools/machine.sh`) | Install |
|---|---|---|---|
| Rust 1.90+ (rustfmt, clippy) | everything | `~/.cargo` (rustup) | rustup |
| rustup target `x86_64-pc-windows-gnu` | the Windows build and clippy | rustup | `rustup target add x86_64-pc-windows-gnu` |
| Zig **0.15.2** | libghostty-vt-sys (first build takes minutes) | `$HAETTY_ZIG_BIN/zig` = `~/.local/bin/zig` (kramers: a link to `~/opt/zig/0.15.2/zig`) | ziglang.org tarball |
| mingw-w64 (`x86_64-w64-mingw32-gcc`, `-strip`, `-objdump`) | linking the Windows build, sizes, linkage | PATH | `sudo apt install mingw-w64` |
| Xvfb, xdotool | `scripts/e2e/` (the GUI end-to-end runs, the soak) | PATH | `sudo apt install xvfb xdotool` (Xvfb without root: `scripts/e2e/README.md`) |
| ImageMagick (`import`, `compare`), xclip, xprop | `scripts/e2e/` | PATH | `sudo apt install imagemagick xclip x11-utils` |
| dbus-launch, ibus, ibus-hangul | the IME case of `scripts/e2e/regression.sh` | PATH | `sudo apt install dbus-x11 ibus-hangul` |
| python3 | tests (core), scripts | PATH | system |
| flock, unshare, setsid, timeout (util-linux, coreutils) | `tools/heavy.sh` (the slots), the e2e scripts | PATH | system |
| git, rsync | the stage scripts, the Dropbox mirror (`$HAETTY_DROPBOX`, default `~/Dropbox/claude-code-config`) | PATH | system |
| Zig **0.17.0** | `tools/ghostty-ref` only (the vtdiff reference) | `$HAETTY_ZIG_REF` = `~/.local/zig-0.17.0/zig` | ziglang.org tarball (sha256 in `tools/ghostty-ref/README.md`) |
| Ghostty source at the pin | `tools/ghostty-ref` only | `$HAETTY_WORK/ghostty-ref/ghostty` = `~/m4/ghostty-ref/ghostty` | `git fetch --depth 1 origin <pin>` |

- Optional: `GHOSTTY_SOURCE_DIR` = a Ghostty checkout at commit
  `a887df42c56f6de86c0fe6da9c4eeca37931e083` to skip the clone.
- `$HAETTY_WORK` (default `~/m4`) also holds `current-wt` (the running stage's worktree, written by `stage.sh`) and
  the VT corpus (`vtcorpus`, recorded Claude Code output; not copied to kramers yet).
- On kramers `/tmp` is a tmpfs (RAM, 15 GB): test run directories stay there (short socket paths), the night
  run's checkout and build go to `$HAETTY_NIGHT_DIR` = `~/build/haetty-overnight`.

## Windows-only checks (the Windows PC, check-only)

The Linux machine cross-builds `x86_64-pc-windows-gnu` (the gate does: build, clippy, sizes, DLL list) but cannot
run the result. Only the Windows PC can do these:

- the native unit and integration tests (`cargo test` built for Windows: `scripts/e2e/README.md`, "Windows");
- the smoke run (`scripts/windows-smoke.ps1`) and the daemon-crash runs;
- the GUI end-to-end runs (`scripts/e2e/windows/tabs.ps1`, `orchestrator.ps1`; an idle desktop);
- the Windows soak and the night run (`tools/overnight-soak.ps1`, HANDOFF §6.1 item 30);
- the installer and dev-run self-tests (`tools/test-install-stable.ps1`, `tools/test-dev-run.ps1`,
  `tools/test-heavy.ps1`), ConPTY, the IME, DWM and HiDPI checks of HANDOFF §6.1 C.

Flow: code changes are made on kramers and pushed to `main`. The Windows PC only pulls (its WSL checkout builds the
Windows binary there, as before), runs the checks, and pushes the result as a document on a branch
`checks/windows-<date>` (`haetty/docs/checks/<date>-windows.md`; never to `main`, no code). kramers fetches that
branch, judges it and records the verdict in HANDOFF; since P1 (2026-11-12) a tag needs the Linux results here only,
and the Windows result goes into the release's CHANGELOG "Windows:" line (verified / unverified / known problems). (Automating the hand-over from `overnight-soak.ps1` is the next stage's work.)

### Windows check list ("Windows unverified")

Items a stage could not check because they need Windows; the PC works through them and records each on its
`checks/windows-<date>` branch. Newest last; an item goes when a check record covers it.

- `70f97a4` (#1): `Focused` and `Occluded(false)` now draw a full frame on every platform. On Windows: restore from
  minimized and switch focus a few times — the window looks as before, idle CPU stays about 0 %.

## Linux night run on the development machine

Prepared 2026-11-10, **not run yet** (the `night` mode of `tools/overnight-soak-linux.sh` has its first run with
the next candidate). One command, about 70 min for a 60 min soak:

```sh
systemd-run --user --unit haetty-night --on-calendar='2026-11-12 02:00' --timer-property=AccuracySec=1s \
  systemd-inhibit --what=sleep:idle --who=haetty --why='haetty night run' \
  bash ~/projects/claude-code-config/haetty/tools/overnight-soak-linux.sh night COMMIT 60 ghostty
```

- Result: `$HAETTY_NIGHT_DIR/out/latest.txt` (first line `RESULT LINUX PASS|FAIL|NOT-RUN …`), `linux.csv`
  beside it. `systemctl --user list-timers haetty-night`, `journalctl --user -u haetty-night` show the timer and
  the run; `systemctl --user stop haetty-night.timer` cancels it.
- It checks that nothing heavy runs (else NOT-RUN), then holds both heavy slots. The soak runs in its own Xvfb,
  so typing does not disturb it the way it stops the Windows part; a busy machine changes the timing, so run it
  when the laptop is not used (02:00–06:00).
- kramers, read 2026-11-10: on external power the lid switch is ignored (`HandleLidSwitchExternalPower=ignore`,
  `/etc/systemd/logind.conf.d/10-lid.conf`) and GNOME does not suspend on idle (`sleep-inactive-ac-type
  'nothing'`), so a closed lid on the charger keeps running. **On battery a closed lid suspends**
  (`HandleLidSwitch=suspend`): the run then stops and its verdict is FAIL or missing — keep the charger in.
  `systemd-inhibit --what=sleep:idle` blocks an idle or requested suspend, not the lid switch.
- A user timer runs only while the user is logged in (`Linger=no`): stay logged in (a locked screen is fine).
- Engine `native` instead of `ghostty` runs every session on haetty's own engine.

## Windows night run (moved from HANDOFF §6.1 item 30, 2026-11-10)

30. **Overnight soak v2, native engine (F0; the night run of 2026-11-09 passed, §10.11).** On the Windows PC
    (the Linux run on kramers: `docs/dev-commands.md`, "Linux night run"). The command, run
    **from a PowerShell window — not from Claude Code**, with the main checkout at the commit to test (`HEAD`, the
    default; the result header prints its hash):
    ```powershell
    pwsh -NoProfile -ExecutionPolicy Bypass -File \\wsl.localhost\Ubuntu\home\<you>\projects\claude-code-config\haetty\tools\overnight-soak.ps1 -Engine native -GuiE2E
    ```
    `-Engine native` passes `--engine native` to every GUI (Linux through `scripts/e2e/engine-gui.sh`, Windows
    through `scripts/e2e/windows/lib.ps1`); `-Engine ghostty` (default) is the libghostty run. `-ExecutionPolicy
    Bypass` is needed: the script on `\\wsl.localhost` is unsigned, and this machine's policy (RemoteSigned,
    LocalMachine) refuses it in a new window (checked 2026-11-07).
    - Before bed, close what you do not need: nothing else heavy may run. Any input of yours stops a Windows part
      (counted as not passed); the screen stays on. About **2 h 10 min** (Linux soak 60 min, Windows soak 60 min,
      GUI e2e).
    - The soaks judge daemon memory with the rule of DESIGN.md section 10.11 (per session: full when its rows stop
      growing; then at least 30 min, and +4 MB at most). They run with the test-only scrollback limit of 800 kB, so
      both engines fill within the first 15 min. The Windows soak and `windows/soak-verdict.ps1` have their first
      real run in this night run.
    - Result: `C:\Users\Public\haetty-overnight\latest.txt`, first lines `RESULT LINUX|WINDOWS|GUI-TABS|
      GUI-ORCHESTRATOR …`, then `engine: native`; `progress.log` beside it.
    - Options: `-Minutes` (default 60), `-IdleWaitMinutes`, `-Commit` (default HEAD), `-Engine`, `-OutDir`, `-GuiE2E`.

## Commands

```sh
cargo build --release
cargo test                                   # core 8+12 (Linux, python3); gui 92 (+1 ignored measurement) + completions, daemoncrash, instances, migrate, observe, orchestrator, permission, threads 1 each; proto 5; cc 17; daemon 1+5 (Unix); testkit 1
cargo clippy --all-targets                   # must stay warning-free
cargo clippy --all-targets --target x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
cargo build --profile dist --target <triple>   # size-optimised, stripped
cargo fmt                                    # rustfmt.toml: max_width 120
cargo run --release -p haetty-core --example mem -- 3 67108864 10000   # anon memory per session (Linux)
```

`scripts/gate.sh` is the full gate (fmt, both targets, clippy on both, all
tests, sizes; ends with `GATE fail=0`); `tools/check-range.sh RANGE` runs the
quick gate on every commit of a range (see Commits and reference frames).
Heavy jobs — builds, the gate, check-range, end-to-end and soak runs,
fuzzing, installer self-tests, WSL builds for Windows — run at most **two at
a time** (on the Windows PC: Linux and Windows together). The scripts
above take a slot themselves (`tools/heavy.sh`; Windows `tools/heavy.ps1`);
run any other heavy command through `tools/heavy.sh CMD` /
`pwsh -File tools\heavy.ps1 -Label TEXT -- CMD`, and never around it.
`tools/heavy.sh --status` shows who holds the slots.
The native engine (DESIGN.md §10, not the default): `crates/haetty-vt`
(pure Rust), wrapped as `haetty_core::native`, chosen per session with
`engine = native` (config, `--engine`). libghostty-vt is the default
feature `ghostty`; `--no-default-features` builds without it and without
Zig (the gate checks this). `cargo run -p haetty-vtdiff --release --
--corpus $HAETTY_WORK/vtcorpus --report FILE` compares the engines cell by cell
(dev only); `--encoders` compares their key/mouse/paste/focus bytes,
`--bisect NAME` finds the first differing byte and a minimal input,
`--probe 'BYTES'` shows both engines' state, `--refresh` measures the
default engine's render-state refresh (DESIGN.md §10.8). A differing run is
KNOWN only if an entry of `docs/vtdiff/known-vt-differences.txt` explains it for the reference it names
(`refs`: `ghostty-main` or `ghostty-0.2.2`; pattern in the input, fields covered, identical with the entry's
`haetty_vt::Compat` switch on); everything else is FAIL. The newest Ghostty (`ghostty-main`, the default
reference) is the target. **Adding an entry is delegated to the agent** when both hold: (a) `ghostty-main`
behaves as haetty-vt does on the inputs, so the difference lies in the other reference; and (b) an ECMA-48
clause or an xterm behaviour gives the reason, cited in `basis`. Such an entry goes straight into the list with
`refs`, `basis` and `approved = <date>, delegated: (a) and (b)`, and is listed in the stage's report. If either
condition fails, or the sources disagree, the proposal goes to `docs/vtdiff/known-candidates.txt` (counted FAIL,
shown "KNOWN if approved") and the person decides. **Changing or removing an entry needs the person's
approval** (record the date and their words in `approved`). The reasoning for each entry is in
`docs/vtdiff/known.md`. Generated tables:
`--gen-width`, `--gen-keys`, `tools/gen-grapheme-table.py`,
`cargo run --release -p haetty-vt --example feed` (the engine's speed alone),
`tools/gen-x11-colors.py`.
`scripts/zig-mode.sh` shows libghostty-vt's Zig mode per profile (all
optimized); `scripts/e2e/measure.sh` measures the §3.4 budgets.
End-to-end checks of the real GUI (Linux, Xvfb): `scripts/e2e/run-all.sh`,
`E2E_ENGINE=native scripts/e2e/run-all.sh` for every session on haetty's engine
(`scripts/e2e/README.md`: what each script checks, Xvfb without root, the
VT fuzz corpus); Windows: `scripts/e2e/windows/`.

Stable installs (the user runs them; `docs/DESIGN.md` §3.15, README):
`tools/install-stable.sh REF` / `tools/install-stable.ps1 REF`, rollback
with `--activate` / `-Activate`; self-tests `tools/test-install-stable.sh
REF1 REF2` / `.ps1` (test run directory only). `haetty --version` names the
commit, build date and instance.

`haetty-headless` drives any program without a window (see README), e.g.
`haetty-headless --trace -e "wait-idle 3000 60000" -e modes -- claude`.
Use it to check behaviour against a real Claude Code before and after a change.
`HAETTY_DUMP_FRAME=f.ppm haetty ...` writes every GUI frame to a file, which
is how rendering can be checked without a screen capture.

## Test isolation (full rule)

- Test isolation (enforced in code): tests write and remove files only in
  their run directory — Rust tests through `haetty_testkit::run_dir` /
  `check` / `remove_dir_all` (a dev-dependency); shell scripts source
  `scripts/test-guard.sh` and call `haetty_guard_init HAETTY_TEST_TMP` and
  `haetty_guard PATH`; PowerShell dot-sources `scripts/test-guard.ps1`
  (`Initialize-HaettyGuard`, `Assert-HaettyPath`). An empty base variable or
  a path in the real HOME, `~/.config`, `~/.local`, `~/.claude`, XDG dirs,
  `%APPDATA%`, `%LOCALAPPDATA%` (and, in WSL, the Windows profile) is refused
  before anything is written. Bases: Linux `/tmp/…`; Windows
  `%PUBLIC%\haetty-test` (the Windows temp folder is under `%LOCALAPPDATA%`).
  Claude Code in tests: an isolated home in the run directory and a dummy
  key only.

## Commits and reference frames (full rules)

- Commits and reference frames:
  - Every commit passes the full gate on its own (both targets, clippy 0
    on both, all tests including the reference frames) — no commit relies
    on a later one to become green.
  - Before every push, check each new commit on its own:
    `HAETTY_TEST_TMP=/tmp/haetty-test tools/check-range.sh origin/main..HEAD`
    (Windows: `tools/check-range.ps1 origin/main..HEAD`) must end with
    "0 failing". It builds each commit that changes `haetty/` in a
    temporary worktree and runs fmt, clippy on both targets (sources
    touched, one run per target) and `cargo test`. A failing commit is
    fixed before the push (the branch is not pushed yet, so it may be
    rewritten); pushed history is never rewritten — a commit found failing
    later is recorded in HANDOFF.md instead.
  - `tools/known-gate-failures` lists the pushed commits known to fail on
    their own (stage T1); check-range reports them as KNOWN — only for the
    steps listed — and everything else as FAIL. **Adding, changing or
    removing an entry needs the user's approval for that entry**; never add
    a commit to make a range pass. Tag conditions use check-range "0
    failing" (KNOWN does not count; FAIL does).
  - An approved change of the outer design (DESIGN.md §2) puts the code and
    the updated reference frames into **one** commit whose message says
    "DESIGN §2 change, approved".
  - Only a reference-frame update without an intended change of the look
    (e.g. new fonts on the reference machine, a rendering fix that moves
    pixels) is a commit of its own, whose message says why the frames
    changed.
  - Update with `HAETTY_UPDATE_REFERENCE=1 cargo test reftest` (Linux) and
    the Windows test binary with `HAETTY_REFERENCE_DIR` (Windows); a rerun
    must report every frame identical on both.
