//! The winit application: one window with tabs. Each tab is a session in the
//! daemon (`haetty daemon`, started on demand); this process is a client
//! that mirrors their screens (DESIGN.md §3.1, §3.6). The window grid is
//! the active tab's screen plus the cells haetty draws itself (tab bar from
//! two tabs on, palette, hints; `crate::ui`).

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use haetty_core::theme::Theme;
use haetty_core::TermColors;
use haetty_core::{GridSize, Key, KeyInput, MouseAction, MouseButton, MouseInput, SessionEvent, Snapshot};
use haetty_proto::paths::Endpoint;
use haetty_proto::wire::{rgb, Attention, ClaudeInfo, CreateSpec, SessionInfo, SessionState};
use haetty_proto::{Client, Origin, RemoteHandle, RemoteSession, Role};
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
use crate::diffview::DiffView;
use crate::diffview::{self, Mode as DiffMode};
use crate::font::Fonts;
use crate::geom::Layout;
use crate::gitview;
use crate::input::{self, RawKey, Wheel};
use crate::keymap::{Action, Chord, Keymap};
use crate::links;
use crate::render::paint::{Look, Painter};
use crate::render::soft::SoftRenderer;
use crate::render::{PaddingStyle, Preedit, Renderer, Scene};
use crate::tabsets::{self, SavedTab};
use crate::tasks::{self, Task};
use crate::titlebar;
use crate::ui::{self, Compose, Palette, Pick, TabView};
use crate::Options;
use haetty_cc::orch::{self, WorkerState};

#[path = "app_settings.rs"]
mod app_settings;
#[path = "app_sidebar.rs"]
mod app_sidebar;

/// How long a close request waits for its confirming second press, and how
/// long a hint stays.
const CONFIRM: Duration = Duration::from_secs(3);

/// Events from other threads into the event loop.
#[derive(Debug)]
pub enum UserEvent {
    /// An event of the session with this id.
    Session(String, SessionEvent),
    /// The updater thread's report (stage PU).
    Update(crate::updater::Event),
    /// The sidebar's background reads (stage PH, H2).
    Sidebar(SidebarEvent),
}

#[derive(Debug)]
pub enum SidebarEvent {
    /// An agent's record was read (its id; the record or why not).
    Record(String, Result<crate::agentview::Record, String>),
    /// A shell tab's git changes and branch (its session id).
    Git(String, crate::sidebar::GitSummary, Option<String>),
}

pub struct App {
    opts: Options,
    proxy: EventLoopProxy<UserEvent>,
    fonts: Option<Fonts>,
    win: Option<Win>,
    pub exit_code: i32,
    /// "Restart now" / "Roll back" closed the window: start the new version (`main`), watching for `previous`.
    pub restart: Option<(haetty_update::layout::Form, Option<String>, u32)>,
}

/// What a tab runs: program, arguments, folder; a name to show until the
/// program sets a title (restored tabs).
#[derive(Clone, Debug, Default)]
struct TabSpec {
    program: String,
    args: Vec<String>,
    cwd: Option<String>,
    name: Option<String>,
    /// `name` was given by the person (kept over automatic names).
    custom: bool,
    /// Arguments for the first start only, never saved (a worker's task
    /// prompt, DESIGN.md §8.1).
    once: Vec<String>,
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
    /// Claude Code with haetty's integration: a status row is reserved.
    integrated: bool,
    claude: Option<Arc<ClaudeInfo>>,
    /// The person's name for the tab (Rename tab).
    custom_name: Option<String>,
    /// Claude Code tabs: `repo:branch` of the tab's folder, and which folder
    /// and when that was read.
    git_name: Option<String>,
    git_read: Option<(String, Instant)>,
    /// An orchestrator worker: its task's id and title.
    task: Option<(String, String)>,
    /// Observe-only readers may see it (DESIGN.md §6.1); the daemon keeps the
    /// flag, this is the window's copy for the tab bar.
    observable: bool,
    /// The sidebar (stage PH, H2): a shell tab's git changes, when read and whether a read runs; the tab's latest
    /// event-log lines and when asked; whether `claude` runs in it without the integration, and when checked.
    git_sum: Option<crate::sidebar::GitSummary>,
    git_sum_at: Option<Instant>,
    git_sum_busy: bool,
    git_branch: Option<String>,
    log: Vec<haetty_proto::wire::LogEvent>,
    log_at: Option<Instant>,
    unintegrated: bool,
    fg_at: Option<Instant>,
    /// Since when the tab waits for the person (its mark is Wait or Ask): "All agents" lists the longest first.
    waiting_since: Option<Instant>,
}

impl Tab {
    /// The folder the tab's program works in (as far as haetty knows).
    fn folder(&self) -> Option<String> {
        let pwd = self.snap.as_ref().map(|s| osc7_path(&s.pwd)).filter(|p| !p.is_empty());
        pwd.or_else(|| self.spec.cwd.clone())
    }

    /// Reads `repo:branch` again when the folder changed or 3 s passed
    /// (files under `.git` only; Claude Code tabs). True when it changed.
    fn refresh_git(&mut self) -> bool {
        if !haetty_cc::settings::is_claude(&self.spec.program) {
            return false;
        }
        let Some(dir) = self.folder() else { return false };
        if self.git_read.as_ref().is_some_and(|(d, t)| *d == dir && t.elapsed() < Duration::from_secs(3)) {
            return false;
        }
        let name = gitview::repo_branch(std::path::Path::new(&dir));
        self.git_read = Some((dir, Instant::now()));
        let changed = name != self.git_name;
        self.git_name = name;
        changed
    }

    fn attention(&self) -> Attention {
        self.claude.as_ref().map_or(Attention::None, |c| c.attention)
    }

    /// Program input is refused: a permission dialog is open or was answered and its turn has not ended (K1).
    fn asking(&self) -> bool {
        self.claude.as_ref().is_some_and(|c| c.asking)
    }

    /// A worker's state (DESIGN.md §8.2).
    fn worker_state(&self) -> WorkerState {
        let c = self.claude.as_deref();
        let alive = self.exited.is_none();
        WorkerState::of(alive, c.is_some_and(|c| c.busy), c.and_then(|c| c.report.as_ref()))
            .with_background(alive, c.map_or(0, |c| c.background))
    }

    fn name(&self) -> String {
        if let Some((_, title)) = &self.task {
            // The state glyph stays; an own name replaces the task title.
            let name = format!("{} {}", self.worker_state().glyph(), self.custom_name.as_deref().unwrap_or(title));
            return match self.exited {
                Some(code) => format!("{name} [exited{}]", code.map(|c| format!(" {c}")).unwrap_or_default()),
                None => name,
            };
        }
        let title = self.snap.as_ref().map_or("", |s| s.title.as_str());
        // Claude Code's session name (`--name`, /rename or its own title) first.
        let session_name: String = self
            .claude
            .as_ref()
            .map(|c| c.name.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string())
            .unwrap_or_default();
        // The person's name; Claude Code's session name; `repo:branch`
        // (Claude Code tabs); a restored name while the program has no title;
        // the title.
        let name = match (&self.spec.name, title.trim().is_empty()) {
            _ if self.custom_name.is_some() => self.custom_name.clone().unwrap_or_default(),
            _ if !session_name.is_empty() => session_name,
            _ if self.git_name.is_some() => self.git_name.clone().unwrap_or_default(),
            (Some(n), true) => n.clone(),
            _ => ui::tab_name(title, &self.program),
        };
        match self.exited {
            Some(code) => format!("{name} [exited{}]", code.map(|c| format!(" {c}")).unwrap_or_default()),
            None => name,
        }
    }
}

/// What the window opens once its scale is known (A7): on Wayland the scale
/// arrives only after the surface is shown on an output, and a session's PTY
/// size (cells and their pixels) must not be set from a guess.
struct PendingStart {
    previous: Vec<tabsets::SavedTab>,
    explicit: bool,
    first_hint: Option<String>,
}

/// How long a Wayland window waits after its first frame for a scale event
/// before it opens its tabs at the scale it has (1.0: no event comes).
const SCALE_WAIT: Duration = Duration::from_millis(300);

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
    /// What is drawn: the active tab's screen with haetty's own cells.
    composed: Option<Snapshot>,
    /// Rows changed since the last frame was drawn.
    dirty: Vec<bool>,
    full: bool,
    /// haetty asked for the next `RedrawRequested` itself. One that comes
    /// without it is the system's (X11 `Expose`): the window lost its
    /// contents, so the frame is drawn in full.
    asked_redraw: bool,
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
    /// The closed tabs the palette lists (Reopen a closed tab).
    closed: Vec<SavedTab>,
    /// Orchestrator tasks (DESIGN.md §8), from `tasks/` in the state directory.
    tasks: Vec<Task>,
    /// Worker sessions no window shows (`haetty task new` makes them so); the sidebar's Workers opens one on a click.
    detached_workers: Vec<SessionInfo>,
    workers_at: Option<Instant>,
    /// The worker program and its arguments (`worker-command`).
    worker_command: Vec<String>,
    hint: Option<(String, Instant)>,
    /// The daemon's connection broke: the tabs' programs ended with it. The
    /// tab sets stay as they were (a restart reopens them), the window stays
    /// open with a hint that does not expire (stage S4).
    daemon_lost: bool,
    /// A close request waiting for its second press: (session id, time).
    pending_close: Option<(String, Instant)>,
    /// Keys whose press was a shortcut: their release is not sent either.
    swallowed: HashSet<KeyCode>,
    /// What new tabs run.
    command: (String, Vec<String>),
    start_cwd: Option<String>,
    /// Colours for new sessions and haetty's own cells.
    theme: Theme,
    /// `decorations` resolved for this window system: haetty draws the title
    /// row (`titlebar.rs`) — always shown, the window buttons in it.
    integrated: bool,
    buttons: titlebar::ButtonLayout,
    /// Window button under the pointer; the one pressed (acts on release).
    hover: Option<titlebar::Button>,
    pressed_button: Option<titlebar::Button>,
    /// Last left press on the title row's empty space (double-click).
    last_title_press: Option<(Instant, PhysicalPosition<f64>)>,
    /// Window edge under the pointer (integrated, not maximized).
    edge: Option<winit::window::ResizeDirection>,
    /// The hyperlink under the pointer, shown on the status row's place
    /// (DESIGN.md §2.2); Ctrl+click opens only this one.
    link_hover: Option<Arc<str>>,
    /// A Ctrl+click on a link took the left button: its release is not
    /// sent either.
    link_press: bool,
    /// Why the last Ctrl+click on the hovered link did not open it (shown
    /// after its target).
    link_note: Option<String>,
    /// The renderer's colours and `minimum-contrast` now in use.
    look: Look,
    /// Command-line settings (applied over the config file on a reload).
    settings: Vec<(String, String)>,
    /// The config file's modification time at the last (re)load.
    config_mtime: Option<SystemTime>,
    scrollback_limit: usize,
    engine: haetty_core::vt::EngineKind,
    claude_integration: bool,
    shell_integration: bool,
    /// `restore-claude = resume` (stage PH, H1).
    restore_claude: bool,
    /// OS notification (Linux `notify-send`) besides the taskbar/urgency
    /// hint when Claude Code waits and the window is unfocused.
    notifications: bool,
    /// New tabs start observable (`observe-new-tabs`, DESIGN.md §6.1).
    observe_new_tabs: bool,
    /// The transcripts read for the token lines, in the background (`tokens.rs`).
    tokens: crate::tokens::Watch,
    /// `token-status` (the config's), read when the window is made.
    token_status: bool,
    /// `clear-hint` (percent, 0 off) and `context-window` (tokens): the dim `/clear` mark of worker tabs.
    clear_hint: u8,
    saving_mode: bool,
    context_window: u64,
    /// The sidebar (DESIGN.md §13.3; stage PH): shown on the right when wide enough (the person can hide it; kept in
    /// the ui state file), its width with the line (columns), shown over the terminal (a narrow window), folded
    /// sections, what each of its rows does, the overlay's keyboard selection, a press taken by it, an agent's record.
    sidebar_shown: bool,
    panel_width: u16,
    sidebar_overlay: bool,
    folded: Vec<crate::sidebar::Section>,
    sidebar_hits: Vec<(usize, crate::sidebar::Hit)>,
    sidebar_sel: Option<usize>,
    /// The docked sidebar has the keyboard (Ctrl+Shift+E, stage PI): nothing typed reaches the program.
    sidebar_focus: bool,
    sidebar_press: bool,
    agent_view: Option<crate::agentview::AgentView>,
    /// The next redraw of running agents' elapsed times (each second).
    sidebar_second: Option<Instant>,
    /// "All agents": the tab of each line in the palette.
    agent_pick: Vec<usize>,
    diff: Option<DiffView>,
    diff_pref: Option<diffview::DiffLayoutPref>,
    last_slow_draw: Option<Instant>,
    /// Tabs not opened yet: waiting for the scale (Wayland; A7).
    pending_start: Option<PendingStart>,
    /// When the waiting window drew its first (blank) frame.
    pending_since: Option<Instant>,
    /// The grid asked for (`--cols`, `--rows`, the title row), kept when
    /// the scale becomes known.
    requested_grid: GridSize,
    /// The settings in effect: the config file with the command line's keys on top (stage PS).
    cfg: crate::config::Config,
    /// The settings panel (DESIGN.md §2.4), and the config file it writes.
    settings_panel: Option<crate::settings::SettingsPanel>,
    cfg_file: crate::config_edit::ConfigFile,
    /// The system's monospaced families, read when the panel's font list first opens.
    font_list: Option<Vec<String>>,
    /// The settings button's text (`ui::GEAR`, or `ui::GEAR_FALLBACK` without a single-colour ⚙).
    gear: &'static str,
    /// The pointer is near the bottom-right corner (other tabs show the button), and on the button.
    gear_near: bool,
    gear_hover: bool,
    /// A left press opened the panel from the button: its release is not sent either.
    gear_press: bool,
    /// The theme list shows a theme that is not in effect.
    previewing: bool,
    /// A font or padding change waits this long before the sessions get the new size.
    pending_relayout: Option<Instant>,
    /// Stage PU: the updater thread's handle and what it found.
    updater: crate::updater::Updater,
    /// "Restart now" or "Roll back" was confirmed: the window closes after this event (`App::window_event`); the
    /// version to go back to if the new one ends at its start.
    restart_req: Option<Option<String>>,
    /// Set when the last tab is gone: the window closes.
    quit: Option<i32>,
    /// The window is closing: closing its tabs must not shrink `last`.
    shutting_down: bool,
}

