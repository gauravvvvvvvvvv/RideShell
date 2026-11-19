//! Shell adapters.
//!
//! RideShell never parses or replaces shell syntax. Each adapter only knows
//! how to (a) quote values, (b) emit `set`/`unset` statements for an
//! [`EnvPlan`], (c) generate a small integration script that calls the
//! binary from the prompt hook, and (d) locate the startup file.

pub mod install;

use std::path::{Path, PathBuf};

use crate::brand;
use crate::config::Config;
use crate::envstate::EnvPlan;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shell {
    /// Windows PowerShell 5.1 (`powershell.exe`).
    PowerShell,
    /// PowerShell 7+ (`pwsh`).
    Pwsh,
    Bash,
    Zsh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syntax {
    PowerShell,
    Posix,
}

impl Shell {
    pub const ALL: [Shell; 4] = [Shell::PowerShell, Shell::Pwsh, Shell::Bash, Shell::Zsh];

    pub fn id(self) -> &'static str {
        match self {
            Shell::PowerShell => "powershell",
            Shell::Pwsh => "pwsh",
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
        }
    }

    pub fn parse(s: &str) -> Option<Shell> {
        Self::ALL.into_iter().find(|sh| sh.id().eq_ignore_ascii_case(s))
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Shell::PowerShell => "Windows PowerShell 5.1",
            Shell::Pwsh => "PowerShell 7+",
            Shell::Bash => "Bash",
            Shell::Zsh => "Zsh",
        }
    }

    pub fn executable(self) -> &'static str {
        self.id()
    }

    pub fn syntax(self) -> Syntax {
        match self {
            Shell::PowerShell | Shell::Pwsh => Syntax::PowerShell,
            Shell::Bash | Shell::Zsh => Syntax::Posix,
        }
    }
}

/// Where a shell is installed, if it is. On Windows, `bash.exe` in System32
/// is the WSL launcher, not a native bash, and is ignored.
pub fn detect(shell: Shell) -> Option<PathBuf> {
    let found = which::which(shell.executable()).ok();
    if cfg!(windows) && shell == Shell::Bash {
        if let Some(p) = &found {
            if !p.to_string_lossy().to_ascii_lowercase().contains("system32") {
                return found;
            }
        }
        for candidate in [r"C:\Program Files\Git\bin\bash.exe", r"C:\Program Files (x86)\Git\bin\bash.exe"] {
            let p = PathBuf::from(candidate);
            if p.is_file() {
                return Some(p);
            }
        }
        return None;
    }
    found
}

// ----- quoting -------------------------------------------------------------

/// POSIX single-quoting: safe for any byte sequence except NUL.
pub fn posix_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// A PowerShell expression evaluating to `s`.
///
/// Windows PowerShell decodes native-command output with the console code
/// page, so non-ASCII characters would be corrupted in transit. The emitted
/// expression is therefore pure ASCII: non-ASCII UTF-16 code units are
/// written as `[char]0xNNNN`. PowerShell also treats typographic quotes
/// (U+2018..U+201B) as single quotes; those are non-ASCII and escaped too.
pub fn ps_literal(s: &str) -> String {
    if s.is_empty() {
        return "''".into();
    }
    let mut parts: Vec<String> = Vec::new();
    let mut run = String::new();
    for ch in s.chars() {
        if ch.is_ascii() && (ch == '\t' || ch == '\n' || ch == '\r' || !ch.is_ascii_control()) {
            if ch == '\'' {
                run.push_str("''");
            } else {
                run.push(ch);
            }
        } else {
            if !run.is_empty() {
                parts.push(format!("'{run}'"));
                run.clear();
            }
            let mut buf = [0u16; 2];
            for unit in ch.encode_utf16(&mut buf) {
                parts.push(format!("[char]0x{unit:04X}"));
            }
        }
    }
    if !run.is_empty() {
        parts.push(format!("'{run}'"));
    }
    if parts.len() == 1 && parts[0].starts_with('\'') {
        parts.pop().unwrap()
    } else {
        format!("({})", parts.join(" + "))
    }
}

/// Shell code applying `plan`. Starts with [`brand::emit_marker`].
pub fn emit(syntax: Syntax, plan: &EnvPlan) -> String {
    let mut out = brand::emit_marker();
    out.push('\n');
    for key in &plan.unset {
        match syntax {
            Syntax::PowerShell => out.push_str(&format!("$env:{key} = $null\n")),
            Syntax::Posix => out.push_str(&format!("unset {key}\n")),
        }
    }
    for (key, value) in &plan.set {
        match syntax {
            Syntax::PowerShell => out.push_str(&format!("$env:{key} = {}\n", ps_literal(value))),
            Syntax::Posix => out.push_str(&format!("export {key}={}\n", posix_quote(value))),
        }
    }
    out
}

// ----- integration scripts --------------------------------------------------

const PS_TEMPLATE: &str = include_str!("scripts/rideshell.ps1");
const BASH_TEMPLATE: &str = include_str!("scripts/rideshell.bash");
const ZSH_TEMPLATE: &str = include_str!("scripts/rideshell.zsh");
const SESSION_TEMPLATE: &str = include_str!("scripts/rideshell-session.ps1");

/// The binary path as a POSIX shell sees it. Git Bash / MSYS accept
/// `C:/...`, which avoids backslash handling entirely.
fn posix_bin_path(bin: &Path) -> String {
    let s = bin.to_string_lossy();
    if cfg!(windows) {
        s.replace('\\', "/")
    } else {
        s.into_owned()
    }
}

