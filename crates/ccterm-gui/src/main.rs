//! `ccterm`: a terminal window for one ccterm-core session.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::num::NonZeroU32;
use std::rc::Rc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

#[derive(Default)]
struct App {
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = Rc::new(el.create_window(Window::default_attributes().with_title("ccterm")).expect("window"));
        let ctx = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        self.surface = Some(softbuffer::Surface::new(&ctx, window.clone()).expect("softbuffer surface"));
        self.window = Some(window);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::RedrawRequested => {
                let (Some(w), Some(s)) = (&self.window, &mut self.surface) else { return };
                let size = w.inner_size();
                let (Some(width), Some(height)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
                    return;
                };
                s.resize(width, height).expect("resize");
                let mut buf = s.buffer_mut().expect("buffer");
                buf.fill(0x001e1e1e);
                buf.present().expect("present");
            }
            _ => {}
        }
    }
}

fn main() {
    let el = EventLoop::new().expect("event loop");
    el.set_control_flow(ControlFlow::Wait);
    let _ = el.run_app(&mut App::default());
}
