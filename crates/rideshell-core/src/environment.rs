//! The environment a process should get, assembled in layers:
//!
//! 1. identity adapters of the active profile (e.g. `CLAUDE_CONFIG_DIR`)
//! 2. the profile's own variables and secrets
//! 3. directory (project) variables and secrets, shallow to deep
//!
//! Later layers win. Secrets are carried as vault references until a command
//! is actually launched; the prompt hook never materialises them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::pathkey;
use crate::providers::{self, ProviderContext};
use crate::store::{Profile, Store};
use crate::vault::{self, Vault};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvValue {
    Plain(String),
    Secret { vault_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvEntry {
    pub value: EnvValue,
    /// Where it came from, for display (`profile work`, `claude identity`, `directory /x`).
    pub origin: String,
}

#[derive(Debug, Clone, Default)]
pub struct LayeredEnv {
    pub vars: BTreeMap<String, EnvEntry>,
    pub warnings: Vec<String>,
    /// Directories the configured tools need (created before launching).
    pub required_dirs: Vec<PathBuf>,
}

impl LayeredEnv {
    pub fn has_secrets(&self) -> bool {
        self.vars.values().any(|e| matches!(e.value, EnvValue::Secret { .. }))
    }
}

pub fn layered_env(
    store: &Store,
    accounts_dir: &Path,
    profile: Option<&Profile>,
    cwd: Option<&Path>,
) -> Result<LayeredEnv> {
    let mut out = LayeredEnv::default();

    if let Some(profile) = profile {
        let ctx = ProviderContext { profile: &profile.name, accounts_dir };
        for (provider_id, settings) in store.identities(profile.id)? {
            let Some(provider) = providers::find(&provider_id) else {
                out.warnings
                    .push(format!("profile '{}' uses unknown provider '{provider_id}' (ignored)", profile.name));
                continue;
            };
            if provider.tool_scoped() {
                continue; // applied only when that tool runs: see `tool_scoped_env`
            }
            match provider.environment(&settings, &ctx) {
                Ok(env) => {
                    for (k, v) in env {
                        out.vars.insert(
                            k,
                            EnvEntry { value: EnvValue::Plain(v), origin: format!("{provider_id} identity") },
                        );
                    }
                }
                Err(e) => out.warnings.push(format!("{provider_id}: {e:#}")),
            }
            out.required_dirs.extend(provider.required_dirs(&settings, &ctx));
        }
        let origin = format!("profile {}", profile.name);
        for (k, v) in store.env_vars(profile.id)? {
            out.vars.insert(k, EnvEntry { value: EnvValue::Plain(v), origin: origin.clone() });
        }
        for k in store.secret_keys(profile.id)? {
            let vault_id = vault::profile_secret_id(profile.id, &k);
            out.vars.insert(k, EnvEntry { value: EnvValue::Secret { vault_id }, origin: origin.clone() });
        }
    }

    if let Some(dir) = cwd.and_then(|c| pathkey::canonical_dir(c).ok()) {
        if let Ok(key) = pathkey::path_key(&dir) {
            let mut vars: Vec<_> =
                store.dir_vars()?.into_iter().filter(|v| pathkey::key_contains(&v.path_key, &key)).collect();
            vars.sort_by_key(|v| v.path_key.len());
            for v in vars {
                let value = if v.secret {
                    EnvValue::Secret { vault_id: vault::dir_secret_id(v.id) }
                } else {
                    EnvValue::Plain(v.value)
                };
                out.vars.insert(v.key, EnvEntry { value, origin: format!("directory {}", v.path) });
            }
        }
    }
    Ok(out)
}

/// Concrete variables. With `vault = None` secrets are left out (prompt
/// hook); otherwise they are read from the vault (command launch).
pub fn materialize(env: &LayeredEnv, vault: Option<&dyn Vault>) -> (BTreeMap<String, String>, Vec<String>) {
    let mut vars = BTreeMap::new();
    let mut warnings = Vec::new();
    for (k, entry) in &env.vars {
        match (&entry.value, vault) {
            (EnvValue::Plain(v), _) => {
                vars.insert(k.clone(), v.clone());
            }
            (EnvValue::Secret { vault_id }, Some(vault)) => match vault.get(vault_id) {
                Ok(Some(v)) => {
                    vars.insert(k.clone(), v);
                }
                Ok(None) => warnings.push(format!(
                    "secret {k} ({}) is missing from the credential store; set it again with `{} secret set`",
                    entry.origin,
                    crate::brand::BIN_NAME
                )),
                Err(e) => warnings.push(format!("{e:#}")),
            },
            (EnvValue::Secret { .. }, None) => {}
        }
    }
    (vars, warnings)
}

/// Variables of `profile`'s tool-scoped identities for `tool` (for example
/// `XDG_CONFIG_HOME` for the Stripe CLI), and the folders they need. Applied
/// only when that tool itself is launched, never to other programs.
pub fn tool_scoped_env(
    store: &Store,
    accounts_dir: &Path,
    profile: &Profile,
    tool: &str,
) -> Result<(std::collections::BTreeMap<String, String>, Vec<std::path::PathBuf>)> {
    let ctx = ProviderContext { profile: &profile.name, accounts_dir };
    let mut env = std::collections::BTreeMap::new();
    let mut dirs = Vec::new();
    for (provider_id, settings) in store.identities(profile.id)? {
        let Some(provider) = providers::find(&provider_id) else { continue };
        // Custom tools name their command in their settings.
        let command = settings.get("tool").map(String::as_str).unwrap_or(provider.tool());
        if provider.tool_scoped() && command.eq_ignore_ascii_case(tool) {
            env.extend(provider.environment(&settings, &ctx)?);
            dirs.extend(provider.required_dirs(&settings, &ctx));
        }
    }
    Ok((env, dirs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::FileVault;

    #[test]
    fn layers_override_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(tmp.path()).unwrap();
        let proj = root.join("proj");
        let deep = proj.join("svc");
        std::fs::create_dir_all(&deep).unwrap();

        let mut s = Store::open_in_memory().unwrap();
        let p = s.create_profile("work", "").unwrap();
        s.set_identity("work", "claude", &Default::default()).unwrap();
        s.set_env("work", "REGION", "eu").unwrap();
        s.set_secret_env("work", "ANTHROPIC_API_KEY").unwrap();
        let k = |d: &Path| pathkey::path_key(d).unwrap();
        s.set_dir_env(proj.to_str().unwrap(), &k(&proj), "REGION", "us", false).unwrap();
        let deep_id = s.set_dir_env(deep.to_str().unwrap(), &k(&deep), "ANTHROPIC_API_KEY", "", true).unwrap();

        let env = layered_env(&s, &root.join("accounts"), Some(&p), Some(&deep)).unwrap();
        assert!(env.vars["CLAUDE_CONFIG_DIR"].origin.contains("claude"));
        assert_eq!(env.vars["REGION"].value, EnvValue::Plain("us".into()), "directory beats profile");
        assert_eq!(
            env.vars["ANTHROPIC_API_KEY"].value,
            EnvValue::Secret { vault_id: vault::dir_secret_id(deep_id) },
            "project key beats account key"
        );

        let vault = FileVault::for_tests(root.join("v.json"));
        vault.set(&vault::dir_secret_id(deep_id), "sk-project").unwrap();
        let (hook_vars, _) = materialize(&env, None);
        assert!(!hook_vars.contains_key("ANTHROPIC_API_KEY"), "the prompt hook never sees secrets");
        let (run_vars, warnings) = materialize(&env, Some(&vault));
        assert_eq!(run_vars["ANTHROPIC_API_KEY"], "sk-project");
        assert!(warnings.is_empty());

        // Outside the project only the profile layers apply.
        let env = layered_env(&s, &root.join("accounts"), Some(&p), Some(&root)).unwrap();
        assert_eq!(env.vars["REGION"].value, EnvValue::Plain("eu".into()));
        let (_, warnings) = materialize(&env, Some(&vault));
        assert_eq!(warnings.len(), 1, "missing profile secret is reported");
    }
}