/// Integration script printed by `rideshell hook <shell>`.
pub fn hook_script(shell: Shell, bin: &Path, shims: &Path, config: &Config) -> String {
    let shims_literal = match shell.syntax() {
        Syntax::PowerShell => ps_literal(&shims.to_string_lossy()),
        Syntax::Posix => posix_quote(&posix_bin_path(shims)),
    };
    let (template, bin_literal, indicator) = match shell.syntax() {
        Syntax::PowerShell => {
            (PS_TEMPLATE, ps_literal(&bin.to_string_lossy()), if config.prompt.indicator { "$true" } else { "$false" })
        }
        Syntax::Posix => (
            if shell == Shell::Zsh { ZSH_TEMPLATE } else { BASH_TEMPLATE },
            posix_quote(&posix_bin_path(bin)),
            if config.prompt.indicator { "1" } else { "0" },
        ),
    };
    fill(template, shell, indicator, &shims_literal, &bin_literal)
}

/// Extra PowerShell code for a RideShell window (`rideshell open`): routes
/// cmd- and bash-style lines to those shells. Appended to [`hook_script`].
pub fn session_script(shell: Shell, config: &Config, bash_available: bool) -> String {
    let b = |v: bool| if v { "$true" } else { "$false" };
    let s = SESSION_TEMPLATE
        .replace("@AUTO_SWITCH@", b(config.session.auto_switch))
        .replace("@BASH_AVAILABLE@", b(bash_available))
        .replace("@APP@", brand::APP_NAME)
        .replace("@ENV_LINE@", brand::env::LINE)
        .replace("@ENV_PS_ERRORS@", brand::env::PS_ERRORS)
        .replace("@ENV_PS_COMMANDS@", brand::env::PS_COMMANDS)
        .replace("@ENV_ACTIVE@", brand::env::ACTIVE_PROFILE);
    fill(&s, shell, "", "", "")
}

fn fill(template: &str, shell: Shell, indicator: &str, shims_literal: &str, bin_literal: &str) -> String {
    // `@BIN@` is substituted last so a path containing `@` cannot form
    // another placeholder.
    template
        .replace("@SHELL@", shell.id())
        .replace("@SHELL_NAME@", shell.display_name())
        .replace("@INDICATOR@", indicator)
        .replace("@CMD@", brand::BIN_NAME)
        .replace("@MARKER@", &brand::emit_marker())
        .replace("@ENV_SHELL@", brand::env::SHELL)
        .replace("@ENV_ACTIVE@", brand::env::ACTIVE_PROFILE)
        .replace("@ENV_PROMPT@", brand::env::PROMPT)
        .replace("@FN@", &format!("__{}", brand::BIN_NAME))
        .replace("@SHIMS@", shims_literal)
        .replace("@BIN@", bin_literal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_quoting() {
        assert_eq!(posix_quote("a b"), "'a b'");
        assert_eq!(posix_quote("it's"), r"'it'\''s'");
        assert_eq!(posix_quote(""), "''");
    }

    #[test]
    fn powershell_literals_are_ascii() {
        assert_eq!(ps_literal("plain"), "'plain'");
        assert_eq!(ps_literal("it's"), "'it''s'");
        assert_eq!(ps_literal("$env:X `n"), "'$env:X `n'");
        assert_eq!(ps_literal("Jürgen"), "('J' + [char]0x00FC + 'rgen')");
        assert_eq!(ps_literal("\u{2019}"), "([char]0x2019)");
        assert_eq!(ps_literal("😀"), "([char]0xD83D + [char]0xDE00)");
        assert!(ps_literal("ünï €").is_ascii());
    }

    #[test]
    fn emit_marks_and_orders() {
        let plan = EnvPlan { set: vec![("A".into(), "x y".into())], unset: vec!["B".into()] };
        let posix = emit(Syntax::Posix, &plan);
        assert_eq!(posix, format!("{}\nunset B\nexport A='x y'\n", brand::emit_marker()));
        let ps = emit(Syntax::PowerShell, &plan);
        assert!(ps.contains("$env:B = $null") && ps.contains("$env:A = 'x y'"));
    }

    #[test]
    fn hook_scripts_have_no_unreplaced_placeholders() {
        for shell in Shell::ALL {
            let s = hook_script(
                shell,
                Path::new("/opt/ride shell/bin/rideshell"),
                Path::new("/data/shims"),
                &Config::default(),
            );
            for placeholder in ["@SHIMS@", "@BIN@", "@SHELL@", "@INDICATOR@", "@CMD@", "@MARKER@", "@FN@", "@ENV_"] {
                assert!(!s.contains(placeholder), "{shell:?} still contains {placeholder}");
            }
            if shell.syntax() == Syntax::PowerShell {
                assert!(s.is_ascii(), "PowerShell scripts must be ASCII");
                let session = session_script(shell, &Config::default(), true);
                assert!(session.is_ascii());
                for placeholder in ["@AUTO_SWITCH@", "@BASH_AVAILABLE@", "@APP@", "@CMD@", "@FN@", "@MARKER@", "@ENV_"]
                {
                    assert!(!session.contains(placeholder), "{shell:?} session still contains {placeholder}");
                }
            }
        }
    }
}
