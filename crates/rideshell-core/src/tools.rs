//! Discover command-line tools installed on this computer.
//!
//! A tool is "usable across the PC" when it is on PATH: every terminal can
//! run it by name. This module lists exactly those, the first match per name
//! winning as it does in a shell, and says where each came from (npm, winget,
//! Scoop, cargo, Python...). It only reads directory listings; nothing is
//! executed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Tool {
    pub name: String,
    pub path: PathBuf,
    /// Where it was installed from, e.g. "npm", "winget", "system".
    pub source: &'static str,
    /// Routed through RideShell (a shim exists).
    pub shimmed: bool,
}

/// Executable extensions, in PATHEXT order (Windows) or "" (Unix).
fn extensions() -> Vec<String> {
    if cfg!(windows) {
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        let mut v: Vec<String> = pathext.split(';').filter(|e| !e.is_empty()).map(|e| e.to_ascii_lowercase()).collect();
        // Scripts PowerShell runs by name.
        v.push(".ps1".into());
        // Rarely meant as commands.
        v.retain(|e| !matches!(e.as_str(), ".vbs" | ".vbe" | ".js" | ".jse" | ".wsf" | ".wsh" | ".msc" | ".cpl"));
        v
    } else {
        vec![String::new()]
    }
}

/// Classify a PATH directory by the package manager or vendor behind it.
pub fn source_of(dir: &Path) -> &'static str {
    let d = dir.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    let has = |s: &str| d.contains(s);
    if has("/rideshell/shims") {
        "rideshell"
    } else if has("/appdata/roaming/npm") || has("/nodejs") || has("/.npm-global") || has("/pnpm") || has("/.bun/") {
        "npm"
    } else if has("/winget/links") || has("/winget/packages") {
        "winget"
    } else if has("/scoop/") {
        "scoop"
    } else if has("/.cargo/bin") {
        "cargo"
    } else if has("/chocolatey/") {
        "chocolatey"
    } else if has("/python") || has("/pipx") || has("/.local/bin") && cfg!(windows) {
        "python"
    } else if has("/go/bin") {
        "go"
    } else if has("/.dotnet") || has("/dotnet") {
        "dotnet"
    } else if has("/git/") {
        "git"
    } else if has("/windowsapps") {
        "store"
    } else if has("/windows/") || d.ends_with("/windows") || has("/system32") {
        "system"
    } else if has("/homebrew/") || has("/usr/local/bin") || has("/opt/homebrew") {
        "homebrew"
    } else if d.starts_with("/usr/") || d.starts_with("/bin") || d.starts_with("/sbin") {
        "system"
    } else if has("/program files") || has("/programs/") {
        "program"
    } else {
        "other"
    }
}

/// Every command on PATH. With `include_system`, also Windows' own folders
/// (System32 alone holds hundreds of executables).
pub fn scan(shims_dir: Option<&Path>, include_system: bool) -> Vec<Tool> {
    scan_path(&std::env::var_os("PATH").unwrap_or_default(), shims_dir, include_system)
}

/// [`scan`] over an explicit PATH value.
pub fn scan_path(path: &std::ffi::OsStr, shims_dir: Option<&Path>, include_system: bool) -> Vec<Tool> {
    let exts = extensions();
    let shimmed: Vec<String> =
        shims_dir.map(crate::shims::list).unwrap_or_default().into_iter().map(|s| s.to_ascii_lowercase()).collect();
    let mut found: BTreeMap<String, Tool> = BTreeMap::new();
    let mut seen_dirs = Vec::new();
    for dir in std::env::split_paths(&path) {
        let key = dir.to_string_lossy().trim_end_matches(['\\', '/']).to_ascii_lowercase();
        if key.is_empty() || seen_dirs.contains(&key) {
            continue;
        }
        seen_dirs.push(key);
        let source = source_of(&dir);
        if source == "rideshell" || (source == "system" && !include_system) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut in_dir: Vec<(usize, String, PathBuf)> = Vec::new();
        for e in entries.flatten() {
            let p = e.path();
            let file = e.file_name().to_string_lossy().into_owned();
            let (stem, ext) = match file.rfind('.') {
                Some(i) if cfg!(windows) => (file[..i].to_string(), file[i..].to_ascii_lowercase()),
                _ => (file.clone(), String::new()),
            };
            let Some(rank) = exts.iter().position(|x| *x == ext) else { continue };
            if stem.is_empty() || stem.starts_with('.') {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                match e.metadata() {
                    Ok(m) if m.is_file() && m.permissions().mode() & 0o111 != 0 => {}
                    _ => continue,
                }
            }
            #[cfg(not(unix))]
            if !e.file_type().is_ok_and(|t| t.is_file() || t.is_symlink()) {
                continue;
            }
            in_dir.push((rank, stem, p));
        }
        // Within one folder PATHEXT order decides (npm puts x, x.cmd and x.ps1 side by side).
        in_dir.sort_by_key(|e| e.0);
        for (_, stem, p) in in_dir {
            let name = stem.to_ascii_lowercase();
            if found.contains_key(&name) {
                continue;
            }
            found.insert(name.clone(), Tool { shimmed: shimmed.contains(&name), name: stem, path: p, source });
        }
    }
    found.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources() {
        assert_eq!(source_of(Path::new(r"C:\Users\a\AppData\Roaming\npm")), "npm");
        assert_eq!(source_of(Path::new(r"C:\Users\a\AppData\Local\Microsoft\WinGet\Links")), "winget");
        assert_eq!(source_of(Path::new(r"C:\Users\a\scoop\shims")), "scoop");
        assert_eq!(source_of(Path::new(r"C:\Users\a\.cargo\bin")), "cargo");
        assert_eq!(source_of(Path::new(r"C:\Windows\System32")), "system");
        assert_eq!(source_of(Path::new(r"C:\Program Files\Git\cmd")), "git");
        assert_eq!(source_of(Path::new(r"C:\Program Files\nodejs")), "npm");
        assert_eq!(source_of(Path::new(r"C:\Users\a\AppData\Local\rideshell\shims")), "rideshell");
        assert_eq!(source_of(Path::new(r"C:\Program Files\Docker\Docker\resources\bin")), "program");
    }

    #[test]
    fn scan_finds_tools_first_on_path_wins() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let exe = if cfg!(windows) { "mytool.exe" } else { "mytool" };
        for d in [a.path(), b.path()] {
            std::fs::write(d.join(exe), "x").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(d.join(exe), std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        std::fs::write(a.path().join("notes.txt"), "x").unwrap();
        let path = std::env::join_paths([a.path(), b.path()]).unwrap();
        let tools = scan_path(&path, None, true);
        let t: Vec<_> = tools.iter().filter(|t| t.name.eq_ignore_ascii_case("mytool")).collect();
        assert_eq!(t.len(), 1);
        assert!(t[0].path.starts_with(a.path()));
        assert!(!tools.iter().any(|t| t.name == "notes"));
    }
}
