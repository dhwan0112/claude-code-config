//! The update's cases against a local HTTPS release server (stage PU, U6): each one checks what the updater does
//! and that the installed layout (the version folders and the link) did not change unless the case installs.
//! Needs the `test-support` feature (the test key, the test CA): `cargo test -p haetty-update --features test-support`.
#![cfg(all(unix, feature = "test-support"))]

use std::path::{Path, PathBuf};
use std::time::Duration;

use haetty_update::archive::make::tar_gz;
use haetty_update::flow::{self, Found, Origin};
use haetty_update::layout::{self, Form, Installed};
use haetty_update::manifest::{Asset, Manifest};
use haetty_update::net::Https;
use haetty_update::testserver::{Route, Server, CA_PATH};
use haetty_update::version::Version;
use haetty_update::{hash, testkey};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    // SAFETY of the env change: set once, before any client is made; only this test binary reads it.
    ONCE.call_once(|| std::env::set_var("HAETTY_UPDATE_TEST_ROOT", CA_PATH));
}

/// The fake new version: prints the commit like `haetty --version`.
fn new_exe(commit: &str) -> Vec<u8> {
    format!("#!/bin/sh\necho 'haetty 0.4.0 (commit {}, built 2026-11-20, instance stable)'\n", &commit[..12])
        .into_bytes()
}

fn archive(commit: &str) -> Vec<u8> {
    tar_gz(&[("haetty", b'0', &new_exe(commit)), ("LICENSE", b'0', b"MIT")])
}

fn manifest(version: &str, min: &str, asset: &[u8], sha: Option<String>, size: Option<u64>) -> Manifest {
    Manifest {
        version: Version::parse(version).unwrap(),
        commit: COMMIT.into(),
        built: "2026-11-20T00:00:00Z".into(),
        protocol: 12,
        min_update_from: Version::parse(min).unwrap(),
        assets: vec![Asset {
            target: "x86_64-linux".into(),
            file: format!("haetty-{version}-x86_64-linux.tar.gz"),
            size: size.unwrap_or(asset.len() as u64),
            sha256: sha.unwrap_or_else(|| hash::sha256_hex(asset)),
            files: vec!["haetty".into(), "LICENSE".into()],
        }],
        notes: "### Added\n- a test".into(),
        next_keys: vec![],
    }
}

/// A server with a signed release `m` and its asset.
fn serve(srv: &Server, m: &Manifest, asset: &[u8]) {
    let text = m.to_json();
    srv.set("/r/manifest.json", Route::Bytes(text.clone().into_bytes()));
    srv.set("/r/manifest.json.minisig", Route::Bytes(testkey::sign(text.as_bytes()).into_bytes()));
    srv.set(&format!("/r/{}", m.assets[0].file), Route::Bytes(asset.to_vec()));
}

struct Env {
    base: PathBuf,
    opt: PathBuf,
    link: PathBuf,
    state: PathBuf,
    form: Form,
}

/// An installed 0.3.2 in a run directory: version folder and link.
fn env(tag: &str) -> Env {
    let base = haetty_testkit::run_dir(tag);
    let opt = base.join("p/.local/opt/haetty");
    let link = layout::link_of(&opt).unwrap();
    let old = new_exe("aaaaaaaaaaaa0000000000000000000000000000");
    let info = Installed { version: "0.3.2-aaaaaaaaaaaa", reference: "t", commit: "aaaaaaaaaaaa", source: "t" };
    layout::put_version(&opt, "0.3.2-aaaaaaaaaaaa", &[("haetty".into(), old)], &info).unwrap();
    layout::activate(&opt, &link, "0.3.2-aaaaaaaaaaaa").unwrap();
    let form = layout::detect(&opt.join("0.3.2-aaaaaaaaaaaa/haetty"), "");
    assert!(matches!(form, Form::Versioned { .. }), "{form:?}");
    let state = base.join("state/update");
    Env { base, opt, link, state, form }
}

/// The layout as it is: the folders and the link's target.
fn snapshot(e: &Env) -> (Vec<String>, Option<String>) {
    let mut v: Vec<String> = layout::list(&e.opt, &e.link).into_iter().map(|x| x.0).collect();
    v.sort();
    (v, layout::active(&e.link))
}

fn https(srv: &Server, timeout: Duration) -> Https {
    init();
    Https::new(Origin::of(&srv.url("/r")).hosts(), timeout)
}

fn current() -> Version {
    Version::parse("0.3.2").unwrap()
}

fn keys() -> Vec<String> {
    haetty_update::sign::trusted()
}

