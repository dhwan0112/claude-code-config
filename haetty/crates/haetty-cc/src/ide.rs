//! haetty as Claude Code's editor (DESIGN.md §13.5, §13.9; unofficial protocol, pinned to Claude Code 2.1.329): the
//! WebSocket handshake and frames, the JSON-RPC answers and the lock file. No sockets here: the daemon owns them.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::json::{obj, s, Json};

/// Claude Code connects at start to the editor on this port.
pub const SSE_PORT_ENV: &str = "CLAUDE_CODE_SSE_PORT";
/// A shell tab's editor port, set by the daemon; the `claude` proxy hands it to the real claude as [`SSE_PORT_ENV`].
pub const PORT_ENV: &str = "HAETTY_IDE_PORT";
/// The lock file's `ideName`.
pub const IDE_NAME: &str = "haetty";
/// The largest frame or proposal taken (the edit snapshot's limit).
pub const MAX_BYTES: usize = crate::edit::MAX_BYTES as usize;

const TOKEN_HEADER: &str = "x-claude-code-ide-authorization";
const PROTOCOL_VERSION: &str = "2024-11-05";
const HEADER_LIMIT: usize = 16 << 10;

/// `<claude config>/ide`: `$CLAUDE_CONFIG_DIR/ide`, else `~/.claude/ide`.
pub fn lock_dir(config_dir: Option<&std::ffi::OsStr>, home: Option<&Path>) -> Option<PathBuf> {
    match config_dir.filter(|d| !d.is_empty()) {
        Some(d) => Some(PathBuf::from(d).join("ide")),
        None => home.map(|h| h.join(".claude").join("ide")),
    }
}

/// The lock file's text: no workspace folder, so only the `claude` started with the port connects.
pub fn lock_json(pid: u32, token: &str) -> String {
    obj([
        ("pid", Json::Num(pid as f64)),
        ("workspaceFolders", Json::Arr(vec![])),
        ("ideName", s(IDE_NAME)),
        ("transport", s("ws")),
        ("authToken", s(token)),
    ])
    .to_string()
}

/// A lock file haetty wrote whose daemon is gone.
pub fn stale_lock(text: &str, alive: impl Fn(u32) -> bool) -> bool {
    let Ok(j) = Json::parse(text) else { return false };
    if j.get("ideName").and_then(Json::str) != Some(IDE_NAME) {
        return false;
    }
    j.get("pid").and_then(Json::num).is_none_or(|p| !alive(p as u32))
}

/// A new 128-bit token, hex.
pub fn token(random: impl FnOnce(&mut [u8; 16]) -> io::Result<()>) -> io::Result<String> {
    let mut b = [0u8; 16];
    random(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, b) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

pub fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `Sec-WebSocket-Accept` for a `Sec-WebSocket-Key` (RFC 6455 §4.2.2).
pub fn accept_key(key: &str) -> String {
    base64(&sha1(format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes()))
}

/// Reads an HTTP request head (up to the blank line) and answers the upgrade: `Ok` with the response to send when the
/// request is a WebSocket upgrade with the session's token, `Err` with a refusal otherwise.
pub fn handshake(r: &mut impl Read, token: &str) -> io::Result<Result<String, String>> {
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() >= HEADER_LIMIT || r.read(&mut b)? == 0 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        head.push(b[0]);
    }
    let text = String::from_utf8_lossy(&head);
    let mut lines = text.split("\r\n");
    let get = lines.next().is_some_and(|l| l.starts_with("GET "));
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let h = |k: &str| headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let refuse = |code: &str| Ok(Err(format!("HTTP/1.1 {code}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")));
    let upgrade = h("upgrade").is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    let (true, true, Some(key), Some("13")) = (get, upgrade, h("sec-websocket-key"), h("sec-websocket-version")) else {
        return refuse("400 Bad Request");
    };
    if !h(TOKEN_HEADER).is_some_and(|t| same(t.as_bytes(), token.as_bytes())) {
        return refuse("401 Unauthorized");
    }
    let mcp = h("sec-websocket-protocol").is_some_and(|p| p.split(',').any(|x| x.trim() == "mcp"));
    Ok(Ok(format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n{}\r\n",
        accept_key(key),
        if mcp { "Sec-WebSocket-Protocol: mcp\r\n" } else { "" }
    )))
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A message from the client.
#[derive(Debug, PartialEq, Eq)]
pub enum Frame {
    Text(String),
    Ping(Vec<u8>),
    Pong,
    Close,
}

