//! Screen dumps: plain text, VT sequences or HTML, byte for byte as
//! libghostty-vt's formatter (`formatter.zig`) writes them with the options
//! haetty's default engine uses: the whole active screen (its scrollback and
//! its rows), no extras (no palette, modes, cursor), no unwrapping, palette
//! colours as indexes.
//!
//! Underline kinds and colours are in VT and HTML dumps, hyperlinks in HTML
//! (`<a href>`) only, as libghostty (its VT dump has no OSC 8 for cells).
//! What haetty-vt does not keep cannot be dumped: the rows libghostty keeps
//! above the alternate screen after `CSI 22 J` (hidden there, but dumped).

use crate::{flags, underline, Color, Line, Style, Terminal, Wide};

/// The output format (libghostty's `Format`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Plain,
    /// SGR sequences for the styles, `\r\n` between rows.
    Vt,
    /// Inline styles in `<div>`s, `\n` between rows.
    Html,
}

/// Bits a standard page has for its rows and cells (libghostty's
/// `availableBitsForGrid` of `std_capacity`), measured with the reference.
const GRID_BITS: usize = 3_002_112;

/// Rows in one page of libghostty's page list at this width
/// (`initialCapacity`): a row costs 64 bits of metadata and 64 per cell.
/// Too wide for a standard page, a page holds the standard 215 rows.
pub(crate) fn rows_per_page(cols: u16) -> usize {
    match GRID_BITS / (64 * (cols as usize + 1)) {
        0 => 215,
        n => n.min(u16::MAX as usize),
    }
}

/// A cell's style as libghostty's formatter sees it (the hyperlink is not
/// part of it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Look {
    fg: Color,
    bg: Color,
    flags: u16,
    /// [`underline`] kinds.
    underline: u8,
    ul: Color,
}

impl Look {
    fn of(s: &Style) -> Look {
        let underline = if s.flags & flags::UNDERLINE != 0 { s.underline.max(underline::SINGLE) } else { 0 };
        Look { fg: s.fg, bg: s.bg, flags: s.flags & !flags::UNDERLINE, underline, ul: s.ul }
    }

    fn is_default(&self) -> bool {
        *self == Look::default()
    }

    fn has(&self, f: u16) -> bool {
        self.flags & f != 0
    }
}

/// The formatter's state across rows and pages.
struct Writer {
    kind: Format,
    trim: bool,
    out: String,
    /// Blank rows and cells not written yet: written only when text follows
    /// (as libghostty's trailing state, kept from page to page).
    blank_rows: usize,
    blank_cells: usize,
}

impl Terminal {
    /// The active screen as libghostty-vt's formatter writes it with
    /// `trim` (trailing spaces dropped from plain text rows).
    pub fn format(&self, kind: Format, trim: bool) -> String {
        let screen = self.screen_ref();
        let history = if self.modes.alt_screen { None } else { Some(&self.scrollback) };
        let lines: Vec<&Line> = history.into_iter().flatten().chain(screen.lines.iter()).collect();
        let mut w = Writer { kind, trim, out: String::new(), blank_rows: 0, blank_cells: 0 };
        // One page at a time, as libghostty: each starts in the default
        // style, and in HTML has its own wrapper.
        for page in lines.chunks(rows_per_page(self.cols)) {
            w.page(self, page);
        }
        w.out
    }
}

impl Writer {
    fn styled(&self) -> bool {
        self.kind != Format::Plain
    }

