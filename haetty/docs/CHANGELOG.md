# Changelog

## Unreleased (for 0.3.2)

### Fixed

- **An X11 window is painted whole after it is restored from minimized** (#1, `70f97a4`): a redraw the system asks for
  (Expose) and a map draw a full frame. Regression test: `scripts/e2e/remap.sh` (Xvfb; fails without the fix).
- **The integrated title row on Xfce shows the buttons xfwm4's `button_layout` names** (#2, `cd2e0e7`), not GNOME's
  unused `button-layout`. Unit test: `titlebar::tests::button_layouts_parse_like_xfwm`.

## 0.3.1 (candidate, 2026-11-09)

The tag `haetty-v0.3.1` is set on this release commit only after its night run passes on both platforms and both
engines, and the kramers re-check list in `docs/HANDOFF.md` section 6.1 is done.

### Fixed

- **The permission mark (`?`) clears when the person answers.** The program-input hold opens at the end of the turn, or
  at the person's next prompt after a rejection (Esc gives no event). Program input is refused while a permission dialog
  is open or was answered and its turn has not ended. The hook order was recorded with Claude Code 2.1.319
  (`docs/history/2026-11-09-k1-attention.md`).
- **Child markers of a Claude Code session are not passed on.** A session haetty starts does not inherit `CLAUDECODE`
  and the other child markers, also when the daemon was started from inside Claude Code: transcript saving stays on and
  `--resume` works (K2, K2b).
- **A killed daemon is reaped by its window**, not left as a zombie (K3a).
- **A test or dev instance keeps its per-session settings beside its own socket**, not in the shared runtime directory
  (K3a; the injected settings path, for review).
- **A resumed session shows the report of its transcript** after a restart: the worker mark no longer waits for the next
  turn (K3a).
- **A tab keeps its own name** across a detach and an attach, also when another window takes the session (K3b).

### Removed

- The former environment names `CCTERM_*` are no longer read; `HAETTY_*` is the only prefix. The file migration of the
  former name (config, tab sets, a running old daemon) stays for this version.

### Changed

- The stage scripts are in the repository (`tools/stage/`); `docs/OPERATING.md` describes one stage per session; the
  project settings deny reads of the build folders (`.claude/settings.json`).

## 0.3.0 (candidate, 2026-11-09)

The tag `haetty-v0.3.0` is set on this release commit only after its night run passes on both platforms and both
engines (`docs/DESIGN.md` section 10.16).

### Added

- **Native terminal engine.** haetty's own engine (`haetty-vt`) is chosen per tab with `engine = "native"` in the
  config. `ghostty` (libghostty-vt) stays the default.
- **Builds without libghostty.** `cargo build --no-default-features` needs no Zig; every tab then runs on the native
  engine. A `ghostty` tab in such a build runs on the native engine and logs a warning.
- **Observing a tab (read-only).** A tab can be made observable from the palette ("Observing: allow or stop for this
  tab") or for every new tab with `observe-new-tabs = true`. `haetty events` and `haetty screen` read an observable
  tab; they cannot write to it.
- **Underline kinds and colour, and OSC 8 hyperlinks.**
- **Generated help and completions.** The CLI help and the shell completions come from one table; the README's config
  table is generated from the code.

### Changed

- The default engine reads every snapshot from a new render state. A combining mark, a variation selector or a ZWJ that
  joins a character drawn in an earlier read is drawn at once (it stayed blank until the row changed). The copy costs
  about 90 µs per snapshot, against 86 µs before.

### Fixed

- The first window opens at its monitor's scale, and the grid is kept on every scale change.
- Windows: Hangul typed into "Rename tab" reached the shell; it now goes into the rename.
- Native engine: C0 controls 0x10 to 0x1F do nothing in the ground state; C1 bytes and the DECRQSS replies for SGR,
  DECSTBM, DECSCUSR and DECSLRM follow the newest Ghostty.

### Performance

- Native engine: runs of printable ASCII are written directly; SGR parameters are parsed in one pass. Session
  throughput of the native engine against libghostty (five runs each, quiet machine, `e5c-tput.sh`): lines 117 %,
  wrapped 132 %, SGR 80 %.
- Dist builds without libghostty are smaller: `haetty` 15 % (Linux) and 23 % (Windows, mingw) smaller;
  `haetty-headless` 49 % and 38 % smaller.

### Build and install

- Windows build without WSL (MSVC, native engine only), described in `docs/SELF-HOSTING.md`.
- The installer still builds the default build in WSL (libghostty and Zig); it has no native-only option yet.

### Known gaps

- The native engine still differs from the newest Ghostty in 744 of 2066 `haetty-vtdiff` runs: kitty graphics
  replies, XTWINOPS size reports, other replies, and some text cases (`docs/vtdiff/2026-11-09-f2.md`).
- Native key-to-frame latency is 0.13 ms above libghostty at the median (budget 16 ms).
- A `ghostty` tab in a build without libghostty records its fallback in the log only; `haetty-headless` shows nothing
  of it without `--trace`.
