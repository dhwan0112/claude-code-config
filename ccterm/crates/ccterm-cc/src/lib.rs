//! Claude Code integration (DESIGN.md §3.13): the `--settings` injected into
//! Claude Code tabs, the `hook` and `statusline` reporters, the per-session
//! state the daemon keeps from them, edit snapshots and diffs, resume.
//! No GUI code; used by the daemon and the GUI.

pub mod edit;
pub mod json;
pub mod report;
pub mod resume;
pub mod settings;
pub mod state;

pub use json::Json;
pub use similar;
pub use state::Claude;
