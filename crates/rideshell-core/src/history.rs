//! Conversation history of AI coding tools across RideShell accounts.
//!
//! Claude Code and Codex keep each conversation ("transcript") and the prompt
//! history inside their config folder. A RideShell account with its own
//! folder therefore starts without the conversations you had before. Two
//! ways to bring them along, chosen per account:
//!
//! * **adopt**: the account uses the tool's original folder (`~/.claude`,
//!   `~/.codex`), so login *and* history are the ones you already had. Any
//!   conversations the account had in its own folder are moved over first.
//! * **share**: the account keeps its own login, but its conversation folder
//!   is a link (a directory junction on Windows) to the original one, and its
//!   prompt-history file is a hard link to the original. Both accounts see
//!   the same past conversations.
//!
//! Only conversation files are moved or linked. Login files (tokens) are
//! never touched.

use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{bail, Context, Result};

/// What holds a tool's history inside its config folder.
pub struct HistorySpec {
    /// Folders of conversations (moved on adopt, linked on share).
    pub dirs: &'static [&'static str],
    /// Line-based history files (merged on adopt, hard-linked on share).
    pub files: &'static [&'static str],
}

/// History layout of tools that have conversations.
pub fn spec(provider_id: &str) -> Option<HistorySpec> {
    match provider_id {
        // projects/: one .jsonl per conversation (what `claude --resume` lists); history.jsonl: prompt history.
        "claude" => Some(HistorySpec { dirs: &["projects"], files: &["history.jsonl"] }),
        // sessions/: rollouts per conversation; history.jsonl: prompt history.
        "codex" => Some(HistorySpec { dirs: &["sessions"], files: &["history.jsonl"] }),
        _ => None,
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Moved {
    pub files: usize,
    /// Files that already existed at the destination and were left where they were.
    pub skipped: usize,
}

/// Move conversations from `from` into `to` (adopt). Never overwrites.
pub fn merge_into(spec: &HistorySpec, from: &Path, to: &Path) -> Result<Moved> {
    let mut moved = Moved::default();
    for d in spec.dirs {
        let src = from.join(d);
        if src.is_dir() && !is_link(&src) {
            move_tree(&src, &to.join(d), &mut moved)?;
        }
    }
    for f in spec.files {
        let src = from.join(f);
        if src.is_file() && !same_file(&src, &to.join(f)) {
            append_new_lines(&src, &to.join(f))?;
            fs::remove_file(&src).ok();
            moved.files += 1;
        }
    }
    Ok(moved)
}

fn move_tree(src: &Path, dst: &Path, moved: &mut Moved) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("cannot create {}", dst.display()))?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            move_tree(&path, &target, moved)?;
            let _ = fs::remove_dir(&path); // only if now empty
        } else if target.exists() {
            moved.skipped += 1;
        } else {
            if fs::rename(&path, &target).is_err() {
                fs::copy(&path, &target).with_context(|| format!("cannot move {}", path.display()))?;
                fs::remove_file(&path).ok();
            }
            moved.files += 1;
        }
    }
    Ok(())
}

/// Append the lines of `src` that `dst` does not have yet.
fn append_new_lines(src: &Path, dst: &Path) -> Result<()> {
    let existing = fs::read_to_string(dst).unwrap_or_default();
    let have: std::collections::HashSet<&str> = existing.lines().collect();
    let new: Vec<String> =
        fs::read_to_string(src)?.lines().filter(|l| !l.is_empty() && !have.contains(l)).map(str::to_string).collect();
    if new.is_empty() {
        return Ok(());
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::OpenOptions::new().create(true).append(true).open(dst)?;
    if !existing.is_empty() && !existing.ends_with('\n') {
        writeln!(f)?;
    }
    for l in new {
        writeln!(f, "{l}")?;
    }
    Ok(())
}

/// Is the account's history shared (its first history folder is a link)?
pub fn is_shared(spec: &HistorySpec, account_dir: &Path) -> bool {
    spec.dirs.first().is_some_and(|d| is_link(&account_dir.join(d)))
}

/// Share history: link the account's history to `original` (keeps the
/// account's own login). Conversations the account already had are moved
/// into the original first, so nothing is lost.
pub fn share(spec: &HistorySpec, account_dir: &Path, original: &Path) -> Result<Moved> {
    if same_dir(account_dir, original) {
        bail!("this account already uses that folder");
    }
    fs::create_dir_all(account_dir)?;
    let moved = merge_into(spec, account_dir, original)?;
    for d in spec.dirs {
        let link = account_dir.join(d);
        let target = original.join(d);
        fs::create_dir_all(&target)?;
        if is_link(&link) {
            continue;
        }
        if link.exists() {
            // Left over after the merge (e.g. skipped duplicates): keep it aside, never delete.
            let aside = account_dir.join(format!("{d}.before-sharing"));
            fs::rename(&link, &aside).with_context(|| format!("cannot move {} aside", link.display()))?;
        }
        link_dir(&target, &link)?;
    }
    for f in spec.files {
        let link = account_dir.join(f);
        let target = original.join(f);
        if !target.exists() {
            fs::File::create(&target)?;
        }
        if link.exists() && same_file(&link, &target) {
            continue;
        }
        let _ = fs::remove_file(&link); // its lines were merged above
        if fs::hard_link(&target, &link).is_err() {
            // Different drive: hard links are impossible; prompt history stays per account.
        }
    }
    Ok(moved)
}

/// Stop sharing: remove the links. The shared conversations stay in the
/// original folder; this account starts its own history again.
pub fn unshare(spec: &HistorySpec, account_dir: &Path) -> Result<()> {
    for d in spec.dirs {
        let link = account_dir.join(d);
        if is_link(&link) {
            remove_link(&link)?;
        }
    }
    for f in spec.files {
        let link = account_dir.join(f);
        if link.is_file() {
            // Turn the hard link into a private copy.
            let content = fs::read(&link)?;
            fs::remove_file(&link)?;
            fs::write(&link, content)?;
        }
    }
    Ok(())
}

/// Number of conversations stored in a config folder.
pub fn conversation_count(spec: &HistorySpec, dir: &Path) -> usize {
    fn count(p: &Path) -> usize {
        fs::read_dir(p)
            .map(|rd| {
                rd.flatten()
                    .map(|e| {
                        let path = e.path();
                        if path.is_dir() {
                            count(&path)
                        } else {
                            usize::from(path.extension().is_some_and(|x| x == "jsonl"))
                        }
                    })
                    .sum()
            })
            .unwrap_or(0)
    }
    spec.dirs.iter().map(|d| count(&dir.join(d))).sum()
}

fn is_link(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink())
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (dunce::canonicalize(a), dunce::canonicalize(b)) {
        (Ok(a), Ok(b)) => crate::pathkey::path_key(&a).ok() == crate::pathkey::path_key(&b).ok(),
        _ => false,
    }
}

