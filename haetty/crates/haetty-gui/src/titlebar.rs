//! `decorations = integrated` (DESIGN.md §2.2, §3.12): haetty's own title
//! row. The tab-bar row is the title bar; it stays with one tab and then
//! shows the window title, dim. The window buttons are single-colour glyphs
//! on the cell grid — dim, the normal text colour under the pointer, close
//! included (no accent) — on the side(s) GNOME's `button-layout` names
//! (right when it cannot be read). This module is the row's layout (one
//! function for drawing and hit testing), its cells, the button layout and
//! the window-edge test for resizing.

use std::ops::Range;

use haetty_core::{Cell, Rgb};

use crate::ui::{self, Greys, TabView};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Minimize,
    Maximize,
    Close,
}

impl Button {
    fn glyph(self, maximized: bool) -> &'static str {
        match self {
            Button::Minimize => "–",
            // A filled square while maximized: "restore".
            Button::Maximize if maximized => "■",
            Button::Maximize => "□",
            Button::Close => "×",
        }
    }
}

/// Which buttons on which side, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ButtonLayout {
    pub left: Vec<Button>,
    pub right: Vec<Button>,
}

impl Default for ButtonLayout {
    /// Right: minimize, maximize, close (when the desktop's setting cannot be read).
    fn default() -> Self {
        ButtonLayout { left: vec![], right: vec![Button::Minimize, Button::Maximize, Button::Close] }
    }
}

impl ButtonLayout {
    /// No buttons (the tab bar of a natively decorated window).
    pub fn none() -> Self {
        ButtonLayout { left: vec![], right: vec![] }
    }

    /// GNOME's `org.gnome.desktop.wm.preferences button-layout`, e.g.
    /// `appmenu:minimize,maximize,close` or `close,minimize,maximize:` (quotes
    /// allowed; `appmenu`, `menu`, `icon`, `spacer` are ignored). `None` when
    /// there is no `:`.
    #[cfg_attr(windows, allow(dead_code))]
    pub fn parse(s: &str) -> Option<ButtonLayout> {
        let s = s.trim().trim_matches(['\'', '"']);
        let (l, r) = s.split_once(':')?;
        let side = |x: &str| -> Vec<Button> {
            let mut v = vec![];
            for b in x.split(',').map(str::trim).filter_map(|t| match t {
                "minimize" => Some(Button::Minimize),
                "maximize" => Some(Button::Maximize),
                "close" => Some(Button::Close),
                _ => None,
            }) {
                if !v.contains(&b) {
                    v.push(b);
                }
            }
            v
        };
        Some(ButtonLayout { left: side(l), right: side(r) })
    }
}

/// The button layout: `HAETTY_TITLE_BUTTONS` (GNOME's syntax, e.g.
/// `close,minimize,maximize:`) when set, else the desktop's — the settings
/// portal (`dbus-send`, as winit's own Adwaita frame does), else
/// `gsettings`, each at most `timeout` — else the default (right).
#[cfg(all(unix, not(target_os = "macos")))]
pub fn read_system(timeout: std::time::Duration) -> (ButtonLayout, &'static str) {
    if let Some(l) = haetty_proto::product::env("TITLE_BUTTONS").as_deref().and_then(ButtonLayout::parse) {
        return (l, "HAETTY_TITLE_BUTTONS");
    }
    let portal = [
        "dbus-send",
        "--reply-timeout=150",
        "--print-reply=literal",
        "--dest=org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings.Read",
        "string:org.gnome.desktop.wm.preferences",
        "string:button-layout",
    ];
    if let Some(l) = run(&portal, timeout).and_then(|o| o.split_whitespace().last().and_then(ButtonLayout::parse)) {
        return (l, "settings portal");
    }
    let gs = ["gsettings", "get", "org.gnome.desktop.wm.preferences", "button-layout"];
    if let Some(l) = run(&gs, timeout).and_then(|o| ButtonLayout::parse(&o)) {
        return (l, "gsettings");
    }
    (ButtonLayout::default(), "default")
}

