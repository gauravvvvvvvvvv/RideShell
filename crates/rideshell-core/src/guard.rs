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
//! A pattern is a tool name, then subcommand words, then optional flags:
//! `git push`, `gh pr create`, `vercel --prod`.
//!
//! * Subcommand words must be the command's **leading subcommands, in
//!   order**. `git push` matches `git push origin main` and
//!   `git -C repo push`, but not `git log --grep push`.
//! * Flag words (starting with `-`) may appear anywhere: `vercel --prod`
//!   matches `vercel deploy --prod` and `vercel --prod=true`.
//!
//! Options before a subcommand are skipped. For options this module doesn't
//! know, it can't tell whether the next word is the option's value or the
//! subcommand, so it errs towards guarding: `git --foo push` matches.

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

/// Options that take the next word as their value (`git -C repo push`).
const VALUE_OPTIONS: &[&str] = &[
    // git
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--config-env",
    "--super-prefix",
    // gh
    "-R",
    "--repo",
    // docker
    "-H",
    "--host",
    "--context",
    "--config",
    "-l",
    "--log-level",
    "--tlscacert",
    "--tlscert",
    "--tlskey",
    // kubectl
    "-n",
    "--namespace",
    "--kubeconfig",
    "--cluster",
    "--user",
    "-s",
    "--server",
    "--token",
    // cargo
    "-Z",
    "--color",
    // npm / pnpm / yarn
    "-w",
    "--workspace",
    "--prefix",
    "--dir",
    "--filter",
    "--cwd",
    "--registry",
    // deploy CLIs
    "--scope",
    "-S",
    "-t",
    "-A",
    "--local-config",
    "-Q",
    "--global-config",
    "--project",
    "-a",
    "--app",
];

/// Options known to take no value, so the next word is a subcommand
/// (`git --no-pager log --grep push` is not a push).
const FLAG_OPTIONS: &[&str] = &[
    "-p",
    "--paginate",
    "-P",
    "--no-pager",
    "--bare",
    "--no-replace-objects",
    "--no-lazy-fetch",
    "--literal-pathspecs",
    "--glob-pathspecs",
    "--noglob-pathspecs",
    "--icase-pathspecs",
    "--no-optional-locks",
    "--debug",
    "--verbose",
    "-q",
    "--quiet",
    "--no-color",
];

/// Other spellings of a guarded command, matched as the command itself:
/// `docker image push` is `docker push`. Built in (not part of the config
/// list), so configs written by older versions are covered too.
const SAME_AS: &[(&str, &[&str], &[&str])] = &[
    ("docker", &["image", "push"], &["push"]),
    ("docker", &["compose", "push"], &["push"]),
    ("wrangler", &["versions", "deploy"], &["deploy"]),
    ("wrangler", &["pages", "deploy"], &["deploy"]),
    ("wrangler", &["pages", "publish"], &["publish"]),
];

/// Tools whose plain invocation (no subcommand, or a folder) deploys:
/// `vercel` and `vercel ./site` are `vercel deploy`.
const DEPLOYS_BY_DEFAULT: &[(&str, &str)] = &[("vercel", "deploy")];

/// The first pattern that `tool args...` matches.
pub fn matching<'a>(patterns: &'a [String], tool: &str, args: &[String]) -> Option<&'a str> {
    let tool = std::path::Path::new(tool).file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase())?;
    let forms = forms_of(&tool, args);
    patterns.iter().map(String::as_str).find(|p| {
        let mut words = p.split_whitespace();
        if !words.next().is_some_and(|t| t.eq_ignore_ascii_case(&tool)) {
            return false;
        }
        let (flags, subcommands): (Vec<&str>, Vec<&str>) = words.partition(|w| is_option(w));
        flags.iter().all(|f| args.iter().any(|a| option_is(a, f)))
            && forms.iter().any(|f| lead(f, &subcommands).is_some())
    })
}

/// `args` as written, plus any built-in equivalent spelling.
fn forms_of(tool: &str, args: &[String]) -> Vec<Vec<String>> {
    let mut forms = vec![args.to_vec()];
    for (t, from, to) in SAME_AS {
        if *t == tool {
            if let Some(end) = lead(args, from) {
                forms.push(to.iter().map(|w| w.to_string()).chain(args[end..].iter().cloned()).collect());
            }
        }
    }
    for (t, sub) in DEPLOYS_BY_DEFAULT {
        if *t == tool && deploys_by_default(args) {
            forms.push(std::iter::once(sub.to_string()).chain(args.iter().cloned()).collect());
        }
    }
    forms
}

/// No subcommand (or a folder to deploy), and not just asking for help.
fn deploys_by_default(args: &[String]) -> bool {
    if args.iter().any(|a| ["-h", "--help", "-v", "--version"].iter().any(|o| a.eq_ignore_ascii_case(o))) {
        return false;
    }
    match first_operand(args) {
        None => true,
        Some(a) => std::path::Path::new(a).is_dir(),
    }
}

fn is_option(word: &str) -> bool {
    word.len() > 1 && word.starts_with('-')
}

/// `arg` is the option `name`, alone or as `name=value`.
fn option_is(arg: &str, name: &str) -> bool {
    arg.eq_ignore_ascii_case(name)
        || arg.len() > name.len() && arg.as_bytes()[name.len()] == b'=' && arg[..name.len()].eq_ignore_ascii_case(name)
}

/// How a word before the subcommand reads.
enum Word {
    /// An option; `true` when the next word is surely its value, `None`
    /// when it may or may not be.
    Option(Option<bool>),
    End,
    Operand,
}

