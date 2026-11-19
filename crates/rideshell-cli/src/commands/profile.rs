use std::path::Path;

use anyhow::{bail, Context, Result};
use rideshell_core::envstate::EnvPlan;
use rideshell_core::pathkey;
use rideshell_core::profile_file::ProfileFile;
use rideshell_core::providers::{self, ProviderContext};
use rideshell_core::resolve::StepOutcome;
use rideshell_core::secrets;
use rideshell_core::shells::{self, Shell};
use rideshell_core::store::Store;
use rideshell_core::vault;
use rideshell_core::{brand, names};

use super::{resolve_here, Ctx};
use crate::cli::{ProfileCommand, ProfileEnvCommand, ShellArg};
use crate::ui;

pub fn run(ctx: &Ctx, cmd: ProfileCommand) -> Result<i32> {
    match cmd {
        ProfileCommand::Create { name, description, from, default } => {
            create(ctx, &name, &description, from.as_deref(), default)
        }
        ProfileCommand::List => list(ctx),
        ProfileCommand::Show { name } => show(ctx, name.as_deref()),
        ProfileCommand::Use { name, clear, emit } => use_profile(ctx, name.as_deref(), clear, emit),
        ProfileCommand::Status => unreachable!("handled by dispatch"),
        ProfileCommand::Why { path } => why(ctx, path.as_deref()),
        ProfileCommand::Edit { name, description, rename } => {
            edit(ctx, &name, description.as_deref(), rename.as_deref())
        }
        ProfileCommand::Rename { old, new } => edit(ctx, &old, None, Some(&new)),
        ProfileCommand::Duplicate { source, target } => {
            let mut store = ctx.store()?;
            let p = duplicate_with_secrets(&mut store, &source, &target)?;
            ui::success(format!("created {} as a copy of {}", ui::accent(&p.name), ui::accent(&source)));
            ui::hint("tool accounts with default settings get their own new login under the new profile's name");
            Ok(0)
        }
        ProfileCommand::Delete { name, force } => delete(ctx, &name, force),
        ProfileCommand::Default { name, clear } => default(ctx, name.as_deref(), clear),
        ProfileCommand::Env(cmd) => env(ctx, cmd),
        ProfileCommand::Bind { path, profile } => bind(ctx, &path, &profile),
        ProfileCommand::Unbind { path } => unbind(ctx, path.as_deref()),
        ProfileCommand::Bindings => bindings(ctx),
        ProfileCommand::Export { name, output } => export(ctx, &name, output.as_deref()),
        ProfileCommand::Import { file, name, replace, not_secret } => {
            import(ctx, &file, name.as_deref(), replace, not_secret)
        }
    }
}

fn create(ctx: &Ctx, name: &str, description: &str, from: Option<&str>, make_default: bool) -> Result<i32> {
    let mut store = ctx.store()?;
    let first = store.list_profiles()?.is_empty();
    let profile = match from {
        Some(src) => {
            let p = duplicate_with_secrets(&mut store, src, name)?;
            if !description.is_empty() {
                store.set_description(&p.name, description)?;
            }
            p
        }
        None => store.create_profile(name, description)?,
    };
    ui::success(format!("created profile {}", ui::accent(&profile.name)));
    if make_default || first {
        store.set_default_profile(Some(&profile.name))?;
        ui::hint(if first { "set as the default profile (first profile)" } else { "set as the default profile" });
    }
    let attached = providers::attach_default_accounts(&mut store, &profile.name)?;
    drop(store);
    if !attached.is_empty() {
        if first {
            ui::success(format!(
                "{} keeps your existing logins for: {}",
                ui::accent(&profile.name),
                attached.join(", ")
            ));
        } else {
            ui::success(format!(
                "{} has its own, separate accounts for: {}",
                ui::accent(&profile.name),
                attached.join(", ")
            ));
        }
        for tool in &attached {
            if rideshell_core::shims::find_real(tool, Some(&ctx.paths.shims_dir())).is_ok() {
                let _ = super::shim::add_quiet(ctx, tool);
            }
        }
        ui::hint(format!("sign in: {} auth login claude --profile {}", brand::BIN_NAME, profile.name));
    }
    ui::hint(format!(
        "add a Git identity: {} identity set {} git name=\"...\" email=...",
        brand::BIN_NAME,
        profile.name
    ));
    Ok(0)
}

