//! Whether a tab's foreground program is Claude Code (stage PH, H1): a `claude` that runs without haetty's
//! integration (the shell put another `claude` first on PATH, an absolute path or an alias) gets one sidebar line.
//! Linux only (`/proc`): the shell's terminal foreground process group (`tpgid` in `/proc/PID/stat`) and that
//! process's command line. Other systems: never (no line). The user's shell files are never read.

/// `tpgid` from the text of `/proc/PID/stat` (the field after `tty_nr`; the command name may hold spaces and
/// parentheses, so fields are counted after the last `)`).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))] // tests and Linux only
pub fn tpgid(stat: &str) -> Option<i32> {
    let rest = &stat[stat.rfind(')')? + 1..];
    // state ppid pgrp session tty_nr tpgid
    rest.split_whitespace().nth(5)?.parse().ok()
}

/// Whether a command line (NUL-separated, as `/proc/PID/cmdline`) is Claude Code: its program is named `claude`
/// (the native build is a link named so), or a script of the npm package runs under node.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))] // tests and Linux only
pub fn is_claude_cmdline(cmdline: &[u8]) -> bool {
    let mut words = cmdline.split(|b| *b == 0).filter(|w| !w.is_empty()).map(String::from_utf8_lossy);
    let base = |w: &str| w.rsplit(['/', '\\']).next().unwrap_or(w).to_ascii_lowercase();
    let Some(first) = words.next() else { return false };
    if matches!(base(&first).as_str(), "claude" | "claude.exe") {
        return true;
    }
    base(&first).starts_with("node") && words.next().is_some_and(|w| w.contains("@anthropic-ai/claude-code"))
}

/// Whether the foreground program of the terminal that `shell_pid` runs in is Claude Code.
#[cfg(target_os = "linux")]
pub fn claude_in_foreground(shell_pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{shell_pid}/stat")) else { return false };
    let Some(fg) = tpgid(&stat).filter(|p| *p > 0) else { return false };
    std::fs::read(format!("/proc/{fg}/cmdline")).is_ok_and(|c| is_claude_cmdline(&c))
}

#[cfg(not(target_os = "linux"))]
pub fn claude_in_foreground(_shell_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_tpgid_and_claude_command_lines() {
        assert_eq!(tpgid("1234 (zsh) S 1200 1234 1234 34817 5678 4194560 0 0"), Some(5678));
        assert_eq!(tpgid("1234 (a (b) c) S 1200 1234 1234 34817 -1 0"), Some(-1));
        assert_eq!(tpgid("garbage"), None);
        assert!(is_claude_cmdline(b"claude\0--resume\0abc\0"));
        assert!(is_claude_cmdline(b"/home/u/.local/bin/claude\0"));
        assert!(is_claude_cmdline(b"/usr/bin/node\0/usr/lib/node_modules/@anthropic-ai/claude-code/cli.js\0"));
        assert!(!is_claude_cmdline(b"vim\0claude.md\0"));
        assert!(!is_claude_cmdline(b"ssh\0host\0claude\0"), "claude inside ssh is out of scope (P6)");
        assert!(!is_claude_cmdline(b""));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_without_a_terminal_has_no_claude_in_front() {
        assert!(!claude_in_foreground(std::process::id()));
    }
}
