//! Account-mismatch guard for sensitive commands.
//!
//! A folder can be bound to an account (`profile bind`), and a terminal can
//! switch to another account on purpose (`profile use`, or a click in
//! RideShell App). Usually that is what you want, but pushing code or
//! deploying as the wrong account is hard to undo. So before a *sensitive*
//! command (`git push`, `gh pr create`, `wrangler deploy`...) the tool shim
//! asks for confirmation when the terminal's account differs from the
//! folder's bound account.
//!
//! Patterns are a tool name followed by words that must all appear among the
//! arguments: `git push` matches `git push origin main` and
//! `git -C repo push`; `vercel --prod` matches `vercel deploy --prod`.

/// Commands guarded by default (`[guard] commands` in config.toml).
pub const DEFAULT_COMMANDS: &[&str] = &[
    "git push",
    "gh pr create",
    "gh pr merge",
    "gh release create",
    "gh repo create",
    "gh repo delete",
    "wrangler deploy",
    "wrangler publish",
    "vercel deploy",
    "vercel --prod",
    "netlify deploy",
    "fly deploy",
    "npm publish",
    "pnpm publish",
    "yarn publish",
    "cargo publish",
    "docker push",
    "terraform apply",
    "kubectl apply",
    "firebase deploy",
];

/// Turn the guard off for one command or session: `RIDESHELL_GUARD=0`.
pub const DISABLE_ENV: &str = "RIDESHELL_GUARD";

/// The first pattern that `tool args...` matches.
pub fn matching<'a>(patterns: &'a [String], tool: &str, args: &[String]) -> Option<&'a str> {
    let tool = std::path::Path::new(tool).file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase())?;
    patterns.iter().map(String::as_str).find(|p| {
        let mut words = p.split_whitespace();
        words.next().is_some_and(|t| t.eq_ignore_ascii_case(&tool))
            && words.all(|w| args.iter().any(|a| a.eq_ignore_ascii_case(w)))
    })
}

/// Is the guard switched off by the environment?
pub fn disabled_by_env() -> bool {
    std::env::var(DISABLE_ENV)
        .is_ok_and(|v| v == "0" || v.eq_ignore_ascii_case("off") || v.eq_ignore_ascii_case("false"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> Vec<String> {
        DEFAULT_COMMANDS.iter().map(|s| s.to_string()).collect()
    }
    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn matches_sensitive_commands_only() {
        let p = defaults();
        assert_eq!(matching(&p, "git", &args("push origin main")), Some("git push"));
        assert_eq!(matching(&p, "git.exe", &args("-C repo push")), Some("git push"));
        assert_eq!(matching(&p, "GIT", &args("push")), Some("git push"));
        assert_eq!(matching(&p, "git", &args("pull")), None);
        assert_eq!(matching(&p, "git", &args("log --grep push")), Some("git push"), "conservative: may over-match");
        assert_eq!(matching(&p, "gh", &args("pr create --fill")), Some("gh pr create"));
        assert_eq!(matching(&p, "gh", &args("pr list")), None);
        assert_eq!(matching(&p, "vercel", &args("deploy --prod")), Some("vercel deploy"));
        assert_eq!(matching(&p, "vercel", &args("--prod")), Some("vercel --prod"));
        assert_eq!(matching(&p, "claude", &args("--resume")), None);
        assert_eq!(matching(&[], "git", &args("push")), None);
    }
}
