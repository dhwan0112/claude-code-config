//! Help texts and shell completions, from one table of subcommands and the
//! settings table ([`crate::config::SETTINGS`]): `haetty --help`, `haetty
//! SUBCOMMAND --help` and `haetty completions bash|zsh|fish|powershell` say
//! the same things. No dependency: the scripts are plain text.

use crate::config::SETTINGS;

/// A subcommand: its usage line, one line about it, its options.
pub struct Sub {
    pub name: &'static str,
    pub usage: &'static str,
    pub about: &'static str,
    pub options: &'static [(&'static str, &'static str)],
}

/// The window's own options (`haetty [gui] …`), besides the settings.
pub const GUI_OPTIONS: &[(&str, &str)] = &[
    ("--font FAMILY", "font family (default: a monospace font found on the system)"),
    ("--font-size PT", "font size in points, 4–96 (default 11)"),
    ("--cols N", "columns of the first window (default 120)"),
    ("--rows N", "rows of the first window (default 36)"),
    ("--version, -V", "version, commit, build date and instance"),
    ("--help, -h", "this help"),
];

pub const SUBS: &[Sub] = &[
    Sub {
        name: "gui",
        usage: "haetty [gui] [OPTIONS] [--] [PROGRAM [ARGS...]]",
        about: "open a window running PROGRAM (default: your shell); `gui` may be left out",
        options: &[],
    },
    Sub {
        name: "daemon",
        usage: "haetty daemon",
        about: "serve the sessions (the window starts it on demand; it exits 10 s after the last session and window)",
        options: &[],
    },
    Sub {
        name: "status",
        usage: "haetty status [--json | --screen ID]",
        about: "list the daemon's sessions, read-only (never starts a daemon)",
        options: &[
            ("--json", "as JSON, with the orchestrator's workers"),
            ("--screen ID", "one session's screen as text"),
        ],
    },
    Sub {
        name: "events",
        usage: "haetty events [--follow] [--json] [--since CURSOR]",
        about: "the event log of observable tabs, read-only (DESIGN.md §6.1)",
        options: &[
            ("--follow, -f", "keep printing new events until the daemon ends"),
            ("--json", "one JSON object per line"),
            ("--since CURSOR", "only events after this seq"),
        ],
    },
    Sub {
        name: "screen",
        usage: "haetty screen [--json] SESSION",
        about: "an observable tab's screen and state, read-only (exit 3 when not observable)",
        options: &[("--json", "as JSON")],
    },
    Sub {
        name: "run",
        usage: "haetty run [--summary-lines N] -- COMMAND [ARGS...]",
        about: "run a command quietly: its output goes to a run log; the terminal gets the exit code, the error lines and the last lines",
        options: &[("--summary-lines N", "how many last lines to show (default 10)")],
    },
    Sub {
        name: "log",
        usage: "haetty log [FILE] [--grep TEXT] [--lines A:B] [--tail N]",
        about: "read a run log in parts (no FILE: the saved runs, newest first; by default the last 50 lines)",
        options: &[
            ("--grep TEXT", "only the lines containing TEXT"),
            ("--lines A:B", "only the lines A to B (1-based, inclusive)"),
            ("--tail N", "only the last N of the selected lines"),
        ],
    },
    Sub {
        name: "tokens",
        usage: "haetty tokens [--top N] [PATH...]",
        about: "token use of Claude Code sessions from their transcripts, read-only; PATH is a .jsonl file or a folder to search (default: the newest under ~/.claude/projects)",
        options: &[
            ("--top N", "how many of the largest tool results to list (default 10)"),
        ],
    },
    Sub {
        name: "install-desktop",
        usage: "haetty install-desktop",
        about: "install the desktop entry and icons (Linux; nothing to do on Windows)",
        options: &[],
    },
    Sub {
        name: "completions",
        usage: "haetty completions bash|zsh|fish|powershell",
        about: "print a shell completion script",
        options: &[],
    },
    Sub {
        name: "hook",
        usage: "haetty hook EVENT",
        about: "run by Claude Code for its hooks (reads stdin, never prints)",
        options: &[],
    },
    Sub {
        name: "statusline",
        usage: "haetty statusline",
        about: "run by Claude Code for its status line (never prints)",
        options: &[],
    },
];

/// Reserved subcommand names (they end with an error, see `main.rs`).
pub const RESERVED: &[&str] = &["run", "headless"];
pub const SHELLS: &[&str] = &["bash", "zsh", "fish", "powershell"];

fn opt_lines(opts: &[(&str, &str)]) -> String {
    let w = opts.iter().map(|(o, _)| o.chars().count()).max().unwrap_or(0);
    opts.iter().map(|(o, d)| format!("  {o:<w$}  {d}\n")).collect()
}

