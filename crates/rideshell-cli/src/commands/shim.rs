//! Tool shims: management commands and the shim runtime.

use std::ffi::OsString;
use std::io::IsTerminal;

use anyhow::{Context, Result};
use rideshell_core::store::Store;
use rideshell_core::{brand, exec, shims};

use super::{run, Ctx};
use crate::cli::ShimCommand;
use crate::ui;

pub fn run(ctx: &Ctx, cmd: ShimCommand) -> Result<i32> {
    let dir = ctx.paths.shims_dir();
    match cmd {
        ShimCommand::Add { tools } => {
            for t in &tools {
                add(ctx, t)?;
            }
            path_hint(ctx);
        }
        ShimCommand::Remove { tools } => {
            for t in &tools {
                if shims::remove(&dir, t)? {
                    ui::success(format!("{t} is no longer routed through {}", brand::APP_NAME));
                } else {
                    ui::warn(format!("no shim for {t}"));
                }
            }
        }
        ShimCommand::List => {
            let names = shims::list(&dir);
            if names.is_empty() {
                anstream::println!("No shims. Add one with `{} shim add <tool>`.", brand::BIN_NAME);
            }
            let mut rows = vec![vec![ui::dim("TOOL"), ui::dim("REAL PROGRAM")]];
            for n in names {
                let real = match shims::find_real(&n, Some(&dir)) {
                    Ok(p) => p.display().to_string(),
                    Err(_) => ui::paint("not installed", ui::WARN),
                };
                rows.push(vec![n, real]);
            }
            ui::table(&rows);
            path_hint(ctx);
        }
        ShimCommand::Refresh => {
            let exe = current_exe()?;
            let names = shims::refresh(&dir, &exe)?;
            ui::success(format!("refreshed {} shim(s)", names.len()));
        }
    }
    Ok(0)
}

fn current_exe() -> Result<std::path::PathBuf> {
    let exe = std::env::current_exe().context("cannot determine the executable path")?;
    Ok(dunce::canonicalize(&exe).unwrap_or(exe))
}

/// Create a shim (idempotent) and say so.
pub fn add(ctx: &Ctx, tool: &str) -> Result<()> {
    let dir = ctx.paths.shims_dir();
    let existed = shims::list(&dir).iter().any(|n| n.eq_ignore_ascii_case(tool));
    shims::create(&dir, tool, &current_exe()?)?;
    if !existed {
        ui::success(format!("`{tool}` now runs through {} in every terminal", brand::APP_NAME));
    }
    if shims::find_real(tool, Some(&dir)).is_err() {
        ui::warn(format!("{tool} itself is not installed (or not on PATH) yet"));
    }
    Ok(())
}

/// Create a shim without output (used when RideShell sets things up itself).
pub fn add_quiet(ctx: &Ctx, tool: &str) -> Result<()> {
    shims::create(&ctx.paths.shims_dir(), tool, &current_exe()?)?;
    Ok(())
}

pub fn path_hint(ctx: &Ctx) {
    let dir = ctx.paths.shims_dir();
    if !shims::on_path(&dir) {
        ui::hint(format!(
            "shims are not on PATH in this terminal yet: run `{} install` (all terminals) or load shell integration",
            brand::BIN_NAME
        ));
    }
}

/// Entry point when the binary is invoked under a tool's name.
pub fn run_shim(tool: &str, args: Vec<OsString>) -> i32 {
    match run_shim_inner(tool, args) {
        Ok(code) => code,
        Err(e) => {
            ui::error(format!("{} ({tool} shim): {e:#}", brand::BIN_NAME));
            ui::hint(format!("to run {tool} without {}: set {}=1", brand::APP_NAME, shims::BYPASS_ENV));
            1
        }
    }
}

fn run_shim_inner(tool: &str, args: Vec<OsString>) -> Result<i32> {
    // Arguments go to the real tool exactly as received (a file name need not
    // be valid Unicode); the guard only reads a lossy copy.
    let os_args = args;
    let args: Vec<String> = os_args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let ctx = Ctx::new()?;
    let real = shims::find_real(tool, Some(&ctx.paths.shims_dir()))?;
    let real_s = real.to_string_lossy().into_owned();

    let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty() && v != "0");
    let bypass = set(shims::BYPASS_ENV);
    // Profiles apply only where RideShell is in use: a RideShell window, a
    // shell with RideShell's integration loaded, or an explicit profile. In a
    // plain PowerShell / cmd / bash the shim (on the user PATH) runs the real
    // tool untouched, so no account or environment leaks into those shells.
    let in_rideshell =
        [brand::env::SESSION, brand::env::SHELL, brand::env::PROFILE, brand::env::ACTIVE_PROFILE].into_iter().any(set);
    let store = if bypass || !in_rideshell { None } else { Store::open_existing(&ctx.paths.db_file())? };
    let Some(store) = store else {
        return exec::run(&real_s, &os_args, &Default::default(), None);
    };

    // Fails closed: if this terminal selected a profile that no longer
    // exists, `prepare` errors instead of using some other account.
    let mut prepared = run::prepare(&ctx, &store, None, None, true)?;
    run::add_tool_scoped(&ctx, &store, &mut prepared, tool)?;
    let config = ctx.config().unwrap_or_default();
    if !guard_allows(&config, &store, &prepared, tool, &args)? {
        return Ok(1);
    }
    drop(store);

    // git runs constantly; announcing the account each time would be noise.
    if config.shims.notify && std::io::stderr().is_terminal() && !tool.eq_ignore_ascii_case("git") {
        let who = match &prepared.resolution.profile {
            Some(p) => {
                format!("{} {}", ui::accent(&p.name), ui::dim(format!("({})", prepared.resolution.source.describe())))
            }
            None => ui::dim("no profile"),
        };
        anstream::eprintln!("{} {}", ui::dim(format!("{} ▸ {tool} as", brand::BIN_NAME)), who);
    }
    exec::run(&real_s, &os_args, &prepared.plan, None)
}

