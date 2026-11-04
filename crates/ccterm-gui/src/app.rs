//! The winit application: one window, one session.

use std::rc::Rc;

use ccterm_core::{
    GridSize, Key, KeyInput, MouseAction, MouseButton, MouseInput, Session, SessionConfig, SessionEvent, SessionHandle,
    Snapshot,
};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey, PhysicalKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{ImePurpose, Window, WindowId};

use crate::clipboard::Clipboard;
use crate::damage;
use crate::font::Fonts;
use crate::geom::Layout;
use crate::input::{self, RawKey, Wheel};
use crate::render::paint::Painter;
use crate::render::soft::SoftRenderer;
use crate::render::{PaddingStyle, Preedit, Renderer, Scene};
use crate::Options;

/// Events from other threads into the event loop.
#[derive(Debug)]
pub enum UserEvent {
    Session(SessionEvent),
}

pub struct App {
    opts: Options,
    proxy: EventLoopProxy<UserEvent>,
    fonts: Option<Fonts>,
    win: Option<Win>,
    pub exit_code: i32,
}

/// Everything that exists once the window is up.
struct Win {
    window: Rc<Window>,
    renderer: Box<dyn Renderer>,
    layout: Layout,
    /// Window inner size, physical pixels.
    size: PhysicalSize<u32>,
    grid: GridSize,
    session: Session,
    snap: Option<Snapshot>,
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
        let attrs = Window::default_attributes().with_title("ccterm").with_inner_size(PhysicalSize::new(w, h));
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

        let mut cfg = SessionConfig::new(self.opts.command.clone());
        cfg.size = grid;
        let session = Session::spawn(cfg).map_err(|e| format!("cannot start {:?}: {e}", self.opts.command.program))?;
        log::info!(
            "session: {} pid {:?}, grid {}x{}",
            session.handle.backend(),
            session.handle.pid(),
            grid.cols,
            grid.rows
        );

