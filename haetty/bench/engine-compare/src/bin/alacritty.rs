use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::{Config, Term, cell::Flags};
use alacritty_terminal::vte::ansi::Processor;
use std::time::Instant;
struct Size { c: usize, r: usize }
impl Dimensions for Size { fn total_lines(&self)->usize{self.r} fn screen_lines(&self)->usize{self.r} fn columns(&self)->usize{self.c} }
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&a[1]).unwrap();
    let (cols, rows): (usize, usize) = (a[2].parse().unwrap(), a[3].parse().unwrap());
    let reps: usize = a.get(4).map(|s| s.parse().unwrap()).unwrap_or(1);
    // Lines of history (5th argument), default 10 000.
    let mut cfg = Config::default(); cfg.scrolling_history = a.get(5).map(|s| s.parse().unwrap()).unwrap_or(10_000);
    let mut term = Term::new(cfg, &Size{c:cols,r:rows}, VoidListener);
    let mut p: Processor = Processor::new();
    let t0 = Instant::now();
    for _ in 0..reps { for ch in data.chunks(65536) { p.advance(&mut term, ch); } }
    let dt = t0.elapsed();
    eprintln!("alacritty: {} MB in {:.3}s = {:.1} MB/s", data.len()*reps/1_000_000, dt.as_secs_f64(), (data.len()*reps) as f64/1e6/dt.as_secs_f64());
    let grid = term.grid();
    let mut out = vec![];
    for l in 0..rows {
        let row = &grid[Line(l as i32)];
        let mut s = String::new();
        for c in 0..cols { let cell = &row[Column(c)]; if cell.flags.contains(Flags::WIDE_CHAR_SPACER) { continue; } s.push(cell.c); if let Some(z)=cell.zerowidth(){ for ch in z { s.push(*ch);} } }
        out.push(s.trim_end().to_string());
    }
    while out.last().is_some_and(|l| l.is_empty()) { out.pop(); }
    println!("{}", out.join("\n"));
}