fn classify(arg: &str) -> Word {
    if arg == "--" {
        return Word::End;
    }
    // `+toolchain` (cargo +nightly publish) works like an option.
    if !(is_option(arg) || arg.starts_with('+')) {
        return Word::Operand;
    }
    if arg.contains('=') || arg.starts_with('+') || FLAG_OPTIONS.contains(&arg) {
        Word::Option(Some(false))
    } else if VALUE_OPTIONS.contains(&arg) {
        Word::Option(Some(true))
    } else {
        Word::Option(None)
    }
}

/// If the command's leading subcommands (options skipped) start with
/// `words`, the index just after the last of them.
fn lead(args: &[String], words: &[&str]) -> Option<usize> {
    if words.is_empty() {
        return Some(0);
    }
    let mut next = 0;
    // The previous word was an option of unknown kind: this word may be its value.
    let mut maybe_value = false;
    let mut skip_value = false;
    for (i, arg) in args.iter().enumerate() {
        if std::mem::take(&mut skip_value) {
            continue;
        }
        match classify(arg) {
            Word::End => return None, // the rest are operands, not subcommands
            Word::Option(takes_value) => {
                skip_value = takes_value == Some(true);
                maybe_value = takes_value.is_none();
            }
            Word::Operand => {
                if arg.eq_ignore_ascii_case(words[next]) {
                    next += 1;
                    if next == words.len() {
                        return Some(i + 1);
                    }
                } else if !maybe_value {
                    return None; // a different subcommand
                }
                maybe_value = false;
            }
        }
    }
    None
}

/// The first word that is surely not an option or an option's value.
fn first_operand(args: &[String]) -> Option<&str> {
    let mut skip_value = false;
    for arg in args {
        if std::mem::take(&mut skip_value) {
            continue;
        }
        match classify(arg) {
            Word::End => return None,
            Word::Option(takes_value) => skip_value = takes_value != Some(false),
            Word::Operand => return Some(arg),
        }
    }
    None
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
        assert_eq!(matching(&p, "git", &args("log --grep push")), None);
        assert_eq!(matching(&p, "git", &args("commit -m push")), None);
        assert_eq!(matching(&p, "git", &args("--no-pager log --grep push")), None);
        assert_eq!(matching(&p, "git", &args("-c core.editor=vim push")), Some("git push"));
        assert_eq!(matching(&p, "git", &args("--git-dir=.git push")), Some("git push"));
        assert_eq!(
            matching(&p, "git", &args("--unknown-option push")),
            Some("git push"),
            "unknown options err towards guarding"
        );
        assert_eq!(matching(&p, "gh", &args("pr -R owner/repo create")), Some("gh pr create"));
        assert_eq!(matching(&p, "gh", &args("issue create --title pr")), None);
        assert_eq!(matching(&p, "gh", &args("pr view create")), None);
        assert_eq!(matching(&p, "vercel", &args("--prod=true")), Some("vercel deploy"));
        assert_eq!(matching(&p, "vercel", &args("--scope team deploy")), Some("vercel deploy"));
        assert_eq!(matching(&p, "cargo", &args("+nightly publish")), Some("cargo publish"));
        assert_eq!(matching(&p, "kubectl", &args("-n prod apply -f app.yaml")), Some("kubectl apply"));
        assert_eq!(matching(&p, "kubectl", &args("get pods apply")), None);
        assert_eq!(matching(&p, "npm", &args("run publish")), None);
        assert_eq!(matching(&p, "docker", &args("--context prod push img")), Some("docker push"));
        assert_eq!(matching(&p, "git", &args("-- push")), None);
        // Other spellings of the same command.
        assert_eq!(matching(&p, "docker", &args("image push img")), Some("docker push"));
        assert_eq!(matching(&p, "docker", &args("image ls")), None);
        assert_eq!(matching(&p, "wrangler", &args("versions deploy")), Some("wrangler deploy"));
        assert_eq!(matching(&p, "wrangler", &args("pages deploy dist")), Some("wrangler deploy"));
        // Bare `vercel` deploys the current folder.
        assert_eq!(matching(&p, "vercel", &[]), Some("vercel deploy"));
        assert_eq!(matching(&p, "vercel", &args("--yes")), Some("vercel deploy"));
        assert_eq!(matching(&p, "vercel", &args("--scope team")), Some("vercel deploy"));
        assert_eq!(matching(&p, "vercel", &args(".")), Some("vercel deploy"));
        assert_eq!(matching(&p, "vercel", &args("ls")), None);
        assert_eq!(matching(&p, "vercel", &args("env pull")), None);
        assert_eq!(matching(&p, "vercel", &args("--version")), None);
        assert_eq!(matching(&p, "vercel", &args("--help")), None);
        assert_eq!(matching(&p, "netlify", &[]), None);
        // Custom lists keep working: a flag pattern, and a tool alone (every use).
        let custom = vec!["vercel --prod".to_string(), "terraform".to_string()];
        assert_eq!(matching(&custom, "vercel", &args("deploy --prod")), Some("vercel --prod"));
        assert_eq!(matching(&custom, "vercel", &args("deploy")), None);
        assert_eq!(matching(&custom, "terraform", &args("plan")), Some("terraform"));
        assert_eq!(matching(&p, "gh", &args("pr create --fill")), Some("gh pr create"));
        assert_eq!(matching(&p, "gh", &args("pr list")), None);
        assert_eq!(matching(&p, "vercel", &args("deploy --prod")), Some("vercel deploy"));
        assert_eq!(matching(&p, "vercel", &args("--prod")), Some("vercel deploy"));
        assert_eq!(matching(&p, "claude", &args("--resume")), None);
        assert_eq!(matching(&[], "git", &args("push")), None);
    }
}
