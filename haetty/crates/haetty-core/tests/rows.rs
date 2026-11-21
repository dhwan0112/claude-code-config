use haetty_core::vt::{new_engine, EngineKind, VtEngine, VtInit};
use haetty_core::{ColorSchemePref, GridSize, Rgb, Snapshot, TermColors, Width};

fn engine(kind: EngineKind, cols: u16, rows: u16, scrollback: usize) -> Box<dyn VtEngine> {
    let init = VtInit {
        size: GridSize::new(cols, rows),
        scrollback_limit: scrollback,
        colors: TermColors {
            default_fg: Rgb { r: 200, g: 200, b: 200 },
            default_bg: Rgb { r: 0, g: 0, b: 0 },
            palette: None,
            cursor_color: None,
        },
        color_scheme: ColorSchemePref::Unknown,
        xtversion: "test".into(),
    };
    new_engine(kind, &init).expect("engine").0
}

fn kinds() -> Vec<EngineKind> {
    let mut v = vec![EngineKind::Native];
    if cfg!(feature = "ghostty") {
        v.push(EngineKind::Ghostty);
    }
    v
}

fn number(s: &Snapshot, y: u16) -> u64 {
    s.scrollbar.dropped + s.scrollbar.offset + y as u64
}

fn line_no(text: &str) -> Option<u64> {
    text.strip_prefix('L')?.split_whitespace().next()?.parse().ok()
}

fn row_string(r: &haetty_core::TextRow) -> String {
    let s: String = r
        .cells
        .iter()
        .filter(|(_, w)| *w != Width::Spacer)
        .map(|(t, _)| if t.is_empty() { " " } else { t.as_str() })
        .collect();
    s.trim_end().to_owned()
}

#[test]
fn a_row_keeps_its_number_while_output_pushes_it_up_and_the_scrollback_drops_rows() {
    for kind in kinds() {
        let mut e = engine(kind, 40, 10, 64 * 1024);
        let mut offset_of_line: Option<i128> = None;
        let mut dropped_seen = 0;
        for chunk in 0..200u64 {
            let mut out = String::new();
            for k in chunk * 100..(chunk + 1) * 100 {
                out.push_str(&format!("L{k} {}\r\n", "x".repeat(20)));
            }
            e.feed(out.as_bytes());
            let s = e.snapshot();
            dropped_seen = dropped_seen.max(s.scrollbar.dropped);
            for y in 0..s.rows {
                if let Some(k) = line_no(&s.row_text(y)) {
                    let d = number(&s, y) as i128 - k as i128;
                    assert_eq!(*offset_of_line.get_or_insert(d), d, "{kind:?}: line L{k} moved to another number");
                }
            }
        }
        assert!(dropped_seen > 0, "{kind:?}: 20 000 lines through a 64 KiB scrollback dropped no row");
        let d = offset_of_line.unwrap();
        let s = e.snapshot();
        let oldest = s.scrollbar.dropped;
        let got = e.rows(oldest, 3);
        assert_eq!(got.first, oldest);
        let k0 = line_no(&row_string(&got.rows[0])).expect("a numbered line");
        assert_eq!(oldest as i128 - k0 as i128, d, "{kind:?}: rows() and the snapshot disagree on numbers");
        assert_eq!(line_no(&row_string(&got.rows[2])), Some(k0 + 2));
        let gone = e.rows(0, 2);
        assert_eq!(gone.first, oldest, "{kind:?}: a dropped row was given out again");
    }
}

#[test]
fn soft_wraps_are_marked_on_screen_and_in_the_scrollback() {
    for kind in kinds() {
        let mut e = engine(kind, 10, 4, 1 << 20);
        e.feed(b"0123456789abcde\r\nshort\r\n");
        let s = e.snapshot();
        assert_eq!(s.wrapped.len(), 4, "{kind:?}");
        assert!(s.is_wrapped(0), "{kind:?}: the long line wraps");
        assert!(!s.is_wrapped(1) && !s.is_wrapped(2), "{kind:?}");
        let first = number(&s, 0);
        let r = e.rows(first, 3);
        assert!(r.rows[0].wrapped && !r.rows[1].wrapped, "{kind:?}");
        assert_eq!(row_string(&r.rows[0]), "0123456789");
        assert_eq!(row_string(&r.rows[1]), "abcde");
        e.feed("a\r\nb\r\nc\r\nd\r\n".as_bytes());
        let r = e.rows(first, 2);
        assert_eq!(r.first, first, "{kind:?}: rows in the scrollback keep their numbers");
        assert!(r.rows[0].wrapped, "{kind:?}: the wrap survives the scrollback");
    }
}

#[test]
fn wide_characters_and_clusters_come_back_cell_by_cell() {
    for kind in kinds() {
        let mut e = engine(kind, 12, 3, 1 << 20);
        e.feed("가a e\u{301}\r\n".as_bytes());
        let s = e.snapshot();
        let r = e.rows(number(&s, 0), 1);
        let cells = &r.rows[0].cells;
        assert_eq!(cells[0], ("가".to_string(), Width::Wide), "{kind:?}");
        assert_eq!(cells[1].1, Width::Spacer, "{kind:?}");
        assert_eq!(cells[2].0, "a", "{kind:?}");
        assert_eq!(cells[4].0, "e\u{301}", "{kind:?}");
    }
}

#[test]
fn a_tab_leaves_blank_cells_so_a_copy_gives_spaces() {
    for kind in kinds() {
        let mut e = engine(kind, 20, 3, 1 << 20);
        e.feed(b"a\tb\r\n");
        let s = e.snapshot();
        let r = e.rows(number(&s, 0), 1);
        let cells = &r.rows[0].cells;
        assert_eq!(cells[0].0, "a", "{kind:?}");
        assert!(cells[1..8].iter().all(|(t, _)| t.is_empty()), "{kind:?}: no tab kept in a cell");
        assert_eq!(cells[8].0, "b", "{kind:?}");
    }
}
