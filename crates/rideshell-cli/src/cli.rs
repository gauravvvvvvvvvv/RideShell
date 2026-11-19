//! Command-line definitions.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use rideshell_core::brand;
use rideshell_core::shells::Shell;

#[derive(Parser)]
#[command(
    name = brand::BIN_NAME,
    version,
    about = "Developer profiles, project-aware identities and isolated sessions for the shells you already use.",
    disable_help_subcommand = true,
    max_term_width = 100
)]
pub struct Cli {
    /// Without a command, RideShell opens a RideShell window
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Open a RideShell window here: PowerShell with cmd and bash commands working too (the default)
    #[command(visible_alias = "terminal")]
    Open(OpenArgs),
    /// Open RideShell App (the windowed terminal with tabs, accounts and launchers)
    App {
        /// Open it as Administrator
        #[arg(long)]
        admin: bool,
    },
    /// Open an Administrator RideShell window (same profile and folder), or run one command as Administrator
    #[command(visible_alias = "sudo")]
    Admin(AdminArgs),
    /// Show whether a command line would run in PowerShell, cmd or bash
    Dialect {
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true, value_name = "LINE")]
        line: Vec<String>,
    },
    /// Internal: classify the line in RIDESHELL_LINE (Enter key in a RideShell window)
    #[command(hide = true)]
    Route,
    /// Internal: run RIDESHELL_LINE in cmd or bash and write the state changes to --out
    #[command(hide = true)]
    DialectExec {
        dialect: String,
        #[arg(long)]
        out: PathBuf,
        /// Attach the command to the console window directly
        #[arg(long)]
        console: bool,
    },
    /// Internal: record directory, environment and exit code of the calling shell
    #[command(name = "__capture", hide = true)]
    Capture { file: PathBuf, code: Option<String> },
    /// Set up configuration, database, a default profile and (with approval) shell integration
    Init(InitArgs),
    /// Create, inspect, switch, bind and manage developer profiles
    #[command(subcommand)]
    Profile(ProfileCommand),
    /// Attach tool accounts (Claude Code, Codex, GitHub CLI, Git...) to a profile
    #[command(subcommand)]
    Identity(IdentityCommand),
    /// Bring your Claude Code / Codex conversations into accounts (adopt the original folder, or share history)
    #[command(subcommand)]
    History(HistoryCommand),
    /// Store API keys and tokens in the OS credential store, per profile or per project
    #[command(subcommand)]
    Secret(SecretCommand),
    /// Variables attached to a project directory (apply to everything below it)
    #[command(subcommand)]
    Project(ProjectCommand),
    /// Sign in to, sign out of, or check a tool account for a profile
    #[command(subcommand)]
    Auth(AuthCommand),
    /// Manage tool shims: `claude`, `codex`, `gh`... routed through RideShell in every terminal
    #[command(subcommand)]
    Shim(ShimCommand),
    /// List identity adapters and whether their tools are installed
    Providers,
    /// List the command-line tools installed on this PC (everything on PATH), and where each came from
    Tools {
        /// Only names containing this text
        filter: Option<String>,
        /// Include Windows' own commands (System32...)
        #[arg(long)]
        all: bool,
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
    /// Run one command with a profile applied, isolated from this shell
    Run(RunArgs),
    /// Print the environment a profile applies (optionally as shell code)
    Env(EnvArgs),
    /// Show the active profile for this session and directory
    Status,
    /// Print shell integration code to load with eval / Invoke-Expression
    Hook {
        #[arg(value_enum)]
        shell: ShellArg,
        /// Include the RideShell window extras (automatic cmd/bash routing)
        #[arg(long, hide = true)]
        session: bool,
    },
    /// Internal: called by the shell prompt hook
    #[command(hide = true)]
    HookEnv {
        #[arg(long, value_enum)]
        shell: ShellArg,
        #[arg(long)]
        cwd: Option<PathBuf>,
        /// First call when the shell starts: apply silently
        #[arg(long)]
        initial: bool,
    },
    /// Install, remove or inspect shell startup integration
    #[command(subcommand)]
    Shell(ShellCommand),
    /// Check the installation, database, shells and identities for problems
    Doctor,
    /// Show configuration
    #[command(subcommand)]
    Config(ConfigCommand),
    /// Install RideShell for this user (copies the exe, adds it and the shims to PATH)
    Install(InstallArgs),
    /// Remove shell integration, and optionally all RideShell data
    Uninstall(UninstallArgs),
}

