//! Feeding speed of haetty-vt alone, for the cases of `scripts/e2e/measure.sh`
//! (plain lines, long wrapped lines, SGR-heavy): `cargo run --release -p
//! haetty-vt --example feed [CASE]`. Reads of 64 KiB, 80x24, the default
//! scrollback limit.
use haetty_vt::{Colors, Options, Terminal, DEFAULT_16};
use std::time::Instant;

fn data(case: &str) -> Vec<u8> {
    let mut b = Vec::new();
    match case {
        "lines" => {
            (0..400_000).for_each(|i| b.extend_from_slice(format!("line {i} of plain text output\n").as_bytes()))
        }
        "wrapped" => (0..12_000).for_each(|_| {
            b.extend_from_slice(&[b'x'; 1000]);
            b.push(b'\n')
        }),
        _ => (0..1_000_000u32).for_each(|i| {
            b.extend_from_slice(
                format!("\x1b[38;2;{};{};{}m{i}\x1b[0m ", i % 256, i * 7 % 256, i * 13 % 256).as_bytes(),
            )
        }),
    }
    b
}

fn main() {
    let cases: Vec<String> = std::env::args().skip(1).collect();
    let cases = if cases.is_empty() { vec!["lines".into(), "wrapped".into(), "sgr".into()] } else { cases };
    for case in cases {
        let d = data(&case);
        let mut best = f64::MAX;
        for _ in 0..3 {
            let mut t = Terminal::new(Options {
                cols: 80,
                rows: 24,
                cell_width: 9,
                cell_height: 18,
                scrollback_limit: 10_000_000,
                colors: Colors { fg: [204; 3], bg: [20; 3], cursor: None, palette: Some(DEFAULT_16) },
                dark: None,
                xtversion: "bench".into(),
            });
            let start = Instant::now();
            for chunk in d.chunks(64 << 10) {
                t.feed(chunk);
                let _ = t.take_replies();
            }
            best = best.min(start.elapsed().as_secs_f64());
        }
        println!(
            "{case}: {:.1} MiB in {:.0} ms = {:.0} MiB/s",
            d.len() as f64 / 1048576.0,
            best * 1e3,
            d.len() as f64 / 1048576.0 / best
        );
    }
}
