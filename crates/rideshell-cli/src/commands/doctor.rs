use std::path::Path;

use anyhow::Result;
use rideshell_core::brand;
use rideshell_core::config::Config;
use rideshell_core::providers::{self, Severity};
use rideshell_core::store::Store;

use super::{shell, Ctx};
use crate::ui;

#[derive(Default)]
struct Report {
    errors: usize,
    warnings: usize,
}

impl Report {
    fn ok(&self, msg: impl AsRef<str>) {
        ui::success(msg);
    }
    fn warn(&mut self, msg: impl AsRef<str>) {
        self.warnings += 1;
        anstream::println!("{} {}", ui::paint("!", ui::WARN), msg.as_ref());
    }
    fn fail(&mut self, msg: impl AsRef<str>) {
        self.errors += 1;
        anstream::println!("{} {}", ui::paint("x", ui::ERR), msg.as_ref());
    }
}

pub fn run(ctx: &Ctx) -> Result<i32> {
    let mut r = Report::default();
    anstream::println!("{} doctor {}", ui::accent(brand::APP_NAME), ui::dim(env!("CARGO_PKG_VERSION")));
    anstream::println!();

    // Configuration.
    let config = Config::load(&ctx.paths);
    match &config {
        Ok(_) if ctx.paths.config_file().exists() => r.ok(format!("config {}", ctx.paths.config_file().display())),
        Ok(_) => r.ok(format!("config: using defaults ({} not created yet)", ctx.paths.config_file().display())),
        Err(e) => r.fail(format!("{e:#}")),
    }

    // Database.
    let db = ctx.paths.db_file();
    let store = if db.exists() {
        match Store::open(&db) {
            Ok(s) => {
                match s.quick_check() {
                    Ok(c) if c == "ok" => {
                        r.ok(format!("database {} (schema v{})", db.display(), s.schema_version().unwrap_or(0)))
                    }
                    Ok(c) => r.fail(format!("database integrity check failed: {c}")),
                    Err(e) => r.fail(format!("database check failed: {e:#}")),
                }
                Some(s)
            }
            Err(e) => {
                r.fail(format!("{e:#}"));
                None
            }
        }
    } else {
        r.warn(format!("database not created yet; run `{} init`", brand::BIN_NAME));
        None
    };

    // Profiles, identities, bindings.
    if let Some(store) = &store {
        let profiles = store.list_profiles()?;
        if profiles.is_empty() {
            r.warn("no profiles");
        } else {
            r.ok(format!("{} profile(s)", profiles.len()));
        }
        match store.default_profile()? {
            Some(p) => r.ok(format!("default profile {}", p.name)),
            None if !profiles.is_empty() => r.warn("no default profile"),
            None => {}
        }
        for p in &profiles {
            for (id, settings) in store.identities(p.id)? {
                let Some(provider) = providers::find(&id) else {
                    r.warn(format!("{}: unknown provider '{id}'", p.name));
                    continue;
                };
                for d in provider.validate(&settings) {
                    match d.severity {
                        Severity::Error => r.fail(format!("{} / {id}: {}", p.name, d.message)),
                        Severity::Warning => r.warn(format!("{} / {id}: {}", p.name, d.message)),
                    }
                }
            }
        }
        for b in store.bindings()? {
            if !Path::new(&b.path).is_dir() {
                r.warn(format!("binding {} -> {}: directory no longer exists", b.path, b.profile));
            }
        }
    }

    // Installation, shims, secrets.
    match super::install::installed_exe() {
        Ok(exe) if super::install::is_installed() => r.ok(format!("installed at {}", exe.display())),
        Ok(_) => ui::hint(format!("not installed for all terminals (run `{} install`)", brand::BIN_NAME)),
        Err(e) => r.warn(format!("{e:#}")),
    }
    #[cfg(windows)]
    {
        let shims_dir = ctx.paths.shims_dir();
        let in_user_path = super::install::windows::user_path_entries()
            .iter()
            .any(|e| e.trim_end_matches('\\').eq_ignore_ascii_case(shims_dir.to_string_lossy().trim_end_matches('\\')));
        if super::install::is_installed() && !in_user_path {
            r.warn("the shims directory is missing from your user PATH; run `rideshell install` again");
        }
        if super::install::is_installed() && !super::install::windows::is_registered() {
            r.warn("not registered in Apps & Features; run `rideshell install` again");
        }
        if super::install::is_installed() && !super::install::windows::launchers_present() {
            r.warn("Start menu entries are missing; run `rideshell install` again");
        }
        if super::install::installed_app().is_some() && !super::install::windows::folder_menu_registered() {
            r.warn("\"Open in RideShell\" is missing from Explorer's folder menu; run `rideshell install` again");
        }
    }
    let shim_names = rideshell_core::shims::list(&ctx.paths.shims_dir());
    if !shim_names.is_empty() {
        if rideshell_core::shims::on_path(&ctx.paths.shims_dir()) {
            r.ok(format!("shims active in this terminal: {}", shim_names.join(", ")));
        } else {
            r.warn(format!(
                "shims ({}) are not on PATH in this terminal; open a new terminal after `{} install`",
                shim_names.join(", "),
                brand::BIN_NAME
            ));
        }
        for n in &shim_names {
            if rideshell_core::shims::find_real(n, Some(&ctx.paths.shims_dir())).is_err() {
                r.warn(format!("shim `{n}`: the real {n} is not installed"));
            }
        }
    }
    if let Ok(config) = &config {
        guard_coverage(config, &shim_names, &ctx.paths.shims_dir());
    }
    {
        let v = rideshell_core::vault::open();
        let probe = format!("doctor-probe/{}", std::process::id());
        let ok = v.set(&probe, "probe").is_ok() && v.get(&probe).ok().flatten().as_deref() == Some("probe");
        let _ = v.delete(&probe);
        if ok {
            r.ok(format!("secret storage: {}", v.backend()));
        } else {
            r.fail(format!("secret storage ({}) is not usable", v.backend()));
        }
    }

    // Shells.
    for s in shell::states() {
        match (&s.path, s.installed) {
            (Some(_), true) => r.ok(format!("{} integration installed", s.shell.display_name())),
            (Some(_), false) => ui::hint(format!(
                "{} found, integration not installed (`{} shell install {}`)",
                s.shell.display_name(),
                brand::BIN_NAME,
                s.shell.id()
            )),
            (None, true) => {
                r.warn(format!("{} integration installed but the shell was not found", s.shell.display_name()))
            }
            (None, false) => {}
        }
    }
    match std::env::var(brand::env::SHELL) {
        Ok(s) => r.ok(format!("integration active in this terminal ({s})")),
        Err(_) => ui::hint("integration not active in this terminal"),
    }

    // Tools.
    // Installed catalogue tools (a PATH lookup each; nothing is run).
    let shims_dir = ctx.paths.shims_dir();
    let installed: Vec<&str> = providers::registry()
        .iter()
        .filter(|p| !p.tool().is_empty() && rideshell_core::shims::find_real(p.tool(), Some(&shims_dir)).is_ok())
        .map(|p| p.display_name())
        .collect();
    if installed.is_empty() {
        ui::hint("no catalogue tools found on PATH");
    } else {
        r.ok(format!("tools found: {}", installed.join(", ")));
    }

    anstream::println!();
    if r.errors == 0 && r.warnings == 0 {
        ui::success("no problems found");
    } else {
        anstream::println!("{} error(s), {} warning(s)", r.errors, r.warnings);
    }
    Ok(if r.errors > 0 { 1 } else { 0 })
}