/// Where a variable or secret lives.
#[derive(Args, Clone)]
#[group(required = true, multiple = false)]
pub struct Scope {
    /// Attach to a profile (follows the profile into every directory)
    #[arg(short, long)]
    pub profile: Option<String>,
    /// Attach to a project directory (applies to it and everything below it)
    #[arg(short, long, value_name = "DIR")]
    pub dir: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum SecretCommand {
    /// Store a secret, e.g. `secret set ANTHROPIC_API_KEY --profile work` (prompts for the value)
    Set {
        key: String,
        #[command(flatten)]
        scope: Scope,
        /// Read the value from standard input instead of prompting
        #[arg(long)]
        stdin: bool,
    },
    /// Delete a secret
    #[command(visible_alias = "rm")]
    Unset {
        key: String,
        #[command(flatten)]
        scope: Scope,
    },
    /// List secret names (values are never shown)
    #[command(visible_alias = "ls")]
    List,
}

#[derive(Subcommand)]
pub enum ProjectCommand {
    /// Manage plain (non-secret) variables of a directory
    #[command(subcommand)]
    Env(ProjectEnvCommand),
}

#[derive(Subcommand)]
pub enum ProjectEnvCommand {
    /// Set a variable for a directory, e.g. `project env set NODE_ENV development`
    Set {
        key: String,
        value: String,
        /// Directory (default: current)
        #[arg(short, long, default_value = ".")]
        dir: PathBuf,
        #[arg(long)]
        not_secret: bool,
    },
    /// Remove a variable from a directory
    Unset {
        key: String,
        #[arg(short, long, default_value = ".")]
        dir: PathBuf,
    },
    /// List all directory variables
    #[command(visible_alias = "ls")]
    List,
}

#[derive(Subcommand)]
pub enum AuthCommand {
    /// Start the tool's own sign-in for a profile's account, e.g. `auth login claude --profile work`
    Login {
        provider: String,
        #[arg(short, long)]
        profile: Option<String>,
        /// Where the sign-in page opens: your normal browser (default) or a private window
        #[arg(long, value_enum, default_value_t = BrowserArg::Default)]
        browser: BrowserArg,
        /// Pre-fill this e-mail on the sign-in page (Claude Code)
        #[arg(long)]
        email: Option<String>,
    },
    /// Sign the profile's account out of the tool
    Logout {
        provider: String,
        #[arg(short, long)]
        profile: Option<String>,
    },
    /// Show the tool's own sign-in status for each profile (or one profile)
    Status {
        provider: Option<String>,
        #[arg(short, long)]
        profile: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum HistoryCommand {
    /// Where each account's conversations live
    Status,
    /// Use the tool's original folder (e.g. ~/.claude): its sign-in and all its conversations
    Adopt {
        /// claude or codex
        tool: String,
        #[arg(short, long)]
        profile: String,
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Keep this account's own sign-in, but see the same conversations as the original folder
    Share {
        tool: String,
        #[arg(short, long)]
        profile: String,
    },
    /// Stop sharing: the account gets its own history again
    Separate {
        tool: String,
        #[arg(short, long)]
        profile: String,
    },
}

#[derive(Subcommand)]
pub enum ShimCommand {
    /// Route a tool through RideShell, e.g. `shim add wrangler`
    Add { tools: Vec<String> },
    /// Stop routing a tool through RideShell
    #[command(visible_alias = "rm")]
    Remove { tools: Vec<String> },
    /// List shims and the real tools they launch
    #[command(visible_alias = "ls")]
    List,
    /// Re-create all shims (after updating RideShell)
    Refresh,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum BrowserArg {
    /// Your default browser, as it is (signed-in accounts, saved passwords)
    Default,
    /// A private window of your default browser (no existing sign-ins)
    Private,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum HostArg {
    /// PowerShell 7+
    Pwsh,
    /// Windows PowerShell 5.1
    Powershell,
}

impl HostArg {
    pub fn id(self) -> &'static str {
        match self {
            HostArg::Pwsh => "pwsh",
            HostArg::Powershell => "powershell",
        }
    }
}

#[derive(Args, Default)]
pub struct OpenArgs {
    /// Profile for this window (default: folder binding / default profile)
    #[arg(short, long)]
    pub profile: Option<String>,
    /// Folder to start in (default: current)
    #[arg(short, long, value_name = "DIR")]
    pub dir: Option<PathBuf>,
    /// PowerShell to host the window (default: `session.host` in config)
    #[arg(long, value_enum)]
    pub host: Option<HostArg>,
    /// Open it as Administrator (same as `rideshell admin`)
    #[arg(long)]
    pub admin: bool,
    /// Run this command in the window after it starts (e.g. `claude`)
    #[arg(long, value_name = "LINE")]
    pub command: Option<String>,
    /// Do not print the welcome lines (used by RideShell App)
    #[arg(long, hide = true)]
    pub no_banner: bool,
}

#[derive(Args)]
pub struct AdminArgs {
    /// Profile (default: this terminal's profile)
    #[arg(short, long)]
    pub profile: Option<String>,
    /// Folder (default: current)
    #[arg(short, long, value_name = "DIR")]
    pub dir: Option<PathBuf>,
    #[arg(long, value_enum)]
    pub host: Option<HostArg>,
    /// Run just this command as Administrator (in a new window that waits for Enter)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, value_name = "COMMAND")]
    pub command: Vec<String>,
}

#[derive(Args)]
pub struct InstallArgs {
    /// Do not ask for confirmation
    #[arg(long, short = 'y')]
    pub yes: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum ShellArg {
    /// Windows PowerShell 5.1
    Powershell,
    /// PowerShell 7+
    Pwsh,
    Bash,
    Zsh,
}

impl From<ShellArg> for Shell {
    fn from(s: ShellArg) -> Shell {
        match s {
            ShellArg::Powershell => Shell::PowerShell,
            ShellArg::Pwsh => Shell::Pwsh,
            ShellArg::Bash => Shell::Bash,
            ShellArg::Zsh => Shell::Zsh,
        }
    }
}

#[derive(Args)]
pub struct InitArgs {
    /// Accept defaults without prompting (never edits shell startup files by itself)
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// Name of the default profile to create if none exist
    #[arg(long, default_value = "personal")]
    pub profile: String,
    /// Install integration into these shells without asking (explicit approval)
    #[arg(long = "install-shell", value_enum)]
    pub install_shell: Vec<ShellArg>,
    /// Do not offer shell integration
    #[arg(long, conflicts_with = "install_shell")]
    pub no_shell: bool,
}

#[derive(Subcommand)]
pub enum ProfileCommand {
    /// Create a profile
    Create {
        name: String,
        #[arg(short, long, default_value = "")]
        description: String,
        /// Copy environment and identities from an existing profile
        #[arg(long)]
        from: Option<String>,
        /// Make it the default profile
        #[arg(long)]
        default: bool,
    },
    /// List profiles
    #[command(visible_alias = "ls")]
    List,
    /// Show a profile's identities, environment and bindings (default: active)
    Show { name: Option<String> },
    /// Switch this shell session to a profile (requires shell integration)
    Use {
        #[arg(required_unless_present = "clear")]
        name: Option<String>,
        /// Remove the session override and fall back to bindings / default
        #[arg(long, conflicts_with = "name")]
        clear: bool,
        /// Internal: print shell code instead of instructions
        #[arg(long, value_enum, hide = true)]
        emit: Option<ShellArg>,
    },
    /// Same as `status`
    Status,
    /// Explain which profile is selected here and why
    Why {
        /// Directory to explain (default: current directory)
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Change a profile's description and/or name
    Edit {
        name: String,
        #[arg(short, long)]
        description: Option<String>,
        #[arg(long)]
        rename: Option<String>,
    },
    /// Rename a profile
    Rename { old: String, new: String },
    /// Copy a profile (environment and identities; not directory bindings)
    #[command(visible_alias = "copy")]
    Duplicate { source: String, target: String },
    /// Delete a profile
    #[command(visible_alias = "rm")]
    Delete {
        name: String,
        /// Also delete its directory bindings without asking
        #[arg(long)]
        force: bool,
    },
    /// Show or set the default profile
    Default {
        #[arg(conflicts_with = "clear")]
        name: Option<String>,
        #[arg(long)]
        clear: bool,
    },
    /// Manage plain environment variables applied by a profile
    #[command(subcommand)]
    Env(ProfileEnvCommand),
    /// Bind a directory (and everything below it) to a profile
    Bind { path: PathBuf, profile: String },
    /// Remove the binding of a directory (default: current directory)
    Unbind { path: Option<PathBuf> },
    /// List directory bindings
    Bindings,
    /// Export a profile as TOML (no secrets, no bindings)
    Export {
        name: String,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Import a profile from a TOML file
    Import {
        file: PathBuf,
        /// Import under a different name
        #[arg(long)]
        name: Option<String>,
        /// Replace an existing profile with the same name
        #[arg(long)]
        replace: bool,
        /// Accept values that look like secrets (they are stored in plain text)
        #[arg(long)]
        not_secret: bool,
    },
}

#[derive(Subcommand)]
pub enum ProfileEnvCommand {
    /// Set a variable, e.g. `profile env set work AWS_PROFILE company-prod`
    Set {
        profile: String,
        key: String,
        value: String,
        /// Confirm the value is not a secret even though it looks like one
        #[arg(long)]
        not_secret: bool,
    },
    /// Remove a variable
    Unset { profile: String, key: String },
    /// List variables
    #[command(visible_alias = "ls")]
    List { profile: String },
}

#[derive(Subcommand)]
pub enum IdentityCommand {
    /// Attach an account, e.g. `identity set work claude` or `identity set work git email=me@corp.example`
    Set {
        profile: String,
        provider: String,
        #[arg(value_name = "KEY=VALUE")]
        settings: Vec<String>,
    },
    /// Remove settings (or the whole identity when no keys are given)
    Unset { profile: String, provider: String, keys: Vec<String> },
    /// Show identities of a profile (default: active profile)
    Show { profile: Option<String> },
}

#[derive(Args)]
pub struct RunArgs {
    /// Profile to use (default: normal resolution for this directory)
    #[arg(short, long)]
    pub profile: Option<String>,
    /// Run in this directory (default: current)
    #[arg(short = 'C', long, value_name = "DIR")]
    pub dir: Option<PathBuf>,
    /// Wait for Enter before exiting (used for Administrator windows)
    #[arg(long, hide = true)]
    pub pause: bool,
    /// Command and arguments (use `--` before commands that take flags)
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true, value_name = "COMMAND")]
    pub command: Vec<String>,
}

#[derive(Args)]
pub struct EnvArgs {
    #[arg(short, long)]
    pub profile: Option<String>,
    /// Emit shell code for this shell instead of a table
    #[arg(long, value_enum)]
    pub shell: Option<ShellArg>,
}

#[derive(Subcommand)]
pub enum ShellCommand {
    /// Add the integration block to a shell's startup file (shows it first)
    Install {
        #[arg(value_enum)]
        shell: ShellArg,
        /// Apply without asking
        #[arg(long, short = 'y')]
        yes: bool,
        /// Only show what would change
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove the integration block from a shell's startup file
    Uninstall {
        #[arg(value_enum)]
        shell: ShellArg,
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Show detected shells and integration state
    Status,
}

#[derive(Subcommand)]
pub enum ConfigCommand {
    /// Print the effective configuration
    Show,
    /// Print configuration and data locations
    Path,
}

#[derive(Args)]
pub struct UninstallArgs {
    /// Also remove the installed executable, PATH entries and Apps & Features entry
    #[arg(long)]
    pub program: bool,
    /// Leave shell startup files alone (used by automated tests)
    #[arg(long, hide = true)]
    pub keep_shell_integration: bool,
    /// Also delete RideShell's configuration and database
    #[arg(long)]
    pub purge: bool,
    #[arg(long, short = 'y')]
    pub yes: bool,
}