#[cfg(windows)]
fn same_file(a: &Path, b: &Path) -> bool {
    match (dunce::canonicalize(a), dunce::canonicalize(b)) {
        (Ok(x), Ok(y)) if x == y => true,
        _ => file_id(a).is_some() && file_id(a) == file_id(b),
    }
}

#[cfg(windows)]
fn file_id(p: &Path) -> Option<(u32, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
    let f = fs::File::open(p).ok()?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetFileInformationByHandle(f.as_raw_handle() as _, &mut info) };
    (ok != 0)
        .then(|| (info.dwVolumeSerialNumber, (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow)))
}

#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

/// A directory link that needs no admin rights: a junction on Windows.
#[cfg(windows)]
fn link_dir(target: &Path, link: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    let status = std::process::Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .stdout(std::process::Stdio::null())
        .status()
        .context("cannot run mklink")?;
    if !status.success() || !is_link(link) {
        bail!("cannot link {} to {}", link.display(), target.display());
    }
    Ok(())
}

#[cfg(unix)]
fn link_dir(target: &Path, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link).with_context(|| format!("cannot link {}", link.display()))
}

/// Remove a link without touching what it points to.
fn remove_link(link: &Path) -> Result<()> {
    // A junction / directory symlink is removed with remove_dir; a file symlink with remove_file.
    fs::remove_dir(link).or_else(|_| fs::remove_file(link)).with_context(|| format!("cannot remove {}", link.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &Path, s: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, s).unwrap();
    }

    #[test]
    fn adopt_moves_conversations_and_merges_prompt_history() {
        let t = tempfile::tempdir().unwrap();
        let (acct, orig) = (t.path().join("acct"), t.path().join("orig"));
        write(&acct.join("projects/C--code/a.jsonl"), "a");
        write(&acct.join("projects/C--code/dup.jsonl"), "acct copy");
        write(&orig.join("projects/C--code/dup.jsonl"), "orig copy");
        write(&acct.join("history.jsonl"), "one\ntwo\n");
        write(&orig.join("history.jsonl"), "zero\ntwo\n");
        write(&acct.join(".credentials.json"), "token");
        let moved = merge_into(&spec("claude").unwrap(), &acct, &orig).unwrap();
        assert_eq!(moved, Moved { files: 2, skipped: 1 });
        assert_eq!(fs::read_to_string(orig.join("projects/C--code/a.jsonl")).unwrap(), "a");
        assert_eq!(
            fs::read_to_string(orig.join("projects/C--code/dup.jsonl")).unwrap(),
            "orig copy",
            "never overwritten"
        );
        assert_eq!(fs::read_to_string(orig.join("history.jsonl")).unwrap(), "zero\ntwo\none\n");
        assert_eq!(fs::read_to_string(acct.join(".credentials.json")).unwrap(), "token", "login files untouched");
    }

    #[test]
    fn share_links_and_unshare_restores_a_separate_history() {
        let t = tempfile::tempdir().unwrap();
        let (acct, orig) = (t.path().join("acct"), t.path().join("orig"));
        write(&orig.join("projects/P/old.jsonl"), "old");
        write(&acct.join("projects/P/mine.jsonl"), "mine");
        let s = spec("claude").unwrap();
        share(&s, &acct, &orig).unwrap();
        assert!(is_shared(&s, &acct));
        assert!(acct.join("projects/P/old.jsonl").is_file(), "sees the original conversations");
        assert!(orig.join("projects/P/mine.jsonl").is_file(), "its own conversations were kept");
        write(&acct.join("projects/P/new.jsonl"), "new");
        assert!(orig.join("projects/P/new.jsonl").is_file(), "new conversations land in the shared folder");
        assert_eq!(conversation_count(&s, &acct), 3);

        unshare(&s, &acct).unwrap();
        assert!(!is_shared(&s, &acct));
        assert!(orig.join("projects/P/old.jsonl").is_file(), "unsharing never deletes conversations");
    }
}
