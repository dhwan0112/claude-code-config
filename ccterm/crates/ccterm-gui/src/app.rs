//! The winit application: one window with tabs. Each tab is a session in the
//! daemon (`ccterm daemon`, started on demand); this process is a client
//! that mirrors their screens (DESIGN.md §3.1, §3.6). The window grid is
//! the active tab's screen plus the cells ccterm draws itself (tab bar from
//! two tabs on, palette, hints; `crate::ui`).

use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use ccterm_core::{
    GridSize, Key, KeyInput, MouseAction, MouseButton, MouseInput, SessionConfig, SessionEvent, Snapshot,
};
use ccterm_proto::paths::Endpoint;
use ccterm_proto::wire::{rgb, CreateSpec};
use ccterm_proto::{Client, RemoteHandle, RemoteSession, Role};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{Key as WKey, KeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{ImePurpose, Window, WindowId};

use crate::chrome::{self, Chrome};
use crate::clipboard::Clipboard;
use crate::config::Decorations;
use crate::damage;
use crate::font::Fonts;
use crate::geom::Layout;
use crate::input::{self, RawKey, Wheel};
use crate::keymap::{Action, Chord, Keymap};
use crate::render::paint::Painter;
use crate::render::soft::SoftRenderer;
use crate::render::{PaddingStyle, Preedit, Renderer, Scene};
use crate::tabsets::{self, SavedTab};
use crate::ui::{self, Compose, Palette, TabView};
use crate::Options;

/// How long a close request waits for its confirming second press, and how
/// long a hint stays.
const CONFIRM: Duration = Duration::from_secs(3);

/// Events from other threads into the event loop.
#[derive(Debug)]
pub enum UserEvent {
    /// An event of the session with this id.
    Session(String, SessionEvent),
}

pub struct App {
    opts: Options,
    proxy: EventLoopProxy<UserEvent>,
    fonts: Option<Fonts>,
    win: Option<Win>,
    pub exit_code: i32,
}

/// What a tab runs: program, arguments, folder; a name to show until the
/// program sets a title (restored tabs).
#[derive(Clone, Debug, Default)]
struct TabSpec {
    program: String,
    args: Vec<String>,
    cwd: Option<String>,
    name: Option<String>,
}

struct Tab {
    session: RemoteSession,
    snap: Option<Snapshot>,
    spec: TabSpec,
    program: String,
    /// The program ended (exit code); the tab stays until closed.
    exited: Option<Option<u32>>,
    /// Size last sent to the daemon.
    size: GridSize,
}

impl Tab {
    fn name(&self) -> String {
        let title = self.snap.as_ref().map_or("", |s| s.title.as_str());
        let name = match (&self.spec.name, title.trim().is_empty()) {
            (Some(n), true) => n.clone(),
            _ => ui::tab_name(title, &self.program),
        };
        match self.exited {
            Some(code) => format!("{name} [exited{}]", code.map(|c| format!(" {c}")).unwrap_or_default()),
            None => name,
        }
    }
}

/// Everything that exists once the window is up.
struct Win {
    window: Rc<Window>,
    renderer: Box<dyn Renderer>,
    layout: Layout,
    /// Window inner size, physical pixels.
    size: PhysicalSize<u32>,
    /// The whole window grid (tab bar included).
    grid: GridSize,
    client: Client,
    proxy: EventLoopProxy<UserEvent>,
    tabs: Vec<Tab>,
    active: usize,
    /// What is drawn: the active tab's screen with ccterm's own cells.
    composed: Option<Snapshot>,
    /// Rows changed since the last frame was drawn.
    dirty: Vec<bool>,
    full: bool,
    focused: bool,
    preedit: Option<Preedit>,
    mods: ModifiersState,
    mouse: PhysicalPosition<f64>,
    /// Mouse buttons currently held, in press order.
    held: Vec<MouseButton>,
    wheel: Wheel,
    /// Last IME cursor area sent to the window (cell column, row).
    ime_at: Option<(u16, u16)>,
    clipboard: Clipboard,
    chrome: Chrome,
    keys: Keymap,
    palette: Option<Palette>,
    hint: Option<(String, Instant)>,
    /// A close request waiting for its second press: (session id, time).
    pending_close: Option<(String, Instant)>,
    /// Keys whose press was a shortcut: their release is not sent either.
    swallowed: HashSet<KeyCode>,
    /// What new tabs run.
    command: (String, Vec<String>),
    start_cwd: Option<String>,
    defaults: (ccterm_core::Rgb, ccterm_core::Rgb),
    scrollback_limit: usize,
    /// Set when the last tab is gone: the window closes.
    quit: Option<i32>,
    /// The window is closing: closing its tabs must not shrink `last`.
    shutting_down: bool,
}

impl App {
    pub fn new(opts: Options, proxy: EventLoopProxy<UserEvent>, fonts: Fonts) -> Self {
        App { opts, proxy, fonts: Some(fonts), win: None, exit_code: 0 }
    }

    fn font_px(&self, scale: f64) -> f32 {
        (self.opts.font_size * 96.0 / 72.0 * scale) as f32
    }

    fn padding(&self, scale: f64) -> PaddingStyle {
        padding_style(&self.opts.config, scale)
    }

    fn create(&mut self, el: &ActiveEventLoop) -> Result<(), String> {
        // Cell metrics are needed for the initial window size, before the
        // window (and its scale factor) exists: use the primary monitor's
        // scale. If the window lands elsewhere, ScaleFactorChanged fixes it.
        let scale0 = el.primary_monitor().map_or(1.0, |m| m.scale_factor());
        let fonts = self.fonts.take().ok_or("fonts already used")?;
        let mut painter = Painter::new(fonts, self.font_px(scale0));
        let pad0 = self.padding(scale0);
        let (w, h) = Layout { cell: painter.cell_metrics(), padding: pad0.min, balance: pad0.balance }
            .window_size(self.opts.cols, self.opts.rows);
        let cfg = SessionConfig::new(self.opts.command.clone());
        let icon = winit::window::Icon::from_rgba(crate::icon::rgba(64), 64, 64).ok();
        let attrs = Window::default_attributes()
            .with_title(ccterm_proto::product::DISPLAY_NAME)
            .with_inner_size(PhysicalSize::new(w, h))
            .with_window_icon(icon.clone());
        // Windows: the big icon (Alt+Tab, taskbar) as well as the small one.
        #[cfg(windows)]
        let attrs = winit::platform::windows::WindowAttributesExtWindows::with_taskbar_icon(attrs, icon.clone());
        // Wayland app_id and X11 WM_CLASS = the desktop file's name.
        #[cfg(all(unix, not(target_os = "macos")))]
        let attrs = {
            let id = ccterm_proto::product::APP_ID;
            let a = winit::platform::wayland::WindowAttributesExtWayland::with_name(attrs, id, "");
            winit::platform::x11::WindowAttributesExtX11::with_name(a, id, id)
        };
        let attrs = chrome::attributes(attrs, cfg.default_bg, cfg.default_fg)
            .with_decorations(decorated(self.opts.config.decorations));
        let chrome = Chrome::new(cfg.default_bg, cfg.default_fg);
        let window = Rc::new(el.create_window(attrs).map_err(|e| format!("window: {e}"))?);
        window.set_ime_allowed(true);
        window.set_ime_purpose(ImePurpose::Terminal);
        let scale = window.scale_factor();
        painter.set_px(self.font_px(scale));
        let mut renderer: Box<dyn Renderer> =
            Box::new(SoftRenderer::new(window.clone(), painter).map_err(|e| format!("renderer: {e}"))?);
        let pad = self.padding(scale);
        renderer.set_padding(pad);
        let layout = Layout { cell: renderer.cell_metrics(), padding: pad.min, balance: pad.balance };
        let size = window.inner_size();
        let grid =
            if size.width > 0 && size.height > 0 { layout.grid(size.width, size.height) } else { layout.grid(w, h) };
        renderer.resize(size.width, size.height);

        let ep = Endpoint::default_for_user().map_err(|e| format!("daemon socket: {e}"))?;
        let client = Client::connect_or_spawn(&ep, Role::Control, "gui").map_err(|e| e.to_string())?;
        let cmd = &self.opts.command;
        let command = (
            cmd.program.to_string_lossy().into_owned(),
            cmd.args.iter().map(|a| a.to_string_lossy().into_owned()).collect(),
        );
        let clipboard = Clipboard::new(&window);
        let mut win = Win {
            window,
            renderer,
            layout,
            size,
            grid,
            client,
            proxy: self.proxy.clone(),
            tabs: vec![],
            active: 0,
            composed: None,
            dirty: vec![],
            full: true,
            focused: true,
            preedit: None,
            mods: ModifiersState::empty(),
            mouse: PhysicalPosition::new(0.0, 0.0),
            held: vec![],
            wheel: Wheel::default(),
            ime_at: None,
            clipboard,
            chrome,
            keys: self.opts.keymap.clone(),
            palette: None,
            hint: None,
            pending_close: None,
            swallowed: HashSet::new(),
            command,
            start_cwd: std::env::current_dir().ok().map(|d| d.to_string_lossy().into_owned()),
            defaults: (cfg.default_fg, cfg.default_bg),
            scrollback_limit: self.opts.config.scrollback_limit,
            quit: None,
            shutting_down: false,
        };
        // "last" becomes "previous" before the first tab rewrites "last".
        let previous = if self.opts.config.restore_tabs { tabsets::rotate_last(&tabsets::dir()) } else { vec![] };
        win.open_tab(None)?;
        if !previous.is_empty() {
            let n = win.open_saved(&previous);
            if n > 0 {
                win.select(0);
                win.show_hint(format!("reopened {n} tab(s) of the previous run"));
            }
        }
        win.save_last();
        self.win = Some(win);
        Ok(())
    }
}

impl Win {
    /// Rows of ccterm's own chrome above the terminal (the tab bar, shown
    /// from two tabs on).
    fn top(&self) -> u16 {
        u16::from(self.tabs.len() >= 2)
    }

    /// The grid a tab's program gets.
    fn term_grid(&self) -> GridSize {
        let mut g = self.grid;
        g.rows = g.rows.saturating_sub(self.top()).max(1);
        g
    }

    fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    fn handle(&self) -> &RemoteHandle {
        &self.tab().session.handle
    }

    /// Opens a tab running the window's command in the active tab's
    /// directory (else where ccterm started) and selects it.
    fn open_tab(&mut self, cwd: Option<String>) -> Result<(), String> {
        let spec = TabSpec { program: self.command.0.clone(), args: self.command.1.clone(), cwd, name: None };
        self.open_spec(spec)
    }

    /// Opens the tabs of a set after the current ones; tabs whose folder is
    /// gone are skipped. Returns how many opened.
    fn open_saved(&mut self, set: &[SavedTab]) -> usize {
        let mut n = 0;
        for t in set {
            if !t.cwd.is_empty() && !std::path::Path::new(&t.cwd).is_dir() {
                log::warn!("tab set: folder {:?} is gone; tab {:?} skipped", t.cwd, t.name);
                continue;
            }
            let spec = TabSpec {
                program: t.program.clone(),
                args: t.args.clone(),
                cwd: (!t.cwd.is_empty()).then(|| t.cwd.clone()),
                name: (!t.name.is_empty()).then(|| t.name.clone()),
            };
            match self.open_spec(spec) {
                Ok(()) => n += 1,
                Err(e) => log::warn!("tab set: {:?}: {e}", t.name),
            }
        }
        n
    }

    /// The open tabs as a tab set.
    fn saved_tabs(&self) -> Vec<SavedTab> {
        self.tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| t.exited.is_none())
            .map(|(i, t)| {
                let pwd = t.snap.as_ref().map(|s| osc7_path(&s.pwd)).filter(|p| !p.is_empty());
                SavedTab {
                    name: t.name(),
                    cwd: pwd.or_else(|| t.spec.cwd.clone()).unwrap_or_default(),
                    program: t.spec.program.clone(),
                    args: t.spec.args.clone(),
                    claude_session: String::new(),
                    active: i == self.active,
                }
            })
            .collect()
    }

    /// Keeps the tab set "last" equal to the open tabs.
    fn save_last(&self) {
        if self.shutting_down {
            return;
        }
        if let Err(e) = tabsets::save(&tabsets::dir(), "last", &self.saved_tabs()) {
            log::warn!("tab set last: {e}");
        }
    }

    fn open_spec(&mut self, spec: TabSpec) -> Result<(), String> {
        let cwd = spec.cwd.clone().or_else(|| {
            self.tabs.get(self.active).and_then(|t| t.snap.as_ref()).map(|s| s.pwd.clone()).filter(|p| !p.is_empty())
        });
        let cwd = cwd.or_else(|| self.start_cwd.clone()).map(|p| osc7_path(&p));
        // The new tab may bring the tab bar: size it for the grid it will have.
        let mut size = self.grid;
        size.rows = size.rows.saturating_sub(u16::from(!self.tabs.is_empty())).max(1);
        let create = CreateSpec {
            program: spec.program.clone(),
            args: spec.args.clone(),
            cwd: cwd.clone(),
            // The daemon may run in an older environment: the child gets ours
            // (never stored, DESIGN.md §3.6).
            env: std::env::vars_os()
                .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
                .collect(),
            size: size.into(),
            scrollback_limit: self.scrollback_limit as u64,
            default_fg: rgb(self.defaults.0),
            default_bg: rgb(self.defaults.1),
        };
        let id = self.client.create(create).map_err(|e| e.to_string())?;
        let session = self.client.attach(&id, None).map_err(|e| format!("attach: {e}"))?;
        log::info!(
            "tab {} = session {id} in daemon {}, grid {}x{}",
            self.tabs.len() + 1,
            self.client.daemon_pid,
            size.cols,
            size.rows
        );
        let events = session.events.clone();
        let proxy = self.proxy.clone();
        let sid = id.clone();
        std::thread::Builder::new()
            .name("ccterm-gui-events".into())
            .spawn(move || {
                for ev in events.iter() {
                    if proxy.send_event(UserEvent::Session(sid.clone(), ev)).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        if let Some(t) = self.tabs.get(self.active) {
            t.session.handle.focus(false);
        }
        let program = spec.program.clone();
        let spec = TabSpec { cwd, ..spec };
        self.tabs.push(Tab { session, snap: None, spec, program, exited: None, size });
        self.active = self.tabs.len() - 1;
        if self.focused {
            self.handle().focus(true);
        }
        self.relayout();
        self.save_last();
        Ok(())
    }

    /// Sends every tab the terminal grid (it changes with the window size
    /// and when the tab bar appears or goes) and redraws everything.
    fn relayout(&mut self) {
        let g = self.term_grid();
        for t in &mut self.tabs {
            if t.size != g && t.exited.is_none() {
                t.session.handle.resize(g);
                t.size = g;
            }
        }
        self.full = true;
        self.ime_at = None;
        self.recompose();
    }

    fn select(&mut self, i: usize) {
        if i >= self.tabs.len() || i == self.active {
            return;
        }
        self.tab().session.handle.focus(false);
        self.active = i;
        if self.focused {
            self.handle().focus(true);
        }
        self.preedit = None;
        self.full = true;
        self.ime_at = None;
        self.update_title();
        self.recompose();
        self.save_last();
    }

    fn close_tab(&mut self, i: usize) {
        let t = self.tabs.remove(i);
        if let Err(e) = self.client.close(&t.session.id) {
            log::warn!("close session {}: {e}", t.session.id);
        }
        let _ = self.client.detach(&t.session.id);
        if self.tabs.is_empty() {
            self.quit = Some(t.exited.flatten().map_or(0, |c| c as i32));
            self.save_last();
            return;
        }
        if self.active >= self.tabs.len() || self.active > i {
            self.active = self.active.saturating_sub(1).min(self.tabs.len() - 1);
        }
        if self.focused {
            self.handle().focus(true);
        }
        self.update_title();
        self.relayout();
        self.save_last();
    }

    /// Close needs the same shortcut twice within [`CONFIRM`] while the
    /// program runs (no dialog, §2.2); an exited tab closes at once.
    fn request_close(&mut self) {
        let id = self.tab().session.id.clone();
        let running = self.tab().exited.is_none();
        let confirmed = self.pending_close.as_ref().is_some_and(|(p, t)| *p == id && t.elapsed() < CONFIRM);
        if !running || confirmed {
            self.pending_close = None;
            self.hint = None;
            self.close_tab(self.active);
        } else {
            self.pending_close = Some((id, Instant::now()));
            let key = self.keys.chord_for(Action::CloseTab).map(|c| c.to_string()).unwrap_or_default();
            self.show_hint(format!("{key} again closes this tab"));
        }
    }

    fn show_hint(&mut self, text: String) {
        self.hint = Some((text, Instant::now()));
        self.recompose();
    }

    fn run(&mut self, a: Action) {
        log::debug!("action {}", a.name());
        match a {
            Action::NewTab => {
                if let Err(e) = self.open_tab(None) {
                    log::error!("new tab: {e}");
                    self.show_hint(format!("new tab failed: {e}"));
                }
            }
            Action::CloseTab => self.request_close(),
            Action::NextTab => self.select((self.active + 1) % self.tabs.len()),
            Action::PreviousTab => self.select((self.active + self.tabs.len() - 1) % self.tabs.len()),
            Action::GotoTab(n) => self.select(n as usize - 1),
            Action::CommandPalette => {
                self.palette = Some(Palette::default());
                self.recompose();
            }
            Action::Paste => {
                if let Some(text) = self.clipboard.get() {
                    if self.tab().exited.is_none() {
                        self.handle().paste(text);
                    }
                }
            }
            Action::SaveTabs => {
                let tabs = self.saved_tabs();
                match tabsets::save(&tabsets::dir(), "saved", &tabs) {
                    Ok(()) => self.show_hint(format!("{} tab(s) saved as \"saved\"", tabs.len())),
                    Err(e) => self.show_hint(format!("saving tabs failed: {e}")),
                }
            }
            Action::OpenSavedTabs | Action::OpenPreviousTabs => {
                let name = if a == Action::OpenSavedTabs { "saved" } else { "previous" };
                let set = tabsets::load(&tabsets::dir(), name).unwrap_or_default();
                let n = self.open_saved(&set);
                self.show_hint(format!("{n} tab(s) of \"{name}\" opened"));
            }
        }
    }

    fn update_title(&self) {
        let t = self.tab().snap.as_ref().map_or("", |s| s.title.as_str());
        self.window.set_title(if t.is_empty() { ccterm_proto::product::DISPLAY_NAME } else { t });
    }

    /// Sizes of the GUI's own heap structures, for the log (`CCTERM_LOG=info`).
    fn log_memory(&self) {
        let empty = vec![];
        let all = self.composed.as_ref().map_or(&empty, |s| &s.cells);
        let cells = all.len();
        let text: usize = all.iter().map(|c| c.text.capacity()).sum();
        let snap_kb = (cells * std::mem::size_of::<ccterm_core::Cell>() + text) / 1024;
        log::info!(
            "memory: {}; {} tab(s); window grid {cells} cells {snap_kb} KB",
            self.renderer.memory(),
            self.tabs.len()
        );
    }

    /// Grid origin in the window (same computation as the renderer's).
    fn origin(&self) -> (u32, u32) {
        self.layout.origin(self.size.width, self.size.height, self.grid.cols, self.grid.rows)
    }

    /// A tab's screen changed: take its snapshot (so its next frame may
    /// come) and redraw if it is the visible one.
    fn refresh(&mut self, sid: &str) {
        let Some(i) = self.tabs.iter().position(|t| t.session.id == sid) else { return };
        let Some(next) = self.tabs[i].session.handle.snapshot() else { return };
        let title_changed = self.tabs[i].snap.as_ref().is_none_or(|s| s.title != next.title);
        self.tabs[i].snap = Some(next);
        if i == self.active {
            if title_changed {
                self.update_title();
            }
            self.recompose();
        } else if title_changed && self.tabs.len() >= 2 {
            self.recompose(); // the tab bar shows its name
        }
        if title_changed {
            self.save_last();
        }
    }

    /// Rebuilds the window grid from the active tab and ccterm's cells.
    fn recompose(&mut self) {
        let (fg, bg) = self.defaults;
        let empty = Snapshot { default_fg: fg, default_bg: bg, ..Default::default() };
        let screen = self.tab().snap.as_ref().unwrap_or(&empty);
        let g = ui::Greys::from(screen.default_fg, screen.default_bg);
        let names: Vec<String> = self.tabs.iter().map(Tab::name).collect();
        let bar = (self.tabs.len() >= 2).then(|| {
            let views: Vec<TabView> =
                names.iter().zip(&self.tabs).map(|(n, t)| TabView { name: n, exited: t.exited.is_some() }).collect();
            ui::tab_bar(&views, self.active, self.grid.cols as usize, g)
        });
        let next = Compose {
            screen,
            cols: self.grid.cols,
            rows: self.grid.rows,
            tab_bar: bar,
            palette: self.palette.as_ref().map(|p| (p, &self.keys)),
            hint: self.hint.as_ref().map(|(h, _)| h.as_str()),
        }
        .build();
        if self.composed.as_ref().is_some_and(|s| (s.cols, s.rows) != (next.cols, next.rows)) {
            self.full = true;
        }
        let d = damage::dirty_rows(self.composed.as_ref(), &next);
        damage::merge(&mut self.dirty, &d);
        self.chrome.update(&self.window, next.default_bg, next.default_fg);
        self.composed = Some(next);
        self.update_ime_area();
        self.window.request_redraw();
    }

    /// Keep the IME candidate window next to the terminal cursor.
    fn update_ime_area(&mut self) {
        let Some(c) = self.composed.as_ref().and_then(|s| s.cursor) else { return };
        if self.ime_at == Some((c.x, c.y)) {
            return;
        }
        self.ime_at = Some((c.x, c.y));
        let (x, y) = self.layout.cell_origin(self.origin(), c.x, c.y);
        let cell = self.layout.cell;
        self.window.set_ime_cursor_area(PhysicalPosition::new(x, y), PhysicalSize::new(cell.width, cell.height));
    }

    fn mouse_tracking(&self) -> bool {
        self.tab().snap.as_ref().is_some_and(|s| s.modes.mouse_tracking) && self.palette.is_none()
    }

    /// Mouse position in the active tab's pixels; `None` over the tab bar.
    fn tab_px(&self) -> Option<(f32, f32)> {
        let (x, y) = self.layout.grid_px(self.origin(), self.mouse.x, self.mouse.y);
        let y = y - (self.top() as u32 * self.layout.cell.height) as f32;
        (y >= 0.0).then_some((x, y))
    }

    fn mouse_event(&self, action: MouseAction, button: Option<MouseButton>) {
        let Some((x, y)) = self.tab_px() else { return };
        let mut m = MouseInput::at_pixel(action, button, x, y);
        m.mods = input::mods(self.mods);
        m.any_pressed = !self.held.is_empty();
        self.handle().mouse(m);
    }

    /// A click on the tab bar selects the tab under the pointer.
    fn tab_bar_click(&mut self) -> bool {
        if self.top() == 0 {
            return false;
        }
        let (col, row) = self.layout.cell_at(self.origin(), self.mouse.x, self.mouse.y, self.grid);
        if row != 0 {
            return false;
        }
        let share = (self.grid.cols as usize / self.tabs.len()).clamp(6, 32);
        let i = col as usize / share;
        if i < self.tabs.len() {
            self.select(i);
        }
        true
    }

    fn keyboard(&mut self, ev: &KeyEvent) {
        // While a composition is shown, text keys belong to the IME; the
        // result arrives as Ime::Commit. Ctrl/Super shortcuts still go out.
        if input::ime_owns_key(self.preedit.is_some(), ev.state == ElementState::Pressed, ev.text.is_some(), self.mods)
        {
            log::debug!("key held back for the IME composition");
            return;
        }
        let code = match ev.physical_key {
            PhysicalKey::Code(c) => Some(c),
            PhysicalKey::Unidentified(_) => None,
        };
        if ev.state == ElementState::Released {
            if code.is_some_and(|c| self.swallowed.remove(&c)) {
                return;
            }
        } else if let Some(chord) = chord_of(ev, self.mods) {
            if self.palette.is_some() {
                self.palette_key(ev, &chord);
                if let Some(c) = code {
                    self.swallowed.insert(c);
                }
                return;
            }
            if let Some(a) = self.keys.action(&chord) {
                if let Some(c) = code {
                    self.swallowed.insert(c);
                }
                self.run(a);
                return;
            }
        }
        if self.palette.is_some() || self.tab().exited.is_some() {
            return;
        }
        let unshifted = match ev.key_without_modifiers() {
            WKey::Character(s) => s.chars().next(),
            WKey::Named(NamedKey::Space) => Some(' '),
            _ => None,
        };
        let raw =
            RawKey { code, state: ev.state, repeat: ev.repeat, text: ev.text.as_deref(), unshifted, mods: self.mods };
        let k = input::key_input(&raw);
        log_key(&k);
        self.handle().key(k);
    }

    /// Keys while the palette is open: type to filter, arrows, Enter, Esc.
    fn palette_key(&mut self, ev: &KeyEvent, chord: &Chord) {
        let Some(p) = self.palette.as_mut() else { return };
        let plain = !chord.ctrl && !chord.alt && !chord.sup;
        match chord.key.as_str() {
            "escape" => self.palette = None,
            "enter" => {
                let a = p.current();
                self.palette = None;
                if let Some(a) = a {
                    self.recompose();
                    self.run(a);
                    return;
                }
            }
            "up" => p.move_by(-1),
            "down" | "tab" => p.move_by(1),
            "backspace" => {
                p.query.pop();
                p.selected = 0;
            }
            _ if plain => {
                if let Some(t) = ev.text.as_deref().filter(|t| !t.chars().any(char::is_control)) {
                    p.query.push_str(t);
                    p.selected = 0;
                }
            }
            _ => {
                if self.keys.action(chord) == Some(Action::CommandPalette) {
                    self.palette = None;
                }
            }
        }
        self.recompose();
    }

    fn ime(&mut self, ime: Ime) {
        log_ime(&ime);
        match ime {
            Ime::Enabled => {}
            Ime::Preedit(text, cursor) => {
                self.preedit =
                    if text.is_empty() { None } else { Some(Preedit { text, cursor: preedit_cursor(cursor) }) };
            }
            Ime::Commit(text) => {
                self.preedit = None;
                if let Some(p) = self.palette.as_mut() {
                    p.query.push_str(&text);
                    p.selected = 0;
                    self.recompose();
                } else if !text.is_empty() && self.tab().exited.is_none() {
                    self.handle().key(KeyInput::press(Key::Unidentified).with_text(text));
                }
            }
            Ime::Disabled => self.preedit = None,
        }
        self.mark_cursor_row();
        self.window.request_redraw();
    }

    fn mark_cursor_row(&mut self) {
        if let Some(c) = self.composed.as_ref().and_then(|s| s.cursor) {
            if let Some(d) = self.dirty.get_mut(c.y as usize) {
                *d = true;
            }
        }
    }

    fn draw(&mut self) {
        let Some(snap) = self.composed.as_ref() else { return };
        if self.dirty.len() != snap.rows as usize {
            self.dirty = vec![true; snap.rows as usize];
        }
        let scene =
            Scene { snap, dirty: &self.dirty, full: self.full, focused: self.focused, preedit: self.preedit.as_ref() };
        let t0 = Instant::now();
        if let Err(e) = self.renderer.render(&scene) {
            log::error!("render: {e}");
        }
        log::trace!("frame drawn in {} us (full {})", t0.elapsed().as_micros(), self.full);
        self.full = false;
        self.dirty.iter_mut().for_each(|d| *d = false);
    }

    fn resized(&mut self, size: PhysicalSize<u32>) {
        self.renderer.resize(size.width, size.height);
        self.size = size;
        self.full = true;
        // The IME area depends on the (balanced) origin.
        self.ime_at = None;
        if size.width == 0 || size.height == 0 {
            return; // minimised
        }
        let grid = self.layout.grid(size.width, size.height);
        if grid != self.grid {
            log::debug!("resize {}x{} px -> grid {}x{}", size.width, size.height, grid.cols, grid.rows);
            self.grid = grid;
            self.relayout();
        }
        self.window.request_redraw();
    }

    /// Expires hints and close requests; returns the next deadline.
    fn tick(&mut self) -> Option<Instant> {
        let now = Instant::now();
        if self.pending_close.as_ref().is_some_and(|(_, t)| now.duration_since(*t) >= CONFIRM) {
            self.pending_close = None;
        }
        if self.hint.as_ref().is_some_and(|(_, t)| now.duration_since(*t) >= CONFIRM) {
            self.hint = None;
            self.recompose();
        }
        self.hint.as_ref().map(|(_, t)| *t + CONFIRM)
    }
}

/// A directory from OSC 7 (`file://host/path`) or a plain path.
fn osc7_path(p: &str) -> String {
    match p.strip_prefix("file://") {
        Some(r) => r.find('/').map_or(r.to_string(), |i| r[i..].to_string()),
        None => p.to_string(),
    }
}

/// The chord of a key press: modifiers plus the key without modifiers.
fn chord_of(ev: &KeyEvent, m: ModifiersState) -> Option<Chord> {
    let key = match ev.key_without_modifiers() {
        WKey::Character(s) => s.to_lowercase(),
        WKey::Named(n) => match n {
            NamedKey::Tab => "tab".into(),
            NamedKey::Enter => "enter".into(),
            NamedKey::Escape => "escape".into(),
            NamedKey::Backspace => "backspace".into(),
            NamedKey::Insert => "insert".into(),
            NamedKey::Delete => "delete".into(),
            NamedKey::PageUp => "pageup".into(),
            NamedKey::PageDown => "pagedown".into(),
            NamedKey::ArrowUp => "up".into(),
            NamedKey::ArrowDown => "down".into(),
            NamedKey::ArrowLeft => "left".into(),
            NamedKey::ArrowRight => "right".into(),
            NamedKey::Home => "home".into(),
            NamedKey::End => "end".into(),
            NamedKey::Space => "space".into(),
            other => format!("{other:?}").to_lowercase(),
        },
        _ => return None,
    };
    Some(Chord { ctrl: m.control_key(), shift: m.shift_key(), alt: m.alt_key(), sup: m.super_key(), key })
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.win.is_some() {
            return;
        }
        if let Err(e) = self.create(el) {
            crate::fatal(&e);
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let Some(win) = self.win.as_mut() else { return };
        if let Some(code) = win.quit.take() {
            self.exit_code = code;
            el.exit();
            return;
        }
        el.set_control_flow(match win.tick() {
            Some(t) => ControlFlow::WaitUntil(t),
            None => ControlFlow::Wait,
        });
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, ev: UserEvent) {
        let Some(win) = self.win.as_mut() else { return };
        let UserEvent::Session(sid, ev) = ev;
        let Some(i) = win.tabs.iter().position(|t| t.session.id == sid) else { return };
        match ev {
            SessionEvent::Wakeup => win.refresh(&sid),
            SessionEvent::Title(_) => {}
            SessionEvent::Exited(status) => {
                log::info!("tab {} ({sid}) exited: {status:?}", i + 1);
                if win.tabs.len() == 1 {
                    // One tab: the window ends with its program, as before tabs.
                    win.log_memory();
                    win.tabs[i].exited = Some(status.code);
                    win.close_tab(i);
                } else {
                    // An event never removes a tab (DESIGN.md §3.10): it stays,
                    // marked, until the user closes it.
                    win.tabs[i].exited = Some(status.code);
                    win.recompose();
                }
            }
            SessionEvent::Error(e) => log::error!("session {sid}: {e}"),
            // No sound or pop-up (DESIGN.md §2.2): only an attention hint
            // (taskbar / urgency), and only when the window is unfocused.
            SessionEvent::Bell if !win.focused => {
                win.window.request_user_attention(Some(winit::window::UserAttentionType::Informational))
            }
            SessionEvent::Bell => {}
            // OSC 52 from the application, e.g. Claude Code copying a
            // mouse selection in its fullscreen renderer.
            SessionEvent::ClipboardWrite(text) => {
                log::debug!("clipboard write from application: {} bytes", text.len());
                win.clipboard.set(text);
            }
            SessionEvent::Pwd(_)
            | SessionEvent::Notification { .. }
            | SessionEvent::Progress(_)
            | SessionEvent::Reply(_) => {}
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(win) = self.win.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => {
                win.log_memory();
                // Closing the window ends its sessions; the tab set "last"
                // keeps them for the next start. A crashed or killed GUI
                // leaves them running.
                win.save_last();
                win.shutting_down = true;
                while !win.tabs.is_empty() {
                    win.close_tab(win.tabs.len() - 1);
                }
                el.exit()
            }
            WindowEvent::RedrawRequested => win.draw(),
            WindowEvent::Resized(size) => win.resized(size),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let px = (self.opts.font_size * 96.0 / 72.0 * scale_factor) as f32;
                win.renderer.set_font_px(px);
                let pad = padding_style(&self.opts.config, scale_factor);
                win.renderer.set_padding(pad);
                win.layout = Layout { cell: win.renderer.cell_metrics(), padding: pad.min, balance: pad.balance };
                let size = win.window.inner_size();
                win.grid = GridSize::new(0, 0);
                win.resized(size);
            }
            WindowEvent::ModifiersChanged(m) => win.mods = m.state(),
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => win.keyboard(&event),
            WindowEvent::Ime(ime) => win.ime(ime),
            WindowEvent::CursorMoved { position, .. } => {
                win.mouse = position;
                if win.mouse_tracking() {
                    win.mouse_event(MouseAction::Motion, win.held.first().copied());
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(b) = input::mouse_button(button) else { return };
                let action = match state {
                    ElementState::Pressed => {
                        if b == MouseButton::Left && win.tab_bar_click() {
                            return;
                        }
                        win.held.retain(|x| *x != b);
                        win.held.push(b);
                        MouseAction::Press
                    }
                    ElementState::Released => {
                        win.held.retain(|x| *x != b);
                        MouseAction::Release
                    }
                };
                if win.mouse_tracking() {
                    win.mouse_event(action, Some(b));
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let n = win.wheel.notches(delta, win.layout.cell.height);
                if n != 0 {
                    let (col, row) = win.layout.cell_at(win.origin(), win.mouse.x, win.mouse.y, win.grid);
                    let Some(row) = row.checked_sub(win.top()) else { return };
                    let lines = input::wheel_lines(n, win.mouse_tracking());
                    log::debug!("wheel {delta:?} -> {n} notches, {lines} lines at {col},{row}");
                    win.handle().wheel(lines, col, row, input::mods(win.mods));
                }
            }
            WindowEvent::Focused(f) => {
                log::debug!("focused {f}");
                win.focused = f;
                win.handle().focus(f);
                win.mark_cursor_row();
                win.window.request_redraw();
            }
            _ => {}
        }
    }
}

/// Debug log of a key event without its content: keys that produce text are
/// logged as `<text>` plus a character count, so a debug log never holds what
/// was typed (passwords, tokens).
fn log_key(k: &KeyInput) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }
    let shortcut = k.mods.intersects(ccterm_core::Mods::CTRL | ccterm_core::Mods::ALT | ccterm_core::Mods::SUPER);
    match &k.text {
        Some(t) if !shortcut => log::debug!("key <text> {:?} chars={}", k.action, t.chars().count()),
        _ if !shortcut && k.unshifted.is_some() => log::debug!("key <text> {:?}", k.action),
        _ => log::debug!("key {:?} {:?} mods={:?}", k.key, k.action, k.mods),
    }
}

/// Debug log of an IME event without the composed text.
fn log_ime(ime: &Ime) {
    match ime {
        Ime::Preedit(t, c) => log::debug!("ime preedit chars={} cursor={c:?}", t.chars().count()),
        Ime::Commit(t) => log::debug!("ime commit chars={}", t.chars().count()),
        other => log::debug!("ime {other:?}"),
    }
}

/// Whether the platform draws a title bar and frame.
fn decorated(d: Decorations) -> bool {
    match d {
        Decorations::Native => true,
        Decorations::None => false,
        Decorations::Integrated => {
            // The tabs milestone draws its own title bar here.
            log::warn!("decorations = integrated is not implemented yet; using native");
            true
        }
    }
}

/// Padding settings at a scale factor; `CCTERM_DEBUG_PADDING=1` paints it red.
fn padding_style(c: &crate::config::Config, scale: f64) -> PaddingStyle {
    PaddingStyle {
        min: (c.window_padding * scale).round() as u32,
        balance: c.padding_balance,
        color: c.padding_color,
        debug: std::env::var_os("CCTERM_DEBUG_PADDING").is_some_and(|v| !v.is_empty() && v != "0"),
    }
}

/// The IME caret inside the preedit text. Workaround for winit#4746 (not
/// fixed in winit 0.30.13): on Windows the Microsoft Korean IME never reports
/// GCS_CURSORPOS and winit then claims the caret is at 0. Like egui-winit
/// (emilk/egui#8083), treat an empty range at 0 as "unknown" and draw no
/// caret; the composition itself stays visible and underlined.
fn preedit_cursor(cursor: Option<(usize, usize)>) -> Option<(usize, usize)> {
    match cursor {
        Some((0, 0)) if cfg!(windows) => None,
        c => c,
    }
}

#[cfg(test)]
mod tests {
    use super::preedit_cursor;

    #[test]
    fn korean_ime_cursor_workaround() {
        assert_eq!(preedit_cursor(Some((3, 3))), Some((3, 3)));
        assert_eq!(preedit_cursor(None), None);
        let zero = preedit_cursor(Some((0, 0)));
        if cfg!(windows) {
            assert_eq!(zero, None);
        } else {
            assert_eq!(zero, Some((0, 0)));
        }
    }
}
