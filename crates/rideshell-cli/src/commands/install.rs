//! Self-installation.
//!
//! `rideshell.exe` is its own installer: double-click it (or run
//! `rideshell install`) and it installs for the current user without admin
//! rights:
//!
//! * copies itself to `%LOCALAPPDATA%\Programs\RideShell\rideshell.exe`
//! * puts the shims directory and that folder at the front of the *user*
//!   PATH (HKCU\Environment), so `claude`, `codex`, `gh`... resolve through
//!   RideShell in every new terminal — PowerShell, cmd, Git Bash, VS Code
//! * registers "RideShell" under Apps & Features for normal uninstallation
//!
//! Limitation: Windows puts the *system* PATH before the user PATH, so a tool
//! installed in a system-wide directory is reached before the shim in plain
//! cmd. RideShell's shell integration (PowerShell/bash) moves the shims to
//! the very front of PATH, which covers that case in those shells.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rideshell_core::brand;
use rideshell_core::config::Config;
use rideshell_core::shims;

use super::Ctx;
use crate::cli::InstallArgs;
use crate::ui;

/// Where the program is installed.
pub fn install_dir() -> Result<PathBuf> {
    if let Some(d) = std::env::var_os("RIDESHELL_TEST_INSTALL_DIR").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(d));
    }
    if cfg!(windows) {
        let base = dirs_local()?;
        Ok(base.join("Programs").join(brand::APP_NAME))
    } else {
        let home = std::env::var_os("HOME").map(PathBuf::from).context("HOME is not set")?;
        Ok(home.join(".local").join("bin"))
    }
}

fn dirs_local() -> Result<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).context("LOCALAPPDATA is not set")
}

mod bundle {
    include!(concat!(env!("OUT_DIR"), "/app_bundle.rs"));
}

/// File name of RideShell App (the windowed terminal).
pub fn app_name() -> &'static str {
    if cfg!(windows) {
        "rideshell-app.exe"
    } else {
        "rideshell-app"
    }
}

/// The installed RideShell App, if present.
pub fn installed_app() -> Option<PathBuf> {
    install_dir().ok().map(|d| d.join(app_name())).filter(|p| p.is_file())
}

/// RideShell App next to this executable (installed or a build folder).
pub fn sibling_app() -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    Some(me.parent()?.join(app_name())).filter(|p| p.is_file())
}

/// Install RideShell App: from the copy bundled into this executable, or
/// from a sibling file (development builds). Returns its path.
fn install_app(dir: &Path) -> Result<Option<PathBuf>> {
    let dst = dir.join(app_name());
    if let Some(bytes) = bundle::APP_EXE {
        let staged = dst.with_extension("download");
        std::fs::write(&staged, bytes).with_context(|| format!("cannot write {}", staged.display()))?;
        let r = replace_file(&staged, &dst);
        let _ = std::fs::remove_file(&staged);
        r?;
        return Ok(Some(dst));
    }
    match sibling_app() {
        Some(src) if same_file(&src, &dst) => Ok(Some(dst)),
        Some(src) => {
            replace_file(&src, &dst)?;
            Ok(Some(dst))
        }
        None => Ok(None),
    }
}

/// Double-clicked and not installed yet: open the install wizard (the
/// bundled RideShell App in `--setup` mode, run from a temporary folder)
/// instead of installing in a console. False when this build has no app.
#[cfg(windows)]
pub fn launch_setup_wizard() -> bool {
    let (Some(bytes), Ok(me)) = (bundle::APP_EXE, std::env::current_exe()) else { return false };
    let dir = std::env::temp_dir().join(format!("{}-setup-{}", brand::APP_NAME, env!("CARGO_PKG_VERSION")));
    let app = dir.join(app_name());
    let ready = app.is_file() && std::fs::metadata(&app).is_ok_and(|m| m.len() == bytes.len() as u64);
    if !ready && (std::fs::create_dir_all(&dir).is_err() || std::fs::write(&app, bytes).is_err()) {
        return false;
    }
    std::process::Command::new(&app).arg("--setup").arg(&me).spawn().is_ok()
}

pub fn installed_exe() -> Result<PathBuf> {
    let name = if cfg!(windows) { format!("{}.exe", brand::BIN_NAME) } else { brand::BIN_NAME.to_string() };
    Ok(install_dir()?.join(name))
}

