//! libghostty's render state and characters that join a cell already drawn
//! (DESIGN.md §10.8): a combining mark, VS16 or ZWJ in a later read than its
//! base. The incremental render state misses such rows; the engine's
//! snapshots must not.
#![cfg(feature = "ghostty")]

use haetty_core::ghostty::{GhosttyEngine, Refresh};
use haetty_core::vt::{VtEngine, VtInit};
use haetty_core::{ColorSchemePref, GridSize, Rgb, TermColors};

fn engine(r: Option<Refresh>) -> GhosttyEngine {
    let mut e = GhosttyEngine::new(&VtInit {
        size: GridSize::new(20, 4).with_cell(9, 18),
        scrollback_limit: 1 << 20,
        colors: TermColors {
            default_fg: Rgb { r: 0xcc, g: 0xcc, b: 0xcc },
            default_bg: Rgb { r: 0, g: 0, b: 0 },
            palette: None,
            cursor_color: None,
        },
        color_scheme: ColorSchemePref::Dark,
        xtversion: "test".into(),
    })
    .expect("engine");
    if let Some(r) = r {
        e.set_refresh(r);
    }
    e
}

/// Each case: the reads, in order.
fn cases() -> Vec<(&'static str, Vec<&'static [u8]>)> {
    vec![
        ("combining", vec![b"cafe", "\u{301}".as_bytes()]),
        ("vs16", vec!["\u{2764}".as_bytes(), "\u{fe0f}".as_bytes()]),
        ("vs16 on a narrow base", vec!["ab\u{263a}".as_bytes(), "\u{fe0f}".as_bytes()]),
        ("zwj", vec!["\u{1f468}".as_bytes(), "\u{200d}".as_bytes(), "\u{1f469}".as_bytes()]),
        ("vs16 split inside", vec!["\u{2764}".as_bytes(), b"\xef", b"\xb8\x8f"]),
        ("2027 combining", vec!["\x1b[?2027hcafe".as_bytes(), "\u{301}".as_bytes()]),
        ("2027 zwj", vec!["\x1b[?2027h\u{1f468}".as_bytes(), "\u{200d}".as_bytes(), "\u{1f469}".as_bytes()]),
    ]
}

/// Feeds the reads with a snapshot after each; returns the last snapshot's
/// first row and the same from a render state built fresh.
fn run(r: Option<Refresh>, reads: &[&[u8]]) -> (String, Vec<haetty_core::Cell>, Vec<haetty_core::Cell>) {
    let mut e = engine(r);
    let mut last = None;
    for b in reads {
        e.feed(b);
        last = Some(e.snapshot());
    }
    let got = last.expect("a read");
    let want = e.snapshot_fresh();
    (got.row_text(0), got.cells, want.cells)
}

#[test]
fn joining_characters_in_a_later_read_are_drawn() {
    for (name, reads) in cases() {
        let (text, got, want) = run(None, &reads);
        assert_eq!(got, want, "{name}: the default refresh shows {text:?}");
    }
}

/// The test above would catch the defect: the incremental render state alone
/// leaves these rows stale.
#[test]
fn the_incremental_render_state_alone_misses_them() {
    let stale: Vec<&str> = cases()
        .into_iter()
        .filter(|(_, reads)| run(Some(Refresh::Incremental), reads).1 != run(None, reads).2)
        .map(|(n, _)| n)
        .collect();
    for n in ["combining", "vs16", "vs16 on a narrow base", "vs16 split inside"] {
        assert!(stale.contains(&n), "{n}: not stale with Refresh::Incremental (stale: {stale:?})");
    }
}
