//! Which shell should run a command line?
//!
//! A RideShell window is hosted by PowerShell, but people type commands from
//! cmd and bash out of habit (or copy them from documentation). Each line is
//! classified before it runs:
//!
//! * an explicit prefix wins: `@cmd`, `@bash`, `@ps` / `@pwsh`;
//! * lines that only make sense in cmd (`dir /s`, `set X=1`, `%VAR%`, `>nul`,
//!   `if exist`, `a && b` where PowerShell cannot parse it) go to cmd;
//! * lines that only make sense in bash (`ls -la`, `export X=1`, `FOO=1 cmd`,
//!   `grep` when PowerShell has no such command) go to bash, if a bash is
//!   installed;
//! * everything else stays in PowerShell.
//!
//! The rules are deliberately conservative: a line that is valid PowerShell
//! and means the same thing there is never moved. PowerShell supplies
//! [`Hints`] (does it parse? what is each command name?) so the classifier
//! does not have to guess what PowerShell knows.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    PowerShell,
    Cmd,
    Bash,
}

impl Dialect {
    pub fn id(self) -> &'static str {
        match self {
            Dialect::PowerShell => "pwsh",
            Dialect::Cmd => "cmd",
            Dialect::Bash => "bash",
        }
    }

    pub fn parse(s: &str) -> Option<Dialect> {
        match s.to_ascii_lowercase().as_str() {
            "pwsh" | "ps" | "powershell" => Some(Dialect::PowerShell),
            "cmd" => Some(Dialect::Cmd),
            "bash" | "sh" => Some(Dialect::Bash),
            _ => None,
        }
    }
}

impl fmt::Display for Dialect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// How PowerShell resolves a command name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    Cmdlet,
    Function,
    Alias,
    Application,
    Script,
    /// PowerShell has no command with this name.
    Missing,
}

