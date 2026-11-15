//! This fork (`almatkai/hex`) publishes Hex on its own GitHub Releases, not
//! Anomaly's release service, and has no Developer ID or Authenticode
//! identity. Each channel's manifest (the release protocol's schema 1) is
//! the asset `<channel>.json` of the prerelease `channel-<channel>`, which
//! `scripts/release-local.ps1` moves forward; the files it names live in the
//! immutable `v<version>` release. Every target file is signed with the
//! fork's ed25519 update key over its SHA-256 (`<target>-sig`), and Hex
//! refuses a file without that signature, on every OS, before it runs.
use std::fmt::Write as _;
use std::path::Path;

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

/// Where each channel's pointer release and the versioned releases live.
pub const FEED: &str = "https://github.com/almatkai/hex/releases/download";
/// Where a person downloads Hex by hand.
pub const RELEASES: &str = "https://github.com/almatkai/hex/releases/latest";

/// The fork's update key; its private half is `secrets/update-signing.key`,
/// which never leaves the release machine (`docs/releasing-fork.md`).
/// Changing it strands every installed copy on the old key.
/// Public key `c0e9603a71c6126828f39d48af5b15827ecf6a8a578351f8bd4f96d421430aab`.
pub const KEY: [u8; 32] = [
    0xc0, 0xe9, 0x60, 0x3a, 0x71, 0xc6, 0x12, 0x68, 0x28, 0xf3, 0x9d, 0x48, 0xaf, 0x5b, 0x15, 0x82,
    0x7e, 0xcf, 0x6a, 0x8a, 0x57, 0x83, 0x51, 0xf8, 0xbd, 0x4f, 0x96, 0xd4, 0x21, 0x43, 0x0a, 0xab,
];

/// The manifest target holding the signature of `target`.
pub fn signature_target(target: &str) -> String {
    format!("{target}-sig")
}

fn message(sha256: &str) -> String {
    format!("almatkai.hex.update.v1\n{sha256}\n")
}

/// Whether `signature` is the fork's key signing the file whose SHA-256 is
/// `sha256`. The manifest names that SHA-256 and the download is checked
/// against it, so this ties the bytes to the key.
pub fn verify(sha256: &str, signature: &[u8]) -> Result<()> {
    verify_with(&KEY, sha256, signature)
}

fn verify_with(key: &[u8; 32], sha256: &str, signature: &[u8]) -> Result<()> {
    let signature =
        Signature::from_slice(signature).wrap_err("the update's signature is malformed")?;
    VerifyingKey::from_bytes(key)
        .wrap_err("invalid update key")?
        .verify_strict(message(sha256).as_bytes(), &signature)
        .wrap_err("the update is not signed with this Hex's update key")
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn read_key(path: &Path) -> Result<SigningKey> {
    let text = std::fs::read_to_string(path)
        .wrap_err_with(|| format!("could not read the update key {}", path.display()))?;
    let text = text.trim();
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("the update key must be 64 hex characters");
    }
    let mut seed = [0_u8; 32];
    for (index, byte) in seed.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)?;
    }
    Ok(SigningKey::from_bytes(&seed))
}

/// `hex release-keygen`: writes a new private key to `path` (never over an
/// existing one) and returns its public half as hex, for [`KEY`].
pub fn keygen(path: &Path) -> Result<String> {
    if path.exists() {
        bail!(
            "{} already exists; Hex never replaces an update key",
            path.display()
        );
    }
    let mut seed = [0_u8; 32];
    getrandom::fill(&mut seed).map_err(|error| eyre!("could not draw a key: {error}"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::durable::write_private(path, format!("{}\n", hex(&seed)).as_bytes())
        .wrap_err_with(|| format!("could not write {}", path.display()))?;
    Ok(hex(SigningKey::from_bytes(&seed)
        .verifying_key()
        .as_bytes()))
}

/// `hex release-sign`: writes `<file>.sig`, the raw 64-byte signature of
/// each file's SHA-256, and returns the SHA-256s. Refuses a key whose public
/// half isn't [`KEY`], so a release can never be signed with a key installed
/// copies don't trust.
pub fn sign_files(key: &Path, files: &[std::path::PathBuf]) -> Result<Vec<String>> {
    let key = read_key(key)?;
    if key.verifying_key().as_bytes() != &KEY {
        bail!("this key is not the update key Hex trusts (src/updater/fork.rs KEY)");
    }
    files
        .iter()
        .map(|file| {
            let sha256 = crate::transcription::models::sha256_hex(file, None)
                .wrap_err_with(|| format!("could not hash {}", file.display()))?;
            let signature = key.sign(message(&sha256).as_bytes()).to_bytes();
            let mut path = file.as_os_str().to_owned();
            path.push(".sig");
            std::fs::write(&path, signature)?;
            Ok(sha256)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_bind_the_key_to_the_sha256() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let public = key.verifying_key().to_bytes();
        let sha = "a".repeat(64);
        let signature = key.sign(message(&sha).as_bytes()).to_bytes();
        verify_with(&public, &sha, &signature).unwrap();
        assert!(verify_with(&public, &"b".repeat(64), &signature).is_err());
        assert!(verify_with(&public, &sha, &signature[..63]).is_err());
        let other = SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes();
        assert!(verify_with(&other, &sha, &signature).is_err());
    }

    #[test]
    fn the_compiled_key_is_a_real_key() {
        assert_ne!(KEY, [0; 32], "set KEY to the fork's public update key");
        VerifyingKey::from_bytes(&KEY).unwrap();
    }

    #[test]
    fn keys_round_trip_through_their_file() {
        let path = std::env::temp_dir().join(format!("hex-update-key-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let public = keygen(&path).unwrap();
        assert_eq!(
            hex(read_key(&path).unwrap().verifying_key().as_bytes()),
            public
        );
        std::fs::write(&path, "not a key").unwrap();
        assert!(read_key(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