/// Runs a program (no shell) and returns its stdout if it ends within
/// `timeout`; a program still running then is killed (our own child).
#[cfg(all(unix, not(target_os = "macos")))]
fn run(argv: &[&str], timeout: std::time::Duration) -> Option<String> {
    use std::process::{Command, Stdio};
    let mut child = Command::new(argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let end = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                let mut out = String::new();
                if st.success() {
                    use std::io::Read;
                    child.stdout.take()?.read_to_string(&mut out).ok()?;
                }
                return st.success().then_some(out);
            }
            Ok(None) if std::time::Instant::now() < end => std::thread::sleep(std::time::Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Cells of a button (glyph with a space on each side).
pub const BUTTON_W: usize = 3;

/// Where things are in the title row of `cols` cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitleLayout {
    /// Cells for the tabs (or the title).
    pub tabs: Range<usize>,
    pub buttons: Vec<(Button, Range<usize>)>,
}

pub fn layout(cols: usize, b: &ButtonLayout) -> TitleLayout {
    let lw = (b.left.len() * BUTTON_W).min(cols);
    let rw = (b.right.len() * BUTTON_W).min(cols - lw);
    let mut buttons = vec![];
    for (i, &k) in b.left.iter().enumerate() {
        let x = i * BUTTON_W;
        if x + BUTTON_W <= lw {
            buttons.push((k, x..x + BUTTON_W));
        }
    }
    let r0 = cols - rw;
    for (i, &k) in b.right.iter().enumerate() {
        let x = r0 + i * BUTTON_W;
        if x + BUTTON_W <= cols {
            buttons.push((k, x..x + BUTTON_W));
        }
    }
    // One blank cell between the buttons and the tabs on each side that has buttons.
    let start = if lw > 0 { lw + 1 } else { 0 };
    let end = if rw > 0 { r0.saturating_sub(1) } else { cols };
    TitleLayout { tabs: start..end.max(start), buttons }
}

/// What a click at column `col` of the title row hits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Tab(usize),
    Button(Button),
    Empty,
}

pub fn hit(t: &TitleLayout, col: usize, tabs: usize) -> Hit {
    if let Some((b, _)) = t.buttons.iter().find(|(_, r)| r.contains(&col)) {
        return Hit::Button(*b);
    }
    if tabs >= 2 && t.tabs.contains(&col) {
        let share = ui::tab_share(t.tabs.len(), tabs);
        let i = (col - t.tabs.start) / share;
        if i < tabs {
            return Hit::Tab(i);
        }
    }
    Hit::Empty
}

/// The title row: tabs (two or more) or the window title (dim), and the
/// buttons — dim, `g.strong` for the one under the pointer.
#[allow(clippy::too_many_arguments)]
pub fn row(
    tabs: &[TabView],
    active: usize,
    cols: usize,
    g: Greys,
    title: &str,
    b: &ButtonLayout,
    hover: Option<Button>,
    maximized: bool,
) -> Vec<Cell> {
    let t = layout(cols, b);
    let mut row = vec![Cell { fg: Some(g.dim), bg: Some(g.bg), ..Default::default() }; cols];
    let w = t.tabs.len();
    if tabs.len() >= 2 {
        let bar = ui::tab_bar(tabs, active, w, g);
        row[t.tabs.clone()].clone_from_slice(&bar);
    } else if w > 2 {
        ui::put(&mut row[t.tabs.clone()], 1, &ui::fit(title, w - 2), g.dim, g.bg, false);
    }
    for (k, r) in &t.buttons {
        let fg: Rgb = if hover == Some(*k) { g.strong } else { g.dim };
        ui::put(&mut row[r.clone()], 1, k.glyph(maximized), fg, g.bg, false);
    }
    row
}

