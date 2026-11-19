//! Reversible edits to shell startup files.
//!
//! RideShell adds exactly one clearly delimited block:
//!
//! ```text
//! # >>> rideshell >>>
//! ...one guarded line...
//! # <<< rideshell <<<
//! ```
//!
//! The line is guarded so the shell keeps working if RideShell is removed.
//! Edits keep the file's line endings and BOM, write a timestamped backup,
//! and replace the file atomically. Uninstall removes only that block.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};

use super::{posix_bin_path, posix_quote, ps_literal, Shell, Syntax};
use crate::brand;

/// Startup file for `shell` for the current user.
pub fn rc_file(shell: Shell) -> Result<PathBuf> {
    match shell {
        Shell::PowerShell | Shell::Pwsh => powershell_profile(shell),
        Shell::Bash => Ok(home()?.join(".bashrc")),
        Shell::Zsh => {
            let dir = std::env::var_os("ZDOTDIR").filter(|v| !v.is_empty()).map(PathBuf::from);
            Ok(dir.map_or_else(home, Ok)?.join(".zshrc"))
        }
    }
}

fn home() -> Result<PathBuf> {
    dirs::home_dir().ok_or_else(|| anyhow!("cannot determine the home directory"))
}

/// Ask PowerShell itself: `$PROFILE` depends on the edition and on folder
/// redirection (e.g. OneDrive "Documents"), so it must not be guessed.
/// `CurrentUserAllHosts` also covers the VS Code PowerShell host.
fn powershell_profile(shell: Shell) -> Result<PathBuf> {
    let exe = which::which(shell.executable()).with_context(|| {
        format!("{} ({}) is not installed or not on PATH", shell.display_name(), shell.executable())
    })?;
    let mut cmd = Command::new(exe);
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", "$PROFILE.CurrentUserAllHosts"]);
    let out = crate::platform::no_window(&mut cmd).output().context("failed to ask PowerShell for its profile path")?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || path.is_empty() {
        bail!("PowerShell did not report a profile path");
    }
    Ok(PathBuf::from(path))
}

/// The managed block (LF line endings; converted on write).
pub fn block(shell: Shell, bin: &Path, use_path_lookup: bool) -> String {
    let (begin, end) = brand::rc_markers();
    let cmd = brand::BIN_NAME;
    let id = shell.id();
    let body = match (shell.syntax(), use_path_lookup) {
        (Syntax::PowerShell, true) => format!(
            "if (Get-Command {cmd} -CommandType Application -ErrorAction SilentlyContinue) {{ \
             Invoke-Expression ((& (Get-Command {cmd} -CommandType Application | Select-Object -First 1).Source hook {id}) -join \"`n\") }}"
        ),
        (Syntax::PowerShell, false) => {
            let lit = ps_literal(&bin.to_string_lossy());
            format!("if (Test-Path -LiteralPath {lit}) {{ Invoke-Expression ((& {lit} hook {id}) -join \"`n\") }}")
        }
        (Syntax::Posix, true) => {
            format!("if command -v {cmd} >/dev/null 2>&1; then eval \"$(command {cmd} hook {id})\"; fi")
        }
        (Syntax::Posix, false) => {
            let q = posix_quote(&posix_bin_path(bin));
            format!("if [ -x {q} ]; then eval \"$({q} hook {id})\"; fi")
        }
    };
    format!("{begin}\n# Managed by {cmd}. Remove with: {cmd} shell uninstall {id}\n{body}\n{end}")
}

/// Byte range of the managed block (including its final newline).
pub fn find_block(content: &str) -> Option<(usize, usize)> {
    let (begin, end) = brand::rc_markers();
    let start = line_start_of(content, &begin)?;
    let end_marker = start + content[start..].find(&end)?;
    let mut stop = end_marker + end.len();
    if content[stop..].starts_with("\r\n") {
        stop += 2;
    } else if content[stop..].starts_with('\n') {
        stop += 1;
    }
    Some((start, stop))
}

