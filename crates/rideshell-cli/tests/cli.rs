//! End-to-end tests of the `rideshell` binary.
//!
//! Every test uses its own `RIDESHELL_HOME`, so the developer's real
//! profiles are never touched. Shell tests run only when the shell exists
//! and print a SKIP line otherwise.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use assert_cmd::cargo::cargo_bin;
use tempfile::TempDir;

struct Env {
    home: TempDir,
    work: TempDir,
}

impl Env {
    fn new() -> Self {
        Self { home: tempfile::tempdir().unwrap(), work: tempfile::tempdir().unwrap() }
    }

    /// A projects tree with spaces and non-ASCII names.
    fn dir(&self, rel: &str) -> PathBuf {
        let p = dunce::canonicalize(self.work.path()).unwrap().join(rel);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn cmd(&self) -> Command {
        let mut c = Command::new(cargo_bin("rideshell"));
        c.env("RIDESHELL_HOME", self.home.path()).env("NO_COLOR", "1").env("RIDESHELL_NO_ADOPT", "1");
        for v in ["RIDESHELL_PROFILE", "RIDESHELL_STATE", "RIDESHELL_ACTIVE_PROFILE", "RIDESHELL_SHELL"] {
            c.env_remove(v);
        }
        for v in ["GIT_AUTHOR_NAME", "GIT_AUTHOR_EMAIL", "GIT_COMMITTER_NAME", "GIT_COMMITTER_EMAIL", "GIT_SSH_COMMAND"]
        {
            c.env_remove(v);
        }
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.cmd().args(args).output().unwrap()
    }

    /// Run and assert success; returns stdout.
    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(out.status.success(), "{args:?} failed:\n{}{}", stdout(&out), stderr(&out));
        stdout(&out)
    }

