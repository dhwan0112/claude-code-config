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
themselves stay unchanged. Only the built-in files are filled in; a theme
of your own is used as written.

**Selected text (every theme):** a selected cell takes the theme's
`selection-foreground` only when its WCAG contrast with the selection
background is **not lower** than that of the cell's own text colour;
otherwise the cell keeps its own text colour (ties go to the theme). The
reason: both CGA files pair `selection-foreground` with the inverted
selection they were made for (`#141414` on `#cecece`); on the `#4e4e4e`
fill-in it gives only 2.21:1 (Black variant: `#000000` 2.52:1), while the
default text `#cecece` gives 5.29:1 (Black: `#c4c4c4` 4.77:1). Contrast is
the WCAG 2.0 ratio (L1 + 0.05) / (L2 + 0.05) of relative luminances from
linearised sRGB (`render::contrast::ratio`). For IBM 5153 CGA (theme's
selection text `#141414`: 2.21:1 on `#4e4e4e`):

| text colour | its own contrast on `#4e4e4e` | selected text drawn in |
|---|---:|---|
| 0 `#141414` | 2.21 | `#141414` (the theme's; equal) |
| 1 `#d03333` | 1.67 | `#141414` (the theme's) |
| 2 `#1bd01b` | 4.00 | its own |
| 3 `#d08c1b` | 2.95 | its own |
| 4 `#1b1bd0` | 1.19 | `#141414` (the theme's) |
| 5 `#d01bd0` | 1.88 | `#141414` (the theme's) |
| 6 `#1bd0d0` | 4.35 | its own |
| 7 `#cecece` (default text) | 5.29 | its own |
| 8 `#4e4e4e` | 1.00 | `#141414` (the theme's) |
| 9 `#dc4e4e` | 2.08 | `#141414` (the theme's) |
| 10 `#4edc4e` | 4.62 | its own |
| 11 `#f3f34e` | 7.03 | its own |
| 12 `#4e4edc` | 1.37 | `#141414` (the theme's) |
| 13 `#f34ef3` | 2.85 | its own |
| 14 `#4ef3f3` | 6.12 | its own |
| 15 `#ffffff` | 8.32 | its own |

Today selection shows nowhere: haetty has no selection of its own yet
(Claude Code's fullscreen selection draws its own highlight).

## Contrast of IBM 5153 CGA

WCAG 2.0 contrast of each colour on the background `#141414` (output of the
test `render::contrast::tests::cga_contrast_against_its_background`, which
also asserts Blue 1.85 and Bright Black 2.21), and what `minimum-contrast`
makes of it. haetty's default is `minimum-contrast = 1`: the colours stay as
the monitor showed them. As in Ghostty, text below the minimum becomes
white (on a dark background) or black (on a light one) — the whole colour,
not a lighter shade of it; box drawing, block elements and Powerline glyphs
are left alone, and so are haetty's own rows (tab bar, status row, panel,
palette, hints).

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