impl App {
    pub fn new(opts: Options, proxy: EventLoopProxy<UserEvent>, fonts: Fonts) -> Self {
        App { opts, proxy, fonts: Some(fonts), win: None, exit_code: 0, restart: None }
    }

    fn font_px(&self, scale: f64) -> f32 {
        (self.opts.config.font_size * 96.0 / 72.0 * scale) as f32
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
        let theme = self.opts.theme.clone();
        let look = Look { min_contrast: self.opts.config.minimum_contrast, ..Look::of(&theme) };
        painter.set_look(look);
        let pad0 = self.padding(scale0);
        #[cfg(windows)]
        let ws = crate::config::WindowSystem::Windows;
        #[cfg(not(windows))]
        let ws = if winit::platform::wayland::ActiveEventLoopExtWayland::is_wayland(el) {
            crate::config::WindowSystem::Wayland
        } else {
            crate::config::WindowSystem::X11
        };
        let deco = self.opts.config.decorations.effective(ws);
        let integrated = deco == Decorations::Integrated;
        log::info!("decorations: {:?} on {ws:?} -> {deco:?}", self.opts.config.decorations);
        #[cfg(all(unix, not(target_os = "macos")))]
        let buttons = if integrated {
            let (b, from) = titlebar::read_system(Duration::from_millis(150));
            log::info!("title buttons from {from}: {b:?}");
            b
        } else {
            titlebar::ButtonLayout::none()
        };
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        let buttons = titlebar::ButtonLayout::none();
        // The title row is one more row: the program keeps the requested rows.
        let requested_grid = GridSize::new(self.opts.cols, self.opts.rows + u16::from(integrated));
        let (w, h) = Layout { cell: painter.cell_metrics(), padding: pad0.min, balance: pad0.balance }
            .window_size(requested_grid.cols, requested_grid.rows);
        log::info!(
            "window: {w}x{h} px asked for a {}x{} grid at scale {scale0}",
            requested_grid.cols,
            requested_grid.rows
        );
        let attrs = Window::default_attributes()
            .with_title(haetty_proto::product::display_name())
            .with_inner_size(PhysicalSize::new(w, h));
        #[cfg(not(windows))]
        let attrs = attrs.with_window_icon(crate::icon::window_icon());
        // Windows: small icon (title bar) and big icon (Alt+Tab, taskbar),
        // both from the executable's icon resource.
        #[cfg(windows)]
        let attrs = {
            let (small, big) = crate::icon::window_icons();
            winit::platform::windows::WindowAttributesExtWindows::with_taskbar_icon(attrs.with_window_icon(small), big)
        };
        // Wayland app_id and X11 WM_CLASS = the desktop file's name.
        #[cfg(all(unix, not(target_os = "macos")))]
        let attrs = {
            let id = haetty_proto::product::app_id();
            let a = winit::platform::wayland::WindowAttributesExtWayland::with_name(attrs, &id, "");
            winit::platform::x11::WindowAttributesExtX11::with_name(a, &id, &id)
        };
        let attrs =
            chrome::attributes(attrs, theme.background, theme.foreground).with_decorations(deco == Decorations::Native);
        let chrome = Chrome::new(theme.background, theme.foreground);
        let window = Rc::new(el.create_window(attrs).map_err(|e| format!("window: {e}"))?);
        window.set_ime_allowed(true);
        window.set_ime_purpose(ImePurpose::Terminal);
        let scale = window.scale_factor();
        painter.set_px(self.font_px(scale));
        let mut renderer: Box<dyn Renderer> =
            Box::new(SoftRenderer::new(window.clone(), painter).map_err(|e| format!("renderer: {e}"))?);
        // DESIGN.md §2.4: a single-colour ⚙, else ≡.
        let gear = if renderer.text_glyph('\u{2699}') { ui::GEAR } else { ui::GEAR_FALLBACK };
        log::info!("settings button: {}", if gear == ui::GEAR { "⚙" } else { "≡ (no single-colour ⚙ in the fonts)" });
        let pad = self.padding(scale);
        renderer.set_padding(pad);
        let layout = Layout { cell: renderer.cell_metrics(), padding: pad.min, balance: pad.balance };
        let size = window.inner_size();
        let grid =
            if size.width > 0 && size.height > 0 { layout.grid(size.width, size.height) } else { layout.grid(w, h) };
        renderer.resize(size.width, size.height);

        let ep = Endpoint::default_for_user().map_err(|e| format!("daemon socket: {e}"))?;
        let client = Client::connect_or_spawn(&ep, Role::Control, "gui").map_err(|e| match &e {
            // Stage PU: an update installed beside a running older daemon; it is not ended, only named.
            haetty_proto::client::ConnectError::Refused(r) if r.starts_with("protocol version") => {
                crate::updater::old_daemon_note(&crate::updater::form()).unwrap_or_else(|| e.to_string())
            }
            _ => e.to_string(),
        })?;
        install_panic_report(client.reporter());
        let cmd = &self.opts.command;
        let command = (
            cmd.program.to_string_lossy().into_owned(),
            cmd.args.iter().map(|a| a.to_string_lossy().into_owned()).collect(),
        );
        let clipboard = Clipboard::new(&window);
        let ui_state = load_ui_state();
        let mut win = Win {
            window,
            renderer,
            layout,
            size,
            grid,
            client,
            proxy: self.proxy.clone(),
            tabs: vec![],
            tokens: crate::tokens::Watch::new(),
            token_status: self.opts.config.token_status,
            clear_hint: self.opts.config.clear_hint,
            saving_mode: self.opts.config.saving_mode,
            context_window: self.opts.config.context_window,
            active: 0,
            composed: None,
            dirty: vec![],
            full: true,
            asked_redraw: false,
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
            closed: vec![],
            tasks: tasks::load_all(&tasks::dir()),
            detached_workers: vec![],
            workers_at: None,
            worker_command: self.opts.config.worker_command.split_whitespace().map(str::to_string).collect(),
            hint: None,
            link_hover: None,
            link_press: false,
            link_note: None,
            daemon_lost: false,
            pending_close: None,
            swallowed: HashSet::new(),
            command,
            start_cwd: std::env::current_dir().ok().map(|d| d.to_string_lossy().into_owned()),
            theme,
            integrated,
            buttons,
            hover: None,
            pressed_button: None,
            last_title_press: None,
            edge: None,
            look,
            settings: self.opts.settings.clone(),
            config_mtime: crate::config::file_mtime(),
            scrollback_limit: self.opts.config.scrollback_limit,
            engine: self.opts.config.engine,
            claude_integration: self.opts.config.claude_integration,
            shell_integration: self.opts.config.shell_integration,
            restore_claude: self.opts.config.restore_claude,
            notifications: self.opts.config.notifications,
            observe_new_tabs: self.opts.config.observe_new_tabs,
            sidebar_shown: ui_state.shown,
            panel_width: ui_state.width.unwrap_or(self.opts.config.sidebar_width).clamp(24, 80),
            sidebar_overlay: false,
            folded: ui_state.folded,
            sidebar_hits: vec![],
            sidebar_sel: None,
            sidebar_focus: false,
            sidebar_press: false,
            agent_view: None,
            sidebar_second: None,
            agent_pick: vec![],
            diff: None,
            diff_pref: None,
            last_slow_draw: None,
            cfg: self.opts.config.clone(),
            settings_panel: None,
            cfg_file: crate::config_edit::ConfigFile::new(crate::config::file_path()),
            font_list: None,
            gear,
            gear_near: false,
            gear_hover: false,
            gear_press: false,
            previewing: false,
            pending_relayout: None,
            updater: {
                let proxy = self.proxy.clone();
                crate::updater::Updater::start(
                    move |ev| {
                        let _ = proxy.send_event(UserEvent::Update(ev));
                    },
                    self.opts.watch_start,
                )
            },
            restart_req: None,
            quit: None,
            shutting_down: false,
            pending_start: None,
            pending_since: None,
            requested_grid,
        };
        // "last" becomes "previous" before the first tab rewrites "last".
        let previous = if self.opts.config.restore_tabs { tabsets::rotate_last(&tabsets::dir()) } else { vec![] };
        let start =
            PendingStart { previous, explicit: self.opts.explicit_program, first_hint: self.opts.first_hint.take() };
        if ws == crate::config::WindowSystem::Wayland {
            // A7: Wayland reports the scale only once the surface is on an
            // output. Show a blank frame of the requested grid and open the
            // tabs when the scale is known, so no session starts at a guessed size.
            win.pending_start = Some(start);
            win.blank_frame();
            win.redraw();
        } else {
            win.start_tabs(start)?;
        }
        self.win = Some(win);
        Ok(())
    }
}

impl Win {
    /// Opens the window's first tabs: sessions a gone window left in the
    /// daemon, the previous run's tabs, the requested program (DESIGN.md §3.8).
    fn start_tabs(&mut self, mut p: PendingStart) -> Result<(), String> {
        // Sessions of a window that is gone (crashed or killed) are still in
        // the daemon: take them back first; restore only the saved tabs the
        // daemon does not have (DESIGN.md §3.8).
        let live = self.client.list_sessions().unwrap_or_default();
        for s in live.iter().filter(|s| s.attached == 0 && s.state != SessionState::Running) {
            let _ = self.client.close(&s.id); // exited, and no window shows it
        }
        let detached: Vec<SessionInfo> =
            live.iter().filter(|s| s.attached == 0 && s.state == SessionState::Running).cloned().collect();
        let missing = tabsets::not_live(&p.previous, &live);
        if p.explicit || (detached.is_empty() && missing.is_empty()) {
            self.open_tab(None)?;
        }
        let mut back = 0;
        for s in &detached {
            match self.reattach(s) {
                Ok(()) => back += 1,
                Err(e) => log::warn!("reattach {}: {e}", s.id),
            }
        }
        let reopened = self.open_saved(&missing);
        if self.tabs.is_empty() {
            self.open_tab(None)?;
        }
        if back + reopened > 0 {
            self.select(0);
            let mut parts = vec![];
            if back > 0 {
                parts.push(format!("reattached {back} running session(s)"));
            }
            if reopened > 0 {
                parts.push(format!("reopened {reopened} tab(s) of the previous run"));
            }
            self.show_hint(parts.join(", "));
        }
        if let Some(h) = p.first_hint.take() {
            self.show_hint(h);
        }
        self.save_last();
        Ok(())
    }

    /// A frame of the requested grid with nothing in it (a Wayland window
    /// waiting for its scale must be shown to get one).
    fn blank_frame(&mut self) {
        let (cols, rows) = (self.grid.cols.max(1), self.grid.rows.max(1));
        let blank = haetty_core::Cell {
            text: String::new(),
            fg: None,
            bg: None,
            flags: haetty_core::CellFlags(0),
            width: haetty_core::Width::Narrow,
            ..Default::default()
        };
        self.composed = Some(Snapshot {
            cols,
            rows,
            cells: vec![blank; cols as usize * rows as usize],
            default_fg: self.theme.foreground,
            default_bg: self.theme.background,
            ..Default::default()
        });
        self.dirty = vec![true; rows as usize];
        self.full = true;
    }

    /// The waiting window was resized (no tabs to tell yet).
    fn pending_resized(&mut self, size: PhysicalSize<u32>) {
        self.renderer.resize(size.width, size.height);
        self.size = size;
        if size.width > 0 && size.height > 0 {
            self.grid = self.layout.grid(size.width, size.height);
        }
        self.blank_frame();
        self.redraw();
    }

    /// The scale is known (or did not come): open the tabs now.
    fn start_pending(&mut self) {
        let Some(p) = self.pending_start.take() else { return };
        log::info!(
            "scale {} known: opening tabs at a {}x{} grid",
            self.window.scale_factor(),
            self.grid.cols,
            self.grid.rows
        );
        self.composed = None;
        if let Err(e) = self.start_tabs(p) {
            log::error!("first tab: {e}");
            self.quit = Some(1);
            return;
        }
        self.redraw();
    }

    /// Rows of haetty's own chrome above the terminal (the tab bar, shown
    /// from two tabs on).
    fn top(&self) -> u16 {
        u16::from(self.integrated || self.tabs.len() >= 2)
    }