/// Is this process the installed copy?
#[cfg(windows)]
pub fn running_installed_copy() -> bool {
    match (std::env::current_exe(), installed_exe()) {
        (Ok(me), Ok(target)) => same_file(&me, &target),
        _ => false,
    }
}

pub fn is_installed() -> bool {
    installed_exe().is_ok_and(|p| p.is_file())
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (dunce::canonicalize(a), dunce::canonicalize(b)) {
        (Ok(a), Ok(b)) => a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy()),
        _ => false,
    }
}

/// Copy `src` over `dst` even while `dst` is running (Windows can rename a
/// running executable but not overwrite it).
fn replace_file(src: &Path, dst: &Path) -> Result<()> {
    let dir = dst.parent().context("install path has no parent")?;
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let staged = dst.with_extension("new");
    std::fs::copy(src, &staged).with_context(|| format!("cannot copy to {}", staged.display()))?;
    if dst.exists() && std::fs::remove_file(dst).is_err() {
        let aside = dst.with_extension(format!("old-{}", rideshell_core::store::now()));
        std::fs::rename(dst, &aside).with_context(|| format!("cannot replace {}", dst.display()))?;
    }
    std::fs::rename(&staged, dst).with_context(|| format!("cannot install {}", dst.display()))?;
    // Best-effort cleanup of earlier set-aside copies.
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().contains(".old-") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    Ok(())
}

pub fn run(ctx: &Ctx, args: InstallArgs) -> Result<i32> {
    let me = std::env::current_exe().context("cannot determine the executable path")?;
    let target = installed_exe()?;
    let bin_dir = install_dir()?;
    let shims_dir = ctx.paths.shims_dir();

    anstream::println!("{} {}", ui::accent(brand::APP_NAME), ui::dim(env!("CARGO_PKG_VERSION")));
    anstream::println!();
    anstream::println!("This will (for your user only, no admin rights needed):");
    anstream::println!("  • install {}", target.display());
    if cfg!(windows) {
        anstream::println!("  • add to the front of your user PATH:");
        anstream::println!("      {}   (tool shims: claude, codex, gh...)", shims_dir.display());
        anstream::println!("      {}", bin_dir.display());
        anstream::println!(
            "  • add {} to Apps & Features (uninstall from there or `{} uninstall --program`)",
            brand::APP_NAME,
            brand::BIN_NAME
        );
        anstream::println!(
            "  • add Start menu entries \"{0}\" and \"{0} (Administrator)\", a Windows Terminal profile,
    and \"Open in {0}\" to the right-click menu of folders in Explorer",
            brand::APP_NAME
        );
    } else {
        anstream::println!("  • refresh tool shims in {}", shims_dir.display());
    }
    anstream::println!("  • create your settings and a default profile if you have none");
    anstream::println!("Nothing else on your system is changed. Your existing tools and logins keep working.");
    anstream::println!();
    if !args.yes && !ui::confirm("Install?", true) {
        anstream::println!("Nothing changed.");
        return Ok(1);
    }

    if same_file(&me, &target) {
        ui::success("already running the installed copy");
    } else {
        replace_file(&me, &target)?;
        ui::success(format!("installed {}", target.display()));
    }

    let app = install_app(&bin_dir)?;
    match &app {
        Some(a) => ui::success(format!("installed RideShell App ({})", a.display())),
        None => ui::hint("RideShell App is not part of this build; the Start menu opens RideShell in a console"),
    }

    // Settings, database, default profile.
    ctx.paths.ensure()?;
    Config::write_default_if_missing(&ctx.paths)?;
    let mut store = ctx.store()?;
    if store.list_profiles()?.is_empty() {
        let p = store.create_profile("personal", "Default profile")?;
        store.set_default_profile(Some(&p.name))?;
        ui::success(format!("created default profile {}", ui::accent(&p.name)));
        // The first profile keeps the logins the tools already have.
        let kept = rideshell_core::providers::attach_default_accounts(&mut store, &p.name)?;
        if !kept.is_empty() {
            ui::hint(format!("{} uses your existing {} logins; new profiles get their own", p.name, kept.join(", ")));
        }
    }
    drop(store);

    // Shims must point at the installed binary.
    std::fs::create_dir_all(&shims_dir)?;
    let refreshed = shims::refresh(&shims_dir, &target)?;
    if !refreshed.is_empty() {
        ui::success(format!("updated shims: {}", refreshed.join(", ")));
    }

    #[cfg(windows)]
    {
        let changed = windows::add_to_user_path(&[&shims_dir, &bin_dir])?;
        if changed {
            ui::success("updated your user PATH");
        } else {
            ui::success("PATH already set up");
        }
        windows::register_uninstall(&target, &bin_dir)?;
        ui::success("registered in Apps & Features");
        if let Some(app) = &app {
            match windows::register_folder_menu(app) {
                Ok(()) => ui::success("added \"Open in RideShell\" to Explorer's right-click menu for folders"),
                Err(e) => ui::warn(format!("could not add the Explorer menu entry: {e:#}")),
            }
        }
        match windows::create_launchers(&target, app.as_deref()) {
            Ok(()) => ui::success("added Start menu entries and a Windows Terminal profile"),
            Err(e) => ui::warn(format!("could not add Start menu entries: {e:#}")),
        }
        windows::broadcast_environment_change();
    }
    #[cfg(not(windows))]
    {
        ui::hint(format!(
            "make sure {} is on PATH, then run `{} shell install bash` (or zsh): it puts the shims first on PATH",
            bin_dir.display(),
            brand::BIN_NAME
        ));
    }

    anstream::println!();
    anstream::println!("Done. Open a {} terminal, then for example:", ui::paint("new", ui::BOLD));
    ui::hint(format!("{} profile create work", brand::BIN_NAME));
    ui::hint(format!("{} profile bind C:\\code\\company work      # this folder uses work", brand::BIN_NAME));
    ui::hint("then type `claude` in that folder: it signs in once per account and remembers it");
    if cfg!(windows) {
        ui::hint(format!(
            "open \"{0}\" from the Start menu (or type `{1}`): PowerShell, cmd and bash commands all work there",
            brand::APP_NAME,
            brand::BIN_NAME
        ));
    }
    Ok(0)
}

