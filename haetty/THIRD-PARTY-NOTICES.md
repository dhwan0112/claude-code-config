# Third-party notices

haetty is MIT-licensed (`LICENSE`). It contains or links the following
third-party material.

## Colour theme "IBM 5153 CGA" (embedded)

`assets/themes/IBM 5153 CGA` and `assets/themes/IBM 5153 CGA (Black)`,
embedded in the binary as built-in themes (the default is "IBM 5153 CGA").

- Author: Jesse Miller (https://jmiller.com/).
- Basis: int10h.org, "IBM 5153's True CGA Palette and Color Output"
  (June 2022), https://int10h.org/blog/2022/06/ibm-5153-color-true-cga-palette/.
- Distributed in mbadolato/iTerm2-Color-Schemes
  (https://github.com/mbadolato/iTerm2-Color-Schemes); the Ghostty files
  were copied unchanged at commit `38bd77c`.
- haetty fills in one value when it loads them (not in the files): a
  selection background missing or equal to the foreground becomes
  `#4e4e4e` (`assets/themes/README.md`).

Licence of the iTerm2-Color-Schemes collection
(`assets/themes/LICENSE-iTerm2-Color-Schemes.txt`):

```
MIT License

Copyright (c) 2011 to Present Mark Badolato

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

This license covers the iTerm-Color-Schemes repository collection of themes. 

The copyright/license for each individual theme belongs to the author of that theme.
```

## Code linked into the binary

- libghostty-vt (Ghostty, MIT) via `libghostty-vt` / `libghostty-vt-sys`
  (MIT OR Apache-2.0). The built-in theme "haetty classic" uses Ghostty's
  default 16 colours.
- `portable-pty` (MIT) on Unix.
- winit, softbuffer (Apache-2.0 / MIT), swash, fontdb, copypasta, memmap2,
  unicode-width (MIT / Apache-2.0).
- postcard, serde, crossbeam-channel, log, libc, windows-sys
  (MIT / Apache-2.0); similar (Apache-2.0).
- Microsoft.Windows.Console.ConPTY `conpty.dll` / `OpenConsole.exe` (MIT,
  Microsoft) via `conpty-bin-vendored` (MIT) on Windows; Microsoft's licence
  is written next to them when they are unpacked.
