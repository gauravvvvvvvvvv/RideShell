//! Product naming.
//!
//! Every user-visible occurrence of the product name, binary name and
//! environment-variable prefix is derived from this module so the project can
//! be renamed in one place. (Shell scripts are templated from these values.)

/// Human-readable product name.
pub const APP_NAME: &str = "RideShell";
/// Executable / command name.
pub const BIN_NAME: &str = "rideshell";
/// Prefix of every environment variable owned by RideShell.
pub const ENV_PREFIX: &str = "RIDESHELL";
/// Format identifier written into exported profile files.
pub const PROFILE_FORMAT: &str = "rideshell-profile/1";

/// Environment variables owned by RideShell. They must all start with
/// [`ENV_PREFIX`] (enforced by a unit test).
pub mod env {
    /// Overrides the configuration and data directory (one directory for both).
    pub const HOME: &str = "RIDESHELL_HOME";
    /// Session-level profile override. Set by `profile use` in a shell.
    pub const PROFILE: &str = "RIDESHELL_PROFILE";
    /// The profile whose environment is currently applied in this process.
    pub const ACTIVE_PROFILE: &str = "RIDESHELL_ACTIVE_PROFILE";
    /// Bookkeeping: which variables RideShell set and their original values.
    pub const STATE: &str = "RIDESHELL_STATE";
    /// Which shell adapter is loaded in this session.
    pub const SHELL: &str = "RIDESHELL_SHELL";
    /// Set to `0` to hide the prompt indicator for this session.
    pub const PROMPT: &str = "RIDESHELL_PROMPT";

    /// Set in a RideShell window (`rideshell open`).
    pub const SESSION: &str = "RIDESHELL_SESSION";
    /// Internal: the command line being routed to cmd / bash.
    pub const LINE: &str = "RIDESHELL_LINE";
    /// Internal: PowerShell parser hints for routing (`1` = parse errors).
    pub const PS_ERRORS: &str = "RIDESHELL_PS_ERRORS";
    /// Internal: `name=kind;...` for each command in the line, as PowerShell resolves it.
    pub const PS_COMMANDS: &str = "RIDESHELL_PS_COMMANDS";

    pub const ALL: [&str; 6] = [HOME, PROFILE, ACTIVE_PROFILE, STATE, SHELL, PROMPT];
}

/// First line of shell code emitted for `eval`. Shell wrappers check for it
/// so they never evaluate unrelated output (such as `--help` text).
pub fn emit_marker() -> String {
    format!("# {BIN_NAME}:emit")
}

/// Start/end markers of the block added to shell startup files.
pub fn rc_markers() -> (String, String) {
    (format!("# >>> {BIN_NAME} >>>"), format!("# <<< {BIN_NAME} <<<"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_vars_use_prefix() {
        for v in env::ALL {
            assert!(v.starts_with(&format!("{ENV_PREFIX}_")), "{v}");
        }
    }
}
