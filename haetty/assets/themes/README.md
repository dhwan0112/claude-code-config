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
