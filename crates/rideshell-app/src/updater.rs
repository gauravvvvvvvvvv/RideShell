//! Updates from GitHub Releases, and the setup wizard's install step.
//!
//! * [`check`] asks GitHub for the latest release (one small HTTPS request,
//!   no ID, generic User-Agent) and reports it when it is newer.
//! * [`install_update`] downloads that release's `rideshell.exe`, checks it
//!   against the `rideshell.exe.sha256` published with it, and runs its
//!   installer without a window. RideShell App shows the steps as a wizard.
//! * [`run_installer`] is the last step on its own, for the first-install
//!   wizard (`rideshell-app --setup <rideshell.exe>`).
//!
//! Network access uses Windows' own `curl.exe` (System32), hidden. Everything
//! runs on background threads.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

/// Where official releases are published.
pub const REPO: &str = "gauravvvvvvvvvv/RideShell";
const ASSET: &str = "rideshell.exe";
const CHECKSUM: &str = "rideshell.exe.sha256";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    body: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    size: u64,
    browser_download_url: String,
}

/// `v1.2.3` / `1.2.3` → (1, 2, 3).
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>().ok());
    let v = (it.next()??, it.next().unwrap_or(Some(0))?, it.next().unwrap_or(Some(0))?);
    Some(v)
}

fn curl() -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let system = root.join("System32").join("curl.exe");
    if system.is_file() {
        system
    } else {
        PathBuf::from("curl")
    }
}

fn curl_cmd(url: &str, timeout_s: u32) -> Command {
    let mut cmd = Command::new(curl());
    cmd.args(["-fsSL", "--proto", "=https", "--max-time", &timeout_s.to_string()])
        .args(["-H", &format!("User-Agent: RideShell-updater/{}", env!("CARGO_PKG_VERSION"))])
        .args(["-H", "Accept: application/vnd.github+json"])
        .arg(url)
        .stdin(Stdio::null());
    rideshell_core::platform::no_window(&mut cmd);
    cmd
}

fn fetch_text(url: &str) -> Result<String> {
    let out = curl_cmd(url, 30).output().context("cannot run curl.exe")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim().trim_start_matches("curl: "));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn latest() -> Result<Release> {
    let text = fetch_text(&format!("https://api.github.com/repos/{REPO}/releases/latest"))?;
    serde_json::from_str(&text).context("unexpected answer from GitHub")
}

/// The newest release when it is newer than this build, as the UI shows it.
pub fn check() -> Result<Option<Value>> {
    let r = latest()?;
    let current = parse_version(env!("CARGO_PKG_VERSION"));
    let newer = matches!((parse_version(&r.tag_name), current), (Some(n), Some(c)) if n > c);
    if r.draft || r.prerelease || !newer || !r.assets.iter().any(|a| a.name == ASSET) {
        return Ok(None);
    }
    Ok(Some(json!({
        "version": r.tag_name.trim_start_matches(['v', 'V']),
        "current": env!("CARGO_PKG_VERSION"),
        "notes": r.body,
        "url": r.html_url,
    })))
}

/// A download link of an official release asset, and nothing else.
fn official(url: &str) -> Result<&str> {
    if url.starts_with(&format!("https://github.com/{REPO}/releases/download/")) {
        Ok(url)
    } else {
        bail!("unexpected download location {url}")
    }
}

fn sha256_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Progress for the wizard: `step` is download, verify, install or done.
pub type Progress<'a> = &'a dyn Fn(Value);

fn step(p: Progress, name: &str, pct: Option<f64>) {
    p(json!({ "t": "wizard", "step": name, "pct": pct }));
}

/// Download, verify and install the latest release.
pub fn install_update(p: Progress) -> Result<String> {
    let (file, version) = download_latest(p)?;
    run_installer(&file, p)?;
    Ok(version)
}

