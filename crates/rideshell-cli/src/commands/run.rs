use std::path::Path;

use anyhow::{bail, Result};
use rideshell_core::environment::{self, EnvValue, LayeredEnv};
use rideshell_core::envstate::{self, EnvPlan, EnvState};
use rideshell_core::resolve::Resolution;
use rideshell_core::shells::{self, Shell};
use rideshell_core::store::Store;
use rideshell_core::{exec, vault};

use super::{resolve_here, Ctx};
use crate::cli::{EnvArgs, RunArgs};
use crate::ui;

/// Everything needed to launch a process for the resolved profile.
pub struct Prepared {
    pub resolution: Resolution,
    pub layered: LayeredEnv,
    pub plan: EnvPlan,
}

/// Resolve the profile and plan the child environment, starting from this
/// process's environment (undoing whatever profile the shell has applied).
/// With `with_secrets`, secrets are read from the credential store and
/// per-account directories are created.
pub fn prepare(
    ctx: &Ctx,
    store: &Store,
    flag: Option<&str>,
    cwd: Option<&Path>,
    with_secrets: bool,
) -> Result<Prepared> {
    let resolution = resolve_here(ctx, store, flag, cwd)?;
    if let Some(e) = &resolution.error {
        bail!("{e}");
    }
    let here = std::env::current_dir().ok();
    let cwd = cwd.or(here.as_deref());
    let layered = environment::layered_env(store, &ctx.paths.accounts_dir(), resolution.profile.as_ref(), cwd)?;
    for w in &layered.warnings {
        ui::warn(w);
    }
    let vault = with_secrets.then(vault::open);
    if with_secrets {
        for dir in &layered.required_dirs {
            if let Err(e) = std::fs::create_dir_all(dir) {
                ui::warn(format!("cannot create {}: {e}", dir.display()));
            }
        }
    }
    let (vars, warnings) = environment::materialize(&layered, vault.as_deref());
    for w in &warnings {
        ui::warn(w);
    }
    let name = resolution.profile.as_ref().map(|p| p.name.as_str());
    let (plan, _) = envstate::plan(|k| std::env::var(k).ok(), &EnvState::from_process_env(), &vars, name);
    Ok(Prepared { resolution, layered, plan })
}

pub fn run(ctx: &Ctx, args: RunArgs) -> Result<i32> {
    let dir = match &args.dir {
        Some(d) => Some(dunce::canonicalize(d).map_err(|e| anyhow::anyhow!("cannot open {}: {e}", d.display()))?),
        None => None,
    };
    let store = ctx.store()?;
    let prepared = prepare(ctx, &store, args.profile.as_deref(), dir.as_deref(), true)?;
    let (program, rest) = args.command.split_first().expect("clap requires a command");
    if prepared.resolution.profile.is_none() {
        ui::warn("no profile selected; running with the current environment (minus any applied profile)");
    }
    drop(store);
    // Look past shims so `run -- claude` does not go through RideShell twice.
    let real = rideshell_core::shims::find_real(program, Some(&ctx.paths.shims_dir()))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| program.clone());
    if args.pause {
        // An Administrator window opened for one command: keep it readable.
        let code = exec::run(&real, rest, &prepared.plan, dir.as_deref()).unwrap_or_else(|e| {
            ui::error(format!("{e:#}"));
            1
        });
        anstream::eprintln!();
        anstream::eprint!("{}", ui::dim(format!("Exit code {code}. Press Enter to close.")));
        let mut s = String::new();
        let _ = std::io::stdin().read_line(&mut s);
        return Ok(code);
    }
    exec::run(&real, rest, &prepared.plan, dir.as_deref())
}

pub fn env(ctx: &Ctx, args: EnvArgs) -> Result<i32> {
    let store = ctx.store()?;
    let prepared = prepare(ctx, &store, args.profile.as_deref(), None, false)?;
    if let Some(shell) = args.shell {
        // Shell code never contains secrets: eval'ing it into an interactive
        // shell would expose them to everything that shell runs.
        let shell: Shell = shell.into();
        print!("{}", shells::emit(shell.syntax(), &prepared.plan));
        if prepared.layered.has_secrets() {
            ui::warn("secrets are not included in shell code; use `rideshell run` or a tool shim");
        }
        return Ok(0);
    }
    match &prepared.resolution.profile {
        Some(p) => anstream::println!("{} ({})", ui::accent(&p.name), prepared.resolution.source.describe()),
        None => anstream::println!("No profile selected."),
    }
    let mut rows = Vec::new();
    for (k, entry) in &prepared.layered.vars {
        let value = match &entry.value {
            EnvValue::Plain(v) => v.clone(),
            EnvValue::Secret { .. } => ui::paint("•••••• (secret)", ui::WARN),
        };
        rows.push(vec![format!("  {k}"), value, ui::dim(&entry.origin)]);
    }
    if rows.is_empty() {
        ui::hint("nothing is applied here");
    } else {
        ui::table(&rows);
    }
    Ok(0)
}
