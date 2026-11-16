//! TEST ONLY: serves a folder as a release over HTTPS on 127.0.0.1 with the TEST-ONLY certificate, for
//! `scripts/e2e/update.sh`. Every file of DIR is served at `/r/<name>`; `manifest.json` is signed with the test key on
//! each (re)load (`/r/manifest.json.minisig`). The port is written to DIR/port; touching DIR/reload reads the folder
//! again. usage: fake_release_server DIR

#[cfg(windows)]
fn main() {
    eprintln!(
        "fake_release_server runs on Linux (it uses rustls, which the Windows build does not link); run it in WSL"
    );
}

#[cfg(not(windows))]
use std::time::Duration;

#[cfg(not(windows))]
use haetty_update::testkey;
#[cfg(not(windows))]
use haetty_update::testserver::{Route, Server};

#[cfg(not(windows))]
fn load(srv: &Server, dir: &std::path::Path) {
    srv.knobs.lock().unwrap().routes.clear();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name == "port" || name == "reload" || !e.path().is_file() {
            continue;
        }
        let data = std::fs::read(e.path()).unwrap();
        if name == "manifest.json" {
            srv.set("/r/manifest.json.minisig", Route::Bytes(testkey::sign(&data).into_bytes()));
        }
        srv.set(&format!("/r/{name}"), Route::Bytes(data));
    }
}

#[cfg(not(windows))]
fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("usage: fake_release_server DIR"));
    let srv = Server::start();
    load(&srv, &dir);
    std::fs::write(dir.join("port"), srv.port.to_string()).unwrap();
    loop {
        std::thread::sleep(Duration::from_millis(200));
        if std::fs::remove_file(dir.join("reload")).is_ok() {
            load(&srv, &dir);
        }
    }
}
