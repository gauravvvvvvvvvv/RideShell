use anyhow::{bail, Context, Result};
use rideshell_core::brand;

use super::{shell, Ctx};
use crate::cli::UninstallArgs;
use crate::ui;

/// Remove shell integration blocks (and with --purge, RideShell's own data).
/// Never touches provider credentials, git config or anything else.
pub fn run(ctx: &Ctx, args: UninstallArgs) -> Result<i32> {
    if args.program {
        anstream::println!("This removes {} (program, PATH entries, tool shims, shell integration).", brand::APP_NAME);
        anstream::println!("Your tools and their logins are not touched.");
        if !args.yes && !ui::confirm("Uninstall?", true) {
            anstream::println!("Nothing changed.");
            return Ok(1);
        }
    }
    let installed: Vec<_> = if args.keep_shell_integration {
        Vec::new()
    } else {
        shell::states().into_iter().filter(|s| s.installed).collect()
    };
    if installed.is_empty() && !args.keep_shell_integration {
        ui::success("no shell integration installed");
    }
    for s in installed {
        shell::uninstall_shell(s.shell, args.yes || args.program)?;
    }

    if args.program {
        super::install::uninstall_program(ctx)?;
    }

    let purge = args.purge
        || (args.program
            && !args.yes
            && ui::confirm("Also delete your RideShell profiles, settings and per-account folders?", false));
    if purge {
        let dirs = if ctx.paths.config_dir == ctx.paths.data_dir {
            vec![ctx.paths.config_dir.clone()]
        } else {
            vec![ctx.paths.config_dir.clone(), ctx.paths.data_dir.clone()]
        };
        let existing: Vec<_> = dirs.into_iter().filter(|d| d.exists()).collect();
        if existing.is_empty() {
            ui::success("no RideShell data found");
        } else {
            anstream::println!(
                "This permanently deletes RideShell's profiles, bindings, settings, stored secrets and the                  per-account folders it created (signing those accounts out):"
            );
            for d in &existing {
                anstream::println!("  {}", d.display());
            }
            anstream::println!("Your tools' own default logins (e.g. ~/.claude, ~/.codex) are not affected.");
            if !args.yes && !args.program {
                if !ui::stdin_is_interactive() {
                    bail!("refusing to delete data without confirmation (re-run with --yes)");
                }
                if !ui::confirm("Delete?", false) {
                    anstream::println!("Data kept.");
                    return Ok(0);
                }
            }
            purge_secrets(ctx);
            for d in existing {
                std::fs::remove_dir_all(&d).with_context(|| format!("cannot delete {}", d.display()))?;
                ui::success(format!("deleted {}", d.display()));
            }
        }
    }
    if !args.program {
        anstream::println!();
        ui::hint(format!("to remove the program as well: `{} uninstall --program`", brand::BIN_NAME));
    } else if ui::stdin_is_interactive() {
        anstream::println!();
        anstream::eprint!("Press Enter to close.");
        let mut s = String::new();
        let _ = std::io::stdin().read_line(&mut s);
    }
    Ok(0)
}

/// Delete every secret RideShell stored in the OS credential store.
fn purge_secrets(ctx: &Ctx) {
    let Ok(Some(store)) = rideshell_core::store::Store::open_existing(&ctx.paths.db_file()) else { return };
    let v = rideshell_core::vault::open();
    let mut n = 0;
    if let Ok(profiles) = store.list_profiles() {
        for p in profiles {
            for k in store.secret_keys(p.id).unwrap_or_default() {
                n += v.delete(&rideshell_core::vault::profile_secret_id(p.id, &k)).is_ok() as usize;
            }
        }
    }
    for d in store.dir_vars().unwrap_or_default().into_iter().filter(|d| d.secret) {
        n += v.delete(&rideshell_core::vault::dir_secret_id(d.id)).is_ok() as usize;
    }
    if n > 0 {
        ui::success(format!("deleted {n} secret(s) from {}", v.backend()));
    }
}