/// Duplicate a profile including copies of its secrets in the vault.
fn duplicate_with_secrets(store: &mut Store, source: &str, target: &str) -> Result<rideshell_core::store::Profile> {
    let src = store.require_profile(source)?;
    let copy = store.duplicate_profile(&src.name, target)?;
    let v = vault::open();
    for key in store.secret_keys(src.id)? {
        match v.get(&vault::profile_secret_id(src.id, &key))? {
            Some(value) => v.set(&vault::profile_secret_id(copy.id, &key), &value)?,
            None => ui::warn(format!("secret {key} of {} is missing from the credential store; not copied", src.name)),
        }
    }
    Ok(copy)
}

fn list(ctx: &Ctx) -> Result<i32> {
    let store = ctx.store()?;
    let profiles = store.list_profiles()?;
    if profiles.is_empty() {
        anstream::println!("No profiles yet. Create one with `{} profile create <name>`.", brand::BIN_NAME);
        return Ok(0);
    }
    let active = resolve_here(ctx, &store, None, None)?.profile.map(|p| p.id);
    let default = store.default_profile()?.map(|p| p.id);
    let bindings = store.bindings()?;
    let mut rows = vec![vec![
        String::new(),
        ui::dim("PROFILE"),
        ui::dim("IDENTITIES"),
        ui::dim("BINDINGS"),
        ui::dim("DESCRIPTION"),
    ]];
    for p in profiles {
        let marker = if Some(p.id) == active { ui::accent("●") } else { " ".into() };
        let mut name = if Some(p.id) == active { ui::accent(&p.name) } else { p.name.clone() };
        if Some(p.id) == default {
            name.push_str(&ui::dim(" (default)"));
        }
        let identities: Vec<String> = store.identities(p.id)?.into_keys().collect();
        let count = bindings.iter().filter(|b| b.profile == p.name).count();
        rows.push(vec![
            marker,
            name,
            if identities.is_empty() { ui::dim("-") } else { identities.join(", ") },
            if count == 0 { ui::dim("-") } else { count.to_string() },
            p.description,
        ]);
    }
    ui::table(&rows);
    Ok(0)
}

fn show(ctx: &Ctx, name: Option<&str>) -> Result<i32> {
    let store = ctx.store()?;
    let profile = match name {
        Some(n) => store.require_profile(n)?,
        None => match resolve_here(ctx, &store, None, None)?.profile {
            Some(p) => p,
            None => bail!("no profile is active here; name one: `{} profile show <name>`", brand::BIN_NAME),
        },
    };
    anstream::println!("{}", ui::paint(&profile.name, ui::ACCENT));
    if !profile.description.is_empty() {
        anstream::println!("{}", profile.description);
    }
    let is_default = store.default_profile()?.is_some_and(|d| d.id == profile.id);
    ui::hint(format!(
        "created {}, updated {}{}",
        ui::fmt_time(profile.created_at),
        ui::fmt_time(profile.updated_at),
        if is_default { ", default profile" } else { "" }
    ));

    anstream::println!();
    anstream::println!("{}", ui::paint("Identities", ui::BOLD));
    let identities = store.identities(profile.id)?;
    if identities.is_empty() {
        ui::hint("none");
    }
    let accounts = ctx.paths.accounts_dir();
    let pctx = ProviderContext { profile: &profile.name, accounts_dir: &accounts };
    for (id, settings) in &identities {
        let summary =
            providers::find(id).map(|p| p.summary(settings, &pctx)).unwrap_or_else(|| "(unknown provider)".into());
        anstream::println!("  {id:<10} {summary}");
    }

    anstream::println!();
    anstream::println!("{}", ui::paint("Environment", ui::BOLD));
    let env = store.env_vars(profile.id)?;
    if env.is_empty() {
        ui::hint("none");
    }
    for (k, v) in &env {
        anstream::println!("  {k}={v}");
    }
    for k in store.secret_keys(profile.id)? {
        anstream::println!("  {k}={}", ui::paint("•••••• (secret)", ui::WARN));
    }

    anstream::println!();
    anstream::println!("{}", ui::paint("Directory bindings", ui::BOLD));
    let bindings: Vec<_> = store.bindings()?.into_iter().filter(|b| b.profile == profile.name).collect();
    if bindings.is_empty() {
        ui::hint("none");
    }
    for b in bindings {
        anstream::println!("  {}", b.path);
    }
    Ok(0)
}

