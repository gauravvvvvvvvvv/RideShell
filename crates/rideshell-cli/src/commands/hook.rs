use std::path::PathBuf;

use anyhow::{Context, Result};
use rideshell_core::brand;
use rideshell_core::config::Config;
use rideshell_core::environment;
use rideshell_core::envstate::{self, EnvState};
use rideshell_core::resolve::{self, Request, Source};
use rideshell_core::shells::{self, Shell};
use rideshell_core::store::Store;

use super::{session_profile, Ctx};
use crate::ui;

/// `rideshell hook <shell>`: print the integration script.
pub fn hook(ctx: &Ctx, shell: Shell, session: bool) -> Result<i32> {
    let exe = std::env::current_exe().context("cannot determine the rideshell executable path")?;
    let exe = dunce::canonicalize(&exe).unwrap_or(exe);
    let config = ctx.config().unwrap_or_else(|e| {
        ui::warn(format!("{e:#}; using default configuration"));
        Config::default()
    });
    print!("{}", shells::hook_script(shell, &exe, &ctx.paths.shims_dir(), &config));
    if session && shell.syntax() == shells::Syntax::PowerShell {
        let bash = super::shell::detect(Shell::Bash).is_some();
        print!("{}", shells::session_script(shell, &config, bash));
    }
    Ok(0)
}

/// `rideshell hook-env`: called from the prompt. Must be fast, must never
/// create state, and must never break the prompt: failures print one warning
/// to stderr and emit nothing.
pub fn hook_env(ctx: &Ctx, shell: Shell, cwd: Option<PathBuf>, initial: bool) -> i32 {
    match hook_env_inner(ctx, shell, cwd, initial) {
        Ok(()) => 0,
        Err(e) => {
            ui::warn(format!("{}: {e:#}", brand::BIN_NAME));
            0
        }
    }
}

fn hook_env_inner(ctx: &Ctx, shell: Shell, cwd: Option<PathBuf>, initial: bool) -> Result<()> {
    let old = EnvState::from_process_env();
    let Some(store) = Store::open_existing(&ctx.paths.db_file())? else {
        // Not initialised: nothing to apply, but undo a stale profile if any.
        if !old.is_empty() {
            let (plan, _) = envstate::plan(|k| std::env::var(k).ok(), &old, &Default::default(), None);
            print!("{}", shells::emit(shell.syntax(), &plan));
        }
        return Ok(());
    };
    let config = ctx.config().unwrap_or_default();
    let cwd = cwd.or_else(|| std::env::current_dir().ok());
    let session = session_profile();
    let resolution = resolve::resolve(
        &store,
        &Request {
            flag: None,
            session: session.as_deref(),
            cwd: cwd.as_deref(),
            use_bindings: config.shell.auto_apply,
        },
    )?;

    // Secrets are deliberately not materialised here: they are injected only
    // into processes started through `rideshell run` or a tool shim.
    let layered =
        environment::layered_env(&store, &ctx.paths.accounts_dir(), resolution.profile.as_ref(), cwd.as_deref())?;
    for w in &layered.warnings {
        ui::warn(w);
    }
    let (desired, _) = environment::materialize(&layered, None);
    let name = resolution.profile.as_ref().map(|p| p.name.clone());
    let (plan, new) = envstate::plan(|k| std::env::var(k).ok(), &old, &desired, name.as_deref());
    if !plan.is_empty() {
        print!("{}", shells::emit(shell.syntax(), &plan));
    }

    if let Some(e) = &resolution.error {
        ui::warn(e);
    }
    // Announce changes, except the silent first activation of the default.
    let changed = old.profile != new.profile;
    // The first activation when a shell starts is silent (the prompt shows it).
    let initial_default = old.profile.is_none() && (initial || resolution.source == Source::Default);
    if config.shell.notify_on_switch && changed && !initial_default {
        match &new.profile {
            Some(n) => anstream::eprintln!(
                "{} {} {}",
                ui::dim(format!("{} →", brand::BIN_NAME)),
                ui::accent(n),
                ui::dim(format!("({})", resolution.source.describe()))
            ),
            None => anstream::eprintln!("{}", ui::dim(format!("{} → no profile", brand::BIN_NAME))),
        }
    }
    Ok(())
}
