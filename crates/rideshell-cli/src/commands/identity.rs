use anyhow::{bail, Context, Result};
use rideshell_core::brand;
use rideshell_core::providers::{self, Provider, ProviderContext, Severity};
use rideshell_core::store::Profile;

use super::{history, profile::check_not_secret, resolve_here, secret, shim, Ctx};
use crate::cli::{IdentityCommand, Scope};
use crate::ui;

pub fn run(ctx: &Ctx, cmd: IdentityCommand) -> Result<i32> {
    match cmd {
        IdentityCommand::Set { profile, provider, settings } => set(ctx, &profile, &provider, &settings),
        IdentityCommand::Unset { profile, provider, keys } => unset(ctx, &profile, &provider, &keys),
        IdentityCommand::Show { profile } => show(ctx, profile.as_deref()),
    }
}

fn provider_or_explain(id: &str) -> Result<&'static dyn Provider> {
    if let Some(p) = providers::find(id) {
        return Ok(p);
    }
    let available: Vec<_> = providers::registry().iter().map(|p| p.id()).collect();
    bail!(
        "'{id}' is not in the catalogue (available: {}). Any other tool: \
         `{bin} identity set <profile> custom tool={id} [folder_var=VAR]`, and \
         `{bin} secret set KEY --profile <profile>` for its API key.",
        available.join(", "),
        bin = brand::BIN_NAME
    )
}

pub fn pctx<'a>(ctx: &'a Ctx, profile: &'a Profile, accounts: &'a std::path::Path) -> ProviderContext<'a> {
    let _ = ctx;
    ProviderContext { profile: &profile.name, accounts_dir: accounts }
}

pub fn set(ctx: &Ctx, profile: &str, provider_id: &str, pairs: &[String]) -> Result<i32> {
    let provider = provider_or_explain(provider_id)?;
    // "Any other tool" is stored per tool: custom-<tool>.
    let identity_id = if provider.id() == "custom" {
        let tool = if providers::catalog::is_custom_id(provider_id) {
            provider_id[7..].to_string()
        } else {
            pairs
                .iter()
                .find_map(|p| p.strip_prefix("tool=").map(str::to_string))
                .context("name the tool: `identity set <profile> custom tool=<command> [folder_var=VAR]`")?
        };
        providers::catalog::custom_id(&tool)
    } else {
        provider.id().to_string()
    };
    let mut store = ctx.store()?;
    let p = store.require_profile(profile)?;
    let is_new = !store.identities(p.id)?.contains_key(&identity_id);
    let mut settings = store.identities(p.id)?.remove(&identity_id).unwrap_or_default();
    if provider.id() == "custom" && !settings.contains_key("tool") {
        settings.insert("tool".into(), identity_id[7..].to_string());
    }

    let mut secrets: Vec<(&'static str, String)> = Vec::new();

    for pair in pairs {
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k.trim(), Some(v.to_string())),
            None => (pair.trim(), None),
        };
        if let Some(spec) = provider.secrets().iter().find(|s| s.key == k) {
            let value = match v {
                Some(v) => {
                    ui::warn(
                        "secrets typed on the command line can end up in shell history; omit the value to be prompted",
                    );
                    v
                }
                None => secret::read_secret(spec.env, false)?,
            };
            secrets.push((spec.env, value));
            continue;
        }
        let Some(v) = v else { bail!("expected KEY=VALUE, got '{pair}'") };
        if !provider.settings().iter().any(|s| s.key == k) {
            let mut known: Vec<_> = provider.settings().iter().map(|s| s.key).collect();
            known.extend(provider.secrets().iter().map(|s| s.key));
            bail!("'{k}' is not a {} setting (known: {})", provider.id(), known.join(", "));
        }
        check_not_secret(k, &v, false)?;
        settings.insert(k.to_string(), v);
    }

    let settings = provider.normalize(settings);
    let diagnostics = provider.validate(&settings);
    for d in &diagnostics {
        match d.severity {
            Severity::Error => ui::error(&d.message),
            Severity::Warning => ui::warn(&d.message),
        }
    }
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        bail!("identity not saved");
    }
    store.set_identity(&p.name, &identity_id, &settings)?;
    let accounts = ctx.paths.accounts_dir();
    let label = settings.get("tool").cloned().unwrap_or_else(|| provider.display_name().to_string());
    ui::success(format!(
        "{} connected {}: {}",
        ui::accent(&p.name),
        label,
        provider.summary(&settings, &pctx(ctx, &p, &accounts))
    ));
    if is_new {
        history::share_by_default(ctx, &store, &p, &identity_id);
    }
    drop(store);

    for (env, value) in secrets {
        secret::set(ctx, &Scope { profile: Some(p.name.clone()), dir: None }, env, &value)?;
    }
    if provider.auto_shim() {
        shim::add(ctx, provider.tool())?;
        shim::path_hint(ctx);
    } else if let Some(tool) = settings.get("tool") {
        shim::add(ctx, tool)?;
        shim::path_hint(ctx);
    }
    if provider.id() != "git" {
        ui::hint(format!(
            "sign in from the tool itself: open a {} tab as {} and run `{}`",
            brand::APP_NAME,
            p.name,
            settings.get("tool").map(String::as_str).unwrap_or(provider.tool())
        ));
    }
    if !provider.secrets().is_empty() {
        let keys: Vec<&str> = provider.secrets().iter().map(|s| s.key).collect();
        ui::hint(format!(
            "or set an API key: {} identity set {} {} {}",
            brand::BIN_NAME,
            p.name,
            identity_id,
            keys.join(" ")
        ));
    }
    Ok(0)
}

