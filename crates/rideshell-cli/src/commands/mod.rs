mod config;
mod connect;
mod doctor;
mod history;
mod hook;
mod identity;
mod init;
pub mod install;
mod profile;
mod run;
mod secret;
mod session;
mod shell;
pub mod shim;
mod status;
mod uninstall;

use std::path::PathBuf;

use anyhow::Result;
use rideshell_core::brand;
use rideshell_core::config::Config;
use rideshell_core::paths::Paths;
use rideshell_core::resolve::{self, Request, Resolution};
use rideshell_core::store::Store;

use crate::cli::{Command, ProfileCommand};

/// Shared per-invocation context.
pub struct Ctx {
    pub paths: Paths,
}

impl Ctx {
    pub fn new() -> Result<Self> {
        Ok(Self { paths: Paths::discover()? })
    }

    /// Open (creating on first use) the database.
    pub fn store(&self) -> Result<Store> {
        self.paths.ensure()?;
        Store::open(&self.paths.db_file())
    }

    pub fn config(&self) -> Result<Config> {
        Config::load(&self.paths)
    }
}

pub fn session_profile() -> Option<String> {
    std::env::var(brand::env::PROFILE).ok().filter(|s| !s.is_empty())
}

pub fn current_dir() -> Option<PathBuf> {
    std::env::current_dir().ok()
}

/// Resolve the profile for this process: flag > session > binding > default.
pub fn resolve_here(ctx: &Ctx, store: &Store, flag: Option<&str>, cwd: Option<&std::path::Path>) -> Result<Resolution> {
    let config = ctx.config()?;
    let session = session_profile();
    let fallback = current_dir();
    resolve::resolve(
        store,
        &Request {
            flag,
            session: session.as_deref(),
            cwd: cwd.or(fallback.as_deref()),
            use_bindings: config.shell.auto_apply,
        },
    )
}

pub fn dispatch(command: Option<Command>) -> Result<i32> {
    let ctx = Ctx::new()?;
    let Some(command) = command else { return open_window(&ctx) };
    match command {
        Command::Open(args) => session::open(&ctx, args),
        Command::App { admin } => launch_app(admin),
        Command::Admin(args) => session::admin(&ctx, args),
        Command::Dialect { line } => session::explain(&line.join(" ")),
        Command::Route => session::route(),
        Command::DialectExec { dialect, out, console } => session::dialect_exec(&ctx, &dialect, &out, console),
        Command::Capture { file, code } => session::capture(&file, code),
        Command::Init(args) => init::run(&ctx, args),
        Command::Profile(ProfileCommand::Status) | Command::Status => status::run(&ctx),
        Command::Profile(cmd) => profile::run(&ctx, cmd),
        Command::Identity(cmd) => identity::run(&ctx, cmd),
        Command::Providers => identity::providers(),
        Command::Connect { tool, profile } => connect::run(&ctx, tool, profile),
        Command::Inspect { tool, json } => shim::inspect(&ctx, &tool, json),
        Command::Tools { filter, all, json } => shim::tools(&ctx, filter.as_deref(), all, json),
        Command::Secret(cmd) => secret::run(&ctx, cmd),
        Command::History(cmd) => history::run(&ctx, cmd),
        Command::Project(cmd) => secret::project(&ctx, cmd),
        Command::Shim(cmd) => shim::run(&ctx, cmd),
        Command::Install(args) => install::run(&ctx, args),
        Command::Run(args) => run::run(&ctx, args),
        Command::Env(args) => run::env(&ctx, args),
        Command::Hook { shell, session } => hook::hook(&ctx, shell.into(), session),
        Command::HookEnv { shell, cwd, initial } => Ok(hook::hook_env(&ctx, shell.into(), cwd, initial)),
        Command::Shell(cmd) => shell::run(cmd),
        Command::Doctor => doctor::run(&ctx),
        Command::Config(cmd) => config::run(&ctx, cmd),
        Command::Uninstall(args) => uninstall::run(&ctx, args),
    }
}

/// Open a RideShell window (no command given, or double-clicked).
pub fn open_window(ctx: &Ctx) -> Result<i32> {
    session::open(ctx, crate::cli::OpenArgs::default())
}

pub use session::base64 as encode_base64;

fn launch_app(admin: bool) -> Result<i32> {
    let app = install::sibling_app().or_else(install::installed_app).ok_or_else(|| {
        anyhow::anyhow!("RideShell App is not installed (run `{} install` from a full download)", brand::BIN_NAME)
    })?;
    let mut cmd = std::process::Command::new(app);
    if admin {
        cmd.arg("--admin");
    }
    if let Ok(dir) = std::env::current_dir() {
        cmd.args(["--dir".as_ref(), dir.as_os_str()]);
    }
    if let Some(p) = session_profile() {
        cmd.args(["--profile", &p]);
    }
    cmd.spawn()?;
    Ok(0)
}
