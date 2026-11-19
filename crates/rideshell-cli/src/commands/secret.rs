//! `rideshell secret` and `rideshell project env`.

use std::io::{IsTerminal, Read};
use std::path::Path;

use anyhow::{bail, Context, Result};
use rideshell_core::vault::{self, Vault};
use rideshell_core::{brand, names, pathkey};

use super::profile::check_not_secret;
use super::Ctx;
use crate::cli::{ProjectCommand, ProjectEnvCommand, Scope, SecretCommand};
use crate::ui;

pub fn run(ctx: &Ctx, cmd: SecretCommand) -> Result<i32> {
    match cmd {
        SecretCommand::Set { key, scope, stdin } => {
            names::validate_env_key(&key)?;
            let value = read_secret(&key, stdin)?;
            set(ctx, &scope, &key, &value)?;
            Ok(0)
        }
        SecretCommand::Unset { key, scope } => unset(ctx, &scope, &key),
        SecretCommand::List => list(ctx),
    }
}

/// Prompt without echo, or read stdin (for scripts: `... | rideshell secret set K --stdin`).
pub fn read_secret(key: &str, stdin: bool) -> Result<String> {
    let value = if stdin || !std::io::stdin().is_terminal() {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s).context("cannot read the secret from stdin")?;
        s.trim_end_matches(['\r', '\n']).to_string()
    } else {
        rpassword::prompt_password(format!("Value for {key} (input hidden): ")).context("cannot read the secret")?
    };
    if value.is_empty() {
        bail!("empty value; nothing stored");
    }
    Ok(value)
}

/// Store a secret for a profile or directory.
pub fn set(ctx: &Ctx, scope: &Scope, key: &str, value: &str) -> Result<()> {
    let v = vault::open();
    let mut store = ctx.store()?;
    match (&scope.profile, &scope.dir) {
        (Some(profile), _) => {
            let p = store.require_profile(profile)?;
            // Vault first: if it fails, the database is left unchanged.
            v.set(&vault::profile_secret_id(p.id, key), value)?;
            store.set_secret_env(&p.name, key)?;
            ui::success(format!("stored {key} for profile {} in {}", ui::accent(&p.name), v.backend()));
        }
        (None, Some(dir)) => {
            let (display, key_path) = dir_key(dir)?;
            // Row first (to get the stable id), then the vault; roll back the row on failure.
            let id = store.set_dir_env(&display, &key_path, key, "", true)?;
            if let Err(e) = v.set(&vault::dir_secret_id(id), value) {
                let _ = store.unset_dir_env(&key_path, key);
                return Err(e);
            }
            ui::success(format!("stored {key} for {display} in {}", v.backend()));
        }
        (None, None) => unreachable!("clap requires a scope"),
    }
    ui::hint("given only to programs started via shims or `rideshell run`, never exported into your shell");
    Ok(())
}

fn dir_key(dir: &Path) -> Result<(String, String)> {
    let d = pathkey::canonical_dir(dir)?;
    Ok((d.to_string_lossy().into_owned(), pathkey::path_key(&d)?))
}

fn unset(ctx: &Ctx, scope: &Scope, key: &str) -> Result<i32> {
    let v = vault::open();
    let mut store = ctx.store()?;
    match (&scope.profile, &scope.dir) {
        (Some(profile), _) => {
            let p = store.require_profile(profile)?;
            if !store.is_secret(p.id, key)? {
                ui::warn(format!("{} has no secret {key}", p.name));
                return Ok(1);
            }
            store.unset_env(&p.name, key)?;
            v.delete(&vault::profile_secret_id(p.id, key))?;
            ui::success(format!("deleted {key} from profile {}", p.name));
        }
        (None, Some(dir)) => {
            let (display, key_path) = dir_key(dir)?;
            match store.unset_dir_env(&key_path, key)? {
                Some(row) if row.secret => {
                    v.delete(&vault::dir_secret_id(row.id))?;
                    ui::success(format!("deleted {key} from {display}"));
                }
                Some(_) => ui::success(format!("removed variable {key} from {display}")),
                None => {
                    ui::warn(format!("{display} has no {key}"));
                    return Ok(1);
                }
            }
        }
        (None, None) => unreachable!(),
    }
    Ok(0)
}

fn list(ctx: &Ctx) -> Result<i32> {
    let store = ctx.store()?;
    let mut rows = vec![vec![ui::dim("SECRET"), ui::dim("SCOPE")]];
    for p in store.list_profiles()? {
        for k in store.secret_keys(p.id)? {
            rows.push(vec![k, format!("profile {}", ui::accent(&p.name))]);
        }
    }
    for d in store.dir_vars()?.into_iter().filter(|d| d.secret) {
        rows.push(vec![d.key, format!("directory {}", d.path)]);
    }
    if rows.len() == 1 {
        anstream::println!("No secrets stored. Add one with `{} secret set <KEY> --profile <name>`.", brand::BIN_NAME);
    } else {
        ui::table(&rows);
        ui::hint(format!("values are kept in {} and never displayed", vault::open().backend()));
    }
    Ok(0)
}

/// Delete all vault entries of a profile (after the profile was deleted).
pub fn purge_profile_secrets(v: &dyn Vault, profile_id: i64, keys: &[String]) {
    for k in keys {
        if let Err(e) = v.delete(&vault::profile_secret_id(profile_id, k)) {
            ui::warn(format!("{e:#}"));
        }
    }
}

pub fn project(ctx: &Ctx, cmd: ProjectCommand) -> Result<i32> {
    let ProjectCommand::Env(cmd) = cmd;
    let mut store = ctx.store()?;
    match cmd {
        ProjectEnvCommand::Set { key, value, dir, not_secret } => {
            names::validate_env_key(&key)?;
            check_not_secret(&key, &value, not_secret)?;
            let (display, key_path) = dir_key(&dir)?;
            // If it used to be a secret, remove the vault copy.
            if let Some(old) =
                store.dir_vars()?.into_iter().find(|d| d.path_key == key_path && d.key.eq_ignore_ascii_case(&key))
            {
                if old.secret {
                    vault::open().delete(&vault::dir_secret_id(old.id))?;
                }
            }
            store.set_dir_env(&display, &key_path, &key, &value, false)?;
            ui::success(format!("{display} sets {key}"));
        }
        ProjectEnvCommand::Unset { key, dir } => {
            let (display, key_path) = dir_key(&dir)?;
            match store.unset_dir_env(&key_path, &key)? {
                Some(row) => {
                    if row.secret {
                        vault::open().delete(&vault::dir_secret_id(row.id))?;
                    }
                    ui::success(format!("removed {key} from {display}"));
                }
                None => {
                    ui::warn(format!("{display} has no {key}"));
                    return Ok(1);
                }
            }
        }
        ProjectEnvCommand::List => {
            let vars = store.dir_vars()?;
            if vars.is_empty() {
                anstream::println!("No directory variables.");
            }
            let mut rows = vec![vec![ui::dim("DIRECTORY"), ui::dim("VARIABLE"), ui::dim("VALUE")]];
            for v in vars {
                let value = if v.secret { ui::paint("•••••• (secret)", ui::WARN) } else { v.value };
                rows.push(vec![v.path, v.key, value]);
            }
            ui::table(&rows);
        }
    }
    Ok(0)
}
