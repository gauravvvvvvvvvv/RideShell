//! Heuristics that stop secrets from being stored in RideShell's database.
//!
//! RideShell stores *references* to identities (profile names, file paths,
//! account names) — never credentials. These checks catch common mistakes; they
//! are not a guarantee and the documentation says so.

/// Name segments (split on `_`) that indicate a credential.
const SECRET_WORDS: &[&str] = &[
    "TOKEN",
    "SECRET",
    "SECRETS",
    "PASSWORD",
    "PASSWD",
    "PASS",
    "PASSPHRASE",
    "CREDENTIAL",
    "CREDENTIALS",
    "APIKEY",
    "AUTH",
    "COOKIE",
    "PAT",
];

/// Adjacent segment pairs that indicate a credential (`API_KEY`, `PRIVATE_KEY`...).
const SECRET_PAIRS: &[(&str, &str)] = &[
    ("API", "KEY"),
    ("PRIVATE", "KEY"),
    ("ACCESS", "KEY"),
    ("SECRET", "KEY"),
    ("SESSION", "KEY"),
    ("SIGNING", "KEY"),
    ("CLIENT", "SECRET"),
];

/// Returns true when an environment variable *name* suggests it holds a secret.
pub fn looks_like_secret_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    let parts: Vec<&str> = upper.split('_').filter(|p| !p.is_empty()).collect();
    if parts.iter().any(|p| SECRET_WORDS.contains(p)) {
        return true;
    }
    parts.windows(2).any(|w| SECRET_PAIRS.contains(&(w[0], w[1])))
}

/// Returns true when a *value* has the shape of a well-known credential.
pub fn looks_like_secret_value(value: &str) -> bool {
    let v = value.trim();
    const PREFIXES: &[&str] = &[
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "github_pat_",
        "glpat-",
        "sk-",
        "sk_live_",
        "rk_live_",
        "xoxb-",
        "xoxp-",
        "xoxa-",
        "AIza",
        "-----BEGIN",
    ];
    if PREFIXES.iter().any(|p| v.starts_with(p)) && v.len() >= 16 {
        return true;
    }
    // AWS access key ids: AKIA/ASIA + 16 upper-case alphanumerics.
    if v.len() == 20
        && (v.starts_with("AKIA") || v.starts_with("ASIA"))
        && v.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_names() {
        for k in ["GITHUB_TOKEN", "API_KEY", "aws_secret_access_key", "DB_PASSWORD", "NPM_AUTH", "MY_PRIVATE_KEY"] {
            assert!(looks_like_secret_key(k), "{k}");
        }
        for k in ["AWS_PROFILE", "GIT_AUTHOR_NAME", "CLOUDSDK_ACTIVE_CONFIG_NAME", "KEYBOARD", "PASSAGE_DIR"] {
            assert!(!looks_like_secret_key(k), "{k}");
        }
    }

    #[test]
    fn secret_values() {
        assert!(looks_like_secret_value("ghp_abcdefghijklmnopqrstuvwxyz0123456789"));
        assert!(looks_like_secret_value("AKIAIOSFODNN7EXAMPLE"));
        assert!(!looks_like_secret_value("company-production"));
        assert!(!looks_like_secret_value("sk-")); // too short to be a key
    }
}