    /// Columns of the docked sidebar (with its line): `sidebar = auto`, not hidden by the person, and a window of at
    /// least `sidebar-min-cols` columns (DESIGN.md §13.3). The same for every tab, so switching tabs never resizes.
    fn panel_cols(&self) -> u16 {
        crate::sidebar::docked_cols(
            self.grid.cols,
            self.cfg.sidebar_auto,
            self.cfg.sidebar_min_cols,
            self.panel_width,
            self.sidebar_shown,
        )
    }

    /// The window is wide enough for the docked sidebar (and `sidebar = auto`).
    fn sidebar_wide(&self) -> bool {
        crate::sidebar::docked_cols(
            self.grid.cols,
            self.cfg.sidebar_auto,
            self.cfg.sidebar_min_cols,
            self.panel_width,
            true,
        ) > 0
    }

    /// Columns of the sidebar shown over the terminal (narrow window, Ctrl+Shift+S): the terminal keeps its size.
    fn overlay_cols(&self) -> u16 {
        if self.panel_cols() == 0 && self.sidebar_overlay {
            self.panel_width.min(self.grid.cols.saturating_sub(10))
        } else {
            0
        }
    }

    /// The status row of the active tab: Claude Code tabs, while the sidebar is not docked (it shows all of it).
    fn status_rows(&self) -> u16 {
        u16::from(self.tabs.get(self.active).is_some_and(|t| t.integrated) && self.panel_cols() == 0)
    }

    /// The grid a tab's program gets: the window minus the tab bar, the
    /// status row of a Claude Code tab and the status panel.
    fn term_grid_for(&self, integrated: bool, tabs: usize) -> GridSize {
        let mut g = self.grid;
        let top = u16::from(self.integrated || tabs >= 2);
        let status = u16::from(integrated && self.panel_cols() == 0);
        g.rows = g.rows.saturating_sub(top + status).max(1);
        g.cols = g.cols.saturating_sub(self.panel_cols()).max(1);
        g
    }

    fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    fn handle(&self) -> &RemoteHandle {
        &self.tab().session.handle
    }

    /// Opens a tab running the window's command in the active tab's
    /// directory (else where haetty started) and selects it.
    fn open_tab(&mut self, cwd: Option<String>) -> Result<(), String> {
        let spec = TabSpec {
            program: self.command.0.clone(),
            args: self.command.1.clone(),
            cwd,
            name: None,
            custom: false,
            once: vec![],
        };
        self.open_spec(spec)
    }

    fn is_integrated(&self, program: &str) -> bool {
        self.claude_integration && haetty_cc::settings::is_claude(program)
    }

    /// Shows a session of the daemon that no window shows, as a new tab.
    fn reattach(&mut self, s: &SessionInfo) -> Result<(), String> {
        // Its own name, if the person gave it one (tabnames; kramers 2026-11-09).
        let own = crate::tabnames::load().remove(&s.id);
        let spec = TabSpec {
            program: s.program.clone(),
            // A worker's task prompt is given once (DESIGN.md §8.1).
            args: s.args.iter().filter(|a| !tasks::is_prompt_arg(a)).cloned().collect(),
            cwd: Some(osc7_path(&s.cwd)),
            custom: own.is_some(),
            name: own,
            once: vec![],
        };
        let integrated = self.is_integrated(&s.program);
        let size = self.term_grid_for(integrated, self.tabs.len() + 1);
        log::info!("reattaching session {} ({})", s.id, s.program);
        self.add_tab(s.id.clone(), spec, size, integrated)?;
        if let Some(t) = self.tabs.iter_mut().find(|t| t.session.id == s.id) {
            t.observable = s.observable;
        }
        // It may have been started with other colours (a theme changed while
        // no window showed it): give it the current ones.
        if let Some(t) = self.tabs.iter().find(|t| t.session.id == s.id) {
            t.session.handle.set_colors(TermColors::of(&self.theme));
        }
        Ok(())
    }

    /// Reads the config file again (`force`, or when it changed since the
    /// last read) and applies what applies while running (`config::When::Live`:
    /// colours, `minimum-contrast`, fonts, padding, the settings button, …) to
    /// haetty and to every open tab. Other settings take effect at the next start.
    fn reload_config(&mut self, force: bool) -> bool {
        let mtime = crate::config::file_mtime();
        if !force && mtime == self.config_mtime {
            return false;
        }
        self.config_mtime = mtime;
        let (cfg, bad) = crate::config::reread(&self.settings);
        for b in bad {
            log::warn!("{}: {b} (ignored)", crate::config::file_path().display());
        }
        let fonts = if cfg.font_family != self.cfg.font_family {
            match crate::font::Fonts::load(cfg.font_family.as_deref()) {
                Ok(f) if cfg.font_family.as_deref().is_none_or(|n| f.has_family(n)) => Some(f),
                _ => {
                    log::warn!("font-family: not on this system; the fonts stay");
                    None
                }
            }
        } else {
            None
        };
        let mut cfg = cfg;
        if fonts.is_none() {
            cfg.font_family = self.cfg.font_family.clone();
        }
        let theme = cfg.theme.clone();
        let changed = self.apply_config(cfg, fonts);
        if changed {
            log::info!("config reloaded: theme {theme}; {} tab(s)", self.tabs.len());
        }
        changed
    }

    /// Attaches every detached running session that is not a tab yet.
    fn attach_detached(&mut self) -> usize {
        let live = self.client.list_sessions().unwrap_or_default();
        let mut n = 0;
        for s in live.iter().filter(|s| s.attached == 0 && s.state == SessionState::Running) {
            if self.tabs.iter().all(|t| t.session.id != s.id) && self.reattach(s).is_ok() {
                n += 1;
            }
        }
        n
    }

    /// Opens the tabs of a set after the current ones; tabs whose folder is
    /// gone are skipped. Returns how many opened.
    fn open_saved(&mut self, set: &[SavedTab]) -> usize {
        // Conversations running in the daemon right now (any window): never
        // resumed a second time. Asked now, not taken from the tabs' Claude
        // Code state, which arrives later.
        let live = self.client.list_sessions().unwrap_or_default();
        let mut n = 0;
        for t in set {
            if !t.cwd.is_empty() && !std::path::Path::new(&t.cwd).is_dir() {
                log::warn!("tab set: folder {:?} is gone; tab {:?} skipped", t.cwd, t.name);
                continue;
            }
            let mut args = t.args.clone();
            // A shell tab saved while a proxied `claude` ran in it (stage PH, H1): the shell, then `claude --resume`.
            let mut resume_in_shell = None;
            let claude_tab = haetty_cc::settings::is_claude(&t.program);
            if !claude_tab
                && !t.claude_session.is_empty()
                && self.restore_claude
                && haetty_cc::resume::valid_id(&t.claude_session)
                && haetty_cc::resume::transcript_exists(&t.cwd, &t.claude_session)
                && !live.iter().any(|s| s.state == SessionState::Running && s.claude_session == t.claude_session)
                && !self.tabs.iter().any(|x| {
                    x.exited.is_none() && x.claude.as_ref().is_some_and(|c| c.claude_session == t.claude_session)
                })
            {
                resume_in_shell = Some(t.claude_session.clone());
            }
            if claude_tab && !t.claude_session.is_empty() {
                // The conversation is already open: do not open it twice. A
                // session started with `--resume ID` counts before Claude Code
                // has reported its id (a second reopen right after the first).
                let id = t.claude_session.as_str();
                let resumes = |args: &[String]| {
                    args.windows(2).any(|w| matches!(w[0].as_str(), "--resume" | "-r") && w[1] == id)
                        || args.iter().any(|a| a.strip_prefix("--resume=") == Some(id))
                };
                let running = live
                    .iter()
                    .any(|s| s.state == SessionState::Running && (s.claude_session == id || resumes(&s.args)));
                if running
                    || self.tabs.iter().any(|x| {
                        x.exited.is_none()
                            && (x.claude.as_ref().is_some_and(|c| c.claude_session == id) || resumes(&x.spec.args))
                    })
                {
                    log::info!("tab set: conversation of {:?} is already open; skipped", t.name);
                    continue;
                }
                if haetty_cc::resume::transcript_exists(&t.cwd, &t.claude_session) {
                    args = haetty_cc::resume::resume_args(&args, &t.claude_session);
                } else {
                    log::info!("tab set: no transcript for {:?}; starting a new conversation", t.name);
                }
            }
            let spec = TabSpec {
                program: t.program.clone(),
                args,
                cwd: (!t.cwd.is_empty()).then(|| t.cwd.clone()),
                name: (!t.name.is_empty()).then(|| t.name.clone()),
                custom: t.custom && !t.name.is_empty(),
                once: vec![],
            };
            match self.open_spec(spec) {
                Ok(()) => {
                    n += 1;
                    if let Some(id) = resume_in_shell {
                        // Typed ahead into the new shell; it reads the line once it is ready.
                        self.tab().session.handle.write(Origin::Program, format!("claude --resume {id}\r"));
                    }
                }
                Err(e) => log::warn!("tab set: {:?}: {e}", t.name),
            }
        }
        n
    }

    /// The open tabs as a tab set.
    fn saved_tabs(&self) -> Vec<SavedTab> {
        (0..self.tabs.len()).filter(|&i| self.tabs[i].exited.is_none()).map(|i| self.saved_tab(i)).collect()
    }

    fn saved_tab(&self, i: usize) -> SavedTab {
        let t = &self.tabs[i];
        SavedTab {
            // An exited tab's name without " [exited …]".
            name: t.custom_name.clone().unwrap_or_else(|| {
                let n = t.name();
                if t.exited.is_some() {
                    n.rsplit_once(" [exited").map_or(n.clone(), |(a, _)| a.to_string())
                } else {
                    n
                }
            }),
            cwd: t.folder().unwrap_or_default(),
            program: t.spec.program.clone(),
            // Without an earlier --resume: the session id below says which.
            args: haetty_cc::resume::without_resume(&t.spec.args),
            claude_session: t.claude.as_ref().map(|c| c.claude_session.clone()).unwrap_or_default(),
            active: i == self.active,
            custom: t.custom_name.is_some(),
        }
    }

    /// Keeps the tab set "last" equal to the open tabs.
    fn save_last(&self) {
        if self.shutting_down || self.daemon_lost {
            return;
        }
        if let Err(e) = tabsets::save(&tabsets::dir(), "last", &self.saved_tabs()) {
            log::warn!("tab set last: {e}");
        }
    }