    fn page(&mut self, t: &Terminal, lines: &[&Line]) {
        if self.kind == Format::Html {
            self.out.push_str("<div style=\"font-family: monospace; white-space: pre;\">");
        }
        let mut look = Look::default();
        // The open `<a>` (HTML), by link number; each page closes its own.
        let mut link = 0u16;
        let cols = t.cols as usize;
        for line in lines {
            // Scrollback lines keep no trailing blanks: past their end a
            // cell is blank.
            let cells = &line.cells[..line.cells.len().min(cols)];
            if cells.iter().all(|c| c.is_empty()) {
                self.blank_rows += 1;
                continue;
            }
            if self.blank_rows > 0 {
                // Back to the default style first, so that no background
                // runs into the next row (as libghostty).
                if !look.is_default() {
                    self.close();
                    look = Look::default();
                }
                let nl = if self.kind == Format::Vt { "\r\n" } else { "\n" };
                for _ in 0..self.blank_rows {
                    self.out.push_str(nl);
                }
                self.blank_rows = 0;
            }
            self.blank_rows = 1;
            self.blank_cells = 0;
            for c in cells {
                if matches!(c.wide, Wide::SpacerTail | Wide::SpacerHead) {
                    continue;
                }
                let text = t.text(c);
                let st = t.style(c);
                // A cell with neither text nor style is blank; in plain text
                // a space is too when trimming. Blanks are written only when
                // something follows them on the row. A hyperlink is not style
                // (libghostty's `hasStyling`).
                let styled_cell = self.styled() && (!c.is_empty() || Style { link: 0, ..st } != Style::default());
                if !styled_cell && (c.is_empty() || (self.trim && text.starts_with(' '))) {
                    self.blank_cells += 1;
                    continue;
                }
                if self.blank_cells > 0 {
                    for _ in 0..self.blank_cells {
                        self.out.push(' ');
                    }
                    self.blank_cells = 0;
                }
                if self.styled() {
                    let l = Look::of(&st);
                    if l != look {
                        // VT closes only to go back to the default: an
                        // opening sequence starts with a reset anyway.
                        if !look.is_default() && (self.kind == Format::Html || l.is_default()) {
                            self.close();
                        }
                        look = l;
                        if !l.is_default() {
                            self.open(&l);
                        }
                    }
                }
                // After the style, as libghostty: `<div …><a href=…>`.
                if self.kind == Format::Html && st.link != link {
                    if link != 0 {
                        self.out.push_str("</a>");
                    }
                    link = 0;
                    if let Some(l) = t.link(st.link) {
                        link = st.link;
                        // Byte by byte, each as a character (as libghostty).
                        self.out.push_str("<a href=\"");
                        for &b in l.uri.iter() {
                            self.char(b as char);
                        }
                        self.out.push_str("\">");
                    }
                }
                if c.is_empty() {
                    // Only a background: a space keeps it.
                    self.out.push(' ');
                } else {
                    for ch in text.chars() {
                        self.char(ch);
                    }
                }
            }
        }
        if !look.is_default() {
            self.close();
        }
        if link != 0 {
            self.out.push_str("</a>");
        }
        if self.kind == Format::Html {
            self.out.push_str("</div>");
        }
    }

    fn char(&mut self, ch: char) {
        if self.kind != Format::Html {
            self.out.push(ch);
            return;
        }
        // As libghostty: the five special characters as entities, and
        // everything outside ASCII as a numeric one.
        match ch {
            '<' => self.out.push_str("&lt;"),
            '>' => self.out.push_str("&gt;"),
            '&' => self.out.push_str("&amp;"),
            '"' => self.out.push_str("&quot;"),
            '\'' => self.out.push_str("&#39;"),
            c if (c as u32) < 0x80 => self.out.push(c),
            c => {
                let _ = std::fmt::Write::write_fmt(&mut self.out, format_args!("&#{};", c as u32));
            }
        }
    }

    fn close(&mut self) {
        self.out.push_str(if self.kind == Format::Html { "</div>" } else { "\x1b[0m" });
    }

    fn open(&mut self, l: &Look) {
        match self.kind {
            Format::Plain => {}
            Format::Vt => self.open_vt(l),
            Format::Html => self.open_html(l),
        }
    }

    /// As libghostty's `Style.formatterVt`: a reset, then one sequence per
    /// attribute.
    fn open_vt(&mut self, l: &Look) {
        let o = &mut self.out;
        o.push_str("\x1b[0m");
        for (f, s) in [
            (flags::BOLD, "\x1b[1m"),
            (flags::FAINT, "\x1b[2m"),
            (flags::ITALIC, "\x1b[3m"),
            (flags::BLINK, "\x1b[5m"),
            (flags::INVERSE, "\x1b[7m"),
            (flags::INVISIBLE, "\x1b[8m"),
            (flags::STRIKETHROUGH, "\x1b[9m"),
            (flags::OVERLINE, "\x1b[53m"),
        ] {
            if l.has(f) {
                o.push_str(s);
            }
        }
        o.push_str(match l.underline {
            underline::NONE => "",
            underline::DOUBLE => "\x1b[4:2m",
            underline::CURLY => "\x1b[4:3m",
            underline::DOTTED => "\x1b[4:4m",
            underline::DASHED => "\x1b[4:5m",
            _ => "\x1b[4m",
        });
        for (p, c) in [(38, l.fg), (48, l.bg), (58, l.ul)] {
            match c {
                Color::Default => {}
                Color::Index(i) => o.push_str(&format!("\x1b[{p};5;{i}m")),
                Color::Rgb([r, g, b]) => o.push_str(&format!("\x1b[{p};2;{r};{g};{b}m")),
            }
        }
    }

