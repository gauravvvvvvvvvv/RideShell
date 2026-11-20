//! User configuration (`config.toml`).
//!
//! Configuration holds *preferences* only. Profiles, bindings and identities
//! live in the database so concurrent terminals can update them safely.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths::Paths;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub prompt: PromptConfig,
    pub shell: ShellConfig,
    pub shims: ShimsConfig,
    pub session: SessionConfig,
    pub history: HistoryConfig,
    pub guard: GuardConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GuardConfig {
    /// Ask before a sensitive command when the terminal's account differs
    /// from the folder's bound account (see `crate::guard`).
    pub enabled: bool,
    /// Sensitive commands: a tool and words that must appear among its arguments.
    pub commands: Vec<String>,
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self { enabled: true, commands: crate::guard::DEFAULT_COMMANDS.iter().map(|s| s.to_string()).collect() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct HistoryConfig {
    /// New Claude Code / Codex accounts see the conversations of the tool's
    /// original folder (~/.claude, ~/.codex). Off: each account's
    /// conversations stay its own (e.g. work and personal never mix).
    pub share_new_accounts: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SessionConfig {
    /// PowerShell that hosts a RideShell window: "auto" (pwsh if installed,
    /// else Windows PowerShell), "pwsh" or "powershell".
    pub host: String,
    /// Run cmd- and bash-style lines in cmd / bash automatically.
    pub auto_switch: bool,
    /// Print a dim `cmd ›` / `bash ›` marker when a line is routed.
    pub show_dialect: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self { host: "auto".into(), auto_switch: true, show_dialect: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShimsConfig {
    /// Print which profile a shimmed tool runs as (only when stderr is a terminal).
    pub notify: bool,
}

impl Default for ShimsConfig {
    fn default() -> Self {
        Self { notify: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PromptConfig {
    /// Prefix the shell prompt with the active profile, e.g. `[work]`.
    pub indicator: bool,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self { indicator: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShellConfig {
    /// Apply directory bindings automatically when the working directory changes.
    pub auto_apply: bool,
    /// Print a one-line notice when the active profile changes.
    pub notify_on_switch: bool,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self { auto_apply: true, notify_on_switch: true }
    }
}

pub const DEFAULT_CONFIG_TOML: &str = r#"# RideShell configuration. Every setting is optional; defaults are shown.

[prompt]
# Show the active profile (for example "[work]") in front of your shell prompt.
# Set RIDESHELL_PROMPT=0 to hide it for a single session.
indicator = true

[shell]
# Apply the profile bound to the current directory automatically after `cd`.
auto_apply = true
# Print a one-line notice when the active profile changes.
notify_on_switch = true

[shims]
# When a shimmed tool (claude, codex, gh...) starts, print which profile it
# runs as. Only shown when stderr is a terminal, never in pipes or scripts.
notify = true

[session]
# The PowerShell that runs a RideShell window: "auto" (PowerShell 7 if
# installed, otherwise Windows PowerShell), "pwsh" or "powershell".
host = "auto"
# Run cmd-style lines (dir /s, set X=1, a && b) in cmd and bash-style lines
# (ls -la, export X=1, grep) in bash automatically. Force one with a
# prefix: @cmd, @bash or @ps.
auto_switch = true
# Show a dim marker when a line runs in cmd or bash.
show_dialect = true

[history]
# New Claude Code / Codex accounts share conversation history with the tool's
# original folder (~/.claude, ~/.codex), so `claude --resume` lists the
# conversations you already had. Off: every account's conversations stay its
# own. Per account: `rideshell history share|separate <tool> --profile <name>`.
share_new_accounts = false

[guard]
# Before a sensitive command (below), ask when this terminal's account differs
# from the account the folder is bound to (for example after `profile use`).
# Works for tools routed through RideShell (claude, codex, gh, git once a Git
# identity is connected, and any `rideshell shim add <tool>`).
# RIDESHELL_GUARD=0 skips it for one command.
enabled = true
# A tool, then words that must all appear among its arguments.
commands = [
    "git push",
    "gh pr create",
    "gh pr merge",
    "gh release create",
    "gh repo create",
    "gh repo delete",
    "wrangler deploy",
    "wrangler publish",
    "vercel deploy",
    "vercel --prod",
    "netlify deploy",
    "fly deploy",
    "npm publish",
    "pnpm publish",
    "yarn publish",
    "cargo publish",
    "docker push",
    "terraform apply",
    "kubectl apply",
    "firebase deploy",
]
"#;

impl Config {
    pub fn load(paths: &Paths) -> Result<Self> {
        Self::load_file(&paths.config_file())
    }

    pub fn load_file(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).with_context(|| format!("invalid configuration file {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
        }
    }

    /// Write the commented default configuration if no file exists yet.
    /// Returns true when a file was created. Never overwrites.
    pub fn write_default_if_missing(paths: &Paths) -> Result<bool> {
        let path = paths.config_file();
        if path.exists() {
            return Ok(false);
        }
        paths.ensure()?;
        std::fs::write(&path, DEFAULT_CONFIG_TOML).with_context(|| format!("cannot write {}", path.display()))?;
        Ok(true)
    }

    /// Save (used by RideShell App's Settings page). Atomic replace.
    pub fn save(&self, paths: &Paths) -> Result<()> {
        paths.ensure()?;
        let path = paths.config_file();
        let text = format!(
            "# RideShell configuration (also editable in RideShell App: Settings).
# Run `rideshell config show` for all settings and their meaning.

{}",
            self.to_toml()
        );
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).with_context(|| format!("cannot write {}", tmp.display()))?;
        std::fs::rename(&tmp, &path).with_context(|| format!("cannot replace {}", path.display()))?;
        Ok(())
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_matches_defaults() {
        let parsed: Config = toml::from_str(DEFAULT_CONFIG_TOML).unwrap();
        assert_eq!(parsed, Config::default());
    }

    #[test]
    fn partial_file_uses_defaults() {
        let parsed: Config = toml::from_str("[prompt]\nindicator = false\n").unwrap();
        assert!(!parsed.prompt.indicator);
        assert!(parsed.shell.auto_apply);
    }

    #[test]
    fn save_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths { config_dir: dir.path().into(), data_dir: dir.path().into() };
        let mut c = Config::default();
        c.session.auto_switch = false;
        c.prompt.indicator = false;
        c.save(&paths).unwrap();
        assert_eq!(Config::load(&paths).unwrap(), c);
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(toml::from_str::<Config>("[prompt]\nindicatr = false\n").is_err());
    }
}
