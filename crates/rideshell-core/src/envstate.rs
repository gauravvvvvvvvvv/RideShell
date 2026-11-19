//! Reversible environment changes.
//!
//! When RideShell applies a profile inside a shell it records, in
//! `RIDESHELL_STATE`, every variable it set together with the value that
//! variable had *before* RideShell touched it. Switching to another profile
//! (or to none) first restores those originals, so profiles never leak into
//! each other and the user's own environment comes back intact.
//!
//! The state lives in the process environment, which is what makes sessions
//! independent: each terminal has its own copy and nothing global is mutated.

use std::collections::BTreeMap;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::brand;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvState {
    /// Profile whose environment is applied.
    #[serde(default, rename = "p", skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Variables set by RideShell.
    #[serde(default, rename = "a")]
    pub applied: BTreeMap<String, Applied>,
}

/// One variable RideShell set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Applied {
    /// Value before RideShell set it (`None`: it was unset).
    #[serde(rename = "o")]
    pub original: Option<String>,
    /// Fingerprint (FNV-1a) of the value RideShell set — never the value
    /// itself, so secrets do not end up in `RIDESHELL_STATE`. If the variable
    /// no longer matches, the user changed it by hand and that is respected.
    #[serde(rename = "h")]
    pub hash: String,
}

/// FNV-1a 64-bit, hex. Used only to detect manual changes.
pub fn fingerprint(value: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in value.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

impl EnvState {
    pub fn from_process_env() -> Self {
        Self::parse(std::env::var(brand::env::STATE).ok().as_deref())
    }

    /// Unparseable state is treated as empty rather than failing the prompt.
    pub fn parse(s: Option<&str>) -> Self {
        s.and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default()
    }

    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.profile.is_none() && self.applied.is_empty()
    }
}

/// A set of environment modifications.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvPlan {
    pub set: Vec<(String, String)>,
    pub unset: Vec<String>,
}

impl EnvPlan {
    pub fn is_empty(&self) -> bool {
        self.set.is_empty() && self.unset.is_empty()
    }

    pub fn apply_to_command(&self, cmd: &mut Command) {
        for key in &self.unset {
            cmd.env_remove(key);
        }
        for (k, v) in &self.set {
            cmd.env(k, v);
        }
    }

    pub fn apply_to_map(&self, env: &mut BTreeMap<String, String>) {
        for key in &self.unset {
            env.remove(key);
        }
        for (k, v) in &self.set {
            env.insert(k.clone(), v.clone());
        }
    }
}

/// Plan the transition from the environment described by `lookup` + `old`
/// to one where exactly `desired` is applied on behalf of `profile`.
///
/// Returns the plan and the new state (which the plan also writes into
/// `RIDESHELL_STATE` / `RIDESHELL_ACTIVE_PROFILE`).
pub fn plan(
    lookup: impl Fn(&str) -> Option<String>,
    old: &EnvState,
    desired: &BTreeMap<String, String>,
    profile: Option<&str>,
) -> (EnvPlan, EnvState) {
    let mut plan = EnvPlan::default();
    let mut new = EnvState { profile: profile.map(str::to_owned), applied: BTreeMap::new() };

    // Was `key` still holding the value RideShell set? If not, the user
    // changed it by hand and we treat their value as authoritative.
    let untouched = |key: &str, a: &Applied| lookup(key).is_some_and(|v| fingerprint(&v) == a.hash);

    // 1. Restore variables we set earlier that the new profile does not set.
    for (key, applied) in &old.applied {
        if desired.contains_key(key) || !untouched(key, applied) {
            continue;
        }
        match &applied.original {
            Some(v) => plan.set.push((key.clone(), v.clone())),
            None => plan.unset.push(key.clone()),
        }
    }

    // 2. Apply the desired variables, remembering their pre-RideShell value.
    for (key, value) in desired {
        let original = match old.applied.get(key) {
            Some(a) if untouched(key, a) => a.original.clone(),
            _ => lookup(key),
        };
        new.applied.insert(key.clone(), Applied { original, hash: fingerprint(value) });
        if lookup(key).as_deref() != Some(value.as_str()) {
            plan.set.push((key.clone(), value.clone()));
        }
    }

    // 3. Bookkeeping variables.
    let encoded = (!new.is_empty()).then(|| new.encode());
    reconcile(&mut plan, brand::env::STATE, encoded, &lookup);
    reconcile(&mut plan, brand::env::ACTIVE_PROFILE, profile.map(str::to_owned), &lookup);

    (plan, new)
}

