use libghostty_vt::{Terminal, TerminalOptions};
use libghostty_vt::fmt::{Formatter, FormatterOptions, Format};
use std::time::Instant;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&a[1]).unwrap();
    let (cols, rows): (u16, u16) = (a[2].parse().unwrap(), a[3].parse().unwrap());
    let reps: usize = a.get(4).map(|s| s.parse().unwrap()).unwrap_or(1);
    // max_scrollback is a byte limit; 11_000_000 keeps ~10 200 rows of 120 columns,
    // comparable to alacritty's 10 000 lines (5th argument, bytes).
    let max_scrollback: usize = a.get(5).map(|s| s.parse().unwrap()).unwrap_or(11_000_000);
    let mut term = Terminal::new(TerminalOptions { cols, rows, max_scrollback }).unwrap();
    let t0 = Instant::now();
    for _ in 0..reps { for ch in data.chunks(65536) { term.vt_write(ch); } }
    let dt = t0.elapsed();
    let mut f = Formatter::new(&term, FormatterOptions::new().with_format(Format::Plain).with_trim(true)).unwrap();
    let txt = String::from_utf8_lossy(&f.format_alloc(None).unwrap().to_vec()).into_owned();
    eprintln!("ghostty: {} MB in {:.3}s = {:.1} MB/s", data.len()*reps/1_000_000, dt.as_secs_f64(), (data.len()*reps) as f64/1e6/dt.as_secs_f64());
    let lines: Vec<&str> = txt.lines().collect();
    let tail = &lines[lines.len().saturating_sub(rows as usize)..];
    println!("{}", tail.join("\n"));
}