    /// As libghostty's `Style.formatterHtml`, in an inline `<div>`.
    fn open_html(&mut self, l: &Look) {
        let o = &mut self.out;
        o.push_str("<div style=\"display: inline;");
        for (p, c) in [("color", l.fg), ("background-color", l.bg), ("text-decoration-color", l.ul)] {
            match c {
                Color::Default => {}
                Color::Index(i) => o.push_str(&format!("{p}: var(--vt-palette-{i});")),
                Color::Rgb([r, g, b]) => o.push_str(&format!("{p}: rgb({r}, {g}, {b});")),
            }
        }
        let underline = l.underline != underline::NONE;
        if underline || l.has(flags::STRIKETHROUGH) || l.has(flags::OVERLINE) || l.has(flags::BLINK) {
            o.push_str("text-decoration-line:");
            for (on, s) in [
                (underline, " underline"),
                (l.has(flags::STRIKETHROUGH), " line-through"),
                (l.has(flags::OVERLINE), " overline"),
                (l.has(flags::BLINK), " blink"),
            ] {
                if on {
                    o.push_str(s);
                }
            }
            o.push(';');
        }
        o.push_str(match l.underline {
            underline::NONE => "",
            underline::DOUBLE => "text-decoration-style: double;",
            underline::CURLY => "text-decoration-style: wavy;",
            underline::DOTTED => "text-decoration-style: dotted;",
            underline::DASHED => "text-decoration-style: dashed;",
            _ => "text-decoration-style: solid;",
        });
        for (f, s) in [
            (flags::BOLD, "font-weight: bold;"),
            (flags::ITALIC, "font-style: italic;"),
            (flags::FAINT, "opacity: 0.5;"),
            (flags::INVISIBLE, "visibility: hidden;"),
            (flags::INVERSE, "filter: invert(100%);"),
        ] {
            if l.has(f) {
                o.push_str(s);
            }
        }
        o.push_str("\">");
    }
}

#[cfg(test)]
mod tests {
    use super::Format::{Html, Plain, Vt};
    use crate::{Colors, Options, Terminal};

    fn term(cols: u16, rows: u16, input: &str) -> Terminal {
        let mut t = Terminal::new(Options {
            cols,
            rows,
            cell_width: 9,
            cell_height: 18,
            colors: Colors { fg: [204; 3], bg: [20; 3], palette: None, cursor: None },
            dark: Some(true),
            xtversion: "haetty(test)".into(),
            scrollback_limit: 1 << 20,
        });
        t.feed(input.as_bytes());
        t
    }

    const DIV: &str = "<div style=\"font-family: monospace; white-space: pre;\">";

    // The expected bytes below are libghostty-vt 0.2.2's, from
    // `haetty-vtdiff --probe`.

    /// Blank rows and cells, trailing spaces, background-only cells.
    #[test]
    fn blanks_and_trimming() {
        let t = term(8, 6, "  a  \r\n\r\n   \r\n\x1b[44m\x1b[K\r\nb \x1b[1;31mc\x1b[0m d\x1b[45m\x1b[K");
        assert_eq!(t.format(Plain, true), "  a\n\n\n\nb c d");
        assert_eq!(
            t.format(Vt, true),
            "  a  \r\n\r\n   \r\n\r\n\x1b[0m\x1b[48;5;4mb \x1b[0m\x1b[1m\x1b[38;5;1m\x1b[48;5;4mc\x1b[0m d\
             \x1b[0m\x1b[48;5;5m   \x1b[0m"
        );
        assert_eq!(
            t.format(Html, true),
            format!(
                "{DIV}  a  \n\n   \n\n<div style=\"display: inline;background-color: var(--vt-palette-4);\">b </div>\
                 <div style=\"display: inline;color: var(--vt-palette-1);background-color: var(--vt-palette-4);\
                 font-weight: bold;\">c</div> d<div style=\"display: inline;background-color: var(--vt-palette-5);\">   \
                 </div></div>"
            )
        );
    }

