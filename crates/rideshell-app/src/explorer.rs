//! Windows 11 Explorer integration the user can opt into.
//!
//! Windows 11's compact right-click menu only lists commands from packaged
//! (MSIX, signed) apps; RideShell's "Open in RideShell" is in the classic menu
//! ("Show more options", or Shift+right-click), at the top. Users who want it
//! directly can switch Explorer to the classic menu: a documented per-user
//! registry setting, fully reversible.

const CLASSIC_KEY: &str = r"Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}";

#[cfg(windows)]
pub fn classic_menu() -> bool {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(format!(r"{CLASSIC_KEY}\InprocServer32"), KEY_READ)
        .is_ok()
}

#[cfg(windows)]
pub fn set_classic_menu(on: bool) -> anyhow::Result<()> {
    use anyhow::Context;
    use winreg::enums::HKEY_CURRENT_USER;
    let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
    if on {
        let (key, _) =
            hkcu.create_subkey(format!(r"{CLASSIC_KEY}\InprocServer32")).context("cannot write the setting")?;
        key.set_value("", &"").context("cannot write the setting")?;
    } else {
        match hkcu.delete_subkey_all(CLASSIC_KEY) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).context("cannot remove the setting"),
        }
    }
    Ok(())
}

/// Restart Explorer so the menu setting applies (open windows close; the
/// taskbar comes back by itself).
#[cfg(windows)]
pub fn restart() {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new("cmd")
        .args(["/d", "/c", "taskkill /f /im explorer.exe >nul & start explorer.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

#[cfg(not(windows))]
pub fn classic_menu() -> bool {
    false
}
#[cfg(not(windows))]
pub fn set_classic_menu(_on: bool) -> anyhow::Result<()> {
    anyhow::bail!("Windows only")
}
#[cfg(not(windows))]
pub fn restart() {}
