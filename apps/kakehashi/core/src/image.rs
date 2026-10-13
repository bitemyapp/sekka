//! Images and their checksums.

use anyhow::{Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

pub(crate) const CHUNK: usize = 8 * 1024 * 1024;

/// Stopped because the person asked.
#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cancelled")
    }
}

impl std::error::Error for Cancelled {}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn valid_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The SHA-256 of a file, reporting bytes read so far.
pub fn sha256_file(path: &Path, report: &mut dyn FnMut(u64), cancelled: &dyn Fn() -> bool) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK];
    let mut done = 0u64;
    loop {
        if cancelled() {
            bail!(Cancelled);
        }
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        done += n as u64;
        report(done);
    }
    Ok(hex(&hasher.finalize()))
}

/// The checksum an image must have and where it came from: a
/// `<image>.sha256` file beside it, or its line in a `SHA256SUMS` there.
pub fn expected_sha256(image: &Path) -> Result<Option<(String, PathBuf)>> {
    let file_name = image.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let sidecar = PathBuf::from(format!("{}.sha256", image.display()));
    if let Ok(text) = fs::read_to_string(&sidecar) {
        let hex = text.split_whitespace().next().unwrap_or("");
        ensure!(valid_sha256(hex), "{} doesn't start with a SHA-256", sidecar.display());
        return Ok(Some((hex.to_ascii_lowercase(), sidecar)));
    }
    let sums = image.with_file_name("SHA256SUMS");
    if let Ok(text) = fs::read_to_string(&sums) {
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            if let (Some(hex), Some(name)) = (parts.next(), parts.next())
                && name.trim_start_matches('*') == file_name
                && valid_sha256(hex)
            {
                return Ok(Some((hex.to_ascii_lowercase(), sums)));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_with_progress() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.iso");
        fs::write(&path, b"abc").unwrap();
        let mut seen = 0;
        let sha = sha256_file(&path, &mut |d| seen = d, &|| false).unwrap();
        assert_eq!(sha, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(seen, 3);
        let err = sha256_file(&path, &mut |_| {}, &|| true).unwrap_err();
        assert!(err.is::<Cancelled>());
    }

    #[test]
    fn checksums_from_files_beside_the_image() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("x.iso");
        let hex = "a".repeat(64);
        assert!(expected_sha256(&image).unwrap().is_none());
        fs::write(dir.path().join("SHA256SUMS"), format!("{} other.iso\n{hex}  x.iso\n", "0".repeat(64))).unwrap();
        assert_eq!(expected_sha256(&image).unwrap().unwrap().0, hex);
        let sidecar = "b".repeat(64);
        fs::write(dir.path().join("x.iso.sha256"), format!("{sidecar}  x.iso\n")).unwrap();
        assert_eq!(expected_sha256(&image).unwrap().unwrap().0, sidecar);
        fs::write(dir.path().join("x.iso.sha256"), "nonsense\n").unwrap();
        assert!(expected_sha256(&image).is_err());
    }
}