fn unset(ctx: &Ctx, profile: &str, provider_id: &str, keys: &[String]) -> Result<i32> {
    let mut store = ctx.store()?;
    let p = store.require_profile(profile)?;
    let id = match providers::find(provider_id) {
        Some(_) if providers::catalog::is_custom_id(provider_id) => provider_id,
        Some(p) => p.id(),
        None => provider_id,
    };
    if keys.is_empty() {
        if store.remove_identity(&p.name, id)? {
            ui::success(format!("removed {id} from {}", ui::accent(&p.name)));
            ui::hint(
                "the account's own directory (and its login) is kept; delete it manually if you no longer need it",
            );
        } else {
            ui::warn(format!("{} has no {id} identity", p.name));
        }
        return Ok(0);
    }
    let Some(mut settings) = store.identities(p.id)?.remove(id) else {
        bail!("{} has no {id} identity", p.name);
    };
    for k in keys {
        if settings.remove(k).is_none() {
            ui::warn(format!("{k} was not set"));
        }
    }
    store.set_identity(&p.name, id, &settings)?;
    ui::success(format!("updated {id} identity of {}", ui::accent(&p.name)));
    Ok(0)
}

fn show(ctx: &Ctx, profile: Option<&str>) -> Result<i32> {
    let store = ctx.store()?;
    let p = match profile {
        Some(n) => store.require_profile(n)?,
        None => resolve_here(ctx, &store, None, None)?
            .profile
            .with_context(|| "no profile is active here; name one explicitly".to_string())?,
    };
    let accounts = ctx.paths.accounts_dir();
    let identities = store.identities(p.id)?;
    if identities.is_empty() {
        anstream::println!("{} has no identities.", p.name);
    }
    for (id, settings) in identities {
        let summary =
            providers::find(&id).map(|pr| pr.summary(&settings, &pctx(ctx, &p, &accounts))).unwrap_or_default();
        anstream::println!("{}  {}", ui::paint(&id, ui::BOLD), summary);
        for (k, v) in settings {
            anstream::println!("  {k} = {v}");
        }
    }
    Ok(0)
}

pub fn providers() -> Result<i32> {
    for p in providers::registry() {
        let det = p.detect();
        let state = match (&det.path, &det.version) {
            (Some(_), Some(v)) => ui::paint(v, ui::OK),
            (Some(path), None) => ui::paint(path.display().to_string(), ui::OK),
            _ => ui::paint("not installed", ui::WARN),
        };
        anstream::println!(
            "{}  {} — {}  {}",
            ui::paint(p.id(), ui::ACCENT),
            p.display_name(),
            p.description(),
            ui::dim(format!("({state})"))
        );
        ui::hint(format!("isolation: {}", p.isolation()));
        for s in p.settings() {
            ui::hint(format!("{:<10} {}", s.key, s.description));
        }
        for s in p.secrets() {
            ui::hint(format!("{:<10} {} (secret, stored as {})", s.key, s.description, s.env));
        }
        anstream::println!();
    }
    anstream::println!(
        "Any other tool: `{bin} secret set KEY --profile P`, `{bin} profile env set P KEY VALUE`, `{bin} shim add <tool>`.",
        bin = brand::BIN_NAME
    );
    Ok(0)
}