/// Remove the installed program, PATH entries, shims and the Apps & Features entry.
pub fn uninstall_program(ctx: &Ctx) -> Result<()> {
    let shims_dir = ctx.paths.shims_dir();
    for name in shims::list(&shims_dir) {
        let _ = shims::remove(&shims_dir, &name);
    }
    let _ = std::fs::remove_dir(&shims_dir);
    ui::success("removed tool shims");

    #[cfg(windows)]
    {
        let bin_dir = install_dir()?;
        windows::remove_from_user_path(&[&shims_dir, &bin_dir])?;
        windows::unregister_uninstall()?;
        windows::remove_launchers();
        windows::unregister_folder_menu();
        windows::broadcast_environment_change();
        ui::success("removed PATH entries and the Apps & Features entry");
        if bin_dir.exists() {
            windows::delete_dir_after_exit(&bin_dir);
            ui::success(format!("{} will be removed when this window closes", bin_dir.display()));
        }
    }
    #[cfg(not(windows))]
    {
        let exe = installed_exe()?;
        if exe.exists() {
            std::fs::remove_file(&exe).with_context(|| format!("cannot remove {}", exe.display()))?;
            ui::success(format!("removed {}", exe.display()));
        }
    }
    Ok(())
}

#[cfg(windows)]
pub mod windows {
    use std::os::windows::process::CommandExt;
    use std::path::Path;

    use anyhow::{Context, Result};
    use winreg::enums::{RegType, HKEY_CURRENT_USER, KEY_READ};
    use winreg::{RegKey, RegValue};

    use rideshell_core::brand;

    /// Registry root for tests (`RIDESHELL_TEST_REGISTRY_ROOT=Software\RideShellTest`)
    /// so automated tests never touch the real user environment.
    fn root() -> Option<String> {
        std::env::var("RIDESHELL_TEST_REGISTRY_ROOT").ok().filter(|s| !s.is_empty())
    }

    fn env_key_path() -> String {
        root().map_or_else(|| "Environment".to_string(), |r| format!(r"{r}\Environment"))
    }

    fn uninstall_key_path() -> String {
        let base = root().unwrap_or_else(|| r"Software\Microsoft\Windows\CurrentVersion".to_string());
        format!(r"{base}\Uninstall\{}", brand::APP_NAME)
    }

    fn to_wide_bytes(s: &str) -> Vec<u8> {
        s.encode_utf16().chain(std::iter::once(0)).flat_map(|u| u.to_le_bytes()).collect()
    }