fn done(e: Env) {
    for (n, _, _) in layout::list(&e.opt, &e.link) {
        let _ = layout::remove_version(&e.opt, &n);
    }
    haetty_testkit::remove_dir_all(&e.base);
}

#[test]
fn a_newer_signed_release_installs_and_switches_the_link() {
    let srv = Server::start();
    let e = env("upd-ok");
    let a = archive(COMMIT);
    let m = manifest("0.4.0", "0.3.0", &a, None, None);
    serve(&srv, &m, &a);
    let h = https(&srv, Duration::from_secs(10));
    let origin = Origin::of(&srv.url("/r"));
    let Found::Newer(rel) = flow::check(&h, &origin, &current(), &keys()).unwrap() else { panic!("not newer") };
    assert_eq!(rel.blocked, None);
    let mut last = (0, 0);
    let files = flow::download(&h, &rel, &e.state, &mut |d, t| last = (d, t)).unwrap();
    assert_eq!(last, (a.len() as u64, a.len() as u64));
    let d = flow::install(&e.form, &rel, &files, &e.state, 2).unwrap();
    assert_eq!(d.new, "0.4.0-0123456789ab");
    assert_eq!(d.previous, "0.3.2-aaaaaaaaaaaa");
    assert_eq!(layout::active(&e.link).as_deref(), Some("0.4.0-0123456789ab"));
    assert!(e.opt.join("0.3.2-aaaaaaaaaaaa/haetty").is_file(), "the old version stays");
    assert_eq!(haetty_update::state::begin_start(&e.state, "0.4.0-0123456789ab"), haetty_update::state::Start::Watch);
    // Only the release's files were asked for, with haetty's User-Agent only (checked by the server's log of paths).
    let seen: Vec<String> = srv.knobs.lock().unwrap().seen.iter().map(|s| s.0.clone()).collect();
    assert_eq!(seen, ["/r/manifest.json", "/r/manifest.json.minisig", "/r/haetty-0.4.0-x86_64-linux.tar.gz"]);
    done(e);
}

