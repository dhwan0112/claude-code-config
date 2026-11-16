//! The window's updater (stage PU, DESIGN.md §2.4, §3.17): one background thread that checks, downloads and installs
//! when asked, and reports back through the event loop (`UserEvent::Update`). It never runs the network on the
//! window's thread, so a slow or absent network changes neither the start nor a key's way to the screen.
//!
//! Automatic checks: 30 s after the window opens, then once a day (`state::Check`), only for an installed build of
//! the stable instance with `update-check` on — never a dev instance, a cargo build or a test run
//! (`HAETTY_TEST_TMP`), unless a test build (`update-test`) is asked to (`HAETTY_UPDATE_TEST=1`). A download starts
//! only from the person's press in the panel.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::time::{Duration, Instant};

use haetty_update::flow::{self, Done, Found, Origin, Release};
use haetty_update::layout::{self, Form};
use haetty_update::state::{self, Check};
use haetty_update::version::Version;

/// The first automatic check after the window opens.
pub const FIRST_CHECK: Duration = Duration::from_secs(30);

pub fn current() -> Version {
    Version::parse(env!("HAETTY_BUILD_VERSION")).unwrap_or(Version { major: 0, minor: 0, patch: 0, pre: vec![] })
}

/// `<state>/update/`.
pub fn dir() -> PathBuf {
    flow::dir(&haetty_proto::paths::state_dir())
}

/// How this executable is installed.
pub fn form() -> Form {
    layout::detect(&haetty_proto::product::exe_path(), &haetty_proto::product::instance())
}

/// Whether automatic checks run here (see the module text).
pub fn may_check(form: &Form) -> bool {
    let test_asked = cfg!(feature = "update-test") && std::env::var_os("HAETTY_UPDATE_TEST").is_some_and(|v| v == "1");
    if test_asked {
        return true;
    }
    !matches!(form, Form::Dev(_)) && haetty_proto::product::env_os("TEST_TMP").is_none()
}

/// This version's name in the start guard: its version folder, or `single`.
pub fn my_name(form: &Form) -> Option<String> {
    match form {
        Form::Versioned { current, .. } => Some(current.clone()),
        Form::Single { .. } => Some("single".into()),
        _ => None,
    }
}

/// What the thread reports.
#[derive(Debug)]
pub enum Event {
    Checked(Result<Found, String>),
    Progress(u64, u64),
    Installed(Result<Done, String>),
}

enum Cmd {
    Check {
        url: String,
    },
    /// Download, check and install the release found last.
    Install {
        keep: usize,
    },
}

/// The window's handle on the thread.
pub struct Updater {
    tx: Sender<Cmd>,
    pub form: Form,
    /// The newer release found by the last check.
    pub release: Option<Release>,
    /// Installed, waiting for a restart.
    pub done: Option<Done>,
    pub busy: bool,
    pub progress: Option<String>,
    pub last: Check,
    /// When the next automatic check is due.
    pub next_check: Option<Instant>,
    /// A new version's first start: confirm it at this time (`state::CONFIRM_AFTER`).
    pub confirm_at: Option<Instant>,
    /// After an automatic rollback.
    pub note: Option<String>,
}

impl Updater {
    pub fn start(send: impl Fn(Event) + Send + 'static, watch: bool) -> Updater {
        let (tx, rx) = channel::<Cmd>();
        let form = form();
        let state_dir = dir();
        let f2 = form.clone();
        std::thread::Builder::new()
            .name("updater".into())
            .spawn(move || {
                let mut release: Option<Release> = None;
                for cmd in rx {
                    match cmd {
                        Cmd::Check { url } => {
                            let origin = Origin::of(&url);
                            let https = haetty_update::net::Https::new(origin.hosts(), haetty_update::net::TIMEOUT);
                            let r = flow::check(&https, &origin, &current(), &haetty_update::sign::trusted());
                            if let Ok(Found::Newer(rel)) = &r {
                                release = Some((**rel).clone());
                            }
                            // The release URL's query (download tokens) never reaches the log.
                            log::info!(
                                "update check: {}",
                                match &r {
                                    Ok(Found::UpToDate(v)) => format!("up to date (latest {v})"),
                                    Ok(Found::Newer(rel)) => format!("{} available", rel.manifest.version),
                                    Err(e) => format!("failed: {}", flow::loggable(e)),
                                }
                            );
                            send(Event::Checked(r));
                        }
                        Cmd::Install { keep } => {
                            let r = (|| {
                                let rel = release.clone().ok_or("check first")?;
                                if !haetty_update::sign::have_release_key() {
                                    return Err(
                                        "this build has no release key: install the new version by hand".to_string()
                                    );
                                }
                                let https = haetty_update::net::Https::new(
                                    Origin::of(&rel.base)
                                        .hosts()
                                        .into_iter()
                                        .chain(flow::GITHUB_HOSTS.iter().map(|h| h.to_string()))
                                        .collect(),
                                    Duration::from_secs(600),
                                );
                                let files =
                                    flow::download(&https, &rel, &state_dir, &mut |d, t| send(Event::Progress(d, t)))?;
                                flow::install(&f2, &rel, &files, &state_dir, keep)
                            })();
                            log::info!(
                                "update install: {}",
                                match &r {
                                    Ok(d) => format!("{} installed (before: {})", d.new, d.previous),
                                    Err(e) => format!("not installed: {e}"),
                                }
                            );
                            send(Event::Installed(r));
                        }
                    }
                }
            })
            .expect("updater thread");
        let last = Check::load(&dir());
        let note = state::take_note(&dir());
        let next_check = Some(Instant::now() + FIRST_CHECK);
        Updater {
            tx,
            form,
            release: None,
            done: None,
            busy: false,
            progress: None,
            last,
            next_check,
            confirm_at: watch.then(|| Instant::now() + state::CONFIRM_AFTER),
            note,
        }
    }

