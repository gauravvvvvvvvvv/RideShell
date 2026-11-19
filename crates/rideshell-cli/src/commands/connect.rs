//! `rideshell connect`: connect any tool to a profile, in plain prompts.
//!
//! RideShell keeps no list of tools: it reads the tool's own help to suggest
//! the variables it mentions, and asks you to confirm. Signing in is the
//! tool's own business: run it in a tab of the profile.

use anyhow::{bail, Result};
use rideshell_core::{discover, providers, shims};

use super::{identity, resolve_here, secret, Ctx};
use crate::cli::Scope;
use crate::ui;

pub fn run(ctx: &Ctx, tool: Option<String>, profile: Option<String>) -> Result<i32> {
    let store = ctx.store()?;
    let p = match &profile {
        Some(n) => store.require_profile(n)?,
        None => match resolve_here(ctx, &store, None, None)?.profile {
            Some(p) => p,
            None => bail!("no profile is active here; pass --profile"),
        },
    };
    drop(store);

    let tool = match tool {
        Some(t) => t,
        None => ui::ask("Tool (command)", "").trim().to_string(),
    };
    if tool.is_empty() {
        bail!("no tool given");
    }
    shims::validate_tool_name(&tool)?;

    // The accounts RideShell is built around connect directly.
    if let Some(builtin) =
        providers::registry().iter().find(|pr| pr.id() != "custom" && pr.tool().eq_ignore_ascii_case(&tool))
    {
        identity::set(ctx, &p.name, builtin.id(), &[])?;
        return Ok(0);
    }

    let found = match shims::find_real(&tool, Some(&ctx.paths.shims_dir())) {
        Ok(exe) => {
            anstream::println!("{}", ui::dim(format!("reading {tool} --help ...")));
            discover::inspect(&exe)
        }
        Err(_) => {
            ui::warn(format!("{tool} is not installed (or not on PATH); enter its details yourself"));
            discover::Discovery::default()
        }
    };
    if !found.key_vars.is_empty() || !found.config_vars.is_empty() {
        anstream::println!("From {tool}'s help:");
        if !found.key_vars.is_empty() {
            anstream::println!("  keys      {}", found.key_vars.join(", "));
        }
        if !found.config_vars.is_empty() {
            anstream::println!("  config    {}", found.config_vars.join(", "));
        }
    }
    anstream::println!("{}", ui::dim("Enter accepts the suggestion, '-' means none."));

    let pick = |answer: String| -> Option<String> {
        let a = answer.trim().to_string();
        (!a.is_empty() && a != "-").then_some(a)
    };
    let keys = pick(ui::ask(
        "API key variables (comma-separated)",
        &if found.key_vars.is_empty() { "-".to_string() } else { found.key_vars.join(",") },
    ));
    let folder = pick(ui::ask("Config folder variable (this account gets its own folder)", "-"));

    let mut pairs = vec![format!("tool={tool}")];
    if let Some(k) = &keys {
        pairs.push(format!(
            "key_vars={}",
            k.split(',').map(str::trim).filter(|k| !k.is_empty()).collect::<Vec<_>>().join(",")
        ));
    }
    if let Some(f) = &folder {
        pairs.push(format!("folder_var={f}"));
    }
    identity::set(ctx, &p.name, "custom", &pairs)?;

    if let Some(k) = &keys {
        if ui::stdin_is_interactive() && ui::confirm("Enter the API key now?", true) {
            for var in k.split(',').map(str::trim).filter(|v| !v.is_empty()) {
                let value = secret::read_secret(var, false)?;
                secret::set(ctx, &Scope { profile: Some(p.name.clone()), dir: None }, var, &value)?;
            }
        }
    }
    Ok(0)
}
