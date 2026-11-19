//! Secret storage.
//!
//! Secret values (API keys, tokens) are kept in the operating system's
//! credential store, never in RideShell's database:
//!
//! * Windows: Credential Manager (generic credentials, per user)
//! * macOS: login Keychain
//! * Linux: Secret Service (GNOME Keyring / KWallet) over D-Bus
//!
//! The database only records *that* a variable is secret. Secrets are read
//! when a command is launched (`rideshell run`, tool shims) and injected
//! into that process only — they are never exported into the interactive
//! shell's environment.
//!
//! `RIDESHELL_INSECURE_TEST_VAULT=<file>` swaps in a plain-JSON file backend.
//! It exists for automated tests and is deliberately named to discourage use.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};

use crate::brand;

/// Environment variable selecting the insecure test backend.
pub const TEST_VAULT_ENV: &str = "RIDESHELL_INSECURE_TEST_VAULT";

pub trait Vault {
    fn get(&self, id: &str) -> Result<Option<String>>;
    fn set(&self, id: &str, secret: &str) -> Result<()>;
    /// Delete; succeeds when the entry does not exist.
    fn delete(&self, id: &str) -> Result<()>;
    fn backend(&self) -> &'static str;
}

/// Vault id of a profile's secret variable. Uses the profile id so a rename
/// keeps the secret; the key is upper-cased because env names are matched
/// case-insensitively.
pub fn profile_secret_id(profile_id: i64, key: &str) -> String {
    format!("profile/{profile_id}/{}", key.to_ascii_uppercase())
}

/// Vault id of a directory secret variable (row id is stable across updates).
pub fn dir_secret_id(row_id: i64) -> String {
    format!("dir/{row_id}")
}

/// The vault for this process.
pub fn open() -> Box<dyn Vault> {
    match std::env::var_os(TEST_VAULT_ENV).filter(|v| !v.is_empty()) {
        Some(path) => Box::new(FileVault { path: PathBuf::from(path) }),
        None => Box::new(OsVault),
    }
}

// ----- OS credential store ---------------------------------------------------

pub struct OsVault;

fn entry(id: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(brand::BIN_NAME, id).map_err(|e| anyhow!("credential store: {e}"))
}

impl Vault for OsVault {
    fn get(&self, id: &str) -> Result<Option<String>> {
        match entry(id)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow!("cannot read secret '{id}' from the credential store: {e}")),
        }
    }

    fn set(&self, id: &str, secret: &str) -> Result<()> {
        entry(id)?.set_password(secret).map_err(|e| anyhow!("cannot save secret '{id}' to the credential store: {e}"))
    }

    fn delete(&self, id: &str) -> Result<()> {
        match entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(anyhow!("cannot delete secret '{id}' from the credential store: {e}")),
        }
    }

    fn backend(&self) -> &'static str {
        if cfg!(windows) {
            "Windows Credential Manager"
        } else if cfg!(target_os = "macos") {
            "macOS Keychain"
        } else {
            "Secret Service"
        }
    }
}

// ----- test backend ------------------------------------------------------------

/// Plain JSON file. **Not secure.** Tests only.
pub struct FileVault {
    path: PathBuf,
}

impl FileVault {
    pub fn for_tests(path: PathBuf) -> Self {
        Self { path }
    }

    fn load(&self) -> Result<BTreeMap<String, String>> {
        match std::fs::read_to_string(&self.path) {
            Ok(t) => serde_json::from_str(&t).context("corrupt test vault"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e.into()),
        }
    }
    fn save(&self, map: &BTreeMap<String, String>) -> Result<()> {
        std::fs::write(&self.path, serde_json::to_string_pretty(map)?)?;
        Ok(())
    }
}

impl Vault for FileVault {
    fn get(&self, id: &str) -> Result<Option<String>> {
        Ok(self.load()?.remove(id))
    }
    fn set(&self, id: &str, secret: &str) -> Result<()> {
        let mut m = self.load()?;
        m.insert(id.into(), secret.into());
        self.save(&m)
    }
    fn delete(&self, id: &str) -> Result<()> {
        let mut m = self.load()?;
        m.remove(id);
        self.save(&m)
    }
    fn backend(&self) -> &'static str {
        "insecure test file (RIDESHELL_INSECURE_TEST_VAULT)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_vault_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let v = FileVault { path: dir.path().join("v.json") };
        assert_eq!(v.get("a").unwrap(), None);
        v.set("a", "s3cret").unwrap();
        assert_eq!(v.get("a").unwrap().as_deref(), Some("s3cret"));
        v.delete("a").unwrap();
        v.delete("a").unwrap();
        assert_eq!(v.get("a").unwrap(), None);
    }

    #[test]
    fn ids_are_case_normalised() {
        assert_eq!(profile_secret_id(3, "anthropic_api_key"), "profile/3/ANTHROPIC_API_KEY");
    }

    /// Exercises the real OS credential store. Run with `--ignored`.
    #[test]
    #[ignore]
    fn os_vault_round_trip() {
        let v = OsVault;
        let id = format!("selftest/{}", std::process::id());
        v.set(&id, "value-ü").unwrap();
        assert_eq!(v.get(&id).unwrap().as_deref(), Some("value-ü"));
        v.delete(&id).unwrap();
        assert_eq!(v.get(&id).unwrap(), None);
    }
}