/// `haetty --help`.
pub fn main_help() -> String {
    let w = SUBS.iter().map(|s| s.name.len()).max().unwrap_or(0);
    let subs: String = SUBS.iter().map(|s| format!("  {:<w$}  {}\n", s.name, s.about.replace('`', ""))).collect();
    format!(
        "usage: haetty [gui] [OPTIONS] [--] [PROGRAM [ARGS...]]\n       haetty SUBCOMMAND [ARGS...]   (haetty SUBCOMMAND --help)\n\n\
         Open a haetty window running PROGRAM (default: your shell).\n\n\
         options:\n{}\n\
         settings (--KEY VALUE or --KEY=VALUE; the same keys go in the config file):\n{}\n\
         subcommands:\n{subs}  reserved: {}\n\n\
         To run a program named like a subcommand: haetty [OPTIONS] -- NAME [ARGS...]",
        opt_lines(GUI_OPTIONS),
        crate::config::help_lines(),
        RESERVED.join(", ")
    )
}

/// `haetty SUBCOMMAND --help` (`gui` gives the main help).
pub fn sub_help(name: &str) -> Option<String> {
    if name == "gui" {
        return Some(main_help());
    }
    let s = SUBS.iter().find(|s| s.name == name)?;
    let mut opts = s.options.to_vec();
    opts.push(("--help, -h", "this help"));
    let t = format!("usage: {}\n\n{}.\n\noptions:\n{}", s.usage, capital(&s.about.replace('`', "")), opt_lines(&opts));
    Some(t)
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// The option words of a list (`--since CURSOR` → `--since`; `--follow, -f`
/// → both).
fn words(opts: &[(&str, &str)]) -> Vec<String> {
    opts.iter()
        .flat_map(|(o, _)| o.split(", ").map(|w| w.split(' ').next().unwrap_or("").to_string()))
        .filter(|w| !w.is_empty())
        .collect()
}

/// Words after `haetty` when no subcommand is given: subcommands, the
/// window's options and every setting as `--KEY`.
fn top_words() -> Vec<String> {
    let mut v: Vec<String> = SUBS.iter().map(|s| s.name.to_string()).collect();
    v.extend(words(GUI_OPTIONS));
    v.extend(SETTINGS.iter().filter(|r| !r.file_only).flat_map(|r| r.keys.iter().map(|k| format!("--{k}"))));
    v
}

/// The window's options and settings (after `haetty gui`, or after options).
fn gui_words() -> Vec<String> {
    top_words().into_iter().filter(|w| w.starts_with('-')).collect()
}

/// Words after `haetty SUB`.
fn sub_words(s: &Sub) -> Vec<String> {
    if s.name == "completions" {
        return SHELLS.iter().map(|x| x.to_string()).collect();
    }
    let mut v = words(s.options);
    v.extend(["--help".to_string(), "-h".to_string()]);
    v
}

/// `haetty completions SHELL`.
pub fn completions(shell: &str) -> Option<String> {
    let subs: Vec<&Sub> = SUBS.iter().filter(|s| s.name != "gui").collect();
    Some(match shell {
        "bash" => {
            let mut cases = String::new();
            for s in &subs {
                cases.push_str(&format!("        {}) words=\"{}\" ;;\n", s.name, sub_words(s).join(" ")));
            }
            format!(
                "# haetty completion for bash. Load it with\n#   eval \"$(haetty completions bash)\"\n\
                 # (e.g. in ~/.bashrc), or save it as ~/.local/share/bash-completion/completions/haetty.\n\
                 _haetty() {{\n    local cur=${{COMP_WORDS[COMP_CWORD]}} words\n    if [ \"$COMP_CWORD\" -eq 1 ]; then\n        words=\"{}\"\n    else\n        case \"${{COMP_WORDS[1]}}\" in\n{cases}        *) words=\"{}\" ;;\n        esac\n    fi\n    COMPREPLY=($(compgen -W \"$words\" -- \"$cur\"))\n}}\ncomplete -o default -F _haetty haetty\n",
                top_words().join(" "),
                gui_words().join(" ")
            )
        }
        "zsh" => {
            let mut cases = String::new();
            for s in &subs {
                cases.push_str(&format!("    {}) w=({}) ;;\n", s.name, sub_words(s).join(" ")));
            }
            format!(
                "#compdef haetty\n# haetty completion for zsh. Save it as _haetty in a folder of $fpath, or load it with\n\
                 #   eval \"$(haetty completions zsh)\"   (after compinit, e.g. in ~/.zshrc)\n\
                 _haetty() {{\n  local -a w\n  if (( CURRENT == 2 )); then\n    w=({})\n  else\n    case $words[2] in\n{cases}    *) w=({}) ;;\n    esac\n  fi\n  compadd -- $w\n  [[ $words[2] == (completions|status|events|screen|daemon|install-desktop|hook|statusline) ]] || _files\n}}\n\
                 if [[ $zsh_eval_context[-1] == loadautofunc ]]; then\n  _haetty \"$@\"\nelse\n  compdef _haetty haetty\nfi\n",
                top_words().join(" "),
                gui_words().join(" ")
            )
        }
        "fish" => {
            let mut t = String::from(
                "# haetty completion for fish. Save it as ~/.config/fish/completions/haetty.fish, or load it with\n#   haetty completions fish | source\n",
            );
            for s in &subs {
                t.push_str(&format!(
                    "complete -c haetty -n __fish_use_subcommand -f -a {} -d '{}'\n",
                    s.name,
                    s.about.replace(['`', '\''], "")
                ));
            }
            for w in gui_words() {
                if let Some(l) = w.strip_prefix("--") {
                    t.push_str(&format!("complete -c haetty -n __fish_use_subcommand -l {l}\n"));
                } else if let Some(sh) = w.strip_prefix('-') {
                    t.push_str(&format!("complete -c haetty -n __fish_use_subcommand -s {sh}\n"));
                }
            }
            for s in &subs {
                for w in sub_words(s) {
                    let n = format!("'__fish_seen_subcommand_from {}'", s.name);
                    if s.name == "completions" {
                        t.push_str(&format!("complete -c haetty -n {n} -f -a {w}\n"));
                    } else if let Some(l) = w.strip_prefix("--") {
                        t.push_str(&format!("complete -c haetty -n {n} -l {l}\n"));
                    } else if let Some(sh) = w.strip_prefix('-') {
                        t.push_str(&format!("complete -c haetty -n {n} -s {sh}\n"));
                    }
                }
            }
            t
        }
        "powershell" => {
            let q = |v: Vec<String>| v.iter().map(|w| format!("'{w}'")).collect::<Vec<_>>().join(", ");
            let mut cases = String::new();
            for s in &subs {
                cases.push_str(&format!("            '{}' {{ @({}) }}\n", s.name, q(sub_words(s))));
            }
            format!(
                "# haetty completion for PowerShell. Load it with\n#   haetty completions powershell | Out-String | Invoke-Expression\n# (e.g. in $PROFILE).\n\
                 Register-ArgumentCompleter -Native -CommandName haetty, haetty.exe -ScriptBlock {{\n    param($wordToComplete, $commandAst, $cursorPosition)\n    $w = @($commandAst.CommandElements | ForEach-Object {{ $_.ToString() }})\n    $n = $w.Count\n    if ($wordToComplete -eq '') {{ $n++ }}\n    $c = if ($n -le 2) {{ @({}) }} else {{\n        switch ($w[1]) {{\n{cases}            default {{ @({}) }}\n        }}\n    }}\n    $c | Where-Object {{ $_ -like \"$wordToComplete*\" }} | ForEach-Object {{\n        [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)\n    }}\n}}\n",
                q(top_words()),
                q(gui_words())
            )
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_names_every_subcommand_and_setting() {
        let h = main_help();
        assert!(h.starts_with("usage: haetty"));
        for s in SUBS {
            assert!(h.contains(&format!("  {} ", s.name)), "{} missing from --help", s.name);
            let sh = sub_help(s.name).unwrap();
            assert!(sh.starts_with("usage: ") && sh.contains("--help"), "{sh}");
            for (o, _) in s.options {
                assert!(sh.contains(o), "{o} missing from {} --help", s.name);
            }
        }
        for r in SETTINGS {
            for k in r.keys {
                assert!(h.contains(k), "setting {k} missing from --help");
            }
        }
        assert!(sub_help("nope").is_none());
    }

    #[test]
    fn completions_know_every_subcommand_option_and_setting() {
        for sh in SHELLS {
            let c = completions(sh).unwrap();
            for s in SUBS.iter().filter(|s| s.name != "gui") {
                assert!(c.contains(s.name), "{sh}: {}", s.name);
                for w in words(s.options) {
                    let w = w.trim_start_matches('-');
                    assert!(c.contains(w), "{sh}: {} {w}", s.name);
                }
            }
            for k in ["font-size", "worker-command", "observe-new-tabs", "minimum-contrast"] {
                assert!(c.contains(k), "{sh}: --{k}");
            }
        }
        assert!(completions("tcsh").is_none());
    }
}