/// The account-mismatch guard (`rideshell_core::guard`): before a sensitive
/// command, confirm when this terminal's account (chosen with `profile use`)
/// differs from the account the folder is bound to. No terminal to ask in:
/// refuse (set RIDESHELL_GUARD=0 to run it anyway).
fn guard_allows(
    config: &rideshell_core::config::Config,
    store: &Store,
    prepared: &run::Prepared,
    tool: &str,
    args: &[String],
) -> Result<bool> {
    use rideshell_core::{guard, resolve};
    if !config.guard.enabled || guard::disabled_by_env() {
        return Ok(true);
    }
    let Some(command) = guard::matching(&config.guard.commands, tool, args) else { return Ok(true) };
    let Some(active) = &prepared.resolution.profile else { return Ok(true) };
    if !matches!(prepared.resolution.source, resolve::Source::Session) {
        return Ok(true); // the folder's own account (or the default) is in use
    }
    let Ok(cwd) = std::env::current_dir() else { return Ok(true) };
    let Some(bound) = resolve::find_binding(&store.bindings()?, &cwd) else { return Ok(true) };
    if bound.profile.eq_ignore_ascii_case(&active.name) {
        return Ok(true);
    }
    ui::warn(format!(
        "this folder belongs to {}, but this terminal is using {} (switched with `profile use`)",
        ui::accent(&bound.profile),
        ui::accent(&active.name)
    ));
    if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        if ui::confirm(&format!("Run `{command}` as {}?", active.name), false) {
            return Ok(true);
        }
        anstream::eprintln!(
            "Cancelled. Switch with `{} profile use {}`, then try again.",
            brand::BIN_NAME,
            bound.profile
        );
        return Ok(false);
    }
    anstream::eprintln!(
        "Refused `{command}` (no terminal to confirm in). Switch accounts, or set {}=0 to run it anyway.",
        rideshell_core::guard::DISABLE_ENV
    );
    Ok(false)
}

/// `rideshell tools`: every command on PATH, grouped by where it came from.
pub fn tools(ctx: &Ctx, filter: Option<&str>, all: bool, json: bool) -> Result<i32> {
    let dir = ctx.paths.shims_dir();
    let mut list = rideshell_core::tools::scan(Some(&dir), all);
    if let Some(f) = filter {
        let f = f.to_ascii_lowercase();
        list.retain(|t| t.name.to_ascii_lowercase().contains(&f));
    }
    if json {
        println!("{}", serde_json::to_string(&list)?);
        return Ok(0);
    }
    let mut rows = vec![vec![ui::dim("TOOL"), ui::dim("FROM"), ui::dim("ROUTED"), ui::dim("PATH")]];
    for t in &list {
        rows.push(vec![
            t.name.clone(),
            t.source.to_string(),
            if t.shimmed { ui::paint("yes", ui::OK) } else { ui::dim("-") },
            ui::dim(t.path.display().to_string()),
        ]);
    }
    ui::table(&rows);
    ui::hint(format!(
        "{} tools on PATH (usable from every terminal){}. Route one through {}: `{} shim add <tool>`",
        list.len(),
        if all { "" } else { ", Windows' own commands hidden (--all)" },
        brand::APP_NAME,
        brand::BIN_NAME
    ));
    Ok(0)
}

/// `rideshell inspect <tool>`: what the tool's own help offers.
pub fn inspect(ctx: &Ctx, tool: &str, json: bool) -> Result<i32> {
    let exe = shims::find_real(tool, Some(&ctx.paths.shims_dir()))?;
    let d = rideshell_core::discover::inspect(&exe);
    if json {
        println!("{}", serde_json::to_string(&d)?);
        return Ok(0);
    }
    anstream::println!("{} {}", ui::accent(tool), ui::dim(exe.display().to_string()));
    let show = |label: &str, v: &[String]| {
        let text = if v.is_empty() { ui::dim("none found in its help") } else { v.join(", ") };
        anstream::println!("  {label:<16} {text}");
    };
    show("API key vars", &d.key_vars);
    show("config vars", &d.config_vars);
    if !d.help_found {
        ui::hint("the tool printed no help; enter its details yourself");
    }
    ui::hint(format!(
        "connect: {} identity set <profile> custom tool={tool} [folder_var=VAR] [key_vars=VAR,...]",
        brand::BIN_NAME
    ));
    Ok(0)
}
