//! Learn what a command-line tool offers, from the tool itself.
//!
//! RideShell keeps no list of tools. To connect any tool to an account it runs
//! the tool's own help (`--help`, then `auth --help` / `help environment` when
//! the help mentions them) and reads:
//!
//! * sign-in commands: subcommands such as `login`, `auth login`, `auth:login`;
//! * environment variables the help mentions, sorted into likely API-key
//!   variables (`..._TOKEN`, `..._API_KEY`, `..._SECRET`) and likely
//!   config-location variables (`..._CONFIG_DIR`, `..._HOME`, `..._CONFIG`).
//!
//! These are suggestions for the user to confirm, not claims about the tool.
//! The help commands run with no input, no window and a short time limit.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
pub struct Discovery {
    /// Sign-in argument lists, e.g. `["login"]`, `["auth", "login"]`.
    #[serde(rename = "signIn")]
    pub sign_in: Vec<String>,
    /// Variables that look like API keys or tokens.
    #[serde(rename = "keyVars")]
    pub key_vars: Vec<String>,
    /// Variables that look like a config folder or file location.
    #[serde(rename = "configVars")]
    pub config_vars: Vec<String>,
    /// The tool printed some help at all.
    #[serde(rename = "helpFound")]
    pub help_found: bool,
}

const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_OUTPUT: usize = 256 * 1024;

/// Run `exe args...` and return what it printed (stdout + stderr), bounded in time and size.
fn capture(exe: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env(crate::shims::BYPASS_ENV, "1")
        .env("NO_COLOR", "1")
        .env("CI", "1"); // many tools skip interactive prompts and update checks in CI
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().ok()?;
    let mut out = child.stdout.take()?;
    let mut err = child.stderr.take()?;
    let t1 = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = (&mut out).take(MAX_OUTPUT as u64).read_to_end(&mut b);
        b
    });
    let t2 = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = (&mut err).take(MAX_OUTPUT as u64).read_to_end(&mut b);
        b
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < TIMEOUT => std::thread::sleep(Duration::from_millis(40)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    let mut text = String::from_utf8_lossy(&t1.join().unwrap_or_default()).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&t2.join().unwrap_or_default()));
    Some(text)
}

/// Inspect the tool at `exe`.
pub fn inspect(exe: &Path) -> Discovery {
    let mut help = capture(exe, &["--help"]).unwrap_or_default();
    if help.trim().len() < 40 {
        help.push_str(&capture(exe, &["help"]).unwrap_or_default());
    }
    let mut extra = String::new();
    let top = subcommands(&help);
    if top.iter().any(|c| c == "auth") {
        extra.push_str(&capture(exe, &["auth", "--help"]).unwrap_or_default());
    }
    if help.to_ascii_lowercase().contains("help environment") || top.iter().any(|c| c == "environment") {
        extra.push_str(&capture(exe, &["help", "environment"]).unwrap_or_default());
    }
    analyse(&help, &extra)
}

/// Pure analysis of help texts (`top` = main help, `more` = follow-up help).
pub fn analyse(top: &str, more: &str) -> Discovery {
    let mut d = Discovery { help_found: top.trim().len() >= 20, ..Default::default() };
    let cmds = subcommands(top);
    const SIGNIN: &[&str] = &["login", "signin", "sign-in", "authenticate", "logon"];
    for c in &cmds {
        if SIGNIN.contains(&c.as_str()) || c.ends_with(":login") {
            push_unique(&mut d.sign_in, c.clone());
        }
    }
    if cmds.iter().any(|c| c == "auth") {
        for c in subcommands(more) {
            if SIGNIN.contains(&c.as_str()) {
                push_unique(&mut d.sign_in, format!("auth {c}"));
            }
        }
    }
    for var in env_vars(&format!("{top}\n{more}")) {
        let v = var.as_str();
        if ["TOKEN", "API_KEY", "APIKEY", "SECRET", "ACCESS_KEY", "AUTH_KEY", "PASSWORD"].iter().any(|k| v.contains(k))
        {
            push_unique(&mut d.key_vars, var);
        } else if v.ends_with("_CONFIG_DIR")
            || v.ends_with("_CONFIG_HOME")
            || v.ends_with("_HOME")
            || v.ends_with("_CONFIG")
            || v.ends_with("_CONFIG_FILE")
            || v.ends_with("_CONFIG_PATH")
            || v == "XDG_CONFIG_HOME"
        {
            push_unique(&mut d.config_vars, var);
        }
    }
    d
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !v.iter().any(|x| x.eq_ignore_ascii_case(&s)) {
        v.push(s);
    }
}

/// Words that start an indented help line, e.g. `  login     Log in to ...`.
fn subcommands(help: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in help.lines() {
        if !line.starts_with([' ', '\t']) {
            continue;
        }
        let Some(word) = line.split_whitespace().next() else { continue };
        let word = word.trim_end_matches([',', ':']).trim_start_matches('*');
        let ok = !word.is_empty()
            && !word.starts_with('-')
            && word.len() <= 32
            && word.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | ':' | '_'));
        if ok {
            push_unique(&mut out, word.to_string());
        }
    }
    out
}

/// UPPER_CASE words with an underscore, e.g. `GH_TOKEN`, `CLOUDFLARE_API_TOKEN`.
fn env_vars(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')) {
        let w = raw.trim_matches('_');
        let ok = w.len() >= 4
            && w.len() <= 60
            && w.contains('_')
            && w.starts_with(|c: char| c.is_ascii_uppercase())
            && w.chars().any(|c| c.is_ascii_uppercase());
        if ok {
            push_unique(&mut out, w.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_sign_in_and_variables_in_help() {
        let top = "Usage: tool <command>\n\nCommands:\n  deploy     Deploy\n  login      Sign in to your account\n  logout     Sign out\n  auth       Manage auth\n\nEnvironment:\n  TOOL_API_TOKEN     token to use instead of logging in\n  TOOL_CONFIG_DIR    where config is stored\n  LOG_LEVEL          verbosity\n";
        let more = "Commands:\n  login   Log in\n  status  Show status\n";
        let d = analyse(top, more);
        assert_eq!(d.sign_in, vec!["login", "auth login"]);
        assert_eq!(d.key_vars, vec!["TOOL_API_TOKEN"]);
        assert_eq!(d.config_vars, vec!["TOOL_CONFIG_DIR"]);
        assert!(d.help_found);
    }

    #[test]
    fn colon_style_commands_and_nothing_found() {
        let d = analyse("COMMANDS\n  auth:login  log in\n  apps:list   list apps\nHEROKU_API_KEY works too\n", "");
        assert_eq!(d.sign_in, vec!["auth:login"]);
        assert_eq!(d.key_vars, vec!["HEROKU_API_KEY"]);
        let none = analyse("just a sentence with no commands at all", "");
        assert!(none.sign_in.is_empty() && none.key_vars.is_empty());
    }

    #[test]
    fn real_tool_help_is_read_safely() {
        // `cmd /?` on Windows, `ls --help` elsewhere: always present, prints help, exits.
        let exe = if cfg!(windows) { which::which("cmd").unwrap() } else { which::which("ls").unwrap() };
        let text = if cfg!(windows) { capture(&exe, &["/?"]) } else { capture(&exe, &["--help"]) };
        assert!(text.unwrap_or_default().trim().len() > 20);
    }
}
