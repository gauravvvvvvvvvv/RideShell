//! SQLite-backed storage for profiles, environment, identities and bindings.
//!
//! Concurrency: many terminals may run RideShell at once. The database uses
//! WAL journaling, a busy timeout, and `BEGIN IMMEDIATE` for every
//! read-modify-write sequence so writers serialise instead of failing.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};

use crate::names;

const MIGRATIONS: &[&str] =
    &[include_str!("migrations/0001_initial.sql"), include_str!("migrations/0002_secrets_and_dir_env.sql")];
const DEFAULT_PROFILE_KEY: &str = "default_profile_id";
const PROFILE_COLS: &str = "id, name, description, created_at, updated_at";

pub type Settings = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// Path as displayed to the user (canonical form).
    pub path: String,
    /// Normalised match key (see [`crate::pathkey`]).
    pub path_key: String,
    pub profile: String,
    pub created_at: i64,
}

/// A variable attached to a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirVar {
    pub id: i64,
    pub path: String,
    pub path_key: String,
    pub key: String,
    /// Empty for secrets.
    pub value: String,
    pub secret: bool,
}

const DIR_COLS: &str = "id, path, path_key, key, value, secret";

fn dir_var_from_row(r: &Row) -> rusqlite::Result<DirVar> {
    Ok(DirVar {
        id: r.get(0)?,
        path: r.get(1)?,
        path_key: r.get(2)?,
        key: r.get(3)?,
        value: r.get(4)?,
        secret: r.get::<_, i64>(5)? != 0,
    })
}

pub struct Store {
    conn: Connection,
}

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn profile_from_row(r: &Row) -> rusqlite::Result<Profile> {
    Ok(Profile { id: r.get(0)?, name: r.get(1)?, description: r.get(2)?, created_at: r.get(3)?, updated_at: r.get(4)? })
}

