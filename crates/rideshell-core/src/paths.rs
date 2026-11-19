//! Locations of RideShell's own files.
//!
//! * Configuration (`config.toml`): the platform config directory
//!   (`%APPDATA%`, `~/Library/Application Support`, `~/.config`).
//! * Database: the platform *local* data directory (`%LOCALAPPDATA%`,
//!   `~/Library/Application Support`, `~/.local/share`). SQLite must not live
//!   on a roaming / synced profile directory.
//!
//! `RIDESHELL_HOME` overrides both with a single directory (used by tests and
//! portable installs).

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};

use crate::brand;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self> {
        if let Some(home) = std::env::var_os(brand::env::HOME).filter(|v| !v.is_empty()) {
            return Ok(Self::in_dir(PathBuf::from(home)));
        }
        let config =
            dirs::config_dir().ok_or_else(|| anyhow!("could not determine the user configuration directory"))?;
        let data = dirs::data_local_dir().ok_or_else(|| anyhow!("could not determine the user data directory"))?;
        Ok(Self { config_dir: config.join(brand::BIN_NAME), data_dir: data.join(brand::BIN_NAME) })
    }

    /// Keep configuration and data in a single directory.
    pub fn in_dir(dir: PathBuf) -> Self {
        Self { config_dir: dir.clone(), data_dir: dir }
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn db_file(&self) -> PathBuf {
        self.data_dir.join(format!("{}.db", brand::BIN_NAME))
    }

    /// Per-account config directories managed by RideShell
    /// (`accounts/<provider>/<account>`).
    pub fn accounts_dir(&self) -> PathBuf {
        self.data_dir.join("accounts")
    }

    /// Tool shims (launchers named after tools, e.g. `claude`).
    pub fn shims_dir(&self) -> PathBuf {
        self.data_dir.join("shims")
    }

    pub fn ensure(&self) -> Result<()> {
        for dir in [&self.config_dir, &self.data_dir] {
            std::fs::create_dir_all(dir).with_context(|| format!("cannot create directory {}", dir.display()))?;
        }
        Ok(())
    }
}
