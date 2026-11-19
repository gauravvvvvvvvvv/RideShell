//! RideShell windows.
//!
//! `rideshell` (or `rideshell open`) starts a RideShell window: real
//! PowerShell with RideShell loaded, so everything behaves exactly like
//! PowerShell, plus:
//!
//! * cmd-style and bash-style lines run in cmd / bash automatically
//!   ([`rideshell_core::dialect`]), and their `cd` / variable changes come
//!   back to the window;
//! * profiles, directory bindings and tool shims are active;
//! * `rideshell admin` opens the same thing as Administrator.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rideshell_core::brand;
use rideshell_core::dialect::{self, CommandKind, Dialect, Hints};
use rideshell_core::shells::{ps_literal, Shell};

use super::{session_profile, shell as shellcmd, Ctx};
use crate::cli::{AdminArgs, HostArg, OpenArgs};
use crate::ui;

// ----- open ------------------------------------------------------------------

fn host_shell(ctx: &Ctx, arg: Option<HostArg>) -> Result<(Shell, PathBuf)> {
    let wanted = match arg {
        Some(HostArg::Pwsh) => "pwsh".to_string(),
        Some(HostArg::Powershell) => "powershell".to_string(),
        None => ctx.config().map(|c| c.session.host).unwrap_or_else(|_| "auto".into()),
    };
    let find = |s: Shell| shellcmd::detect(s).map(|p| (s, p));
    match wanted.as_str() {
        "pwsh" => find(Shell::Pwsh).context("PowerShell 7 (pwsh) is not installed"),
        "powershell" => find(Shell::PowerShell).context("Windows PowerShell is not available"),
        "auto" => find(Shell::Pwsh).or_else(|| find(Shell::PowerShell)).context("no PowerShell found on PATH"),
        other => bail!("unknown session host {other:?} in config (use auto, pwsh or powershell)"),
    }
}

pub fn open(ctx: &Ctx, args: OpenArgs) -> Result<i32> {
    if args.admin {
        return admin(ctx, AdminArgs { profile: args.profile, dir: args.dir, host: args.host, command: Vec::new() });
    }
    if !cfg!(windows) {
        bail!(
            "RideShell windows are Windows-only for now. On macOS/Linux use `{} shell install bash` (or zsh)",
            brand::BIN_NAME
        );
    }
    if let Some(name) = &args.profile {
        ctx.store()?.require_profile(name)?;
    }
    let dir = match &args.dir {
        Some(d) => dunce::canonicalize(d).with_context(|| format!("cannot open {}", d.display()))?,
        None => std::env::current_dir().context("cannot determine the current directory")?,
    };
    let (shell, host) = host_shell(ctx, args.host)?;
    let exe = std::env::current_exe().context("cannot determine the executable path")?;
    let exe = dunce::canonicalize(&exe).unwrap_or(exe);

    let mut bootstrap = format!(
        "Invoke-Expression ((& {} hook {} --session) -join [Environment]::NewLine)",
        ps_literal(&exe.to_string_lossy()),
        shell.id()
    );
    if let Some(line) = &args.command {
        // Run through the same router as typed lines (cmd/bash lines work too).
        bootstrap.push_str(&format!("; __{}_start {}", brand::BIN_NAME, ps_literal(line)));
    }

    if !args.no_banner {
        banner(shell, args.profile.as_deref());
    }

    let mut cmd = std::process::Command::new(&host);
    cmd.args(["-NoLogo", "-NoExit", "-EncodedCommand", &encode_command(&bootstrap)])
        .current_dir(&dir)
        .env(brand::env::SESSION, "1");
    for internal in [brand::env::LINE, brand::env::PS_ERRORS, brand::env::PS_COMMANDS] {
        cmd.env_remove(internal);
    }
    if let Some(p) = &args.profile {
        cmd.env(brand::env::PROFILE, p);
    }
    #[cfg(windows)]
    rideshell_core::exec::ignore_ctrl_c();
    let status = cmd.status().with_context(|| format!("cannot start {}", host.display()))?;
    Ok(status.code().unwrap_or(1))
}

fn banner(shell: Shell, profile: Option<&str>) {
    let admin = if rideshell_core::platform::is_elevated() {
        format!("  {}", ui::paint("Administrator", ui::WARN))
    } else {
        String::new()
    };
    anstream::println!(
        "{} {}  {}{}",
        ui::accent(brand::APP_NAME),
        ui::dim(env!("CARGO_PKG_VERSION")),
        ui::dim(format!("{} · cmd and bash commands work too", shell.display_name())),
        admin
    );
    if let Some(p) = profile {
        anstream::println!("{}", ui::dim(format!("profile: {p}")));
    }
    anstream::println!(
        "{}",
        ui::dim(format!(
            "`{0} --help` for commands · `{0} admin` for an Administrator window · @cmd/@bash/@ps to force a shell",
            brand::BIN_NAME
        ))
    );
    anstream::println!();
}

