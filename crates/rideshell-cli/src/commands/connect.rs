//! `rideshell connect`: connect any tool to a profile, in plain prompts.
//!
//! RideShell keeps no list of tools: it reads the tool's own help to suggest
//! its sign-in command and the variables it mentions, and asks you to confirm.

use anyhow::{bail, Result};
use rideshell_core::{discover, providers, shims};

use super::{identity, resolve_here, secret, Ctx};
use crate::cli::{BrowserArg, Scope};
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
        if builtin.login_args().is_some() && ui::stdin_is_interactive() && ui::confirm("Sign in now?", false) {
            return identity::login_logout(ctx, builtin.id(), Some(&p.name), true, BrowserArg::Default, None);
        }
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
    if !found.sign_in.is_empty() || !found.key_vars.is_empty() || !found.config_vars.is_empty() {
        anstream::println!("From {tool}'s help:");
        if !found.sign_in.is_empty() {
            anstream::println!("  sign-in   {}", found.sign_in.join(", "));
        }
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
    let login = pick(ui::ask(
        "Sign-in command (e.g. login, auth login)",
        found.sign_in.first().map(String::as_str).unwrap_or("-"),
    ));
    let keys = pick(ui::ask(
        "API key variables (comma-separated)",
        &if found.key_vars.is_empty() { "-".to_string() } else { found.key_vars.join(",") },
    ));
    let folder = pick(ui::ask("Config folder variable (this account gets its own folder)", "-"));

    let mut pairs = vec![format!("tool={tool}")];
    if let Some(l) = &login {
        pairs.push(format!("login={l}"));
    }
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
    let id = providers::catalog::custom_id(&tool);

    if let Some(k) = &keys {
        if ui::stdin_is_interactive() && ui::confirm("Enter the API key now?", true) {
            for var in k.split(',').map(str::trim).filter(|v| !v.is_empty()) {
                let value = secret::read_secret(var, false)?;
                secret::set(ctx, &Scope { profile: Some(p.name.clone()), dir: None }, var, &value)?;
            }
        }
    }
    // Only ever on an explicit "yes" in a terminal: never by default.
    if login.is_some() && ui::stdin_is_interactive() && ui::confirm("Sign in now?", false) {
        return identity::login_logout(ctx, &id, Some(&p.name), true, BrowserArg::Default, None);
    }
    Ok(0)
}
