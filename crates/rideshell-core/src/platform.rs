//! Small platform helpers: elevation (UAC), Windows argument quoting,
//! opening files with their default program.

use std::path::Path;

use anyhow::{bail, Context, Result};

#[cfg(not(windows))]
use crate::brand;

/// Quote one argument for the Windows command line (CommandLineToArgvW rules).
pub fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut out = String::from('"');
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat('\\').take(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat('\\').take(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.extend(std::iter::repeat('\\').take(backslashes * 2));
    out.push('"');
    out
}

#[cfg(windows)]
pub fn elevate(exe: &Path, params: &str, dir: &Path) -> Result<()> {
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let verb = wide("runas");
    let file = wide(&exe.to_string_lossy());
    let params = wide(params);
    let dir = wide(&dir.to_string_lossy());
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOASYNC;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = params.as_ptr();
    info.lpDirectory = dir.as_ptr();
    info.nShow = SW_SHOWNORMAL;
    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok == 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(1223) {
            bail!("Administrator permission was declined");
        }
        return Err(err).context("cannot start an Administrator window");
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn elevate(_exe: &Path, _params: &str, _dir: &Path) -> Result<()> {
    bail!("`{} admin` is Windows-only; use sudo", brand::BIN_NAME)
}

/// Is this process running elevated (as Administrator)?
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut core::ffi::c_void,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}

/// Open a file or URL with its default program.
#[cfg(windows)]
pub fn open_with_default(target: &str) -> Result<()> {
    std::process::Command::new("explorer.exe").arg(target).spawn().context("cannot open")?;
    Ok(())
}

#[cfg(not(windows))]
pub fn open_with_default(target: &str) -> Result<()> {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    std::process::Command::new(opener).arg(target).spawn().context("cannot open")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_argument_quoting() {
        assert_eq!(quote_arg("plain"), "plain");
        assert_eq!(quote_arg(""), "\"\"");
        assert_eq!(quote_arg("a b"), "\"a b\"");
        assert_eq!(quote_arg(r"C:\Program Files\"), r#""C:\Program Files\\""#);
        assert_eq!(quote_arg(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote_arg(r"C:\x\y"), r"C:\x\y");
    }
}
