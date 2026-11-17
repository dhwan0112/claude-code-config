//! HTTPS for the updates (stage PU, U0's choice): `ureq` 3 — on Linux with rustls on the `ring` provider and the
//! system's trust store (`rustls-platform-verifier`), on Windows with the system's Schannel (`native-tls`) and its
//! certificate store. HTTPS only; redirects are
//! followed by hand, only to the origin's hosts (`flow::Origin::hosts`); there is no setting that turns certificate
//! checks off. `HTTPS_PROXY` / `ALL_PROXY` / `NO_PROXY` are honoured (ureq reads them). The only header besides
//! what HTTP needs is `User-Agent: haetty/<version>`; no identifier, statistic or session information is sent.

use std::io::{Read, Write};
use std::time::Duration;

use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

use crate::flow::Fetch;

/// One request's limit (connect, headers, body).
pub const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_REDIRECTS: usize = 5;

pub struct Https {
    agent: ureq::Agent,
    hosts: Vec<String>,
}

impl Https {
    /// `hosts`: where requests and redirects may go; `version`: this build's, for the User-Agent.
    pub fn new(hosts: Vec<String>, timeout: Duration) -> Https {
        #[cfg(not(windows))]
        let _ = rustls::crypto::ring::default_provider().install_default();
        let provider = if cfg!(windows) { TlsProvider::NativeTls } else { TlsProvider::Rustls };
        let roots = test_roots().unwrap_or(RootCerts::PlatformVerifier);
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .max_redirects(0)
            .https_only(true)
            .http_status_as_error(false)
            .user_agent(format!("haetty/{}", env!("CARGO_PKG_VERSION")))
            .proxy(ureq::Proxy::try_from_env())
            .tls_config(TlsConfig::builder().provider(provider).root_certs(roots).build())
            .build()
            .into();
        Https { agent, hosts }
    }

    fn allowed(&self, url: &str) -> Result<(), String> {
        match crate::flow::host_of(url) {
            Some(h) if self.hosts.contains(&h) => Ok(()),
            Some(h) => Err(format!("refused: {h} is not a release host")),
            None => Err("refused: not an https:// address".into()),
        }
    }

    /// Follows up to five redirects within the allowed hosts; the final response.
    fn open(&self, url: &str, from: u64) -> Result<ureq::http::Response<ureq::Body>, String> {
        let mut url = url.to_string();
        for _ in 0..=MAX_REDIRECTS {
            self.allowed(&url)?;
            let mut req = self.agent.get(&url);
            if from > 0 {
                req = req.header("Range", &format!("bytes={from}-"));
            }
            let r = req.call().map_err(|e| format!("{}: {e}", crate::flow::loggable(&url)))?;
            let st = r.status().as_u16();
            if (300..400).contains(&st) {
                let loc =
                    r.headers().get("location").and_then(|v| v.to_str().ok()).ok_or("redirect without a target")?;
                url = if loc.starts_with("https://") {
                    loc.to_string()
                } else if loc.starts_with('/') {
                    let host = crate::flow::host_of(&url).ok_or("redirect")?;
                    format!("https://{host}{loc}")
                } else {
                    return Err("refused: a redirect away from https".into());
                };
                continue;
            }
            if st == 200 || (st == 206 && from > 0) {
                return Ok(r);
            }
            return Err(format!("{}: HTTP {st}", crate::flow::loggable(&url)));
        }
        Err("too many redirects".into())
    }
}

impl Fetch for Https {
    fn get(&self, url: &str, max: usize) -> Result<Vec<u8>, String> {
        let r = self.open(url, 0)?;
        let mut v = vec![];
        r.into_body()
            .into_reader()
            .take(max as u64 + 1)
            .read_to_end(&mut v)
            .map_err(|e| format!("{}: {e}", crate::flow::loggable(url)))?;
        if v.len() > max {
            return Err(format!("{}: larger than {max} bytes", crate::flow::loggable(url)));
        }
        Ok(v)
    }

    fn get_to(
        &self,
        url: &str,
        from: u64,
        out: &mut std::fs::File,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<(), String> {
        let r = self.open(url, from)?;
        // A server that ignored the range sends everything: start the file again.
        let resumed = r.status().as_u16() == 206;
        if from > 0 && !resumed {
            out.set_len(0).map_err(|e| e.to_string())?;
            use std::io::Seek;
            out.seek(std::io::SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        }
        let start = if resumed { from } else { 0 };
        let total = r
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .map_or(0, |n| n + start);
        let mut body = r.into_body().into_reader();
        let mut buf = vec![0u8; 64 * 1024];
        let mut done = start;
        loop {
            let n = body.read(&mut buf).map_err(|e| format!("download broke off at {done} bytes: {e}"))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| format!("writing the download: {e}"))?;
            done += n as u64;
            progress(done, total);
        }
        Ok(())
    }
}

/// Test builds only: trust the test CA in `$HAETTY_UPDATE_TEST_ROOT` (PEM) instead of the system store.
#[cfg(all(feature = "test-support", not(windows)))]
fn test_roots() -> Option<RootCerts> {
    use rustls::pki_types::pem::PemObject;
    let path = std::env::var_os("HAETTY_UPDATE_TEST_ROOT")?;
    let certs: Vec<ureq::tls::Certificate<'static>> = rustls::pki_types::CertificateDer::pem_file_iter(path)
        .ok()?
        .filter_map(Result::ok)
        .map(|c| ureq::tls::Certificate::from_der(c.as_ref()).to_owned())
        .collect();
    Some(RootCerts::new_with_certs(&certs))
}

/// Test builds only, Windows: the same variable for Schannel. ureq hands the certificates to native-tls as the only
/// roots of this connection (an in-memory store; the Windows certificate stores are neither read nor changed).
#[cfg(all(feature = "test-support", windows))]
fn test_roots() -> Option<RootCerts> {
    let pem = std::fs::read(std::env::var_os("HAETTY_UPDATE_TEST_ROOT")?).ok()?;
    let certs: Vec<ureq::tls::Certificate<'static>> = ureq::tls::parse_pem(&pem)
        .filter_map(Result::ok)
        .filter_map(|item| match item {
            ureq::tls::PemItem::Certificate(c) => Some(c),
            _ => None,
        })
        .collect();
    (!certs.is_empty()).then(|| RootCerts::new_with_certs(&certs))
}

#[cfg(not(feature = "test-support"))]
fn test_roots() -> Option<RootCerts> {
    None
}
