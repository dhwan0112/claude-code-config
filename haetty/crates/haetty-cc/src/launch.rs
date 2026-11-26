//! The arguments a `claude` typed in a shell tab was started with, as far as they belong to the conversation and not
//! to one run of it: kept with the tab set and given again when the conversation is resumed (`claude ARGS --resume
//! ID`). Dropped: the first prompt (positional arguments, `-p`), the resume and continue requests and the session id
//! (a new `--resume ID` replaces them), `--settings` (haetty injects its own and merges the user's), and an argument
//! whose value looks like a key or a token. An argument this table does not know is kept, its value too.

/// How Claude Code (`claude --help`, 2.1.332) reads an option's value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Takes {
    None,
    One,
    Maybe,
    Many,
}

const OPTIONS: &[(&str, Takes)] = &[
    ("--add-dir", Takes::Many),
    ("--agent", Takes::One),
    ("--agents", Takes::One),
    ("--allow-dangerously-skip-permissions", Takes::None),
    ("--allowedTools", Takes::Many),
    ("--allowed-tools", Takes::Many),
    ("--append-system-prompt", Takes::One),
    ("--autocompact", Takes::One),
    ("--ax-screen-reader", Takes::None),
    ("--bare", Takes::None),
    ("--betas", Takes::Many),
    ("--brief", Takes::None),
    ("--chrome", Takes::None),
    ("--cloud", Takes::Maybe),
    ("--dangerously-skip-permissions", Takes::None),
    ("--debug", Takes::Maybe),
    ("-d", Takes::Maybe),
    ("--debug-file", Takes::One),
    ("--disable-slash-commands", Takes::None),
    ("--disallowedTools", Takes::Many),
    ("--disallowed-tools", Takes::Many),
    ("--effort", Takes::One),
    ("--environment", Takes::One),
    ("--exclude-dynamic-system-prompt-sections", Takes::None),
    ("--fallback-model", Takes::One),
    ("--file", Takes::Many),
    ("--forward-subagent-text", Takes::None),
    ("--ide", Takes::None),
    ("--include-hook-events", Takes::None),
    ("--include-partial-messages", Takes::None),
    ("--input-format", Takes::One),
    ("--json-schema", Takes::One),
    ("--max-budget-usd", Takes::One),
    ("--mcp-config", Takes::Many),
    ("--model", Takes::One),
    ("--name", Takes::One),
    ("-n", Takes::One),
    ("--no-chrome", Takes::None),
    ("--no-session-persistence", Takes::None),
    ("--output-format", Takes::One),
    ("--permission-mode", Takes::One),
    ("--permission-prompts", Takes::One),
    ("--plugin-dir", Takes::One),
    ("--plugin-url", Takes::One),
    ("--prompt-suggestions", Takes::Maybe),
    ("--remote-control", Takes::Maybe),
    ("--remote-control-session-name-prefix", Takes::One),
    ("--replay-user-messages", Takes::None),
    ("--restricted", Takes::None),
    ("--safe-mode", Takes::None),
    ("--setting-sources", Takes::One),
    ("--strict-mcp-config", Takes::None),
    ("--system-prompt", Takes::One),
    ("--system-prompt-snapshot", Takes::One),
    ("--tmux", Takes::None),
    ("--tools", Takes::Many),
    ("--verbose", Takes::None),
    ("--worktree", Takes::Maybe),
    ("-w", Takes::Maybe),
];

/// Options that belong to one run, not to the conversation: dropped with their values.
const DROPPED: &[(&str, Takes)] = &[
    ("--resume", Takes::Maybe),
    ("-r", Takes::Maybe),
    ("--continue", Takes::None),
    ("-c", Takes::None),
    ("--session-id", Takes::One),
    ("--fork-session", Takes::None),
    ("--from-pr", Takes::Maybe),
    ("--teleport", Takes::Maybe),
    ("--settings", Takes::One),
    ("--print", Takes::None),
    ("-p", Takes::None),
    ("--bg", Takes::None),
    ("--background", Takes::None),
];

fn takes(table: &[(&str, Takes)], name: &str) -> Option<Takes> {
    table.iter().find(|(n, _)| *n == name).map(|(_, t)| *t)
}

/// A value shaped like a key, a token or a password.
pub fn looks_secret(v: &str) -> bool {
    let low = v.to_ascii_lowercase();
    let prefixes = ["sk-", "ghp_", "gho_", "ghs_", "github_pat_", "glpat-", "xoxb-", "xoxp-", "akia", "aiza"];
    let words = ["token", "secret", "password", "passwd", "apikey", "api_key", "api-key", "bearer ", "authorization"];
    if prefixes.iter().any(|p| low.starts_with(p)) || words.iter().any(|w| low.contains(w)) {
        return true;
    }
    let long = v.len() >= 32 && v.chars().all(|c| c.is_ascii_alphanumeric() || "-_+/=".contains(c));
    long && v.chars().any(|c| c.is_ascii_digit()) && v.chars().any(|c| c.is_ascii_alphabetic())
}

