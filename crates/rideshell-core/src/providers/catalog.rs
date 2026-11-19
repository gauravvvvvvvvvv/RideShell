//! Any tool, connected to an account by what the tool itself offers.
//!
//! RideShell keeps no list of tools. A tool is connected as `custom-<tool>`
//! with whatever its own CLI provides (see [`crate::discover`]): a variable
//! for its config folder or file (each account gets its own location), and
//! the variables that hold its API keys. Signing in is the tool's own job. The
//! folder and file variables reach only that tool, never other programs.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;

use super::{absolute_path, Diagnostic, Provider, ProviderContext, SettingSpec};
use crate::store::Settings;

// ----- any other tool -----------------------------------------------------------

/// Identity ids of custom tools are `custom-<tool>`.
pub fn is_custom_id(id: &str) -> bool {
    id.len() > 7 && id[..7].eq_ignore_ascii_case("custom-")
}

pub fn custom_id(tool: &str) -> String {
    format!("custom-{}", tool.to_ascii_lowercase())
}

/// A tool the catalogue does not know: the user names it and, optionally, the
/// variable that selects its config folder (`folder_var`) or config file
/// (`file_var` + `file`). API keys for it are ordinary profile secrets.
pub struct CustomProvider;

pub const CUSTOM: CustomProvider = CustomProvider;

const CUSTOM_SETTINGS: &[SettingSpec] = &[
    SettingSpec { key: "tool", description: "The command, e.g. vercel" },
    SettingSpec {
        key: "folder_var",
        description: "Variable that selects the tool's config folder (each account gets its own folder)",
    },
    SettingSpec { key: "file_var", description: "Variable that selects the tool's config file" },
    SettingSpec {
        key: "file",
        description: "Config file name inside the account folder (with file_var; default: config)",
    },
    SettingSpec { key: "config_dir", description: "Use an existing folder instead of a RideShell-managed one" },
    SettingSpec {
        key: "key_vars",
        description: "Variables that hold its API keys, comma-separated, e.g. VERCEL_TOKEN",
    },
];

/// API-key variables of a custom tool.
pub fn custom_key_vars(settings: &Settings) -> Vec<String> {
    settings
        .get("key_vars")
        .map(|v| v.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect())
        .unwrap_or_default()
}

impl CustomProvider {
    fn dir(settings: &Settings, ctx: &ProviderContext) -> PathBuf {
        match settings.get("config_dir") {
            Some(d) => PathBuf::from(d),
            None => ctx
                .accounts_dir
                .join(custom_id(settings.get("tool").map(String::as_str).unwrap_or("tool")))
                .join(ctx.profile),
        }
    }
}