fn use_profile(ctx: &Ctx, name: Option<&str>, clear: bool, emit: Option<ShellArg>) -> Result<i32> {
    let store = ctx.store()?;
    let canonical = match name {
        Some(n) if !clear => Some(store.require_profile(n)?.name),
        _ => None,
    };
    let Some(shell) = emit else {
        // Not called through the shell function: we cannot change the
        // parent shell's environment, so explain how to.
        let target = canonical.as_deref().unwrap_or("<name>");
        ui::warn("shell integration is not loaded in this terminal, so its environment cannot be changed from here");
        anstream::println!("  Load it for this session:");
        anstream::println!("    PowerShell:  Invoke-Expression ((& {} hook powershell) -join \"`n\")", brand::BIN_NAME);
        anstream::println!("    bash / zsh:  eval \"$({} hook bash)\"   (or: hook zsh)", brand::BIN_NAME);
        anstream::println!("  Install it permanently:  {} shell install <shell>", brand::BIN_NAME);
        anstream::println!("  Or run a single command: {} run --profile {target} -- <command>", brand::BIN_NAME);
        return Ok(2);
    };
    let shell: Shell = shell.into();
    let plan = match &canonical {
        Some(n) => EnvPlan { set: vec![(brand::env::PROFILE.to_string(), n.clone())], unset: vec![] },
        None => EnvPlan { set: vec![], unset: vec![brand::env::PROFILE.to_string()] },
    };
    print!("{}", shells::emit(shell.syntax(), &plan));
    match &canonical {
        Some(n) => anstream::eprintln!("{} session profile set to {}", brand::BIN_NAME, ui::accent(n)),
        None => anstream::eprintln!("{} session override cleared", brand::BIN_NAME),
    }
    Ok(0)
}

fn why(ctx: &Ctx, path: Option<&Path>) -> Result<i32> {
    let store = ctx.store()?;
    let dir = match path {
        Some(p) => pathkey::canonical_dir(p)?,
        None => std::env::current_dir()?,
    };
    let r = resolve_here(ctx, &store, None, Some(&dir))?;
    match &r.profile {
        Some(p) => anstream::println!("{} is active because of the {}", ui::accent(&p.name), r.source.describe()),
        None => anstream::println!("No profile is active in {}", dir.display()),
    }
    if let Some(e) = &r.error {
        ui::warn(e);
    }
    anstream::println!();
    for (i, step) in r.steps.iter().enumerate() {
        let tag = match step.outcome {
            StepOutcome::Selected => ui::paint("selected", ui::OK),
            StepOutcome::Shadowed => ui::dim("overridden"),
            StepOutcome::NotSet => ui::dim("-"),
            StepOutcome::Missing => ui::paint("missing profile", ui::ERR),
        };
        anstream::println!("  {}. {:<18} {:<12} {}", i + 1, step.level, tag_pad(&tag), step.detail);
    }
    Ok(0)
}

fn tag_pad(tag: &str) -> String {
    let pad = 12usize.saturating_sub(ui::visible_width(tag));
    format!("{tag}{}", " ".repeat(pad))
}

fn edit(ctx: &Ctx, name: &str, description: Option<&str>, rename: Option<&str>) -> Result<i32> {
    if description.is_none() && rename.is_none() {
        bail!("nothing to change; pass --description and/or --rename");
    }
    let mut store = ctx.store()?;
    let mut current = store.require_profile(name)?.name;
    if let Some(d) = description {
        store.set_description(&current, d)?;
        ui::success(format!("updated description of {}", ui::accent(&current)));
    }
    if let Some(new) = rename {
        let p = store.rename_profile(&current, new)?;
        ui::success(format!("renamed {} to {}", current, ui::accent(&p.name)));
        ui::hint("terminals that selected the old name with `profile use` must select it again");
        current = p.name;
    }
    let _ = current;
    Ok(0)
}

