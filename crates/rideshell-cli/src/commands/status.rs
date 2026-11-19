use anyhow::Result;
use rideshell_core::brand;
use rideshell_core::envstate::EnvState;
use rideshell_core::providers;

use super::{resolve_here, session_profile, Ctx};
use crate::ui;

pub fn run(ctx: &Ctx) -> Result<i32> {
    let store = ctx.store()?;
    let resolution = resolve_here(ctx, &store, None, None)?;
    let mut rows: Vec<(String, String)> = Vec::new();

    match &resolution.profile {
        Some(p) => {
            rows.push(("Profile".into(), ui::accent(&p.name)));
            rows.push(("Source".into(), resolution.source.describe()));
            for (id, settings) in store.identities(p.id)? {
                let accounts = ctx.paths.accounts_dir();
                let pctx = providers::ProviderContext { profile: &p.name, accounts_dir: &accounts };
                let summary = providers::find(&id).map(|pr| pr.summary(&settings, &pctx)).unwrap_or_default();
                let label = providers::find(&id).map(|pr| pr.display_name().to_string()).unwrap_or(id);
                rows.push((label, summary));
            }
            let env = store.env_vars(p.id)?;
            if !env.is_empty() {
                rows.push(("Env".into(), env.keys().cloned().collect::<Vec<_>>().join(", ")));
            }
            let secrets = store.secret_keys(p.id)?;
            if !secrets.is_empty() {
                rows.push(("Secrets".into(), secrets.join(", ")));
            }
        }
        None => rows.push(("Profile".into(), ui::dim("none"))),
    }
    if let Ok(cwd) = std::env::current_dir() {
        rows.push(("Directory".into(), cwd.display().to_string()));
    }
    rows.push((
        "Session".into(),
        session_profile().map_or_else(|| ui::dim("no override"), |s| format!("{}={s}", brand::env::PROFILE)),
    ));
    let shell = std::env::var(brand::env::SHELL).ok();
    rows.push(("Shell".into(), shell.clone().unwrap_or_else(|| ui::dim("integration not loaded"))));

    ui::panel(brand::BIN_NAME, &rows);

    if let Some(e) = &resolution.error {
        ui::warn(e);
    }
    let applied = EnvState::from_process_env().profile;
    let resolved = resolution.profile.as_ref().map(|p| p.name.clone());
    if shell.is_some() && applied != resolved {
        ui::hint(format!(
            "this shell currently has {} applied; it updates at the next prompt",
            applied.as_deref().unwrap_or("no profile")
        ));
    } else if shell.is_none() {
        ui::hint(format!(
            "commands in this terminal use their normal identity; use `{} run -- <cmd>` or install shell integration",
            brand::BIN_NAME
        ));
    }
    Ok(0)
}
