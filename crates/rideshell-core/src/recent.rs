//! "Recently worked" folders, gathered from tools already on this PC.
//!
//! Read-only and local: nothing here is sent anywhere. Sources:
//!
//! * VS Code / Cursor: their own "recently opened" list (`state.vscdb`).
//! * Claude Code and Codex: the working folder recorded at the start of each
//!   conversation, in every given config folder (all RideShell accounts plus
//!   the originals).
//!
//! Only folders that still exist are returned, newest first.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct WorkedFolder {
    pub path: String,
    /// "VS Code", "Cursor", "Claude Code", "Codex"
    pub source: &'static str,
    /// Unix seconds (best effort).
    pub when: u64,
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Folders worked in recently. `claude_dirs` / `codex_dirs` are config folders to scan.
pub fn worked_folders(claude_dirs: &[PathBuf], codex_dirs: &[PathBuf], limit: usize) -> Vec<WorkedFolder> {
    let mut all = Vec::new();
    if let Some(appdata) = dirs::config_dir() {
        for (app, source) in [("Code", "VS Code"), ("Cursor", "Cursor")] {
            all.extend(vscode_recent(
                &appdata.join(app).join("User").join("globalStorage").join("state.vscdb"),
                source,
            ));
        }
    }
    for d in claude_dirs {
        all.extend(transcript_folders(&d.join("projects"), "Claude Code", 2));
    }
    for d in codex_dirs {
        all.extend(transcript_folders(&d.join("sessions"), "Codex", 5));
    }
    // Newest per folder.
    let mut best: HashMap<String, WorkedFolder> = HashMap::new();
    for f in all {
        if !Path::new(&f.path).is_dir() {
            continue;
        }
        let key = f.path.trim_end_matches(['\\', '/']).to_ascii_lowercase();
        match best.get(&key) {
            Some(b) if b.when >= f.when => {}
            _ => {
                best.insert(key, f);
            }
        }
    }
    let mut out: Vec<WorkedFolder> = best.into_values().collect();
    out.sort_by_key(|f| std::cmp::Reverse(f.when));
    out.truncate(limit);
    out
}

/// VS Code's "recently opened" folders. Times are approximated by the folder's own
/// modification time (the list itself only has an order).
fn vscode_recent(db: &Path, source: &'static str) -> Vec<WorkedFolder> {
    if !db.is_file() {
        return Vec::new();
    }
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let Ok(conn) = rusqlite::Connection::open_with_flags(db, flags) else { return Vec::new() };
    let _ = conn.busy_timeout(std::time::Duration::from_millis(200));
    let Ok(json): Result<String, _> =
        conn.query_row("SELECT value FROM ItemTable WHERE key = 'history.recentlyOpenedPathsList'", [], |r| r.get(0))
    else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) else { return Vec::new() };
    let mut out = Vec::new();
    for e in v.get("entries").and_then(|e| e.as_array()).into_iter().flatten().take(30) {
        let Some(uri) = e.get("folderUri").and_then(|u| u.as_str()) else { continue };
        let Some(path) = file_uri_to_path(uri) else { continue };
        let when = std::fs::metadata(&path).and_then(|m| m.modified()).map(secs).unwrap_or(0);
        out.push(WorkedFolder { path, source, when });
    }
    out
}

/// `file:///c%3A/Users/me/code` -> `C:\Users\me\code` (local files only).
fn file_uri_to_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file:///")?;
    let decoded = percent_decode(rest);
    if cfg!(windows) {
        let mut p = decoded.replace('/', "\\");
        if p.len() >= 2 && p.as_bytes()[1] == b':' {
            p[..1].make_ascii_uppercase();
        }
        Some(p)
    } else {
        Some(format!("/{decoded}"))
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Working folders of conversations under `root` (newest file per folder).
/// Reads only the first lines of each file, looking for the `cwd` field.
fn transcript_folders(root: &Path, source: &'static str, max_depth: usize) -> Vec<WorkedFolder> {
    let mut files = Vec::new();
    collect_jsonl(root, max_depth, &mut files);
    files.sort_by_key(|f| std::cmp::Reverse(f.1));
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (path, when) in files.into_iter().take(60) {
        if let Some(cwd) = first_cwd(&path) {
            if seen.insert(cwd.to_ascii_lowercase()) {
                out.push(WorkedFolder { path: cwd, source, when });
            }
        }
    }
    out
}

fn collect_jsonl(dir: &Path, depth: usize, out: &mut Vec<(PathBuf, u64)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() && depth > 0 {
            collect_jsonl(&p, depth - 1, out);
        } else if ft.is_file() && p.extension().is_some_and(|x| x == "jsonl") {
            let when = e.metadata().and_then(|m| m.modified()).map(secs).unwrap_or(0);
            out.push((p, when));
        }
    }
}

fn first_cwd(file: &Path) -> Option<String> {
    let f = std::fs::File::open(file).ok()?;
    for line in BufReader::new(f).lines().take(25) {
        let line = line.ok()?;
        if !line.contains("\"cwd\"") {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(&line).ok()?;
        let cwd = v.get("cwd").or_else(|| v.get("payload").and_then(|p| p.get("cwd")))?;
        if let Some(s) = cwd.as_str() {
            return Some(s.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris() {
        if cfg!(windows) {
            assert_eq!(file_uri_to_path("file:///c%3A/Users/me/my%20code").as_deref(), Some(r"C:\Users\me\my code"));
        }
        assert_eq!(file_uri_to_path("vscode-remote://ssh/x"), None);
    }

    #[test]
    fn transcripts_give_working_folders() {
        let t = tempfile::tempdir().unwrap();
        let work = t.path().join("proj");
        std::fs::create_dir_all(&work).unwrap();
        let conv = t.path().join("cfg/projects/x");
        std::fs::create_dir_all(&conv).unwrap();
        let line = serde_json::json!({ "type": "user", "cwd": work.to_string_lossy() }).to_string();
        std::fs::write(conv.join("a.jsonl"), format!("{{\"type\":\"summary\"}}\n{line}\n")).unwrap();
        let got = worked_folders(&[t.path().join("cfg")], &[], 10);
        assert!(got.iter().any(|f| f.path == work.to_string_lossy() && f.source == "Claude Code"), "{got:?}");
    }
}
