//! What the sidebar shows: accounts (profiles), their tools, and launchers.

use std::path::PathBuf;

use anyhow::Result;
use rideshell_core::paths::Paths;
use rideshell_core::providers;
use rideshell_core::shells::{self, Shell};
use rideshell_core::shims;
use rideshell_core::store::Store;
use serde::Serialize;

#[derive(Serialize)]
pub struct ProfileView {
    pub name: String,
    pub description: String,
    #[serde(rename = "isDefault")]
    pub is_default: bool,
    /// Attached tool accounts (provider ids: claude, codex, gh, git).
    pub tools: Vec<String>,
    pub secrets: usize,
}

#[derive(Serialize)]
pub struct AppView {
    pub id: &'static str,
    pub name: &'static str,
    /// PowerShell line the tab runs.
    pub command: String,
    pub available: bool,
    /// Provider id whose account this app uses, if any.
    pub provider: Option<&'static str>,
    /// Executable whose own icon the launcher shows.
    pub icon: Option<String>,
    /// Official install command, offered when the program is missing.
    pub install: Option<String>,
}

#[derive(Serialize)]
pub struct ProviderView {
    pub id: &'static str,
    pub name: &'static str,
}

#[derive(Serialize)]
pub struct Place {
    pub name: String,
    pub path: String,
    /// "home", "desktop", "documents", "downloads", "project"
    pub kind: &'static str,
    /// Account bound to this folder (projects).
    pub profile: Option<String>,
}

#[derive(Serialize)]
pub struct StateView {
    pub version: &'static str,
    pub elevated: bool,
    pub home: String,
    pub host: &'static str,
    pub bash: bool,
    pub cli: bool,
    pub profiles: Vec<ProfileView>,
    pub apps: Vec<AppView>,
    pub providers: Vec<ProviderView>,
    /// Executables of the account tools (claude, codex, gh, git), for their icons.
    #[serde(rename = "toolIcons")]
    pub tool_icons: std::collections::BTreeMap<String, String>,
    pub places: Vec<Place>,
    pub error: Option<String>,
}

/// The vendor's own install command for a launcher (shown before running).
fn install_command(id: &str) -> Option<String> {
    let winget = |pkg: &str| Some(format!("winget install --id {pkg} --exact --source winget"));
    match id {
        "claude" => Some("irm https://claude.ai/install.ps1 | iex".into()),
        "codex" => Some("npm install --global @openai/codex".into()),
        "gh" => winget("GitHub.cli"),
        "git" | "bash" => winget("Git.Git"),
        "node" => winget("OpenJS.NodeJS.LTS"),
        "python" => winget("Python.Python.3.13"),
        "pwsh" => winget("Microsoft.PowerShell"),
        _ => None,
    }
}

fn places(paths: &Paths) -> Vec<Place> {
    let mut out = Vec::new();
    let mut add = |name: &str, p: Option<PathBuf>, kind: &'static str| {
        if let Some(p) = p.filter(|p| p.is_dir()) {
            out.push(Place { name: name.into(), path: p.to_string_lossy().into_owned(), kind, profile: None });
        }
    };
    add("Home", dirs::home_dir(), "home");
    add("Desktop", dirs::desktop_dir(), "desktop");
    add("Documents", dirs::document_dir(), "documents");
    add("Downloads", dirs::download_dir(), "downloads");
    if let Ok(Some(store)) = Store::open_existing(&paths.db_file()) {
        for b in store.bindings().unwrap_or_default() {
            let name = std::path::Path::new(&b.path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| b.path.clone());
            out.push(Place { name, path: b.path, kind: "project", profile: Some(b.profile) });
        }
    }
    out
}

#[derive(Serialize)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
}

/// Folder contents for the Files panel: folders first, hidden/system items skipped.
pub fn list_dir(path: &std::path::Path) -> Result<(String, Option<String>, Vec<Entry>)> {
    let path = dunce::canonicalize(path)?;
    let mut entries = Vec::new();
    for e in std::fs::read_dir(&path)?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(meta) = e.metadata() else { continue };
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            const HIDDEN: u32 = 0x2;
            const SYSTEM: u32 = 0x4;
            if meta.file_attributes() & (HIDDEN | SYSTEM) != 0 {
                continue;
            }
        }
        if name.starts_with('.') && !matches!(name.as_str(), ".github" | ".vscode" | ".config") {
            continue;
        }
        entries.push(Entry { dir: meta.is_dir(), name });
    }
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    entries.truncate(1000);
    let parent = path.parent().map(|p| p.to_string_lossy().into_owned());
    Ok((path.to_string_lossy().into_owned(), parent, entries))
}

fn ps_quote(p: &std::path::Path) -> String {
    format!("& '{}'", p.to_string_lossy().replace('\'', "''"))
}