/// Every refusal: (case, what to break) → an error or "not newer", and the layout unchanged.
#[test]
fn every_bad_release_leaves_the_layout_unchanged() {
    let srv = Server::start();
    let a = archive(COMMIT);
    type Case = (&'static str, Box<dyn Fn(&Server, &Manifest, &[u8])>);
    let sign = |s: &Server, text: &str| {
        s.set("/r/manifest.json.minisig", Route::Bytes(testkey::sign(text.as_bytes()).into_bytes()))
    };
    let cases: Vec<Case> = vec![
        (
            "no newer version",
            Box::new(|s: &Server, _: &Manifest, a: &[u8]| serve(s, &manifest("0.3.2", "0.0.0", a, None, None), a)),
        ),
        (
            "an older version signed again (no going back)",
            Box::new(|s: &Server, _: &Manifest, a: &[u8]| serve(s, &manifest("0.3.1", "0.0.0", a, None, None), a)),
        ),
        (
            "a wrong signature",
            Box::new(move |s: &Server, m: &Manifest, a: &[u8]| {
                serve(s, m, a);
                sign(s, "something else");
            }),
        ),
        (
            "no signature",
            Box::new(|s: &Server, m: &Manifest, a: &[u8]| {
                serve(s, m, a);
                s.set("/r/manifest.json.minisig", Route::Status(404));
            }),
        ),
        (
            "another key",
            Box::new(|s: &Server, m: &Manifest, a: &[u8]| {
                serve(s, m, a);
                let t = m.to_json();
                let sig = testkey::sign_with(&testkey::OTHER_SEED, true, t.as_bytes(), "x");
                s.set("/r/manifest.json.minisig", Route::Bytes(sig.into_bytes()));
            }),
        ),
        (
            "a changed manifest",
            Box::new(|s: &Server, m: &Manifest, a: &[u8]| {
                serve(s, m, a);
                let changed = m.to_json().replace("a test", "an attack");
                s.set("/r/manifest.json", Route::Bytes(changed.into_bytes()));
            }),
        ),
        (
            "the asset's sha256",
            Box::new(|s: &Server, _: &Manifest, a: &[u8]| {
                serve(s, &manifest("0.4.0", "0.0.0", a, Some("f".repeat(64)), None), a)
            }),
        ),
        (
            "the asset's size",
            Box::new(|s: &Server, _: &Manifest, a: &[u8]| {
                serve(s, &manifest("0.4.0", "0.0.0", a, None, Some(a.len() as u64 + 7)), a)
            }),
        ),
        (
            "too old for an in-app update",
            Box::new(|s: &Server, _: &Manifest, a: &[u8]| serve(s, &manifest("0.4.0", "0.3.5", a, None, None), a)),
        ),
        (
            "a redirect to another host",
            Box::new(|s: &Server, m: &Manifest, a: &[u8]| {
                serve(s, m, a);
                let port = s.port;
                s.set(
                    &format!("/r/{}", m.assets[0].file),
                    Route::Redirect(format!("https://other.localhost:{port}/x")),
                );
            }),
        ),
        (
            "an archive that escapes",
            Box::new(|s: &Server, _: &Manifest, _: &[u8]| {
                let evil = tar_gz(&[("../haetty", b'0', b"x"), ("LICENSE", b'0', b"MIT")]);
                serve(s, &manifest("0.4.0", "0.0.0", &evil, None, None), &evil)
            }),
        ),
        (
            "an archive with a link",
            Box::new(|s: &Server, _: &Manifest, _: &[u8]| {
                let evil = tar_gz(&[("haetty", b'2', b""), ("LICENSE", b'0', b"MIT")]);
                serve(s, &manifest("0.4.0", "0.0.0", &evil, None, None), &evil)
            }),
        ),
    ];
    for (i, (name, setup)) in cases.iter().enumerate() {
        srv.knobs.lock().unwrap().routes.clear();
        let e = env(&format!("upd-bad-{i}"));
        let before = snapshot(&e);
        let m = manifest("0.4.0", "0.0.0", &a, None, None);
        setup(&srv, &m, &a);
        let h = https(&srv, Duration::from_secs(10));
        let origin = Origin::of(&srv.url("/r"));
        let outcome = match flow::check(&h, &origin, &current(), &keys()) {
            Err(why) => format!("check refused: {why}"),
            Ok(Found::UpToDate(v)) => format!("not newer ({v})"),
            Ok(Found::Newer(rel)) => match flow::download(&h, &rel, &e.state, &mut |_, _| {}) {
                Err(why) => format!("download refused: {why}"),
                Ok(files) => match flow::install(&e.form, &rel, &files, &e.state, 2) {
                    Err(why) => format!("install refused: {why}"),
                    Ok(d) => panic!("{name}: installed {d:?}"),
                },
            },
        };
        assert_eq!(snapshot(&e), before, "{name}: the layout changed");
        let left: Vec<_> = std::fs::read_dir(&e.state)
            .map(|r| r.filter_map(|x| x.ok()).map(|x| x.file_name()).collect())
            .unwrap_or_default();
        assert!(left.iter().all(|n| !n.to_string_lossy().ends_with(".part")), "{name}: a download was left: {left:?}");
        eprintln!("{name}: {outcome}");
        assert!(!outcome.is_empty());
        done(e);
    }
}

#[test]
fn a_broken_download_resumes_and_slow_or_absent_servers_fail_in_time() {
    let srv = Server::start();
    let e = env("upd-resume");
    // A larger asset, cut once after 5000 bytes.
    // Incompressible bytes, so the asset is large.
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    let big: Vec<u8> = (0..200_000)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect();
    let a = tar_gz(&[("haetty", b'0', &new_exe(COMMIT)), ("LICENSE", b'0', &big)]);
    let m = manifest("0.4.0", "0.0.0", &a, None, None);
    serve(&srv, &m, &a);
    srv.knobs.lock().unwrap().cut_once = Some(5000);
    let h = https(&srv, Duration::from_secs(10));
    let origin = Origin::of(&srv.url("/r"));
    let Found::Newer(rel) = flow::check(&h, &origin, &current(), &keys()).unwrap() else { panic!() };
    let first = flow::download(&h, &rel, &e.state, &mut |_, _| {});
    assert!(first.is_err(), "the cut download fails");
    let part = e.state.join(format!("{}.part", m.assets[0].file));
    let kept = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    assert!(kept > 0, "the part is kept for a resume");
    flow::download(&h, &rel, &e.state, &mut |_, _| {}).expect("resumed");
    let ranges: Vec<Option<String>> = srv.knobs.lock().unwrap().seen.iter().map(|s| s.1.clone()).collect();
    assert!(ranges.contains(&Some(format!("bytes={kept}-"))), "{ranges:?}");
    // Slow: the server waits 3 s, the client gives up after 1 s.
    srv.knobs.lock().unwrap().delay = Duration::from_secs(3);
    let t = std::time::Instant::now();
    let slow = https(&srv, Duration::from_secs(1));
    assert!(flow::check(&slow, &origin, &current(), &keys()).is_err());
    assert!(t.elapsed() < Duration::from_secs(3), "timed out in time: {:?}", t.elapsed());
    // No server: a closed port fails at once.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let none = Origin::of(&format!("https://localhost:{port}/r"));
    let h2 = Https::new(none.hosts(), Duration::from_secs(5));
    let t = std::time::Instant::now();
    assert!(flow::check(&h2, &none, &current(), &keys()).is_err());
    assert!(t.elapsed() < Duration::from_secs(5));
    // Plain http and other hosts are refused before any connection.
    let h3 = Https::new(vec!["localhost".into()], Duration::from_secs(5));
    use haetty_update::flow::Fetch;
    assert!(h3.get("http://localhost/x", 10).unwrap_err().contains("https"));
    assert!(h3.get("https://example.invalid/x", 10).unwrap_err().contains("not a release host"));
    done(e);
}

#[test]
fn a_full_disk_or_a_read_only_folder_installs_nothing() {
    let srv = Server::start();
    let a = archive(COMMIT);
    let m = manifest("0.4.0", "0.0.0", &a, None, None);
    serve(&srv, &m, &a);
    let h = https(&srv, Duration::from_secs(10));
    let origin = Origin::of(&srv.url("/r"));
    let Found::Newer(rel) = flow::check(&h, &origin, &current(), &keys()).unwrap() else { panic!() };
    // Disk full: the download's file is /dev/full.
    let e = env("upd-full");
    let before = snapshot(&e);
    std::fs::create_dir_all(&e.state).unwrap();
    if Path::new("/dev/full").exists() {
        std::os::unix::fs::symlink("/dev/full", e.state.join(format!("{}.part", m.assets[0].file))).unwrap();
        let r = flow::download(&h, &rel, &e.state, &mut |_, _| {});
        assert!(r.is_err(), "{r:?}");
        assert_eq!(snapshot(&e), before);
    }
    done(e);
    // Read-only: the versions folder cannot be written (form d) — refused before anything is written.
    let e = env("upd-ro");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&e.opt, std::fs::Permissions::from_mode(0o555)).unwrap();
    let form = layout::detect(&e.opt.join("0.3.2-aaaaaaaaaaaa/haetty"), "");
    let files = flow::download(&h, &rel, &e.state, &mut |_, _| {}).unwrap();
    if matches!(form, Form::NoWrite(_)) {
        assert!(flow::install(&form, &rel, &files, &e.state, 2).unwrap_err().contains("by hand"));
    }
    std::fs::set_permissions(&e.opt, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(layout::active(&e.link).as_deref(), Some("0.3.2-aaaaaaaaaaaa"));
    done(e);
    // A dev instance and a cargo build are never updated.
    assert!(
        flow::install(&Form::Dev("a cargo build".into()), &rel, &files_of(&a), Path::new("/nonexistent"), 2).is_err()
    );
}

fn files_of(a: &[u8]) -> Vec<(String, Vec<u8>)> {
    haetty_update::archive::untar_gz(a, &["haetty".into(), "LICENSE".into()]).unwrap()
}

/// The single-file form (b): the executable replaced, the old one beside it; a new version that reports another
/// commit is refused before anything is replaced.
#[test]
fn a_single_executable_is_replaced_and_a_bad_one_is_not() {
    let base = haetty_testkit::run_dir("upd-single");
    let exe = base.join("haetty");
    std::fs::write(&exe, b"old").unwrap();
    let a = archive(COMMIT);
    let rel =
        flow::Release { manifest: manifest("0.4.0", "0.0.0", &a, None, None), base: String::new(), blocked: None };
    let state = base.join("state");
    let form = layout::detect(&exe, "");
    assert_eq!(form, Form::Single { exe: std::fs::canonicalize(&exe).unwrap() });
    // The new executable reports another commit: refused, nothing replaced.
    let wrong = haetty_update::archive::untar_gz(
        &archive("bbbbbbbbbbbb0000000000000000000000000000"),
        &["haetty".into(), "LICENSE".into()],
    )
    .unwrap();
    assert!(flow::install(&form, &rel, &wrong, &state, 2).is_err());
    assert_eq!(std::fs::read(&exe).unwrap(), b"old");
    flow::install(&form, &rel, &files_of(&a), &state, 2).unwrap();
    assert_eq!(std::fs::read(&exe).unwrap(), new_exe(COMMIT));
    assert_eq!(std::fs::read(base.join("haetty.old")).unwrap(), b"old");
    haetty_testkit::remove_dir_all(&base);
}