    pub fn check(&mut self, url: &str) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.progress = Some("checking…".into());
        let _ = self.tx.send(Cmd::Check { url: url.to_string() });
    }

    pub fn install(&mut self, keep: usize) {
        if self.busy || self.release.is_none() {
            return;
        }
        self.busy = true;
        self.progress = Some("downloading…".into());
        let _ = self.tx.send(Cmd::Install { keep });
    }

    /// The thread's report.
    pub fn handle(&mut self, ev: Event) {
        let now = haetty_update::now_ms();
        match ev {
            Event::Checked(r) => {
                self.busy = false;
                self.progress = None;
                self.last.last_ms = now;
                match r {
                    Ok(Found::UpToDate(v)) => {
                        self.last.failures = 0;
                        self.last.latest.clear();
                        self.last.note = format!("no newer version (latest {v})");
                        self.release = None;
                    }
                    Ok(Found::Newer(rel)) => {
                        self.last.failures = 0;
                        self.last.latest = rel.manifest.version.to_string();
                        self.last.note = format!("{} available", rel.manifest.version);
                        self.release = Some(*rel);
                    }
                    Err(e) => {
                        self.last.failures = self.last.failures.saturating_add(1);
                        self.last.note = format!("failed: {}", flow::loggable(&e));
                    }
                }
                let _ = self.last.save(&dir());
            }
            Event::Progress(d, t) => {
                let mb = |b: u64| b as f64 / 1_048_576.0;
                self.progress = Some(if t > 0 {
                    format!("downloading {:.1} of {:.1} MB", mb(d), mb(t))
                } else {
                    format!("downloading {:.1} MB", mb(d))
                });
            }
            Event::Installed(r) => {
                self.busy = false;
                match r {
                    Ok(d) => {
                        self.progress = None;
                        self.done = Some(d);
                    }
                    Err(e) => self.progress = Some(format!("not installed: {e}")),
                }
            }
        }
    }

    /// The automatic check is due (`tick`): returns the next deadline.
    pub fn due(&mut self, now: Instant, enabled: bool) -> Option<bool> {
        let t = self.next_check?;
        if now < t {
            return Some(false);
        }
        let wait_ms = self.last.next_due_ms().saturating_sub(haetty_update::now_ms());
        if !enabled || wait_ms > 0 {
            self.next_check = Some(now + Duration::from_millis(wait_ms.max(3_600_000)));
            return Some(false);
        }
        self.next_check = Some(now + state::EVERY);
        Some(true)
    }

    /// The version a rollback goes back to.
    pub fn rollback_target(&self) -> Option<String> {
        match &self.form {
            Form::Versioned { opt, link, current } => {
                layout::list(opt, link).into_iter().map(|x| x.0).find(|n| n != current)
            }
            Form::Single { exe } => {
                let old = exe.with_file_name(format!("{}.old", exe.file_name()?.to_string_lossy()));
                old.is_file().then(|| "the version before (beside the executable)".into())
            }
            _ => None,
        }
    }

    /// Why this build cannot install `rel`.
    pub fn blocked(&self, rel: &Release) -> Option<String> {
        if let Some(b) = &rel.blocked {
            return Some(b.clone());
        }
        if let Some(r) = self.form.refusal() {
            return Some(r);
        }
        (!haetty_update::sign::have_release_key()).then(|| "this build has no release key: install it by hand".into())
    }
}