impl Provider for CustomProvider {
    fn id(&self) -> &'static str {
        "custom"
    }
    fn display_name(&self) -> &'static str {
        "Any other tool"
    }
    fn description(&self) -> &'static str {
        "name a tool and the variable that selects its config folder or file"
    }
    fn tool(&self) -> &'static str {
        ""
    }
    fn isolation(&self) -> &'static str {
        "the folder or file variable you name points to a separate location per account, for this tool only"
    }
    fn tool_scoped(&self) -> bool {
        true
    }
    fn settings(&self) -> &'static [SettingSpec] {
        CUSTOM_SETTINGS
    }
    fn normalize(&self, mut settings: Settings) -> Settings {
        if let Some(dir) = settings.get_mut("config_dir") {
            *dir = absolute_path(dir);
        }
        for k in ["folder_var", "file_var"] {
            if let Some(v) = settings.get_mut(k) {
                *v = v.trim().to_string();
            }
        }
        settings
    }
    fn validate(&self, settings: &Settings) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        match settings.get("tool") {
            Some(t) if crate::shims::validate_tool_name(t).is_ok() => {}
            Some(t) => out.push(Diagnostic::error(format!("'{t}' is not a valid command name"))),
            None => out.push(Diagnostic::error("set tool=<command>")),
        }
        for k in ["folder_var", "file_var"] {
            if let Some(v) = settings.get(k) {
                if let Err(e) = crate::names::validate_env_key(v) {
                    out.push(Diagnostic::error(format!("{k}: {e}")));
                }
            }
        }
        for k in custom_key_vars(settings) {
            if let Err(e) = crate::names::validate_env_key(&k) {
                out.push(Diagnostic::error(format!("key_vars: {e}")));
            }
        }
        if settings.contains_key("file") && !settings.contains_key("file_var") {
            out.push(Diagnostic::error("'file' needs 'file_var'"));
        }
        if let Some(f) = settings.get("file") {
            if f.contains(['/', '\\']) || f.starts_with('.') && f.len() < 2 {
                out.push(Diagnostic::error("'file' must be a plain file name"));
            }
        }
        out
    }
    fn environment(&self, settings: &Settings, ctx: &ProviderContext) -> Result<BTreeMap<String, String>> {
        let dir = Self::dir(settings, ctx);
        let mut env = BTreeMap::new();
        if let Some(v) = settings.get("folder_var") {
            env.insert(v.clone(), dir.to_string_lossy().into_owned());
        }
        if let Some(v) = settings.get("file_var") {
            let file = settings.get("file").map(String::as_str).unwrap_or("config");
            env.insert(v.clone(), dir.join(file).to_string_lossy().into_owned());
        }
        Ok(env)
    }
    fn required_dirs(&self, settings: &Settings, ctx: &ProviderContext) -> Vec<PathBuf> {
        if settings.contains_key("folder_var") || settings.contains_key("file_var") {
            vec![Self::dir(settings, ctx)]
        } else {
            Vec::new()
        }
    }
    fn summary(&self, settings: &Settings, ctx: &ProviderContext) -> String {
        let tool = settings.get("tool").map(String::as_str).unwrap_or("?");
        let mut parts = Vec::new();
        if let Some(v) = settings.get("folder_var") {
            parts.push(format!("{v} → {}", Self::dir(settings, ctx).display()));
        }
        if let Some(v) = settings.get("file_var") {
            parts.push(format!("{v} → {}", settings.get("file").map(String::as_str).unwrap_or("config")));
        }
        if parts.is_empty() {
            format!("{tool} (sign-in and keys as set up)")
        } else {
            format!("{tool}: {}", parts.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn ctx(profile: &str) -> ProviderContext<'_> {
        ProviderContext { profile, accounts_dir: Path::new("/data/accounts") }
    }

    #[test]
    fn custom_tools() {
        assert!(is_custom_id("custom-vercel") && !is_custom_id("custom") && !is_custom_id("claude"));
        let s: Settings = [
            ("tool".into(), "vercel".into()),
            ("folder_var".into(), "VERCEL_HOME".into()),
            ("key_vars".into(), "VERCEL_TOKEN, OTHER_KEY".into()),
        ]
        .into();
        assert!(CUSTOM.validate(&s).is_empty(), "{:?}", CUSTOM.validate(&s));
        let env = CUSTOM.environment(&s, &ctx("work")).unwrap();
        assert_eq!(PathBuf::from(&env["VERCEL_HOME"]), Path::new("/data/accounts/custom-vercel/work"));
        assert_eq!(custom_key_vars(&s), vec!["VERCEL_TOKEN", "OTHER_KEY"]);
        assert!(CUSTOM.tool_scoped(), "a tool's own variables never reach other programs");
        let bad: Settings = [("tool".into(), "a b".into())].into();
        assert!(!CUSTOM.validate(&bad).is_empty());
        assert!(!CUSTOM.validate(&Settings::new()).is_empty(), "tool is required");
    }

    #[test]
    fn every_registry_id_is_unique() {
        let ids: Vec<_> = super::super::registry().iter().map(|p| p.id()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len());
    }
}