/// Reads one message (continuations joined); binary messages are skipped.
pub fn read_frame(r: &mut impl Read) -> io::Result<Frame> {
    let mut msg: Vec<u8> = Vec::new();
    let mut text = false;
    loop {
        let mut h = [0u8; 2];
        r.read_exact(&mut h)?;
        let (fin, op, masked) = (h[0] & 0x80 != 0, h[0] & 0x0f, h[1] & 0x80 != 0);
        let len = match h[1] & 0x7f {
            126 => {
                let mut b = [0u8; 2];
                r.read_exact(&mut b)?;
                u16::from_be_bytes(b) as u64
            }
            127 => {
                let mut b = [0u8; 8];
                r.read_exact(&mut b)?;
                u64::from_be_bytes(b)
            }
            n => n as u64,
        };
        if len as usize > MAX_BYTES || msg.len() + len as usize > MAX_BYTES * 2 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
        }
        let mut mask = [0u8; 4];
        if masked {
            r.read_exact(&mut mask)?;
        }
        let mut p = vec![0u8; len as usize];
        r.read_exact(&mut p)?;
        if masked {
            for (i, b) in p.iter_mut().enumerate() {
                *b ^= mask[i % 4];
            }
        }
        match op {
            0x8 => return Ok(Frame::Close),
            0x9 => return Ok(Frame::Ping(p)),
            0xA => return Ok(Frame::Pong),
            0x0..=0x2 => {
                if op != 0 {
                    text = op == 1;
                    msg.clear();
                }
                msg.extend_from_slice(&p);
                if fin {
                    if text {
                        return String::from_utf8(msg)
                            .map(Frame::Text)
                            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "text is not UTF-8"));
                    }
                    msg = Vec::new();
                }
            }
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "unknown opcode")),
        }
    }
}

