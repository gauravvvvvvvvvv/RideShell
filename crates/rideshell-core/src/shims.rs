//! Tool shims.
//!
//! A shim is a launcher named after a tool (`claude`, `codex`, `gh`, ...) in
//! RideShell's shims directory, which is placed first on PATH. Typing
//! `claude` in *any* terminal — PowerShell, cmd, Git Bash, VS Code, Windows
//! Terminal — runs the shim, which resolves the profile for that terminal
//! and directory, applies its account/config directory and secrets, and
//! then runs the real tool with all arguments unchanged.
//!
//! * Windows: the shim is a hard link (or copy) of `rideshell.exe` named
//!   `<tool>.exe`; no `.cmd` wrappers, so quoting and Ctrl+C behave normally.
//! * Unix: a symlink to `rideshell` named `<tool>`.
//!
//! The binary detects shim mode from the name it was invoked as. The real
//! tool is found by searching PATH while skipping the shims directory.
//! `RIDESHELL_SHIM_BYPASS=1` makes a shim run the real tool untouched.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

use crate::brand;
use crate::paths::Paths;

pub const BYPASS_ENV: &str = "RIDESHELL_SHIM_BYPASS";

pub fn validate_tool_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || name.starts_with('.')
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        bail!("'{name}' is not a valid tool name (letters, digits, '-', '_', '.')");
    }
    if name.eq_ignore_ascii_case(brand::BIN_NAME) {
        bail!("cannot shim {} itself", brand::BIN_NAME);
    }
    Ok(())
}

pub fn shim_file(dir: &Path, name: &str) -> PathBuf {
    if cfg!(windows) {
        dir.join(format!("{name}.exe"))
    } else {
        dir.join(name)
    }
}

/// Names of installed shims.
pub fn list(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if cfg!(windows) {
                let lower = name.to_ascii_lowercase();
                lower.strip_suffix(".exe").map(|_| name[..name.len() - 4].to_string())
            } else if name.contains(".old") {
                None
            } else {
                Some(name)
            }
        })
        .collect();
    names.sort_by_key(|n| n.to_ascii_lowercase());
    names
}

/// Create or replace the shim for `name`, pointing at `exe`.
pub fn create(dir: &Path, name: &str, exe: &Path) -> Result<PathBuf> {
    validate_tool_name(name)?;
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let target = shim_file(dir, name);
    set_aside(&target)?;
    #[cfg(windows)]
    {
        if std::fs::hard_link(exe, &target).is_err() {
            std::fs::copy(exe, &target).with_context(|| format!("cannot create shim {}", target.display()))?;
        }
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(exe, &target).with_context(|| format!("cannot create shim {}", target.display()))?;
    }
    cleanup_old(dir);
    Ok(target)
}

pub fn remove(dir: &Path, name: &str) -> Result<bool> {
    let target = shim_file(dir, name);
    if std::fs::symlink_metadata(&target).is_err() {
        return Ok(false);
    }
    set_aside(&target)?;
    cleanup_old(dir);
    Ok(true)
}

/// Re-point every shim at `exe` (after RideShell itself was updated).
pub fn refresh(dir: &Path, exe: &Path) -> Result<Vec<String>> {
    let names = list(dir);
    for n in &names {
        create(dir, n, exe)?;
    }
    Ok(names)
}

/// Move an existing file out of the way. A running executable cannot be
/// deleted on Windows but can be renamed; leftovers are cleaned up later.
fn set_aside(target: &Path) -> Result<()> {
    if std::fs::symlink_metadata(target).is_err() {
        return Ok(());
    }
    if std::fs::remove_file(target).is_ok() {
        return Ok(());
    }
    let aside = target.with_file_name(format!(
        "{}.old-{}-{}",
        target.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id(),
        crate::store::now()
    ));
    std::fs::rename(target, &aside).with_context(|| format!("cannot replace {}", target.display()))
}

fn cleanup_old(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().contains(".old-") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (dunce::canonicalize(a), dunce::canonicalize(b)) {
        (Ok(a), Ok(b)) => {
            if cfg!(windows) {
                a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
            } else {
                a == b
            }
        }
        _ => false,
    }
}

/// The real `name` on PATH, skipping RideShell's shims directory (and any
/// other copy of RideShell). `shims_dir` defaults to the configured one.
pub fn find_real(name: &str, shims_dir: Option<&Path>) -> Result<PathBuf> {
    let discovered;
    let shims_dir = match shims_dir {
        Some(d) => Some(d),
        None => {
            discovered = Paths::discover().ok().map(|p| p.shims_dir());
            discovered.as_deref()
        }
    };
    let me = std::env::current_exe().ok().and_then(|p| dunce::canonicalize(p).ok());
    let candidates = which::which_all(name).map_err(|e| anyhow!("{name}: {e}"))?;
    for candidate in candidates {
        let in_shims = candidate.parent().zip(shims_dir).is_some_and(|(parent, shims)| same_dir(parent, shims));
        let is_me = me.as_ref().is_some_and(|me| dunce::canonicalize(&candidate).ok().as_ref() == Some(me));
        if !in_shims && !is_me {
            return Ok(candidate);
        }
    }
    bail!("{name} is not installed (no '{name}' found on PATH apart from the {} shim)", brand::APP_NAME)
}

/// Is `dir` on the PATH of this process?
pub fn on_path(dir: &Path) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|entry| same_dir(&entry, dir)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        validate_tool_name("claude").unwrap();
        validate_tool_name("docker-compose").unwrap();
        for bad in ["", "a/b", "..", ".x", "rideshell", "RideShell", "a b"] {
            assert!(validate_tool_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn create_list_remove_refresh() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join(if cfg!(windows) { "fake.exe" } else { "fake" });
        std::fs::write(&exe, b"binary").unwrap();
        let dir = tmp.path().join("shims");
        create(&dir, "claude", &exe).unwrap();
        create(&dir, "codex", &exe).unwrap();
        create(&dir, "claude", &exe).unwrap(); // replace is fine
        assert_eq!(list(&dir), ["claude", "codex"]);
        assert_eq!(refresh(&dir, &exe).unwrap().len(), 2);
        assert!(remove(&dir, "codex").unwrap());
        assert!(!remove(&dir, "codex").unwrap());
        assert_eq!(list(&dir), ["claude"]);
    }
}