    /// The environment of a new session: haetty's own without the variables that mark a Claude Code child
    /// (`config::CHILD_MARKERS`), and in saving mode the output limits for Claude Code (`config::SAVING_ENV`).
    fn session_env(&self, program: &str) -> Vec<(String, String)> {
        let inherited: Vec<(String, String)> =
            std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?))).collect();
        crate::config::child_env(inherited, self.saving_mode && haetty_cc::settings::is_claude(program))
    }

    fn open_spec(&mut self, spec: TabSpec) -> Result<(), String> {
        // The active tab's folder: what its program reported (OSC 7), else
        // where the tab started — Claude Code reports no folder, so a new tab
        // next to a worker opens in the worker's worktree (T6).
        let cwd = spec.cwd.clone().or_else(|| self.tabs.get(self.active).and_then(Tab::folder));
        let cwd = cwd.or_else(|| self.start_cwd.clone()).map(|p| osc7_path(&p));
        // Size it for the grid it will have (the tab bar may appear with it).
        let integrated = self.is_integrated(&spec.program);
        let size = self.term_grid_for(integrated, self.tabs.len() + 1);
        let create = CreateSpec {
            program: spec.program.clone(),
            args: spec.args.iter().chain(&spec.once).cloned().collect(),
            cwd: cwd.clone(),
            // The daemon may run in an older environment: the child gets ours
            // (never stored, DESIGN.md §3.6).
            env: self.session_env(&spec.program),
            size: size.into(),
            scrollback_limit: self.scrollback_limit as u64,
            default_fg: rgb(self.theme.foreground),
            default_bg: rgb(self.theme.background),
            palette: self.theme.palette.iter().map(|c| rgb(*c)).collect(),
            cursor_color: self.theme.cursor_color.map(rgb),
            claude_integration: self.claude_integration,
            shell_integration: self.shell_integration,
            engine: self.engine.name().into(),
        };
        let id = self.client.create(create).map_err(|e| e.to_string())?;
        let spec = TabSpec { cwd, once: vec![], ..spec };
        self.add_tab(id.clone(), spec, size, integrated)?;
        if self.observe_new_tabs {
            self.set_observable(self.tabs.len() - 1, true);
        }
        Ok(())
    }

    /// Allows or stops observe-only readers for tab `i` (DESIGN.md §6.1).
    fn set_observable(&mut self, i: usize, on: bool) {
        let Some(t) = self.tabs.get_mut(i) else { return };
        match self.client.set_observable(&t.session.id, on) {
            Ok(()) => t.observable = on,
            Err(e) => log::warn!("observable {}: {e}", t.session.id),
        }
    }

    /// Attaches session `id` as a new, selected tab.
    fn add_tab(&mut self, id: String, spec: TabSpec, size: GridSize, integrated: bool) -> Result<(), String> {
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
            .name("haetty-gui-events".into())
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
        let custom_name = spec.name.clone().filter(|_| spec.custom);
        let mut tab = Tab {
            session,
            snap: None,
            spec,
            program,
            exited: None,
            size,
            integrated,
            claude: None,
            custom_name,
            git_name: None,
            git_read: None,
            task: None,
            observable: false,
            git_sum: None,
            git_sum_at: None,
            git_sum_busy: false,
            git_branch: None,
            log: vec![],
            log_at: None,
            unintegrated: false,
            fg_at: None,
            waiting_since: None,
        };
        tab.task = tab
            .spec
            .cwd
            .as_deref()
            .and_then(|c| self.tasks.iter().find(|k| k.is_folder(c)))
            .map(|k| (k.id.clone(), k.title.clone()));
        tab.refresh_git();
        self.tabs.push(tab);
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
        let n = self.tabs.len();
        let grids: Vec<GridSize> = self.tabs.iter().map(|t| self.term_grid_for(t.integrated, n)).collect();
        for (t, g) in self.tabs.iter_mut().zip(grids) {
            if t.size != g && t.exited.is_none() {
                log::debug!("resize session {} to {}x{}", t.session.id, g.cols, g.rows);
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
            self.seen();
        }
        self.diff = None;
        self.preedit = None;
        self.full = true;
        self.ime_at = None;
        self.update_title();
        self.recompose();
        self.save_last();
    }

    fn close_tab(&mut self, i: usize) {
        // Closed on purpose (not the window closing): kept for "Reopen a
        // closed tab".
        if !self.shutting_down && !self.daemon_lost {
            if let Err(e) = tabsets::push_closed(&tabsets::dir(), self.saved_tab(i)) {
                log::warn!("tab set closed: {e}");
            }
        }
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
        self.after_removal(i);
    }

    /// A tab left the window (closed or detached): the next one is active.
    fn after_removal(&mut self, i: usize) {
        if self.active >= self.tabs.len() || self.active > i {
            self.active = self.active.saturating_sub(1).min(self.tabs.len() - 1);
        }
        if self.focused {
            self.handle().focus(true);
        }
        self.diff = None;
        self.update_title();
        self.relayout();
        self.save_last();
    }

    /// A task's worker state, from its open tab (no tab: no report known).
    fn task_state(&self, t: &Task) -> WorkerState {
        self.tabs
            .iter()
            .filter(|x| x.task.as_ref().is_some_and(|(id, _)| *id == t.id))
            .map(Tab::worker_state)
            .next()
            .unwrap_or_else(|| crate::coord::task_state(self.detached_worker(t)))
    }

    fn detached_worker(&self, t: &Task) -> Option<&SessionInfo> {
        if self.worker_tab(&t.id).is_some() {
            return None;
        }
        crate::coord::task_session(t, &self.detached_workers)
    }

    /// The worker's tab: the one whose task is `task` (DESIGN.md §8.1).
    fn worker_tab(&self, task: &str) -> Option<&Tab> {
        self.tabs.iter().find(|x| x.task.as_ref().is_some_and(|(id, _)| id.as_str() == task))
    }

    /// The token line of a tab's session, when `token-status` is on and its transcript has been read (`tokens.rs`).
    /// Whether a worker tab's context has passed the `clear-hint` share of `context-window`: the dim `/clear` mark. The
    /// transcript is read; nothing is sent.
    fn clear_mark(&self, tab: &Tab) -> bool {
        tab.task.is_some()
            && self
                .tokens
                .summary(&tab.session.id)
                .is_some_and(|s| crate::tokens::over_share(s.context, self.context_window, self.clear_hint))
    }

    fn token_line_of(&self, tab: &Tab) -> Option<String> {
        if !self.token_status {
            return None;
        }
        let id = tab.claude.as_ref().map(|c| c.claude_session.as_str()).filter(|s| !s.is_empty())?;
        self.tokens.summary(id).map(|s| crate::tokens::line(&s))
    }

    /// The transcripts the token lines need: the active tab's, and each worker's.
    fn watched_transcripts(&self) -> crate::tokens::Watched {
        let file = |t: &Tab| -> Option<(String, std::path::PathBuf)> {
            let id = t.claude.as_ref().map(|c| c.claude_session.clone()).filter(|s| !s.is_empty())?;
            let folder = t.folder()?;
            let path = haetty_cc::resume::transcript_path(&folder, &id)?;
            Some((id, path))
        };
        let mut out: Vec<(String, std::path::PathBuf)> = file(self.tab()).into_iter().collect();
        for t in &self.tasks {
            if let Some(f) = self.worker_tab(&t.id).and_then(file) {
                if !out.iter().any(|(id, _)| *id == f.0) {
                    out.push(f);
                }
            }
        }
        out
    }

    /// The WORKERS section of the status panel.
    fn worker_lines(&self) -> Vec<ui::WorkerLine> {
        let active = self.tab().task.as_ref().map(|(id, _)| id.clone());
        let first = |s: &str| s.lines().next().unwrap_or("").to_string();
        self.tasks
            .iter()
            .map(|t| {
                let st = self.task_state(t);
                let detached = self.detached_worker(t);
                let asks = self.worker_tab(&t.id).is_some_and(|w| w.attention() == Attention::Ask)
                    || detached.is_some_and(|s| s.attention == Attention::Ask);
                let (glyph, name) = match (asks, detached) {
                    (true, _) => ("?", "asks"),
                    (false, Some(_)) => (st.glyph(), "detached"),
                    (false, None) => (st.glyph(), st.name()),
                };
                let report = (active.as_deref() == Some(t.id.as_str()))
                    .then(|| self.tab().claude.as_ref().and_then(|c| c.report.clone()))
                    .flatten();
                ui::WorkerLine {
                    text: format!("{glyph} {} {} · {name}", t.id, t.title),
                    blockers: report.as_ref().map(|r| first(&r.blockers.0)).filter(|s| !s.is_empty()),
                    next: report.as_ref().map(|r| first(&r.next_action.0)).filter(|s| !s.is_empty()),
                    tokens: self.worker_tab(&t.id).and_then(|w| self.token_line_of(w)),
                }
            })
            .collect()
    }

    /// Makes a task and opens its worker (DESIGN.md §8.1). `place` is
    /// "repository [base]" (no base: the repository's current branch).
    fn start_task(&mut self, draft: &tasks::Draft, place: &str) {
        let place = place.trim();
        let (repo, base) = match place.rsplit_once(char::is_whitespace) {
            Some((r, b)) if !b.trim().is_empty() && !r.trim().is_empty() => {
                (r.trim().to_string(), b.trim().to_string())
            }
            _ => {
                let b = gitview::repo_branch(std::path::Path::new(place))
                    .and_then(|n| n.split_once(':').map(|x| x.1.to_string()))
                    .unwrap_or_default();
                (place.to_string(), b)
            }
        };
        let t = match tasks::create(&tasks::dir(), draft, &repo, &base) {
            Ok(t) => t,
            Err(e) => {
                self.show_hint(format!("task not made: {e}"));
                return;
            }
        };
        // Ids and counts only: titles and descriptions are typed text.
        log::info!("orchestrator: task {} made (worktree and branch created)", t.id);
        self.tasks = tasks::load_all(&tasks::dir());
        let Some((program, args)) = self.worker_command.split_first() else { return };
        let spec = TabSpec {
            program: program.clone(),
            args: tasks::worker_args(args, &t),
            cwd: Some(t.worktree.clone()),
            name: Some(t.title.clone()),
            custom: false,
            once: vec![t.prompt()],
        };
        match self.open_spec(spec) {
            Ok(()) => {
                if self.cfg.observe_workers && !self.tabs.is_empty() {
                    self.set_observable(self.tabs.len() - 1, true);
                }
                self.show_hint(format!("task {} started on {}", t.id, t.branch))
            }
            Err(e) => self.show_hint(format!("task {} made, worker not started: {e}", t.id)),
        }
    }

    /// Takes the active tab out of the window; the daemon keeps its session
    /// ("Attach detached sessions" shows it again). Not the only tab: the
    /// window would close.
    fn detach_tab(&mut self) {
        if self.tabs.len() < 2 {
            self.show_hint("the only tab cannot be detached: open another tab first".into());
            return;
        }
        let i = self.active;
        let t = self.tabs.remove(i);
        if let Err(e) = self.client.detach(&t.session.id) {
            log::warn!("detach session {}: {e}", t.session.id);
        }
        log::info!("tab {} detached: session {} keeps running in the daemon", i + 1, t.session.id);
        self.after_removal(i);
        let key = Action::AttachDetached.label();
        self.show_hint(format!("tab detached; \"{key}\" in the palette shows it again"));
    }

    /// The files of the open diff's list: the conversation's changed files (Claude Code tabs), else git's.
    fn diff_file_list(&self) -> Vec<diffview::DiffFile> {
        let tab = self.tab();
        let file = |path: String, added: u64, removed: u64| diffview::DiffFile {
            path,
            added: added as usize,
            removed: removed as usize,
        };
        match tab.claude.as_deref() {
            Some(c) => c.files.iter().map(|f| file(f.path.clone(), f.added, f.removed)).collect(),
            None => tab
                .git_sum
                .as_ref()
                .map(|g| g.files.iter().map(|(p, a, r)| file(self.full_path(p), *a, *r)).collect())
                .unwrap_or_default(),
        }
    }

    fn full_path(&self, path: &str) -> String {
        if std::path::Path::new(path).is_absolute() {
            return path.to_string();
        }
        let folder = self.tab().folder().unwrap_or_default();
        std::path::Path::new(&folder).join(path).to_string_lossy().into_owned()
    }

    /// Columns the diff overlay covers: the terminal area, beside the sidebar when it is shown.
    fn diff_cols(&self) -> usize {
        (self.grid.cols - self.panel_cols().max(self.overlay_cols())) as usize
    }

    fn diff_rows(&self) -> usize {
        self.grid.rows.saturating_sub(self.top() + self.status_rows()) as usize
    }

    fn diff_pref(&self) -> diffview::DiffLayoutPref {
        self.diff_pref.unwrap_or_else(|| self.cfg.diff_view.into())
    }

    /// The path the shown diff is of, for the sidebar's mark.
    pub(super) fn diff_shown(&self) -> Option<String> {
        let d = self.diff.as_ref()?;
        d.files().get(d.current_file()).map(|f| f.path.clone())
    }

    /// Opens the diff for a file of the sidebar: a fresh overlay on it, or the open one switched to it (same view).
    fn open_file_diff(&mut self, path: &str) {
        let (mode, old) = match self.diff.take() {
            Some(d) if d.mode == DiffMode::Tree => (DiffMode::Tree, Some(d)),
            Some(d) => (d.mode, None),
            None => {
                self.diff_pref = None;
                let last = self.tab().claude.as_deref().and_then(|c| c.last_edit.as_ref()).map(|m| m.path.clone());
                let edited = last.is_some_and(|l| same_file(&l, path));
                (if edited { DiffMode::Edit } else { DiffMode::Head }, None)
            }
        };
        self.open_diff(mode, old.as_ref(), Some(path));
        self.full = true;
    }

    /// An edit by Claude Code in the active tab: with `diff-after-edit` on, the overlay opens on that file, or the
    /// open one switches to it. Never over the palette or the settings panel, never while the tab awaits permission.
    fn after_edit(&mut self, path: &str) {
        let busy = self.palette.is_some() || self.settings_panel.is_some() || self.agent_view.is_some();
        let tab = self.tab();
        if !after_edit_allowed(self.cfg.diff_after_edit, tab.asking(), tab.attention(), busy) {
            return;
        }
        if self.diff.is_some() {
            self.open_file_diff(path);
        } else {
            self.diff_pref = None;
            self.open_diff(DiffMode::Edit, None, None);
        }
        self.full = true;
    }

    /// Opens the diff view `mode` for the active tab (`old`: the view it replaces, for a reread keeping its place;
    /// `file`: the file to show, else the last edit's).
    fn open_diff(&mut self, mode: DiffMode, old: Option<&diffview::DiffView>, file: Option<&str>) {
        let id = self.tab().session.id.clone();
        let edit = if mode == DiffMode::Head && file.is_some() {
            None
        } else {
            match self.client.get_edit(&id) {
                Ok(e) => e,
                Err(e) => {
                    self.show_hint(format!("diff: {e}"));
                    return;
                }
            }
        };
        let mode = match (mode, file, edit.as_deref()) {
            (DiffMode::Edit, Some(f), Some(e)) if !same_file(&e.meta.path, f) => DiffMode::Head,
            (DiffMode::Edit, Some(_), None) => DiffMode::Head,
            _ => mode,
        };
        let limits = gitview::Limits::default();
        let mut v = match mode {
            DiffMode::Edit => diffview::DiffView::new(edit.as_deref()),
            DiffMode::Head => {
                match file.map(|f| self.full_path(f)).or_else(|| edit.as_deref().map(|e| e.meta.path.clone())) {
                    Some(p) => diffview::DiffView::git(mode, gitview::head(&p, &limits)),
                    None => diffview::DiffView::note_only(
                        mode,
                        "[vs HEAD] no edit yet".into(),
                        "Claude Code has not edited a file in this tab yet: no file to compare (m: the working tree)."
                            .into(),
                    ),
                }
            }
            DiffMode::Tree => {
                // The repository of the last edit, else of the tab's folder.
                let dir = edit
                    .as_deref()
                    .and_then(|e| std::path::Path::new(&e.meta.path).parent().map(|p| p.to_string_lossy().into_owned()))
                    .or_else(|| self.tab().folder());
                match dir {
                    Some(d) => diffview::DiffView::git(mode, gitview::tree(&d, &limits)),
                    None => diffview::DiffView::note_only(mode, "[working tree]".into(), "No folder known.".into()),
                }
            }
        };
        let (cols, area) = (self.diff_cols(), self.diff_rows());
        if let Some(t) = v.collect_time() {
            log::info!("diff view {}: {:?} to collect, {} rows", mode.label(), t, v.row_count(cols));
        }
        if mode != DiffMode::Tree {
            let (files, at) = merged_list(v.files(), self.diff_file_list());
            v.set_files(files, at);
        }
        if v.root().is_empty() {
            v.set_root(&self.tab().folder().unwrap_or_default());
        }
        let layout = v.layout(cols, self.diff_pref(), self.cfg.diff_split_cols);
        if let Some(o) = old {
            v.keep_scroll(o, area, layout);
        }
        if mode == DiffMode::Tree {
            if let Some(i) = file.and_then(|f| v.files().iter().position(|x| same_file(&x.path, f))) {
                v.show_file(i, layout, area);
            }
        }
        self.diff = Some(v);
    }

    /// The closed tabs, for the palette: (name, folder · program · resume).
    fn closed_items(list: &[SavedTab]) -> Vec<(String, String)> {
        let home = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).ok();
        list.iter()
            .map(|t| {
                let cwd = match &home {
                    Some(h) if !h.is_empty() && t.cwd.starts_with(h.as_str()) => format!("~{}", &t.cwd[h.len()..]),
                    _ => t.cwd.clone(),
                };
                let prog = std::path::Path::new(&t.program)
                    .file_name()
                    .map_or(t.program.clone(), |n| n.to_string_lossy().into_owned());
                let resume = if t.claude_session.is_empty() { "" } else { " · resume" };
                (t.name.clone(), format!("{cwd} · {prog}{resume}"))
            })
            .collect()
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

    /// Runs an action; `origin` is what any input it sends to the program
    /// carries: `Human` for a shortcut pressed by the person, `Program` from
    /// the palette.
    fn run(&mut self, a: Action, origin: Origin) {
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
                    if self.tab().exited.is_some() || self.sidebar_keys() {
                    } else if origin == Origin::Program && self.tab().asking() {
                        // The daemon would refuse it anyway (§3.7); say why.
                        self.show_hint("not sent: a permission dialog is open or was just answered — type it".into());
                    } else {
                        self.handle().paste(origin, text);
                    }
                }
            }
            Action::DiffView => {
                if self.diff.take().is_none() {
                    if !self.tab().integrated && self.tab().claude.is_none() {
                        self.show_hint("the diff view is for Claude Code tabs".into());
                    } else {
                        self.diff_pref = None;
                        self.open_diff(DiffMode::Edit, None, None);
                    }
                }
                self.full = true;
                self.recompose();
            }
            Action::RenameTab => {
                let now = self.tab().name();
                self.palette = Some(Palette::rename(now, self.tab().custom_name.clone()));
                self.recompose();
            }
            Action::ReopenClosedTab => {
                self.closed = tabsets::load(&tabsets::dir(), tabsets::CLOSED).unwrap_or_default();
                self.palette = Some(Palette::closed(Self::closed_items(&self.closed)));
                self.recompose();
            }
            Action::DetachTab => self.detach_tab(),
            Action::ToggleObserve => {
                let on = !self.tab().observable;
                self.set_observable(self.active, on);
                self.show_hint(if self.tab().observable == on && on {
                    "observing allowed: haetty screen and haetty events can read this tab (the palette again stops it)"
                        .into()
                } else if self.tab().observable == on {
                    "observing stopped for this tab".into()
                } else {
                    "observing could not be changed (see the log)".into()
                });
                self.recompose();
            }
            Action::NewTask => {
                // Prefilled: the active tab's repository and its branch.
                let prefill = self
                    .tab()
                    .folder()
                    .and_then(|f| gitview::find_top(std::path::Path::new(&f)))
                    .map(|(top, _)| {
                        let b = gitview::repo_branch(&top).and_then(|n| n.split_once(':').map(|x| x.1.to_string()));
                        format!("{} {}", top.display(), b.unwrap_or_default())
                    })
                    .unwrap_or_default();
                self.palette = Some(Palette::new_task(prefill));
                self.recompose();
            }
            Action::MessageWorker => {
                if self.tab().task.is_none() {
                    self.show_hint("not a worker tab (palette: New task)".into());
                } else if let Err(h) = orch::ready(self.tab().claude.as_deref()) {
                    self.show_hint(format!("not now: {}", h.reason()));
                } else {
                    self.palette = Some(Palette::message());
                    self.recompose();
                }
            }
            Action::RemoveTaskWorktree => {
                self.tasks = tasks::load_all(&tasks::dir());
                let items = self
                    .tasks
                    .iter()
                    .map(|t| (format!("{} {}", t.id, t.title), format!("{} · {}", t.branch, self.task_state(t).name())))
                    .collect();
                self.palette = Some(Palette::tasks(items));
                self.recompose();
            }
            Action::StatusPanel => {
                self.sidebar_focus = false;
                if self.sidebar_wide() {
                    // The person's choice: the terminal resizes once (DESIGN.md §13.3).
                    self.sidebar_shown = !self.sidebar_shown;
                    self.save_ui();
                    self.relayout();
                } else {
                    // Narrow (or `sidebar = hide`): over the terminal, which keeps its size.
                    self.sidebar_overlay = !self.sidebar_overlay;
                    self.sidebar_sel = self.sidebar_overlay.then_some(0);
                    self.full = true;
                    self.recompose();
                }
            }
            Action::StatusPanelWider | Action::StatusPanelNarrower => {
                let d: i32 = if a == Action::StatusPanelWider { 2 } else { -2 };
                self.panel_width = (self.panel_width as i32 + d).clamp(24, 80) as u16;
                self.save_ui();
                self.relayout();
            }
            Action::ReloadConfig => {
                let changed = self.reload_config(true);
                self.show_hint(if changed {
                    "config reloaded: applied".into()
                } else {
                    "config reloaded: no change".into()
                });
            }
            Action::Settings => self.toggle_settings(),
            Action::Updates => self.open_updates(),
            Action::AllAgents => self.open_all_agents(),
            Action::FocusSidebar => self.focus_sidebar(),
            Action::AttachDetached => {
                let n = self.attach_detached();
                self.show_hint(if n == 0 { "no detached sessions".into() } else { format!("attached {n} session(s)") });
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
        let p = haetty_proto::product::title_prefix();
        let title = if t.is_empty() { haetty_proto::product::display_name() } else { format!("{p}{t}") };
        self.window.set_title(&title);
    }

    /// The window title (also shown in the integrated title row with one tab).
    fn title_text(&self) -> String {
        let t = self.tab().snap.as_ref().map_or("", |s| s.title.as_str());
        if t.is_empty() {
            haetty_proto::product::display_name()
        } else {
            format!("{}{t}", haetty_proto::product::title_prefix())
        }
    }

    /// The title row's layout (no buttons unless integrated).
    fn title_layout(&self) -> titlebar::TitleLayout {
        titlebar::layout(self.grid.cols as usize, &self.buttons)
    }

    /// Pointer over the title row: (column, true) — `None` elsewhere.
    fn title_col(&self) -> Option<usize> {
        if self.top() == 0 {
            return None;
        }
        let (col, row) = self.layout.cell_at(self.origin(), self.mouse.x, self.mouse.y, self.grid);
        // Above the grid (the top padding) counts as the title row too.
        let above = self.mouse.y < self.origin().1 as f64;
        (row == 0 || above).then_some(col as usize)
    }

    /// Pointer moved: button hover and (integrated) the resize cursor.
    fn title_hover(&mut self) {
        if !self.integrated {
            return;
        }
        let edge = if self.window.is_maximized() {
            None
        } else {
            let border = (6.0 * self.window.scale_factor()).round().max(2.0);
            titlebar::edge_at(self.mouse.x, self.mouse.y, self.size.width, self.size.height, border)
        };
        if edge != self.edge {
            self.edge = edge;
            self.window.set_cursor(edge.map_or(winit::window::CursorIcon::Default, |d| d.into()));
        }
        let hover = match self.title_col().filter(|_| edge.is_none()) {
            Some(c) => match titlebar::hit(&self.title_layout(), c, self.tabs.len()) {
                titlebar::Hit::Button(b) => Some(b),
                _ => None,
            },
            None => None,
        };
        if hover != self.hover {
            self.hover = hover;
            self.recompose();
        }
    }

    /// Left press on the window: an edge starts a resize, the title row
    /// selects a tab, arms a button or moves / maximizes the window. True
    /// when consumed (not sent to the program).
    fn title_press(&mut self) -> bool {
        if self.integrated {
            if let Some(d) = self.edge {
                log::info!("title bar: drag_resize_window {d:?}");
                let _ = self.window.drag_resize_window(d);
                return true;
            }
        }
        let Some(col) = self.title_col() else { return false };
        match titlebar::hit(&self.title_layout(), col, self.tabs.len()) {
            titlebar::Hit::Tab(i) => self.select(i),
            titlebar::Hit::Button(b) => self.pressed_button = Some(b),
            titlebar::Hit::Empty if self.integrated => {
                let now = Instant::now();
                let near = |p: PhysicalPosition<f64>| {
                    let d = 4.0 * self.window.scale_factor();
                    (p.x - self.mouse.x).abs() <= d && (p.y - self.mouse.y).abs() <= d
                };
                if self.last_title_press.take().is_some_and(|(t, p)| now - t < Duration::from_millis(400) && near(p)) {
                    let m = !self.window.is_maximized();
                    log::info!("title bar: double click -> maximized {m}");
                    self.window.set_maximized(m);
                } else {
                    self.last_title_press = Some((now, self.mouse));
                    log::info!("title bar: drag_window");
                    let _ = self.window.drag_window();
                }
            }
            titlebar::Hit::Empty => {}
        }
        true
    }

    /// Left release: a button acts when the pointer is still on it. Returns
    /// true for "close the window".
    fn title_release(&mut self) -> bool {
        let Some(b) = self.pressed_button.take() else { return false };
        let still = self.title_col().map(|c| titlebar::hit(&self.title_layout(), c, self.tabs.len()));
        if still != Some(titlebar::Hit::Button(b)) {
            return false;
        }
        match b {
            titlebar::Button::Minimize => {
                log::info!("title bar: minimize");
                self.window.set_minimized(true);
            }
            titlebar::Button::Maximize => {
                let m = !self.window.is_maximized();
                log::info!("title bar: maximize button -> maximized {m}");
                self.window.set_maximized(m);
            }
            titlebar::Button::Close => {
                log::info!("title bar: close");
                return true;
            }
        }
        false
    }

    /// Sizes of the GUI's own heap structures, for the log (`HAETTY_LOG=info`).
    fn log_memory(&self) {
        let empty = vec![];
        let all = self.composed.as_ref().map_or(&empty, |s| &s.cells);
        let cells = all.len();
        let text: usize = all.iter().map(|c| c.text.capacity()).sum();
        let snap_kb = (cells * std::mem::size_of::<haetty_core::Cell>() + text) / 1024;
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
        if self.tabs[i].refresh_git() && i != self.active {
            self.recompose(); // tab bar: name
        }
        let claude = self.tabs[i].session.handle.claude();
        let claude_changed = match (&claude, &self.tabs[i].claude) {
            (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
            (a, b) => a.is_some() != b.is_some(),
        };
        if claude_changed {
            let (before, before_sid) = (
                self.tabs[i].attention(),
                self.tabs[i].claude.as_ref().map(|c| c.claude_session.clone()).unwrap_or_default(),
            );
            let arrived = edit_arrived(self.tabs[i].claude.as_deref(), claude.as_deref());
            self.tabs[i].claude = claude;
            self.tabs[i].log_at = None;
            self.attention_changed(i, before);
            if let Some(path) = arrived.filter(|_| i == self.active) {
                self.after_edit(&path);
            }
            let sid_changed = self.tabs[i].claude.as_ref().is_some_and(|c| c.claude_session != before_sid);
            if i != self.active {
                self.recompose(); // tab bar: mark, name
            }
            if sid_changed {
                self.save_last(); // the conversation to resume
            }
        }
        if i == self.active {
            if title_changed {
                self.update_title();
            }
            // The text under a still pointer may have changed.
            self.update_link_hover();
            self.recompose();
        } else if title_changed && self.tabs.len() >= 2 {
            self.recompose(); // the tab bar shows its name
        }
        if title_changed {
            self.save_last();
        }
    }

    /// Claude Code's attention of tab `i` changed from `before`.
    fn attention_changed(&mut self, i: usize, before: Attention) {
        let now = self.tabs[i].attention();
        if now == Attention::None {
            self.tabs[i].waiting_since = None;
        } else if self.tabs[i].waiting_since.is_none() {
            self.tabs[i].waiting_since = Some(Instant::now());
        }
        if now != before {
            log::debug!(
                "tab {} attention {before:?} -> {now:?} (active {}, focused {})",
                i + 1,
                self.active + 1,
                self.focused
            );
        }
        if now == before || now == Attention::None {
            return;
        }
        if i == self.active && self.focused {
            self.seen(); // the user is looking at it
            if now == Attention::Wait {
                return;
            }
        }
        if !self.focused {
            let kind = if now == Attention::Ask {
                winit::window::UserAttentionType::Critical
            } else {
                winit::window::UserAttentionType::Informational
            };
            self.window.request_user_attention(Some(kind));
            if self.notifications {
                let body = if now == Attention::Ask {
                    "Claude Code asks you something"
                } else {
                    "Claude Code is waiting for you"
                };
                notify(&self.tabs[i].name(), body);
            }
        }
    }

    /// The active tab was looked at: a "waiting" mark goes.
    fn seen(&self) {
        let t = self.tab();
        if t.attention() == Attention::Wait {
            let _ = self.client.clear_attention(&t.session.id);
        }
    }

    /// Rebuilds the window grid from the active tab and haetty's cells.
    fn recompose(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        let (pref, split) = (self.diff_pref(), self.cfg.diff_split_cols);
        if let Some(d) = self.diff.as_mut() {
            d.set_layout(pref, split);
        }
        let (fg0, bg0) = self
            .tab()
            .snap
            .as_ref()
            .map_or((self.theme.foreground, self.theme.background), |s| (s.default_fg, s.default_bg));
        let settings = self.draw_settings(ui::Greys::new(fg0, bg0, &self.theme.palette));
        let gear_at = self.gear_place();
        let (fg, bg) = (self.theme.foreground, self.theme.background);
        let empty = Snapshot { default_fg: fg, default_bg: bg, ..Default::default() };
        let screen = self.tab().snap.as_ref().unwrap_or(&empty);
        let g = ui::Greys::new(screen.default_fg, screen.default_bg, &self.theme.palette);
        let names: Vec<String> = self.tabs.iter().map(Tab::name).collect();
        let bar = (self.top() > 0).then(|| {
            let views: Vec<TabView> = names
                .iter()
                .zip(&self.tabs)
                .map(|(n, t)| TabView {
                    name: n,
                    exited: t.exited.is_some(),
                    attention: t.attention(),
                    observed: t.observable,
                    clear: self.clear_mark(t),
                })
                .collect();
            if self.integrated {
                let title = self.title_text();
                let max = self.window.is_maximized();
                titlebar::row(&views, self.active, self.grid.cols as usize, g, &title, &self.buttons, self.hover, max)
            } else {
                ui::tab_bar(&views, self.active, self.grid.cols as usize, g)
            }
        });
        let tab = self.tab();
        let diff_key = self.keys.chord_for(Action::DiffView).map(|c| c.to_string()).unwrap_or_default();
        let docked = self.panel_cols() as usize;
        let status = (tab.integrated && docked == 0).then(|| {
            let gear = gear_at.is_some();
            ui::status_row_gear(tab.claude.as_deref(), self.grid.cols as usize, g, &diff_key, self.saving_mode, gear)
        });
        let pc = docked.max(self.overlay_cols() as usize);
        let area = (self.grid.rows as usize).saturating_sub(self.top() as usize + usize::from(status.is_some()));
        if (self.token_status || self.clear_hint > 0) && pc > 0 {
            self.tokens.follow(self.watched_transcripts());
        }
        let mut new_hits = vec![];
        let panel = (pc > 0).then(|| {
            let (rows, hits) = self.sidebar_rows(pc, area, g);
            new_hits = hits;
            rows
        });
        let agent_rows = self.agent_view.as_ref().map(|v| {
            let w = (self.grid.cols as usize).saturating_sub(docked);
            (v.draw(w, area, g, now_ms()), None)
        });
        let settings = settings.or(agent_rows);
        let next = Compose {
            screen,
            accents: &self.theme.palette,
            cols: self.grid.cols,
            rows: self.grid.rows,
            tab_bar: bar,
            status,
            panel,
            diff: self.diff.as_ref(),
            palette: self.palette.as_ref().map(|p| (p, &self.keys)),
            hint: self.hint.as_ref().map(|(h, _)| h.as_str()),
            link: self.link_hover.as_deref().map(|l| (l, self.link_row(), self.link_note.as_deref())),
            settings,
            // A newer version is out: the button in the text colour instead of dim (DESIGN.md §2.4; no accent).
            gear: gear_at.map(|(row, col)| ui::Gear {
                row,
                col,
                text: self.gear,
                hover: self.gear_hover || self.update_pending(),
            }),
        }
        .build();
        self.sidebar_hits = new_hits;
        if self.composed.as_ref().is_some_and(|s| (s.cols, s.rows) != (next.cols, next.rows)) {
            self.full = true;
        }
        let d = damage::dirty_rows(self.composed.as_ref(), &next);
        damage::merge(&mut self.dirty, &d);
        self.chrome.update(&self.window, next.default_bg, next.default_fg);
        self.composed = Some(next);
        self.update_ime_area();
        self.redraw();
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

    /// The hyperlink of the screen cell under the pointer: none over haetty's
    /// own rows and columns, the palette or the diff view.
    fn hovered_link(&self) -> Option<Arc<str>> {
        if self.palette.is_some() || self.diff.is_some() || self.settings_panel.is_some() {
            return None;
        }
        let snap = self.tab().snap.as_ref()?;
        let (col, row) = self.layout.cell_at(self.origin(), self.mouse.x, self.mouse.y, self.grid);
        let row = row.checked_sub(self.top())?;
        if col >= self.grid.cols.saturating_sub(self.panel_cols().max(self.overlay_cols())) || self.agent_view.is_some()
        {
            return None;
        }
        snap.cell(col, row)?.link.clone()
    }

    /// The window row that shows the hovered link: the bottom one, or the
    /// first screen row while the pointer is on the bottom one.
    fn link_row(&self) -> usize {
        let bottom = (self.grid.rows as usize).saturating_sub(1);
        let (_, row) = self.layout.cell_at(self.origin(), self.mouse.x, self.mouse.y, self.grid);
        if row as usize >= bottom {
            self.top() as usize
        } else {
            bottom
        }
    }

    /// Pointer moved or the screen changed: the link under the pointer.
    fn update_link_hover(&mut self) {
        let now = self.hovered_link();
        if now != self.link_hover {
            self.link_hover = now;
            self.link_note = None;
            // The underline is on cells the composed grid does not mark.
            self.full = true;
            self.recompose();
        }
    }

    /// Ctrl+left press: on a link that is shown, open it (allowed schemes) or
    /// say why not. True when the press was taken.
    fn link_click(&mut self) -> bool {
        let Some(l) = self.hovered_link() else { return false };
        if self.link_hover.as_ref() != Some(&l) {
            // Not shown yet: show it, do not open it.
            self.link_hover = Some(l);
            self.link_note = None;
            self.full = true;
            self.recompose();
            return true;
        }
        match links::check(&l) {
            Ok(()) => match links::open(&l) {
                Ok(()) => log::info!("link: opened ({}:)", links::scheme(&l).unwrap_or_default()),
                Err(e) => {
                    log::warn!("link: could not open ({}:): {e}", links::scheme(&l).unwrap_or_default());
                    self.link_note = Some("could not open it".into());
                }
            },
            Err(why) => self.link_note = Some(why),
        }
        self.recompose();
        true
    }

    fn mouse_tracking(&self) -> bool {
        self.tab().snap.as_ref().is_some_and(|s| s.modes.mouse_tracking)
            && self.palette.is_none()
            && self.settings_panel.is_none()
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
        self.handle().mouse(Origin::Human, m);
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
        if ev.state == ElementState::Pressed && !self.tab().integrated {
            // A key hides the button over a program's cell (DESIGN.md §2.4).
            self.gear_away(false);
        }
        if ev.state == ElementState::Released {
            if code.is_some_and(|c| self.swallowed.remove(&c)) || self.settings_panel.is_some() {
                return;
            }
        } else if let Some(chord) = chord_of(ev, self.mods) {
            if self.settings_panel.is_some() {
                self.settings_key(ev, &chord);
                if let Some(c) = code {
                    self.swallowed.insert(c);
                }
                return;
            }
            if self.palette.is_some() {
                self.palette_key(ev, &chord);
                if let Some(c) = code {
                    self.swallowed.insert(c);
                }
                return;
            }
            let action = self.keys.action(&chord);
            if action.is_none() && (self.agent_view.is_some() || self.sidebar_keys()) {
                let taken = if self.agent_view.is_some() { self.agent_key(&chord) } else { self.sidebar_key(&chord) };
                if taken {
                    if let Some(c) = code {
                        self.swallowed.insert(c);
                    }
                    return;
                }
            }
            if let Some(a) = action {
                if let Some(c) = code {
                    self.swallowed.insert(c);
                }
                self.run(a, Origin::Human);
                return;
            }
        } else if self.settings_panel.is_some() {
            // As for the palette below: never past the panel into the program.
            if let Some(t) = input::palette_text(ev.text.as_deref()) {
                self.settings_text(t);
            }
            return;
        } else if self.palette.is_some() {
            // A key with text but no key name (Windows VK_PACKET: SendInput
            // Unicode, on-screen keyboards, AutoHotkey) goes into the palette
            // like an IME commit — never past it into the program (it did:
            // Hangul typed into "Rename tab" reached the shell, V6).
            if let Some(t) = input::palette_text(ev.text.as_deref()) {
                if let Some(p) = self.palette.as_mut() {
                    p.query.push_str(t);
                    p.selected = 0;
                }
                self.recompose();
            }
            return;
        }
        if self.sidebar_keys() {
            return;
        }
        if self.diff.is_some() {
            if ev.state == ElementState::Pressed {
                if let Some(chord) = chord_of(ev, self.mods) {
                    let a = DiffArea {
                        height: self.diff_rows(),
                        cols: self.diff_cols(),
                        setting: self.cfg.diff_view.into(),
                        split_cols: self.cfg.diff_split_cols,
                    };
                    let mut pref = self.diff_pref;
                    let key = self.diff.as_mut().map(|d| diff_key(d, &chord.key, chord.shift, a, &mut pref));
                    self.diff_pref = pref;
                    match key {
                        Some(diffview::Key::Close) => {
                            self.diff = None;
                            self.full = true;
                        }
                        Some(diffview::Key::Next) => {
                            let mode = self.diff.as_ref().map_or(DiffMode::Edit, |d| d.mode.next());
                            self.open_diff(mode, None, None);
                            self.full = true;
                        }
                        Some(diffview::Key::Refresh) => {
                            if let Some(old) = self.diff.take() {
                                let file = (old.mode == DiffMode::Head)
                                    .then(|| old.files().get(old.current_file()).map(|f| f.path.clone()))
                                    .flatten();
                                self.open_diff(old.mode, Some(&old), file.as_deref());
                            }
                            self.full = true;
                        }
                        Some(k @ (diffview::Key::NextFile | diffview::Key::PrevFile)) => {
                            let path = self.diff.as_ref().and_then(|d| {
                                let at = step_index(d.current_file(), d.file_count(), k == diffview::Key::NextFile)?;
                                d.files().get(at).map(|f| f.path.clone())
                            });
                            if let Some(p) = path {
                                self.open_file_diff(&p);
                            }
                            self.full = true;
                        }
                        _ => {}
                    }
                    if let Some(c) = code {
                        self.swallowed.insert(c);
                    }
                    self.recompose();
                }
            }
            return;
        }
        if self.palette.is_some() || self.tab().exited.is_some() || self.agent_view.is_some() {
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
        self.handle().key(Origin::Human, k);
    }

    /// Keys while the palette is open: type to filter, arrows, Enter, Esc.
    fn palette_key(&mut self, ev: &KeyEvent, chord: &Chord) {
        let Some(p) = self.palette.as_mut() else { return };
        let plain = !chord.ctrl && !chord.alt && !chord.sup;
        match chord.key.as_str() {
            "escape" => self.palette = None,
            "enter" if p.task_step() => {}
            "enter" => {
                let pick = p.current();
                self.palette = None;
                match pick {
                    Some(Pick::Agent(i)) => {
                        if let Some(&t) = self.agent_pick.get(i) {
                            if t < self.tabs.len() {
                                self.select(t);
                            }
                        }
                        self.recompose();
                    }
                    Some(Pick::Action(a)) => {
                        self.recompose();
                        // Chosen in haetty's own UI: what it sends is made up by
                        // haetty, not typed into the program (DESIGN.md §3.7).
                        self.run(a, Origin::Program);
                        return;
                    }
                    Some(Pick::Rename(name)) => {
                        // haetty's own label: nothing reaches the program.
                        let clean: String = name.chars().filter(|c| !c.is_control()).collect();
                        let clean: String = clean.trim().chars().take(64).collect();
                        let auto = clean.is_empty();
                        let own = (!auto).then_some(clean);
                        crate::tabnames::set(&self.tabs[self.active].session.id, own.as_deref());
                        self.tabs[self.active].custom_name = own;
                        log::info!(
                            "tab {} renamed ({})",
                            self.active + 1,
                            if auto { "automatic name" } else { "own name" }
                        );
                        self.save_last();
                        self.update_title();
                        self.show_hint(if auto { "automatic tab name".into() } else { "tab renamed".into() });
                        return;
                    }
                    Some(Pick::NewTask(draft, place)) => {
                        self.recompose();
                        self.start_task(&draft, &place);
                        return;
                    }
                    Some(Pick::Message(text)) => {
                        let id = self.tab().task.as_ref().map(|(id, _)| id.clone()).unwrap_or_default();
                        let t = self.tab();
                        // Always as a program (orch::send has no origin to choose).
                        let r = orch::send(&t.session.handle, t.claude.as_deref(), &text);
                        match r {
                            Ok(()) => {
                                log::info!(
                                    "orchestrator: message to worker {id} sent ({} chars)",
                                    text.chars().count()
                                );
                                self.show_hint(format!("sent to worker {id}"));
                            }
                            Err(h) => {
                                log::info!("orchestrator: message to worker {id} held: {h:?}");
                                self.show_hint(format!("not sent: {}", h.reason()));
                            }
                        }
                        return;
                    }
                    Some(Pick::Task(i)) => {
                        if let Some(t) = self.tasks.get(i).cloned() {
                            let open = self
                                .tabs
                                .iter()
                                .any(|x| x.exited.is_none() && x.task.as_ref().is_some_and(|(id, _)| *id == t.id));
                            if open {
                                self.show_hint(format!("close worker {} first", t.id));
                                return;
                            }
                            match tasks::remove(&tasks::dir(), &t) {
                                Ok(()) => {
                                    log::info!("orchestrator: worktree of task {} removed", t.id);
                                    self.tasks = tasks::load_all(&tasks::dir());
                                    self.show_hint(format!("worktree of {} removed; branch {} kept", t.id, t.branch));
                                }
                                Err(e) => self.show_hint(format!("not removed: {e}")),
                            }
                            return;
                        }
                    }
                    Some(Pick::Closed(i)) => {
                        if let Some(t) = self.closed.get(i).cloned() {
                            let n = self.open_saved(std::slice::from_ref(&t));
                            if n == 1 {
                                if let Err(e) = tabsets::remove_closed(&tabsets::dir(), &t) {
                                    log::warn!("tab set closed: {e}");
                                }
                            } else {
                                self.show_hint("not reopened: its conversation is open or its folder is gone".into());
                                return;
                            }
                        }
                    }
                    None => {}
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
                if self.settings_panel.is_some() {
                    self.settings_text(&text);
                } else if let Some(p) = self.palette.as_mut() {
                    p.query.push_str(&text);
                    p.selected = 0;
                    self.recompose();
                } else if self.diff.is_some() || self.sidebar_keys() {
                    // Not for the program while the diff view or the sidebar has the keyboard.
                } else if !text.is_empty() && self.tab().exited.is_none() {
                    self.handle().key(Origin::Human, KeyInput::press(Key::Unidentified).with_text(text));
                }
            }
            Ime::Disabled => self.preedit = None,
        }
        self.mark_cursor_row();
        self.redraw();
    }

    fn mark_cursor_row(&mut self) {
        if let Some(c) = self.composed.as_ref().and_then(|s| s.cursor) {
            if let Some(d) = self.dirty.get_mut(c.y as usize) {
                *d = true;
            }
        }
    }

    /// Asks for a frame (see `asked_redraw`).
    fn redraw(&mut self) {
        self.asked_redraw = true;
        self.window.request_redraw();
    }

    fn draw(&mut self) {
        if !std::mem::take(&mut self.asked_redraw) {
            self.full = true;
        }
        let Some(snap) = self.composed.as_ref() else { return };
        if self.dirty.len() != snap.rows as usize {
            self.dirty = vec![true; snap.rows as usize];
        }
        let scene = Scene {
            snap,
            dirty: &self.dirty,
            full: self.full,
            focused: self.focused,
            preedit: self.preedit.as_ref(),
            hover_link: self.link_hover.as_deref(),
        };
        let t0 = Instant::now();
        if let Err(e) = self.renderer.render(&scene) {
            log::error!("render: {e}");
        }
        let took = t0.elapsed();
        log::trace!("frame drawn in {} us (full {})", took.as_micros(), self.full);
        // Friction log: a frame over the 16 ms budget, at most every 10 s.
        if took > Duration::from_millis(16)
            && self.last_slow_draw.is_none_or(|t| t.elapsed() >= Duration::from_secs(10))
        {
            self.last_slow_draw = Some(Instant::now());
            self.client.reporter().friction(
                "slow_draw",
                &[
                    ("ms", format!("{:.1}", took.as_secs_f64() * 1000.0)),
                    ("full", self.full.to_string()),
                    ("grid", format!("{}x{}", self.grid.cols, self.grid.rows)),
                ],
            );
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
            self.relayout();
        }
        self.redraw();
    }

    /// Expires hints and close requests; returns the next deadline.
    fn tick(&mut self) -> Option<Instant> {
        let now = Instant::now();
        if self.pending_start.is_some() {
            match self.pending_since {
                Some(t) if now.duration_since(t) >= SCALE_WAIT => self.start_pending(),
                Some(t) => return Some(t + SCALE_WAIT),
                None => return None,
            }
        }
        if self.pending_close.as_ref().is_some_and(|(_, t)| now.duration_since(*t) >= CONFIRM) {
            self.pending_close = None;
        }
        if self.hint.as_ref().is_some_and(|(_, t)| now.duration_since(*t) >= CONFIRM) && !self.daemon_lost {
            self.hint = None;
            self.recompose();
        }
        let relayout = self.settle_relayout(now);
        let update = self.update_tick(now);
        let sidebar = if self.daemon_lost { None } else { self.sidebar_tick(now) };
        let hint = self.hint.as_ref().filter(|_| !self.daemon_lost).map(|(_, t)| *t + CONFIRM);
        [hint, relayout, update, sidebar].into_iter().flatten().min()
    }
}

/// A GUI panic is reported to the daemon's event log (location and our own
/// message), then handled as before.
fn install_panic_report(r: haetty_proto::Reporter) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let what = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        r.friction("panic", &[("what", what), ("at", at)]);
        previous(info);
    }));
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// A desktop notification without content from the session (Linux:
/// `notify-send` when installed; elsewhere the taskbar hint is all).
fn notify(tab: &str, body: &str) {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let r = std::process::Command::new("notify-send")
            .args(["--app-name", &haetty_proto::product::display_name(), "--expire-time", "8000"])
            .arg(format!("{} — {tab}", haetty_proto::product::display_name()))
            .arg(body)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match r {
            Ok(mut c) => {
                // Reap it without blocking the event loop.
                std::thread::spawn(move || c.wait());
            }
            Err(e) => log::debug!("notify-send: {e}"),
        }
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let _ = (tab, body);
}