/// `-EncodedCommand` takes base64 of UTF-16LE. It avoids every quoting rule
/// between here and PowerShell's parser.
pub fn encode_command(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    base64(&bytes)
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

// ----- admin -------------------------------------------------------------------

pub fn admin(ctx: &Ctx, args: AdminArgs) -> Result<i32> {
    let exe = std::env::current_exe().context("cannot determine the executable path")?;
    let exe = dunce::canonicalize(&exe).unwrap_or(exe);
    let dir = match &args.dir {
        Some(d) => dunce::canonicalize(d).with_context(|| format!("cannot open {}", d.display()))?,
        None => std::env::current_dir().context("cannot determine the current directory")?,
    };
    // Elevated processes do not inherit this terminal's environment, so the
    // session's profile is passed explicitly.
    let profile = args.profile.clone().or_else(session_profile);
    if let Some(name) = &profile {
        ctx.store()?.require_profile(name)?;
    }
    let dir_s = dir.to_string_lossy().into_owned();
    let mut params: Vec<String> = Vec::new();
    if args.command.is_empty() {
        params.extend(["open".into(), "--dir".into(), dir_s.clone()]);
        if let Some(h) = args.host {
            params.extend(["--host".into(), h.id().into()]);
        }
    } else {
        params.extend(["run".into(), "--pause".into(), "--dir".into(), dir_s.clone()]);
    }
    if let Some(p) = &profile {
        params.extend(["--profile".into(), p.clone()]);
    }
    if !args.command.is_empty() {
        params.push("--".into());
        params.extend(args.command.iter().cloned());
    }
    let line = params.iter().map(|p| rideshell_core::platform::quote_arg(p)).collect::<Vec<_>>().join(" ");

    if std::env::var_os("RIDESHELL_TEST_ELEVATE_ECHO").is_some() {
        println!("{}", exe.display());
        println!("{line}");
        return Ok(0);
    }
    anstream::eprintln!("{}", ui::dim("Opening an Administrator window (Windows asks for permission)..."));
    rideshell_core::platform::elevate(&exe, &line, &dir)?;
    Ok(0)
}

// ----- routing ------------------------------------------------------------------

/// Internal: called by the Enter key in a RideShell window. Reads the line
/// and PowerShell's hints from the environment, prints `pwsh`, `cmd` or `bash`.
pub fn route() -> Result<i32> {
    let line = std::env::var(brand::env::LINE).unwrap_or_default();
    let bash = shellcmd::detect(Shell::Bash);
    let hints = Hints {
        ps_parse_errors: std::env::var(brand::env::PS_ERRORS).is_ok_and(|v| v.trim() != "0" && !v.trim().is_empty()),
        commands: std::env::var(brand::env::PS_COMMANDS).ok().map(|s| Hints::parse_commands(&s)),
        bash_available: bash.is_some(),
    };
    println!("{}", dialect::classify(&line, &hints).dialect);
    Ok(0)
}

/// `rideshell dialect <line>`: explain where a line would run.
pub fn explain(line: &str) -> Result<i32> {
    let mut commands = Vec::new();
    for word in line.split(['|', ';', '&']).filter_map(|seg| seg.split_whitespace().next()) {
        let kind = if which::which(word).is_ok() { CommandKind::Application } else { CommandKind::Missing };
        commands.push((word.to_string(), kind));
    }
    let bash = shellcmd::detect(Shell::Bash);
    let hints = Hints { ps_parse_errors: false, commands: Some(commands), bash_available: bash.is_some() };
    let d = dialect::classify(line, &hints);
    anstream::println!("{}  {}", ui::accent(d.dialect.id()), ui::dim(format!("({})", d.reason)));
    if hints.commands.is_some() {
        ui::hint("in a RideShell window PowerShell also reports its own commands and parse errors");
    }
    Ok(0)
}

const SKIP_KEYS: &[&str] = &["PWD", "OLDPWD", "SHLVL", "_"];

fn internal(key: &str) -> bool {
    key.starts_with('=')
        || SKIP_KEYS.iter().any(|k| k.eq_ignore_ascii_case(key))
        || [brand::env::LINE, brand::env::PS_ERRORS, brand::env::PS_COMMANDS]
            .iter()
            .any(|k| k.eq_ignore_ascii_case(key))
}

#[derive(Debug, Default)]
struct Captured {
    cwd: PathBuf,
    code: i32,
    env: BTreeMap<String, String>,
}

fn read_capture(path: &Path) -> Option<Captured> {
    let text = std::fs::read_to_string(path).ok()?;
    let _ = std::fs::remove_file(path);
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let env =
        v.get("env")?.as_object()?.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect();
    Some(Captured {
        cwd: PathBuf::from(v.get("cwd")?.as_str()?),
        code: v.get("code").and_then(|c| c.as_i64()).unwrap_or(0) as i32,
        env,
    })
}

/// Internal: `rideshell __capture <file> [code]` records the calling shell's
/// directory, environment and last exit code.
pub fn capture(file: &Path, code: Option<String>) -> Result<i32> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let env: serde_json::Map<String, serde_json::Value> = std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, serde_json::Value::String(v.into_string().ok()?))))
        .collect();
    let code: i32 = code.and_then(|c| c.trim().parse().ok()).unwrap_or(0);
    let doc = serde_json::json!({ "cwd": cwd.to_string_lossy(), "code": code, "env": env });
    std::fs::write(file, doc.to_string()).with_context(|| format!("cannot write {}", file.display()))?;
    Ok(code)
}

