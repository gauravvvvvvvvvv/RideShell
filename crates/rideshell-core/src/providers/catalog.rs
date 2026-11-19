//! The tool catalogue: every account a profile can connect.
//!
//! Each entry uses the mechanism the tool itself documents for choosing an
//! account per process:
//!
//! * **Account folder** ([`AccountDirProvider`]): the tool keeps its login in a
//!   folder or file chosen by an environment variable. Each RideShell account
//!   gets its own, and signs in once with the tool's own login command.
//! * **API token** ([`TokenProvider`]): the tool reads a token from an
//!   environment variable. Each account stores its own token in the OS
//!   credential store; it is passed only to programs RideShell launches.
//! * **Any other tool** ([`CustomProvider`]): the user names the tool and the
//!   variable that selects its config folder or file.
//!
//! Tools whose documentation offers neither are not listed: RideShell does not
//! copy or move a tool's login files behind its back.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;

use super::{absolute_path, AccountDirProvider, Diagnostic, Provider, ProviderContext, SecretSpec, SettingSpec};
use crate::store::Settings;

// ----- account folders -----------------------------------------------------------

pub const GLAB: AccountDirProvider = AccountDirProvider {
    id: "glab",
    display_name: "GitLab CLI",
    tool: "glab",
    vars: &[("GLAB_CONFIG_DIR", "")],
    secrets: &[SecretSpec {
        key: "token",
        env: "GITLAB_TOKEN",
        description: "GitLab token (instead of glab auth login)",
    }],
    login: Some(&["auth", "login"]),
    logout: Some(&["auth", "logout"]),
    status: Some(&["auth", "status"]),
    isolation: "separate GLAB_CONFIG_DIR per account",
    category: "Code hosting",
    auto_attach: false,
};

pub const AWS: AccountDirProvider = AccountDirProvider {
    id: "aws",
    display_name: "AWS CLI",
    tool: "aws",
    vars: &[("AWS_CONFIG_FILE", "config"), ("AWS_SHARED_CREDENTIALS_FILE", "credentials")],
    secrets: &[],
    login: Some(&["configure"]),
    logout: None,
    status: Some(&["sts", "get-caller-identity"]),
    isolation:
        "separate AWS_CONFIG_FILE and AWS_SHARED_CREDENTIALS_FILE per account (SSO token cache stays in ~/.aws/sso)",
    category: "Cloud",
    auto_attach: false,
};

pub const AZURE: AccountDirProvider = AccountDirProvider {
    id: "az",
    display_name: "Azure CLI",
    tool: "az",
    vars: &[("AZURE_CONFIG_DIR", "")],
    secrets: &[],
    login: Some(&["login"]),
    logout: Some(&["logout"]),
    status: Some(&["account", "show"]),
    isolation: "separate AZURE_CONFIG_DIR per account: login and subscriptions are per account",
    category: "Cloud",
    auto_attach: false,
};

pub const GCLOUD: AccountDirProvider = AccountDirProvider {
    id: "gcloud",
    display_name: "Google Cloud CLI",
    tool: "gcloud",
    vars: &[("CLOUDSDK_CONFIG", "")],
    secrets: &[],
    login: Some(&["auth", "login"]),
    logout: Some(&["auth", "revoke"]),
    status: Some(&["auth", "list"]),
    isolation: "separate CLOUDSDK_CONFIG per account: credentials and configurations are per account",
    category: "Cloud",
    auto_attach: false,
};

pub const KUBECTL: AccountDirProvider = AccountDirProvider {
    id: "kubectl",
    display_name: "Kubernetes (kubectl)",
    tool: "kubectl",
    vars: &[("KUBECONFIG", "config")],
    secrets: &[],
    login: None,
    logout: None,
    status: Some(&["config", "current-context"]),
    isolation: "separate KUBECONFIG per account (contexts, clusters and users)",
    category: "Containers",
    auto_attach: false,
};