/// The arguments kept for resuming the conversation, in their order.
pub fn kept(args: &[String]) -> Vec<String> {
    let mut out = vec![];
    let mut k = 0;
    while k < args.len() {
        let a = &args[k];
        k += 1;
        if a == "--" {
            break;
        }
        if !a.starts_with('-') || a == "-" {
            continue;
        }
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v)),
            _ => (a.as_str(), None),
        };
        let (drop, how) = match (takes(DROPPED, name), takes(OPTIONS, name)) {
            (Some(t), _) => (true, t),
            (None, Some(t)) => (false, t),
            (None, None) => (false, Takes::Maybe),
        };
        let mut values: Vec<&String> = vec![];
        if inline.is_none() {
            let more = |k: usize| args.get(k).is_some_and(|v| !v.starts_with('-'));
            match how {
                Takes::None => {}
                Takes::One => {
                    if k < args.len() {
                        values.push(&args[k]);
                        k += 1;
                    }
                }
                Takes::Maybe => {
                    if more(k) {
                        values.push(&args[k]);
                        k += 1;
                    }
                }
                Takes::Many => {
                    while more(k) {
                        values.push(&args[k]);
                        k += 1;
                    }
                }
            }
        }
        let secret = inline.is_some_and(looks_secret) || values.iter().any(|v| looks_secret(v));
        if drop || secret {
            continue;
        }
        out.push(a.clone());
        out.extend(values.into_iter().cloned());
    }
    out
}

/// The arguments turn permission checks off (`--dangerously-skip-permissions`, `--permission-mode bypassPermissions`).
pub fn bypasses(args: &[String]) -> bool {
    args.iter().enumerate().any(|(k, a)| {
        a == "--dangerously-skip-permissions"
            || a == "--permission-mode=bypassPermissions"
            || a == "--permission-mode" && args.get(k + 1).is_some_and(|v| v == "bypassPermissions")
    })
}

/// One word for a POSIX shell, fish included: plain when it is safe, else in single quotes.
pub fn quote(s: &str) -> String {
    let plain = !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:,@%+".contains(c));
    if plain {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// The line typed into a restored shell tab: `claude ARGS --resume ID`.
pub fn resume_line(args: &[String], id: &str) -> String {
    let mut line = String::from("claude");
    for a in args {
        line.push(' ');
        line.push_str(&quote(a));
    }
    line.push_str(" --resume ");
    line.push_str(&quote(id));
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn options_keep_their_order_and_values_and_the_prompt_goes() {
        let a = v(&[
            "--model",
            "opus",
            "--add-dir",
            "/w/one",
            "/w/my dir 한글",
            "--permission-mode=acceptEdits",
            "--dangerously-skip-permissions",
            "-d",
            "fix the parser",
        ]);
        assert_eq!(
            kept(&a),
            v(&[
                "--model",
                "opus",
                "--add-dir",
                "/w/one",
                "/w/my dir 한글",
                "--permission-mode=acceptEdits",
                "--dangerously-skip-permissions",
                "-d",
                "fix the parser"
            ]),
            "-d takes an optional value: what Claude Code reads as its value stays with it"
        );
        assert_eq!(kept(&v(&["--verbose", "fix the parser"])), v(&["--verbose"]), "the first prompt goes");
        assert_eq!(kept(&v(&["--model", "opus", "--", "-x prompt"])), v(&["--model", "opus"]));
    }

    #[test]
    fn resume_continue_session_settings_and_print_go_with_their_values() {
        let a = v(&[
            "--resume",
            "old",
            "-c",
            "--session-id",
            "u-1",
            "--settings",
            "/u/s.json",
            "--fork-session",
            "--from-pr",
            "12",
            "--model",
            "haiku",
            "-r",
            "--resume=x",
            "--settings={\"a\":1}",
        ]);
        assert_eq!(kept(&a), v(&["--model", "haiku"]));
    }

    #[test]
    fn unknown_options_stay_with_their_values_and_secrets_go() {
        assert_eq!(kept(&v(&["--advisor", "opus", "--new-flag"])), v(&["--advisor", "opus", "--new-flag"]));
        let a =
            v(&["--mcp-config", "{\"headers\":{\"Authorization\":\"x\"}}", "--api-key=sk-ant-abc", "--model", "opus"]);
        assert_eq!(kept(&a), v(&["--model", "opus"]));
        assert!(looks_secret("ghp_0123456789abcdefghijklmnopqrstuvwxyzAB"));
        assert!(looks_secret("a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7"));
        assert!(!looks_secret("/home/me/projects/very-long-folder-name-for-tests"));
        assert!(!looks_secret("acceptEdits"));
    }

    #[test]
    fn the_resume_line_quotes_for_the_shell_and_bypass_is_seen() {
        let a = v(&["--add-dir", "/w/my dir 한글", "--name", "it's", "--model", "opus"]);
        assert_eq!(
            resume_line(&a, "0f0f-1"),
            "claude --add-dir '/w/my dir 한글' --name 'it'\\''s' --model opus --resume 0f0f-1"
        );
        assert!(bypasses(&v(&["--dangerously-skip-permissions"])));
        assert!(bypasses(&v(&["--permission-mode", "bypassPermissions"])));
        assert!(bypasses(&v(&["--permission-mode=bypassPermissions"])));
        assert!(!bypasses(&v(&["--permission-mode", "plan", "--allow-dangerously-skip-permissions"])));
    }
}
