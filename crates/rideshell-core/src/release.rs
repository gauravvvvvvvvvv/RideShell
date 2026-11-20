//! Signed releases.
//!
//! Every release publishes `SHA256SUMS` (one `<sha256>  <file>` line per
//! download) and `SHA256SUMS.sig`: an Ed25519 signature of that file, made
//! with the project's release key, which is kept offline by the maintainer.
//! RideShell App installs an update only when the signature verifies with the
//! public key below **and** the download's SHA-256 is the one listed. A
//! tampered release (or a compromised GitHub account without the key) cannot
//! produce a valid signature.
//!
//! The signature file is the 64-byte signature as hex, optionally followed by
//! a newline. Keys are 32 bytes, as hex.

use anyhow::{bail, Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

/// The project's release public key (Ed25519, hex). Updates must be signed
/// with the matching private key.
pub const RELEASE_PUBLIC_KEY: &str = "58da827a9ba22231fbc0283fab554ad184f0251822b1efef8bcbecb749179939";

pub const MANIFEST: &str = "SHA256SUMS";
pub const SIGNATURE: &str = "SHA256SUMS.sig";

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex(s: &str) -> Result<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("not a hex string");
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).context("bad hex")).collect()
}

fn fixed<const N: usize>(hex: &str, what: &str) -> Result<[u8; N]> {
    from_hex(hex)?.try_into().map_err(|_| anyhow::anyhow!("{what} must be {N} bytes"))
}

pub fn sha256_hex(data: &[u8]) -> String {
    to_hex(&Sha256::digest(data))
}

/// Check `signature_hex` over `manifest` with `public_key_hex`.
pub fn verify(manifest: &[u8], signature_hex: &str, public_key_hex: &str) -> Result<()> {
    let key = VerifyingKey::from_bytes(&fixed::<32>(public_key_hex, "public key")?).context("invalid public key")?;
    let sig = Signature::from_bytes(&fixed::<64>(signature_hex, "signature")?);
    key.verify_strict(manifest, &sig).map_err(|_| anyhow::anyhow!("the release signature is not valid"))
}

/// The SHA-256 the manifest lists for `file`.
pub fn listed_hash(manifest: &str, file: &str) -> Option<String> {
    manifest.lines().find_map(|l| {
        let mut parts = l.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        (name == file && hash.len() == 64).then(|| hash.to_ascii_lowercase())
    })
}

/// Verify the signed manifest, then that `data_sha256` is what it lists for `file`.
pub fn check_download(manifest: &[u8], signature_hex: &str, file: &str, data_sha256: &str) -> Result<()> {
    if RELEASE_PUBLIC_KEY.starts_with('@') {
        bail!("this build has no release key, so it cannot verify updates");
    }
    verify(manifest, signature_hex, RELEASE_PUBLIC_KEY)?;
    let text = std::str::from_utf8(manifest).context("the checksum list is not text")?;
    let want = listed_hash(text, file).with_context(|| format!("the signed checksum list has no entry for {file}"))?;
    if !want.eq_ignore_ascii_case(data_sha256) {
        bail!("{file} does not match the signed checksum list; nothing was installed");
    }
    Ok(())
}

// ----- maintainers: making signatures (used by the rideshell-release tool) -----

/// A new signing key: (secret seed hex, public key hex).
pub fn generate_key() -> Result<(String, String)> {
    let mut seed = [0u8; 32];
    getrandom(&mut seed)?;
    let key = SigningKey::from_bytes(&seed);
    let out = (to_hex(&seed), to_hex(key.verifying_key().as_bytes()));
    seed.fill(0);
    Ok(out)
}

/// Sign `manifest` with a secret seed (hex); returns the signature as hex.
pub fn sign(manifest: &[u8], secret_hex: &str) -> Result<String> {
    let key = SigningKey::from_bytes(&fixed::<32>(secret_hex, "secret key")?);
    Ok(to_hex(&key.sign(manifest).to_bytes()))
}

/// Public key (hex) of a secret seed (hex).
pub fn public_key_of(secret_hex: &str) -> Result<String> {
    Ok(to_hex(SigningKey::from_bytes(&fixed::<32>(secret_hex, "secret key")?).verifying_key().as_bytes()))
}

fn getrandom(buf: &mut [u8]) -> Result<()> {
    // The operating system's cryptographic random generator.
    #[cfg(windows)]
    {
        use windows_sys::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
        let ok = unsafe {
            BCryptGenRandom(std::ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG)
        };
        if ok != 0 {
            bail!("the system random generator failed");
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")?.read_exact(buf)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_round_trip() {
        let (secret, public) = generate_key().unwrap();
        assert_eq!(public_key_of(&secret).unwrap(), public);
        let manifest = b"abc123  rideshell.exe\n";
        let sig = sign(manifest, &secret).unwrap();
        assert!(verify(manifest, &sig, &public).is_ok());
        assert!(verify(b"abc124  rideshell.exe\n", &sig, &public).is_err(), "a changed list fails");
        let (_, other) = generate_key().unwrap();
        assert!(verify(manifest, &sig, &other).is_err(), "another key fails");
    }

    #[test]
    fn listed_hashes() {
        let h = "a".repeat(64);
        let m = format!("{h}  rideshell.exe\n{}  *rideshell-linux-x86_64.tar.gz\n", "b".repeat(64));
        assert_eq!(listed_hash(&m, "rideshell.exe"), Some(h));
        assert_eq!(listed_hash(&m, "rideshell-linux-x86_64.tar.gz"), Some("b".repeat(64)));
        assert_eq!(listed_hash(&m, "other.exe"), None);
    }

    #[test]
    fn hex() {
        assert_eq!(to_hex(&[0, 255, 16]), "00ff10");
        assert_eq!(from_hex("00ff10").unwrap(), vec![0, 255, 16]);
        assert!(from_hex("0g").is_err() && from_hex("abc").is_err());
    }
}