/// Writes one unmasked frame (a server's frames are never masked, RFC 6455 §5.1).
pub fn write_frame(w: &mut impl Write, op: u8, payload: &[u8]) -> io::Result<()> {
    let mut out = vec![0x80 | op];
    match payload.len() {
        n if n < 126 => out.push(n as u8),
        n if n <= u16::MAX as usize => {
            out.push(126);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            out.push(127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    out.extend_from_slice(payload);
    w.write_all(&out)?;
    w.flush()
}

pub const OP_TEXT: u8 = 0x1;
pub const OP_CLOSE: u8 = 0x8;
pub const OP_PING: u8 = 0x9;
pub const OP_PONG: u8 = 0xA;

/// What a JSON-RPC message from Claude Code asks.
#[derive(Debug, PartialEq)]
pub enum Call {
    /// Send this answer now.
    Reply(String),
    /// A notification: no answer.
    Quiet,
    /// An edit proposal: answered later with [`accepted`] or [`rejected`], or never.
    OpenDiff { id: Json, path: String, new_text: String, tab: String },
    /// Claude Code closed a proposal (answered in the terminal): close it; the answer is in `reply`.
    CloseTab { tab: String, reply: String },
    /// The start of a user turn: close every proposal.
    CloseAll { reply: String },
}

/// Answers one message (DESIGN.md §13.9: what Claude Code 2.1.329 calls).
pub fn call(text: &str, version: &str) -> Call {
    let Ok(m) = Json::parse(text) else { return Call::Reply(error(&Json::Null, -32700, "parse error")) };
    let Some(id) = m.get("id").cloned() else { return Call::Quiet };
    let method = m.get("method").and_then(Json::str).unwrap_or_default();
    match method {
        "initialize" => Call::Reply(result(
            &id,
            obj([
                ("protocolVersion", s(PROTOCOL_VERSION)),
                ("capabilities", obj([("tools", obj([("listChanged", Json::Bool(true))]))])),
                ("serverInfo", obj([("name", s(IDE_NAME)), ("version", s(version))])),
            ]),
        )),
        "ping" => Call::Reply(result(&id, obj([]))),
        "tools/list" => Call::Reply(result(&id, obj([("tools", tools())]))),
        "prompts/list" => Call::Reply(result(&id, obj([("prompts", Json::Arr(vec![]))]))),
        "resources/list" => Call::Reply(result(&id, obj([("resources", Json::Arr(vec![]))]))),
        "tools/call" => {
            let name = m.at(&["params", "name"]).and_then(Json::str).unwrap_or_default();
            let arg = |k: &str| m.at(&["params", "arguments", k]).and_then(Json::str).map(str::to_string);
            match name {
                "openDiff" => match (arg("new_file_path").or_else(|| arg("old_file_path")), arg("new_file_contents")) {
                    (Some(path), Some(new_text)) => {
                        Call::OpenDiff { id, path, new_text, tab: arg("tab_name").unwrap_or_default() }
                    }
                    _ => Call::Reply(error(&id, -32602, "openDiff needs new_file_path and new_file_contents")),
                },
                // claudecode.nvim's and VS Code's answer; Claude Code ignores the text.
                "close_tab" => {
                    Call::CloseTab { tab: arg("tab_name").unwrap_or_default(), reply: tool_text(&id, &["TAB_CLOSED"]) }
                }
                "closeAllDiffTabs" => Call::CloseAll { reply: tool_text(&id, &["CLOSED_0_DIFF_TABS"]) },
                "getDiagnostics" => Call::Reply(tool_text(&id, &["[]"])),
                _ => Call::Reply(error(&id, -32602, "unknown tool")),
            }
        }
        _ => Call::Reply(error(&id, -32601, "method not found")),
    }
}

fn tools() -> Json {
    let tool = |name: &str, desc: &str, props: Vec<(&str, &str)>| {
        let p = Json::Obj(
            props.iter().map(|(k, d)| (k.to_string(), obj([("type", s("string")), ("description", s(*d))]))).collect(),
        );
        let req = Json::Arr(props.iter().map(|(k, _)| s(*k)).collect());
        obj([
            ("name", s(name)),
            ("description", s(desc)),
            ("inputSchema", obj([("type", s("object")), ("properties", p), ("required", req)])),
        ])
    };
    Json::Arr(vec![
        tool(
            "openDiff",
            "Show a proposed edit and wait for the person to accept or reject it",
            vec![
                ("old_file_path", "The file's path"),
                ("new_file_path", "The file's path after the edit"),
                ("new_file_contents", "The whole proposed text"),
                ("tab_name", "The proposal's name"),
            ],
        ),
        tool("close_tab", "Close a proposal", vec![("tab_name", "The proposal's name")]),
        tool("closeAllDiffTabs", "Close every proposal", vec![]),
        tool("getDiagnostics", "Diagnostics (haetty has none)", vec![]),
    ])
}

fn result(id: &Json, r: Json) -> String {
    obj([("jsonrpc", s("2.0")), ("id", id.clone()), ("result", r)]).to_string()
}

fn error(id: &Json, code: i32, msg: &str) -> String {
    obj([
        ("jsonrpc", s("2.0")),
        ("id", id.clone()),
        ("error", obj([("code", Json::Num(code as f64)), ("message", s(msg))])),
    ])
    .to_string()
}

fn tool_text(id: &Json, texts: &[&str]) -> String {
    let content = Json::Arr(texts.iter().map(|t| obj([("type", s("text")), ("text", s(*t))])).collect());
    result(id, obj([("content", content)]))
}

/// The person accepted: Claude Code writes `text` (an unchanged text counts as a rejection there).
pub fn accepted(id: &Json, text: &str) -> String {
    tool_text(id, &["FILE_SAVED", text])
}

/// The person rejected.
pub fn rejected(id: &Json, tab: &str) -> String {
    tool_text(id, &["DIFF_REJECTED", tab])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn sha1_and_base64_match_the_standard_vectors() {
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        let long = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(hex(&sha1(long)), "84983e441c3bd26ebaae4aa1f95129e5e54670f1");
        assert_eq!(hex(&sha1(&[b'a'; 1000])), "291e9a6c66994949b57ba5e650361e98fc36b1ba");
        for (i, o) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(i.as_bytes()), o);
        }
        assert_eq!(accept_key("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    fn upgrade(token: Option<&str>, proto: bool) -> String {
        let mut r = "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
                     Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n"
            .to_string();
        if let Some(t) = token {
            r.push_str(&format!("X-Claude-Code-Ide-Authorization: {t}\r\n"));
        }
        if proto {
            r.push_str("Sec-WebSocket-Protocol: mcp\r\n");
        }
        r + "\r\n"
    }

    #[test]
    fn the_upgrade_needs_the_sessions_token() {
        let ok = handshake(&mut upgrade(Some("abc"), true).as_bytes(), "abc").unwrap().unwrap();
        assert!(ok.starts_with("HTTP/1.1 101 "), "{ok}");
        assert!(ok.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"));
        assert!(ok.contains("Sec-WebSocket-Protocol: mcp\r\n"));
        let plain = handshake(&mut upgrade(Some("abc"), false).as_bytes(), "abc").unwrap().unwrap();
        assert!(!plain.contains("Sec-WebSocket-Protocol"));
        for t in [None, Some("abd"), Some("ab"), Some("")] {
            let no = handshake(&mut upgrade(t, true).as_bytes(), "abc").unwrap().unwrap_err();
            assert!(no.starts_with("HTTP/1.1 401 "), "{t:?}: {no}");
        }
        let get = "GET / HTTP/1.1\r\nX-Claude-Code-Ide-Authorization: abc\r\n\r\n";
        assert!(handshake(&mut get.as_bytes(), "abc").unwrap().unwrap_err().starts_with("HTTP/1.1 400 "));
        assert!(handshake(&mut "GET / HTTP/1.1\r\n".as_bytes(), "abc").is_err());
    }

    fn client_frame(op: u8, fin: bool, payload: &[u8]) -> Vec<u8> {
        let mask = [1u8, 2, 3, 4];
        let mut v = vec![if fin { 0x80 } else { 0 } | op];
        match payload.len() {
            n if n < 126 => v.push(0x80 | n as u8),
            n if n <= 65535 => {
                v.push(0x80 | 126);
                v.extend_from_slice(&(n as u16).to_be_bytes());
            }
            n => {
                v.push(0x80 | 127);
                v.extend_from_slice(&(n as u64).to_be_bytes());
            }
        }
        v.extend_from_slice(&mask);
        v.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        v
    }

    #[test]
    fn frames_are_unmasked_joined_and_sized() {
        let mut all = client_frame(1, true, b"hello");
        all.extend(client_frame(9, true, b"p"));
        all.extend(client_frame(1, false, b"he"));
        all.extend(client_frame(0, true, b"llo"));
        let big = "x".repeat(70_000);
        all.extend(client_frame(1, true, big.as_bytes()));
        all.extend(client_frame(2, true, b"bin"));
        all.extend(client_frame(1, true, &[0xff]));
        let mut r = all.as_slice();
        assert_eq!(read_frame(&mut r).unwrap(), Frame::Text("hello".into()));
        assert_eq!(read_frame(&mut r).unwrap(), Frame::Ping(b"p".to_vec()));
        assert_eq!(read_frame(&mut r).unwrap(), Frame::Text("hello".into()));
        assert_eq!(read_frame(&mut r).unwrap(), Frame::Text(big));
        assert!(read_frame(&mut r).is_err(), "binary skipped, then text that is not UTF-8");
        let mut huge = vec![0x81, 0x80 | 127];
        huge.extend_from_slice(&(MAX_BYTES as u64 + 1).to_be_bytes());
        assert!(read_frame(&mut huge.as_slice()).is_err());
        for n in [5usize, 300, 70_000] {
            let mut out = Vec::new();
            write_frame(&mut out, OP_TEXT, &vec![b'a'; n]).unwrap();
            assert_eq!(out[0], 0x81);
            assert_eq!(
                out.len() - n,
                if n < 126 {
                    2
                } else if n < 65536 {
                    4
                } else {
                    10
                }
            );
        }
    }

    #[test]
    fn calls_are_answered_as_claude_code_expects() {
        let reply = |t: &str| match call(t, "0.4.0") {
            Call::Reply(r) => Json::parse(&r).unwrap(),
            other => panic!("{other:?}"),
        };
        let init = reply(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#);
        assert_eq!(init.at(&["result", "protocolVersion"]).and_then(Json::str), Some("2024-11-05"));
        assert_eq!(init.get("id"), Some(&Json::Num(0.0)));
        let list = reply(r#"{"jsonrpc":"2.0","id":"a","method":"tools/list"}"#);
        let names: Vec<&str> = list
            .at(&["result", "tools"])
            .and_then(Json::arr)
            .unwrap()
            .iter()
            .filter_map(|t| t.get("name").and_then(Json::str))
            .collect();
        assert_eq!(names, ["openDiff", "close_tab", "closeAllDiffTabs", "getDiagnostics"]);
        assert_eq!(call(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, ""), Call::Quiet);
        assert_eq!(call(r#"{"jsonrpc":"2.0","method":"ide_connected","params":{"pid":1}}"#, ""), Call::Quiet);
        assert_eq!(
            reply(r#"{"jsonrpc":"2.0","id":3,"method":"nope"}"#).at(&["error", "code"]),
            Some(&Json::Num(-32601.0))
        );
        assert_eq!(reply("{").at(&["error", "code"]), Some(&Json::Num(-32700.0)));
        let diag = reply(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"getDiagnostics","arguments":{}}}"#,
        );
        assert_eq!(diag.at(&["result", "content"]).and_then(Json::arr).map(|a| a.len()), Some(1));
        let open = r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"openDiff","arguments":
            {"old_file_path":"/w/a.txt","new_file_path":"/w/a.txt","new_file_contents":"two\n","tab_name":"✻ [Claude Code] a.txt (x1y2z3) ⧉"}}}"#;
        match call(open, "") {
            Call::OpenDiff { id, path, new_text, tab } => {
                assert_eq!((id, path.as_str(), new_text.as_str()), (Json::Num(7.0), "/w/a.txt", "two\n"));
                let acc = Json::parse(&accepted(&Json::Num(7.0), "two\n")).unwrap();
                let texts: Vec<&str> = acc
                    .at(&["result", "content"])
                    .and_then(Json::arr)
                    .unwrap()
                    .iter()
                    .filter_map(|c| c.get("text").and_then(Json::str))
                    .collect();
                assert_eq!(texts, ["FILE_SAVED", "two\n"]);
                let rej = rejected(&Json::Num(7.0), &tab);
                assert!(rej.contains("DIFF_REJECTED") && rej.contains("x1y2z3"));
                assert!(!rej.contains("TAB_CLOSED"));
            }
            other => panic!("{other:?}"),
        }
        let bad = reply(
            r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"openDiff","arguments":{"tab_name":"t"}}}"#,
        );
        assert_eq!(bad.at(&["error", "code"]), Some(&Json::Num(-32602.0)));
        let close = call(
            r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"close_tab","arguments":{"tab_name":"t1"}}}"#,
            "",
        );
        assert!(matches!(&close, Call::CloseTab { tab, reply } if tab == "t1" && reply.contains("TAB_CLOSED")));
        let all = call(
            r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"closeAllDiffTabs","arguments":{}}}"#,
            "",
        );
        assert!(matches!(all, Call::CloseAll { .. }));
    }

    #[test]
    fn locks_name_haetty_and_no_folder() {
        let j = Json::parse(&lock_json(42, "00ff")).unwrap();
        assert_eq!(j.get("workspaceFolders").and_then(Json::arr).map(<[Json]>::len), Some(0));
        assert_eq!(j.get("transport").and_then(Json::str), Some("ws"));
        assert_eq!(j.get("authToken").and_then(Json::str), Some("00ff"));
        assert!(stale_lock(&lock_json(42, "t"), |_| false));
        assert!(!stale_lock(&lock_json(42, "t"), |p| p == 42));
        assert!(!stale_lock(r#"{"pid":1,"ideName":"Neovim"}"#, |_| false));
        assert!(!stale_lock("/some/folder\n", |_| false));
        let t = token(|b| {
            b.copy_from_slice(&[0xab; 16]);
            Ok(())
        })
        .unwrap();
        assert_eq!(t, "ab".repeat(16));
        assert_eq!(lock_dir(Some("/c".as_ref()), Some(Path::new("/h"))), Some(PathBuf::from("/c/ide")));
        assert_eq!(lock_dir(Some("".as_ref()), Some(Path::new("/h"))), Some(PathBuf::from("/h/.claude/ide")));
        assert_eq!(lock_dir(None, None), None);
    }
}