/// At start (before the window): a new version's start guard. Returns whether to watch this start, or rolls back and
/// starts the version before in this process's place (it does not return then).
pub fn start_guard() -> bool {
    let form = form();
    let Some(me) = my_name(&form) else { return false };
    let d = dir();
    match state::begin_start(&d, &me) {
        state::Start::Normal => {
            if let Form::Single { exe } = &form {
                layout::clean_old(exe);
            }
            false
        }
        state::Start::Watch => true,
        state::Start::RollBack { new, previous } => {
            let back = match &form {
                Form::Versioned { opt, link, .. } => layout::activate(opt, link, &previous).map(|_| link.clone()),
                Form::Single { exe } => layout::rollback_single(exe).map(|_| exe.clone()),
                _ => return false,
            };
            match back {
                Ok(launcher) => {
                    let note = format!("{new} did not stay up for 20 s: rolled back to {previous}");
                    log::warn!("update: {note}");
                    state::set_note(&d, &note);
                    relaunch(&launcher);
                }
                Err(e) => {
                    log::error!("update: rollback to {previous} failed: {e}");
                    false
                }
            }
        }
    }
}

/// Starts `launcher` with this process's arguments in its place.
pub fn relaunch(launcher: &std::path::Path) -> ! {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let e = std::process::Command::new(launcher).args(&args).exec();
        log::error!("update: could not start {}: {e}", launcher.display());
        std::process::exit(1)
    }
    #[cfg(not(unix))]
    {
        let _ = std::process::Command::new(launcher).args(&args).spawn();
        std::process::exit(0)
    }
}

/// The executable a restart starts: the link of a versioned install (it follows updates), else this executable.
pub fn launcher(form: &Form) -> PathBuf {
    match form {
        Form::Versioned { link, .. } if cfg!(windows) => link.join(layout::exe_name()),
        Form::Versioned { link, .. } => link.clone(),
        _ => haetty_proto::product::exe_path(),
    }
}

/// "Restart now", after the window closed: waits for this instance's daemon to end (its sessions are closed; it
/// exits 10 s after the last window), starts the new version detached, and watches it for 20 s. If it ends that
/// soon, the link goes back to `previous` and the version before starts instead (it shows why).
pub fn restart_after_close(form: &Form, previous: Option<String>) {
    let ep = haetty_proto::paths::Endpoint::default_for_user();
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        let gone = match &ep {
            Ok(ep) => matches!(
                haetty_proto::Client::connect(ep, haetty_proto::Role::Observe, "restart"),
                Err(haetty_proto::client::ConnectError::NotRunning)
            ),
            Err(_) => true,
        };
        if gone {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let exe = launcher(form);
    let mut cmd = std::process::Command::new(&exe);
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let started = Instant::now();
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            log::error!("restart: {} did not start: {e}", exe.display());
            return;
        }
    };
    log::info!("restart: {} started", exe.display());
    while started.elapsed() < state::CONFIRM_AFTER {
        if let Ok(Some(st)) = child.try_wait() {
            log::warn!("restart: the new version ended after {:?} ({st})", started.elapsed());
            let d = dir();
            if let (Form::Versioned { opt, link, .. }, Some(prev)) = (form, previous.as_deref()) {
                if layout::activate(opt, link, prev).is_ok() {
                    state::set_note(&d, &format!("the new version ended at its start: rolled back to {prev}"));
                    state::confirm(&d);
                    let _ = std::process::Command::new(launcher(form)).spawn();
                }
            } else if let Form::Single { exe } = form {
                if layout::rollback_single(exe).is_ok() {
                    state::set_note(&d, "the new version ended at its start: rolled back");
                    state::confirm(&d);
                    let _ = std::process::Command::new(exe).spawn();
                }
            }
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The message for a new window that met an older daemon (protocol mismatch): how many sessions it still runs,
/// asked through the version before's own `status --json` when it is installed beside this one.
pub fn old_daemon_note(form: &Form) -> Option<String> {
    let Form::Versioned { opt, link, current } = form else { return None };
    let prev = layout::list(opt, link).into_iter().map(|x| x.0).find(|n| n != current)?;
    let out = std::process::Command::new(opt.join(&prev).join(layout::exe_name()))
        .args(["status", "--json"])
        .output()
        .ok()?;
    let j = haetty_cc::json::Json::parse(&String::from_utf8_lossy(&out.stdout)).ok()?;
    let n = j.get("sessions").and_then(|s| s.arr()).map_or(0, |s| s.len());
    Some(format!(
        "old daemon still running {n} session(s) ({prev}); they are not ended. It exits by itself 10 s after its last window closes; then start haetty again."
    ))
}
