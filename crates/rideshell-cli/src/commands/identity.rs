use std::process::Command;

use anyhow::{bail, Context, Result};
use rideshell_core::providers::{self, Provider, ProviderContext, Severity};
use rideshell_core::store::{Profile, Store};
use rideshell_core::{brand, exec, shims};

use super::{history, profile::check_not_secret, resolve_here, run, secret, shim, Ctx};
use crate::cli::BrowserArg;
use crate::cli::{AuthCommand, IdentityCommand, Scope};
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
    if provider.login_args().is_some() || settings.contains_key("login") {
        ui::hint(format!("sign in: {} auth login {} --profile {}", brand::BIN_NAME, identity_id, p.name));
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

// ----- auth ------------------------------------------------------------------

pub fn auth(ctx: &Ctx, cmd: AuthCommand) -> Result<i32> {
    match cmd {
        AuthCommand::Login { provider, profile, browser, email } => {
            login_logout(ctx, &provider, profile.as_deref(), true, browser, email.as_deref())
        }
        AuthCommand::Logout { provider, profile } => {
            login_logout(ctx, &provider, profile.as_deref(), false, BrowserArg::Default, None)
        }
        AuthCommand::Status { provider, profile } => status(ctx, provider.as_deref(), profile.as_deref()),
    }
}

fn pick_profile(ctx: &Ctx, store: &Store, name: Option<&str>) -> Result<Profile> {
    match name {
        Some(n) => store.require_profile(n),
        None => resolve_here(ctx, store, None, None)?
            .profile
            .with_context(|| "no profile is active here; pass --profile".to_string()),
    }
}

pub fn login_logout(
    ctx: &Ctx,
    provider_id: &str,
    profile: Option<&str>,
    login: bool,
    browser: BrowserArg,
    email: Option<&str>,
) -> Result<i32> {
    let provider = provider_or_explain(provider_id)?;
    let mut store = ctx.store()?;
    let p = pick_profile(ctx, &store, profile)?;
    // Custom tools keep their own sign-in arguments.
    let custom_settings = if providers::catalog::is_custom_id(provider_id) {
        Some(store.identities(p.id)?.remove(provider_id).with_context(|| format!("{} has no {provider_id}", p.name))?)
    } else {
        None
    };
    let args: Vec<String> = match &custom_settings {
        Some(s) if login => providers::catalog::custom_login(s).with_context(|| {
            format!("no sign-in command saved for {provider_id} (identity set ... login=\"auth login\")")
        })?,
        Some(_) => bail!("custom tools have no saved sign-out command; run the tool's own"),
        None => if login { provider.login_args() } else { provider.logout_args() }
            .with_context(|| format!("{} has no sign-in command", provider.display_name()))?
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };
    let tool_name: String = match &custom_settings {
        Some(s) => s.get("tool").cloned().unwrap_or_default(),
        None => provider.tool().to_string(),
    };
    if custom_settings.is_none() && !store.identities(p.id)?.contains_key(provider.id()) {
        // First use: give this profile its own account with defaults.
        store.set_identity(&p.name, provider.id(), &Default::default())?;
        ui::success(format!("{} now has its own {} account", ui::accent(&p.name), provider.display_name()));
        history::share_by_default(ctx, &store, &p, provider.id());
        if provider.auto_shim() {
            shim::add(ctx, provider.tool())?;
        }
    }
    let mut prepared = run::prepare(ctx, &store, Some(&p.name), None, true)?;
    run::add_tool_scoped(ctx, &store, &mut prepared, &tool_name)?;
    let others: Vec<String> = store.list_profiles()?.into_iter().filter(|o| o.id != p.id).map(|o| o.name).collect();
    drop(store);
    let real = shims::find_real(&tool_name, Some(&ctx.paths.shims_dir()))?;
    let mut args = args;
    if login {
        if let Some(e) = email {
            if provider.id() == "claude" {
                args.extend(["--email".to_string(), e.to_string()]);
            } else {
                ui::warn(format!("{} does not support pre-filling an e-mail; ignored", provider.display_name()));
            }
        }
        if browser == BrowserArg::Private {
            let me = std::env::current_exe().context("cannot determine the executable path")?;
            let me = me.to_string_lossy().into_owned();
            prepared.plan.set.push(("BROWSER".into(), me.clone()));
            prepared.plan.set.push(("GH_BROWSER".into(), me));
            prepared.plan.set.push((super::browser::PRIVATE_ENV.into(), "1".into()));
        }
        sign_in_help(&tool_name, &p.name, &others, browser);
    }
    anstream::eprintln!(
        "{} {} {} for profile {}",
        ui::dim(format!("{} ▸", brand::BIN_NAME)),
        tool_name,
        args.join(" "),
        ui::accent(&p.name)
    );
    exec::run(&real.to_string_lossy(), &args, &prepared.plan, None)
}

/// What to expect before the tool opens its sign-in page.
fn sign_in_help(tool: &str, profile: &str, others: &[String], browser: BrowserArg) {
    anstream::eprintln!();
    anstream::eprintln!("  Signing {} in to {tool}.", ui::accent(profile));
    match browser {
        BrowserArg::Default => {
            if !others.is_empty() {
                anstream::eprintln!(
                    "  {}",
                    ui::dim(format!(
                        "Your browser may already be signed in as the account you use in {}.",
                        others.join(", ")
                    ))
                );
                anstream::eprintln!(
                    "  {}",
                    ui::dim("To use a different account, switch account or sign out on the page that opens,")
                );
                anstream::eprintln!(
                    "  {}",
                    ui::dim("or copy the link shown below into another browser or browser profile.")
                );
                anstream::eprintln!(
                    "  {}",
                    ui::dim(
                        "(A private window: add `--browser private`; pre-fill an e-mail: `--email you@example.com`.)"
                    )
                );
            }
        }
        BrowserArg::Private => {
            anstream::eprintln!("  {}", ui::dim("The sign-in page opens in a private window of your default browser."));
        }
    }
    anstream::eprintln!();
}

fn status(ctx: &Ctx, provider_id: Option<&str>, profile: Option<&str>) -> Result<i32> {
    let wanted: Vec<&'static dyn Provider> = match provider_id {
        Some(id) => vec![provider_or_explain(id)?],
        None => providers::registry().iter().copied().filter(|p| p.status_args().is_some()).collect(),
    };
    let store = ctx.store()?;
    let profiles = match profile {
        Some(n) => vec![store.require_profile(n)?],
        None => store.list_profiles()?,
    };
    let mut any = false;
    for provider in wanted {
        let Some(args) = provider.status_args() else { continue };
        let Ok(real) = shims::find_real(provider.tool(), Some(&ctx.paths.shims_dir())) else {
            continue;
        };
        for p in &profiles {
            let Some(settings) = store.identities(p.id)?.remove(provider.id()) else { continue };
            any = true;
            let accounts = ctx.paths.accounts_dir();
            anstream::println!(
                "{} {} {}",
                ui::accent(&p.name),
                ui::paint(provider.display_name(), ui::BOLD),
                ui::dim(provider.summary(&settings, &pctx(ctx, p, &accounts)))
            );
            let prepared = run::prepare(ctx, &store, Some(&p.name), None, true)?;
            let mut cmd = Command::new(&real);
            cmd.args(args.iter());
            prepared.plan.apply_to_command(&mut cmd);
            match cmd.output() {
                Ok(out) => {
                    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
                    for line in text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with("WARNING: proceeding")) {
                        anstream::println!("  {line}");
                    }
                }
                Err(e) => ui::warn(format!("cannot run {}: {e}", real.display())),
            }
            anstream::println!();
        }
    }
    if !any {
        anstream::println!(
            "No profile has a tool account yet. Example: `{} identity set work claude`.",
            brand::BIN_NAME
        );
    }
    Ok(0)
}
