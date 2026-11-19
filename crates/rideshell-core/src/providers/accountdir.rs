//! Providers for tools that keep each account in its own config directory.
//!
//! These tools store their login (OAuth session, tokens), settings and
//! history under one directory whose location is selected by an environment
//! variable. RideShell gives every account its own directory, so each
//! account signs in once through the tool's normal login flow and stays
//! signed in, and processes with different accounts run side by side.
//!
//! | Tool        | Variable            | Verified (2026-11, Windows 11)                         |
//! |-------------|---------------------|--------------------------------------------------------|
//! | Claude Code | `CLAUDE_CONFIG_DIR` | `claude auth status` reports the directory and a separate login state |
//! | Codex CLI   | `CODEX_HOME`        | `codex login status` reports a separate login state     |
//! | GitHub CLI  | `GH_CONFIG_DIR`     | documented in `gh help environment`                     |
//!
//! Caveat (documented to users): on macOS, Claude Code keeps OAuth
//! credentials in the Keychain; whether entries are separated per config
//! directory depends on the Claude Code version and is not yet verified.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;

use super::{absolute_path, Diagnostic, Provider, ProviderContext, SecretSpec, SettingSpec};
use crate::store::Settings;

pub struct AccountDirProvider {
    pub id: &'static str,
    pub display_name: &'static str,
    pub tool: &'static str,
    /// Environment variables pointing into the account's directory:
    /// `(VAR, "")` selects the directory itself, `(VAR, "file")` a file in it.
    pub vars: &'static [(&'static str, &'static str)],
    pub secrets: &'static [SecretSpec],
    pub login: Option<&'static [&'static str]>,
    pub logout: Option<&'static [&'static str]>,
    pub status: Option<&'static [&'static str]>,
    pub isolation: &'static str,
    pub category: &'static str,
    /// Every new profile gets its own account automatically.
    pub auto_attach: bool,
}

const SETTINGS: &[SettingSpec] = &[
    SettingSpec {
        key: "account",
        description: "Account name; each name gets its own login (default: the profile name)",
    },
    SettingSpec {
        key: "config_dir",
        description: "Use an existing config directory instead (e.g. ~/.claude to keep your current login)",
    },
];

pub const CLAUDE: AccountDirProvider = AccountDirProvider {
    id: "claude",
    display_name: "Claude Code",
    tool: "claude",
    vars: &[("CLAUDE_CONFIG_DIR", "")],
    secrets: &[SecretSpec {
        key: "api_key",
        env: "ANTHROPIC_API_KEY",
        description: "Anthropic API key (instead of signing in with a Claude account)",
    }],
    login: Some(&["auth", "login"]),
    logout: Some(&["auth", "logout"]),
    status: Some(&["auth", "status"]),
    isolation: "separate config directory per account (CLAUDE_CONFIG_DIR): login, settings, history and MCP config are per account",
    category: "AI coding agents",
    auto_attach: true,
};

pub const CODEX: AccountDirProvider = AccountDirProvider {
    id: "codex",
    display_name: "Codex CLI",
    tool: "codex",
    vars: &[("CODEX_HOME", "")],
    secrets: &[SecretSpec { key: "api_key", env: "OPENAI_API_KEY", description: "OpenAI API key" }],
    login: Some(&["login"]),
    logout: Some(&["logout"]),
    status: Some(&["login", "status"]),
    isolation: "separate CODEX_HOME per account: auth.json, config and history are per account",
    category: "AI coding agents",
    auto_attach: true,
};

pub const GH: AccountDirProvider = AccountDirProvider {
    id: "gh",
    display_name: "GitHub CLI",
    tool: "gh",
    vars: &[("GH_CONFIG_DIR", "")],
    secrets: &[SecretSpec { key: "token", env: "GH_TOKEN", description: "GitHub token (instead of gh auth login)" }],
    login: Some(&["auth", "login"]),
    logout: Some(&["auth", "logout"]),
    status: Some(&["auth", "status"]),
    isolation: "separate GH_CONFIG_DIR per account (gh may keep the token itself in the OS credential store, keyed by host and user)",
    category: "Code hosting",
    auto_attach: true,
};

impl AccountDirProvider {
    fn account<'a>(&self, settings: &'a Settings, ctx: &'a ProviderContext) -> &'a str {
        settings.get("account").map(String::as_str).unwrap_or(ctx.profile)
    }

    pub fn config_dir(&self, settings: &Settings, ctx: &ProviderContext) -> PathBuf {
        match settings.get("config_dir") {
            Some(dir) => PathBuf::from(dir),
            None => account_dir(ctx.accounts_dir, self.id, self.account(settings, ctx)),
        }
    }
}