fn line_start_of(content: &str, needle: &str) -> Option<usize> {
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        if line.trim_end() == needle {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

fn newline_of(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Content with the block inserted (or replaced in place if present).
pub fn with_block(content: &str, block: &str) -> String {
    let nl = newline_of(content);
    let block = format!("{}{nl}", block.replace('\n', nl));
    if let Some((start, stop)) = find_block(content) {
        return format!("{}{block}{}", &content[..start], &content[stop..]);
    }
    let mut out = content.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(nl);
    }
    if !out.trim().is_empty() {
        out.push_str(nl);
    }
    out.push_str(&block);
    out
}

/// Content with the block removed, or `None` if there is no block.
pub fn without_block(content: &str) -> Option<String> {
    let (start, stop) = find_block(content)?;
    let mut before = content[..start].to_string();
    // Drop the blank separator line `with_block` added.
    if before.ends_with("\n\n") {
        before.pop();
    } else if before.ends_with("\r\n\r\n") {
        before.truncate(before.len() - 2);
    }
    Some(format!("{before}{}", &content[stop..]))
}

/// Read a startup file. A UTF-8 BOM is preserved separately.
pub fn read_rc(path: &Path) -> Result<(bool, String)> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let (bom, body) = match bytes.strip_prefix(b"\xEF\xBB\xBF") {
                Some(rest) => (true, rest.to_vec()),
                None => (false, bytes),
            };
            let text = String::from_utf8(body)
                .map_err(|_| anyhow!("{} is not valid UTF-8; refusing to edit it", path.display()))?;
            Ok((bom, text))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((false, String::new())),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
}

/// Write `content` atomically, backing up the existing file first.
/// Returns the backup path, if a backup was made.
pub fn write_rc(path: &Path, bom: bool, content: &str) -> Result<Option<PathBuf>> {
    let dir = path.parent().ok_or_else(|| anyhow!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let backup = if path.exists() {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let stem = format!("{name}.{}-backup-{}", brand::BIN_NAME, crate::store::now());
        let mut backup = dir.join(&stem);
        let mut n = 1;
        while backup.exists() {
            backup = dir.join(format!("{stem}-{n}"));
            n += 1;
        }
        std::fs::copy(path, &backup).with_context(|| format!("cannot back up {}", path.display()))?;
        Some(backup)
    } else {
        None
    };
    let tmp =
        dir.join(format!(".{}.{}.tmp", path.file_name().unwrap_or_default().to_string_lossy(), std::process::id()));
    let mut bytes = Vec::with_capacity(content.len() + 3);
    if bom {
        bytes.extend_from_slice(b"\xEF\xBB\xBF");
    }
    bytes.extend_from_slice(content.as_bytes());
    std::fs::write(&tmp, &bytes).with_context(|| format!("cannot write {}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("cannot replace {}", path.display()));
    }
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blk() -> String {
        block(Shell::Bash, Path::new("/usr/bin/rideshell"), true)
    }

    #[test]
    fn install_is_idempotent_and_reversible() {
        let original = "export A=1\nalias ll='ls -l'\n";
        let once = with_block(original, &blk());
        let twice = with_block(&once, &blk());
        assert_eq!(once, twice);
        assert!(once.starts_with(original));
        assert_eq!(without_block(&once).unwrap(), original);
    }

    #[test]
    fn crlf_files_stay_crlf() {
        let original = "$x = 1\r\n";
        let ps = block(Shell::PowerShell, Path::new(r"C:\Tools\rideshell.exe"), false);
        let installed = with_block(original, &ps);
        assert!(!installed.replace("\r\n", "").contains('\n'), "mixed line endings: {installed:?}");
        assert_eq!(without_block(&installed).unwrap(), original);
    }

    #[test]
    fn empty_file() {
        let installed = with_block("", &blk());
        assert!(installed.starts_with("# >>> rideshell >>>"));
        assert_eq!(without_block(&installed).unwrap(), "");
        assert!(without_block("nothing here\n").is_none());
    }

    #[test]
    fn block_in_middle_is_replaced_in_place() {
        let content = format!("a\n{}\nb\n", blk());
        let replaced = with_block(&content, &blk().replace("hook bash", "hook bash "));
        assert!(replaced.starts_with("a\n# >>>") && replaced.ends_with("\nb\n"));
    }

    #[test]
    fn non_ascii_install_path_stays_ascii_for_powershell() {
        let b = block(Shell::PowerShell, Path::new(r"C:\Users\Jürgen\bin\rideshell.exe"), false);
        assert!(b.is_ascii());
    }

    #[test]
    fn write_keeps_bom_and_makes_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile with space.ps1");
        std::fs::write(&path, b"\xEF\xBB\xBF$a = 1\n").unwrap();
        let (bom, text) = read_rc(&path).unwrap();
        assert!(bom);
        let backup = write_rc(&path, bom, &with_block(&text, &blk())).unwrap().unwrap();
        assert!(std::fs::read(&path).unwrap().starts_with(b"\xEF\xBB\xBF$a = 1\n"));
        assert_eq!(std::fs::read(backup).unwrap(), b"\xEF\xBB\xBF$a = 1\n");
    }
}