    fn from_wide_bytes(bytes: &[u8]) -> String {
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units).trim_end_matches('\0').to_string()
    }

    fn read_path() -> Result<(RegKey, Vec<String>, RegType)> {
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(env_key_path())
            .context("cannot open the user environment in the registry")?;
        let (entries, vtype) = match key.get_raw_value("Path") {
            Ok(v) => {
                let s = from_wide_bytes(&v.bytes);
                (s.split(';').filter(|e| !e.trim().is_empty()).map(str::to_string).collect(), v.vtype)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Vec::new(), RegType::REG_EXPAND_SZ),
            Err(e) => return Err(e).context("cannot read the user PATH"),
        };
        Ok((key, entries, vtype))
    }

    fn write_path(key: &RegKey, entries: &[String], vtype: RegType) -> Result<()> {
        let value = RegValue { bytes: to_wide_bytes(&entries.join(";")), vtype };
        key.set_raw_value("Path", &value).context("cannot write the user PATH")
    }

    fn same(a: &str, b: &Path) -> bool {
        a.trim_end_matches('\\').eq_ignore_ascii_case(b.to_string_lossy().trim_end_matches('\\'))
    }

    /// Put `dirs` at the front of the user PATH (in order). Keeps the value
    /// type (REG_EXPAND_SZ) so `%VARS%` in other entries keep working.
    pub fn add_to_user_path(dirs: &[&Path]) -> Result<bool> {
        let (key, entries, vtype) = read_path()?;
        let mut new: Vec<String> = dirs.iter().map(|d| d.to_string_lossy().into_owned()).collect();
        new.extend(entries.iter().filter(|e| !dirs.iter().any(|d| same(e, d))).cloned());
        if new == entries {
            return Ok(false);
        }
        write_path(&key, &new, vtype)?;
        Ok(true)
    }

    pub fn remove_from_user_path(dirs: &[&Path]) -> Result<()> {
        let (key, entries, vtype) = read_path()?;
        let new: Vec<String> = entries.iter().filter(|e| !dirs.iter().any(|d| same(e, d))).cloned().collect();
        if new != entries {
            write_path(&key, &new, vtype)?;
        }
        Ok(())
    }

    pub fn user_path_entries() -> Vec<String> {
        read_path().map(|(_, e, _)| e).unwrap_or_default()
    }

    pub fn register_uninstall(exe: &Path, dir: &Path) -> Result<()> {
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(uninstall_key_path())?;
        let exe_s = exe.to_string_lossy();
        key.set_value("DisplayName", &brand::APP_NAME)?;
        key.set_value("DisplayVersion", &env!("CARGO_PKG_VERSION"))?;
        key.set_value("Publisher", &format!("{} contributors", brand::APP_NAME))?;
        key.set_value("InstallLocation", &dir.to_string_lossy().as_ref())?;
        key.set_value("DisplayIcon", &exe_s.as_ref())?;
        key.set_value("UninstallString", &format!("\"{exe_s}\" uninstall --program"))?;
        key.set_value("QuietUninstallString", &format!("\"{exe_s}\" uninstall --program --yes"))?;
        key.set_value("NoModify", &1u32)?;
        key.set_value("NoRepair", &1u32)?;
        // What Settings → Apps and Control Panel show: size, install date, links.
        key.set_value("EstimatedSize", &folder_size_kb(dir))?;
        key.set_value("InstallDate", &install_date(rideshell_core::store::now()))?;
        key.set_value("Comments", &"One terminal for every shell and every account")?;
        let repo = "https://github.com/gauravvvvvvvvvv/RideShell";
        key.set_value("URLInfoAbout", &repo)?;
        key.set_value("HelpLink", &format!("{repo}/issues"))?;
        key.set_value("URLUpdateInfo", &format!("{repo}/releases"))?;
        key.set_value("Readme", &format!("{repo}#readme"))?;
        Ok(())
    }

    /// Size of the installed files in KB (EstimatedSize is a DWORD in KB).
    fn folder_size_kb(dir: &Path) -> u32 {
        fn walk(p: &Path) -> u64 {
            std::fs::read_dir(p)
                .map(|rd| {
                    rd.flatten()
                        .map(|e| match e.file_type() {
                            Ok(t) if t.is_dir() => walk(&e.path()),
                            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
                            Err(_) => 0,
                        })
                        .sum()
                })
                .unwrap_or(0)
        }
        u32::try_from(walk(dir).div_ceil(1024)).unwrap_or(u32::MAX)
    }

    /// `YYYYMMDD` (UTC) for Unix seconds, as Apps & Features expects.
    pub(super) fn install_date(unix: i64) -> String {
        // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
        let z = unix.div_euclid(86_400) + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        format!("{y:04}{m:02}{d:02}")
    }

    pub fn unregister_uninstall() -> Result<()> {
        match RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(uninstall_key_path()) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).context("cannot remove the Apps & Features entry"),
        }
    }

    /// Explorer's right-click menu: "Open in RideShell" on folders, on the
    /// background of an open folder, and on drives. Per user (HKCU\Software\Classes).
    fn folder_menu_keys() -> Vec<String> {
        let base = root().map_or_else(|| r"Software\Classes".to_string(), |r| format!(r"{r}\Classes"));
        ["Directory", r"Directory\Background", "Drive"]
            .iter()
            .map(|kind| format!(r"{base}\{kind}\shell\{}", brand::APP_NAME))
            .collect()
    }

    pub fn register_folder_menu(app: &Path) -> Result<()> {
        let app_s = app.to_string_lossy();
        for key_path in folder_menu_keys() {
            let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(&key_path)?;
            key.set_value("", &format!("Open in {}", brand::APP_NAME))?;
            key.set_value("Icon", &format!("\"{app_s}\",0"))?;
            // First in the (classic) menu, above "Open in Terminal".
            key.set_value("Position", &"Top")?;
            let (cmd, _) = key.create_subkey("command")?;
            cmd.set_value("", &format!("\"{app_s}\" --dir \"%V\""))?;
        }
        Ok(())
    }

    pub fn unregister_folder_menu() {
        for key_path in folder_menu_keys() {
            let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&key_path);
        }
    }

    pub fn folder_menu_registered() -> bool {
        folder_menu_keys().iter().all(|k| RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(k, KEY_READ).is_ok())
    }

    pub fn is_registered() -> bool {
        RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(uninstall_key_path(), KEY_READ).is_ok()
    }

    /// Folders for Start menu shortcuts and Windows Terminal fragments.
    /// `RIDESHELL_TEST_SHELL_FOLDERS` redirects both for automated tests.
    fn start_menu_dir() -> Option<std::path::PathBuf> {
        if let Some(t) = std::env::var_os("RIDESHELL_TEST_SHELL_FOLDERS").filter(|v| !v.is_empty()) {
            return Some(std::path::PathBuf::from(t).join("StartMenu"));
        }
        let appdata = std::env::var_os("APPDATA")?;
        Some(std::path::PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs"))
    }

    fn terminal_fragment_dir() -> Option<std::path::PathBuf> {
        if let Some(t) = std::env::var_os("RIDESHELL_TEST_SHELL_FOLDERS").filter(|v| !v.is_empty()) {
            return Some(std::path::PathBuf::from(t).join("Fragments").join(brand::APP_NAME));
        }
        let local = std::env::var_os("LOCALAPPDATA")?;
        Some(std::path::PathBuf::from(local).join(r"Microsoft\Windows Terminal\Fragments").join(brand::APP_NAME))
    }

    fn shortcut_paths() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
        let dir = start_menu_dir()?;
        Some((
            dir.join(format!("{}.lnk", brand::APP_NAME)),
            dir.join(format!("{} (Administrator).lnk", brand::APP_NAME)),
        ))
    }

    /// Start menu entries (normal and "run as administrator") and a Windows
    /// Terminal profile, all opening `rideshell open`.
    ///
    /// The two shortcuts must differ in target or arguments: the Start menu
    /// merges shortcuts that launch exactly the same command line, and the
    /// merged entry could be the elevated one.
    pub fn create_launchers(exe: &Path, app: Option<&Path>) -> Result<()> {
        let (normal, admin) = shortcut_paths().context("cannot locate the Start menu")?;
        let dir = normal.parent().context("bad Start menu path")?;
        std::fs::create_dir_all(dir)?;
        let description = "Terminal with accounts and profiles; PowerShell, cmd and bash commands";
        match app {
            Some(app) => {
                create_shortcut(&normal, app, "", app, description)?;
                create_shortcut(&admin, app, "--admin", app, description)?;
            }
            None => {
                create_shortcut(&normal, exe, "open", exe, description)?;
                create_shortcut(&admin, exe, "open --admin", exe, description)?;
            }
        }
        set_run_as_admin(&admin)?;

        if let Some(frag) = terminal_fragment_dir() {
            std::fs::create_dir_all(&frag)?;
            let profile = serde_json::json!({
                "profiles": [{
                    "name": brand::APP_NAME,
                    "commandline": format!("\"{}\" open", exe.display()),
                    "startingDirectory": "%USERPROFILE%",
                }]
            });
            std::fs::write(frag.join(format!("{}.json", brand::BIN_NAME)), serde_json::to_string_pretty(&profile)?)?;
        }
        Ok(())
    }

    pub fn remove_launchers() {
        if let Some((normal, admin)) = shortcut_paths() {
            let _ = std::fs::remove_file(normal);
            let _ = std::fs::remove_file(admin);
        }
        if let Some(frag) = terminal_fragment_dir() {
            let _ = std::fs::remove_dir_all(frag);
        }
    }

    pub fn launchers_present() -> bool {
        shortcut_paths().is_some_and(|(n, a)| n.is_file() && a.is_file())
    }

    /// Shortcuts are COM objects; Windows PowerShell's WScript.Shell is the
    /// simplest dependency-free way to write one.
    fn create_shortcut(lnk: &Path, target: &Path, args: &str, icon: &Path, description: &str) -> Result<()> {
        let lit = |s: &str| format!("'{}'", s.replace('\'', "''"));
        let workdir = std::env::var("USERPROFILE").unwrap_or_default();
        let script = format!(
            "$s = (New-Object -ComObject WScript.Shell).CreateShortcut({}); $s.TargetPath = {}; $s.Arguments = {}; \
             $s.WorkingDirectory = {}; $s.Description = {}; $s.IconLocation = {}; $s.Save()",
            lit(&lnk.to_string_lossy()),
            lit(&target.to_string_lossy()),
            lit(args),
            lit(&workdir),
            lit(description),
            lit(&format!("{},0", icon.display())),
        );
        let encoded: String = {
            let bytes: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
            crate::commands::encode_base64(&bytes)
        };
        let status = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .status()
            .context("cannot run PowerShell to create a shortcut")?;
        if !status.success() || !lnk.is_file() {
            anyhow::bail!("creating {} failed", lnk.display());
        }
        Ok(())
    }

    /// Set the "Run as administrator" flag (SLDF_RUNAS_USER, 0x2000 in the
    /// LinkFlags field at offset 0x14 of the .lnk header).
    fn set_run_as_admin(lnk: &Path) -> Result<()> {
        let mut bytes = std::fs::read(lnk)?;
        if bytes.len() < 0x18 || bytes[0..4] != [0x4C, 0, 0, 0] {
            anyhow::bail!("{} is not a shortcut file", lnk.display());
        }
        bytes[0x15] |= 0x20;
        std::fs::write(lnk, bytes)?;
        Ok(())
    }

    /// Tell Explorer (and terminals started from it) that PATH changed.
    pub fn broadcast_environment_change() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
        };
        if root().is_some() {
            return; // tests
        }
        let name: Vec<u16> = "Environment".encode_utf16().chain(std::iter::once(0)).collect();
        let mut result = 0usize;
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                name.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                2000,
                &mut result,
            );
        }
    }

    /// A running executable cannot delete its own folder; a hidden `cmd`
    /// waits a moment and removes it after this process exits.
    ///
    /// Only CREATE_NO_WINDOW: it gives `cmd` its own *invisible* console,
    /// which the `ping` used as a delay inherits, and which does not close
    /// with this window. Adding DETACHED_PROCESS would make Windows ignore
    /// CREATE_NO_WINDOW; `ping` would then open a visible console window.
    pub fn delete_dir_after_exit(dir: &Path) {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let script = format!("ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"", dir.display());
        let _ = std::process::Command::new("cmd")
            .args(["/d", "/c"])
            .raw_arg(&script)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }

    /// Was this process started by double-clicking (its own new console)?
    pub fn launched_from_explorer() -> bool {
        use windows_sys::Win32::System::Console::GetConsoleProcessList;
        let mut ids = [0u32; 4];
        let n = unsafe { GetConsoleProcessList(ids.as_mut_ptr(), ids.len() as u32) };
        n == 1
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn install_dates_are_yyyymmdd() {
        use super::windows::install_date;
        assert_eq!(install_date(0), "19700101");
        assert_eq!(install_date(951_782_400), "20000229"); // leap day
        assert_eq!(install_date(1_795_171_019), "20261120");
    }
}
