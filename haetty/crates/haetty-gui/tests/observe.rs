//! The observe-only interface (DESIGN.md §6.1) against a real daemon:
//! a session nobody allowed is invisible (screen denied, no events), an
//! allowed one is visible; an Observe connection cannot type or change
//! observability; following the event log delivers only allowed events; a
//! cursor continues where it stopped, also across a daemon restart.

mod common;

use std::io::ErrorKind;
use std::time::{Duration, Instant};

use common::*;
use haetty_proto::wire::{ClientMsg, DaemonMsg, LogEvent, Origin, Secret};
use haetty_proto::{Client, ObserverClient, Role};

/// A program that prints `marker` and then waits (Unix: copies its input
/// to `input`, so typed bytes would show there).
fn marked(marker: &str, input: &std::path::Path) -> haetty_proto::wire::CreateSpec {
    #[cfg(unix)]
    {
        spec("sh", &["-c", &format!("echo {marker}; cat > '{}'", input.display())])
    }
    #[cfg(windows)]
    {
        let _ = input;
        spec("cmd", &["/c", &format!("echo {marker}& ping -n 120 127.0.0.1 > nul")])
    }
}

fn wait_screen(o: &mut ObserverClient, id: &str, needle: &str) -> String {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let v = o.screen(id).expect("screen of an observable session");
        if v.text.0.contains(needle) || Instant::now() > end {
            return v.text.0;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn kinds(ev: &[LogEvent]) -> Vec<String> {
    ev.iter()
        .map(|e| {
            format!(
                "{}:{}:{}",
                e.session,
                e.kind,
                e.fields.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(",")
            )
        })
        .collect()
}

#[test]
fn observe_only_interface() {
    let dir = haetty_testkit::run_dir("observe");
    let ep = endpoint(&dir, "observe");
    let mut d = daemon(&dir, &ep);
    let c = connect(&ep, "test-control");
    let a_in = dir.join("a-input.txt");
    let b_in = dir.join("b-input.txt");
    let a = c.create(marked("hidden-a", &a_in)).unwrap();
    let b = c.create(marked("shown-b", &b_in)).unwrap();
    c.set_observable(&b, true).unwrap();
    assert!(c.sync(Duration::from_secs(5)));

    let mut o = ObserverClient::connect(&ep, "test-observer").unwrap();
    // Not allowed: no screen.
    let e = o.screen(&a).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::PermissionDenied, "{e}");
    // Allowed: the screen and its state.
    assert!(wait_screen(&mut o, &b, "shown-b").contains("shown-b"));
    let v = o.screen(&b).unwrap();
    assert_eq!((v.id.as_str(), v.claude.is_none()), (b.as_str(), true));
    // The session list says which.
    let list = o.sessions().unwrap();
    let flag = |id: &str| list.iter().find(|s| s.id == id).map(|s| s.observable);
    assert_eq!((flag(&a), flag(&b)), (Some(false), Some(true)));

    // Events: only B's, and only from the moment it was allowed.
    let (ev, cursor) = o.events(0).unwrap();
    assert!(!ev.is_empty() && ev.iter().all(|e| e.session == b), "{:?}", kinds(&ev));
    assert_eq!(kinds(&ev[..1]), [format!("{b}:observe:on=1")], "B's creation came before it was allowed");
    assert!(cursor >= ev.last().unwrap().seq);

    // An Observe connection cannot type, resize, close or allow observing.
    let raw = Client::connect(&ep, Role::Observe, "test-raw-observer").unwrap();
    let tries = [
        ClientMsg::Paste { session: b.clone(), origin: Origin::Human, text: Secret("typed-by-observer\n".into()) },
        ClientMsg::Write { session: b.clone(), origin: Origin::Human, bytes: Secret(b"written\n".to_vec()) },
        ClientMsg::Close { session: b.clone() },
        ClientMsg::SetObservable { session: a.clone(), on: true },
    ];
    for m in &tries {
        raw.send(m).unwrap();
        match raw.next_reply(Duration::from_secs(5)) {
            Some(DaemonMsg::Denied { what }) => assert!(what.contains("as Observe"), "{what}"),
            other => panic!("{m:?}: expected Denied, got {other:?}"),
        }
    }
    // A Control connection cannot use the observer's requests either.
    c.send(&ClientMsg::GetScreen { session: b.clone() }).unwrap();
    assert!(matches!(c.next_reply(Duration::from_secs(5)), Some(DaemonMsg::Denied { .. })));
    std::thread::sleep(Duration::from_millis(500));
    #[cfg(unix)]
    assert_eq!(std::fs::read_to_string(&b_in).unwrap_or_default(), "", "nothing reached the program");
    assert!(o.screen(&a).is_err(), "A is still not observable");

    // Following: B stopped, A and B closed, D allowed — the follower gets B's
    // "observe off" and D's "observe on", nothing of A, nothing of B after off.
    let (tx, rx) = std::sync::mpsc::channel();
    let ep2 = ep.clone();
    let follower = std::thread::spawn(move || {
        let mut f = ObserverClient::connect(&ep2, "test-follower").unwrap();
        f.follow(cursor, |e| {
            tx.send(e.clone()).unwrap();
            !(e.kind == "observe" && e.fields.iter().any(|(k, v)| k == "on" && v == "1"))
        })
    });
    std::thread::sleep(Duration::from_millis(300));
    c.set_observable(&b, false).unwrap();
    c.close(&a).unwrap();
    c.close(&b).unwrap();
    let dd = c.create(marked("shown-d", &dir.join("d-input.txt"))).unwrap();
    c.set_observable(&dd, true).unwrap();
    follower.join().unwrap().unwrap();
    let got: Vec<LogEvent> = rx.try_iter().collect();
    assert_eq!(kinds(&got), [format!("{b}:observe:on=0"), format!("{dd}:observe:on=1")]);

    // The cursor continues: only what came after it.
    let (after, cursor2) = o.events(cursor).unwrap();
    assert_eq!(kinds(&after), kinds(&got));
    assert!(o.events(cursor2).unwrap().0.is_empty());

    // A daemon restart: the log continues; what was allowed in the old run is
    // not shown by the new one; a session allowed now is.
    drop(o);
    drop(raw);
    drop(c);
    d.kill().unwrap();
    d.wait().unwrap();
    let mut d2 = daemon(&dir, &ep);
    let c2 = connect(&ep, "test-control-2");
    let e2 = c2.create(marked("shown-e", &dir.join("e-input.txt"))).unwrap();
    c2.set_observable(&e2, true).unwrap();
    assert!(c2.sync(Duration::from_secs(5)));
    let mut o2 = ObserverClient::connect(&ep, "test-observer-2").unwrap();
    let (all, _) = o2.events(0).unwrap();
    assert!(all.iter().all(|e| e.session == e2), "only the new run's allowed session: {:?}", kinds(&all));
    let (since, _) = o2.events(cursor2).unwrap();
    assert!(since.iter().all(|e| e.seq > cursor2 && e.session == e2) && !since.is_empty(), "{:?}", kinds(&since));
    assert!(wait_screen(&mut o2, &e2, "shown-e").contains("shown-e"));
    drop(o2);
    drop(c2);
    d2.kill().unwrap();
    d2.wait().unwrap();
    haetty_testkit::remove_dir_all(&dir);
}