fn reconcile(plan: &mut EnvPlan, key: &str, want: Option<String>, lookup: &impl Fn(&str) -> Option<String>) {
    let have = lookup(key);
    match want {
        Some(v) if have.as_deref() != Some(v.as_str()) => plan.set.push((key.to_string(), v)),
        None if have.is_some() => plan.unset.push(key.to_string()),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Env = BTreeMap<String, String>;

    fn env(pairs: &[(&str, &str)]) -> Env {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    /// Apply a profile to `current` the way a shell hook would.
    fn switch(current: &mut Env, desired: &Env, profile: Option<&str>) -> EnvPlan {
        let old = EnvState::parse(current.get(brand::env::STATE).map(String::as_str));
        let snapshot = current.clone();
        let (plan, _) = plan(|k| snapshot.get(k).cloned(), &old, desired, profile);
        plan.apply_to_map(current);
        plan
    }

    fn user_visible(e: &Env) -> Env {
        e.iter().filter(|(k, _)| !k.starts_with(brand::ENV_PREFIX)).map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    #[test]
    fn switching_restores_original_values() {
        let original = env(&[("HOME", "/h"), ("GIT_AUTHOR_NAME", "Mine")]);
        let mut current = original.clone();
        let work = env(&[("GIT_AUTHOR_NAME", "Work"), ("AWS_PROFILE", "corp")]);
        let personal = env(&[("AWS_PROFILE", "me")]);

        switch(&mut current, &work, Some("work"));
        assert_eq!(current["GIT_AUTHOR_NAME"], "Work");
        assert_eq!(current["AWS_PROFILE"], "corp");
        assert_eq!(current[brand::env::ACTIVE_PROFILE], "work");

        switch(&mut current, &personal, Some("personal"));
        assert_eq!(current["GIT_AUTHOR_NAME"], "Mine", "work's value must not leak");
        assert_eq!(current["AWS_PROFILE"], "me");

        switch(&mut current, &Env::new(), None);
        assert_eq!(current, original, "deactivating returns the exact original environment");
    }

    #[test]
    fn reapplying_is_a_no_op() {
        let mut current = env(&[("X", "1")]);
        let work = env(&[("AWS_PROFILE", "corp")]);
        switch(&mut current, &work, Some("work"));
        let second = switch(&mut current, &work, Some("work"));
        assert!(second.is_empty(), "{second:?}");
    }

    #[test]
    fn empty_profile_still_records_active_profile() {
        let mut current = Env::new();
        switch(&mut current, &Env::new(), Some("empty"));
        assert_eq!(current[brand::env::ACTIVE_PROFILE], "empty");
        switch(&mut current, &Env::new(), None);
        assert!(current.is_empty());
    }

    #[test]
    fn many_switches_never_accumulate() {
        let base = env(&[("A", "base")]);
        let mut current = base.clone();
        let p1 = env(&[("A", "one"), ("B", "one")]);
        let p2 = env(&[("C", "two")]);
        for _ in 0..5 {
            switch(&mut current, &p1, Some("p1"));
            switch(&mut current, &p2, Some("p2"));
        }
        assert_eq!(user_visible(&current), env(&[("A", "base"), ("C", "two")]));
    }

    #[test]
    fn values_changed_by_hand_are_respected() {
        let mut current = Env::new();
        let work = env(&[("GIT_AUTHOR_NAME", "Work")]);
        switch(&mut current, &work, Some("work"));
        // The user overrides the variable while `work` is active.
        current.insert("GIT_AUTHOR_NAME".into(), "Mine".into());
        switch(&mut current, &Env::new(), None);
        assert_eq!(current.get("GIT_AUTHOR_NAME").map(String::as_str), Some("Mine"));
        // ...and it becomes the original that later profiles restore to.
        switch(&mut current, &work, Some("work"));
        assert_eq!(current["GIT_AUTHOR_NAME"], "Work");
        switch(&mut current, &Env::new(), None);
        assert_eq!(current["GIT_AUTHOR_NAME"], "Mine");
    }

    #[test]
    fn state_never_contains_applied_values() {
        let mut current = Env::new();
        switch(&mut current, &env(&[("ANTHROPIC_API_KEY", "sk-very-secret")]), Some("work"));
        assert!(!current[brand::env::STATE].contains("sk-very-secret"));
    }

    #[test]
    fn corrupt_state_is_ignored() {
        assert_eq!(EnvState::parse(Some("{not json")), EnvState::default());
        assert_eq!(EnvState::parse(None), EnvState::default());
    }
}
