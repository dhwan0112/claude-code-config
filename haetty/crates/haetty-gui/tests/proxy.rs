//! The `claude` proxy (stage PH, H1): haetty started under the name `claude` from the proxy folder runs the real
//! claude with the same arguments plus the session's `--settings`, only when a person runs a conversation in a
//! terminal. A fake real claude records what it got. Unix only (sh, `script` for a terminal).
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_haetty");

struct Setup {
    dir: PathBuf,
    proxy: PathBuf,
    real: PathBuf,
    settings: PathBuf,
    out: PathBuf,
}

fn setup(name: &str) -> Setup {
    let dir = haetty_testkit::run_dir(name);
    let proxy = dir.join("rt/bin");
    let real = dir.join("real");
    std::fs::create_dir_all(&proxy).unwrap();
    std::fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink(BIN, proxy.join("claude")).unwrap();
    let out = dir.join("out.txt");
    // The fake real claude: its pid, the recursion marker and its arguments, one per line; exit code 7.
    let fake = format!(
        "#!/bin/sh\n{{ echo \"pid=$$\"; echo \"proxied=$HAETTY_PROXIED\"; echo \"path=$PATH\"; for a in \"$@\"; do echo \"arg=$a\"; done; }} > {}\nexit 7\n",
        out.display()
    );
    std::fs::write(real.join("claude"), fake).unwrap();
    std::fs::set_permissions(real.join("claude"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let settings = dir.join("rt/settings-s1.json");
    std::fs::write(&settings, r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"x"}]}]},"statusLine":{"type":"command","command":"y"}}"#).unwrap();
    Setup { dir, proxy, real, settings, out }
}

impl Setup {
    /// Runs `claude ARGS` through the proxy; `tty` runs it under `script` (a terminal on stdout). Returns the exit code
    /// and the fake's record.
    fn run(&self, args: &[&str], tty: bool, settings: bool, extra: &[(&str, &str)]) -> (i32, Vec<String>) {
        let _ = std::fs::remove_file(&self.out);
        let path = format!("{}:{}:/usr/bin:/bin", self.proxy.display(), self.real.display());
        let quoted: Vec<String> = args.iter().map(|a| format!("'{}'", a.replace('\'', "'\\''"))).collect();
        let line = format!("claude {}", quoted.join(" "));
        let mut c = if tty {
            let mut c = Command::new("script");
            c.args(["-qec", &line, "/dev/null"]);
            c
        } else {
            let mut c = Command::new("/bin/sh");
            c.args(["-c", &line]);
            c
        };
        c.env("PATH", &path).env("HAETTY_PROXY_DIR", &self.proxy).env_remove("HAETTY_PROXIED");
        if settings {
            c.env("HAETTY_CLAUDE_SETTINGS", &self.settings);
        } else {
            c.env_remove("HAETTY_CLAUDE_SETTINGS");
        }
        for (k, v) in extra {
            c.env(k, v);
        }
        let st = c.current_dir(&self.dir).output().unwrap();
        let rec = std::fs::read_to_string(&self.out).unwrap_or_default().lines().map(str::to_string).collect();
        (st.status.code().unwrap_or(-1), rec)
    }
}

fn args_of(rec: &[String]) -> Vec<&str> {
    rec.iter().filter_map(|l| l.strip_prefix("arg=")).collect()
}

#[test]
fn injects_the_settings_into_a_conversation_and_passes_the_rest() {
    let s = setup("proxy-inject");
    let set = s.settings.to_string_lossy().into_owned();
    let (code, rec) = s.run(&["--model", "haiku", "fix it"], true, true, &[]);
    assert_eq!(code, 7, "the real claude's exit code: {rec:?}");
    assert_eq!(args_of(&rec), ["--model", "haiku", "fix it", "--settings", set.as_str()]);
    assert!(rec.contains(&"proxied=1".to_string()), "marked for recursion: {rec:?}");
    let path = rec.iter().find_map(|l| l.strip_prefix("path=")).unwrap();
    assert!(!path.split(':').any(|p| Path::new(p) == s.proxy), "the proxy folder is off PATH: {path}");

    // Not injected: non-interactive, a subcommand, not a terminal, integration off, inside a proxied claude.
    for (args, tty, settings, extra) in [
        (vec!["-p", "hi"], true, true, vec![]),
        (vec!["mcp", "list"], true, true, vec![]),
        (vec!["update"], true, true, vec![]),
        (vec!["fix it"], false, true, vec![]),
        (vec!["fix it"], true, false, vec![]),
        (vec!["fix it"], true, true, vec![("HAETTY_PROXIED", "1")]),
    ] {
        let (code, rec) = s.run(&args, tty, settings, &extra);
        assert_eq!(code, 7);
        assert_eq!(args_of(&rec), args, "passed unchanged: {args:?} tty={tty} settings={settings} {extra:?}");
    }
    haetty_testkit::remove_dir_all(&s.dir);
}

#[test]
fn merges_the_users_settings_and_execs_the_real_claude() {
    let s = setup("proxy-merge");
    let (code, rec) = s.run(&["--settings", r#"{"model":"x","hooks":{"Stop":[]}}"#, "hi"], true, true, &[]);
    assert_eq!(code, 7);
    let a = args_of(&rec);
    assert_eq!(a[..1], ["hi"], "the user's --settings is taken out: {a:?}");
    assert_eq!(a[1], "--settings");
    let merged = Path::new(a[2]);
    assert!(merged.file_name().unwrap().to_string_lossy().starts_with("settings-s1-p"), "{merged:?}");
    let text = std::fs::read_to_string(merged).unwrap();
    assert!(text.contains("\"model\"") && text.contains("statusLine"), "ours and the user's keys: {text}");
    assert_eq!(std::fs::metadata(merged).unwrap().permissions().mode() & 0o777, 0o600);
    // exec: the real claude runs as the proxy's own process (same pid), so signals and the exit code are its own.
    let _ = std::fs::remove_file(&s.out);
    let mut child = Command::new(s.proxy.join("claude"))
        .args(["-p", "x"])
        .env("PATH", format!("{}:{}:/usr/bin:/bin", s.proxy.display(), s.real.display()))
        .env("HAETTY_PROXY_DIR", &s.proxy)
        .env_remove("HAETTY_PROXIED")
        .spawn()
        .unwrap();
    let pid = child.id();
    assert_eq!(child.wait().unwrap().code(), Some(7));
    let rec = std::fs::read_to_string(&s.out).unwrap();
    assert!(rec.lines().any(|l| l == format!("pid={pid}")), "same process: {pid} vs {rec}");
    haetty_testkit::remove_dir_all(&s.dir);
}

#[test]
fn a_missing_real_claude_is_command_not_found() {
    let s = setup("proxy-missing");
    std::fs::remove_file(s.real.join("claude")).unwrap();
    let st = Command::new(s.proxy.join("claude"))
        .env("PATH", format!("{}:{}", s.proxy.display(), s.real.display()))
        .env("HAETTY_PROXY_DIR", &s.proxy)
        .output()
        .unwrap();
    assert_eq!(st.status.code(), Some(127));
    assert_eq!(String::from_utf8_lossy(&st.stderr).trim(), "haetty: claude: command not found");
    // The proxy never finds itself: a PATH with only the proxy folder (even under another name) has no claude.
    let st = Command::new(s.proxy.join("claude")).env("PATH", s.proxy.display().to_string()).output().unwrap();
    assert_eq!(st.status.code(), Some(127));
    haetty_testkit::remove_dir_all(&s.dir);
}
