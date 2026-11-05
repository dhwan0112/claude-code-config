//! `haetty hook EVENT` and `haetty statusline`: run by Claude Code, forward
//! their stdin JSON to the daemon and end. They never write to stdout or
//! stderr (UserPromptSubmit's stdout would go into the model's context),
//! always exit 0, never start a daemon, and end within [`SYNC_DEADLINE`]
//! (synchronous hooks, which Claude Code waits for) or [`DEADLINE`] (the
//! others) whatever happens (stdin left open, a stuck daemon); without a
//! daemon or outside a haetty session they end at once (DESIGN.md §3.13).

use std::io::Read;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ccterm_proto::paths::Endpoint;
use ccterm_proto::wire::{ClientMsg, Secret};
use ccterm_proto::{product, Client, Role};

use crate::edit;
use crate::json::Json;

/// Upper bound for the whole process when a daemon answers (asynchronous
/// hooks and the status line: Claude Code does not wait for them).
pub const DEADLINE: Duration = Duration::from_millis(1000);
/// Upper bound for a synchronous hook, which Claude Code waits for
/// (DESIGN.md §3.4): the edit tools' PreToolUse and PermissionRequest.
pub const SYNC_DEADLINE: Duration = Duration::from_millis(250);
/// Of that, the file copy may take at most this; else the event is
/// reported without it (the edit is then shown without a diff).
const SNAPSHOT_BUDGET: Duration = Duration::from_millis(150);

/// The hooks haetty installs as synchronous (settings.rs).
pub fn is_synchronous(event: &str) -> bool {
    matches!(event, "PreToolUse" | "PermissionRequest")
}

/// Connect timeout (a local socket: refused or accepted at once).
const DIAL: Duration = Duration::from_millis(50);
/// Largest stdin read (a Write tool's content is in it).
const MAX_STDIN: u64 = 16 << 20;

pub enum Kind {
    Hook(String),
    StatusLine,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Runs the reporter and exits 0.
pub fn main(kind: Kind) -> ! {
    std::panic::set_hook(Box::new(|_| {})); // silent: nothing on stderr
    let start = Instant::now();
    let ts_ms = now_ms();
    let Some(session) = product::env("SESSION_ID").filter(|s| !s.is_empty()) else {
        std::process::exit(0); // not a haetty tab
    };
    let deadline = match &kind {
        Kind::Hook(e) if is_synchronous(e) => SYNC_DEADLINE,
        _ => DEADLINE,
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new().name("haetty-report".into()).spawn(move || {
        let _ = run(kind, session, ts_ms, start + deadline);
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(deadline.saturating_sub(start.elapsed()));
    std::process::exit(0);
}

fn run(kind: Kind, session: String, ts_ms: u64, end: Instant) -> Option<()> {
    let ep = Endpoint::default_for_user().ok()?;
    // Connect first: without a daemon, stdin is not even read.
    let client = Client::connect_with(&ep, Role::Report, "hook", DIAL).ok()?;
    let mut input = String::new();
    std::io::stdin().lock().take(MAX_STDIN).read_to_string(&mut input).ok()?;
    let msg = match kind {
        Kind::StatusLine => ClientMsg::StatusLine { session: Some(session), json: Secret(input), ts_ms },
        Kind::Hook(event) => {
            let snapshot = if event == "PreToolUse" { timed_snapshot(&input) } else { None };
            ClientMsg::Hook { session: Some(session), event, json: Secret(input), ts_ms, snapshot }
        }
    };
    client.send(&msg).ok()?;
    // Wait until the daemon has handled it: a synchronous hook then really
    // orders its event before the next one, and nothing is lost on exit.
    client.sync(end.saturating_duration_since(Instant::now())).then_some(())
}

/// [`pre_snapshot`] within [`SNAPSHOT_BUDGET`]; `None` when it takes longer
/// (a slow disk or network file system), so the hook still ends in time.
fn timed_snapshot(input: &str) -> Option<ccterm_proto::wire::PreSnapshot> {
    let (tx, rx) = std::sync::mpsc::channel();
    let input = input.to_string();
    std::thread::Builder::new()
        .name("haetty-snapshot".into())
        .spawn(move || {
            let _ = tx.send(pre_snapshot(&input));
        })
        .ok()?;
    rx.recv_timeout(SNAPSHOT_BUDGET).ok().flatten()
}

/// For an edit tool, the file before the edit (read here, in the hook,
/// while Claude Code waits for the synchronous PreToolUse).
fn pre_snapshot(input: &str) -> Option<ccterm_proto::wire::PreSnapshot> {
    let j = Json::parse(input).ok()?;
    let tool = j.get("tool_name").and_then(Json::str)?;
    if !edit::is_edit_tool(tool) || tool == "NotebookEdit" {
        return None;
    }
    let path = edit::target(j.get("tool_input")?)?;
    Some(edit::snapshot(path))
}
