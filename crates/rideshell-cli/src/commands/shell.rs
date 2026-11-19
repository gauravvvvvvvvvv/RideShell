use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use rideshell_core::brand;
use rideshell_core::shells::install;
use rideshell_core::shells::Shell;

use crate::cli::ShellCommand;
use crate::ui;

pub fn run(cmd: ShellCommand) -> Result<i32> {
    match cmd {
        ShellCommand::Install { shell, yes, dry_run } => install_shell(shell.into(), yes, dry_run).map(|_| 0),
        ShellCommand::Uninstall { shell, yes } => uninstall_shell(shell.into(), yes).map(|_| 0),
        ShellCommand::Status => status(),
    }
}

/// Where a shell is installed, if it is.
pub fn detect(shell: Shell) -> Option<PathBuf> {
    rideshell_core::shells::detect(shell)
}

/// Can startup files refer to the binary by name (it is the one on PATH)?
fn on_path() -> bool {
    let (Ok(found), Ok(me)) = (which::which(brand::BIN_NAME), std::env::current_exe()) else { return false };
    match (dunce::canonicalize(found), dunce::canonicalize(me)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

pub fn planned_block(shell: Shell) -> Result<String> {
    let exe = std::env::current_exe().context("cannot determine the executable path")?;
    let exe = dunce::canonicalize(&exe).unwrap_or(exe);
    Ok(install::block(shell, &exe, on_path()))
}

/// Returns true when the file was changed.
pub fn install_shell(shell: Shell, yes: bool, dry_run: bool) -> Result<bool> {
    let rc = install::rc_file(shell)?;
    let block = planned_block(shell)?;
    let (bom, content) = install::read_rc(&rc)?;
    let updated = install::with_block(&content, &block);
    if updated == content {
        ui::success(format!("{} integration is already installed in {}", shell.display_name(), rc.display()));
        return Ok(false);
    }
    let verb = if install::find_block(&content).is_some() { "update the block in" } else { "append to" };
    anstream::println!("This will {verb} {}:", rc.display());
    anstream::println!();
    for line in block.lines() {
        anstream::println!("  {}", ui::paint(line, ui::OK));
    }
    anstream::println!();
    if !on_path() {
        ui::hint(format!("{} is not on PATH, so the block uses this executable's full path", brand::BIN_NAME));
    }
    if dry_run {
        ui::hint("dry run: nothing was changed");
        return Ok(false);
    }
    if !yes {
        if !ui::stdin_is_interactive() {
            bail!("refusing to edit {} without confirmation (re-run with --yes)", rc.display());
        }
        if !ui::confirm("Apply this change?", false) {
            anstream::println!("Nothing changed.");
            return Ok(false);
        }
    }
    let backup = install::write_rc(&rc, bom, &updated)?;
    ui::success(format!("installed {} integration in {}", shell.display_name(), rc.display()));
    if let Some(b) = backup {
        ui::hint(format!("backup: {}", b.display()));
    }
    if cfg!(windows) && shell == Shell::Bash {
        check_git_bash_profile();
    }
    if matches!(shell, Shell::PowerShell | Shell::Pwsh) {
        ui::hint("if scripts are blocked, PowerShell's execution policy must allow your profile (e.g. RemoteSigned)");
    }
    ui::hint("open a new terminal (or reload the startup file) to activate it");
    Ok(true)
}

/// Git Bash starts login shells, which read ~/.bash_profile. Git for Windows
/// generates one that sources ~/.bashrc, but a custom one may not.
fn check_git_bash_profile() {
    let Some(home) = dirs_home() else { return };
    let profile = home.join(".bash_profile");
    if let Ok(text) = std::fs::read_to_string(&profile) {
        if !text.contains(".bashrc") {
            ui::warn(format!(
                "{} does not source ~/.bashrc; Git Bash login shells will not load the integration",
                profile.display()
            ));
        }
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)
}

pub fn uninstall_shell(shell: Shell, yes: bool) -> Result<bool> {
    let rc = install::rc_file(shell)?;
    let (bom, content) = install::read_rc(&rc)?;
    let Some(updated) = install::without_block(&content) else {
        ui::success(format!("no {} integration found in {}", shell.display_name(), rc.display()));
        return Ok(false);
    };
    anstream::println!("This will remove the {} block from {}.", brand::BIN_NAME, rc.display());
    if !yes {
        if !ui::stdin_is_interactive() {
            bail!("refusing to edit {} without confirmation (re-run with --yes)", rc.display());
        }
        if !ui::confirm("Remove it?", true) {
            anstream::println!("Nothing changed.");
            return Ok(false);
        }
    }
    let backup = install::write_rc(&rc, bom, &updated)?;
    ui::success(format!("removed {} integration from {}", shell.display_name(), rc.display()));
    if let Some(b) = backup {
        ui::hint(format!("backup: {}", b.display()));
    }
    Ok(true)
}

pub struct ShellState {
    pub shell: Shell,
    pub path: Option<PathBuf>,
    pub rc: Option<PathBuf>,
    pub installed: bool,
}

pub fn states() -> Vec<ShellState> {
    Shell::ALL
        .into_iter()
        .map(|shell| {
            let path = detect(shell);
            let rc = if path.is_some() { install::rc_file(shell).ok() } else { None };
            let installed = rc
                .as_ref()
                .and_then(|rc| install::read_rc(rc).ok())
                .is_some_and(|(_, text)| install::find_block(&text).is_some());
            ShellState { shell, path, rc, installed }
        })
        .collect()
}

fn status() -> Result<i32> {
    let mut rows = vec![vec![ui::dim("SHELL"), ui::dim("FOUND"), ui::dim("INTEGRATION"), ui::dim("STARTUP FILE")]];
    for s in states() {
        rows.push(vec![
            s.shell.id().to_string(),
            if s.path.is_some() { ui::paint("yes", ui::OK) } else { ui::dim("no") },
            match (s.path.is_some(), s.installed) {
                (_, true) => ui::paint("installed", ui::OK),
                (true, false) => ui::paint("not installed", ui::WARN),
                (false, false) => ui::dim("-"),
            },
            s.rc.map(|p| p.display().to_string()).unwrap_or_default(),
        ]);
    }
    ui::table(&rows);
    match std::env::var(brand::env::SHELL) {
        Ok(s) => ui::hint(format!("integration is active in this terminal ({s})")),
        Err(_) => ui::hint("integration is not active in this terminal"),
    }
    ui::hint("fish and cmd.exe integrations are planned (see ROADMAP.md)");
    Ok(0)
}
