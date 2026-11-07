//! haetty's own terminal engine in pure Rust (DESIGN.md §10). It knows
//! nothing of haetty's sessions or wire types: `haetty-core` wraps it in its
//! engine trait (feature `native`), and `haetty-vtdiff` compares it with
//! libghostty-vt byte stream by byte stream.
//!
//! Stage E1: a skeleton for the differential tool — printable characters,
//! CR, LF, BS, wrapping at the right edge and scrolling at the bottom. The
//! engine proper is stage E2.

/// One cell of the screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    /// The grapheme cluster; empty for a blank cell.
    pub text: String,
}

pub struct Terminal {
    cols: u16,
    rows: u16,
    cells: Vec<Cell>,
    cx: u16,
    cy: u16,
    /// Rows scrolled off the top (the skeleton keeps none of their content).
    scrolled: u64,
    parser: vte::Parser,
    replies: Vec<u8>,
}

impl Terminal {
    pub fn new(cols: u16, rows: u16) -> Terminal {
        let (cols, rows) = (cols.max(1), rows.max(1));
        Terminal {
            cols,
            rows,
            cells: vec![Cell::default(); cols as usize * rows as usize],
            cx: 0,
            cy: 0,
            scrolled: 0,
            parser: vte::Parser::new(),
            replies: Vec::new(),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        let mut parser = std::mem::take(&mut self.parser);
        parser.advance(self, bytes);
        self.parser = parser;
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let (cols, rows) = (cols.max(1), rows.max(1));
        let mut cells = vec![Cell::default(); cols as usize * rows as usize];
        for y in 0..rows.min(self.rows) {
            for x in 0..cols.min(self.cols) {
                cells[y as usize * cols as usize + x as usize] = self.cell(x, y).clone();
            }
        }
        self.cells = cells;
        self.cols = cols;
        self.rows = rows;
        self.cx = self.cx.min(cols - 1);
        self.cy = self.cy.min(rows - 1);
    }

    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    pub fn cell(&self, x: u16, y: u16) -> &Cell {
        &self.cells[y as usize * self.cols as usize + x as usize]
    }

    pub fn cursor(&self) -> (u16, u16) {
        (self.cx, self.cy)
    }

    pub fn scrollback_rows(&self) -> u64 {
        self.scrolled
    }

    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    fn linefeed(&mut self) {
        if self.cy + 1 < self.rows {
            self.cy += 1;
        } else {
            self.cells.drain(..self.cols as usize);
            self.cells.extend(std::iter::repeat_n(Cell::default(), self.cols as usize));
            self.scrolled += 1;
        }
    }
}

impl vte::Perform for Terminal {
    fn print(&mut self, c: char) {
        if self.cx >= self.cols {
            self.cx = 0;
            self.linefeed();
        }
        let i = self.cy as usize * self.cols as usize + self.cx as usize;
        self.cells[i] = Cell { text: c.to_string() };
        self.cx += 1;
    }

    fn execute(&mut self, b: u8) {
        match b {
            b'\r' => self.cx = 0,
            b'\n' | 0x0b | 0x0c => self.linefeed(),
            0x08 => self.cx = self.cx.min(self.cols - 1).saturating_sub(1),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_wraps_and_scrolls() {
        let mut t = Terminal::new(4, 2);
        t.feed(b"abcdef\r\nxy");
        assert_eq!(t.cell(0, 0).text, "e");
        assert_eq!(t.cell(1, 0).text, "f");
        assert_eq!(t.cell(0, 1).text, "x");
        assert_eq!(t.cursor(), (2, 1));
        assert_eq!(t.scrollback_rows(), 1);
    }
}
