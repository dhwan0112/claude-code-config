//! The winit application: one window, one session.

use std::rc::Rc;

use ccterm_core::{GridSize, Session, SessionConfig, SessionEvent, SessionHandle, Snapshot};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use crate::damage;
use crate::font::Fonts;
use crate::geom::Layout;
use crate::render::paint::Painter;
use crate::render::soft::SoftRenderer;
use crate::render::{Preedit, Renderer, Scene};
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
    grid: GridSize,
    session: Session,
    snap: Option<Snapshot>,
    /// Rows changed since the last frame was drawn.
    dirty: Vec<bool>,
    full: bool,
    focused: bool,
    preedit: Option<Preedit>,
}

impl App {
    pub fn new(opts: Options, proxy: EventLoopProxy<UserEvent>, fonts: Fonts) -> Self {
        App { opts, proxy, fonts: Some(fonts), win: None, exit_code: 0 }
    }

    fn font_px(&self, scale: f64) -> f32 {
        (self.opts.font_size * 96.0 / 72.0 * scale) as f32
    }

    fn create(&mut self, el: &ActiveEventLoop) -> Result<(), String> {
        // Cell metrics are needed for the initial window size, before the
        // window (and its scale factor) exists: use the primary monitor's
        // scale. If the window lands elsewhere, ScaleFactorChanged fixes it.
        let scale0 = el.primary_monitor().map_or(1.0, |m| m.scale_factor());
        let fonts = self.fonts.take().ok_or("fonts already used")?;
        let mut painter = Painter::new(fonts, self.font_px(scale0));
        let padding0 = (4.0 * scale0).round() as u32;
        let (w, h) =
            Layout { cell: painter.cell_metrics(), padding: padding0 }.window_size(self.opts.cols, self.opts.rows);
        let attrs = Window::default_attributes().with_title("ccterm").with_inner_size(PhysicalSize::new(w, h));
        let window = Rc::new(el.create_window(attrs).map_err(|e| format!("window: {e}"))?);
        let scale = window.scale_factor();
        painter.set_px(self.font_px(scale));
        let mut renderer: Box<dyn Renderer> =
            Box::new(SoftRenderer::new(window.clone(), painter).map_err(|e| format!("renderer: {e}"))?);
        let padding = (4.0 * scale).round() as u32;
        renderer.set_padding(padding);
        let layout = Layout { cell: renderer.cell_metrics(), padding };
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

        self.win = Some(Win {
            window,
            renderer,
            layout,
            grid,
            session,
            snap: None,
            dirty: vec![],
            full: true,
            focused: true,
            preedit: None,
        });
        Ok(())
    }
}

impl Win {
    fn handle(&self) -> &SessionHandle {
        &self.session.handle
    }

    /// Take a snapshot now (so the engine may send the next wakeup) and
    /// schedule a redraw.
    fn refresh(&mut self) {
        let Some(next) = self.handle().snapshot() else { return };
        let d = damage::dirty_rows(self.snap.as_ref(), &next);
        damage::merge(&mut self.dirty, &d);
        self.snap = Some(next);
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
        self.full = true;
        if size.width == 0 || size.height == 0 {
            return; // minimised
        }
        let grid = self.layout.grid(size.width, size.height);
        if grid != self.grid {
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
            SessionEvent::ClipboardWrite(_) => {}
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
                let padding = (4.0 * scale_factor).round() as u32;
                win.renderer.set_padding(padding);
                win.layout = Layout { cell: win.renderer.cell_metrics(), padding };
                let size = win.window.inner_size();
                win.grid = GridSize::new(0, 0);
                win.resized(size);
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
