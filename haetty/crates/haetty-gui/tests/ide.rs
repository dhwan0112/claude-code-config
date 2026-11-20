//! haetty as Claude Code's editor (DESIGN.md §13.9, stage P3c): a real `haetty daemon`, a fake `claude` that records
//! its `CLAUDE_CODE_SSE_PORT`, and this test as Claude Code on the WebSocket, as Claude Code 2.1.329 speaks it. Proposals
//! reach a Control client and are answered only by a person's input; the lock file lives as long as the session.
//! Unix only.
#![cfg(unix)]

mod common;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use haetty_cc::json::Json;
use haetty_proto::paths::Endpoint;
use haetty_proto::wire::{Attention, ClientMsg, CreateSpec, DaemonMsg, Grid, Origin, Secret};
use haetty_proto::{Client, Role};

struct Daemon {
    dir: PathBuf,
    ep: Endpoint,
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
        .env("HOME", &dir)
        .env("CLAUDE_CONFIG_DIR", dir.join("no-config"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for v in ["HAETTY_PROXY_DIR", "HAETTY_CLAUDE_SETTINGS", "HAETTY_PROXIED", "HAETTY_IDE_PORT", "CLAUDE_CODE_SSE_PORT"]
    {
        c.env_remove(v);
    }
    let child = c.spawn().expect("start daemon");
    let client = common::connect(&ep, "ide-test");
    Daemon { dir, ep, child, client }
}

fn exe(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

const FAKE_CLAUDE: &str =
    "#!/bin/sh\nprintf '%s\\n' \"${CLAUDE_CODE_SSE_PORT-unset}\" > \"$HOME/port.new\" && mv \"$HOME/port.new\" \"$HOME/port\"\nexec sleep 600\n";

fn home(d: &Daemon, case: &str) -> PathBuf {
    let h = d.dir.join(case);
    exe(&h.join("bin/claude"), FAKE_CLAUDE);
    std::fs::create_dir_all(h.join("w")).unwrap();
    h
}

fn create(d: &Daemon, h: &Path, program: &str, args: &[&str], ide: bool) -> String {
    d.client
        .create(CreateSpec {
            program: program.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            cwd: Some(h.join("w").to_string_lossy().into_owned()),
            env: vec![
                ("HOME".into(), h.to_string_lossy().into_owned()),
                ("CLAUDE_CONFIG_DIR".into(), h.join("cfg").to_string_lossy().into_owned()),
                ("PATH".into(), format!("{}:/usr/bin:/bin", h.join("bin").display())),
            ],
            size: Grid { cols: 120, rows: 20, cell_width: 9, cell_height: 18 },
            scrollback_limit: 1000,
            default_fg: [200, 200, 200],
            default_bg: [20, 20, 20],
            palette: vec![],
            cursor_color: None,
            claude_integration: true,
            shell_integration: false,
            ide_integration: ide,
            engine: String::new(),
        })
        .unwrap()
}

fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(15);
    while !f() {
        assert!(Instant::now() < end, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn port_of(h: &Path) -> String {
    let p = h.join("port");
    wait_for("the fake claude's port file", || p.exists());
    std::fs::read_to_string(p).unwrap().trim().to_string()
}

fn locks(h: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> =
        std::fs::read_dir(h.join("cfg/ide")).into_iter().flatten().flatten().map(|e| e.path()).collect();
    v.sort();
    v
}

fn upgrade(port: &str, token: Option<&str>) -> (TcpStream, String) {
    let mut s = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut req = format!(
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Protocol: mcp\r\n"
    );
    if let Some(t) = token {
        req.push_str(&format!("x-claude-code-ide-authorization: {t}\r\n"));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).unwrap();
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if s.read(&mut b).unwrap_or(0) == 0 {
            break;
        }
        head.push(b[0]);
    }
    (s, String::from_utf8_lossy(&head).into_owned())
}

fn send(s: &mut TcpStream, text: &str) {
    let mask = [7u8, 1, 9, 3];
    let p = text.as_bytes();
    let mut f = vec![0x81];
    match p.len() {
        n if n < 126 => f.push(0x80 | n as u8),
        n if n < 65536 => {
            f.push(0x80 | 126);
            f.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            f.push(0x80 | 127);
            f.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    f.extend_from_slice(&mask);
    f.extend(p.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    s.write_all(&f).unwrap();
}

fn recv(s: &mut TcpStream) -> std::io::Result<Json> {
    let mut h = [0u8; 2];
    s.read_exact(&mut h)?;
    assert_eq!(h[1] & 0x80, 0, "a server frame is never masked");
    let len = match h[1] & 0x7f {
        126 => {
            let mut b = [0u8; 2];
            s.read_exact(&mut b)?;
            u16::from_be_bytes(b) as usize
        }
        127 => {
            let mut b = [0u8; 8];
            s.read_exact(&mut b)?;
            u64::from_be_bytes(b) as usize
        }
        n => n as usize,
    };
    let mut p = vec![0u8; len];
    s.read_exact(&mut p)?;
    assert_eq!(h[0], 0x81, "a text frame");
    Ok(Json::parse(&String::from_utf8(p).unwrap()).unwrap())
}

fn texts(j: &Json) -> Vec<String> {
    j.at(&["result", "content"])
        .and_then(Json::arr)
        .map(|a| a.iter().filter_map(|c| c.get("text").and_then(Json::str).map(str::to_string)).collect())
        .unwrap_or_default()
}

fn open_diff(s: &mut TcpStream, id: u32, path: &Path, text: &str, tab: &str) {
    let msg = format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"openDiff","arguments":{{"old_file_path":{p},"new_file_path":{p},"new_file_contents":{t},"tab_name":{tab}}}}}}}"#,
        p = Json::Str(path.to_string_lossy().into_owned()),
        t = Json::Str(text.into()),
        tab = Json::Str(tab.into()),
    );
    send(s, &msg);
}

fn quiet(s: &mut TcpStream) -> bool {
    s.set_read_timeout(Some(Duration::from_millis(400))).unwrap();
    let mut b = [0u8; 1];
    let r = s.peek(&mut b);
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    r.is_err()
}

#[test]
fn a_claude_code_tab_gets_an_editor_that_only_the_person_answers() {
    let d = start("ide-tab");
    let h = home(&d, "tab");
    let ide = h.join("cfg/ide");
    std::fs::create_dir_all(&ide).unwrap();
    std::fs::write(ide.join("11111.lock"), r#"{"pid":999999999,"ideName":"haetty","transport":"ws","authToken":"x"}"#)
        .unwrap();
    std::fs::write(ide.join("22222.lock"), r#"{"pid":999999999,"ideName":"Neovim","transport":"ws"}"#).unwrap();
    let id = create(&d, &h, &h.join("bin/claude").to_string_lossy(), &[], true);
    let port = port_of(&h);
    assert!(port.parse::<u16>().is_ok_and(|p| p >= 10000), "CLAUDE_CODE_SSE_PORT = {port}");
    let lock = ide.join(format!("{port}.lock"));
    let mut want = vec![ide.join("22222.lock"), lock.clone()];
    want.sort();
    assert_eq!(locks(&h), want, "the stale haetty lock went, Neovim's stayed");
    assert_eq!(std::fs::metadata(&lock).unwrap().permissions().mode() & 0o777, 0o600);
    let l = Json::parse(&std::fs::read_to_string(&lock).unwrap()).unwrap();
    assert_eq!(l.get("ideName").and_then(Json::str), Some("haetty"));
    assert_eq!(l.get("transport").and_then(Json::str), Some("ws"));
    assert_eq!(l.get("workspaceFolders").and_then(Json::arr).map(<[Json]>::len), Some(0));
    let token = l.get("authToken").and_then(Json::str).unwrap().to_string();
    assert_eq!(token.len(), 32);

    for bad in [None, Some("0".repeat(32)), Some(format!("{}0", &token[..31]))] {
        let (_, head) = upgrade(&port, bad.as_deref());
        assert!(head.starts_with("HTTP/1.1 401 "), "{bad:?}: {head}");
    }
    let (mut ws, head) = upgrade(&port, Some(&token));
    assert!(head.starts_with("HTTP/1.1 101 ") && head.contains("Sec-WebSocket-Protocol: mcp\r\n"), "{head}");
    send(
        &mut ws,
        r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"claude-code","version":"2.1.329"}}}"#,
    );
    assert_eq!(recv(&mut ws).unwrap().at(&["result", "protocolVersion"]).and_then(Json::str), Some("2024-11-05"));
    send(&mut ws, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    send(&mut ws, r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#);
    assert_eq!(recv(&mut ws).unwrap().at(&["result", "tools"]).and_then(Json::arr).map(<[Json]>::len), Some(4));

    let file = h.join("w/a.txt");
    std::fs::write(&file, "one\n").unwrap();
    let gui = d.client.attach(&id, None).unwrap();
    open_diff(&mut ws, 7, &file, "two\n", "✻ [Claude Code] a.txt (aaaaaa) ⧉");
    wait_for("the proposal", || gui.handle.ide().is_some());
    let p = gui.handle.ide().unwrap();
    assert_eq!((p.path.as_str(), p.old.0.as_str(), p.new.0.as_str()), (file.to_str().unwrap(), "one\n", "two\n"));

    gui.handle.ide_answer(Origin::Program, p.id, true);
    let denied = d.client.next_reply(Duration::from_secs(5));
    assert!(matches!(&denied, Some(DaemonMsg::Denied { what }) if what == "IdeAnswer from a program"), "{denied:?}");
    let observer = Client::connect(&d.ep, Role::Observe, "ide-observer").unwrap();
    observer
        .send(&ClientMsg::IdeAnswer { session: id.clone(), origin: Origin::Human, id: p.id, accept: true })
        .unwrap();
    let denied = observer.next_reply(Duration::from_secs(5));
    assert!(matches!(&denied, Some(DaemonMsg::Denied { what }) if what.contains("IdeAnswer")), "{denied:?}");
    assert!(d.client.sync(Duration::from_secs(5)));
    assert!(quiet(&mut ws), "no answer from a program or an observer");
    assert_eq!(gui.handle.ide().map(|q| q.id), Some(p.id));

    gui.handle.ide_answer(Origin::Human, p.id, true);
    let r = recv(&mut ws).unwrap();
    assert_eq!(r.get("id"), Some(&Json::Num(7.0)));
    assert_eq!(texts(&r), ["FILE_SAVED", "two\n"]);
    wait_for("the proposal to go", || gui.handle.ide().is_none());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\n", "Claude Code writes the file, haetty never does");

    open_diff(&mut ws, 8, &h.join("w/.env"), "SECRET=1\n", "t-secret");
    open_diff(&mut ws, 9, &file, "three\n", "t9");
    wait_for("the next proposal", || gui.handle.ide().is_some());
    let p = gui.handle.ide().unwrap();
    assert_eq!(p.new.0, "three\n", "a credential-like file is never read nor shown");
    gui.handle.ide_answer(Origin::Human, p.id, false);
    let r = recv(&mut ws).unwrap();
    assert_eq!((r.get("id"), texts(&r)), (Some(&Json::Num(9.0)), vec!["DIFF_REJECTED".to_string(), "t9".to_string()]));

    open_diff(&mut ws, 10, &file, "four\n", "t10");
    wait_for("a proposal", || gui.handle.ide().is_some());
    send(
        &mut ws,
        r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"close_tab","arguments":{"tab_name":"t10"}}}"#,
    );
    let r = recv(&mut ws).unwrap();
    assert_eq!((r.get("id"), texts(&r)), (Some(&Json::Num(11.0)), vec!["TAB_CLOSED".to_string()]));
    wait_for("the closed proposal to go", || gui.handle.ide().is_none());

    open_diff(&mut ws, 12, &file, "five\n", "t12");
    wait_for("a proposal", || gui.handle.ide().is_some());
    drop(ws);
    wait_for("the proposal of a gone connection to go", || gui.handle.ide().is_none());

    d.client.close(&id).unwrap();
    wait_for("the lock file to go", || !lock.exists());
    assert!(TcpStream::connect(format!("127.0.0.1:{port}")).is_err(), "the port is free again");
}

#[test]
fn a_shell_tabs_proxy_hands_the_port_to_the_real_claude_and_off_means_nothing() {
    let d = start("ide-proxy");
    for (case, on) in [("on", true), ("off", false)] {
        let h = home(&d, case);
        let id = create(&d, &h, "/bin/sh", &["-c", "claude; exec sleep 600"], on);
        let port = port_of(&h);
        if on {
            assert!(h.join(format!("cfg/ide/{port}.lock")).exists(), "{case}: the lock of port {port}");
        } else {
            assert_eq!(port, "unset");
            assert!(locks(&h).is_empty(), "off: no lock");
        }
        d.client.close(&id).unwrap();
    }
    let h = home(&d, "no-integration");
    let spec = CreateSpec { claude_integration: false, ..common::spec(&h.join("bin/claude").to_string_lossy(), &[]) };
    let id = d.client.create(CreateSpec { ide_integration: true, ..spec }).unwrap();
    assert!(d.client.sync(Duration::from_secs(5)));
    assert!(locks(&h).is_empty() && !h.join("cfg").exists(), "no editor without claude-integration");
    let _ = d.client.close(&id);
}

fn hook(d: &Daemon, session: &str, event: &str, json: &str) {
    let mut c = Command::new(common::bin())
        .args(["hook", event])
        .env("HAETTY_SOCKET", &d.ep.0)
        .env("HAETTY_SESSION_ID", session)
        .env("HAETTY_INSTANCE", "dev")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(json.as_bytes()).unwrap();
    let out = c.wait_with_output().unwrap();
    assert!(out.status.success() && out.stdout.is_empty(), "{out:?}");
}

fn attention(d: &Daemon, id: &str) -> Attention {
    d.client.list_sessions().unwrap().into_iter().find(|s| s.id == id).unwrap().attention
}

fn program_write_taken(c: &Client, id: &str) -> bool {
    c.send(&ClientMsg::Write { session: id.to_owned(), origin: Origin::Program, bytes: Secret(b"x".to_vec()) })
        .unwrap();
    c.send(&ClientMsg::Sync).unwrap();
    let mut denied = false;
    loop {
        match c.next_reply(Duration::from_secs(5)) {
            Some(DaemonMsg::Denied { .. }) => denied = true,
            Some(DaemonMsg::Synced) => return !denied,
            Some(_) => {}
            None => panic!("no Synced from the daemon"),
        }
    }
}

#[test]
fn an_answer_in_the_overlay_is_the_persons_input_and_keeps_the_hold() {
    let d = start("ide-k1");
    let h = home(&d, "k1");
    let id = create(&d, &h, &h.join("bin/claude").to_string_lossy(), &[], true);
    let port = port_of(&h);
    let l = Json::parse(&std::fs::read_to_string(h.join(format!("cfg/ide/{port}.lock"))).unwrap()).unwrap();
    let (mut ws, _) = upgrade(&port, l.get("authToken").and_then(Json::str));
    let orch = Client::connect(&d.ep, Role::Control, "orchestrator").unwrap();
    let gui = d.client.attach(&id, None).unwrap();
    let file = h.join("w/b.txt");
    std::fs::write(&file, "old\n").unwrap();
    for accept in [true, false] {
        hook(&d, &id, "UserPromptSubmit", r#"{"session_id":"s"}"#);
        hook(&d, &id, "PermissionRequest", r#"{"session_id":"s","tool_name":"Write"}"#);
        assert_eq!(attention(&d, &id), Attention::Ask);
        assert!(!program_write_taken(&orch, &id), "held while the dialog waits");
        open_diff(&mut ws, 1, &file, "new\n", "t");
        wait_for("the proposal", || gui.handle.ide().is_some());
        let p = gui.handle.ide().unwrap();
        gui.handle.ide_answer(Origin::Human, p.id, accept);
        recv(&mut ws).unwrap();
        assert!(d.client.sync(Duration::from_secs(5)));
        assert_eq!(attention(&d, &id), Attention::None, "accept={accept}: the answer clears the mark");
        assert!(!program_write_taken(&orch, &id), "accept={accept}: the hold waits for the turn's end");
        hook(&d, &id, "Stop", r#"{"session_id":"s"}"#);
        assert!(program_write_taken(&orch, &id), "accept={accept}: the turn's end opens the hold");
    }
    d.client.close(&id).unwrap();
}
