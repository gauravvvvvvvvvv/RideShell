//! Isolated command execution (`rideshell run`).
//!
//! The child inherits the current environment with the [`EnvPlan`] applied.
//! Nothing global changes: the identity exists only in the child process tree.
//!
//! * Unix: the RideShell process is replaced via `exec(2)`, so signals, job
//!   control and exit status behave exactly as if the command ran directly.
//! * Windows: the child is spawned and awaited. RideShell ignores Ctrl+C while
//!   waiting (the child, which shares the console, still receives it) and
//!   exits with the child's exit code.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use crate::envstate::EnvPlan;

/// Resolve `program` on PATH (honouring PATHEXT on Windows, so `npm` finds
/// `npm.cmd`) and run it. Returns the exit code (Windows / non-exec paths).
pub fn run<A: AsRef<OsStr>>(program: &str, args: &[A], plan: &EnvPlan, cwd: Option<&Path>) -> Result<i32> {
    let resolved = which::which(program).with_context(|| format!("command not found: {program}"))?;
    let mut cmd = Command::new(&resolved);
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    plan.apply_to_command(&mut cmd);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        Err(err).with_context(|| format!("failed to execute {}", resolved.display()))
    }

    #[cfg(not(unix))]
    {
        #[cfg(windows)]
        ignore_ctrl_c();
        let status = cmd.status().with_context(|| format!("failed to execute {}", resolved.display()))?;
        Ok(status.code().unwrap_or(1))
    }
}

/// Keep this process alive on Ctrl+C while a child that shares the console
/// handles it.
#[cfg(windows)]
pub fn ignore_ctrl_c() {
    use windows_sys::Win32::Foundation::BOOL;
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

    // A handler routine (unlike `SetConsoleCtrlHandler(NULL, TRUE)`) is not
    // inherited, so the child still reacts to Ctrl+C normally.
    unsafe extern "system" fn handler(_ctrl_type: u32) -> BOOL {
        1
    }
    unsafe {
        SetConsoleCtrlHandler(Some(handler), 1);
    }
}
