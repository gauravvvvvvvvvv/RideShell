//! Profile resolution.
//!
//! Precedence (highest first):
//! 1. explicit command-level override (`--profile`)
//! 2. session override (`RIDESHELL_PROFILE`, set by `profile use`)
//! 3. the most specific directory binding containing the working directory
//! 4. the user's default profile
//!
//! Safety rule: if a higher level names a profile that does not exist,
//! resolution does **not** silently fall through to a lower level — using an
//! unintended identity is worse than using none.

use std::path::Path;

use anyhow::Result;

use crate::brand;
use crate::pathkey;
use crate::store::{Binding, Profile, Store};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Flag,
    Session,
    Binding { path: String },
    Default,
    None,
}

impl Source {
    pub fn describe(&self) -> String {
        match self {
            Source::Flag => "command-line --profile".into(),
            Source::Session => format!("session ({})", brand::env::PROFILE),
            Source::Binding { path } => format!("directory binding {path}"),
            Source::Default => "default profile".into(),
            Source::None => "nothing selected".into(),
        }
    }

    pub fn short(&self) -> &'static str {
        match self {
            Source::Flag => "flag",
            Source::Session => "session",
            Source::Binding { .. } => "binding",
            Source::Default => "default",
            Source::None => "none",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    Selected,
    Shadowed,
    NotSet,
    Missing,
}

/// One precedence level and what it contributed, for `profile why`.
#[derive(Debug, Clone)]
pub struct Step {
    pub level: &'static str,
    pub detail: String,
    pub outcome: StepOutcome,
}

#[derive(Debug, Clone)]
pub struct Resolution {
    pub profile: Option<Profile>,
    pub source: Source,
    pub steps: Vec<Step>,
    /// Set when a level named a profile that does not exist.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Request<'a> {
    pub flag: Option<&'a str>,
    pub session: Option<&'a str>,
    /// Working directory; bindings are skipped when `None`.
    pub cwd: Option<&'a Path>,
    pub use_bindings: bool,
}

/// Find the most specific binding containing `dir` (already canonical).
pub fn find_binding(bindings: &[Binding], dir: &Path) -> Option<Binding> {
    let key = pathkey::path_key(dir).ok()?;
    bindings.iter().filter(|b| pathkey::key_contains(&b.path_key, &key)).max_by_key(|b| b.path_key.len()).cloned()
}

pub fn resolve(store: &Store, req: &Request) -> Result<Resolution> {
    let mut candidates: Vec<(&'static str, String, Option<Option<Profile>>, Source)> = Vec::new();

    // 1. Flag.
    match req.flag {
        Some(name) => {
            candidates.push(("command flag", format!("--profile {name}"), Some(store.get_profile(name)?), Source::Flag))
        }
        None => candidates.push(("command flag", "not given".into(), None, Source::Flag)),
    }

    // 2. Session.
    match req.session.filter(|s| !s.is_empty()) {
        Some(name) => candidates.push((
            "session",
            format!("{}={name}", brand::env::PROFILE),
            Some(store.get_profile(name)?),
            Source::Session,
        )),
        None => candidates.push(("session", format!("{} not set", brand::env::PROFILE), None, Source::Session)),
    }

    // 3. Binding.
    if !req.use_bindings {
        candidates.push(("directory binding", "disabled (shell.auto_apply = false)".into(), None, Source::None));
    } else if let Some(cwd) = req.cwd {
        match pathkey::canonical_dir(cwd) {
            Ok(dir) => match find_binding(&store.bindings()?, &dir) {
                Some(b) => {
                    let p = store.get_profile(&b.profile)?;
                    candidates.push((
                        "directory binding",
                        format!("{} -> {}", b.path, b.profile),
                        Some(p),
                        Source::Binding { path: b.path.clone() },
                    ))
                }
                None => {
                    candidates.push(("directory binding", format!("none covers {}", dir.display()), None, Source::None))
                }
            },
            Err(e) => candidates.push(("directory binding", format!("skipped: {e:#}"), None, Source::None)),
        }
    } else {
        candidates.push(("directory binding", "no working directory".into(), None, Source::None));
    }

    // 4. Default.
    match store.default_profile()? {
        Some(p) => candidates.push(("default", p.name.clone(), Some(Some(p)), Source::Default)),
        None => candidates.push(("default", "not set".into(), None, Source::None)),
    }

    let mut steps = Vec::new();
    let mut chosen: Option<(Option<Profile>, Source, Option<String>)> = None;
    for (level, detail, candidate, source) in candidates {
        let outcome = match (&candidate, &chosen) {
            (None, _) => StepOutcome::NotSet,
            (Some(_), Some(_)) => StepOutcome::Shadowed,
            (Some(Some(p)), None) => {
                chosen = Some((Some(p.clone()), source, None));
                StepOutcome::Selected
            }
            (Some(None), None) => {
                let msg = format!("{level} names a profile that does not exist ({detail}); no profile is active");
                chosen = Some((None, source, Some(msg)));
                StepOutcome::Missing
            }
        };
        steps.push(Step { level, detail, outcome });
    }

    let (profile, source, error) = chosen.unwrap_or((None, Source::None, None));
    Ok(Resolution { profile, source, steps, error })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _dir: tempfile::TempDir,
        root: std::path::PathBuf,
        store: Store,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        for sub in ["company/backend/src", "company-other", "client a/ünï"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        let mut store = Store::open_in_memory().unwrap();
        for p in ["personal", "work", "client-a", "backend"] {
            store.create_profile(p, "").unwrap();
        }
        for (path, profile) in [("company", "work"), ("company/backend", "backend"), ("client a", "client-a")] {
            let d = root.join(path);
            store.bind(d.to_str().unwrap(), &pathkey::path_key(&d).unwrap(), profile).unwrap();
        }
        store.set_default_profile(Some("personal")).unwrap();
        Fixture { _dir: dir, root, store }
    }

    fn resolve_in(f: &Fixture, sub: &str, flag: Option<&str>, session: Option<&str>) -> Resolution {
        let cwd = f.root.join(sub);
        resolve(&f.store, &Request { flag, session, cwd: Some(&cwd), use_bindings: true }).unwrap()
    }

    fn name(r: &Resolution) -> Option<&str> {
        r.profile.as_ref().map(|p| p.name.as_str())
    }

    #[test]
    fn most_specific_binding_wins() {
        let f = fixture();
        assert_eq!(name(&resolve_in(&f, "company", None, None)), Some("work"));
        assert_eq!(name(&resolve_in(&f, "company/backend/src", None, None)), Some("backend"));
        assert_eq!(name(&resolve_in(&f, "client a/ünï", None, None)), Some("client-a"));
    }

    #[test]
    fn sibling_with_common_prefix_is_not_matched() {
        let f = fixture();
        let r = resolve_in(&f, "company-other", None, None);
        assert_eq!(name(&r), Some("personal"));
        assert_eq!(r.source, Source::Default);
    }

    #[test]
    fn precedence_flag_session_binding_default() {
        let f = fixture();
        let r = resolve_in(&f, "company", Some("client-a"), Some("personal"));
        assert_eq!((name(&r), &r.source), (Some("client-a"), &Source::Flag));
        let r = resolve_in(&f, "company", None, Some("personal"));
        assert_eq!((name(&r), &r.source), (Some("personal"), &Source::Session));
        let r = resolve_in(&f, "company", None, None);
        assert!(matches!(r.source, Source::Binding { .. }));
        let outcomes: Vec<_> = r.steps.iter().map(|s| s.outcome.clone()).collect();
        assert_eq!(outcomes, [StepOutcome::NotSet, StepOutcome::NotSet, StepOutcome::Selected, StepOutcome::Shadowed]);
    }

    #[test]
    fn missing_session_profile_does_not_fall_through() {
        let f = fixture();
        let r = resolve_in(&f, "company", None, Some("deleted"));
        assert_eq!(name(&r), None);
        assert_eq!(r.source, Source::Session);
        assert!(r.error.is_some());
    }

    #[test]
    fn bindings_can_be_disabled() {
        let f = fixture();
        let cwd = f.root.join("company");
        let r = resolve(&f.store, &Request { cwd: Some(&cwd), use_bindings: false, ..Default::default() }).unwrap();
        assert_eq!(name(&r), Some("personal"));
    }

    #[test]
    fn nothing_configured() {
        let store = Store::open_in_memory().unwrap();
        let r = resolve(&store, &Request::default()).unwrap();
        assert!(r.profile.is_none());
        assert_eq!(r.source, Source::None);
        assert!(r.error.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_directory_resolves_to_target_binding() {
        let f = fixture();
        let link = f.root.join("link-to-company");
        std::os::unix::fs::symlink(f.root.join("company"), &link).unwrap();
        assert_eq!(name(&resolve_in(&f, "link-to-company", None, None)), Some("work"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_matching_is_case_insensitive() {
        let f = fixture();
        let upper = std::path::PathBuf::from(f.root.to_str().unwrap().to_uppercase()).join("COMPANY");
        let r = resolve(&f.store, &Request { cwd: Some(&upper), use_bindings: true, ..Default::default() }).unwrap();
        assert_eq!(name(&r), Some("work"));
    }
}
