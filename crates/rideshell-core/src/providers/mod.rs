//! Identity adapters ("providers").
//!
//! A provider makes one developer tool use a particular account *through a
//! mechanism the tool itself supports*: an environment variable, a
//! config-directory selector or a named profile. Providers never read, copy
//! or extract the tool's credentials; each account simply gets its own,
//! separate place where the tool keeps its own login.
//!
//! Adding a provider: implement [`Provider`] in a new module and register it
//! in [`registry`]. Resolution, shells and shims need no changes.

mod accountdir;
pub mod catalog;
mod git;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;

pub use accountdir::{AccountDirProvider, CLAUDE, CODEX, GH};
pub use git::Git;

use crate::store::Settings;

#[derive(Debug, Clone, Copy)]
pub struct SettingSpec {
    pub key: &'static str,
    pub description: &'static str,
}

/// A secret a provider can take (e.g. an API key). It is stored in the
/// vault as a profile secret variable named `env`.
#[derive(Debug, Clone, Copy)]
pub struct SecretSpec {
    pub key: &'static str,
    pub env: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
}

impl Diagnostic {
    pub fn warning(message: impl Into<String>) -> Self {
        Self { severity: Severity::Warning, message: message.into() }
    }
    pub fn error(message: impl Into<String>) -> Self {
        Self { severity: Severity::Error, message: message.into() }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Detection {
    pub path: Option<PathBuf>,
    pub version: Option<String>,
}

impl Detection {
    pub fn installed(&self) -> bool {
        self.path.is_some()
    }
}

/// Context for computing a provider's environment.
#[derive(Debug, Clone)]
pub struct ProviderContext<'a> {
    pub profile: &'a str,
    /// Root of RideShell-managed per-account directories.
    pub accounts_dir: &'a Path,
}

pub trait Provider: Send + Sync {
    /// Stable identifier used in the database and on the command line.
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    /// Executable this provider configures (used for tool shims).
    fn tool(&self) -> &'static str;
    /// Whether a shim for [`tool`](Self::tool) is created automatically
    /// when the identity is configured.
    fn auto_shim(&self) -> bool {
        false
    }
    /// How isolation is achieved, shown to users so guarantees are explicit.
    fn isolation(&self) -> &'static str;
    /// Grouping for lists ("AI coding agents", "Cloud", "Deploy"...).
    fn category(&self) -> &'static str {
        "Other"
    }
    /// New profiles get their own account for this tool automatically.
    fn auto_attach(&self) -> bool {
        false
    }
    /// The variables only reach the tool itself (launched through its shim,
    /// `run` or `auth`), never other programs: e.g. `XDG_CONFIG_HOME`.
    fn tool_scoped(&self) -> bool {
        false
    }
    fn settings(&self) -> &'static [SettingSpec];
    fn secrets(&self) -> &'static [SecretSpec] {
        &[]
    }
    /// Locate the tool. May spawn a process; never call from prompt hooks.
    fn detect(&self) -> Detection {
        detect_binary(self.tool(), &["--version"])
    }
    /// Canonicalise user input before it is validated and stored.
    fn normalize(&self, settings: Settings) -> Settings {
        settings
    }
    /// Check settings. Errors block saving; warnings are shown.
    fn validate(&self, settings: &Settings) -> Vec<Diagnostic>;
    /// Environment variables that select this identity for a process.
    /// Must be fast and side-effect free (runs in the prompt hook).
    fn environment(&self, settings: &Settings, ctx: &ProviderContext) -> Result<BTreeMap<String, String>>;
    /// Directories that must exist before the tool runs (created on launch).
    fn required_dirs(&self, _settings: &Settings, _ctx: &ProviderContext) -> Vec<PathBuf> {
        Vec::new()
    }
    /// One-line human summary, e.g. `Jane Doe <jane@corp.example>`.
    fn summary(&self, settings: &Settings, ctx: &ProviderContext) -> String;
    /// Arguments that start the tool's own sign-in flow.
    fn login_args(&self) -> Option<&'static [&'static str]> {
        None
    }
    fn logout_args(&self) -> Option<&'static [&'static str]> {
        None
    }
    /// Arguments that print the tool's own authentication status.
    fn status_args(&self) -> Option<&'static [&'static str]> {
        None
    }
}

