//! Path normalisation for directory bindings.
//!
//! Bindings are matched on a normalised *key*:
//! * the directory is canonicalised (symlinks / junctions resolved, `..`
//!   removed) so a project reached through a link resolves to one binding;
//! * on Windows `\` becomes `/` and `\\?\` prefixes are dropped (via `dunce`);
//! * on case-insensitive platforms (Windows, macOS default) keys are
//!   lower-cased. macOS volumes *can* be case-sensitive; this is a documented
//!   limitation.
//!
//! Matching is component-wise: `/a/bc` is not inside `/a/b`.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

pub const CASE_INSENSITIVE: bool = cfg!(any(windows, target_os = "macos"));

/// Resolve `p` (relative to the process working directory) to a canonical,
/// existing directory.
pub fn canonical_dir(p: &Path) -> Result<PathBuf> {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().context("cannot determine the current directory")?.join(p)
    };
    let canonical = dunce::canonicalize(&abs).with_context(|| format!("cannot resolve path {}", abs.display()))?;
    if !canonical.is_dir() {
        bail!("{} is not a directory", canonical.display());
    }
    Ok(canonical)
}

/// Normalised key of an already-canonical path.
pub fn path_key(p: &Path) -> Result<String> {
    let s = p.to_str().ok_or_else(|| anyhow!("path is not valid UTF-8: {}", p.display()))?;
    Ok(key_from_str(s, cfg!(windows), CASE_INSENSITIVE))
}

pub fn key_from_str(s: &str, windows: bool, case_insensitive: bool) -> String {
    let mut k = if windows { s.replace('\\', "/") } else { s.to_string() };
    while k.len() > 1 && k.ends_with('/') && !is_root(&k) {
        k.pop();
    }
    if case_insensitive {
        k = k.to_lowercase();
    }
    k
}

fn is_root(k: &str) -> bool {
    k == "/" || (k.len() == 3 && k.as_bytes()[1] == b':' && k.ends_with('/'))
}

/// Does the directory identified by `child` lie at or below `parent`?
pub fn key_contains(parent: &str, child: &str) -> bool {
    if child == parent {
        return true;
    }
    if !child.starts_with(parent) {
        return false;
    }
    parent.ends_with('/') || child.as_bytes()[parent.len()] == b'/'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_keys() {
        assert_eq!(key_from_str(r"C:\Users\Dev\Proj\", true, true), "c:/users/dev/proj");
        assert_eq!(key_from_str(r"C:\", true, true), "c:/");
        assert_eq!(key_from_str(r"\\server\share\x", true, true), "//server/share/x");
    }

    #[test]
    fn unix_keys_keep_backslash_and_case() {
        assert_eq!(key_from_str("/home/Dev/a\\b/", false, false), "/home/Dev/a\\b");
        assert_eq!(key_from_str("/", false, false), "/");
    }

    #[test]
    fn unicode_and_spaces() {
        assert_eq!(key_from_str(r"C:\Projets\Été Ünïcode", true, true), "c:/projets/été ünïcode");
    }

    #[test]
    fn containment_is_component_wise() {
        assert!(key_contains("/a/b", "/a/b"));
        assert!(key_contains("/a/b", "/a/b/c"));
        assert!(!key_contains("/a/b", "/a/bc"));
        assert!(key_contains("/", "/a"));
        assert!(key_contains("c:/", "c:/x"));
        assert!(!key_contains("/a/b/c", "/a/b"));
    }
}