/// The sidebar's window state (`ui` in the state directory, `key = value`; stage PH). The status panel's old keys
/// (`status-panel`, `status-panel-width`) are not read: the sidebar shows by default.
pub(crate) struct UiState {
    pub shown: bool,
    pub width: Option<u16>,
    pub folded: Vec<crate::sidebar::Section>,
}

pub(crate) fn load_ui_state() -> UiState {
    let text = std::fs::read_to_string(haetty_proto::paths::state_dir().join("ui")).unwrap_or_default();
    let mut st = UiState { shown: true, width: None, folded: vec![] };
    for line in text.lines() {
        match line.split_once('=').map(|(k, v)| (k.trim(), v.trim())) {
            Some(("sidebar", v)) => st.shown = v != "hidden",
            Some(("sidebar-width", v)) => st.width = v.parse().ok().map(|w: u16| w.clamp(24, 80)),
            Some(("sidebar-folded", v)) => {
                st.folded = v.split(',').filter_map(crate::sidebar::Section::parse).collect()
            }
            _ => {}
        }
    }
    st
}

fn save_ui_state(st: &UiState) {
    let dir = haetty_proto::paths::state_dir();
    let _ = std::fs::create_dir_all(&dir);
    let folded: Vec<&str> = st.folded.iter().map(|s| s.name()).collect();
    let mut text = format!(
        "# haetty window state\nsidebar = {}\nsidebar-folded = {}\n",
        if st.shown { "shown" } else { "hidden" },
        folded.join(",")
    );
    if let Some(w) = st.width {
        text.push_str(&format!("sidebar-width = {w}\n"));
    }
    if let Err(e) = std::fs::write(dir.join("ui"), text) {
        log::warn!("ui state: {e}");
    }
}

