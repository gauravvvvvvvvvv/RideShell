//! Portable profile files (`profile export` / `profile import`).
//!
//! A profile file contains the profile's name, description, plain environment
//! variables and identity settings. It never contains secrets (they are never
//! stored) and never contains directory bindings (machine-specific).

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::brand;
use crate::store::{Settings, Store};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileFile {
    pub format: String,
    pub profile: ProfileMeta,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub identities: BTreeMap<String, Settings>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

impl ProfileFile {
    pub fn export(store: &Store, name: &str) -> Result<Self> {
        let p = store.require_profile(name)?;
        Ok(Self {
            format: brand::PROFILE_FORMAT.into(),
            profile: ProfileMeta { name: p.name.clone(), description: p.description.clone() },
            env: store.env_vars(p.id)?,
            identities: store.identities(p.id)?,
        })
    }

    pub fn to_toml(&self) -> Result<String> {
        Ok(format!(
            "# {} profile. Contains no secrets and no machine-specific directory bindings.\n{}",
            brand::APP_NAME,
            toml::to_string_pretty(self)?
        ))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let file: Self = toml::from_str(text).context("invalid profile file")?;
        if file.format != brand::PROFILE_FORMAT {
            bail!("unsupported profile file format '{}' (expected '{}')", file.format, brand::PROFILE_FORMAT);
        }
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut s = Store::open_in_memory().unwrap();
        s.create_profile("work", "Company \"quoted\" ü").unwrap();
        s.set_env("work", "AWS_PROFILE", "corp").unwrap();
        s.set_identity("work", "git", &[("email".to_string(), "w@corp.example".to_string())].into()).unwrap();
        let text = ProfileFile::export(&s, "work").unwrap().to_toml().unwrap();
        let parsed = ProfileFile::parse(&text).unwrap();
        assert_eq!(parsed.profile.description, "Company \"quoted\" ü");
        assert_eq!(parsed.env["AWS_PROFILE"], "corp");
        assert_eq!(parsed.identities["git"]["email"], "w@corp.example");
    }

    #[test]
    fn rejects_other_formats() {
        assert!(ProfileFile::parse("format = \"x/9\"\n[profile]\nname = \"a\"\n").is_err());
    }
}