fn delete(ctx: &Ctx, name: &str, force: bool) -> Result<i32> {
    let mut store = ctx.store()?;
    let p = store.require_profile(name)?;
    let bound: Vec<_> = store.bindings()?.into_iter().filter(|b| b.profile == p.name).collect();
    if !bound.is_empty() && !force {
        anstream::println!("{} is bound to:", p.name);
        for b in &bound {
            anstream::println!("  {}", b.path);
        }
        if !ui::confirm("Delete the profile and these bindings?", false) {
            bail!("not deleted (use --force to delete without asking)");
        }
    }
    let secret_keys = store.secret_keys(p.id)?;
    let removed = store.delete_profile(&p.name)?;
    super::secret::purge_profile_secrets(vault::open().as_ref(), p.id, &secret_keys);
    ui::success(format!("deleted profile {}", p.name));
    if removed > 0 {
        ui::hint(format!("removed {removed} directory binding(s)"));
    }
    Ok(0)
}

fn default(ctx: &Ctx, name: Option<&str>, clear: bool) -> Result<i32> {
    let mut store = ctx.store()?;
    if clear {
        store.set_default_profile(None)?;
        ui::success("default profile cleared");
    } else if let Some(n) = name {
        store.set_default_profile(Some(n))?;
        let p = store.require_profile(n)?;
        ui::success(format!("default profile is now {}", ui::accent(&p.name)));
    } else {
        match store.default_profile()? {
            Some(p) => anstream::println!("{}", ui::accent(&p.name)),
            None => anstream::println!("No default profile is set."),
        }
    }
    Ok(0)
}

pub fn check_not_secret(key: &str, value: &str, allowed: bool) -> Result<()> {
    if allowed {
        return Ok(());
    }
    if secrets::looks_like_secret_key(key) || secrets::looks_like_secret_value(value) {
        bail!(
            "'{key}' looks like it holds a secret. {} stores profile values in plain text, so secrets do not \
             belong here — keep credentials in the tool's own login/credential store and reference them \
             (e.g. AWS_PROFILE instead of AWS_SECRET_ACCESS_KEY). If this value is not secret, pass --not-secret.",
            brand::APP_NAME
        );
    }
    Ok(())
}

fn env(ctx: &Ctx, cmd: ProfileEnvCommand) -> Result<i32> {
    let mut store = ctx.store()?;
    match cmd {
        ProfileEnvCommand::Set { profile, key, value, not_secret } => {
            names::validate_env_key(&key)?;
            check_not_secret(&key, &value, not_secret)?;
            let p = store.require_profile(&profile)?;
            let was_secret = store.is_secret(p.id, &key)?;
            store.set_env(&p.name, &key, &value)?;
            if was_secret {
                vault::open().delete(&vault::profile_secret_id(p.id, &key))?;
            }
            ui::success(format!("{} sets {key}", ui::accent(&p.name)));
        }
        ProfileEnvCommand::Unset { profile, key } => {
            let p = store.require_profile(&profile)?;
            let was_secret = store.is_secret(p.id, &key)?;
            if store.unset_env(&p.name, &key)? {
                if was_secret {
                    vault::open().delete(&vault::profile_secret_id(p.id, &key))?;
                }
                ui::success(format!("{} no longer sets {key}", ui::accent(&p.name)));
            } else {
                ui::warn(format!("{} did not set {key}", p.name));
            }
        }
        ProfileEnvCommand::List { profile } => {
            let p = store.require_profile(&profile)?;
            for (k, v) in store.env_vars(p.id)? {
                anstream::println!("{k}={v}");
            }
        }
    }
    Ok(0)
}

