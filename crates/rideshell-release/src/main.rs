//! `rideshell-release`: the maintainer's release-signing tool. Not shipped.
//!
//!   rideshell-release keygen            create the release key (once; keep it safe)
//!   rideshell-release sign <dir>        write <dir>/SHA256SUMS and SHA256SUMS.sig
//!   rideshell-release verify <dir>      check them with the key built into RideShell
//!
//! The secret key lives outside the repository, by default in
//! `~/.rideshell-release/signing.key` (or the file named by
//! RIDESHELL_SIGNING_KEY). Back it up: without it, existing installs can't
//! accept updates; with it, anyone can sign one.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rideshell_core::release;

fn key_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("RIDESHELL_SIGNING_KEY").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).context("no home folder")?;
    Ok(PathBuf::from(home).join(".rideshell-release").join("signing.key"))
}

fn read_secret() -> Result<String> {
    let p = key_path()?;
    let s = std::fs::read_to_string(&p).with_context(|| format!("cannot read the release key {}", p.display()))?;
    Ok(s.trim().to_string())
}

fn keygen() -> Result<()> {
    let p = key_path()?;
    if p.exists() {
        bail!("{} already exists; refusing to replace the release key", p.display());
    }
    let (secret, public) = release::generate_key()?;
    std::fs::create_dir_all(p.parent().context("bad key path")?)?;
    std::fs::write(&p, format!("{secret}\n"))?;
    println!("secret key  {}  (back it up; never commit it)", p.display());
    println!("public key  {public}");
    println!("put the public key in crates/rideshell-core/src/release.rs (RELEASE_PUBLIC_KEY)");
    Ok(())
}

/// Release files in `dir`: everything except the manifest and signature.
fn release_files(dir: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        if e.file_type()?.is_file()
            && name != release::MANIFEST
            && name != release::SIGNATURE
            && !name.ends_with(".sha256")
        {
            out.push((name, std::fs::read(e.path())?));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    if out.is_empty() {
        bail!("no release files in {}", dir.display());
    }
    Ok(out)
}

fn sign(dir: &Path) -> Result<()> {
    let secret = read_secret()?;
    let public = release::public_key_of(&secret)?;
    if public != release::RELEASE_PUBLIC_KEY {
        bail!("this key ({public}) is not the one built into RideShell ({})", release::RELEASE_PUBLIC_KEY);
    }
    let mut manifest = String::new();
    for (name, data) in release_files(dir)? {
        let hash = release::sha256_hex(&data);
        manifest.push_str(&format!("{hash}  {name}\n"));
        // Per-file checksum too, for people verifying by hand.
        std::fs::write(dir.join(format!("{name}.sha256")), format!("{hash}  {name}\n"))?;
    }
    let sig = release::sign(manifest.as_bytes(), &secret)?;
    std::fs::write(dir.join(release::MANIFEST), &manifest)?;
    std::fs::write(dir.join(release::SIGNATURE), format!("{sig}\n"))?;
    print!("{manifest}");
    println!("signed with {public}");
    verify(dir)
}

fn verify(dir: &Path) -> Result<()> {
    let manifest = std::fs::read(dir.join(release::MANIFEST))?;
    let sig = std::fs::read_to_string(dir.join(release::SIGNATURE))?;
    release::verify(&manifest, &sig, release::RELEASE_PUBLIC_KEY)?;
    let text = String::from_utf8(manifest.clone())?;
    for (name, data) in release_files(dir)? {
        release::check_download(&manifest, &sig, &name, &release::sha256_hex(&data))
            .with_context(|| format!("{name} in {}", dir.display()))?;
    }
    println!("OK: signature valid, {} file(s) match", text.lines().count());
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["keygen"] => keygen(),
        ["sign", dir] => sign(Path::new(dir)),
        ["verify", dir] => verify(Path::new(dir)),
        ["public-key"] => {
            println!("{}", release::public_key_of(&read_secret()?)?);
            Ok(())
        }
        _ => bail!("usage: rideshell-release keygen | sign <dir> | verify <dir> | public-key"),
    }
}