/// A directory from OSC 7 (`file://host/path`) or a plain path.
pub(crate) fn osc7_path(p: &str) -> String {
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
        let (sid, ev) = match ev {
            UserEvent::Session(sid, ev) => (sid, ev),
            UserEvent::Update(u) => {
                win.updater.handle(u);
                win.recompose();
                return;
            }
            UserEvent::Sidebar(e) => {
                win.sidebar_event(e);
                return;
            }
        };
        let Some(i) = win.tabs.iter().position(|t| t.session.id == sid) else { return };
        match ev {
            SessionEvent::Wakeup => win.refresh(&sid),
            SessionEvent::Title(_) => {}
            SessionEvent::Exited(status) => {
                log::info!("tab {} ({sid}) exited: {status:?}", i + 1);
                if win.client.is_lost() {
                    // Not the program's own end: the daemon is gone. Keep
                    // the window and the tab sets; say what happened.
                    if !win.daemon_lost {
                        log::error!(
                            "the daemon (pid {}) is gone; tab sets kept for the next start",
                            win.client.daemon_pid
                        );
                        win.save_last(); // the last state while tabs still count as running
                        win.daemon_lost = true;
                    }
                    win.tabs[i].exited = Some(None);
                    win.hint = Some((
                        "haetty's daemon stopped and its programs ended — start haetty again to reopen these tabs"
                            .into(),
                        Instant::now(),
                    ));
                    win.recompose();
                    return;
                }
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
        if win.pending_start.is_some() && !matches!(event, WindowEvent::ScaleFactorChanged { .. }) {
            match event {
                WindowEvent::CloseRequested => win.quit = Some(0),
                WindowEvent::RedrawRequested => {
                    win.draw();
                    win.pending_since.get_or_insert_with(Instant::now);
                }
                WindowEvent::Resized(size) => win.pending_resized(size),
                _ => {}
            }
            return;
        }
        match event {
            WindowEvent::CloseRequested => close_window(win, el),
            WindowEvent::CursorLeft { .. } => {
                if win.hover.take().is_some() {
                    win.recompose();
                }
                win.gear_away(true);
            }
            WindowEvent::RedrawRequested => win.draw(),
            WindowEvent::Resized(size) => win.resized(size),
            WindowEvent::ScaleFactorChanged { scale_factor, mut inner_size_writer } => {
                let px = (win.cfg.font_size * 96.0 / 72.0 * scale_factor) as f32;
                win.renderer.set_font_px(px);
                let pad = padding_style(&win.cfg, scale_factor);
                win.renderer.set_padding(pad);
                win.layout = Layout { cell: win.renderer.cell_metrics(), padding: pad.min, balance: pad.balance };
                // A7: the grid stays (the one asked for, while the tabs wait;
                // else the current one — also when the window moves to a
                // monitor of another scale); the window gets the pixels it
                // needs at this scale. Keeping the window's pixels instead gave
                // a 64x19 grid for --cols 80 --rows 24 at 125 % (kramers).
                let keep =
                    if win.pending_start.is_some() || win.grid.cols == 0 { win.requested_grid } else { win.grid };
                let (w, h) = win.layout.window_size(keep.cols, keep.rows);
                log::info!("scale {scale_factor}: keeping the {}x{} grid, window {w}x{h} px", keep.cols, keep.rows);
                if let Err(e) = inner_size_writer.request_inner_size(PhysicalSize::new(w, h)) {
                    log::debug!("window size {w}x{h}: {e}");
                }
                let size = PhysicalSize::new(w, h);
                if win.pending_start.is_some() {
                    win.pending_resized(size);
                    win.start_pending();
                } else {
                    win.grid = GridSize::new(0, 0);
                    win.resized(size);
                }
            }
            WindowEvent::ModifiersChanged(m) => win.mods = m.state(),
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => win.keyboard(&event),
            WindowEvent::Ime(ime) => win.ime(ime),
            WindowEvent::CursorMoved { position, .. } => {
                win.mouse = position;
                win.title_hover();
                win.update_link_hover();
                win.gear_pointer();
                if win.mouse_tracking() {
                    win.mouse_event(MouseAction::Motion, win.held.first().copied());
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(b) = input::mouse_button(button) else { return };
                let action = match state {
                    ElementState::Pressed => {
                        if b == MouseButton::Left && win.title_press() {
                            return;
                        }
                        // The settings button, then the panel: neither reaches the program.
                        if b == MouseButton::Left && win.gear_click() {
                            win.gear_press = true;
                            return;
                        }
                        if win.settings_panel.is_some() {
                            if b == MouseButton::Left {
                                let (col, row) = win.layout.cell_at(win.origin(), win.mouse.x, win.mouse.y, win.grid);
                                win.settings_click(col, row);
                            }
                            return;
                        }
                        // The sidebar (stage PH): its rows act, nothing reaches the program.
                        if b == MouseButton::Left && win.sidebar_click() {
                            win.sidebar_press = true;
                            return;
                        }
                        if win.sidebar_focus {
                            win.sidebar_focus = false;
                            win.full = true;
                            win.recompose();
                        }
                        // Ctrl+click on a hyperlink (DESIGN.md §2.2); not
                        // sent to the program.
                        if b == MouseButton::Left && win.mods.control_key() && win.link_click() {
                            win.link_press = true;
                            return;
                        }
                        // The title row has no menu (the window menu is not done).
                        if win.integrated && win.title_col().is_some() {
                            return;
                        }
                        win.held.retain(|x| *x != b);
                        win.held.push(b);
                        MouseAction::Press
                    }
                    ElementState::Released => {
                        if b == MouseButton::Left && std::mem::take(&mut win.link_press) {
                            return;
                        }
                        if b == MouseButton::Left && std::mem::take(&mut win.gear_press) {
                            return;
                        }
                        if b == MouseButton::Left && std::mem::take(&mut win.sidebar_press) {
                            return;
                        }
                        if win.settings_panel.is_some() {
                            win.held.retain(|x| *x != b);
                            return;
                        }
                        if b == MouseButton::Left && win.pressed_button.is_some() {
                            if win.title_release() {
                                close_window(win, el);
                            }
                            return;
                        }
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
                    if win.settings_panel.is_some() {
                        win.settings_wheel(n);
                        return;
                    }
                    if win.diff.is_some() {
                        let page = win.diff_rows();
                        let (cols, pref, split) = (win.diff_cols(), win.diff_pref(), win.cfg.diff_split_cols);
                        if let Some(d) = win.diff.as_mut() {
                            let k = if n > 0 { "k" } else { "j" };
                            let layout = d.layout(cols, pref, split);
                            for _ in 0..(n.unsigned_abs() * 3) {
                                d.key(k, false, page, layout);
                            }
                        }
                        win.recompose();
                        return;
                    }
                    let lines = input::wheel_lines(n, win.mouse_tracking());
                    log::debug!("wheel {delta:?} -> {n} notches, {lines} lines at {col},{row}");
                    win.handle().wheel(Origin::Human, lines, col, row, input::mods(win.mods));
                }
            }
            WindowEvent::Focused(f) => {
                log::debug!("focused {f}");
                win.focused = f;
                win.handle().focus(f);
                if f {
                    win.seen();
                    // Edited the config in another window: apply its colours.
                    win.reload_config(false);
                }
                win.mark_cursor_row();
                // X11 sends this on every map (also after un-minimizing), and
                // the window's contents are gone then; an unchanged screen
                // would otherwise leave it unpainted.
                win.full = true;
                win.redraw();
            }
            WindowEvent::Occluded(false) => {
                win.full = true;
                win.redraw();
            }
            _ => {}
        }
        // Stage PU: a confirmed "Restart now" or "Roll back" (only ever from the person's key or click in the
        // panel) closes the window like its close button; `main` then starts the new version.
        if let Some(win) = self.win.as_mut() {
            if let Some(previous) = win.restart_req.take() {
                let form = win.updater.form.clone();
                let daemon = win.client.daemon_pid;
                close_window(win, el);
                self.restart = Some((form, previous, daemon));
            }
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
    let shortcut = k.mods.intersects(haetty_core::Mods::CTRL | haetty_core::Mods::ALT | haetty_core::Mods::SUPER);
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

/// The window is closed (close button, title bar ×): closing the window
/// ends its sessions; the tab set "last" keeps them for the next start. A
/// crashed or killed GUI leaves them running.
fn close_window(win: &mut Win, el: &ActiveEventLoop) {
    win.log_memory();
    win.save_last();
    win.shutting_down = true;
    while !win.tabs.is_empty() {
        win.close_tab(win.tabs.len() - 1);
    }
    el.exit()
}

/// Padding settings at a scale factor; `HAETTY_DEBUG_PADDING=1` paints it red.
fn padding_style(c: &crate::config::Config, scale: f64) -> PaddingStyle {
    PaddingStyle {
        min: ((c.window_padding_x * scale).round() as u32, (c.window_padding_y * scale).round() as u32),
        balance: c.padding_balance,
        color: c.padding_color,
        debug: haetty_proto::product::env_os("DEBUG_PADDING").is_some_and(|v| !v.is_empty() && v != "0"),
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

/// The same file, one path perhaps relative to its repository: equal, or one ends with the other after a `/`.
fn same_file(a: &str, b: &str) -> bool {
    a.replace('\\', "/") == b.replace('\\', "/")
}

/// The list of files with the shown view's own file in it, and its index: the view's file is put first when the list
/// does not hold it.
fn merged_list(own: &[diffview::DiffFile], mut list: Vec<diffview::DiffFile>) -> (Vec<diffview::DiffFile>, usize) {
    let Some(own) = own.first() else { return (list, 0) };
    match list.iter().position(|f| same_file(&f.path, &own.path)) {
        Some(i) => (list, i),
        None => {
            list.insert(0, own.clone());
            (list, 0)
        }
    }
}

/// The next (or previous) index of `n` files, wrapping; `None` when there is nothing to step to.
fn step_index(cur: usize, n: usize, forward: bool) -> Option<usize> {
    (n > 1).then(|| if forward { (cur + 1) % n } else { (cur + n - 1) % n })
}

/// The path of an edit that arrived with `new` (the edit count rose); `None` for the first sight of a session.
fn edit_arrived(old: Option<&ClaudeInfo>, new: Option<&ClaudeInfo>) -> Option<String> {
    let (old, new) = (old?, new?);
    (new.edits > old.edits).then(|| new.last_edit.as_ref().map(|m| m.path.clone())).flatten()
}

fn after_edit_allowed(on: bool, asking: bool, attention: Attention, ui_busy: bool) -> bool {
    on && !asking && attention != Attention::Ask && !ui_busy
}

/// The overlay's rectangle and the layout settings a key needs.
#[derive(Clone, Copy)]
struct DiffArea {
    height: usize,
    cols: usize,
    setting: diffview::DiffLayoutPref,
    split_cols: usize,
}

/// A key for the open diff: the layout toggle becomes a per-overlay override of the setting.
fn diff_key(
    d: &mut DiffView,
    key: &str,
    shift: bool,
    a: DiffArea,
    pref: &mut Option<diffview::DiffLayoutPref>,
) -> diffview::Key {
    let (height, split_cols) = (a.height, a.split_cols);
    let layout = d.layout(a.cols, pref.unwrap_or(a.setting), split_cols);
    match d.key(key, shift, height, layout) {
        diffview::Key::ToggleLayout => {
            let (next, p) = match layout {
                diffview::Layout::Split => (diffview::Layout::Stacked, diffview::DiffLayoutPref::Stacked),
                diffview::Layout::Stacked => (diffview::Layout::Split, diffview::DiffLayoutPref::Split),
            };
            *pref = Some(p);
            d.key("", false, height, next);
            diffview::Key::Stay
        }
        k => k,
    }
}

#[cfg(test)]
mod tests {
    /// A7: the window size asked for a grid gives that grid back at 100, 125,
    /// 150 and 200 % with the real font and padding; after a provisional
    /// scale of 1.0 the scale change keeps the grid, where keeping the
    /// window's pixels (the old way) changed it.
    #[test]
    fn grid_kept_across_scales() {
        use super::{padding_style, GridSize, Layout};
        use crate::font::Fonts;
        use crate::render::paint::Painter;
        let Ok(fonts) = Fonts::load(None) else { return };
        let mut painter = Painter::new(fonts, 1.0);
        let c = crate::config::Config::default();
        let layout_at = |p: &mut Painter, s: f64| {
            p.set_px((11.0 * 96.0 / 72.0 * s) as f32);
            let pad = padding_style(&c, s);
            Layout { cell: p.cell_metrics(), padding: pad.min, balance: pad.balance }
        };
        let want = GridSize::new(80, 24);
        let first = layout_at(&mut painter, 1.0);
        let (w0, h0) = first.window_size(want.cols, want.rows);
        for s in [1.0, 1.25, 1.5, 2.0] {
            let l = layout_at(&mut painter, s);
            let (w, h) = l.window_size(want.cols, want.rows);
            let g = l.grid(w, h);
            assert_eq!((g.cols, g.rows), (80, 24), "scale {s}: {w}x{h} px");
            assert_eq!(
                (w, h),
                (80 * l.cell.width + 2 * l.padding.0, 24 * l.cell.height + 2 * l.padding.1),
                "scale {s}"
            );
            if s > 1.0 {
                let old = l.grid(w0, h0);
                assert!(
                    (old.cols, old.rows) != (80, 24),
                    "scale {s}: the old way kept {w0}x{h0} px = {}x{}",
                    old.cols,
                    old.rows
                );
            }
        }
    }

    use super::{after_edit_allowed, diff_key, edit_arrived, merged_list, same_file, step_index};
    use crate::diffview::{DiffFile, DiffLayoutPref, DiffView, Key as DKey, Layout as DLayout};
    use haetty_proto::wire::{Attention, ClaudeInfo, EditMeta, EditView, Secret, SnapStatus};

    fn fake(path: &str, before: &str, after: &str) -> DiffView {
        let e = EditView {
            meta: EditMeta { path: path.into(), agent: "main".into(), at_ms: 0, added: 1, removed: 1, diffable: true },
            status: SnapStatus::Copied,
            before: Secret(before.into()),
            after: Secret(after.into()),
        };
        DiffView::new(Some(&e))
    }

    fn file(p: &str) -> DiffFile {
        DiffFile { path: p.into(), added: 1, removed: 0 }
    }

    fn info(edits: u32, path: &str) -> ClaudeInfo {
        ClaudeInfo {
            edits,
            last_edit: Some(EditMeta {
                path: path.into(),
                agent: "main".into(),
                at_ms: 0,
                added: 1,
                removed: 0,
                diffable: true,
            }),
            ..Default::default()
        }
    }

    fn area(height: usize, cols: usize, setting: DiffLayoutPref) -> super::DiffArea {
        super::DiffArea { height, cols, setting, split_cols: 100 }
    }

    #[test]
    fn diff_keys_with_a_fake_view() {
        let long: String = (0..300).map(|i| format!("{i}\n")).collect();
        let changed = long.replace("\n30\n", "\nX\n").replace("\n250\n", "\nY\n");
        let mut v = fake("/p/a.rs", &long, &changed);
        let mut pref = None;
        let key = |v: &mut DiffView, k: &str, pref: &mut Option<DiffLayoutPref>| {
            diff_key(v, k, false, area(6, 120, DiffLayoutPref::Auto), pref)
        };
        assert_eq!(key(&mut v, "n", &mut pref), DKey::NextFile);
        assert_eq!(key(&mut v, "p", &mut pref), DKey::PrevFile);
        assert_eq!(key(&mut v, "m", &mut pref), DKey::Next);
        assert_eq!(key(&mut v, "r", &mut pref), DKey::Refresh);
        assert_eq!(key(&mut v, "escape", &mut pref), DKey::Close);
        assert_eq!(key(&mut v, "q", &mut pref), DKey::Close);
        assert_eq!(key(&mut v, "]", &mut pref), DKey::Stay);
        let first = v.scroll;
        assert!(first > 0);
        assert_eq!(key(&mut v, "[", &mut pref), DKey::Stay);
        assert!(v.scroll < first);
        assert_eq!(key(&mut v, "]", &mut pref), DKey::Stay);
        assert_eq!(v.scroll, first);
        assert_eq!(pref, None);
        assert_eq!(v.layout(120, DiffLayoutPref::Auto, 100), DLayout::Split);
        assert_eq!(key(&mut v, "v", &mut pref), DKey::Stay);
        assert_eq!(pref, Some(DiffLayoutPref::Stacked));
        assert_eq!(key(&mut v, "v", &mut pref), DKey::Stay);
        assert_eq!(pref, Some(DiffLayoutPref::Split));
    }

    #[test]
    fn layout_override_beats_the_setting_until_the_next_overlay() {
        let mut v = fake("/p/a.rs", "a\n", "b\n");
        let mut pref = None;
        diff_key(&mut v, "v", false, area(12, 80, DiffLayoutPref::Auto), &mut pref);
        assert_eq!(pref, Some(DiffLayoutPref::Split));
        assert_eq!(v.layout(80, pref.unwrap(), 100), DLayout::Split);
        let mut fresh = None;
        diff_key(&mut v, "v", false, area(12, 200, DiffLayoutPref::Stacked), &mut fresh);
        assert_eq!(fresh, Some(DiffLayoutPref::Split));
    }

    #[test]
    fn file_list_and_stepping() {
        assert_eq!(step_index(0, 3, true), Some(1));
        assert_eq!(step_index(2, 3, true), Some(0));
        assert_eq!(step_index(0, 3, false), Some(2));
        assert_eq!(step_index(0, 1, true), None);
        assert_eq!(step_index(0, 0, false), None);
        assert!(same_file("/r/a.rs", "/r/a.rs"));
        assert!(same_file("C:\\r\\a.rs", "C:/r/a.rs"));
        assert!(!same_file("/r/src/a.rs", "src/a.rs"), "full paths only");
        assert!(!same_file("/wt1/src/a.rs", "/wt2/src/a.rs"), "the same tail in another worktree is another file");
        assert!(!same_file("/r/xa.rs", "a.rs"));
        let list = vec![file("/r/b.rs"), file("/r/src/a.rs")];
        let (l, at) = merged_list(&[file("/r/src/a.rs")], list.clone());
        assert_eq!((l.len(), at), (2, 1));
        let (l, at) = merged_list(&[file("c.rs")], list.clone());
        assert_eq!((l.len(), at, l[0].path.as_str()), (3, 0, "c.rs"));
        assert_eq!(merged_list(&[], list).1, 0);
    }

    #[test]
    fn after_edit_wiring_decisions() {
        let (a, b) = (info(1, "/r/a.rs"), info(2, "/r/b.rs"));
        assert_eq!(edit_arrived(Some(&a), Some(&b)).as_deref(), Some("/r/b.rs"));
        assert_eq!(edit_arrived(Some(&b), Some(&b)), None);
        assert_eq!(edit_arrived(None, Some(&b)), None);
        assert_eq!(edit_arrived(Some(&a), None), None);
        assert!(after_edit_allowed(true, false, Attention::None, false));
        assert!(after_edit_allowed(true, false, Attention::Wait, false));
        assert!(!after_edit_allowed(false, false, Attention::None, false));
        assert!(!after_edit_allowed(true, true, Attention::None, false));
        assert!(!after_edit_allowed(true, false, Attention::Ask, false));
        assert!(!after_edit_allowed(true, false, Attention::None, true));
    }

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