fn bind(ctx: &Ctx, path: &Path, profile: &str) -> Result<i32> {
    let mut store = ctx.store()?;
    let p = store.require_profile(profile)?;
    let dir = pathkey::canonical_dir(path)?;
    let key = pathkey::path_key(&dir)?;
    let display = dir.to_string_lossy().to_string();
    let previous = store.bind(&display, &key, &p.name)?;
    match previous {
        Some(prev) if prev != p.name => {
            ui::success(format!("{display} is now bound to {} (was {prev})", ui::accent(&p.name)))
        }
        _ => ui::success(format!("{display} is bound to {}", ui::accent(&p.name))),
    }
    ui::hint("applies to this directory and everything below it, unless a deeper binding exists");
    Ok(0)
}

fn unbind(ctx: &Ctx, path: Option<&Path>) -> Result<i32> {
    let mut store = ctx.store()?;
    let dir = pathkey::canonical_dir(path.unwrap_or(Path::new(".")))?;
    let key = pathkey::path_key(&dir)?;
    match store.unbind(&key)? {
        Some(prev) => {
            ui::success(format!("removed binding {} -> {prev}", dir.display()));
            Ok(0)
        }
        None => {
            ui::warn(format!("{} has no binding of its own", dir.display()));
            if let Some(b) = rideshell_core::resolve::find_binding(&store.bindings()?, &dir) {
                ui::hint(format!("it inherits {} from {}", b.profile, b.path));
            }
            Ok(1)
        }
    }
}

fn bindings(ctx: &Ctx) -> Result<i32> {
    let store = ctx.store()?;
    let bindings = store.bindings()?;
    if bindings.is_empty() {
        anstream::println!("No directory bindings. Add one with `{} profile bind <dir> <profile>`.", brand::BIN_NAME);
        return Ok(0);
    }
    let mut rows = vec![vec![ui::dim("DIRECTORY"), ui::dim("PROFILE"), ui::dim("")]];
    for b in bindings {
        let missing =
            if Path::new(&b.path).is_dir() { String::new() } else { ui::paint("(directory missing)", ui::WARN) };
        rows.push(vec![b.path, ui::accent(&b.profile), missing]);
    }
    ui::table(&rows);
    Ok(0)
}

fn export(ctx: &Ctx, name: &str, output: Option<&Path>) -> Result<i32> {
    let store = ctx.store()?;
    let text = ProfileFile::export(&store, name)?.to_toml()?;
    match output {
        Some(path) => {
            if path.exists() && !ui::confirm(&format!("{} exists. Overwrite?", path.display()), false) {
                bail!("not overwritten");
            }
            std::fs::write(path, text).with_context(|| format!("cannot write {}", path.display()))?;
            ui::success(format!("exported {name} to {}", path.display()));
        }
        None => anstream::print!("{text}"),
    }
    Ok(0)
}

fn import(ctx: &Ctx, file: &Path, name: Option<&str>, replace: bool, not_secret: bool) -> Result<i32> {
    let text = std::fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?;
    let pf = ProfileFile::parse(&text)?;
    let target = name.unwrap_or(&pf.profile.name).to_string();
    names::validate_profile_name(&target)?;

    // Validate everything before writing anything.
    for (k, v) in &pf.env {
        names::validate_env_key(k)?;
        check_not_secret(k, v, not_secret)?;
    }
    let mut identities = std::collections::BTreeMap::new();
    for (provider_id, settings) in &pf.identities {
        let provider = providers::find(provider_id).with_context(|| {
            format!("the file uses identity provider '{provider_id}', which this version does not support")
        })?;
        if let Some(k) = providers::unknown_keys(provider, settings).first() {
            bail!("unknown {provider_id} setting '{k}'");
        }
        for (k, v) in settings {
            check_not_secret(k, v, not_secret)?;
        }
        for d in provider.validate(settings) {
            ui::warn(format!("{provider_id}: {}", d.message));
        }
        identities.insert(provider.id().to_string(), settings.clone());
    }

    let mut store = ctx.store()?;
    if store.get_profile(&target)?.is_some() && !replace {
        bail!("profile '{target}' already exists (use --replace or --name)");
    }
    let p = store.import_profile(&target, &pf.profile.description, &pf.env, &identities, replace)?;
    ui::success(format!("imported profile {}", ui::accent(&p.name)));
    Ok(0)
}
