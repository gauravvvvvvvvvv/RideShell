//! Validation of user-supplied identifiers.

use anyhow::{bail, Result};

use crate::brand;

/// Profile names are used in prompts, environment variables and shell code,
/// so they are restricted to a conservative, shell-safe character set.
pub fn validate_profile_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("profile name must not be empty");
    }
    if name.len() > 64 {
        bail!("profile name '{name}' is longer than 64 characters");
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphanumeric() {
        bail!("profile name '{name}' must start with a letter or digit");
    }
    if let Some(bad) = chars.find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))) {
        bail!("profile name '{name}' contains '{bad}'; use letters, digits, '-', '_' or '.'");
    }
    Ok(())
}

/// Environment variable names a profile may set.
pub fn validate_env_key(key: &str) -> Result<()> {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => bail!("environment variable name '{key}' must start with a letter or '_'"),
    }
    if chars.any(|c| !(c.is_ascii_alphanumeric() || c == '_')) {
        bail!("environment variable name '{key}' may only contain letters, digits and '_'");
    }
    let upper = key.to_ascii_uppercase();
    if upper.starts_with(&format!("{}_", brand::ENV_PREFIX)) {
        bail!("'{key}' is reserved for {} itself", brand::APP_NAME);
    }
    if upper == "PATH" {
        bail!("replacing PATH from a profile is not supported (it would break the shell); PATH management is planned");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_names() {
        for ok in ["work", "client-a", "Personal_2", "a.b", "9"] {
            validate_profile_name(ok).unwrap();
        }
        for bad in ["", "-x", "has space", "semi;colon", "quote'", "ünï", &"x".repeat(65)] {
            assert!(validate_profile_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn env_keys() {
        validate_env_key("AWS_PROFILE").unwrap();
        validate_env_key("_x1").unwrap();
        for bad in ["", "1A", "A-B", "A B", "PATH", "Path", "RIDESHELL_PROFILE", "rideshell_x"] {
            assert!(validate_env_key(bad).is_err(), "{bad}");
        }
    }
}
