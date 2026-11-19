mod cli;
mod commands;
mod ui;

use std::path::Path;

use clap::Parser;
use rideshell_core::brand;

fn main() {
    let mut args = std::env::args_os();
    let argv0 = args.next().unwrap_or_default();
    let invoked_as = Path::new(&argv0).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();

    // Invoked under a tool's name (`claude`, `codex`...): act as that tool's shim.
    if !invoked_as.is_empty() && !invoked_as.eq_ignore_ascii_case(brand::BIN_NAME) {
        std::process::exit(commands::shim::run_shim(&invoked_as, args.collect()));
    }

    // Double-clicked in Explorer (or started from the Start menu): the
    // installed copy opens a RideShell window; any other copy installs itself.
    #[cfg(windows)]
    if std::env::args_os().len() == 1 && commands::install::windows::launched_from_explorer() {
        std::process::exit(double_clicked());
    }

    let cli = cli::Cli::parse();
    let code = match commands::dispatch(cli.command) {
        Ok(code) => code,
        Err(e) => {
            ui::error(format!("{e:#}"));
            1
        }
    };
    std::process::exit(code);
}

#[cfg(windows)]
fn double_clicked() -> i32 {
    let ctx = match commands::Ctx::new() {
        Ok(ctx) => ctx,
        Err(e) => {
            ui::error(format!("{e:#}"));
            return pause(1);
        }
    };
    if commands::install::running_installed_copy() {
        if let Some(app) = commands::install::sibling_app() {
            // Typed into Explorer's address bar, the working directory is the
            // open folder: start there. (Start menu: the shortcut's folder.)
            let mut cmd = std::process::Command::new(app);
            if let Ok(dir) = std::env::current_dir() {
                let low = dir.to_string_lossy().to_ascii_lowercase();
                if !low.contains("\\windows\\system32") && !low.contains("\\programs\\rideshell") {
                    cmd.arg("--dir").arg(dir);
                }
            }
            return match cmd.spawn() {
                Ok(_) => 0,
                Err(e) => {
                    ui::error(format!("cannot start RideShell App: {e}"));
                    pause(1)
                }
            };
        }
        return commands::open_window(&ctx).unwrap_or_else(|e| {
            ui::error(format!("{e:#}"));
            pause(1)
        });
    }
    let code = match commands::install::run(&ctx, cli::InstallArgs { yes: false }) {
        Ok(code) => code,
        Err(e) => {
            ui::error(format!("{e:#}"));
            1
        }
    };
    if code != 0 {
        return pause(code);
    }
    anstream::eprintln!();
    anstream::eprint!("Press Enter to open {}.", rideshell_core::brand::APP_NAME);
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    // Start the installed copy (RideShell App if present) so PATH is fresh.
    if let Some(app) = commands::install::installed_app() {
        return std::process::Command::new(app).spawn().map(|_| 0).unwrap_or(1);
    }
    match commands::install::installed_exe() {
        Ok(exe) => std::process::Command::new(exe).arg("open").status().map(|s| s.code().unwrap_or(0)).unwrap_or(1),
        Err(_) => 0,
    }
}

#[cfg(windows)]
fn pause(code: i32) -> i32 {
    anstream::eprintln!();
    anstream::eprint!("Press Enter to close.");
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    code
}