        // Forward session events into the event loop; this thread ends when
        // the session's channel closes or the loop is gone.
        let events = session.events.clone();
        let proxy = self.proxy.clone();
        std::thread::Builder::new()
            .name("ccterm-gui-events".into())
            .spawn(move || {
                for ev in events.iter() {
                    if proxy.send_event(UserEvent::Session(ev)).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;

        let clipboard = Clipboard::new(&window);
        self.win = Some(Win {
            window,
            renderer,
            layout,
            size,
            grid,
            session,
            snap: None,
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
        });
        Ok(())
    }
}

impl Win {
    fn handle(&self) -> &SessionHandle {
        &self.session.handle
    }

    /// Grid origin in the window (same computation as the renderer's).
    fn origin(&self) -> (u32, u32) {
        self.layout.origin(self.size.width, self.size.height, self.grid.cols, self.grid.rows)
    }

    /// Take a snapshot now (so the engine may send the next wakeup) and
    /// schedule a redraw.
    fn refresh(&mut self) {
        let Some(next) = self.handle().snapshot() else { return };
        // A grid of a different size leaves stale pixels outside the new
        // rows (e.g. the old last row in what is now padding): repaint all.
        if self.snap.as_ref().is_some_and(|s| (s.cols, s.rows) != (next.cols, next.rows)) {
            self.full = true;
        }
        let d = damage::dirty_rows(self.snap.as_ref(), &next);
        damage::merge(&mut self.dirty, &d);
        self.snap = Some(next);
        self.update_ime_area();
        self.window.request_redraw();
    }

    /// Keep the IME candidate window next to the terminal cursor.
    fn update_ime_area(&mut self) {
        let Some(c) = self.snap.as_ref().and_then(|s| s.cursor) else { return };
        if self.ime_at == Some((c.x, c.y)) {
            return;
        }
        self.ime_at = Some((c.x, c.y));
        let (x, y) = self.layout.cell_origin(self.origin(), c.x, c.y);
        let cell = self.layout.cell;
        self.window.set_ime_cursor_area(PhysicalPosition::new(x, y), PhysicalSize::new(cell.width, cell.height));
    }

    fn mouse_tracking(&self) -> bool {
        self.snap.as_ref().is_some_and(|s| s.modes.mouse_tracking)
    }

    fn mouse_event(&self, action: MouseAction, button: Option<MouseButton>) {
        let (x, y) = self.layout.grid_px(self.origin(), self.mouse.x, self.mouse.y);
        let mut m = MouseInput::at_pixel(action, button, x, y);
        m.mods = input::mods(self.mods);
        m.any_pressed = !self.held.is_empty();
        self.handle().mouse(m);
    }

    fn keyboard(&mut self, ev: &KeyEvent) {
        // While a composition is shown, text keys belong to the IME; the
        // result arrives as Ime::Commit.
        if self.preedit.is_some() && ev.state == ElementState::Pressed && ev.text.is_some() {
            return;
        }
        let code = match ev.physical_key {
            PhysicalKey::Code(c) => Some(c),
            PhysicalKey::Unidentified(_) => None,
        };
        if ev.state == ElementState::Pressed && input::is_paste(code, self.mods) {
            if let Some(text) = self.clipboard.get() {
                self.handle().paste(text);
            }
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
        log::debug!("key {:?} {:?} mods={:?} text={:?} unshifted={:?}", k.key, k.action, k.mods, k.text, k.unshifted);
        self.handle().key(k);
    }

    fn ime(&mut self, ime: Ime) {
        log::debug!("ime {ime:?}");
        match ime {
            Ime::Enabled => {}
            Ime::Preedit(text, cursor) => {
                self.preedit =
                    if text.is_empty() { None } else { Some(Preedit { text, cursor: preedit_cursor(cursor) }) };
            }
            Ime::Commit(text) => {
                self.preedit = None;
                if !text.is_empty() {
                    self.handle().key(KeyInput::press(Key::Unidentified).with_text(text));
                }
            }
            Ime::Disabled => self.preedit = None,
        }
        self.mark_cursor_row();
        self.window.request_redraw();
    }

    fn mark_cursor_row(&mut self) {
        if let Some(c) = self.snap.as_ref().and_then(|s| s.cursor) {
            if let Some(d) = self.dirty.get_mut(c.y as usize) {
                *d = true;
            }
        }
    }

    fn draw(&mut self) {
        let Some(snap) = self.snap.as_ref() else { return };
        if self.dirty.len() != snap.rows as usize {
            self.dirty = vec![true; snap.rows as usize];
        }
        let scene =
            Scene { snap, dirty: &self.dirty, full: self.full, focused: self.focused, preedit: self.preedit.as_ref() };
        if let Err(e) = self.renderer.render(&scene) {
            log::error!("render: {e}");
        }
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
            self.handle().resize(grid);
        }
        self.window.request_redraw();
    }
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

    fn user_event(&mut self, el: &ActiveEventLoop, ev: UserEvent) {
        let Some(win) = self.win.as_mut() else { return };
        let UserEvent::Session(ev) = ev;
        match ev {
            SessionEvent::Wakeup => win.refresh(),
            SessionEvent::Title(t) => win.window.set_title(if t.is_empty() { "ccterm" } else { &t }),
            SessionEvent::Exited(status) => {
                log::info!("child exited: {status:?}");
                self.exit_code = status.code.map_or(1, |c| c as i32);
                el.exit();
            }
            SessionEvent::Error(e) => log::error!("session: {e}"),
            SessionEvent::Bell => {
                win.window.request_user_attention(Some(winit::window::UserAttentionType::Informational))
            }
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
            WindowEvent::CloseRequested => el.exit(),
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
                    let lines = input::wheel_lines(n, win.mouse_tracking());
                    log::debug!("wheel {delta:?} -> {n} notches, {lines} lines at {col},{row}");
                    win.handle().wheel(lines, col, row, input::mods(win.mods));
                }
            }
            WindowEvent::Focused(f) => {
                win.focused = f;
                win.handle().focus(f);
                win.mark_cursor_row();
                win.window.request_redraw();
            }
            _ => {}
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
