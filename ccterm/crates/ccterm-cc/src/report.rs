//! `ccterm hook EVENT` and `ccterm statusline`: run by Claude Code, forward
//! their stdin JSON to the daemon and end. They never write to stdout or
//! stderr (UserPromptSubmit's stdout would go into the model's context),
//! always exit 0, never start a daemon, and end within [`DEADLINE`] whatever
//! happens (stdin left open, a stuck daemon); without a daemon or outside a
//! ccterm session they end at once (DESIGN.md §3.13).

use std::io::Read;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ccterm_proto::paths::Endpoint;
use ccterm_proto::wire::{ClientMsg, Secret};
use ccterm_proto::{product, Client, Role};

use crate::edit;
use crate::json::Json;

/// Upper bound for the whole process when a daemon answers.
pub const DEADLINE: Duration = Duration::from_millis(1000);
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
    let ts_ms = now_ms();
    let Some(session) = std::env::var(product::SESSION_ENV).ok().filter(|s| !s.is_empty()) else {
        std::process::exit(0); // not a ccterm tab
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new().name("ccterm-report".into()).spawn(move || {
        let _ = run(kind, session, ts_ms);
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(DEADLINE);
    std::process::exit(0);
}

fn run(kind: Kind, session: String, ts_ms: u64) -> Option<()> {
    let ep = Endpoint::default_for_user().ok()?;
    // Connect first: without a daemon, stdin is not even read.
    let client = Client::connect_with(&ep, Role::Report, "hook", DIAL).ok()?;
    let mut input = String::new();
    std::io::stdin().lock().take(MAX_STDIN).read_to_string(&mut input).ok()?;
    let msg = match kind {
        Kind::StatusLine => ClientMsg::StatusLine { session: Some(session), json: Secret(input), ts_ms },
        Kind::Hook(event) => {
            let snapshot = if event == "PreToolUse" { pre_snapshot(&input) } else { None };
            ClientMsg::Hook { session: Some(session), event, json: Secret(input), ts_ms, snapshot }
        }
    };
    client.send(&msg).ok()?;
    // Wait until the daemon has handled it: a synchronous hook then really
    // orders its event before the next one, and nothing is lost on exit.
    client.sync(DEADLINE).then_some(())
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