pub fn load(paths: &Paths, cli_found: bool) -> StateView {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let bash = shells::detect(Shell::Bash);
    let host = if shells::detect(Shell::Pwsh).is_some() { "PowerShell 7" } else { "Windows PowerShell 5.1" };
    let (profiles, error) = match load_profiles(paths) {
        Ok(p) => (p, None),
        Err(e) => (Vec::new(), Some(format!("{e:#}"))),
    };
    let shims_dir = paths.shims_dir();
    let real = |tool: &str| shims::find_real(tool, Some(&shims_dir)).ok();
    let path_str = |p: &std::path::Path| p.to_string_lossy().into_owned();
    let mut apps = Vec::new();
    let mut add = |id: &'static str,
                   name: &'static str,
                   command: String,
                   exe: Option<PathBuf>,
                   provider: Option<&'static str>,
                   icon: Option<PathBuf>| {
        let install = if exe.is_some() { None } else { install_command(id) };
        apps.push(AppView {
            id,
            name,
            command,
            available: exe.is_some(),
            provider,
            icon: icon.or_else(|| exe.clone()).as_deref().map(path_str),
            install,
        });
    };
    add("claude", "Claude Code", "claude".into(), real("claude"), Some("claude"), None);
    add("codex", "Codex", "codex".into(), real("codex"), Some("codex"), None);
    add("gh", "GitHub CLI", "gh status".into(), real("gh"), Some("gh"), None);
    // Git's launcher (cmd\git.exe) has no icon; Git's own icon is on git-bash.exe in the same install.
    let git = real("git");
    let git_icon = git
        .as_ref()
        .and_then(|g| g.ancestors().find(|a| a.join("git-bash.exe").is_file()).map(|a| a.join("git-bash.exe")));
    add("git", "Git", "git status".into(), git, Some("git"), git_icon);
    // Git Bash's icon lives on git-bash.exe, next to the bin folder.
    let git_bash_icon =
        bash.as_ref().and_then(|b| b.parent()?.parent().map(|g| g.join("git-bash.exe"))).filter(|p| p.is_file());
    add(
        "bash",
        "Git Bash",
        bash.as_ref().map(|b| format!("{} --login -i", ps_quote(b))).unwrap_or_default(),
        bash.clone(),
        None,
        git_bash_icon,
    );
    let cmd_exe = std::env::var_os("ComSpec").map(PathBuf::from).filter(|p| p.is_file());
    add("cmd", "Command Prompt", "cmd.exe /k".into(), cmd_exe, None, None);
    let pwsh = shells::detect(Shell::Pwsh);
    let pwsh_cmd = pwsh.as_ref().map(|p| format!("{} -NoLogo", ps_quote(p))).unwrap_or_default();
    add("pwsh", "PowerShell 7", pwsh_cmd, pwsh, None, None);
    add("node", "Node.js", "node".into(), real("node"), None, None);
    let python = which::which("python").ok().filter(|p| !p.to_string_lossy().contains("WindowsApps"));
    add("python", "Python", "python".into(), python, None, None);

    let mut tool_icons = std::collections::BTreeMap::new();
    for p in providers::registry() {
        if let Some(app) = apps.iter().find(|a| a.provider == Some(p.id())) {
            if let Some(icon) = &app.icon {
                tool_icons.insert(p.id().to_string(), icon.clone());
            }
        }
    }

    StateView {
        version: env!("CARGO_PKG_VERSION"),
        elevated: rideshell_core::platform::is_elevated(),
        home,
        host,
        bash: bash.is_some(),
        cli: cli_found,
        profiles,
        apps,
        tool_icons,
        places: places(paths),
        providers: providers::registry().iter().map(|p| ProviderView { id: p.id(), name: p.display_name() }).collect(),
        error,
    }
}

fn load_profiles(paths: &Paths) -> Result<Vec<ProfileView>> {
    let Some(store) = Store::open_existing(&paths.db_file())? else { return Ok(Vec::new()) };
    let default = store.default_profile()?.map(|p| p.name);
    let mut out = Vec::new();
    for p in store.list_profiles()? {
        let tools = store.identities(p.id)?.keys().cloned().collect();
        let secrets = store.secret_keys(p.id)?.len();
        out.push(ProfileView {
            is_default: default.as_deref() == Some(p.name.as_str()),
            name: p.name,
            description: p.description,
            tools,
            secrets,
        });
    }
    Ok(out)
}

/// Create a profile (and make it the default if it is the first one).
pub fn create_profile(paths: &Paths, cli: Option<&std::path::Path>, name: &str, description: &str) -> Result<()> {
    paths.ensure()?;
    rideshell_core::config::Config::write_default_if_missing(paths)?;
    let mut store = Store::open(&paths.db_file())?;
    let first = store.list_profiles()?.is_empty();
    store.create_profile(name, description)?;
    if first {
        store.set_default_profile(Some(name))?;
    }
    // Its own Claude Code / Codex / GitHub CLI accounts (the first profile
    // keeps the existing logins), routed through RideShell when installed.
    let tools = providers::attach_default_accounts(&mut store, name)?;
    drop(store);
    if let Some(cli) = cli {
        let dir = paths.shims_dir();
        for tool in tools {
            if shims::find_real(tool, Some(&dir)).is_ok() {
                let _ = shims::create(&dir, tool, cli);
            }
        }
    }
    Ok(())
}

pub fn set_default(paths: &Paths, name: &str) -> Result<()> {
    let mut store = Store::open(&paths.db_file())?;
    store.set_default_profile(Some(name))
}