/// `<accounts>/<provider>/<account>`
pub fn account_dir(accounts_dir: &Path, provider: &str, account: &str) -> PathBuf {
    accounts_dir.join(provider).join(account)
}

fn valid_account_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !name.starts_with('.')
}

impl Provider for AccountDirProvider {
    fn id(&self) -> &'static str {
        self.id
    }
    fn display_name(&self) -> &'static str {
        self.display_name
    }
    fn description(&self) -> &'static str {
        "multiple accounts, each with its own login"
    }
    fn category(&self) -> &'static str {
        self.category
    }
    fn auto_attach(&self) -> bool {
        self.auto_attach
    }
    fn tool(&self) -> &'static str {
        self.tool
    }
    fn auto_shim(&self) -> bool {
        true
    }
    fn isolation(&self) -> &'static str {
        self.isolation
    }
    fn settings(&self) -> &'static [SettingSpec] {
        SETTINGS
    }
    fn secrets(&self) -> &'static [SecretSpec] {
        self.secrets
    }

    fn normalize(&self, mut settings: Settings) -> Settings {
        if let Some(dir) = settings.get_mut("config_dir") {
            *dir = absolute_path(dir);
        }
        settings
    }

    fn validate(&self, settings: &Settings) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        if let Some(a) = settings.get("account") {
            if !valid_account_name(a) {
                out.push(Diagnostic::error(format!(
                    "account name '{a}' must use letters, digits, '-', '_' or '.' (max 64)"
                )));
            }
        }
        if settings.contains_key("account") && settings.contains_key("config_dir") {
            out.push(Diagnostic::error("set either 'account' or 'config_dir', not both"));
        }
        if let Some(dir) = settings.get("config_dir") {
            if Path::new(dir).exists() && !Path::new(dir).is_dir() {
                out.push(Diagnostic::error(format!("{dir} is not a directory")));
            }
        }
        out
    }

    fn environment(&self, settings: &Settings, ctx: &ProviderContext) -> Result<BTreeMap<String, String>> {
        let dir = self.config_dir(settings, ctx);
        let mut env = BTreeMap::new();
        for (var, file) in self.vars {
            let path = if file.is_empty() { dir.clone() } else { dir.join(file) };
            env.insert(var.to_string(), path.to_string_lossy().into_owned());
        }
        Ok(env)
    }

    fn required_dirs(&self, settings: &Settings, ctx: &ProviderContext) -> Vec<PathBuf> {
        vec![self.config_dir(settings, ctx)]
    }

    fn summary(&self, settings: &Settings, ctx: &ProviderContext) -> String {
        match settings.get("config_dir") {
            Some(dir) => format!("config {dir}"),
            None => format!("account {}", self.account(settings, ctx)),
        }
    }

    fn login_args(&self) -> Option<&'static [&'static str]> {
        self.login
    }
    fn logout_args(&self) -> Option<&'static [&'static str]> {
        self.logout
    }
    fn status_args(&self) -> Option<&'static [&'static str]> {
        self.status
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(profile: &str) -> ProviderContext<'_> {
        ProviderContext { profile, accounts_dir: Path::new("/data/accounts") }
    }

    #[test]
    fn default_account_is_profile_name() {
        let env = CLAUDE.environment(&Settings::new(), &ctx("work")).unwrap();
        assert_eq!(PathBuf::from(&env["CLAUDE_CONFIG_DIR"]), Path::new("/data/accounts/claude/work"));
    }

    #[test]
    fn two_profiles_never_share_a_directory() {
        let a = CLAUDE.environment(&Settings::new(), &ctx("work")).unwrap();
        let b = CLAUDE.environment(&Settings::new(), &ctx("personal")).unwrap();
        assert_ne!(a["CLAUDE_CONFIG_DIR"], b["CLAUDE_CONFIG_DIR"]);
    }

    #[test]
    fn explicit_account_and_config_dir() {
        let s: Settings = [("account".to_string(), "team".to_string())].into();
        let env = CODEX.environment(&s, &ctx("work")).unwrap();
        assert_eq!(PathBuf::from(&env["CODEX_HOME"]), Path::new("/data/accounts/codex/team"));
        let s: Settings = [("config_dir".to_string(), "/home/me/.config/gh".to_string())].into();
        assert_eq!(GH.environment(&s, &ctx("work")).unwrap()["GH_CONFIG_DIR"], "/home/me/.config/gh");
    }

    #[test]
    fn account_names_are_path_safe() {
        for bad in ["../x", "a/b", ".hidden", "", "a b"] {
            let s: Settings = [("account".to_string(), bad.to_string())].into();
            assert!(!CLAUDE.validate(&s).is_empty(), "{bad}");
        }
    }
}
