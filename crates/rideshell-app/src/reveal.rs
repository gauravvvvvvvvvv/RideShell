//! Clickable paths in terminal output: show the file or folder in Explorer.
//!
//! The terminal underlines things that look like paths (`C:\x\y.txt`,
//! `~\x`, `/c/x`, `src/main.rs:12:5`, `"C:\Program Files\x"`); a click sends
//! the text and the tab's folder here. A file opens Explorer with the file
//! selected, a folder opens the folder. If the path does not exist, the
//! nearest existing parent folder opens instead.

use std::path::{Path, PathBuf};

/// Turn clicked text into an absolute path (it may not exist).
pub fn resolve(text: &str, cwd: Option<&Path>, home: Option<&Path>, var: impl Fn(&str) -> Option<String>) -> PathBuf {
    let mut s = text.trim().trim_matches(['"', '\'', '`']).to_string();
    // `file.rs:12` / `file.rs:12:5` (compiler and grep output): drop the position.
    s = strip_position(&s).to_string();
    // %USERPROFILE%\x and other %VAR% references.
    while let Some(a) = s.find('%') {
        let Some(len) = s[a + 1..].find('%') else { break };
        let Some(v) = var(&s[a + 1..a + 1 + len]) else { break };
        s.replace_range(a..a + len + 2, &v);
    }
    // Git Bash style /c/Users/x → C:\Users\x.
    let b = s.as_bytes();
    if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b'/' {
        s = format!("{}:\\{}", (b[1] as char).to_ascii_uppercase(), &s[3..]);
    }
    let p = if let Some(rest) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\")) {
        home.map(|h| h.join(rest)).unwrap_or_else(|| PathBuf::from(&s))
    } else if s == "~" {
        home.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(&s))
    } else {
        let p = PathBuf::from(s.replace('/', "\\"));
        match cwd {
            Some(c) if p.is_relative() => c.join(p),
            _ => p,
        }
    };
    PathBuf::from(p.to_string_lossy().replace('/', "\\"))
}

fn strip_position(s: &str) -> &str {
    let mut out = s;
    for _ in 0..2 {
        if let Some(i) = out.rfind(':') {
            // Keep the drive colon (C:\...).
            if i > 1 && out[i + 1..].chars().all(|c| c.is_ascii_digit()) && !out[i + 1..].is_empty() {
                out = &out[..i];
            }
        }
    }
    out
}

/// What a click does: (what to open, select the file in it?).
pub fn target(p: &Path) -> Option<(PathBuf, bool)> {
    if p.is_file() {
        return Some((p.to_path_buf(), true));
    }
    if p.is_dir() {
        return Some((p.to_path_buf(), false));
    }
    p.ancestors().skip(1).find(|a| a.is_dir() && !a.as_os_str().is_empty()).map(|a| (a.to_path_buf(), false))
}

/// Show `p` in Explorer. Returns false when nothing on the path exists.
pub fn reveal(p: &Path) -> bool {
    let Some((path, select)) = target(p) else { return false };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("explorer.exe");
        if select {
            cmd.raw_arg(format!("/select,\"{}\"", path.display()));
        } else {
            cmd.arg(&path);
        }
        cmd.spawn().is_ok()
    }
    #[cfg(not(windows))]
    {
        let _ = select;
        rideshell_core::platform::open_with_default(&path.to_string_lossy()).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(text: &str) -> String {
        let var = |n: &str| (n == "USERPROFILE").then(|| r"C:\Users\me".to_string());
        resolve(text, Some(Path::new(r"C:\code\app")), Some(Path::new(r"C:\Users\me")), var).display().to_string()
    }

    #[test]
    fn resolves_the_path_forms_terminals_print() {
        assert_eq!(r(r"C:\Users\me\.rideshell-release\signing.key"), r"C:\Users\me\.rideshell-release\signing.key");
        assert_eq!(r(r#""C:\Program Files\GitHub CLI\gh.exe""#), r"C:\Program Files\GitHub CLI\gh.exe");
        assert_eq!(r("`C:/Users/me/x.txt`"), r"C:\Users\me\x.txt");
        assert_eq!(r("/c/Users/me/x"), r"C:\Users\me\x");
        assert_eq!(r(r"~\Videos\demo.mp4"), r"C:\Users\me\Videos\demo.mp4");
        assert_eq!(r("~/x"), r"C:\Users\me\x");
        assert_eq!(r(r"%USERPROFILE%\Downloads"), r"C:\Users\me\Downloads");
        assert_eq!(r("src/main.rs:12:5"), r"C:\code\app\src\main.rs");
        assert_eq!(r("crates/x/lib.rs:7"), r"C:\code\app\crates\x\lib.rs");
        assert_eq!(r(r"C:\x\y.rs:42"), r"C:\x\y.rs");
        assert_eq!(r(r"C:\"), r"C:\");
    }

    #[test]
    fn missing_paths_fall_back_to_the_nearest_folder() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("a.txt");
        std::fs::write(&f, "x").unwrap();
        assert_eq!(target(&f), Some((f.clone(), true)));
        assert_eq!(target(t.path()), Some((t.path().to_path_buf(), false)));
        assert_eq!(target(&t.path().join("gone").join("b.txt")), Some((t.path().to_path_buf(), false)));
    }
}

#[cfg(all(test, windows))]
mod live {
    #[test]
    #[ignore = "opens an Explorer window"]
    fn reveal_opens_explorer() {
        let p = std::path::PathBuf::from(std::env::var("REVEAL_TEST_FILE").unwrap());
        assert!(super::reveal(&p));
        std::thread::sleep(std::time::Duration::from_secs(4));
    }
}