/// Which guarded tools are protected. The guard runs inside the shims, so a
/// tool that isn't routed through RideShell is never checked.
fn guard_coverage(config: &Config, shim_names: &[String], shims_dir: &Path) {
    if !config.guard.enabled {
        ui::hint("wrong-account guard is off (`[guard] enabled = false`)");
        return;
    }
    let mut tools: Vec<String> = Vec::new();
    for t in config.guard.commands.iter().filter_map(|c| c.split_whitespace().next()) {
        let t = t.to_ascii_lowercase();
        if !tools.contains(&t) {
            tools.push(t);
        }
    }
    let shimmed = |t: &str| shim_names.iter().any(|n| n.eq_ignore_ascii_case(t));
    let protected: Vec<&str> = tools.iter().map(String::as_str).filter(|t| shimmed(t)).collect();
    let unprotected: Vec<&str> = tools
        .iter()
        .map(String::as_str)
        .filter(|t| !shimmed(t) && rideshell_core::shims::find_real(t, Some(shims_dir)).is_ok())
        .collect();
    if protected.is_empty() {
        ui::hint("wrong-account guard: no guarded tool is routed through RideShell yet");
    } else {
        ui::success(format!("wrong-account guard protects: {}", protected.join(", ")));
    }
    if !unprotected.is_empty() {
        ui::hint(format!(
            "not guarded (installed, not routed through {}): {} (`{} shim add <tool>` to guard one)",
            brand::APP_NAME,
            unprotected.join(", "),
            brand::BIN_NAME
        ));
    }
}