impl CommandKind {
    pub fn parse(s: &str) -> CommandKind {
        match s.to_ascii_lowercase().as_str() {
            "cmdlet" => CommandKind::Cmdlet,
            "function" | "filter" | "configuration" => CommandKind::Function,
            "alias" => CommandKind::Alias,
            "application" => CommandKind::Application,
            "externalscript" | "script" => CommandKind::Script,
            _ => CommandKind::Missing,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Hints {
    /// PowerShell's parser reported errors for the line.
    pub ps_parse_errors: bool,
    /// Command names in the line as PowerShell resolves them. `None` means
    /// unknown (no hint was given), which never triggers a switch on its own.
    pub commands: Option<Vec<(String, CommandKind)>>,
    /// A bash is installed (Git Bash on Windows).
    pub bash_available: bool,
}

impl Hints {
    /// Parse `name=kind;name=kind` as produced by the PowerShell side.
    pub fn parse_commands(s: &str) -> Vec<(String, CommandKind)> {
        s.split(';')
            .filter_map(|pair| {
                let (name, kind) = pair.rsplit_once('=')?;
                (!name.is_empty()).then(|| (name.to_string(), CommandKind::parse(kind)))
            })
            .collect()
    }

    fn kind(&self, name: &str) -> Option<CommandKind> {
        self.commands.as_ref()?.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, k)| *k)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub dialect: Dialect,
    /// The line to run (an explicit prefix removed).
    pub line: String,
    /// Short human-readable reason, for `rideshell dialect`.
    pub reason: &'static str,
}

fn decide(dialect: Dialect, line: &str, reason: &'static str) -> Decision {
    Decision { dialect, line: line.to_string(), reason }
}

/// Classify `line`.
pub fn classify(line: &str, hints: &Hints) -> Decision {
    let t = line.trim();
    if let Some((dialect, rest)) = explicit_prefix(t) {
        return decide(dialect, rest, "explicit prefix");
    }
    if t.is_empty() || t.starts_with('#') {
        return decide(Dialect::PowerShell, line, "default");
    }
    let segments = segments(t);
    let first_words = words(segments.first().map(String::as_str).unwrap_or(""));
    let first = first_words.first().map(|w| w.to_ascii_lowercase()).unwrap_or_default();
    let args = &first_words[first_words.len().min(1)..];

    if hints.bash_available {
        if let Some(reason) = bash_reason(t, &segments, &first, args, hints) {
            return decide(Dialect::Bash, line, reason);
        }
    }
    if let Some(reason) = cmd_reason(t, &first, args, hints) {
        return decide(Dialect::Cmd, line, reason);
    }
    decide(Dialect::PowerShell, line, "valid PowerShell")
}

fn explicit_prefix(t: &str) -> Option<(Dialect, &str)> {
    let rest = t.strip_prefix('@')?;
    let (word, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    Some((Dialect::parse(word)?, tail.trim_start()))
}

// ----- cmd -------------------------------------------------------------------

/// Built-ins that exist only in cmd (PowerShell has no command of that name,
/// or a different one, like the `prompt` function).
const CMD_ONLY: &[&str] = &[
    "ver", "vol", "mklink", "assoc", "ftype", "call", "goto", "setlocal", "endlocal", "rem", "prompt", "break",
    "verify", "dpath", "title", "color",
];

/// cmd built-ins that PowerShell maps to an alias with different options.
/// With `/x` switches they are certainly meant for cmd.
const CMD_WITH_PS_ALIAS: &[&str] = &[
    "dir", "copy", "del", "erase", "move", "ren", "rename", "type", "rd", "rmdir", "md", "mkdir", "cd", "chdir",
    "echo", "start", "cls", "sort", "where", "set",
];

fn cmd_reason(t: &str, first: &str, args: &[String], hints: &Hints) -> Option<&'static str> {
    if has_percent_var(t) {
        return Some("%VARIABLE% syntax");
    }
    if has_nul_redirect(t) {
        return Some(">nul redirection");
    }
    if t.ends_with('^') {
        return Some("^ line continuation");
    }
    if CMD_ONLY.contains(&first) {
        // `title`/`color`/`prompt` could be user functions in PowerShell.
        match hints.kind(first) {
            Some(CommandKind::Function | CommandKind::Cmdlet | CommandKind::Script) if !matches!(first, "prompt") => {
                return None
            }
            _ => return Some("cmd built-in"),
        }
    }
    if first == "set" && is_cmd_set(args) {
        return Some("cmd `set`");
    }
    if first == "echo" && args.first().is_some_and(|a| a.eq_ignore_ascii_case("off") || a.eq_ignore_ascii_case("on"))
        || first.starts_with("echo.")
    {
        return Some("cmd `echo`");
    }
    if first == "if" {
        if let Some(a) = args.first() {
            let a = a.to_ascii_lowercase();
            if matches!(a.as_str(), "exist" | "not" | "defined" | "errorlevel" | "/i") {
                return Some("cmd `if`");
            }
        }
    }
    if first == "for" && args.first().is_some_and(|a| a.starts_with('/') || a.starts_with('%')) {
        return Some("cmd `for`");
    }
    if CMD_WITH_PS_ALIAS.contains(&first) && args.iter().any(|a| is_slash_switch(a)) {
        return Some("cmd-style /switch");
    }
    if hints.ps_parse_errors && (t.contains("&&") || t.contains("||") || has_bare_ampersand(t)) {
        return Some("&& / & chaining that PowerShell cannot parse");
    }
    None
}

/// `set`, `set X`, `set X=1`, `set "X=1"`, `set /a ...`, `set /p ...`.
/// PowerShell's `set` (Set-Variable) is used with `-Name`/`-Value` or two words.
fn is_cmd_set(args: &[String]) -> bool {
    match args.first() {
        None => true,
        Some(a) if a.starts_with('/') => true,
        Some(a) if a.starts_with('"') && a.contains('=') => true,
        Some(a) if a.starts_with('-') || a.starts_with('$') => false,
        Some(a) => a.contains('=') || args.len() == 1,
    }
}

/// `/s`, `/b`, `/ad`, `/a:h`, `/?`. Not paths like `/Users`.
fn is_slash_switch(a: &str) -> bool {
    let Some(rest) = a.strip_prefix('/') else { return false };
    let (name, _) = rest.split_once(':').unwrap_or((rest, ""));
    (1..=2).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphabetic() || c == '?')
}

fn has_percent_var(t: &str) -> bool {
    let b = t.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let start = i + 1;
            let mut j = start;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b"_()#$-.".contains(&b[j])) {
                j += 1;
            }
            if j < b.len() && b[j] == b'%' && j > start && b[start].is_ascii_alphabetic() {
                return true;
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    false
}

fn has_nul_redirect(t: &str) -> bool {
    let l = t.to_ascii_lowercase();
    l.split_whitespace().any(|w| matches!(w, ">nul" | "1>nul" | "2>nul" | ">>nul" | "2>&1>nul")) || l.contains("> nul")
}

/// A single `&` outside quotes (cmd's command separator; PowerShell's call
/// operator only appears at the start of a command).
fn has_bare_ampersand(t: &str) -> bool {
    let mut quote = None;
    let chars: Vec<char> = t.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '&') => {
                let prev = i.checked_sub(1).map(|p| chars[p]);
                let next = chars.get(i + 1).copied();
                if prev != Some('&') && next != Some('&') && prev.is_some_and(|p| p != '(' && p != '|') {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

// ----- bash ------------------------------------------------------------------

/// Commands that come with bash environments (Git Bash, MSYS2) but have no
/// PowerShell or Windows equivalent of the same name.
const POSIX_TOOLS: &[&str] = &[
    "grep",
    "egrep",
    "fgrep",
    "sed",
    "awk",
    "gawk",
    "head",
    "tail",
    "wc",
    "touch",
    "chmod",
    "chown",
    "which",
    "xargs",
    "less",
    "cut",
    "tr",
    "uniq",
    "ln",
    "basename",
    "dirname",
    "realpath",
    "printenv",
    "du",
    "df",
    "tee",
    "nano",
    "vi",
    "vim",
    "env",
    "file",
    "readlink",
    "seq",
    "yes",
    "column",
    "md5sum",
    "sha256sum",
    "base64",
    "uname",
    "whoami",
    "id",
    "printf",
    "test",
    "true",
    "false",
    "nl",
    "rev",
    "split",
    "stat",
    "watch",
    "ssh-add",
];

/// PowerShell aliases that mimic POSIX tools; with POSIX-style flags they
/// fail in PowerShell (e.g. `ls -la`, `rm -rf`).
const POSIX_ALIASES: &[(&str, &[&str])] = &[
    ("ls", GCI_PARAMS),
    ("rm", RI_PARAMS),
    ("cp", CI_PARAMS),
    ("mv", MI_PARAMS),
    ("cat", GC_PARAMS),
    ("ps", GPS_PARAMS),
    ("kill", KILL_PARAMS),
    ("sleep", &["seconds", "milliseconds", "duration"]),
    ("mkdir", &["path", "name", "value", "force", "credential", "itemtype"]),
    ("pwd", &["psprovider", "psdrive", "stack", "stackname"]),
    (
        "diff",
        &[
            "referenceobject",
            "differenceobject",
            "syncwindow",
            "property",
            "excludedifferent",
            "includeequal",
            "passthru",
            "culture",
            "casesensitive",
        ],
    ),
];

const COMMON_PARAMS: &[&str] = &[
    "verbose",
    "debug",
    "erroraction",
    "warningaction",
    "informationaction",
    "errorvariable",
    "warningvariable",
    "informationvariable",
    "outvariable",
    "outbuffer",
    "pipelinevariable",
    "whatif",
    "confirm",
];
const GCI_PARAMS: &[&str] = &[
    "path",
    "literalpath",
    "filter",
    "include",
    "exclude",
    "recurse",
    "depth",
    "force",
    "name",
    "attributes",
    "followsymlink",
    "directory",
    "file",
    "hidden",
    "readonly",
    "system",
];
const RI_PARAMS: &[&str] =
    &["path", "literalpath", "filter", "include", "exclude", "recurse", "force", "credential", "stream"];
const CI_PARAMS: &[&str] = &[
    "path",
    "literalpath",
    "destination",
    "container",
    "force",
    "filter",
    "include",
    "exclude",
    "recurse",
    "passthru",
    "credential",
    "tosession",
    "fromsession",
];
const MI_PARAMS: &[&str] =
    &["path", "literalpath", "destination", "force", "filter", "include", "exclude", "passthru", "credential"];
const GC_PARAMS: &[&str] = &[
    "path",
    "literalpath",
    "readcount",
    "totalcount",
    "tail",
    "filter",
    "include",
    "exclude",
    "force",
    "credential",
    "delimiter",
    "wait",
    "raw",
    "encoding",
    "stream",
    "head",
    "first",
    "last",
];
const GPS_PARAMS: &[&str] =
    &["name", "id", "inputobject", "includeusername", "computername", "module", "fileversioninfo"];
const KILL_PARAMS: &[&str] = &["name", "id", "inputobject", "passthru", "force"];

fn bash_reason(t: &str, segments: &[String], first: &str, args: &[String], hints: &Hints) -> Option<&'static str> {
    if is_env_assignment(first) {
        return Some("VAR=value prefix");
    }
    if matches!(first, "export" | "unset" | "source") {
        return Some("bash built-in");
    }
    if first == "alias" && args.first().is_some_and(|a| a.contains('=')) {
        return Some("bash alias");
    }
    let lower = first.to_ascii_lowercase();
    if lower.ends_with(".sh") {
        return Some("shell script");
    }
    if t.starts_with("if [") || t.starts_with("[[") || t.starts_with("[ ") || t.contains("; then") || t.contains("; do")
    {
        return Some("bash syntax");
    }
    if let Some((_, params)) = POSIX_ALIASES.iter().find(|(name, _)| *name == lower) {
        if matches!(hints.kind(first), None | Some(CommandKind::Alias)) && args.iter().any(|a| is_posix_flag(a, params))
        {
            return Some("POSIX-style flags");
        }
    }
    // A POSIX tool PowerShell cannot find, anywhere in a pipeline.
    if hints.commands.is_some() {
        for seg in segments {
            let w = words(seg);
            if let Some(name) = w.first() {
                let n = name.to_ascii_lowercase();
                if POSIX_TOOLS.contains(&n.as_str()) && hints.kind(name) == Some(CommandKind::Missing) {
                    return Some("POSIX tool not available in PowerShell");
                }
            }
        }
    }
    None
}

fn is_env_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else { return false };
    !name.is_empty()
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Single-letter flags that mean the same thing in PowerShell and POSIX
/// (`rm -r`, `ls -r` is close enough), so PowerShell may keep them.
const SHARED_SHORT_FLAGS: &[char] = &['r'];

/// `-la`, `-rf`, `-d`, `--color`: a POSIX flag rather than a PowerShell
/// parameter. Longer flags count as PowerShell when they are a prefix of one
/// of the cmdlet's parameters (`-Force`, `-rec`); single letters are POSIX
/// unless they mean the same in both.
fn is_posix_flag(a: &str, params: &[&str]) -> bool {
    if let Some(long) = a.strip_prefix("--") {
        return !long.is_empty();
    }
    let Some(name) = a.strip_prefix('-') else { return false };
    let name = name.split_once(':').map_or(name, |(n, _)| n);
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    if lower.len() == 1 {
        let c = lower.chars().next().unwrap_or_default();
        return c.is_ascii_alphanumeric() && !SHARED_SHORT_FLAGS.contains(&c);
    }
    !params.iter().chain(COMMON_PARAMS).any(|p| p.starts_with(&lower))
}

// ----- tokenising --------------------------------------------------------------

/// Split into pipeline / list segments on unquoted `|`, `;`, `&&`, `||`.
fn segments(t: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote = None;
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (None, '"' | '\'') => {
                quote = Some(c);
                cur.push(c);
            }
            (Some(q), c) if c == q => {
                quote = None;
                cur.push(c);
            }
            (None, '|' | ';') | (None, '&') if c != '&' || chars.peek() == Some(&'&') => {
                if c != ';' && chars.peek() == Some(&c) {
                    chars.next();
                }
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// Whitespace-separated words, keeping quoted runs together (quotes kept).
fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote = None;
    for c in s.chars() {
        match (quote, c) {
            (None, '"' | '\'') => {
                quote = Some(c);
                cur.push(c);
            }
            (Some(q), c) if c == q => {
                quote = None;
                cur.push(c);
            }
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hints(errors: bool, cmds: &str) -> Hints {
        Hints { ps_parse_errors: errors, commands: Some(Hints::parse_commands(cmds)), bash_available: true }
    }

    fn d(line: &str, h: &Hints) -> Dialect {
        classify(line, h).dialect
    }

    #[test]
    fn plain_powershell_stays() {
        let h = hints(false, "Get-ChildItem=cmdlet");
        for line in [
            "Get-ChildItem -Recurse",
            "ls",
            "ls -r",
            "ls -Force C:\\",
            "rm -Recurse foo",
            "cd ..",
            "cd C:\\Windows",
            "echo hello",
            "git status",
            "claude",
            "npm install",
            "$x = 5 % 3",
            "1..3 | % { $_ * 2 }",
            "set x 5",
            "set -Name x -Value 5",
            "cd /Users",
            "",
            "# comment",
        ] {
            assert_eq!(d(line, &h), Dialect::PowerShell, "{line}");
        }
    }

    #[test]
    fn cmd_lines() {
        let h = hints(false, "dir=alias;set=alias;echo=alias;copy=alias");
        for line in [
            "dir /s /b",
            "dir /ad",
            "copy a.txt b.txt /y",
            "del /q *.tmp",
            "rd /s /q build",
            "cd /d D:\\work",
            "set FOO=bar",
            "set \"FOO=bar baz\"",
            "set",
            "set PATH",
            "set /a x=1+2",
            "echo %PATH%",
            "echo off",
            "echo.",
            "ver",
            "mklink /j link target",
            "if exist foo.txt echo yes",
            "for %i in (*.txt) do echo %i",
            "for /f \"tokens=*\" %a in ('dir /b') do echo %a",
            "dir >nul 2>nul",
            "prompt $p$g",
        ] {
            assert_eq!(d(line, &h), Dialect::Cmd, "{line}");
        }
    }

    #[test]
    fn chaining_goes_to_cmd_only_when_powershell_cannot_parse_it() {
        assert_eq!(d("npm install && npm test", &hints(true, "npm=application")), Dialect::Cmd);
        assert_eq!(d("npm install && npm test", &hints(false, "npm=application")), Dialect::PowerShell);
        assert_eq!(d("make & echo done", &hints(true, "make=application")), Dialect::Cmd);
        assert_eq!(d("& 'C:\\a b\\x.exe' arg", &hints(false, "")), Dialect::PowerShell);
    }

    #[test]
    fn bash_lines() {
        let h = hints(false, "ls=alias;rm=alias;grep=missing;wc=missing;cat=alias;head=missing");
        for line in [
            "ls -la",
            "ls -d etc",
            "ls -l",
            "ls -lh /tmp",
            "rm -rf node_modules",
            "ls --color=auto",
            "export FOO=bar",
            "unset FOO",
            "source venv/bin/activate",
            "FOO=1 npm test",
            "./build.sh",
            "grep -rn TODO src",
            "cat file.txt | grep foo | wc -l",
            "if [ -f x ]; then echo yes; fi",
            "for f in *.txt; do echo $f; done",
            "alias ll='ls -la'",
        ] {
            assert_eq!(d(line, &h), Dialect::Bash, "{line}");
        }
    }

    #[test]
    fn installed_tools_are_not_moved() {
        // grep installed as a Windows program: PowerShell runs it fine.
        let h = hints(false, "cat=alias;grep=application");
        assert_eq!(d("cat file.txt | grep foo", &h), Dialect::PowerShell);
        // A user function named `ls` is respected.
        let h = hints(false, "ls=function");
        assert_eq!(d("ls -la", &h), Dialect::PowerShell);
    }

    #[test]
    fn without_bash_nothing_goes_to_bash() {
        let mut h = hints(false, "ls=alias;grep=missing");
        h.bash_available = false;
        assert_eq!(d("ls -la", &h), Dialect::PowerShell);
        assert_eq!(d("grep foo x", &h), Dialect::PowerShell);
        assert_eq!(d("dir /s", &h), Dialect::Cmd);
    }

    #[test]
    fn explicit_prefixes() {
        let h = hints(false, "");
        let dec = classify("@cmd dir", &h);
        assert_eq!((dec.dialect, dec.line.as_str()), (Dialect::Cmd, "dir"));
        let dec = classify("  @bash ls -la", &h);
        assert_eq!((dec.dialect, dec.line.as_str()), (Dialect::Bash, "ls -la"));
        let dec = classify("@ps set FOO=bar", &h);
        assert_eq!((dec.dialect, dec.line.as_str()), (Dialect::PowerShell, "set FOO=bar"));
        // `@(...)` and `@{...}` are PowerShell syntax, not prefixes.
        assert_eq!(d("@(1,2) | % { $_ }", &h), Dialect::PowerShell);
        assert_eq!(d("@{a=1}", &h), Dialect::PowerShell);
    }

    #[test]
    fn helpers() {
        assert!(is_slash_switch("/s") && is_slash_switch("/a:h") && is_slash_switch("/?") && is_slash_switch("/ad"));
        assert!(!is_slash_switch("/Users") && !is_slash_switch("/") && !is_slash_switch("C:/x"));
        assert!(has_percent_var("echo %USERPROFILE%") && has_percent_var("%ProgramFiles(x86)%"));
        assert!(!has_percent_var("5 % 3 % 2") && !has_percent_var("% { $_ }") && !has_percent_var("100%"));
        assert_eq!(segments("a | b && c; d || e"), vec!["a", "b", "c", "d", "e"]);
        assert_eq!(segments("echo 'a|b' | x"), vec!["echo 'a|b'", "x"]);
        assert_eq!(words("git commit -m \"a b\""), vec!["git", "commit", "-m", "\"a b\""]);
        assert!(
            is_posix_flag("-la", GCI_PARAMS)
                && !is_posix_flag("-r", GCI_PARAMS)
                && is_posix_flag("-d", GCI_PARAMS)
                && !is_posix_flag("-Force", GCI_PARAMS)
        );
        assert!(is_env_assignment("FOO=1") && !is_env_assignment("=x") && !is_env_assignment("a-b=1"));
    }
}