impl Store {
    /// Open (creating if needed) and migrate the database.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("cannot create directory {}", parent.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("cannot open database {}", path.display()))?;
        Self::setup(conn).with_context(|| format!("cannot initialise database {}", path.display()))
    }

    /// Open the database only if it already exists (used by prompt hooks,
    /// which must never create state as a side effect).
    pub fn open_existing(path: &Path) -> Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        Self::open(path).map(Some)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::setup(Connection::open_in_memory()?)
    }

    fn setup(mut conn: Connection) -> Result<Self> {
        conn.busy_timeout(Duration::from_secs(10))?;
        let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") && !mode.eq_ignore_ascii_case("memory") {
            let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        }
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL;")?;
        migrate(&mut conn)?;
        Ok(Self { conn })
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    pub fn latest_schema_version() -> i64 {
        MIGRATIONS.len() as i64
    }

    /// `PRAGMA quick_check`; returns "ok" when healthy.
    pub fn quick_check(&self) -> Result<String> {
        Ok(self.conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?)
    }

    // ----- profiles ------------------------------------------------------

    pub fn get_profile(&self, name: &str) -> Result<Option<Profile>> {
        Ok(self
            .conn
            .query_row(&format!("SELECT {PROFILE_COLS} FROM profiles WHERE name = ?1"), [name], profile_from_row)
            .optional()?)
    }

    pub fn get_profile_by_id(&self, id: i64) -> Result<Option<Profile>> {
        Ok(self
            .conn
            .query_row(&format!("SELECT {PROFILE_COLS} FROM profiles WHERE id = ?1"), [id], profile_from_row)
            .optional()?)
    }

    /// Like [`get_profile`](Self::get_profile) but errors with a helpful message.
    pub fn require_profile(&self, name: &str) -> Result<Profile> {
        if let Some(p) = self.get_profile(name)? {
            return Ok(p);
        }
        let known: Vec<String> = self.list_profiles()?.into_iter().map(|p| p.name).collect();
        if known.is_empty() {
            bail!("profile '{name}' does not exist (no profiles yet; create one with `profile create {name}`)")
        }
        bail!("profile '{name}' does not exist (known profiles: {})", known.join(", "))
    }

    pub fn list_profiles(&self) -> Result<Vec<Profile>> {
        let mut stmt =
            self.conn.prepare(&format!("SELECT {PROFILE_COLS} FROM profiles ORDER BY name COLLATE NOCASE"))?;
        let rows = stmt.query_map([], profile_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn create_profile(&mut self, name: &str, description: &str) -> Result<Profile> {
        names::validate_profile_name(name)?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if profile_id(&tx, name)?.is_some() {
            bail!("a profile named '{name}' already exists");
        }
        let t = now();
        tx.execute(
            "INSERT INTO profiles (name, description, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            params![name, description, t],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(Profile { id, name: name.into(), description: description.into(), created_at: t, updated_at: t })
    }

    pub fn set_description(&mut self, name: &str, description: &str) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE profiles SET description = ?2, updated_at = ?3 WHERE name = ?1",
            params![name, description, now()],
        )?;
        if n == 0 {
            self.require_profile(name)?;
        }
        Ok(())
    }

    pub fn rename_profile(&mut self, old: &str, new: &str) -> Result<Profile> {
        names::validate_profile_name(new)?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = profile_id(&tx, old)?.ok_or_else(|| anyhow!("profile '{old}' does not exist"))?;
        if let Some(other) = profile_id(&tx, new)? {
            if other != id {
                bail!("a profile named '{new}' already exists");
            }
        }
        tx.execute("UPDATE profiles SET name = ?2, updated_at = ?3 WHERE id = ?1", params![id, new, now()])?;
        tx.commit()?;
        self.get_profile_by_id(id)?.ok_or_else(|| anyhow!("profile vanished during rename"))
    }

    /// Copy a profile including its environment and identities (not bindings,
    /// which belong to directories rather than identities).
    pub fn duplicate_profile(&mut self, source: &str, target: &str) -> Result<Profile> {
        names::validate_profile_name(target)?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let src = profile_id(&tx, source)?.ok_or_else(|| anyhow!("profile '{source}' does not exist"))?;
        if profile_id(&tx, target)?.is_some() {
            bail!("a profile named '{target}' already exists");
        }
        let t = now();
        tx.execute(
            "INSERT INTO profiles (name, description, created_at, updated_at)
             SELECT ?2, description, ?3, ?3 FROM profiles WHERE id = ?1",
            params![src, target, t],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO profile_env (profile_id, key, value, secret) SELECT ?2, key, value, secret FROM profile_env WHERE profile_id = ?1",
            params![src, id],
        )?;
        tx.execute(
            "INSERT INTO identities (profile_id, provider, settings, updated_at)
             SELECT ?2, provider, settings, ?3 FROM identities WHERE profile_id = ?1",
            params![src, id, t],
        )?;
        tx.commit()?;
        self.get_profile_by_id(id)?.ok_or_else(|| anyhow!("profile vanished during duplicate"))
    }

    /// Delete a profile. Its env, identities and bindings are removed with it.
    /// Returns the number of directory bindings that were removed.
    pub fn delete_profile(&mut self, name: &str) -> Result<usize> {
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = profile_id(&tx, name)?.ok_or_else(|| anyhow!("profile '{name}' does not exist"))?;
        let bindings: i64 = tx.query_row("SELECT COUNT(*) FROM bindings WHERE profile_id = ?1", [id], |r| r.get(0))?;
        tx.execute("DELETE FROM profiles WHERE id = ?1", [id])?;
        tx.execute("DELETE FROM settings WHERE key = ?1 AND value = ?2", params![DEFAULT_PROFILE_KEY, id.to_string()])?;
        tx.commit()?;
        Ok(bindings as usize)
    }

    /// Create (or with `replace`, overwrite the contents of) a profile in a
    /// single transaction, so an interrupted import never leaves a half-made
    /// profile. Bindings and the default setting of a replaced profile stay.
    pub fn import_profile(
        &mut self,
        name: &str,
        description: &str,
        env: &BTreeMap<String, String>,
        identities: &BTreeMap<String, Settings>,
        replace: bool,
    ) -> Result<Profile> {
        names::validate_profile_name(name)?;
        for key in env.keys() {
            names::validate_env_key(key)?;
        }
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let t = now();
        let id = match profile_id(&tx, name)? {
            Some(_) if !replace => bail!("profile '{name}' already exists"),
            Some(id) => {
                tx.execute(
                    "UPDATE profiles SET description = ?2, updated_at = ?3 WHERE id = ?1",
                    params![id, description, t],
                )?;
                tx.execute("DELETE FROM profile_env WHERE profile_id = ?1", [id])?;
                tx.execute("DELETE FROM identities WHERE profile_id = ?1", [id])?;
                id
            }
            None => {
                tx.execute(
                    "INSERT INTO profiles (name, description, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
                    params![name, description, t],
                )?;
                tx.last_insert_rowid()
            }
        };
        for (k, v) in env {
            tx.execute("INSERT INTO profile_env (profile_id, key, value) VALUES (?1, ?2, ?3)", params![id, k, v])?;
        }
        for (provider, settings) in identities {
            tx.execute(
                "INSERT INTO identities (profile_id, provider, settings, updated_at) VALUES (?1, ?2, ?3, ?4)",
                params![id, provider, serde_json::to_string(settings)?, t],
            )?;
        }
        tx.commit()?;
        self.get_profile_by_id(id)?.ok_or_else(|| anyhow!("profile vanished during import"))
    }

    // ----- default profile -----------------------------------------------

    pub fn default_profile(&self) -> Result<Option<Profile>> {
        let Some(id) = self.setting(DEFAULT_PROFILE_KEY)? else { return Ok(None) };
        let Ok(id) = id.parse::<i64>() else { return Ok(None) };
        self.get_profile_by_id(id)
    }

    pub fn set_default_profile(&mut self, name: Option<&str>) -> Result<()> {
        match name {
            Some(name) => {
                let p = self.require_profile(name)?;
                self.set_setting(DEFAULT_PROFILE_KEY, &p.id.to_string())
            }
            None => self.delete_setting(DEFAULT_PROFILE_KEY),
        }
    }

    // ----- environment ---------------------------------------------------

    /// Plain (non-secret) variables of a profile.
    pub fn env_vars(&self, profile_id: i64) -> Result<BTreeMap<String, String>> {
        let mut stmt = self.conn.prepare("SELECT key, value FROM profile_env WHERE profile_id = ?1 AND secret = 0")?;
        let rows = stmt.query_map([profile_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<BTreeMap<_, _>>>()?)
    }

    /// Names of a profile's secret variables (values live in the vault).
    pub fn secret_keys(&self, profile_id: i64) -> Result<Vec<String>> {
        let mut stmt =
            self.conn.prepare("SELECT key FROM profile_env WHERE profile_id = ?1 AND secret = 1 ORDER BY key")?;
        let rows = stmt.query_map([profile_id], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Is `key` a secret variable of the profile?
    pub fn is_secret(&self, profile_id: i64, key: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT secret FROM profile_env WHERE profile_id = ?1 AND key = ?2",
                params![profile_id, key],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            == Some(1))
    }

    /// Record that `key` is a secret of the profile. The caller stores the
    /// value in the vault under [`crate::vault::profile_secret_id`].
    pub fn set_secret_env(&mut self, profile: &str, key: &str) -> Result<()> {
        self.write_env(profile, key, "", true)
    }

    pub fn set_env(&mut self, profile: &str, key: &str, value: &str) -> Result<()> {
        self.write_env(profile, key, value, false)
    }

    fn write_env(&mut self, profile: &str, key: &str, value: &str, secret: bool) -> Result<()> {
        names::validate_env_key(key)?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = profile_id(&tx, profile)?.ok_or_else(|| anyhow!("profile '{profile}' does not exist"))?;
        // Delete first so a change of case in the key name is honoured.
        tx.execute("DELETE FROM profile_env WHERE profile_id = ?1 AND key = ?2", params![id, key])?;
        tx.execute(
            "INSERT INTO profile_env (profile_id, key, value, secret) VALUES (?1, ?2, ?3, ?4)",
            params![id, key, value, secret as i64],
        )?;
        touch(&tx, id)?;
        tx.commit()?;
        Ok(())
    }

    pub fn unset_env(&mut self, profile: &str, key: &str) -> Result<bool> {
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = profile_id(&tx, profile)?.ok_or_else(|| anyhow!("profile '{profile}' does not exist"))?;
        let n = tx.execute("DELETE FROM profile_env WHERE profile_id = ?1 AND key = ?2", params![id, key])?;
        touch(&tx, id)?;
        tx.commit()?;
        Ok(n > 0)
    }

    // ----- directory (project) variables ---------------------------------

    /// Set a variable on a directory. Returns the row id (stable across
    /// updates; used as the vault id for secrets).
    pub fn set_dir_env(&mut self, path: &str, path_key: &str, key: &str, value: &str, secret: bool) -> Result<i64> {
        names::validate_env_key(key)?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO dir_env (path, path_key, key, value, secret) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (path_key, key) DO UPDATE SET path = excluded.path, key = excluded.key,
                 value = excluded.value, secret = excluded.secret",
            params![path, path_key, key, value, secret as i64],
        )?;
        let id: i64 =
            tx.query_row("SELECT id FROM dir_env WHERE path_key = ?1 AND key = ?2", params![path_key, key], |r| {
                r.get(0)
            })?;
        tx.commit()?;
        Ok(id)
    }

    /// Remove a directory variable; returns the removed row.
    pub fn unset_dir_env(&mut self, path_key: &str, key: &str) -> Result<Option<DirVar>> {
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row = tx
            .query_row(
                &format!("SELECT {DIR_COLS} FROM dir_env WHERE path_key = ?1 AND key = ?2"),
                params![path_key, key],
                dir_var_from_row,
            )
            .optional()?;
        tx.execute("DELETE FROM dir_env WHERE path_key = ?1 AND key = ?2", params![path_key, key])?;
        tx.commit()?;
        Ok(row)
    }

    pub fn dir_vars(&self) -> Result<Vec<DirVar>> {
        let mut stmt = self.conn.prepare(&format!("SELECT {DIR_COLS} FROM dir_env ORDER BY path_key, key"))?;
        let rows = stmt.query_map([], dir_var_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ----- identities ----------------------------------------------------

    pub fn identities(&self, profile_id: i64) -> Result<BTreeMap<String, Settings>> {
        let mut stmt =
            self.conn.prepare("SELECT provider, settings FROM identities WHERE profile_id = ?1 ORDER BY provider")?;
        let rows = stmt.query_map([profile_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (provider, json) = row?;
            let settings: Settings = serde_json::from_str(&json)
                .with_context(|| format!("corrupt identity settings for provider '{provider}'"))?;
            out.insert(provider, settings);
        }
        Ok(out)
    }

    /// Replace the settings of one provider identity for a profile.
    pub fn set_identity(&mut self, profile: &str, provider: &str, settings: &Settings) -> Result<()> {
        let json = serde_json::to_string(settings)?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = profile_id(&tx, profile)?.ok_or_else(|| anyhow!("profile '{profile}' does not exist"))?;
        tx.execute(
            "INSERT INTO identities (profile_id, provider, settings, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (profile_id, provider) DO UPDATE SET settings = excluded.settings, updated_at = excluded.updated_at",
            params![id, provider, json, now()],
        )?;
        touch(&tx, id)?;
        tx.commit()?;
        Ok(())
    }

    pub fn remove_identity(&mut self, profile: &str, provider: &str) -> Result<bool> {
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = profile_id(&tx, profile)?.ok_or_else(|| anyhow!("profile '{profile}' does not exist"))?;
        let n = tx.execute("DELETE FROM identities WHERE profile_id = ?1 AND provider = ?2", params![id, provider])?;
        touch(&tx, id)?;
        tx.commit()?;
        Ok(n > 0)
    }

    // ----- bindings ------------------------------------------------------

    /// Bind a directory to a profile, replacing any existing binding of the
    /// same directory. Returns the previously bound profile, if any.
    pub fn bind(&mut self, path: &str, path_key: &str, profile: &str) -> Result<Option<String>> {
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = profile_id(&tx, profile)?.ok_or_else(|| anyhow!("profile '{profile}' does not exist"))?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT p.name FROM bindings b JOIN profiles p ON p.id = b.profile_id WHERE b.path_key = ?1",
                [path_key],
                |r| r.get(0),
            )
            .optional()?;
        tx.execute(
            "INSERT INTO bindings (path, path_key, profile_id, created_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (path_key) DO UPDATE SET path = excluded.path, profile_id = excluded.profile_id, created_at = excluded.created_at",
            params![path, path_key, id, now()],
        )?;
        tx.commit()?;
        Ok(previous)
    }

    /// Remove the binding of exactly this directory. Returns its profile.
    pub fn unbind(&mut self, path_key: &str) -> Result<Option<String>> {
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT p.name FROM bindings b JOIN profiles p ON p.id = b.profile_id WHERE b.path_key = ?1",
                [path_key],
                |r| r.get(0),
            )
            .optional()?;
        tx.execute("DELETE FROM bindings WHERE path_key = ?1", [path_key])?;
        tx.commit()?;
        Ok(previous)
    }

    pub fn bindings(&self) -> Result<Vec<Binding>> {
        let mut stmt = self.conn.prepare(
            "SELECT b.path, b.path_key, p.name, b.created_at FROM bindings b
             JOIN profiles p ON p.id = b.profile_id ORDER BY b.path_key",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Binding { path: r.get(0)?, path_key: r.get(1)?, profile: r.get(2)?, created_at: r.get(3)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ----- settings ------------------------------------------------------

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn delete_setting(&mut self, key: &str) -> Result<()> {
        self.conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
        Ok(())
    }
}

fn profile_id(conn: &Connection, name: &str) -> Result<Option<i64>> {
    Ok(conn.query_row("SELECT id FROM profiles WHERE name = ?1", [name], |r| r.get(0)).optional()?)
}

fn touch(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("UPDATE profiles SET updated_at = ?2 WHERE id = ?1", params![id, now()])?;
    Ok(())
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let latest = MIGRATIONS.len() as i64;
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current == latest {
        return Ok(());
    }
    if current > latest {
        bail!(
            "database schema version {current} is newer than this build supports ({latest}); \
             please upgrade {}",
            crate::brand::BIN_NAME
        );
    }
    // Re-read inside an IMMEDIATE transaction: another process may have
    // migrated between the check above and acquiring the write lock.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        tx.execute_batch(sql).with_context(|| format!("database migration {} failed", i + 1))?;
        tx.execute_batch(&format!("PRAGMA user_version = {}", i + 1))?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::open_in_memory().unwrap()
    }

    #[test]
    fn create_list_and_case_insensitive_uniqueness() {
        let mut s = store();
        s.create_profile("work", "Company").unwrap();
        s.create_profile("personal", "").unwrap();
        assert!(s.create_profile("WORK", "").is_err());
        let names: Vec<_> = s.list_profiles().unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["personal", "work"]);
        assert_eq!(s.get_profile("Work").unwrap().unwrap().name, "work");
    }

    #[test]
    fn rename_keeps_default_and_bindings() {
        let mut s = store();
        s.create_profile("work", "").unwrap();
        s.set_default_profile(Some("work")).unwrap();
        s.bind("/p", "/p", "work").unwrap();
        s.rename_profile("work", "company").unwrap();
        assert_eq!(s.default_profile().unwrap().unwrap().name, "company");
        assert_eq!(s.bindings().unwrap()[0].profile, "company");
        // Changing only the case of a name is allowed.
        s.rename_profile("company", "Company").unwrap();
    }

    #[test]
    fn duplicate_copies_env_and_identities_not_bindings() {
        let mut s = store();
        s.create_profile("work", "desc").unwrap();
        s.set_env("work", "AWS_PROFILE", "corp").unwrap();
        let mut git = Settings::new();
        git.insert("name".into(), "W".into());
        s.set_identity("work", "git", &git).unwrap();
        s.bind("/p", "/p", "work").unwrap();
        let copy = s.duplicate_profile("work", "work2").unwrap();
        assert_eq!(copy.description, "desc");
        assert_eq!(s.env_vars(copy.id).unwrap()["AWS_PROFILE"], "corp");
        assert_eq!(s.identities(copy.id).unwrap()["git"]["name"], "W");
        assert_eq!(s.bindings().unwrap().len(), 1);
    }

    #[test]
    fn delete_cascades_and_clears_default() {
        let mut s = store();
        s.create_profile("work", "").unwrap();
        s.set_env("work", "A", "1").unwrap();
        s.bind("/p", "/p", "work").unwrap();
        s.set_default_profile(Some("work")).unwrap();
        assert_eq!(s.delete_profile("work").unwrap(), 1);
        assert!(s.default_profile().unwrap().is_none());
        assert!(s.bindings().unwrap().is_empty());
        let rows: i64 = s.conn.query_row("SELECT COUNT(*) FROM profile_env", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn rebinding_replaces() {
        let mut s = store();
        s.create_profile("a", "").unwrap();
        s.create_profile("b", "").unwrap();
        assert_eq!(s.bind("/p", "/p", "a").unwrap(), None);
        assert_eq!(s.bind("/p", "/p", "b").unwrap(), Some("a".into()));
        assert_eq!(s.bindings().unwrap().len(), 1);
        assert_eq!(s.unbind("/p").unwrap(), Some("b".into()));
        assert_eq!(s.unbind("/p").unwrap(), None);
    }

    #[test]
    fn env_key_case_change_replaces() {
        let mut s = store();
        let p = s.create_profile("a", "").unwrap();
        s.set_env("a", "foo", "1").unwrap();
        s.set_env("a", "FOO", "2").unwrap();
        let env = s.env_vars(p.id).unwrap();
        assert_eq!(env.len(), 1);
        assert_eq!(env["FOO"], "2");
    }

    #[test]
    fn secret_rows_hold_no_value() {
        let mut s = store();
        let p = s.create_profile("a", "").unwrap();
        s.set_env("a", "PLAIN", "1").unwrap();
        s.set_secret_env("a", "API_KEY").unwrap();
        assert_eq!(s.env_vars(p.id).unwrap().len(), 1, "secrets are not plain vars");
        assert_eq!(s.secret_keys(p.id).unwrap(), ["API_KEY"]);
        assert!(s.is_secret(p.id, "api_key").unwrap());
        let value: String =
            s.conn.query_row("SELECT value FROM profile_env WHERE key = 'API_KEY'", [], |r| r.get(0)).unwrap();
        assert_eq!(value, "");
        // Turning a secret back into a plain value replaces the row.
        s.set_env("a", "API_KEY", "not secret").unwrap();
        assert!(s.secret_keys(p.id).unwrap().is_empty());
    }

    #[test]
    fn dir_vars_upsert_keeps_id() {
        let mut s = store();
        let id1 = s.set_dir_env("/p", "/p", "KEY", "1", false).unwrap();
        let id2 = s.set_dir_env("/p", "/p", "key", "", true).unwrap();
        assert_eq!(id1, id2);
        let vars = s.dir_vars().unwrap();
        assert_eq!(vars.len(), 1);
        assert!(vars[0].secret);
        assert_eq!(s.unset_dir_env("/p", "KEY").unwrap().unwrap().id, id1);
        assert!(s.dir_vars().unwrap().is_empty());
    }

    #[test]
    fn file_database_reopens_and_migrates_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub dir ü").join("db.sqlite");
        {
            let mut s = Store::open(&path).unwrap();
            s.create_profile("x", "").unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), Store::latest_schema_version());
        assert_eq!(s.list_profiles().unwrap().len(), 1);
        assert_eq!(s.quick_check().unwrap(), "ok");
    }

    #[test]
    fn concurrent_writers_do_not_fail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        Store::open(&path).unwrap();
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let mut s = Store::open(&path).unwrap();
                    for j in 0..10 {
                        s.create_profile(&format!("p{i}-{j}"), "").unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(Store::open(&path).unwrap().list_profiles().unwrap().len(), 80);
    }
}
