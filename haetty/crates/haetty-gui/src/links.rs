//! Hyperlinks (OSC 8) the person opens with Ctrl+click (DESIGN.md §2.2):
//! only `http`, `https` and `file` targets, with the operating system's
//! default program, never through a shell. A link's text and its target can
//! differ, so the GUI shows the target (`ui::link_row`) before a click can
//! open it. Targets are shown text: never logged (only their scheme is).

use std::io;

/// The schemes that open.
pub const SCHEMES: [&str; 3] = ["http", "https", "file"];

/// The target's scheme, lower case (RFC 3986: a letter, then letters,
/// digits, `+`, `-`, `.`, up to the colon).
pub fn scheme(uri: &str) -> Option<String> {
    let (s, _) = uri.split_once(':')?;
    let mut cs = s.chars();
    let first = cs.next()?;
    (first.is_ascii_alphabetic() && cs.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
        .then(|| s.to_ascii_lowercase())
}

/// Whether the target may be opened: an allowed scheme and no control
/// characters. `Err` holds what to tell the person.
pub fn check(uri: &str) -> Result<(), String> {
    match scheme(uri) {
        Some(s) if SCHEMES.contains(&s.as_str()) => {
            if uri.chars().any(|c| c.is_control()) {
                Err("not opened (control characters in it)".into())
            } else {
                Ok(())
            }
        }
        Some(_) | None => Err("not opened (only http, https, file)".into()),
    }
}

/// Opens an allowed target with the default program. `HAETTY_LINK_OPENER`
/// (tests) names a program run with the target as its only argument instead.
pub fn open(uri: &str) -> io::Result<()> {
    check(uri).map_err(io::Error::other)?;
    if let Some(p) = std::env::var_os("HAETTY_LINK_OPENER").filter(|p| !p.is_empty()) {
        return spawn(std::process::Command::new(p).arg(uri));
    }
    open_default(uri)
}

fn spawn(cmd: &mut std::process::Command) -> io::Result<()> {
    use std::process::Stdio;
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    // Reaped in the background: the opener may wait for the program.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(not(windows))]
fn open_default(uri: &str) -> io::Result<()> {
    spawn(std::process::Command::new("xdg-open").arg(uri))
}

#[cfg(windows)]
fn open_default(uri: &str) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: *mut std::ffi::c_void,
            op: *const u16,
            file: *const u16,
            params: *const u16,
            dir: *const u16,
            show: i32,
        ) -> isize;
    }
    let w = |s: &str| std::ffi::OsStr::new(s).encode_wide().chain([0]).collect::<Vec<u16>>();
    let (op, file) = (w("open"), w(uri));
    // SW_SHOWNORMAL; a result above 32 is success.
    let r = unsafe {
        ShellExecuteW(std::ptr::null_mut(), op.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), 1)
    };
    if r > 32 {
        Ok(())
    } else {
        Err(io::Error::other(format!("ShellExecuteW: {r}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_http_https_and_file_open() {
        for ok in ["http://example.com", "HTTPS://example.com/a?b#c", "file:///tmp/x.txt", "File://host/share"] {
            assert_eq!(check(ok), Ok(()), "{ok}");
        }
        for bad in [
            "javascript:alert(1)",
            "ssh://host",
            "mailto:a@b",
            "/etc/passwd",
            "example.com",
            "1http://x",
            "http://a\u{7}b",
            "",
        ] {
            assert!(check(bad).is_err(), "{bad}");
        }
        assert_eq!(scheme("vscode-insiders+x://a").as_deref(), Some("vscode-insiders+x"));
    }

    #[test]
    fn refused_targets_are_not_run() {
        // A refused target fails before any program starts.
        assert!(open("ssh://host").is_err());
        assert!(open("javascript:alert(1)").is_err());
    }
}
