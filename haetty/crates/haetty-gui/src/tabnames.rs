//! A tab's own name (Ctrl+Shift+R), kept per session in the state directory (`tab-names.tsv`: the session id, a tab, and
//! the name). A window that reattaches the session, or a new window that takes it over, shows the name again. Before K3b
//! the name lived in the window only, so a detach and an attach lost it (kramers 2026-11-09).

use std::collections::BTreeMap;
use std::path::PathBuf;

fn path() -> PathBuf {
    haetty_proto::paths::state_dir().join("tab-names.tsv")
}

/// The own names by session id.
pub fn load() -> BTreeMap<String, String> {
    from_tsv(&std::fs::read_to_string(path()).unwrap_or_default())
}

/// Sets the own name of a session, or with `None` clears it. The name is a convenience: a failed write is logged only.
pub fn set(session: &str, name: Option<&str>) {
    let mut names = load();
    match name {
        Some(n) => names.insert(session.to_string(), n.to_string()),
        None => names.remove(session),
    };
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = p.with_extension("tsv.tmp");
    if let Err(e) = std::fs::write(&tmp, to_tsv(&names)).and_then(|()| std::fs::rename(&tmp, &p)) {
        log::warn!("tab names: {} not written: {e}", p.display());
    }
}

fn from_tsv(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let (id, name) = l.split_once('\t')?;
            (!id.is_empty() && !name.is_empty()).then(|| (id.to_string(), name.to_string()))
        })
        .collect()
}

fn to_tsv(names: &BTreeMap<String, String>) -> String {
    names.iter().map(|(id, n)| format!("{id}\t{n}\n")).collect()
}

#[cfg(test)]
mod tests {
    use super::{from_tsv, to_tsv};

    #[test]
    fn own_names_round_trip_and_bad_lines_are_skipped() {
        let names = from_tsv("s1\tbuild\n\nno-tab-here\ns2\t\ns3\tnotes (2)\n");
        assert_eq!(names.get("s1").map(String::as_str), Some("build"));
        assert_eq!(names.get("s3").map(String::as_str), Some("notes (2)"));
        assert_eq!(names.len(), 2);
        assert_eq!(from_tsv(&to_tsv(&names)), names);
    }
}
