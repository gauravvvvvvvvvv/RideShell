//! Opening a sign-in page in a private browser window, only when the user
//! asks for it (`rideshell auth login ... --browser private`).
//!
//! Tools such as Claude Code and GitHub CLI open their sign-in page through
//! the standard `BROWSER` variable. For a private sign-in RideShell sets
//! `BROWSER` to itself with `RIDESHELL_BROWSER_PRIVATE=1`. When the tool
//! "opens the browser", RideShell receives the URL and opens it in a private
//! window of the user's default browser.

/// Set in the environment of a login started with `--browser private`.
pub const PRIVATE_ENV: &str = "RIDESHELL_BROWSER_PRIVATE";

/// Called from `main` when RideShell was started as the `BROWSER` of a login.
pub fn open_private(url: &str) -> i32 {
    match open_private_inner(url) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("rideshell: could not open a private window ({e:#}); open this link yourself:\n{url}");
            1
        }
    }
}

#[cfg(windows)]
fn open_private_inner(url: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    let (exe, flag) = default_browser().or_else(edge).context("no browser found")?;
    std::process::Command::new(&exe)
        .arg(flag)
        .arg(url)
        .spawn()
        .with_context(|| format!("cannot start {}", exe.display()))?;
    Ok(())
}

#[cfg(not(windows))]
fn open_private_inner(url: &str) -> anyhow::Result<()> {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    std::process::Command::new(opener).arg(url).spawn()?;
    Ok(())
}

/// The private-window switch of well-known browsers, by executable name.
#[cfg(windows)]
fn private_flag(exe: &std::path::Path) -> Option<&'static str> {
    let name = exe.file_name()?.to_string_lossy().to_ascii_lowercase();
    Some(match name.as_str() {
        "msedge.exe" => "--inprivate",
        "chrome.exe" | "brave.exe" | "vivaldi.exe" | "chromium.exe" | "arc.exe" => "--incognito",
        "firefox.exe" | "librewolf.exe" | "waterfox.exe" | "floorp.exe" | "zen.exe" => "-private-window",
        "opera.exe" | "launcher.exe" => "--private",
        _ => return None,
    })
}

/// The user's default browser for https links (from the registry).
#[cfg(windows)]
fn default_browser() -> Option<(std::path::PathBuf, &'static str)> {
    use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER};
    use winreg::RegKey;
    let choice = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice")
        .ok()?;
    let prog_id: String = choice.get_value("ProgId").ok()?;
    let command: String = RegKey::predef(HKEY_CLASSES_ROOT)
        .open_subkey(format!(r"{prog_id}\shell\open\command"))
        .ok()?
        .get_value("")
        .ok()?;
    let exe = first_token(&command)?;
    let flag = private_flag(&exe)?;
    Some((exe, flag))
}

#[cfg(windows)]
fn edge() -> Option<(std::path::PathBuf, &'static str)> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let path: String = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\msedge.exe")
        .ok()?
        .get_value("")
        .ok()?;
    Some((std::path::PathBuf::from(path), "--inprivate"))
}

/// The program of a registry command line (`"C:\x\y.exe" --flag "%1"`).
#[cfg(windows)]
fn first_token(command: &str) -> Option<std::path::PathBuf> {
    let c = command.trim();
    let exe =
        if let Some(rest) = c.strip_prefix('"') { rest.split('"').next()? } else { c.split_whitespace().next()? };
    Some(std::path::PathBuf::from(exe)).filter(|p| p.is_file())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn known_browsers_have_private_flags() {
        assert_eq!(private_flag(std::path::Path::new(r"C:\x\msedge.exe")), Some("--inprivate"));
        assert_eq!(private_flag(std::path::Path::new(r"C:\x\Chrome.exe")), Some("--incognito"));
        assert_eq!(private_flag(std::path::Path::new(r"C:\x\firefox.exe")), Some("-private-window"));
        assert_eq!(private_flag(std::path::Path::new(r"C:\x\unknown.exe")), None);
    }
}
