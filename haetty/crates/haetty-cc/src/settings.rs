//! The `--settings` haetty gives Claude Code tabs (DESIGN.md §3.13): hooks
//! that report to the daemon (`haetty hook EVENT`, exec form: no shell) and
//! a status line (`haetty statusline`, prints nothing). Only these two keys;
//! none of ccx's behaviour-changing settings (permissions, tui, env).
//!
//! A `--settings` the user passed (file or inline JSON) is merged: our hook
//! entries first, the user's appended per event; every other key the user's;
//! the user's `statusLine` is dropped with a warning (ours feeds haetty's
//! status row).

use crate::json::{obj, s, Json};

/// The edit tools whose PreToolUse runs synchronously: the hook copies the
/// file before the tool changes it, while Claude Code waits.
pub const EDIT_MATCHER: &str = "Write|Edit|MultiEdit|NotebookEdit";

/// PreToolUse for every tool, asynchronous (clears a "waiting" mark; never
/// holds Claude Code up — a process start alone is ~50 ms on Windows). The
/// daemon ignores it for edit tools, which the synchronous entry reports.
pub const PRE_ALL: &str = "PreToolUseAll";

/// (Claude Code event, matcher, async, name passed to `haetty hook`).
pub const EVENTS: &[(&str, Option<&str>, bool, &str)] = &[
    ("SessionStart", None, true, "SessionStart"),
    ("UserPromptSubmit", None, true, "UserPromptSubmit"),
    ("PreToolUse", Some(EDIT_MATCHER), false, "PreToolUse"),
    ("PreToolUse", None, true, PRE_ALL),
    // Synchronous: the question is in the daemon before Claude Code shows
    // its dialog (no output: the normal permission flow decides).
    ("PermissionRequest", None, false, "PermissionRequest"),
    ("PermissionDenied", None, true, "PermissionDenied"),
    ("PostToolUse", None, true, "PostToolUse"),
    ("PostToolUseFailure", None, true, "PostToolUseFailure"),
    ("Notification", None, true, "Notification"),
    ("Stop", None, true, "Stop"),
    ("StopFailure", None, true, "StopFailure"),
    ("SubagentStart", None, true, "SubagentStart"),
    ("SubagentStop", None, true, "SubagentStop"),
    ("SessionEnd", None, true, "SessionEnd"),
];

/// Hook timeout for Claude Code (seconds); `haetty hook` ends far sooner.
const TIMEOUT: f64 = 5.0;

/// Whether `program` is Claude Code (`claude`, `claude.exe`, `claude.cmd`).
pub fn is_claude(program: &str) -> bool {
    let base = program.rsplit(['/', '\\']).next().unwrap_or(program).to_ascii_lowercase();
    matches!(base.as_str(), "claude" | "claude.exe" | "claude.cmd" | "claude.bat")
}

/// The executable path as Claude Code should see it: forward slashes
/// (Windows accepts them, Git Bash needs them).
pub fn exe_for_claude(exe: &str) -> String {
    exe.replace('\\', "/")
}

/// A path every shell Claude Code may use runs unquoted: `sh -c`, Git
/// Bash, PowerShell (no spaces or shell characters; `~` only inside, as in
/// 8.3 short names).
pub fn is_plain(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('~')
        && path.chars().all(|c| c.is_ascii_alphanumeric() || "/._-:+~".contains(c))
}

