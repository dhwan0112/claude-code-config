//! `haetty completions SHELL` driven by the real shells: bash on Unix (the
//! completion function called as bash would), PowerShell's `TabExpansion2`
//! on Windows. Skipped when the shell is missing.

mod common;

use std::process::Command;

fn script(shell: &str) -> String {
    let out = Command::new(common::bin()).args(["completions", shell]).env("HAETTY_INSTANCE", "dev").output().unwrap();
    assert!(out.status.success(), "{out:?}");
    String::from_utf8(out.stdout).unwrap()
}

/// (words typed so far, the last one being completed) → expected candidates.
const CASES: &[(&str, &str)] = &[
    ("haetty ev", "events"),
    ("haetty events --f", "--follow"),
    ("haetty completions ", "bash zsh fish powershell"),
    ("haetty --obs", "--observe-new-tabs"),
    ("haetty status --", "--json --screen --help"),
    ("haetty gui --font-s", "--font-size"),
];

#[cfg(unix)]
#[test]
fn bash_completes() {
    if Command::new("bash").arg("-c").arg("true").status().is_err() {
        eprintln!("SKIP: no bash");
        return;
    }
    let dir = haetty_testkit::run_dir("completions");
    let f = dir.join("haetty.bash");
    std::fs::write(&f, script("bash")).unwrap();
    for (line, want) in CASES {
        let mut words: Vec<&str> = line.split(' ').collect();
        if line.ends_with(' ') {
            words.pop();
            words.push("");
        }
        let arr: Vec<String> = words.iter().map(|w| format!("'{w}'")).collect();
        let prog = format!(
            ". '{}'; COMP_WORDS=({}); COMP_CWORD={}; _haetty; echo \"${{COMPREPLY[*]}}\"",
            f.display(),
            arr.join(" "),
            words.len() - 1
        );
        let out = Command::new("bash").arg("-c").arg(&prog).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), *want, "{line:?}: {out:?}");
    }
    haetty_testkit::remove_dir_all(&dir);
}

#[cfg(windows)]
#[test]
fn powershell_completes() {
    let dir = haetty_testkit::run_dir("completions");
    let f = dir.join("haetty.ps1");
    std::fs::write(&f, script("powershell")).unwrap();
    for (line, want) in CASES {
        let cmd = format!(
            ". '{}'; $r = TabExpansion2 -inputScript '{line}' -cursorColumn {}; ($r.CompletionMatches | ForEach-Object CompletionText) -join ' '",
            f.display(),
            line.len()
        );
        let out =
            match Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", &cmd]).output() {
                Ok(o) => o,
                Err(_) => {
                    eprintln!("SKIP: no powershell");
                    return;
                }
            };
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), *want, "{line:?}: {out:?}");
    }
    haetty_testkit::remove_dir_all(&dir);
}
