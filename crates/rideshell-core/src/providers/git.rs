//! Git identity adapter.
//!
//! Uses only Git's documented environment variables (see `git help
//! git-commit-tree` / `git help git`):
//! * `GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`, `GIT_COMMITTER_NAME`,
//!   `GIT_COMMITTER_EMAIL` take precedence over `user.name` / `user.email`.
//! * `GIT_SSH_COMMAND` selects the SSH command (and so the key) Git uses for
//!   SSH remotes; it takes precedence over `core.sshCommand`.
//!
//! Isolation is per process: nothing in `~/.gitconfig` is modified.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;

use super::{absolute_path, Diagnostic, Provider, ProviderContext, SettingSpec};
use crate::store::Settings;

pub struct Git;

const SETTINGS: &[SettingSpec] = &[
    SettingSpec { key: "name", description: "Author and committer name" },
    SettingSpec { key: "email", description: "Author and committer e-mail" },
    SettingSpec {
        key: "ssh_key",
        description: "Path to the SSH private key Git uses for SSH remotes (the key file is never read)",
    },
];

impl Provider for Git {
    fn id(&self) -> &'static str {
        "git"
    }
    fn display_name(&self) -> &'static str {
        "Git"
    }
    fn description(&self) -> &'static str {
        "Git author/committer identity and SSH key selection"
    }
    fn category(&self) -> &'static str {
        "Code hosting"
    }
    fn isolation(&self) -> &'static str {
        "per-process environment (GIT_AUTHOR_*, GIT_COMMITTER_*, GIT_SSH_COMMAND); git config files are not modified"
    }
    fn tool(&self) -> &'static str {
        "git"
    }
    /// Route `git` through RideShell once an identity is connected, so the
    /// account guard can check `git push` (outside RideShell it runs as is).
    fn auto_shim(&self) -> bool {
        true
    }
    fn settings(&self) -> &'static [SettingSpec] {
        SETTINGS
    }

    fn normalize(&self, mut settings: Settings) -> Settings {
        if let Some(key) = settings.get_mut("ssh_key") {
            *key = absolute_path(key);
        }
        settings
    }

    fn validate(&self, settings: &Settings) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        if let Some(name) = settings.get("name") {
            if name.trim().is_empty() {
                out.push(Diagnostic::error("git name must not be empty"));
            }
        }
        if let Some(email) = settings.get("email") {
            if !email.contains('@') || email.chars().any(char::is_whitespace) {
                out.push(Diagnostic::error(format!("'{email}' does not look like an e-mail address")));
            }
        }
        if let Some(key) = settings.get("ssh_key") {
            if key.contains('"') {
                out.push(Diagnostic::error("ssh_key path must not contain '\"'"));
            } else if !Path::new(key).is_file() {
                out.push(Diagnostic::warning(format!("ssh key file {key} does not exist on this machine")));
            }
        }
        if settings.is_empty() {
            out.push(Diagnostic::warning("no git settings configured"));
        }
        out
    }

    fn environment(&self, settings: &Settings, _ctx: &ProviderContext) -> Result<BTreeMap<String, String>> {
        let mut env = BTreeMap::new();
        if let Some(name) = settings.get("name") {
            env.insert("GIT_AUTHOR_NAME".into(), name.clone());
            env.insert("GIT_COMMITTER_NAME".into(), name.clone());
        }
        if let Some(email) = settings.get("email") {
            env.insert("GIT_AUTHOR_EMAIL".into(), email.clone());
            env.insert("GIT_COMMITTER_EMAIL".into(), email.clone());
        }
        if let Some(key) = settings.get("ssh_key") {
            env.insert("GIT_SSH_COMMAND".into(), ssh_command(key));
        }
        Ok(env)
    }

    fn summary(&self, settings: &Settings, _ctx: &ProviderContext) -> String {
        let mut s = match (settings.get("name"), settings.get("email")) {
            (Some(n), Some(e)) => format!("{n} <{e}>"),
            (Some(n), None) => n.clone(),
            (None, Some(e)) => format!("<{e}>"),
            (None, None) => String::new(),
        };
        if let Some(key) = settings.get("ssh_key") {
            if !s.is_empty() {
                s.push_str(", ");
            }
            s.push_str(&format!("ssh key {key}"));
        }
        s
    }
}

/// Git runs `GIT_SSH_COMMAND` through a POSIX shell (also on Windows, via the
/// bundled `sh`), so the path is double-quoted and uses forward slashes.
fn ssh_command(key: &str) -> String {
    let path = if cfg!(windows) { key.replace('\\', "/") } else { key.to_string() };
    let escaped = path.replace('\\', "\\\\").replace('$', "\\$").replace('`', "\\`");
    format!("ssh -i \"{escaped}\" -o IdentitiesOnly=yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ProviderContext<'static> {
        ProviderContext { profile: "p", accounts_dir: std::path::Path::new("/a") }
    }

    fn settings(pairs: &[(&str, &str)]) -> Settings {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn environment_sets_author_and_committer() {
        let env = Git.environment(&settings(&[("name", "Jane Doe"), ("email", "jane@corp.example")]), &ctx()).unwrap();
        assert_eq!(env["GIT_AUTHOR_NAME"], "Jane Doe");
        assert_eq!(env["GIT_COMMITTER_EMAIL"], "jane@corp.example");
        assert!(!env.contains_key("GIT_SSH_COMMAND"));
    }

    #[test]
    fn ssh_command_quotes_paths() {
        let cmd = ssh_command("/home/me/My Keys/id_$work");
        assert_eq!(cmd, "ssh -i \"/home/me/My Keys/id_\\$work\" -o IdentitiesOnly=yes");
    }

    #[test]
    fn ssh_key_paths_become_absolute() {
        let s = Git.normalize(settings(&[("ssh_key", "~/.ssh/id_work")]));
        let key = std::path::PathBuf::from(&s["ssh_key"]);
        assert!(key.is_absolute(), "{key:?}");
        assert!(key.ends_with(".ssh/id_work"), "{key:?}");
    }

    #[test]
    fn validation() {
        let d = Git.validate(&settings(&[("email", "not-an-email")]));
        assert!(d.iter().any(|d| d.severity == super::super::Severity::Error));
        let d = Git.validate(&settings(&[("name", "A"), ("email", "a@b.c")]));
        assert!(d.is_empty());
    }
}
