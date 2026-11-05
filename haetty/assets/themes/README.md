# Built-in themes

Colour themes in Ghostty's theme file format (`key = value` lines:
`palette = N=#rrggbb`, `background`, `foreground`, `cursor-color`,
`cursor-text`, `selection-background`, `selection-foreground`). They are
embedded in the binary (`haetty_core::theme`); the README of the repository
explains how to choose one or add your own.

| File | Name in `theme = …` | Source |
|---|---|---|
| `IBM 5153 CGA` | `IBM 5153 CGA` (the default) | copied unchanged, see below |
| `IBM 5153 CGA (Black)` | `IBM 5153 CGA (Black)` | copied unchanged, see below |
| `haetty classic` | `haetty classic` | haetty's colours before the themes: Ghostty's default 16 colours (libghostty-vt's built-in palette, `src/terminal/color.zig` at the pinned Ghostty commit), background `#1e1e1e`, foreground `#dcdcdc`; no cursor or selection colours (cursor = foreground, text under it = the cell's background, selection = colours swapped) |

## IBM 5153 CGA

The colours of IBM's 5153 Personal Computer Color Display for the CGA,
measured and published by int10h.org; the theme is by Jesse Miller.

- Author: Jesse Miller (https://jmiller.com/).
- Basis: int10h.org, "IBM 5153's True CGA Palette and Color Output"
  (June 2022), https://int10h.org/blog/2022/06/ibm-5153-color-true-cga-palette/.
- Copied from the `ghostty/` folder of mbadolato/iTerm2-Color-Schemes
  (https://github.com/mbadolato/iTerm2-Color-Schemes), commit `38bd77c`,
  byte for byte. Licence: MIT, `LICENSE-iTerm2-Color-Schemes.txt` (also in
  `THIRD-PARTY-NOTICES.md`).

Colour 3 ("yellow") is brown, as on the real monitor: the 5153 turns dark
yellow into brown.

**Filled in by haetty, not in the files:** both files set
`selection-background` to the foreground colour (`#cecece` / `#c4c4c4`).
When the built-in theme is loaded, a selection background that is missing
or equal to the foreground is replaced by `#4e4e4e` (colour 8); the files
themselves stay unchanged. The files' `selection-foreground` (`#141414` /
`#000000`) is kept, which gives selected text a contrast of 2.21:1 /
2.52:1 on `#4e4e4e` (the cell's own default text, `#cecece`, would give
5.29:1). Today this shows nowhere: haetty has no selection of its own yet
(Claude Code's fullscreen selection draws its own highlight). Only the
built-in files are filled in; a theme of your own is used as written.

## Contrast of IBM 5153 CGA

WCAG 2.0 contrast of each colour on the background `#141414` (output of the
test `render::contrast::tests::cga_contrast_against_its_background`, which
also asserts Blue 1.85 and Bright Black 2.21), and what `minimum-contrast`
makes of it. haetty's default is `minimum-contrast = 1`: the colours stay as
the monitor showed them. As in Ghostty, text below the minimum becomes
white (on a dark background) or black (on a light one) — the whole colour,
not a lighter shade of it; box drawing, block elements and Powerline glyphs
are left alone.

| # | name | colour | contrast on #141414 | at minimum-contrast 3 | at 4.5 |
|---:|---|---|---:|---|---|
| 0 | Black | `#141414` | 1.00 | → #ffffff | → #ffffff |
| 1 | Red | `#d03333` | 3.69 | unchanged | → #ffffff |
| 2 | Green | `#1bd01b` | 8.85 | unchanged | unchanged |
| 3 | Yellow (brown) | `#d08c1b` | 6.53 | unchanged | unchanged |
| 4 | Blue | `#1b1bd0` | 1.85 | → #ffffff | → #ffffff |
| 5 | Magenta | `#d01bd0` | 4.17 | unchanged | → #ffffff |
| 6 | Cyan | `#1bd0d0` | 9.63 | unchanged | unchanged |
| 7 | White | `#cecece` | 11.71 | unchanged | unchanged |
| 8 | Bright Black | `#4e4e4e` | 2.21 | → #ffffff | → #ffffff |
| 9 | Bright Red | `#dc4e4e` | 4.60 | unchanged | unchanged |
| 10 | Bright Green | `#4edc4e` | 10.24 | unchanged | unchanged |
| 11 | Bright Yellow | `#f3f34e` | 15.56 | unchanged | unchanged |
| 12 | Bright Blue | `#4e4edc` | 3.02 | unchanged | → #ffffff |
| 13 | Bright Magenta | `#f34ef3` | 6.31 | unchanged | unchanged |
| 14 | Bright Cyan | `#4ef3f3` | 13.54 | unchanged | unchanged |
| 15 | Bright White | `#ffffff` | 18.42 | unchanged | unchanged |
| – | foreground | `#cecece` | 11.71 | unchanged | unchanged |

Frame dumps of the real window (Linux, Xvfb, DejaVu Sans Mono 11 pt; colours
0–15 with `38;5;N`, default text, faint, inverse, box drawing):
`docs/theme/minimum-contrast-1.png`, `-3.png`, `-4.5.png`, and the same
chart in `haetty classic` (`docs/theme/haetty-classic.png`).
