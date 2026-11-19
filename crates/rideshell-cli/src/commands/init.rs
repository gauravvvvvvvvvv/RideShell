use anyhow::Result;
use rideshell_core::brand;
use rideshell_core::config::Config;
use rideshell_core::providers;
use rideshell_core::shells::Shell;

use super::{shell, Ctx};
use crate::cli::InitArgs;
use crate::ui;

pub fn run(ctx: &Ctx, args: InitArgs) -> Result<i32> {
    anstream::println!("{} {}", ui::accent(brand::APP_NAME), ui::dim(env!("CARGO_PKG_VERSION")));
    anstream::println!();

    // 1. Platform.
    anstream::println!("System      {} ({})", std::env::consts::OS, std::env::consts::ARCH);

    // 2. Local state.
    ctx.paths.ensure()?;
    let created = Config::write_default_if_missing(&ctx.paths)?;
    anstream::println!(
        "Config      {}{}",
        ctx.paths.config_file().display(),
        if created { ui::dim(" (created)") } else { String::new() }
    );
    let mut store = ctx.store()?;
    anstream::println!("Database    {}", ctx.paths.db_file().display());

    // 3. Shells.
    let states = shell::states();
    let found: Vec<String> = states
        .iter()
        .filter(|s| s.path.is_some())
        .map(|s| format!("{}{}", s.shell.id(), if s.installed { ui::dim(" (integrated)") } else { String::new() }))
        .collect();
    anstream::println!(
        "Shells      {}",
        if found.is_empty() { ui::dim("none supported found") } else { found.join(", ") }
    );

    // 4. Tools.
    let tools: Vec<String> = providers::registry()
        .iter()
        .filter(|p| !p.tool().is_empty() && rideshell_core::shims::find_real(p.tool(), None).is_ok())
        .map(|p| p.id().to_string())
        .collect();
    anstream::println!("Adapters    {}", tools.join(", "));
    anstream::println!();

    // 5. Default profile.
    if store.list_profiles()?.is_empty() {
        let name = if args.yes { args.profile.clone() } else { ui::ask("Name of your first profile?", &args.profile) };
        let p = store.create_profile(&name, "")?;
        store.set_default_profile(Some(&p.name))?;
        ui::success(format!("created default profile {}", ui::accent(&p.name)));
        // The first profile keeps the logins the tools already have.
        let kept = rideshell_core::providers::attach_default_accounts(&mut store, &p.name)?;
        if !kept.is_empty() {
            ui::hint(format!("{} uses your existing {} logins; new profiles get their own", p.name, kept.join(", ")));
        }
    } else if let Some(d) = store.default_profile()? {
        ui::success(format!("default profile is {}", ui::accent(&d.name)));
    } else {
        ui::warn(format!("no default profile; set one with `{} profile default <name>`", brand::BIN_NAME));
    }
    drop(store);

    // 6. Shell integration — only with explicit approval.
    if !args.no_shell {
        let explicit: Vec<Shell> = args.install_shell.iter().map(|s| (*s).into()).collect();
        for state in states.iter().filter(|s| s.path.is_some() && !s.installed) {
            anstream::println!();
            if explicit.contains(&state.shell) {
                shell::install_shell(state.shell, true, false)?;
            } else if ui::stdin_is_interactive() && !args.yes {
                if ui::confirm(&format!("Set up {} integration?", state.shell.display_name()), false) {
                    shell::install_shell(state.shell, false, false)?;
                }
            } else {
                ui::hint(format!(
                    "{} integration not installed; run `{} shell install {}` to review and add it",
                    state.shell.display_name(),
                    brand::BIN_NAME,
                    state.shell.id()
                ));
            }
        }
    }

    // 7. Verify.
    anstream::println!();
    let store = ctx.store()?;
    let check = store.quick_check()?;
    if check == "ok" {
        ui::success("installation verified");
    } else {
        ui::warn(format!("database check reported: {check}"));
    }
    anstream::println!();
    anstream::println!("Next steps:");
    ui::hint(format!("{} profile create work", brand::BIN_NAME));
    ui::hint(format!("{} identity set work git name=\"Your Name\" email=you@company.example", brand::BIN_NAME));
    ui::hint(format!("{} profile bind ~/projects/company work", brand::BIN_NAME));
    ui::hint(format!("{} run --profile work -- git var GIT_AUTHOR_IDENT", brand::BIN_NAME));
    Ok(0)
}