pub const DOCKER: AccountDirProvider = AccountDirProvider {
    id: "docker",
    display_name: "Docker CLI",
    tool: "docker",
    vars: &[("DOCKER_CONFIG", "")],
    secrets: &[],
    login: Some(&["login"]),
    logout: Some(&["logout"]),
    status: None,
    isolation:
        "separate DOCKER_CONFIG per account (a credential helper set in config.json may still use the shared OS store)",
    category: "Containers",
    auto_attach: false,
};

pub const NPM: AccountDirProvider = AccountDirProvider {
    id: "npm",
    display_name: "npm registry",
    tool: "npm",
    vars: &[("NPM_CONFIG_USERCONFIG", ".npmrc")],
    secrets: &[],
    login: Some(&["login"]),
    logout: Some(&["logout"]),
    status: Some(&["whoami"]),
    isolation: "separate user .npmrc per account (registry logins and settings)",
    category: "Packages",
    auto_attach: false,
};

pub const HUGGINGFACE: AccountDirProvider = AccountDirProvider {
    id: "huggingface",
    display_name: "Hugging Face",
    tool: "hf",
    vars: &[("HF_HOME", "")],
    secrets: &[SecretSpec { key: "token", env: "HF_TOKEN", description: "Hugging Face access token" }],
    login: None,
    logout: None,
    status: None,
    isolation: "separate HF_HOME per account (token and cache); or an HF_TOKEN per account",
    category: "AI",
    auto_attach: false,
};

// ----- API tokens -----------------------------------------------------------

/// A tool that takes its account from an API token in an environment variable.
pub struct TokenProvider {
    pub id: &'static str,
    pub display_name: &'static str,
    pub tool: &'static str,
    pub secrets: &'static [SecretSpec],
    pub category: &'static str,
    pub status: Option<&'static [&'static str]>,
}

macro_rules! token {
    ($name:ident, $id:literal, $display:literal, $tool:literal, $env:literal, $what:literal, $cat:literal, $status:expr) => {
        pub const $name: TokenProvider = TokenProvider {
            id: $id,
            display_name: $display,
            tool: $tool,
            secrets: &[SecretSpec { key: "token", env: $env, description: $what }],
            category: $cat,
            status: $status,
        };
    };
}

token!(GEMINI, "gemini", "Gemini CLI", "gemini", "GEMINI_API_KEY", "Gemini API key", "AI coding agents", None);
token!(
    WRANGLER,
    "wrangler",
    "Cloudflare (Wrangler)",
    "wrangler",
    "CLOUDFLARE_API_TOKEN",
    "Cloudflare API token",
    "Deploy",
    Some(&["whoami"])
);
token!(
    NETLIFY,
    "netlify",
    "Netlify CLI",
    "netlify",
    "NETLIFY_AUTH_TOKEN",
    "Netlify personal access token",
    "Deploy",
    Some(&["status"])
);
token!(
    SUPABASE,
    "supabase",
    "Supabase CLI",
    "supabase",
    "SUPABASE_ACCESS_TOKEN",
    "Supabase access token",
    "Deploy",
    Some(&["projects", "list"])
);
token!(HEROKU, "heroku", "Heroku CLI", "heroku", "HEROKU_API_KEY", "Heroku API key", "Deploy", Some(&["auth:whoami"]));
token!(RAILWAY, "railway", "Railway CLI", "railway", "RAILWAY_TOKEN", "Railway token", "Deploy", Some(&["whoami"]));
token!(FLY, "fly", "Fly.io (flyctl)", "fly", "FLY_API_TOKEN", "Fly.io API token", "Deploy", Some(&["auth", "whoami"]));
token!(
    DOCTL,
    "doctl",
    "DigitalOcean (doctl)",
    "doctl",
    "DIGITALOCEAN_ACCESS_TOKEN",
    "DigitalOcean access token",
    "Cloud",
    Some(&["account", "get"])
);
token!(STRIPE, "stripe", "Stripe CLI", "stripe", "STRIPE_API_KEY", "Stripe secret key", "Payments", None);
token!(NGROK, "ngrok", "ngrok", "ngrok", "NGROK_AUTHTOKEN", "ngrok authtoken", "Networking", None);
token!(FIREBASE, "firebase", "Firebase CLI", "firebase", "FIREBASE_TOKEN", "Firebase CI token", "Deploy", None);