/// The window edge (or corner) under a pointer at (x, y) in a window of
/// `w` × `h` physical pixels, with a grab zone of `border` pixels.
pub fn edge_at(x: f64, y: f64, w: u32, h: u32, border: f64) -> Option<winit::window::ResizeDirection> {
    use winit::window::ResizeDirection as D;
    let (w, h) = (w as f64, h as f64);
    let (l, r, t, b) = (x < border, x >= w - border, y < border, y >= h - border);
    Some(match (l, r, t, b) {
        (true, _, true, _) => D::NorthWest,
        (_, true, true, _) => D::NorthEast,
        (true, _, _, true) => D::SouthWest,
        (_, true, _, true) => D::SouthEast,
        (true, ..) => D::West,
        (_, true, ..) => D::East,
        (_, _, true, _) => D::North,
        (_, _, _, true) => D::South,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use haetty_proto::wire::Attention;
    use winit::window::ResizeDirection as D;

    #[test]
    fn button_layouts_parse_like_gnome() {
        use Button::*;
        assert_eq!(
            ButtonLayout::parse("'appmenu:minimize,maximize,close'").unwrap().right,
            [Minimize, Maximize, Close]
        );
        let l = ButtonLayout::parse("close,minimize,maximize:appmenu").unwrap();
        assert_eq!((l.left, l.right), (vec![Close, Minimize, Maximize], vec![]));
        assert_eq!(ButtonLayout::parse("appmenu:close").unwrap().right, [Close]);
        assert_eq!(ButtonLayout::parse(":"), Some(ButtonLayout::none()));
        assert_eq!(ButtonLayout::parse("close"), None, "no colon");
        assert_eq!(
            ButtonLayout::parse("menu,close,close:spacer").unwrap().left,
            [Close],
            "duplicates and others dropped"
        );
        assert_eq!(ButtonLayout::default().right, [Minimize, Maximize, Close]);
    }

    #[test]
    fn layout_and_hits_agree_with_the_drawn_row() {
        let b = ButtonLayout::default();
        let t = layout(60, &b);
        assert_eq!(t.tabs, 0..50, "three buttons of 3 cells and a gap on the right");
        assert_eq!(t.buttons[2], (Button::Close, 57..60));
        assert_eq!(hit(&t, 58, 3), Hit::Button(Button::Close));
        assert_eq!(hit(&t, 51, 3), Hit::Button(Button::Minimize));
        assert_eq!(hit(&t, 50, 3), Hit::Empty, "the gap");
        assert_eq!(hit(&t, 0, 3), Hit::Tab(0));
        assert_eq!(hit(&t, 20, 3), Hit::Tab(1), "share 16");
        assert_eq!(hit(&t, 49, 3), Hit::Empty, "past the last tab");
        assert_eq!(hit(&t, 5, 1), Hit::Empty, "one tab: the title is empty space (drag)");
        let left = layout(60, &ButtonLayout::parse("close:").unwrap());
        assert_eq!((left.tabs.clone(), left.buttons.clone()), (4..60, vec![(Button::Close, 0..3)]));
        assert_eq!(hit(&left, 4, 2), Hit::Tab(0));
        let none = layout(60, &ButtonLayout::none());
        assert_eq!((none.tabs, none.buttons.len()), (0..60, 0));
    }

    #[test]
    fn title_row_cells() {
        let g = Greys::from(Rgb { r: 220, g: 220, b: 220 }, Rgb { r: 30, g: 30, b: 30 });
        let one = [TabView { name: "bash", exited: false, attention: Attention::None, observed: false }];
        let b = ButtonLayout::default();
        let r = row(&one, 0, 40, g, "haetty", &b, Some(Button::Close), false);
        let text: String = r.iter().map(|c| if c.text.is_empty() { " ".into() } else { c.text.clone() }).collect();
        assert!(text.starts_with(" haetty") && text.trim_end().ends_with("–  □  ×"), "{text:?}");
        assert!(r.iter().all(|c| c.bg == Some(g.bg)), "one background, no band");
        let glyph = |s: &str| r.iter().find(|c| c.text == s).unwrap().fg;
        assert_eq!(
            (glyph("h"), glyph("–"), glyph("□"), glyph("×")),
            (Some(g.dim), Some(g.dim), Some(g.dim), Some(g.strong))
        );
        // Maximized: the restore glyph; two tabs: the tab bar in the tab area.
        let two = [one[0].clone(), TabView { name: "vim", exited: false, attention: Attention::None, observed: false }];
        let r = row(&two, 1, 40, g, "x", &b, None, true);
        let text: String = r.iter().map(|c| if c.text.is_empty() { " ".into() } else { c.text.clone() }).collect();
        assert!(text.contains("1 bash") && text.contains("2 vim") && text.contains('■'), "{text:?}");
        assert!(r.iter().all(|c| c.fg.is_some_and(|f| f.r == f.g && f.g == f.b)), "greys only");
    }

    #[test]
    fn window_edges() {
        assert_eq!(edge_at(2.0, 300.0, 800, 600, 6.0), Some(D::West));
        assert_eq!(edge_at(797.0, 300.0, 800, 600, 6.0), Some(D::East));
        assert_eq!(edge_at(400.0, 1.0, 800, 600, 6.0), Some(D::North));
        assert_eq!(edge_at(400.0, 599.0, 800, 600, 6.0), Some(D::South));
        assert_eq!(edge_at(1.0, 1.0, 800, 600, 6.0), Some(D::NorthWest));
        assert_eq!(edge_at(799.0, 599.0, 800, 600, 6.0), Some(D::SouthEast));
        assert_eq!(edge_at(400.0, 300.0, 800, 600, 6.0), None);
        assert_eq!(edge_at(10.0, 300.0, 800, 600, 12.0), Some(D::West), "a wider zone at 200 %");
    }
}