/// Download the latest release's rideshell.exe and check its SHA-256.
/// Returns the verified file and the version.
pub fn download_latest(p: Progress) -> Result<(PathBuf, String)> {
    let r = latest()?;
    let version = r.tag_name.trim_start_matches(['v', 'V']).to_string();
    let exe = r.assets.iter().find(|a| a.name == ASSET).context("the release has no rideshell.exe")?;
    let sum = r
        .assets
        .iter()
        .find(|a| a.name == CHECKSUM)
        .context("the release has no checksum file, so it cannot be verified")?;

    // 1. Download (progress from the file's size as curl writes it).
    step(p, "download", Some(0.0));
    let dir = std::env::temp_dir().join(format!("RideShell-update-{version}"));
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let file = dir.join(ASSET);
    let _ = std::fs::remove_file(&file);
    let mut child = curl_cmd(official(&exe.browser_download_url)?, 900)
        .arg("-o")
        .arg(&file)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("cannot run curl.exe")?;
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        let got = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
        step(p, "download", Some((got as f64 / exe.size.max(1) as f64 * 100.0).min(99.0)));
        std::thread::sleep(Duration::from_millis(150));
    };
    if !status.success() {
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = e.read_to_string(&mut err);
        }
        bail!("download failed: {}", err.trim().trim_start_matches("curl: "));
    }
    step(p, "download", Some(100.0));

    // 2. Verify.
    step(p, "verify", None);
    let got = std::fs::metadata(&file)?.len();
    if got != exe.size {
        let _ = std::fs::remove_file(&file);
        bail!("the download is incomplete ({got} of {} bytes)", exe.size);
    }
    let expected = fetch_text(official(&sum.browser_download_url)?)?;
    let expected = expected.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    let actual = sha256_file(&file)?;
    if expected.len() != 64 || expected != actual {
        let _ = std::fs::remove_file(&file);
        bail!("the download does not match its published SHA-256 checksum; nothing was installed");
    }
    Ok((file, version))
}

/// Run `<installer> install --yes` hidden and report the result.
pub fn run_installer(installer: &Path, p: Progress) -> Result<()> {
    step(p, "install", None);
    let mut cmd = Command::new(installer);
    cmd.args(["install", "--yes"]).stdin(Stdio::null());
    rideshell_core::platform::no_window(&mut cmd);
    let out = cmd.output().with_context(|| format!("cannot run {}", installer.display()))?;
    if !out.status.success() {
        let text = String::from_utf8_lossy(&out.stderr).into_owned() + &String::from_utf8_lossy(&out.stdout);
        let last: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).rev().take(3).collect();
        bail!("the installer failed: {}", last.into_iter().rev().collect::<Vec<_>>().join(" · "));
    }
    step(p, "done", None);
    Ok(())
}

/// The installed RideShell App (what a restart opens).
pub fn installed_app() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    Some(base.join("Programs").join("RideShell").join("rideshell-app.exe")).filter(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(parse_version("v0.1.10"), Some((0, 1, 10)));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("v2.0.0-beta.1"), Some((2, 0, 0)));
        assert!(parse_version("v0.1.10") > parse_version("0.1.9"));
        assert_eq!(parse_version("latest"), None);
    }

    #[test]
    #[ignore = "downloads the latest release from GitHub (nothing is installed)"]
    fn live_check_download_and_verify() {
        println!("CHECK: {:?}", check().map(|v| v.map(|i| i["version"].clone())));
        let steps = std::cell::RefCell::new(Vec::new());
        let (file, version) = download_latest(&|v| steps.borrow_mut().push(v["step"].to_string())).unwrap();
        let mut seen = steps.into_inner();
        seen.dedup();
        println!(
            "VERIFIED {version}: {} ({} bytes), steps {seen:?}",
            file.display(),
            std::fs::metadata(&file).unwrap().len()
        );
    }

    #[test]
    fn only_official_downloads() {
        assert!(official(&format!("https://github.com/{REPO}/releases/download/v1.0.0/rideshell.exe")).is_ok());
        assert!(official("https://example.com/rideshell.exe").is_err());
        assert!(official(&format!("https://github.com/{REPO}.evil/releases/download/x")).is_err());
    }
}
