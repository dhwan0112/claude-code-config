//! Shell integration (DESIGN.md §13.8, stage PI): a zsh or bash shell tab keeps the `claude` proxy folder first on
//! `PATH` after the person's own startup files, which zsh and bash still read once each, in their own order. A real
//! `haetty daemon` runs real shells in isolated homes whose startup files log their names; a fake `claude` and `haetty`
//! sit in `~/.local/bin`, which the startup files put first. Unix only; a missing zsh or bash fails the test.
#![cfg(unix)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use haetty_proto::paths::Endpoint;
use haetty_proto::wire::{CreateSpec, Grid, Origin};
use haetty_proto::Client;

struct Daemon {
    dir: PathBuf,
    child: Child,
    client: Client,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(name: &str) -> Daemon {
    let dir = haetty_testkit::run_dir(name);
    let ep: Endpoint = common::endpoint(&dir, name);
    let mut c = Command::new(common::bin());
    c.arg("daemon")
        .env("HAETTY_INSTANCE", "dev")
        .env("HAETTY_SOCKET", &ep.0)
        .env("HAETTY_STATE_DIR", dir.join("state"))
        .env("XDG_RUNTIME_DIR", dir.join("rt"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for v in ["ZDOTDIR", "PROMPT_COMMAND", "HAETTY_PROXY_DIR", "HAETTY_CLAUDE_SETTINGS", "HAETTY_PROXIED", "ENV"] {
        c.env_remove(v);
    }
    let child = c.spawn().expect("start daemon");
    let client = common::connect(&ep, "shell-test");
    Daemon { dir, child, client }
}

fn which(prog: &str) -> String {
    for d in ["/usr/bin", "/bin", "/usr/local/bin"] {
        let p = Path::new(d).join(prog);
        if p.exists() {
            return p.to_string_lossy().into_owned();
        }
    }
    panic!("{prog} is needed for this test and was not found in /usr/bin, /bin or /usr/local/bin");
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn exe(path: &Path, text: &str) {
    write(path, text);
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A home with a fake `claude` and `haetty` in `~/.local/bin`.
fn home(d: &Daemon, case: &str) -> PathBuf {
    let h = d.dir.join(case);
    exe(&h.join(".local/bin/claude"), "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOME/claude-args\"\n");
    exe(&h.join(".local/bin/haetty"), "#!/bin/sh\nexit 3\n");
    h
}

fn log_line(name: &str) -> String {
    format!("echo {name} >> \"$HOME/order\"\n")
}

const PREPEND: &str = "PATH=$HOME/.opencode/bin:$HOME/.local/bin:$PATH\n";

const ZSH_PROBE: &str = "print -rl -- \"first=${PATH%%:*}\" \"count=${#${(@M)path:#$HAETTY_PROXY_DIR}}\" \
     \"claude=$(whence -p claude)\" \"haetty=$(whence -p haetty)\" \"zdotdir=${ZDOTDIR-unset}\" \
     \"saved=${HAETTY_ZSH_ZDOTDIR-unset}\" > $HOME/result.new && mv $HOME/result.new $HOME/result";

const BASH_PROBE: &str = "false; s=$?; c=0; IFS=:; for p in $PATH; do [ \"$p\" = \"$HAETTY_PROXY_DIR\" ] && c=$((c+1)); done; \
     unset IFS; printf '%s\\n' \"first=${PATH%%:*}\" \"count=$c\" \"claude=$(type -P claude)\" \"status=$s\" > $HOME/result.new \
     && mv $HOME/result.new $HOME/result";

struct Run {
    home: PathBuf,
    id: String,
}

fn shell(d: &Daemon, home: &Path, program: &str, args: &[&str], env: &[(&str, String)], integration: bool) -> Run {
    let mut e = vec![
        ("HOME".to_string(), home.to_string_lossy().into_owned()),
        ("PATH".to_string(), "/usr/bin:/bin".to_string()),
        ("TERM".to_string(), "xterm-256color".to_string()),
    ];
    e.extend(env.iter().map(|(k, v)| (k.to_string(), v.clone())));
    let id = d
        .client
        .create(CreateSpec {
            program: program.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            cwd: Some(home.to_string_lossy().into_owned()),
            env: e,
            size: Grid { cols: 200, rows: 10, cell_width: 9, cell_height: 18 },
            scrollback_limit: 100_000,
            default_fg: [200, 200, 200],
            default_bg: [20, 20, 20],
            palette: vec![],
            cursor_color: None,
            claude_integration: true,
            shell_integration: integration,
            ide_integration: false,
            engine: String::new(),
        })
        .unwrap();
    Run { home: home.to_path_buf(), id }
}

impl Run {
    /// Types `line` and waits for `file` in the home (written last by the line); its `key=value` lines.
    fn probe(&self, d: &Daemon, line: &str, file: &str) -> Vec<(String, String)> {
        let path = self.home.join(file);
        let _ = std::fs::remove_file(&path);
        let s = d.client.attach(&self.id, None).unwrap();
        s.handle.write(Origin::Human, format!("{line}\r").into_bytes());
        let end = Instant::now() + Duration::from_secs(20);
        while !path.exists() {
            assert!(Instant::now() < end, "{}: no {file}", self.home.display());
            std::thread::sleep(Duration::from_millis(20));
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = d.client.detach(&self.id);
        text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn order(&self) -> Vec<String> {
        std::fs::read_to_string(self.home.join("order")).unwrap_or_default().lines().map(String::from).collect()
    }

    fn claude_args(&self, d: &Daemon) -> Vec<String> {
        let path = self.home.join("claude-args");
        let _ = std::fs::remove_file(&path);
        let s = d.client.attach(&self.id, None).unwrap();
        s.handle.write(Origin::Human, b"claude\r".to_vec());
        let end = Instant::now() + Duration::from_secs(20);
        while std::fs::read_to_string(&path).map_or(true, |t| t.is_empty()) {
            assert!(Instant::now() < end, "{}: claude did not run", self.home.display());
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = d.client.detach(&self.id);
        std::fs::read_to_string(&path).unwrap().lines().map(String::from).collect()
    }
}

fn get<'a>(r: &'a [(String, String)], k: &str) -> &'a str {
    r.iter().find(|(x, _)| x == k).map_or("", |(_, v)| v.as_str())
}

#[test]
fn zsh_keeps_the_proxy_first_after_the_person_s_startup_files() {
    let zsh = which("zsh");
    let d = start("shell-zsh");
    let bin = d.dir.join("rt/bin");
    let proxy = bin.to_string_lossy().into_owned();
    let local = |h: &Path| h.join(".local/bin").to_string_lossy().into_owned();
    let theirs = |h: &Path| h.join(".opencode/bin").to_string_lossy().into_owned();
    let user_files = |h: &Path| {
        write(&h.join(".zshenv"), &log_line("zshenv"));
        write(&h.join(".zprofile"), &log_line("zprofile"));
        write(&h.join(".zshrc"), &format!("{}{PREPEND}", log_line("zshrc")));
        write(&h.join(".zlogin"), &log_line("zlogin"));
    };

    // The person's .zshrc puts ~/.local/bin first: claude and haetty are the proxy folder's again at the prompt.
    let h = home(&d, "plain");
    user_files(&h);
    let r = shell(&d, &h, &zsh, &[], &[], true);
    let p = r.probe(&d, ZSH_PROBE, "result");
    assert_eq!(get(&p, "first"), proxy);
    assert_eq!(get(&p, "count"), "1", "the proxy folder once on PATH");
    assert_eq!(get(&p, "claude"), format!("{proxy}/claude"));
    assert_eq!(get(&p, "haetty"), format!("{proxy}/haetty"));
    assert_eq!(std::fs::read_link(bin.join("haetty")).unwrap(), std::fs::read_link(bin.join("claude")).unwrap());
    assert_eq!((get(&p, "zdotdir"), get(&p, "saved")), ("unset", "unset"), "ZDOTDIR as the person had it");
    assert_eq!(r.order(), ["zshenv", "zshrc"], "each file once, in zsh's order");
    let args = r.claude_args(&d);
    assert!(args.iter().any(|a| a == "--settings"), "claude typed in the tab went through the proxy: {args:?}");
    let p = r.probe(&d, &format!("PATH=$HOME/.local/bin:$PATH; {ZSH_PROBE}"), "result");
    assert_eq!(get(&p, "first"), local(&h), "the line itself ran with the person's PATH");
    let p = r.probe(&d, ZSH_PROBE, "result");
    assert_eq!((get(&p, "first"), get(&p, "count")), (proxy.as_str(), "1"), "the next prompt moved it back");

    // Off: today's behaviour, and the same files in the same order.
    let h = home(&d, "off");
    user_files(&h);
    let r = shell(&d, &h, &zsh, &[], &[], false);
    let p = r.probe(&d, ZSH_PROBE, "result");
    assert_eq!(get(&p, "first"), theirs(&h));
    assert_eq!(get(&p, "zdotdir"), "unset");
    assert_eq!(r.order(), ["zshenv", "zshrc"]);

    // A shell command with arguments is left alone.
    let h = home(&d, "args");
    user_files(&h);
    let r = shell(&d, &h, &zsh, &["-i"], &[], true);
    let p = r.probe(&d, ZSH_PROBE, "result");
    assert_eq!((get(&p, "first"), get(&p, "zdotdir")), (theirs(&h).as_str(), "unset"));

    // The person's own ZDOTDIR: its files are read, not the home's, and ZDOTDIR is theirs at the prompt.
    let h = home(&d, "zdotdir");
    let z = h.join("zdot");
    write(&h.join(".zshenv"), &log_line("home-zshenv"));
    write(&z.join(".zshenv"), &log_line("zdot-zshenv"));
    write(&z.join(".zshrc"), &format!("{}{PREPEND}", log_line("zdot-zshrc")));
    let r = shell(&d, &h, &zsh, &[], &[("ZDOTDIR", z.to_string_lossy().into_owned())], true);
    let p = r.probe(&d, ZSH_PROBE, "result");
    assert_eq!((get(&p, "first"), get(&p, "count")), (proxy.as_str(), "1"));
    assert_eq!((get(&p, "zdotdir"), get(&p, "saved")), (z.to_string_lossy().as_ref(), "unset"));
    assert_eq!(r.order(), ["zdot-zshenv", "zdot-zshrc"]);

    // A .zshenv that sets ZDOTDIR itself: zsh goes on in that folder.
    let h = home(&d, "zshenv-sets");
    let cfg = h.join("cfg");
    write(&h.join(".zshenv"), &format!("{}ZDOTDIR=$HOME/cfg\n", log_line("home-zshenv")));
    write(&cfg.join(".zshrc"), &format!("{}{PREPEND}", log_line("cfg-zshrc")));
    let r = shell(&d, &h, &zsh, &[], &[], true);
    let p = r.probe(&d, ZSH_PROBE, "result");
    assert_eq!((get(&p, "first"), get(&p, "zdotdir")), (proxy.as_str(), cfg.to_string_lossy().as_ref()));
    assert_eq!(r.order(), ["home-zshenv", "cfg-zshrc"]);

    // No .zshenv at all.
    let h = home(&d, "no-zshenv");
    write(&h.join(".zshrc"), &format!("{}{PREPEND}", log_line("zshrc")));
    let r = shell(&d, &h, &zsh, &[], &[], true);
    let p = r.probe(&d, ZSH_PROBE, "result");
    assert_eq!((get(&p, "first"), get(&p, "count")), (proxy.as_str(), "1"));
    assert_eq!(r.order(), ["zshrc"]);

    // A .zshenv that writes the whole PATH (dropping the proxy folder).
    let h = home(&d, "zshenv-path");
    write(&h.join(".zshenv"), &format!("{}PATH=$HOME/.local/bin:/usr/bin:/bin\n", log_line("zshenv")));
    write(&h.join(".zshrc"), &log_line("zshrc"));
    let r = shell(&d, &h, &zsh, &[], &[], true);
    let p = r.probe(&d, ZSH_PROBE, "result");
    assert_eq!((get(&p, "first"), get(&p, "count")), (proxy.as_str(), "1"));

    // Frameworks' hooks: add-zsh-hook (oh-my-zsh) and a hook that moves itself last at every prompt
    // (powerlevel10k's instant prompt does) leave the proxy first.
    let h = home(&d, "frameworks");
    write(
        &h.join(".zshrc"),
        &format!(
            "{}{PREPEND}autoload -Uz add-zsh-hook\nomz_title() {{ :; }}\nadd-zsh-hook precmd omz_title\n\
             p10k_last() {{ precmd_functions=(${{precmd_functions:#p10k_last}} p10k_last); }}\n\
             precmd_functions+=(p10k_last)\n",
            log_line("zshrc")
        ),
    );
    let r = shell(&d, &h, &zsh, &[], &[], true);
    for _ in 0..2 {
        let p = r.probe(&d, ZSH_PROBE, "result");
        assert_eq!((get(&p, "first"), get(&p, "count")), (proxy.as_str(), "1"));
    }

    // The known limit: a .zshrc that empties precmd_functions removes the hook too (the sidebar then names the
    // claude that runs without the integration).
    let h = home(&d, "cleared");
    write(&h.join(".zshrc"), &format!("{PREPEND}precmd_functions=()\n"));
    let r = shell(&d, &h, &zsh, &[], &[], true);
    let p = r.probe(&d, ZSH_PROBE, "result");
    assert_eq!(get(&p, "first"), h.join(".opencode/bin").to_string_lossy());
    let dir = d.dir.clone();
    drop(d);
    haetty_testkit::remove_dir_all(&dir);
}

#[test]
fn bash_keeps_the_proxy_first_and_the_exit_status() {
    let bash = which("bash");
    let d = start("shell-bash");
    let proxy = d.dir.join("rt/bin").to_string_lossy().into_owned();
    let h = home(&d, "plain");
    write(
        &h.join(".bashrc"),
        &format!("{}{PREPEND}PROMPT_COMMAND=\"history -a; $PROMPT_COMMAND\"\n", log_line("bashrc")),
    );
    write(&h.join(".bash_profile"), &log_line("bash_profile"));
    let r = shell(&d, &h, &bash, &[], &[], true);
    let p = r.probe(&d, BASH_PROBE, "result");
    assert_eq!((get(&p, "first"), get(&p, "count")), (proxy.as_str(), "1"));
    assert_eq!(get(&p, "claude"), format!("{proxy}/claude"));
    assert_eq!(get(&p, "status"), "1");
    assert_eq!(r.order(), ["bashrc"]);
    let args = r.claude_args(&d);
    assert!(args.iter().any(|a| a == "--settings"), "{args:?}");
    let s = d.client.attach(&r.id, None).unwrap();
    s.handle.write(Origin::Human, b"false\r".to_vec());
    let _ = d.client.detach(&r.id);
    let p = r.probe(&d, "echo \"status=$?\" > $HOME/st.new && mv $HOME/st.new $HOME/st", "st");
    assert_eq!(get(&p, "status"), "1", "the hook keeps $? for the person's prompt");

    let h = home(&d, "off");
    write(&h.join(".bashrc"), PREPEND);
    let r = shell(&d, &h, &bash, &[], &[], false);
    let p = r.probe(&d, BASH_PROBE, "result");
    assert_eq!(get(&p, "first"), h.join(".opencode/bin").to_string_lossy());
    let dir = d.dir.clone();
    drop(d);
    haetty_testkit::remove_dir_all(&dir);
}
