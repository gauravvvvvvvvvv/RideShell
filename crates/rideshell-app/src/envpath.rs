//! Keep the app's PATH current while it runs.
//!
//! A program gets its PATH once, when it starts. Installers (winget, setup
//! programs) add their folder to the PATH stored in the registry, so a tool
//! installed while RideShell App is open would stay "not installed" until a
//! restart. [`refresh`] adds the registry's new PATH folders to this
//! process's PATH, so the Launch list and "All installed tools" update live,
//! and new tabs start with the new PATH too.
//!
//! Folders already on PATH keep their order (RideShell's shims stay first);
//! new ones are appended. Cheap: two registry reads. Call it off the UI thread.

#[cfg(windows)]
pub fn refresh() {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    let read = |root, sub: &str| -> String {
        RegKey::predef(root)
            .open_subkey_with_flags(sub, KEY_READ)
            .and_then(|k| k.get_value::<String, _>("Path"))
            .unwrap_or_default()
    };
    // Same order Windows uses: system first, then user.
    let stored = format!(
        "{};{}",
        read(HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment"),
        read(HKEY_CURRENT_USER, "Environment")
    );
    let current = std::env::var("PATH").unwrap_or_default();
    if let Some(merged) = merge(&current, &stored, |name| std::env::var(name).ok()) {
        std::env::set_var("PATH", merged);
    }
}

#[cfg(not(windows))]
pub fn refresh() {}

fn key(dir: &str) -> String {
    dir.trim().trim_end_matches(['\\', '/']).to_ascii_lowercase()
}

/// Expand `%NAME%` references (registry PATH values are often REG_EXPAND_SZ).
fn expand(s: &str, var: &impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        let Some(len) = rest[start + 1..].find('%') else { break };
        out.push_str(&rest[..start]);
        let name = &rest[start + 1..start + 1 + len];
        match var(name) {
            Some(v) if !name.is_empty() => out.push_str(&v),
            _ => out.push_str(&rest[start..start + len + 2]),
        }
        rest = &rest[start + len + 2..];
    }
    out.push_str(rest);
    out
}

/// `current` plus the folders of `stored` it lacks; `None` when nothing is new.
fn merge(current: &str, stored: &str, var: impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut seen: std::collections::HashSet<String> = current.split(';').map(key).filter(|k| !k.is_empty()).collect();
    let mut added = Vec::new();
    for dir in stored.split(';') {
        let dir = expand(dir.trim(), &var);
        let k = key(&dir);
        if !k.is_empty() && seen.insert(k) {
            added.push(dir);
        }
    }
    if added.is_empty() {
        return None;
    }
    let mut out = current.trim_end_matches(';').to_string();
    for d in added {
        if !out.is_empty() {
            out.push(';');
        }
        out.push_str(&d);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_only_new_folders_and_keeps_order() {
        let var = |n: &str| (n == "LOCALAPPDATA").then(|| r"C:\Users\me\AppData\Local".to_string());
        let current = r"C:\shims;C:\Windows\system32";
        let stored = r"C:\Windows\System32\;%LOCALAPPDATA%\Programs\GitHub CLI;;C:\shims";
        assert_eq!(
            merge(current, stored, var).as_deref(),
            Some(r"C:\shims;C:\Windows\system32;C:\Users\me\AppData\Local\Programs\GitHub CLI")
        );
        assert_eq!(merge(current, r"C:\shims\", |_| None), None, "nothing new");
    }

    #[test]
    fn unknown_variables_stay_as_written() {
        assert_eq!(expand(r"%NOPE%\bin;50%", &|_| None), r"%NOPE%\bin;50%");
    }
}

#[cfg(all(test, windows))]
mod live {
    #[test]
    #[ignore = "reads this PC's registry PATH"]
    fn finds_tools_installed_after_start() {
        println!("BEFORE gh: {:?}", which::which("gh").ok());
        super::refresh();
        println!("AFTER  gh: {:?}", which::which("gh").ok());
    }
}
