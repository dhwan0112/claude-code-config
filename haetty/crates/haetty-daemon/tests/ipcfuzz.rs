//! IPC fuzzing (stage S4): raw connections send bad length prefixes,
//! broken postcard, unknown messages, wrong magic or version, truncated and
//! mutated frames. The daemon must survive and drop only that connection: a
//! session keeps producing output to an attached client, and new clients
//! connect and list sessions. Deterministic (fixed seed). Unix only.
#![cfg(unix)]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use haetty_daemon::Config;
use haetty_proto::paths::Endpoint;
use haetty_proto::wire::{read_msg, write_msg, ClientMsg, CreateSpec, DaemonMsg, Grid, Origin, Secret, MAGIC, VERSION};
use haetty_proto::{Client, Role};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn frame(body: &[u8]) -> Vec<u8> {
    let mut v = (body.len() as u32).to_le_bytes().to_vec();
    v.extend_from_slice(body);
    v
}

fn hello(role: Role, version: u32, magic: [u8; 4]) -> Vec<u8> {
    let m = ClientMsg::Hello { magic, version, role, name: "fuzz".into() };
    frame(&postcard::to_stdvec(&m).unwrap())
}

/// Sends `bytes` on a fresh connection, reads whatever comes back for a
/// moment, closes. Never panics on the daemon's behaviour.
fn poke(ep: &Endpoint, bytes: &[u8]) -> Vec<u8> {
    let Ok(mut s) = UnixStream::connect(&ep.0) else { return vec![] };
    let _ = s.set_read_timeout(Some(Duration::from_millis(30)));
    let _ = s.write_all(bytes);
    let mut out = vec![];
    let mut buf = [0u8; 4096];
    while let Ok(n) = s.read(&mut buf) {
        if n == 0 || out.len() > 1 << 20 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
    }
    out
}

fn spec() -> CreateSpec {
    CreateSpec {
        program: "sh".into(),
        // Ignores signals from typed control characters; never reads input.
        args: vec!["-c".into(), "stty -isig raw; i=0; while :; do echo n$i; i=$((i+1)); sleep 0.05; done".into()],
        cwd: None,
        env: vec![],
        size: Grid { cols: 40, rows: 8, cell_width: 9, cell_height: 18 },
        scrollback_limit: 100_000,
        default_fg: [200, 200, 200],
        default_bg: [20, 20, 20],
        palette: vec![],
        cursor_color: None,
        claude_integration: false,
    }
}

/// The daemon still serves: a new client lists the session, the attached
/// victim still gets fresh output.
fn alive(ep: &Endpoint, s: &haetty_proto::RemoteSession, what: &str) {
    let c = Client::connect(ep, Role::Observe, "check").unwrap_or_else(|e| panic!("{what}: daemon gone: {e}"));
    let list = c.list_sessions().unwrap_or_else(|e| panic!("{what}: list: {e}"));
    assert_eq!(list.len(), 1, "{what}");
    let before = s.handle.snapshot().map(|x| x.text()).unwrap_or_default();
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let _ = s.events.recv_timeout(Duration::from_millis(50));
        let now = s.handle.snapshot().map(|x| x.text()).unwrap_or_default();
        if now != before {
            return;
        }
        assert!(Instant::now() < end, "{what}: the session's output stopped");
    }
}