fn temp_file(tag: &str, ext: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    std::env::temp_dir().join(format!("{}-{tag}-{}-{nanos}.{ext}", brand::BIN_NAME, std::process::id()))
}

/// Internal: run `RIDESHELL_LINE` in cmd or bash. Writes PowerShell code that
/// applies the resulting directory and environment changes to `out`.
///
/// With `console`, the child is attached straight to the console window
/// (`CONIN$`/`CONOUT$`): PowerShell captures the output of commands run from
/// a key handler, which would otherwise hide it.
pub fn dialect_exec(ctx: &Ctx, which: &str, out: &Path, console: bool) -> Result<i32> {
    let d = Dialect::parse(which).context("dialect must be cmd or bash")?;
    let raw = std::env::var(brand::env::LINE).context("nothing to run")?;
    // Removes an explicit `@cmd` / `@bash` prefix; other lines are unchanged.
    let line = dialect::classify(&raw, &Hints::default()).line;
    let exe = std::env::current_exe().context("cannot determine the executable path")?;
    let exe = dunce::canonicalize(&exe).unwrap_or(exe);
    let start_cwd = std::env::current_dir().unwrap_or_default();
    let show = ctx.config().map(|c| c.session.show_dialect).unwrap_or(true);
    let stdio = if console { Some(ConsoleStdio::open()?) } else { None };
    if show {
        if let Some(io) = &stdio {
            // VT colour only where the terminal is known to understand it.
            let modern = std::env::var_os("WT_SESSION").is_some()
                || std::env::var("TERM_PROGRAM").is_ok_and(|t| t == "vscode" || t == brand::APP_NAME);
            if modern {
                io.note(&format!("[38;5;245m({})[0m", d.id()));
            } else {
                io.note(&format!("({})", d.id()));
            }
        } else if std::io::IsTerminal::is_terminal(&std::io::stderr()) {
            anstream::eprintln!("{}", ui::dim(format!("{} ›", d.id())));
        }
    }

    let after = temp_file("after", "json");
    let (status, before) = match d {
        Dialect::Cmd => {
            let comspec = std::env::var_os("ComSpec").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("cmd.exe"));
            let mut cmd = std::process::Command::new(comspec);
            // No space before `&`: cmd would keep it (`set X=1 &` sets "1 ").
            let script = format!(
                "/d /s /c \"{line}& call \"{}\" __capture \"{}\" %^errorlevel%\"",
                exe.display(),
                after.display()
            );
            raw_arg(&mut cmd, &script);
            if let Some(io) = &stdio {
                io.attach(&mut cmd)?;
            }
            #[cfg(windows)]
            rideshell_core::exec::ignore_ctrl_c();
            let status = cmd.status().context("cannot start cmd")?;
            let before: BTreeMap<String, String> =
                std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?))).collect();
            (status, Some(Captured { cwd: start_cwd.clone(), code: 0, env: before }))
        }
        Dialect::Bash => {
            let bash = shellcmd::detect(Shell::Bash).context("bash is not installed (Git for Windows provides it)")?;
            let before_file = temp_file("before", "json");
            let script_file = temp_file("line", "sh");
            let posix = |p: &Path| p.to_string_lossy().replace('\\', "/");
            let q = |p: &Path| rideshell_core::shells::posix_quote(&posix(p));
            let script = format!(
                "{exe} __capture {before} 0\n{line}\n__rideshell_status=$?\n{exe} __capture {after} \"$__rideshell_status\"\nexit $__rideshell_status\n",
                exe = q(&exe),
                before = q(&before_file),
                after = q(&after),
            );
            std::fs::write(&script_file, script).context("cannot write a temporary script")?;
            #[cfg(windows)]
            rideshell_core::exec::ignore_ctrl_c();
            let mut cmd = std::process::Command::new(bash);
            cmd.arg(posix(&script_file));
            if let Some(io) = &stdio {
                io.attach(&mut cmd)?;
            }
            let status = cmd.status().context("cannot start bash");
            let _ = std::fs::remove_file(&script_file);
            (status?, read_capture(&before_file))
        }
        Dialect::PowerShell => bail!("PowerShell lines run in PowerShell itself"),
    };

    let mut code = status.code().unwrap_or(1);
    if let (Some(after), Some(before)) = (read_capture(&after), before) {
        code = after.code;
        std::fs::write(out, apply_script(&start_cwd, &before, &after))
            .with_context(|| format!("cannot write {}", out.display()))?;
    }
    let _ = std::fs::remove_file(&after);
    Ok(code)
}