    /// Two profiles with distinct Git identities, `personal` the default.
    fn with_profiles(self) -> Self {
        self.ok(&["profile", "create", "personal"]);
        self.ok(&["profile", "create", "work", "-d", "Company"]);
        self.ok(&["identity", "set", "personal", "git", "name=Pat Personal", "email=pat@home.example"]);
        self.ok(&["identity", "set", "work", "git", "name=Pat Work", "email=pat@corp.example"]);
        self.ok(&["profile", "env", "set", "work", "AWS_PROFILE", "corp-prod"]);
        self
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn have(program: &str) -> bool {
    which::which(program).is_ok()
}

// ----- profiles -------------------------------------------------------------

#[test]
fn profile_lifecycle() {
    let e = Env::new();
    e.ok(&["profile", "create", "personal"]);
    assert!(e.ok(&["profile", "default"]).contains("personal"), "first profile becomes default");
    e.ok(&["profile", "create", "work"]);
    assert!(!e.run(&["profile", "create", "WORK"]).status.success(), "names are case-insensitive unique");
    assert!(!e.run(&["profile", "create", "bad name"]).status.success());

    e.ok(&["profile", "edit", "work", "--description", "Company", "--rename", "company"]);
    e.ok(&["profile", "duplicate", "company", "client-a"]);
    let list = e.ok(&["profile", "list"]);
    for name in ["personal", "company", "client-a"] {
        assert!(list.contains(name), "{list}");
    }
    assert!(list.contains("Company"));

    e.ok(&["profile", "delete", "client-a"]);
    assert!(!e.ok(&["profile", "list"]).contains("client-a"));
    assert!(!e.run(&["profile", "show", "client-a"]).status.success());
}

#[test]
fn secrets_are_refused() {
    let e = Env::new().with_profiles();
    let out = e.run(&["profile", "env", "set", "work", "GITHUB_TOKEN", "abc"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("secret"));
    let out = e.run(&["profile", "env", "set", "work", "SOME_VAR", "ghp_abcdefghijklmnopqrstuvwxyz0123"]);
    assert!(!out.status.success(), "token-shaped values are refused too");
    e.ok(&["profile", "env", "set", "work", "MY_PASSWORD_HINT_FILE", "x", "--not-secret"]);
}

#[test]
fn export_import_round_trip() {
    let e = Env::new().with_profiles();
    let file = e.work.path().join("work profile.toml");
    e.ok(&["profile", "export", "work", "-o", file.to_str().unwrap()]);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("AWS_PROFILE") && text.contains("pat@corp.example"));

    assert!(!e.run(&["profile", "import", file.to_str().unwrap()]).status.success(), "exists");
    e.ok(&["profile", "import", file.to_str().unwrap(), "--name", "work2"]);
    let shown = e.ok(&["profile", "show", "work2"]);
    assert!(shown.contains("AWS_PROFILE=corp-prod") && shown.contains("Pat Work"), "{shown}");
}

// ----- resolution -----------------------------------------------------------

fn git_ident(e: &Env, dir: &Path, extra_env: &[(&str, &str)], args: &[&str]) -> String {
    let mut c = e.cmd();
    c.current_dir(dir).arg("run").args(args).args(["--", "git", "var", "GIT_AUTHOR_IDENT"]);
    for (k, v) in extra_env {
        c.env(k, v);
    }
    let out = c.output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    stdout(&out)
}

#[test]
fn bindings_and_precedence() {
    if !have("git") {
        eprintln!("SKIP: git not installed");
        return;
    }
    let e = Env::new().with_profiles();
    let company = e.dir("projects/company dir");
    let nested = e.dir("projects/company dir/backend/src");
    let personal = e.dir("projects/persönlich");
    e.ok(&["profile", "bind", company.to_str().unwrap(), "work"]);

    assert!(git_ident(&e, &nested, &[], &[]).starts_with("Pat Work <pat@corp.example>"), "binding is inherited");
    assert!(git_ident(&e, &personal, &[], &[]).starts_with("Pat Personal"), "default elsewhere");
    assert!(
        git_ident(&e, &nested, &[("RIDESHELL_PROFILE", "personal")], &[]).starts_with("Pat Personal"),
        "session beats binding"
    );
    assert!(
        git_ident(&e, &nested, &[("RIDESHELL_PROFILE", "personal")], &["--profile", "work"]).starts_with("Pat Work"),
        "flag beats session"
    );

    let mut why = e.cmd();
    let out = why.current_dir(&nested).args(["profile", "why"]).output().unwrap();
    let text = stdout(&out);
    assert!(text.contains("directory binding") && text.contains("selected"), "{text}");

    // A session naming a deleted profile must not fall through to another identity.
    let out = e
        .cmd()
        .current_dir(&nested)
        .env("RIDESHELL_PROFILE", "ghost")
        .args(["run", "--", "git", "--version"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(stderr(&out).contains("does not exist"));
}

#[test]
fn run_propagates_exit_code() {
    if !have("git") {
        eprintln!("SKIP: git not installed");
        return;
    }
    let e = Env::new().with_profiles();
    let out = e.run(&["run", "--", "git", "definitely-not-a-git-command"]);
    assert_eq!(out.status.code(), Some(1));
    let out = e.run(&["run", "--", "rideshell-no-such-program-xyz"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("command not found"));
}

#[test]
fn concurrent_sessions_are_isolated() {
    if !have("git") {
        eprintln!("SKIP: git not installed");
        return;
    }
    let e = Env::new().with_profiles();
    let dir = e.dir("anywhere");
    // 16 "terminals" at once, alternating profiles via their own session variable.
    let children: Vec<_> = (0..16)
        .map(|i| {
            let profile = if i % 2 == 0 { "work" } else { "personal" };
            let child = e
                .cmd()
                .current_dir(&dir)
                .env("RIDESHELL_PROFILE", profile)
                .args(["run", "--", "git", "var", "GIT_AUTHOR_IDENT"])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            (profile, child)
        })
        .collect();
    for (profile, child) in children {
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let expected = if profile == "work" { "Pat Work" } else { "Pat Personal" };
        assert!(stdout(&out).starts_with(expected), "{profile}: {}", stdout(&out));
    }
}

// ----- shell plumbing -------------------------------------------------------

#[test]
fn hook_env_without_database_is_silent_and_creates_nothing() {
    let e = Env::new();
    let out = e.run(&["hook-env", "--shell", "bash"]);
    assert!(out.status.success());
    assert!(stdout(&out).is_empty());
    assert!(std::fs::read_dir(e.home.path()).unwrap().next().is_none(), "no files created");
}

#[test]
fn profile_use_without_integration_explains() {
    let e = Env::new().with_profiles();
    let out = e.run(&["profile", "use", "work"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stdout(&out).contains("hook"));
    let emitted = e.ok(&["profile", "use", "work", "--emit", "bash"]);
    assert!(emitted.starts_with("# rideshell:emit\n") && emitted.contains("export RIDESHELL_PROFILE='work'"));
}

#[test]
fn shell_install_is_reversible() {
    // zsh's startup file honours ZDOTDIR, so it can be redirected safely.
    let e = Env::new();
    let zdot = e.dir("zdotdir");
    let rc = zdot.join(".zshrc");
    let original = "export EDITOR=vim\r\nalias g=git\r\n";
    std::fs::write(&rc, original).unwrap();

    let out = e.cmd().env("ZDOTDIR", &zdot).args(["shell", "install", "zsh", "--dry-run"]).output().unwrap();
    assert!(out.status.success());
    assert_eq!(std::fs::read_to_string(&rc).unwrap(), original, "dry run changes nothing");

    let out = e.cmd().env("ZDOTDIR", &zdot).args(["shell", "install", "zsh"]).output().unwrap();
    assert!(!out.status.success(), "non-interactive install without --yes is refused");

    let out = e.cmd().env("ZDOTDIR", &zdot).args(["shell", "install", "zsh", "--yes"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let installed = std::fs::read_to_string(&rc).unwrap();
    assert!(installed.starts_with(original) && installed.contains("# >>> rideshell >>>"));
    assert!(!installed.replace("\r\n", "").contains('\n'), "CRLF preserved");

    let out = e.cmd().env("ZDOTDIR", &zdot).args(["shell", "uninstall", "zsh", "--yes"]).output().unwrap();
    assert!(out.status.success());
    assert_eq!(std::fs::read_to_string(&rc).unwrap(), original, "uninstall restores the file exactly");
    let backups = std::fs::read_dir(&zdot)
        .unwrap()
        .filter(|f| f.as_ref().unwrap().file_name().to_string_lossy().contains("backup"))
        .count();
    assert_eq!(backups, 2, "a backup is written before each edit");
}

// ----- real shells ----------------------------------------------------------

/// Lines of the form `label|active|git|aws` printed by the shell scripts.
fn parse_report(out: &str) -> Vec<Vec<String>> {
    out.lines().filter(|l| l.starts_with("R|")).map(|l| l.split('|').skip(1).map(str::to_string).collect()).collect()
}

fn assert_shell_report(out: &Output) {
    let text = stdout(out);
    assert!(out.status.success(), "stdout:\n{text}\nstderr:\n{}", stderr(out));
    let rows = parse_report(&text);
    let expect = [
        ["unbound", "personal", "Pat Personal", ""],
        ["bound", "work", "Pat Work", "corp-prod"],
        ["use personal", "personal", "Pat Personal", ""],
        ["clear", "work", "Pat Work", "corp-prod"],
        ["no profile", "", "Mine", ""],
    ];
    assert_eq!(rows.len(), expect.len(), "{text}\n{}", stderr(out));
    for (row, exp) in rows.iter().zip(expect) {
        assert_eq!(row, &exp.map(String::from).to_vec(), "{text}");
    }
}

fn shell_fixture() -> (Env, PathBuf, PathBuf) {
    let e = Env::new().with_profiles();
    let bound = e.dir("ws/company ü");
    let unbound = e.dir("ws/other");
    e.ok(&["profile", "bind", bound.to_str().unwrap(), "work"]);
    (e, bound, unbound)
}

#[test]
fn powershell_integration_end_to_end() {
    let shells: Vec<&str> = ["powershell", "pwsh"].into_iter().filter(|s| have(s)).collect();
    if shells.is_empty() {
        eprintln!("SKIP: no PowerShell found");
        return;
    }
    for shell in shells {
        let (e, bound, unbound) = shell_fixture();
        let script = format!(
            r#"
$ErrorActionPreference = 'Stop'
Invoke-Expression ((& '{bin}' hook {shell}) -join "`n")
function Report($l) {{ "R|$l|$env:RIDESHELL_ACTIVE_PROFILE|$env:GIT_AUTHOR_NAME|$env:AWS_PROFILE" }}
Set-Location -LiteralPath '{unbound}'; $null = prompt; Report 'unbound'
Set-Location -LiteralPath '{bound}'; $null = prompt; Report 'bound'
rideshell profile use personal; Report 'use personal'
rideshell profile use --clear; Report 'clear'
$env:GIT_AUTHOR_NAME = 'Mine'
rideshell profile default --clear | Out-Null
Set-Location -LiteralPath '{unbound}'; $null = prompt; Report 'no profile'
"#,
            bin = cargo_bin("rideshell").display(),
            unbound = unbound.display(),
            bound = bound.display(),
        );
        let file = e.work.path().join("t.ps1");
        // UTF-8 with BOM so Windows PowerShell reads the non-ASCII path correctly.
        std::fs::write(&file, [b"\xEF\xBB\xBF".as_slice(), script.as_bytes()].concat()).unwrap();
        let mut c = Command::new(shell);
        c.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"]).arg(&file);
        c.env("RIDESHELL_HOME", e.home.path()).env("NO_COLOR", "1");
        for v in ["RIDESHELL_PROFILE", "RIDESHELL_STATE", "RIDESHELL_ACTIVE_PROFILE", "GIT_AUTHOR_NAME", "AWS_PROFILE"]
        {
            c.env_remove(v);
        }
        let out = c.output().unwrap();
        eprintln!("checked {shell}");
        assert_shell_report(&out);
    }
}

fn posix_shells() -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Ok(p) = std::env::var("RIDESHELL_TEST_BASH") {
        found.push(PathBuf::from(p));
    } else if cfg!(windows) {
        // Avoid System32\bash.exe (the WSL launcher).
        for p in [r"C:\Program Files\Git\bin\bash.exe", r"C:\Program Files (x86)\Git\bin\bash.exe"] {
            if Path::new(p).is_file() {
                found.push(PathBuf::from(p));
                break;
            }
        }
    } else if let Ok(p) = which::which("bash") {
        found.push(p);
    }
    if let Ok(p) = which::which("zsh") {
        found.push(p);
    }
    found
}

#[test]
fn posix_integration_end_to_end() {
    let shells = posix_shells();
    if shells.is_empty() {
        eprintln!("SKIP: no bash/zsh found");
        return;
    }
    for sh in shells {
        let is_zsh = sh.file_stem().is_some_and(|s| s.to_string_lossy().contains("zsh"));
        let (kind, prompt_fn) = if is_zsh { ("zsh", "__rideshell_precmd") } else { ("bash", "__rideshell_prompt") };
        let (e, bound, unbound) = shell_fixture();
        let bin = cargo_bin("rideshell").to_string_lossy().replace('\\', "/");
        let q = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\\', "/").replace('\'', r"'\''"));
        let script = format!(
            r#"
eval "$('{bin}' hook {kind})"
R() {{ printf 'R|%s|%s|%s|%s\n' "$1" "$RIDESHELL_ACTIVE_PROFILE" "$GIT_AUTHOR_NAME" "$AWS_PROFILE"; }}
cd {unbound}; {prompt_fn}; R unbound
cd {bound}; {prompt_fn}; R bound
rideshell profile use personal; R 'use personal'
rideshell profile use --clear; R clear
export GIT_AUTHOR_NAME=Mine
rideshell profile default --clear >/dev/null
cd {unbound}; {prompt_fn}; R 'no profile'
"#,
            unbound = q(&unbound),
            bound = q(&bound),
        );
        let file = e.work.path().join("t.sh");
        std::fs::write(&file, script).unwrap();
        let mut c = Command::new(&sh);
        if is_zsh {
            c.args(["-f"]);
        } else {
            c.args(["--noprofile", "--norc"]);
        }
        c.arg(&file).env("RIDESHELL_HOME", e.home.path()).env("NO_COLOR", "1");
        for v in ["RIDESHELL_PROFILE", "RIDESHELL_STATE", "RIDESHELL_ACTIVE_PROFILE", "GIT_AUTHOR_NAME", "AWS_PROFILE"]
        {
            c.env_remove(v);
        }
        let out = c.output().unwrap();
        eprintln!("checked {}", sh.display());
        assert_shell_report(&out);
    }
}

// ----- multi-account tools, secrets, shims ---------------------------------------

/// A fake `claude` that prints which account directory and API key it got.
fn fake_claude(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    if cfg!(windows) {
        let f = dir.join("claude.cmd");
        std::fs::write(
            &f,
            "@echo off\r\necho DIR=%CLAUDE_CONFIG_DIR%\r\necho KEY=%ANTHROPIC_API_KEY%\r\necho ARGS=%*\r\n",
        )
        .unwrap();
        f
    } else {
        let f = dir.join("claude");
        std::fs::write(
            &f,
            "#!/bin/sh\necho \"DIR=$CLAUDE_CONFIG_DIR\"\necho \"KEY=$ANTHROPIC_API_KEY\"\necho \"ARGS=$*\"\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        f
    }
}

struct Accounts {
    e: Env,
    vault: PathBuf,
    path: std::ffi::OsString,
    shim: PathBuf,
    project: PathBuf,
}

impl Accounts {
    fn new() -> Self {
        let e = Env::new();
        let vault = e.work.path().join("vault.json");
        let fake_bin = e.dir("fake bin");
        fake_claude(&fake_bin);
        let shims = e.home.path().join("shims");
        let path = std::env::join_paths(
            [shims.clone(), fake_bin].into_iter().chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        let a = Self {
            shim: shims.join(if cfg!(windows) { "claude.exe" } else { "claude" }),
            project: e.dir("client project"),
            e,
            vault,
            path,
        };
        for p in ["work", "personal"] {
            a.ok(&["profile", "create", p]);
            a.ok(&["identity", "set", p, "claude"]);
        }
        a
    }

    fn cmd(&self) -> Command {
        let mut c = self.e.cmd();
        c.env("RIDESHELL_INSECURE_TEST_VAULT", &self.vault).env("PATH", &self.path).env_remove("CLAUDE_CONFIG_DIR");
        c.env_remove("ANTHROPIC_API_KEY");
        c
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.cmd().args(args).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}{}", stdout(&out), stderr(&out));
        stdout(&out)
    }

    fn secret(&self, args: &[&str], value: &str) {
        use std::io::Write;
        let mut child = self
            .cmd()
            .args(["secret", "set"])
            .args(args)
            .arg("--stdin")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(value.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
    }

    /// Run `claude` exactly as a user would (through the shim on PATH).
    fn claude(&self, profile: Option<&str>, dir: &Path) -> std::process::Child {
        let mut c = Command::new(&self.shim);
        c.env("RIDESHELL_HOME", self.e.home.path())
            .env("RIDESHELL_INSECURE_TEST_VAULT", &self.vault)
            .env("PATH", &self.path)
            .env_remove("RIDESHELL_PROFILE")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("ANTHROPIC_API_KEY")
            .current_dir(dir)
            .args(["--model", "x y"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if let Some(p) = profile {
            c.env("RIDESHELL_PROFILE", p);
        }
        c.spawn().unwrap()
    }
}

fn field(out: &str, name: &str) -> String {
    out.lines().find_map(|l| l.trim().strip_prefix(&format!("{name}="))).unwrap_or("").trim().to_string()
}

#[test]
fn parallel_claude_sessions_use_separate_accounts() {
    let a = Accounts::new();
    assert!(a.shim.exists(), "identity set creates the claude shim");
    a.secret(&["ANTHROPIC_API_KEY", "--profile", "work"], "sk-work");
    a.secret(&["ANTHROPIC_API_KEY", "--profile", "personal"], "sk-personal");

    let anywhere = a.e.dir("anywhere");
    // Ten sessions at once, alternating accounts.
    let children: Vec<_> = (0..10)
        .map(|i| {
            let p = if i % 2 == 0 { "work" } else { "personal" };
            (p, a.claude(Some(p), &anywhere))
        })
        .collect();
    let mut dirs = std::collections::BTreeMap::new();
    for (p, child) in children {
        let out = child.wait_with_output().unwrap();
        let text = stdout(&out);
        assert!(out.status.success(), "{text}{}", stderr(&out));
        let dir = field(&text, "DIR");
        assert!(dir.replace('\\', "/").ends_with(&format!("accounts/claude/{p}")), "{p}: {dir}");
        assert_eq!(field(&text, "KEY"), format!("sk-{p}"));
        assert!(field(&text, "ARGS").contains("--model"), "arguments pass through: {text}");
        dirs.entry(p).or_insert(dir.clone());
        assert_eq!(dirs[p], dir, "same profile, same account");
    }
    assert_ne!(dirs["work"], dirs["personal"]);
    assert!(Path::new(&dirs["work"]).is_dir(), "account directory is created on first use");
}

#[test]
fn project_secret_overrides_account_secret_and_binding_picks_account() {
    let a = Accounts::new();
    a.secret(&["ANTHROPIC_API_KEY", "--profile", "work"], "sk-work");
    a.ok(&["profile", "bind", a.project.to_str().unwrap(), "work"]);
    let nested = a.project.join("src");
    std::fs::create_dir_all(&nested).unwrap();

    let out = a.claude(None, &nested).wait_with_output().unwrap();
    let text = stdout(&out);
    assert!(field(&text, "DIR").replace('\\', "/").ends_with("accounts/claude/work"), "binding selects work: {text}");
    assert_eq!(field(&text, "KEY"), "sk-work");

    a.secret(&["ANTHROPIC_API_KEY", "--dir", a.project.to_str().unwrap()], "sk-client-project");
    let out = a.claude(None, &nested).wait_with_output().unwrap();
    assert_eq!(field(&stdout(&out), "KEY"), "sk-client-project", "project key wins inside the project");

    // Secrets are listed by name only and never appear in shell code.
    let list = a.ok(&["secret", "list"]);
    assert!(list.contains("ANTHROPIC_API_KEY") && !list.contains("sk-"), "{list}");
    let hook = a.cmd().current_dir(&nested).args(["hook-env", "--shell", "bash"]).output().unwrap();
    let hook = stdout(&hook);
    assert!(hook.contains("CLAUDE_CONFIG_DIR") && !hook.contains("sk-"), "hook never exports secrets: {hook}");
    let vault_text = std::fs::read_to_string(&a.vault).unwrap();
    let db = std::fs::read(a.e.home.path().join("rideshell.db")).unwrap();
    assert!(vault_text.contains("sk-work"));
    assert!(!String::from_utf8_lossy(&db).contains("sk-work"), "secrets never reach the database");

    // Deleting the profile deletes its secrets.
    a.ok(&["profile", "delete", "work", "--force"]);
    assert!(!std::fs::read_to_string(&a.vault).unwrap().contains("sk-work"));
}

#[test]
fn shim_fails_closed_for_a_missing_profile() {
    let a = Accounts::new();
    let anywhere = a.e.dir("anywhere");
    let out = a.claude(Some("deleted-profile"), &anywhere).wait_with_output().unwrap();
    assert!(!out.status.success());
    assert!(stderr(&out).contains("does not exist"), "{}", stderr(&out));
    assert!(!stdout(&out).contains("DIR="), "the real tool must not run with an unintended account");
}

#[test]
fn shim_management() {
    let a = Accounts::new();
    a.ok(&["shim", "add", "wrangler"]);
    let list = a.ok(&["shim", "list"]);
    assert!(list.contains("claude") && list.contains("wrangler"), "{list}");
    a.ok(&["shim", "remove", "wrangler"]);
    assert!(!a.ok(&["shim", "list"]).contains("wrangler"));
    let out = a.cmd().args(["shim", "add", "rideshell"]).output().unwrap();
    assert!(!out.status.success(), "cannot shim itself");
}

#[cfg(windows)]
#[test]
fn windows_install_and_uninstall_are_reversible() {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let e = Env::new();
    let root = format!(r"Software\RideShellTest-{}", std::process::id());
    let install_dir = e.work.path().join("Programs").join("RideShell");
    let folders = e.work.path().join("ShellFolders");
    let run = |args: &[&str]| {
        let out = e
            .cmd()
            .env("RIDESHELL_TEST_REGISTRY_ROOT", &root)
            .env("RIDESHELL_TEST_INSTALL_DIR", &install_dir)
            .env("RIDESHELL_TEST_SHELL_FOLDERS", &folders)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {}{}", stdout(&out), stderr(&out));
    };
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (env_key, _) = hkcu.create_subkey(format!(r"{root}\Environment")).unwrap();
    env_key.set_value("Path", &r"C:\existing\tool;%USERPROFILE%\bin").unwrap();

    run(&["install", "--yes"]);
    assert!(install_dir.join("rideshell.exe").is_file());
    let path: String = env_key.get_value("Path").unwrap();
    let entries: Vec<&str> = path.split(';').collect();
    assert!(entries[0].ends_with("shims"), "shims first: {path}");
    assert!(entries[1].eq_ignore_ascii_case(install_dir.to_str().unwrap()), "{path}");
    assert!(path.ends_with(r"C:\existing\tool;%USERPROFILE%\bin"), "existing entries kept: {path}");
    let uninstall = hkcu.open_subkey(format!(r"{root}\Uninstall\RideShell")).unwrap();
    let cmdline: String = uninstall.get_value("UninstallString").unwrap();
    assert!(cmdline.contains("uninstall --program"));
    let normal = folders.join("StartMenu").join("RideShell.lnk");
    let admin = folders.join("StartMenu").join("RideShell (Administrator).lnk");
    assert!(normal.is_file() && admin.is_file(), "Start menu entries");
    assert_eq!(std::fs::read(&normal).unwrap()[0x15] & 0x20, 0, "normal entry is not elevated");
    assert_eq!(std::fs::read(&admin).unwrap()[0x15] & 0x20, 0x20, "admin entry has 'Run as administrator'");
    let fragment = std::fs::read_to_string(folders.join("Fragments").join("RideShell").join("rideshell.json")).unwrap();
    assert!(fragment.contains("rideshell.exe") && fragment.contains("open"), "{fragment}");

    if install_dir.join("rideshell-app.exe").is_file() {
        let menu = hkcu.open_subkey(format!(r"{root}\Classes\Directory\shell\RideShell\command")).unwrap();
        let cmd: String = menu.get_value("").unwrap();
        assert!(cmd.contains("rideshell-app.exe") && cmd.contains("--dir \"%V\""), "{cmd}");
        assert!(hkcu.open_subkey(format!(r"{root}\Classes\Directory\Background\shell\RideShell")).is_ok());
        assert_ne!(std::fs::read(&normal).unwrap(), std::fs::read(&admin).unwrap(), "shortcuts must differ");
    }

    run(&["install", "--yes"]); // idempotent
    let again: String = env_key.get_value("Path").unwrap();
    assert_eq!(again, path, "reinstall does not duplicate PATH entries");

    run(&["uninstall", "--program", "--yes", "--keep-shell-integration"]);
    let path: String = env_key.get_value("Path").unwrap();
    assert_eq!(path, r"C:\existing\tool;%USERPROFILE%\bin", "PATH restored");
    assert!(hkcu.open_subkey(format!(r"{root}\Uninstall\RideShell")).is_err());
    assert!(!normal.exists() && !admin.exists(), "Start menu entries removed");
    assert!(hkcu.open_subkey(format!(r"{root}\Classes\Directory\shell\RideShell")).is_err(), "Explorer menu removed");
    assert!(!folders.join("Fragments").join("RideShell").exists(), "Terminal profile removed");
    hkcu.delete_subkey_all(&root).unwrap();
}

// ----- RideShell windows: cmd / bash lines, admin -------------------------------

/// Run `line` the way the Enter key in a RideShell window does, and return
/// (exit code, PowerShell code that applies the resulting changes).
#[cfg(windows)]
fn dialect_exec(e: &Env, dialect: &str, line: &str, cwd: &Path) -> (i32, String) {
    let out_file = e.work.path().join(format!("apply-{dialect}.txt"));
    let _ = std::fs::remove_file(&out_file);
    let out = e
        .cmd()
        .env("RIDESHELL_LINE", line)
        .env("RS_KEEP", "kept")
        .env("RS_DROP", "x")
        .current_dir(cwd)
        .args(["dialect-exec", dialect, "--out"])
        .arg(&out_file)
        .output()
        .unwrap();
    (out.status.code().unwrap_or(-1), std::fs::read_to_string(&out_file).unwrap_or_default())
}

#[cfg(windows)]
#[test]
fn cmd_lines_bring_directory_and_variables_back() {
    let e = Env::new();
    let start = e.dir("start");
    let target = e.dir("tårget dir");
    let line = format!("cd /d \"{}\" & set RS_NEW=hello world& set RS_DROP=", target.display());
    let (code, apply) = dialect_exec(&e, "cmd", &line, &start);
    assert_eq!(code, 0, "{apply}");
    assert!(apply.starts_with("# rideshell:emit"), "{apply}");
    assert!(apply.contains("Set-Location -LiteralPath"), "{apply}");
    assert!(apply.contains("SetEnvironmentVariable('RS_NEW', 'hello world')"), "no trailing space: {apply}");
    assert!(apply.contains("SetEnvironmentVariable('RS_DROP', $null)"), "{apply}");
    assert!(!apply.contains("RS_KEEP") && !apply.contains("RIDESHELL_LINE"), "{apply}");
    assert!(apply.is_ascii(), "PowerShell code must be ASCII");

    let (code, _) = dialect_exec(&e, "cmd", "dir zz_no_such_file >nul 2>nul", &start);
    assert_eq!(code, 1, "cmd's errorlevel is passed through");
    let (code, apply) = dialect_exec(&e, "cmd", "@cmd exit /b 7", &start);
    assert_eq!(code, 7);
    assert!(apply.is_empty(), "an exiting line changes nothing: {apply}");
}

#[cfg(windows)]
#[test]
fn bash_lines_bring_directory_and_variables_back() {
    if !Path::new(r"C:\Program Files\Git\bin\bash.exe").is_file() {
        eprintln!("SKIP: Git Bash not installed");
        return;
    }
    let e = Env::new();
    let start = e.dir("start");
    e.dir("start/sub");
    let (code, apply) = dialect_exec(&e, "bash", "export RS_NEW='from bash' && unset RS_DROP && cd sub", &start);
    assert_eq!(code, 0, "{apply}");
    assert!(apply.contains("SetEnvironmentVariable('RS_NEW', 'from bash')"), "{apply}");
    assert!(apply.contains("SetEnvironmentVariable('RS_DROP', $null)"), "{apply}");
    assert!(apply.contains("Set-Location") && apply.contains("sub"), "{apply}");
    // bash's own bookkeeping (PWD, SHLVL, MSYS paths) does not leak back.
    assert!(!apply.contains("'PWD'") && !apply.contains("SHLVL") && !apply.contains("'PATH'"), "{apply}");
    let (code, _) = dialect_exec(&e, "bash", "false", &start);
    assert_eq!(code, 1);
}

#[test]
fn dialect_explains_where_a_line_runs() {
    let e = Env::new();
    assert!(e.ok(&["dialect", "dir", "/s", "/b"]).starts_with("cmd"));
    assert!(e.ok(&["dialect", "set", "FOO=1"]).starts_with("cmd"));
    assert!(e.ok(&["dialect", "Get-ChildItem", "-Recurse"]).starts_with("pwsh"));
    assert!(e.ok(&["dialect", "@bash", "echo", "hi"]).starts_with("bash"));
}

#[cfg(windows)]
#[test]
fn admin_window_keeps_profile_and_folder() {
    let e = Env::new().with_profiles();
    let folder = e.dir("a folder");
    let elevate = |extra: &[&str], session: Option<&str>| {
        let mut c = e.cmd();
        c.env("RIDESHELL_TEST_ELEVATE_ECHO", "1").current_dir(&folder).arg("admin").args(extra);
        if let Some(p) = session {
            c.env("RIDESHELL_PROFILE", p);
        }
        let out = c.output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        stdout(&out).lines().nth(1).unwrap_or_default().to_string()
    };
    let window = elevate(&[], Some("work"));
    assert!(window.starts_with("open --dir \""), "{window}");
    assert!(window.contains("a folder\"") && window.ends_with("--profile work"), "{window}");
    let one = elevate(&["--", "net", "session"], None);
    assert!(one.starts_with("run --pause --dir"), "{one}");
    assert!(one.ends_with("-- net session") && !one.contains("--profile"), "{one}");
    // A terminal pointing at a deleted profile does not open an admin window as someone else.
    let mut c = e.cmd();
    let out = c.env("RIDESHELL_TEST_ELEVATE_ECHO", "1").env("RIDESHELL_PROFILE", "gone").arg("admin").output().unwrap();
    assert!(!out.status.success());
}

#[test]
fn run_accepts_a_directory() {
    let e = Env::new().with_profiles();
    let d = e.dir("rundir");
    let (prog, args): (&str, Vec<&str>) = if cfg!(windows) { ("cmd", vec!["/c", "cd"]) } else { ("pwd", vec![]) };
    let mut all = vec!["run", "--profile", "work", "-C", d.to_str().unwrap(), "--", prog];
    all.extend(args);
    let out = e.ok(&all);
    assert!(out.trim().eq_ignore_ascii_case(d.to_str().unwrap()), "{out}");
}

#[test]
fn every_new_profile_gets_its_own_tool_accounts() {
    let e = Env::new();
    e.ok(&["profile", "create", "one"]);
    let out = e.ok(&["profile", "create", "two"]);
    assert!(out.contains("own, separate accounts"), "{out}");
    let show = e.ok(&["identity", "show", "two"]);
    for tool in ["claude", "codex", "gh"] {
        assert!(show.contains(&format!("{tool}  account two")), "{tool} isolated for the new profile: {show}");
    }
    assert!(!show.contains("config_dir"), "a later profile never adopts existing logins: {show}");
}
