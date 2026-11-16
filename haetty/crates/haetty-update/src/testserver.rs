//! TEST ONLY (feature `test-support`): a small HTTPS release server on 127.0.0.1 with the TEST-ONLY certificate
//! (`tests/TEST-ONLY-server.pem`, signed by `tests/TEST-ONLY-ca.pem`), for the update tests and the end-to-end
//! scripts (`examples/fake_release_server.rs`). It serves files by path, honours `Range`, and can redirect, answer
//! late, or break a download off once.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

const CERT: &str = include_str!("../tests/TEST-ONLY-server.pem");
const KEY: &str = include_str!("../tests/TEST-ONLY-server.key");
/// The CA the client must trust (`HAETTY_UPDATE_TEST_ROOT`).
pub const CA_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/TEST-ONLY-ca.pem");

#[derive(Clone, Debug)]
pub enum Route {
    Bytes(Vec<u8>),
    /// 302 to this URL.
    Redirect(String),
    Status(u16),
}

#[derive(Default)]
pub struct Knobs {
    pub routes: HashMap<String, Route>,
    /// Wait this long before every answer.
    pub delay: Duration,
    /// Break the next full-file answer off after this many body bytes (once).
    pub cut_once: Option<usize>,
    /// Requests seen (path and the Range header), for the tests.
    pub seen: Vec<(String, Option<String>)>,
}

pub struct Server {
    pub port: u16,
    pub knobs: Arc<Mutex<Knobs>>,
}

impl Server {
    pub fn start() -> Server {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let certs: Vec<CertificateDer<'static>> =
            CertificateDer::pem_slice_iter(CERT.as_bytes()).collect::<Result<_, _>>().expect("test cert");
        let key = PrivateKeyDer::from_pem_slice(KEY.as_bytes()).expect("test key");
        let cfg = Arc::new(
            rustls::ServerConfig::builder().with_no_client_auth().with_single_cert(certs, key).expect("server config"),
        );
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let knobs = Arc::new(Mutex::new(Knobs::default()));
        let k = knobs.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let cfg = cfg.clone();
                let k = k.clone();
                std::thread::spawn(move || {
                    let Ok(conn) = rustls::ServerConnection::new(cfg) else { return };
                    let mut tls = rustls::StreamOwned::new(conn, stream);
                    // A clean end only after a whole answer: a cut download must look broken.
                    if serve(&mut tls, &k).is_ok() {
                        tls.conn.send_close_notify();
                        let _ = tls.flush();
                    }
                });
            }
        });
        Server { port, knobs }
    }

    pub fn url(&self, path: &str) -> String {
        format!("https://localhost:{}{path}", self.port)
    }

    pub fn set(&self, path: &str, r: Route) {
        self.knobs.lock().unwrap().routes.insert(path.to_string(), r);
    }
}

fn serve(s: &mut (impl Read + Write), knobs: &Mutex<Knobs>) -> std::io::Result<()> {
    let mut head = vec![];
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if s.read(&mut b)? == 0 || head.len() > 16 * 1024 {
            return Ok(());
        }
        head.push(b[0]);
    }
    let head = String::from_utf8_lossy(&head).to_string();
    let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
    let range =
        head.lines().find_map(|l| l.strip_prefix("Range: ").or_else(|| l.strip_prefix("range: "))).map(str::to_string);
    let (route, delay, cut) = {
        let mut k = knobs.lock().unwrap();
        k.seen.push((path.clone(), range.clone()));
        (k.routes.get(&path).cloned(), k.delay, k.cut_once)
    };
    std::thread::sleep(delay);
    match route {
        None => write!(s, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
        Some(Route::Status(c)) => write!(s, "HTTP/1.1 {c} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
        Some(Route::Redirect(u)) => {
            write!(s, "HTTP/1.1 302 Found\r\nLocation: {u}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        }
        Some(Route::Bytes(data)) => {
            let from = range
                .as_deref()
                .and_then(|r| r.strip_prefix("bytes="))
                .and_then(|r| r.trim_end_matches('-').parse::<usize>().ok())
                .filter(|f| *f < data.len());
            let body = &data[from.unwrap_or(0)..];
            match from {
                Some(f) => write!(
                    s,
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {f}-{}/{}\r\nConnection: close\r\n\r\n",
                    body.len(),
                    data.len() - 1,
                    data.len()
                )?,
                None => write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())?,
            }
            if let (None, Some(n)) = (from, cut.filter(|n| *n < body.len())) {
                knobs.lock().unwrap().cut_once = None;
                s.write_all(&body[..n.min(body.len())])?;
                s.flush()?;
                return Err(std::io::Error::other("cut"));
            }
            s.write_all(body)
        }
    }
}
