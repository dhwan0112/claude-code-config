//! Shell integration (DESIGN.md §13.8): a zsh shell tab started by haetty reads haetty's startup file first. It gives
//! zsh back the person's own `ZDOTDIR`, reads their `.zshenv` and adds one prompt hook that keeps the `claude` proxy
//! folder first on `PATH`; zsh then reads the person's other startup files in its own order. A bash tab gets the same
//! hook as an exported function named in `PROMPT_COMMAND`, which bash runs before each prompt.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// The person's `ZDOTDIR` while haetty's folder stands in for it; unset when they had none.
pub const ZDOTDIR_ENV: &str = "HAETTY_ZSH_ZDOTDIR";

const ZSHENV: &str = include_str!("shell/zshenv.zsh");

/// The bash hook's name, and the function as bash imports it from the environment.
pub const BASH_HOOK: &str = "_haetty_path_first";
const BASH_FUNC: &str = "() {  local s=$?;
 case \":$PATH\" in
 \":$HAETTY_PROXY_DIR:\"*) ;;
 *) if [ -n \"${HAETTY_PROXY_DIR-}\" ]; then local p=\":$PATH:\"; p=${p//\":$HAETTY_PROXY_DIR:\"/:}; p=${p#:}; p=${p%:}; PATH=$HAETTY_PROXY_DIR${p:+:$p}; fi ;;
 esac;
 return $s
}";

/// A zsh by its file name (`zsh`, `/usr/bin/zsh`).
pub fn is_zsh(program: &str) -> bool {
    Path::new(program).file_name().is_some_and(|n| n == "zsh")
}

/// Writes `<run_dir>/zsh/.zshenv` and returns the folder.
pub fn write_zsh(run_dir: &Path) -> io::Result<PathBuf> {
    let dir = run_dir.join("zsh");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(".zshenv");
    if std::fs::read_to_string(&file).ok().as_deref() != Some(ZSHENV) {
        let tmp = dir.join(".zshenv.new");
        std::fs::write(&tmp, ZSHENV)?;
        std::fs::rename(&tmp, &file)?;
    }
    Ok(dir)
}

/// The variables a zsh tab gets: haetty's folder as `ZDOTDIR` and the person's own value, if any, beside it.
pub fn zsh_env(dir: &Path, own: Option<OsString>) -> Vec<(String, String)> {
    let mut env = vec![("ZDOTDIR".to_string(), dir.to_string_lossy().into_owned())];
    if let Some(v) = own {
        env.push((ZDOTDIR_ENV.to_string(), v.to_string_lossy().into_owned()));
    }
    env
}

/// A bash by its file name.
pub fn is_bash(program: &str) -> bool {
    Path::new(program).file_name().is_some_and(|n| n == "bash")
}

/// The variables a bash tab gets: the hook as an exported function, run first in `PROMPT_COMMAND` (before what the
/// environment had there already).
pub fn bash_env(own: Option<&str>) -> Vec<(String, String)> {
    let command = match own.filter(|c| !c.trim().is_empty()) {
        Some(c) if c.split([';', '\n', ' ']).any(|w| w == BASH_HOOK) => c.to_string(),
        Some(c) => format!("{BASH_HOOK}; {c}"),
        None => BASH_HOOK.to_string(),
    };
    vec![(format!("BASH_FUNC_{BASH_HOOK}%%"), BASH_FUNC.to_string()), ("PROMPT_COMMAND".to_string(), command)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zsh_is_known_by_its_file_name() {
        assert!(is_zsh("zsh") && is_zsh("/usr/bin/zsh") && is_zsh("/bin/zsh"));
        assert!(!is_zsh("bash") && !is_zsh("/usr/bin/zsh5") && !is_zsh("fish") && !is_zsh(""));
    }

    #[test]
    fn the_person_s_zdotdir_rides_beside_haetty_s() {
        let d = Path::new("/run/h/zsh");
        assert_eq!(zsh_env(d, None), vec![("ZDOTDIR".to_string(), "/run/h/zsh".to_string())]);
        let e = zsh_env(d, Some("/home/u/.config/zsh".into()));
        assert_eq!(e[1], (ZDOTDIR_ENV.to_string(), "/home/u/.config/zsh".to_string()));
        assert_eq!(zsh_env(d, Some("".into()))[1].1, "", "an empty ZDOTDIR stays empty, not unset");
    }

    #[test]
    fn bash_runs_the_hook_first_and_once() {
        let pc = |own: Option<&str>| bash_env(own)[1].1.clone();
        assert_eq!(pc(None), BASH_HOOK);
        assert_eq!(pc(Some(" ")), BASH_HOOK);
        assert_eq!(pc(Some("history -a")), format!("{BASH_HOOK}; history -a"));
        assert_eq!(pc(Some("_haetty_path_first; history -a")), "_haetty_path_first; history -a");
        assert!(is_bash("/bin/bash") && !is_bash("rbash") && !is_bash("bash5"));
        assert!(bash_env(None)[0].1.starts_with("() {"), "the form bash imports functions in");
    }
}
