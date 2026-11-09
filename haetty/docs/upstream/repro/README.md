# Re-runs on the pinned Ghostty (2026-11-09)

The pin is Ghostty `e035beb9437118b803830a5864ea2f4dd7094354`, the commit `tools/ghostty-ref/build.sh` builds. Build its
library first (with `GHOSTTY_REF_TARGET` set to a folder outside the repository), then, with PREFIX that folder's
`install` directory:

    zig cc -std=c11 -O2 -DGHOSTTY_STATIC -I $PREFIX/include 03-repro.c $PREFIX/lib/libghostty-vt.a -o repro03
    zig cc -std=c11 -O2 -DGHOSTTY_STATIC -I $PREFIX/include show.c $PREFIX/lib/libghostty-vt.a -o show

Add the extra libraries listed in `$PREFIX/share/pkgconfig/libghostty-vt-static.pc`, if that file lists any (the
build script does the same).

- `show COLS ROWS BYTES`: prints the non-blank rows of BYTES written to a terminal, as a fresh render state has them
  (drafts 01 and 02; use `$'...'` in bash for the escape sequences).
- `repro03`: the cases of draft 03. It exits with 1 when a kept render state is stale.