impl Provider for TokenProvider {
    fn id(&self) -> &'static str {
        self.id
    }
    fn display_name(&self) -> &'static str {
        self.display_name
    }
    fn description(&self) -> &'static str {
        "one API token per account"
    }
    fn tool(&self) -> &'static str {
        self.tool
    }
    fn auto_shim(&self) -> bool {
        true
    }
    fn isolation(&self) -> &'static str {
        "each account keeps its own token in the OS credential store; it is passed only to programs RideShell launches"
    }
    fn category(&self) -> &'static str {
        self.category
    }
    fn settings(&self) -> &'static [SettingSpec] {
        &[]
    }
    fn secrets(&self) -> &'static [SecretSpec] {
        self.secrets
    }
    fn validate(&self, _settings: &Settings) -> Vec<Diagnostic> {
        Vec::new()
    }
    fn environment(&self, _settings: &Settings, _ctx: &ProviderContext) -> Result<BTreeMap<String, String>> {
        // The token is a profile secret (`secret set <ENV> --profile p`), applied with the others.
        Ok(BTreeMap::new())
    }
    fn summary(&self, _settings: &Settings, _ctx: &ProviderContext) -> String {
        format!("API token ({})", self.secrets.iter().map(|s| s.env).collect::<Vec<_>>().join(", "))
    }
    fn status_args(&self) -> Option<&'static [&'static str]> {
        self.status
    }
}

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
];

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
        "the folder or file variable you name points to a separate location per account"
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
            format!("{tool} (routed through RideShell; add keys with `secret set`)")
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
    fn file_variables_point_into_the_account_folder() {
        let env = AWS.environment(&Settings::new(), &ctx("work")).unwrap();
        assert_eq!(PathBuf::from(&env["AWS_CONFIG_FILE"]), Path::new("/data/accounts/aws/work/config"));
        assert_eq!(
            PathBuf::from(&env["AWS_SHARED_CREDENTIALS_FILE"]),
            Path::new("/data/accounts/aws/work/credentials")
        );
        let env = KUBECTL.environment(&Settings::new(), &ctx("personal")).unwrap();
        assert_eq!(PathBuf::from(&env["KUBECONFIG"]), Path::new("/data/accounts/kubectl/personal/config"));
    }

    #[test]
    fn token_providers_set_nothing_themselves() {
        assert!(WRANGLER.environment(&Settings::new(), &ctx("work")).unwrap().is_empty());
        assert_eq!(WRANGLER.secrets()[0].env, "CLOUDFLARE_API_TOKEN");
    }

    #[test]
    fn custom_tools() {
        assert!(is_custom_id("custom-vercel") && !is_custom_id("custom") && !is_custom_id("claude"));
        let s: Settings = [("tool".into(), "vercel".into()), ("folder_var".into(), "VERCEL_HOME".into())].into();
        assert!(CUSTOM.validate(&s).is_empty());
        let env = CUSTOM.environment(&s, &ctx("work")).unwrap();
        assert_eq!(PathBuf::from(&env["VERCEL_HOME"]), Path::new("/data/accounts/custom-vercel/work"));
        let bad: Settings = [("tool".into(), "a b".into())].into();
        assert!(!CUSTOM.validate(&bad).is_empty());
        assert!(!CUSTOM.validate(&Settings::new()).is_empty(), "tool is required");
    }

    #[test]
    fn every_catalogue_id_is_unique() {
        let ids: Vec<_> = super::super::registry().iter().map(|p| p.id()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len());
    }
}