#[test]
fn malformed_connections_drop_alone_and_the_daemon_keeps_serving() {
    let dir = haetty_testkit::run_dir("ipcfuzz");
    let ep = Endpoint(dir.join("rt").join("daemon.sock"));
    let cfg = Config { endpoint: ep.clone(), state_dir: dir.join("state"), run_dir: dir.join("rt"), idle_exit: None };
    let _d = haetty_daemon::spawn(cfg);
    let end = Instant::now() + Duration::from_secs(5);
    let victim = loop {
        match Client::connect(&ep, Role::Control, "victim") {
            Ok(c) => break c,
            Err(_) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => panic!("daemon: {e}"),
        }
    };
    let id = victim.create(spec()).unwrap();
    let s = victim.attach(&id, None).unwrap();
    let mut r = Rng(0x0dd_b1a5_e5);
    let t0 = Instant::now();

    // 1. Length prefixes: huge, over the cap, zero, short then close.
    for len in [u32::MAX, (32 << 20) + 1, 0, 3] {
        poke(&ep, &len.to_le_bytes());
    }
    poke(&ep, &[0x10, 0, 0]);
    alive(&ep, &s, "bad length prefixes");

    // 2. Wrong magic, old and future versions: refused with a reason.
    for (v, m) in [(VERSION - 1, MAGIC), (VERSION + 1, MAGIC), (u32::MAX, MAGIC), (VERSION, *b"XXXX")] {
        let got = poke(&ep, &hello(Role::Control, v, m));
        let reply: DaemonMsg = read_msg(&mut got.as_slice()).expect("a Refused reply");
        assert!(matches!(reply, DaemonMsg::Refused { .. }), "version {v}: {reply:?}");
    }
    alive(&ep, &s, "version mismatch");

    // 3. Random bodies as the first frame, then after a valid Hello.
    for i in 0..300 {
        let body: Vec<u8> = (0..r.below(2048)).map(|_| r.below(256) as u8).collect();
        let mut bytes = if i % 2 == 0 { vec![] } else { hello(Role::Control, VERSION, MAGIC) };
        bytes.extend(frame(&body));
        poke(&ep, &bytes);
    }
    alive(&ep, &s, "random frames");

    // 4. Unknown message kinds (variant numbers past the last) and truncated frames.
    for v in [23u8, 24, 100, 127, 255] {
        let mut bytes = hello(Role::Control, VERSION, MAGIC);
        bytes.extend(frame(&[v, 1, 2, 3]));
        poke(&ep, &bytes);
    }
    let mut cut = hello(Role::Control, VERSION, MAGIC);
    cut.extend_from_slice(&100u32.to_le_bytes());
    cut.extend_from_slice(&[1, 2, 3]);
    poke(&ep, &cut);
    alive(&ep, &s, "unknown kinds and truncated frames");

    // 5. Valid messages for the live session with mutated payloads (the
    //    variant byte is kept, so no Create or Close is ever made up).
    let base: Vec<ClientMsg> = vec![
        ClientMsg::Paste { session: id.clone(), text: Secret("hello".into()), origin: Origin::Program },
        ClientMsg::Write { session: id.clone(), bytes: Secret(b"abc".to_vec()), origin: Origin::Program },
        ClientMsg::Resize { session: id.clone(), size: Grid { cols: 40, rows: 8, cell_width: 9, cell_height: 18 } },
        ClientMsg::Focus { session: id.clone(), focused: true },
        ClientMsg::Attach { session: id.clone() },
        ClientMsg::Detach { session: id.clone() },
        ClientMsg::GetEdit { session: id.clone() },
        ClientMsg::ListSessions,
        ClientMsg::Sync,
    ];
    for i in 0..600 {
        let m = &base[i % base.len()];
        let mut body = postcard::to_stdvec(m).unwrap();
        for _ in 0..1 + r.below(6) {
            if body.len() > 1 {
                let at = 1 + r.below(body.len() - 1);
                match r.below(3) {
                    0 => body[at] = r.below(256) as u8,
                    1 => body.insert(at, r.below(256) as u8),
                    _ => body.truncate(at),
                }
            }
        }
        let mut bytes = hello(*[&Role::Control, &Role::Observe][i % 2], VERSION, MAGIC);
        bytes.extend(frame(&body));
        poke(&ep, &bytes);
    }
    alive(&ep, &s, "mutated messages");
    // Extreme sizes through a proper client: refused or clamped, never a panic.
    let c = Client::connect(&ep, Role::Control, "sizes").unwrap();
    let h = c.attach(&id, None).unwrap();
    for (cols, rows) in [(0u16, 0u16), (1, 1), (u16::MAX, u16::MAX), (1000, 3)] {
        h.handle.resize(haetty_core::GridSize::new(cols, rows));
    }
    h.handle.resize(haetty_core::GridSize::new(40, 8));
    alive(&ep, &s, "extreme sizes");
    eprintln!("ipcfuzz: {} connections in {:?}", 4 + 1 + 4 + 300 + 6 + 600, t0.elapsed());
    // A valid client still works end to end.
    let mut ok = UnixStream::connect(&ep.0).unwrap();
    ok.write_all(&hello(Role::Observe, VERSION, MAGIC)).unwrap();
    let w: DaemonMsg = read_msg(&mut ok).unwrap();
    assert!(matches!(w, DaemonMsg::Welcome { .. }), "{w:?}");
    write_msg(&mut ok, &ClientMsg::ListSessions).unwrap();
    let l: DaemonMsg = read_msg(&mut ok).unwrap();
    assert!(matches!(l, DaemonMsg::Sessions(ref v) if v.len() == 1), "{l:?}");
    haetty_testkit::remove_dir_all(&dir);
}