/// The console window itself, bypassing whatever PowerShell did to stdio.
struct ConsoleStdio {
    input: std::fs::File,
    output: std::fs::File,
}

impl ConsoleStdio {
    fn open() -> Result<Self> {
        let (inp, outp) = if cfg!(windows) { ("CONIN$", "CONOUT$") } else { ("/dev/tty", "/dev/tty") };
        let input = std::fs::OpenOptions::new().read(true).write(true).open(inp).context("no console input")?;
        let output = std::fs::OpenOptions::new().read(true).write(true).open(outp).context("no console output")?;
        Ok(Self { input, output })
    }

    fn attach(&self, cmd: &mut std::process::Command) -> Result<()> {
        cmd.stdin(self.input.try_clone()?).stdout(self.output.try_clone()?).stderr(self.output.try_clone()?);
        Ok(())
    }

    /// Plain ASCII: the console's code page may not be UTF-8.
    fn note(&self, text: &str) {
        use std::io::Write;
        let _ = writeln!(&self.output, "{text}");
    }
}

#[cfg(windows)]
fn raw_arg(cmd: &mut std::process::Command, s: &str) {
    use std::os::windows::process::CommandExt;
    cmd.raw_arg(s);
}

#[cfg(not(windows))]
fn raw_arg(cmd: &mut std::process::Command, s: &str) {
    cmd.arg(s);
}

/// PowerShell code that moves to the new directory and applies variable
/// changes between `before` and `after` (names compared case-insensitively).
fn apply_script(start_cwd: &Path, before: &Captured, after: &Captured) -> String {
    let mut out = brand::emit_marker();
    out.push('\n');
    let norm = |p: &Path| p.to_string_lossy().trim_end_matches('\\').to_ascii_lowercase();
    if !after.cwd.as_os_str().is_empty() && norm(&after.cwd) != norm(start_cwd) {
        out.push_str(&format!("Set-Location -LiteralPath {}\n", ps_literal(&after.cwd.to_string_lossy())));
    }
    let upper = |m: &BTreeMap<String, String>| -> BTreeMap<String, (String, String)> {
        m.iter().filter(|(k, _)| !internal(k)).map(|(k, v)| (k.to_ascii_uppercase(), (k.clone(), v.clone()))).collect()
    };
    let (b, a) = (upper(&before.env), upper(&after.env));
    for (key, (name, value)) in &a {
        if b.get(key).map(|(_, v)| v) != Some(value) {
            out.push_str(&format!(
                "[Environment]::SetEnvironmentVariable({}, {})\n",
                ps_literal(name),
                ps_literal(value)
            ));
        }
    }
    for (key, (name, _)) in &b {
        if !a.contains_key(key) {
            out.push_str(&format!("[Environment]::SetEnvironmentVariable({}, $null)\n", ps_literal(name)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_reference() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        // PowerShell: [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes('dir'))
        assert_eq!(encode_command("dir"), "ZABpAHIA");
    }

    #[test]
    fn apply_script_diffs_case_insensitively() {
        let env = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let before =
            Captured { cwd: "C:\\a".into(), code: 0, env: env(&[("Path", "x"), ("GONE", "1"), ("SAME", "s")]) };
        let after = Captured {
            cwd: "C:\\b".into(),
            code: 0,
            env: env(&[("PATH", "x"), ("SAME", "s"), ("NEW", "it's"), ("=C:", "C:\\b"), ("PWD", "/c/b")]),
        };
        let s = apply_script(Path::new("C:\\a"), &before, &after);
        assert!(s.starts_with(&brand::emit_marker()));
        assert!(s.contains("Set-Location -LiteralPath 'C:\\b'"));
        assert!(s.contains("SetEnvironmentVariable('NEW', 'it''s')"));
        assert!(s.contains("SetEnvironmentVariable('GONE', $null)"));
        assert!(
            !s.contains("'PATH'")
                && !s.contains("'Path'")
                && !s.contains("SAME")
                && !s.contains("=C:")
                && !s.contains("PWD")
        );
    }
}