    /// Every attribute, RGB and palette colours, a style kept across cells.
    #[test]
    fn attributes_and_colours() {
        let t =
            term(8, 2, "\x1b[2;3;4;5;7;8;9;53m1\x1b[0m2\x1b[38;2;1;2;3;48;5;200m3\x1b[m\x1b[30;47m4\x1b[1m\x1b[22m5");
        assert_eq!(t.format(Plain, true), "12345");
        assert_eq!(
            t.format(Vt, true),
            "\x1b[0m\x1b[2m\x1b[3m\x1b[5m\x1b[7m\x1b[8m\x1b[9m\x1b[53m\x1b[4m1\x1b[0m2\
             \x1b[0m\x1b[38;2;1;2;3m\x1b[48;5;200m3\x1b[0m\x1b[38;5;0m\x1b[48;5;7m45\x1b[0m"
        );
        assert_eq!(
            t.format(Html, true),
            format!(
                "{DIV}<div style=\"display: inline;text-decoration-line: underline line-through overline blink;\
                 text-decoration-style: solid;font-style: italic;opacity: 0.5;visibility: hidden;\
                 filter: invert(100%);\">1</div>2<div style=\"display: inline;color: rgb(1, 2, 3);\
                 background-color: var(--vt-palette-200);\">3</div><div style=\"display: inline;\
                 color: var(--vt-palette-0);background-color: var(--vt-palette-7);\">45</div></div>"
            )
        );
    }

    /// Underline kinds and colours; a hyperlink in HTML (after the style's
    /// `<div>`, closed when the link changes), not in VT.
    #[test]
    fn underlines_and_hyperlinks() {
        let t = term(
            16,
            2,
            "\x1b[4:3;58;2;1;2;3ma\x1b[21mb\x1b[0m\x1b]8;;http://x/\u{e9}\x1b\\\x1b[1mc\x1b[0md\x1b]8;;\x1b\\e",
        );
        assert_eq!(t.format(Plain, true), "abcde");
        assert_eq!(
            t.format(Vt, true),
            "\x1b[0m\x1b[4:3m\x1b[58;2;1;2;3ma\x1b[0m\x1b[4:2m\x1b[58;2;1;2;3mb\x1b[0m\x1b[1mc\x1b[0mde"
        );
        assert_eq!(
            t.format(Html, true),
            format!(
                "{DIV}<div style=\"display: inline;text-decoration-color: rgb(1, 2, 3);text-decoration-line: underline;\
                 text-decoration-style: wavy;\">a</div><div style=\"display: inline;text-decoration-color: rgb(1, 2, 3);\
                 text-decoration-line: underline;text-decoration-style: double;\">b</div><div style=\"display: inline;\
                 font-weight: bold;\"><a href=\"http://x/&#195;&#169;\">c</div>d</a>e</div>"
            )
        );
    }

    /// HTML escapes, wide characters, clusters; the scrollback is dumped too.
    #[test]
    fn escapes_wide_clusters_and_scrollback() {
        let t = term(8, 2, "<&>\"' \u{4e2d} e\u{301}\r\n1\r\n2\r\n3");
        assert_eq!(t.format(Plain, true), "<&>\"' \u{4e2d}\n e\u{301}\n1\n2\n3");
        assert_eq!(t.format(Vt, true), "<&>\"' \u{4e2d}\r\n e\u{301}\r\n1\r\n2\r\n3");
        assert_eq!(t.format(Html, true), format!("{DIV}&lt;&amp;&gt;&quot;&#39; &#20013;\n e&#769;\n1\n2\n3</div>"));
    }

    /// The alternate screen alone, without the primary's rows.
    #[test]
    fn alternate_screen() {
        let t = term(8, 2, "main\r\n\x1b[?1049halt");
        assert_eq!(t.format(Plain, true), "\nalt");
        assert_eq!(t.format(Vt, true), "\r\nalt");
        assert_eq!(t.format(Html, true), format!("{DIV}\nalt</div>"));
    }

    /// At 215 columns a page holds 217 rows: HTML wraps each page, the
    /// style closes at its end and the newline waits for the next page.
    #[test]
    fn html_pages() {
        let t = term(215, 3, &format!("\x1b[31m{}y", "x\r\n".repeat(218)));
        let cell = |c: char| format!("<div style=\"display: inline;color: var(--vt-palette-1);\">{c}</div>");
        let page1 = vec![cell('x'); 217].join("\n");
        let (x, y) = (cell('x'), cell('y'));
        assert_eq!(t.format(Html, true), format!("{DIV}{page1}</div>{DIV}\n{x}\n{y}</div>"));
        let vt = "\x1b[0m\x1b[38;5;1mx\x1b[0m\r\n".repeat(218);
        assert_eq!(t.format(Vt, true), format!("{vt}\x1b[0m\x1b[38;5;1my\x1b[0m"));
    }
}