/// The status-line command line (run by a shell: `sh -c`, Git Bash, or
/// PowerShell without Git Bash — Claude Code has no shell-free form for the
/// status line, unlike hooks). Give it [`shell_safe_exe`]: a plain path is
/// left bare, which every one of those shells runs; anything else is
/// double-quoted, which `sh` and Git Bash run but PowerShell does not.
pub fn statusline_command(exe: &str) -> String {
    let exe = exe_for_claude(exe);
    if is_plain(&exe) {
        format!("{exe} statusline")
    } else {
        format!(
            "\"{}\" statusline",
            exe.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$").replace('`', "\\`")
        )
    }
}

/// The executable as a path no shell mangles (DESIGN.md §3.13). Windows,
/// when the path has spaces or shell characters (an install folder or a
/// user name with a space): its 8.3 short path (`C:/PROGRA~1/haetty/…`),
/// else a copy in a folder without spaces (`%ProgramData%\haetty\<dir>-<SID>`,
/// refreshed when the executable changes), else the path as it is (quoted
/// later; works with Git Bash only). Elsewhere: the path as it is.
pub fn shell_safe_exe(exe: &std::path::Path) -> String {
    let long = exe.to_string_lossy().into_owned();
    #[cfg(windows)]
    {
        if is_plain(&exe_for_claude(&long)) {
            return long;
        }
        if let Some(short) = win::short_path(exe).filter(|s| is_plain(&exe_for_claude(s))) {
            return short;
        }
        match win::space_free_copy(exe) {
            Some(copy) => return copy,
            None => {
                log::warn!("no space-free path for {long}: the status line runs only where Claude Code uses Git Bash")
            }
        }
    }
    long
}

#[cfg(windows)]
mod win {
    use std::path::{Path, PathBuf};

    #[link(name = "kernel32")]
    extern "system" {
        fn GetShortPathNameW(long: *const u16, short: *mut u16, len: u32) -> u32;
    }

    /// The 8.3 short path, when the volume has short names.
    pub fn short_path(p: &Path) -> Option<String> {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut buf = vec![0u16; 1024];
        // SAFETY: `wide` is NUL-terminated; `buf` has the length passed.
        let n = unsafe { GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
        (n > 0 && n < buf.len()).then(|| String::from_utf16_lossy(&buf[..n]))
    }

    /// A copy of `exe` in `%ProgramData%\haetty\<dir>-<SID>\` when that path is
    /// plain; refreshed (via a temporary file) when size or time differ.
    pub fn space_free_copy(exe: &Path) -> Option<String> {
        let base = PathBuf::from(std::env::var_os("ProgramData")?);
        let sid = haetty_proto::transport::current_user_sid().ok()?;
        let dir = base.join(haetty_proto::product::DIR).join(format!("{}-{sid}", haetty_proto::product::dir()));
        std::fs::create_dir_all(&dir).ok()?;
        let name = exe.file_name()?;
        let copy = dir.join(name);
        let path = copy.to_string_lossy().into_owned();
        let path = if super::is_plain(&super::exe_for_claude(&path)) { path } else { short_path(&copy)? };
        let src = std::fs::metadata(exe).ok()?;
        let fresh =
            std::fs::metadata(&copy).is_ok_and(|m| m.len() == src.len() && m.modified().ok() >= src.modified().ok());
        if !fresh {
            let tmp = dir.join(format!("{}.tmp{}", name.to_string_lossy(), std::process::id()));
            std::fs::copy(exe, &tmp).ok()?;
            if std::fs::rename(&tmp, &copy).is_err() {
                // In use by a running status line: keep the old copy.
                let _ = std::fs::remove_file(&tmp);
            }
        }
        if !copy.is_file() {
            return None;
        }
        log::info!("status line runs a copy without spaces in its path: {path}");
        Some(path)
    }
}

/// haetty's settings for `exe`; the status line runs `statusline_exe`
/// ([`shell_safe_exe`]).
pub fn ours(exe: &str, statusline_exe: &str) -> Json {
    let exe = exe_for_claude(exe);
    let mut hooks: Vec<(String, Json)> = vec![];
    for (ev, matcher, asynchronous, name) in EVENTS {
        let mut h = obj([
            ("type", s("command")),
            ("command", s(exe.clone())),
            ("args", Json::Arr(vec![s("hook"), s(*name)])),
            ("timeout", Json::Num(TIMEOUT)),
        ]);
        if *asynchronous {
            h.set("async", Json::Bool(true));
        }
        let mut entry = obj([("hooks", Json::Arr(vec![h]))]);
        if let Some(m) = matcher {
            entry.set("matcher", s(*m));
        }
        match hooks.iter_mut().find(|(e, _)| e == ev) {
            Some((_, Json::Arr(a))) => a.push(entry),
            _ => hooks.push((ev.to_string(), Json::Arr(vec![entry]))),
        }
    }
    obj([
        ("hooks", Json::Obj(hooks)),
        (
            "statusLine",
            obj([
                ("type", s("command")),
                ("command", s(statusline_command(statusline_exe))),
                ("refreshInterval", Json::Num(3.0)),
            ]),
        ),
    ])
}

/// Merges the user's settings into ours; returns warnings for the log.
pub fn merge(mut ours: Json, user: Option<Json>) -> (Json, Vec<String>) {
    let mut warnings = vec![];
    let Some(Json::Obj(user)) = user else {
        return (ours, warnings);
    };
    for (k, v) in user {
        match k.as_str() {
            "statusLine" => warnings
                .push("your --settings statusLine is not used in haetty tabs (haetty draws its own status row)".into()),
            "hooks" => {
                let Json::Obj(user_hooks) = v else {
                    warnings.push("your --settings hooks is not an object; ignored".into());
                    continue;
                };
                let Some(Json::Obj(mine)) = ours.get("hooks").cloned() else { continue };
                let mut merged = mine;
                for (ev, entries) in user_hooks {
                    let extra = match entries {
                        Json::Arr(a) => a,
                        other => vec![other],
                    };
                    match merged.iter_mut().find(|(e, _)| *e == ev) {
                        Some((_, Json::Arr(a))) => a.extend(extra),
                        _ => merged.push((ev, Json::Arr(extra))),
                    }
                }
                ours.set("hooks", Json::Obj(merged));
            }
            _ => ours.set(&k, v),
        }
    }
    (ours, warnings)
}

/// Removes `--settings X` / `--settings=X` from Claude Code's arguments;
/// returns the remaining arguments and X (the last one given).
pub fn split_user_settings(args: &[String]) -> (Vec<String>, Option<String>) {
    let mut rest = vec![];
    let mut value = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--" {
            rest.push(a.clone());
            rest.extend(it.cloned());
            break;
        }
        if a == "--settings" {
            value = it.next().cloned();
        } else if let Some(v) = a.strip_prefix("--settings=") {
            value = Some(v.to_string());
        } else {
            rest.push(a.clone());
        }
    }
    (rest, value)
}

/// The user's `--settings` value: inline JSON (starts with `{`) or a file.
pub fn load_user(value: &str, cwd: Option<&std::path::Path>) -> Result<Json, String> {
    let t = value.trim_start();
    if t.starts_with('{') {
        return Json::parse(t).map_err(|e| format!("--settings JSON: {e}"));
    }
    let p = std::path::Path::new(value);
    let p = match cwd {
        Some(c) if p.is_relative() => c.join(p),
        _ => p.to_path_buf(),
    };
    let text = std::fs::read_to_string(&p).map_err(|e| format!("--settings {}: {e}", p.display()))?;
    Json::parse(&text).map_err(|e| format!("--settings {}: {e}", p.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_have_exec_form_hooks_and_a_status_line() {
        let v = ours(r"C:\Program Files\haetty\haetty.exe", r"C:\Program Files\haetty\haetty.exe");
        let pre = v.at(&["hooks", "PreToolUse"]).and_then(Json::arr).unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0].get("matcher").and_then(Json::str), Some(EDIT_MATCHER));
        let h = &pre[0].get("hooks").and_then(Json::arr).unwrap()[0];
        assert_eq!(h.get("command").and_then(Json::str), Some("C:/Program Files/haetty/haetty.exe"));
        assert_eq!(h.get("args").unwrap().to_string(), r#"["hook","PreToolUse"]"#);
        assert_eq!(h.get("async"), None, "PreToolUse of edit tools is synchronous");
        let all = &pre[1].get("hooks").and_then(Json::arr).unwrap()[0];
        assert!(pre[1].get("matcher").is_none());
        assert_eq!(all.get("args").unwrap().to_string(), r#"["hook","PreToolUseAll"]"#);
        assert_eq!(all.get("async"), Some(&Json::Bool(true)));
        let stop =
            &v.at(&["hooks", "Stop"]).and_then(Json::arr).unwrap()[0].get("hooks").and_then(Json::arr).unwrap()[0];
        assert_eq!(stop.get("async"), Some(&Json::Bool(true)));
        let pr = &v.at(&["hooks", "PermissionRequest"]).and_then(Json::arr).unwrap()[0]
            .get("hooks")
            .and_then(Json::arr)
            .unwrap()[0];
        assert_eq!(pr.get("async"), None, "PermissionRequest is synchronous");
        for (ev, _, asynchronous, name) in EVENTS {
            assert_eq!(!asynchronous, crate::report::is_synchronous(name), "{ev}/{name}: settings and reporter agree");
        }
        assert_eq!(
            v.at(&["statusLine", "command"]).and_then(Json::str),
            Some(r#""C:/Program Files/haetty/haetty.exe" statusline"#)
        );
        assert_eq!(statusline_command("/home/a/.local/bin/haetty"), "/home/a/.local/bin/haetty statusline");
        assert_eq!(statusline_command("/x/a$b/haetty"), r#""/x/a\$b/haetty" statusline"#);
        // 8.3 short paths are plain: no quotes, so PowerShell runs them too.
        assert_eq!(statusline_command(r"C:\PROGRA~1\haetty\haetty.exe"), "C:/PROGRA~1/haetty/haetty.exe statusline");
        assert!(!is_plain("~/x") && !is_plain("") && is_plain("C:/Users/KIMTES~1/x.exe"));
        assert!(v.get("permissions").is_none() && v.get("env").is_none());
    }

    #[test]
    fn user_settings_merge_and_split() {
        let user = Json::parse(
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"mine"}]}],"Custom":[{"x":1}]},
                "statusLine":{"type":"command","command":"theirs"},"model":"opus","permissions":{"ask":["Bash(rm:*)"]}}"#,
        )
        .unwrap();
        let (m, w) = merge(ours("/bin/haetty", "/bin/haetty"), Some(user));
        assert_eq!(w.len(), 1);
        let stop = m.at(&["hooks", "Stop"]).and_then(Json::arr).unwrap();
        assert_eq!(stop.len(), 2, "ours first, theirs appended");
        assert!(stop[1].to_string().contains("mine"));
        assert!(m.at(&["hooks", "Custom"]).is_some());
        assert_eq!(m.at(&["statusLine", "command"]).and_then(Json::str), Some("/bin/haetty statusline"));
        assert_eq!(m.get("model").and_then(Json::str), Some("opus"));
        assert!(m.at(&["permissions", "ask"]).is_some());
        let args: Vec<String> = ["--model", "x", "--settings", "a.json", "--settings={}", "--", "--settings", "y"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (rest, v) = split_user_settings(&args);
        assert_eq!(v.as_deref(), Some("{}"));
        assert_eq!(rest, ["--model", "x", "--", "--settings", "y"]);
        assert!(load_user(" {\"a\":1}", None).is_ok());
        assert!(load_user("/nonexistent/x.json", None).is_err());
        assert!(is_claude("/usr/local/bin/claude") && is_claude(r"C:\x\Claude.EXE") && !is_claude("claude-dev"));
    }

    /// Windows: a path with spaces gets a plain 8.3 short path; without one
    /// (forced here), a copy in a space-free folder — `%ProgramData%` pointed
    /// at the test's run directory — refreshed when the executable changes.
    #[cfg(windows)]
    #[test]
    fn windows_status_line_path_without_spaces() {
        let d = haetty_testkit::run_dir("shell-safe");
        let dir = d.join("Program Files X").join("haetty");
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("haetty.exe");
        std::fs::write(&exe, b"v1").unwrap();
        let short = shell_safe_exe(&exe);
        assert!(is_plain(&exe_for_claude(&short)) && short.contains('~'), "{short}");
        assert_eq!(statusline_command(&short), format!("{} statusline", exe_for_claude(&short)));
        // The fallback copy (also used when the volume has no short names).
        let pd = d.join("programdata");
        std::fs::create_dir_all(&pd).unwrap();
        // SAFETY of the env change: only this test reads ProgramData.
        std::env::set_var("ProgramData", &pd);
        let copy = win::space_free_copy(&exe).expect("copy");
        assert!(is_plain(&exe_for_claude(&copy)), "{copy}");
        assert_eq!(std::fs::read(&copy).unwrap(), b"v1");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(&exe, b"v2-longer").unwrap();
        assert_eq!(win::space_free_copy(&exe).as_deref(), Some(copy.as_str()));
        assert_eq!(std::fs::read(&copy).unwrap(), b"v2-longer", "refreshed after the executable changed");
        std::env::remove_var("ProgramData");
        haetty_testkit::remove_dir_all(&d);
    }
}