/// All built-in providers.
/// Where a tool keeps its login when RideShell does not redirect it
/// (`~/.claude`, `~/.codex`, GitHub CLI's config folder), if it exists.
pub fn default_account_dir(provider_id: &str) -> Option<std::path::PathBuf> {
    let home = dirs::home_dir()?;
    let dir = match provider_id {
        "claude" => home.join(".claude"),
        "codex" => home.join(".codex"),
        "gh" if cfg!(windows) => dirs::config_dir()?.join("GitHub CLI"),
        "gh" => home.join(".config").join("gh"),
        _ => return None,
    };
    dir.is_dir().then_some(dir)
}

/// Give a profile its own account for every tool that keeps logins in a
/// config directory (Claude Code, Codex, GitHub CLI), so a new profile never
/// silently uses another profile's login.
///
/// The *first* profile adopts each tool's existing default directory, so the
/// logins a user already has stay with that profile. Later profiles get new,
/// empty directories and sign in on their own. Existing identities are kept.
/// Returns the tools that were attached.
pub fn attach_default_accounts(store: &mut crate::store::Store, profile: &str) -> anyhow::Result<Vec<&'static str>> {
    let profiles = store.list_profiles()?;
    // `RIDESHELL_NO_ADOPT=1`: every profile starts with fresh accounts (also used by tests,
    // which must never point at the developer's real logins).
    let adopt = std::env::var_os("RIDESHELL_NO_ADOPT").map_or(true, |v| v.is_empty() || v == "0");
    let first = profiles.len() == 1 && adopt;
    let p = store.require_profile(profile)?;
    let existing = store.identities(p.id)?;
    let mut attached = Vec::new();
    for prov in registry() {
        if existing.contains_key(prov.id()) || !prov.auto_attach() {
            continue;
        }
        let mut settings = crate::store::Settings::new();
        if first {
            if let Some(dir) = default_account_dir(prov.id()) {
                settings.insert("config_dir".into(), dir.to_string_lossy().into_owned());
            }
        }
        store.set_identity(&p.name, prov.id(), &settings)?;
        attached.push(prov.tool());
    }
    Ok(attached)
}

pub fn registry() -> &'static [&'static dyn Provider] {
    // Built in: the accounts RideShell is about. Every other tool is connected
    // through `custom`, with what its own CLI offers (see `discover`).
    static REGISTRY: [&dyn Provider; 5] = [&CLAUDE, &CODEX, &GH, &Git, &catalog::CUSTOM];
    &REGISTRY
}

pub fn find(id: &str) -> Option<&'static dyn Provider> {
    if catalog::is_custom_id(id) {
        return Some(&catalog::CUSTOM);
    }
    registry().iter().copied().find(|p| p.id().eq_ignore_ascii_case(id))
}

/// Provider configuring `tool` (e.g. `claude` -> Claude Code), if any.
pub fn for_tool(tool: &str) -> Option<&'static dyn Provider> {
    registry().iter().copied().find(|p| p.tool().eq_ignore_ascii_case(tool))
}

/// Keys in `settings` not known to `provider`.
pub fn unknown_keys(provider: &dyn Provider, settings: &Settings) -> Vec<String> {
    settings.keys().filter(|k| !provider.settings().iter().any(|s| s.key == k.as_str())).cloned().collect()
}

/// Expand a leading `~` and make a path absolute (without requiring it to exist).
pub fn absolute_path(value: &str) -> String {
    let expanded = match value.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => match dirs::home_dir() {
            Some(home) => home.join(rest.trim_start_matches(['/', '\\'])),
            None => PathBuf::from(value),
        },
        _ => PathBuf::from(value),
    };
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir().map(|d| d.join(&expanded)).unwrap_or(expanded)
    };
    dunce::canonicalize(&absolute).unwrap_or(absolute).to_string_lossy().into_owned()
}

/// Locate an executable on PATH and ask it for its version. Tool shims
/// (RideShell's own launchers) are skipped so the real tool is reported.
pub fn detect_binary(name: &str, version_args: &[&str]) -> Detection {
    let Some(path) = crate::shims::find_real(name, None).ok() else { return Detection::default() };
    let mut cmd = Command::new(&path);
    cmd.args(version_args).env(crate::shims::BYPASS_ENV, "1");
    let version = crate::platform::no_window(&mut cmd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.lines().next().unwrap_or("").trim().to_string())
        .filter(|s| !s.is_empty());
    Detection { path: Some(path), version }
}
