//! Sekka's nightly builds: the list at downloads.sekka.computer, and a
//! download that resumes, and is checked against the list's SHA-256 as it
//! arrives. Large downloads use adaptive, concurrent byte ranges.

mod parallel;

use crate::image::{CHUNK, Cancelled, hex, sha256_file, valid_sha256};
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const NIGHTLY: &str = "https://downloads.sekka.computer/nightly";

/// One build, as nix/publish-nightly.nix lists it; paths are relative to
/// the nightly directory.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Build {
    pub id: String,
    pub revision: String,
    pub uploaded: String,
    pub iso: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Deserialize)]
struct Builds {
    builds: Vec<Build>,
}

impl Build {
    pub fn file_name(&self) -> &str {
        self.iso.rsplit('/').next().unwrap_or(&self.iso)
    }

    pub fn url(&self, base: &str) -> String {
        format!("{base}/{}", self.iso)
    }
}

fn agent() -> ureq::Agent {
    use ureq::tls::{TlsConfig, TlsProvider};
    let provider = if cfg!(target_os = "linux") { TlsProvider::Rustls } else { TlsProvider::NativeTls };
    ureq::Agent::config_builder()
        .max_idle_connections_per_host(32)
        .max_idle_connections(32)
        .user_agent(concat!("kakehashi/", env!("CARGO_PKG_VERSION")))
        .tls_config(TlsConfig::builder().provider(provider).build())
        .build()
        .into()
}

pub(crate) fn parse(json: &str) -> Result<Vec<Build>> {
    let builds: Builds = serde_json::from_str(json).context("the list of builds is not what Kakehashi expects")?;
    for build in &builds.builds {
        ensure!(valid_sha256(&build.sha256), "build {} has no valid SHA-256", build.id);
        // Paths stay inside the nightly directory.
        ensure!(
            !build.iso.starts_with('/') && !build.iso.split('/').any(|p| p == ".." || p.is_empty()),
            "build {} has an odd path",
            build.id
        );
    }
    Ok(builds.builds)
}

/// The builds kept, newest first; none if nothing is published yet.
pub fn builds(base: &str) -> Result<Vec<Build>> {
    match agent().get(&format!("{base}/builds.json")).call() {
        Ok(mut response) => parse(&response.body_mut().read_to_string()?),
        Err(ureq::Error::StatusCode(404)) => Ok(Vec::new()),
        Err(e) => Err(e).context("could not reach downloads.sekka.computer"),
    }
}

/// Where a download is: whether an existing file is being checked, and
/// how far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetch {
    Checking { done: u64, total: u64 },
    Downloading { done: u64, total: u64 },
}

/// Download `build` into `dir`, resuming a partial download, and return
/// the verified file. A complete file already there is checked, not
/// downloaded again. A `.sha256` file is left beside it.
pub fn download(
    build: &Build,
    base: &str,
    dir: &Path,
    report: &mut dyn FnMut(Fetch),
    cancelled: &dyn Fn() -> bool,
) -> Result<PathBuf> {
    ensure!(!build.file_name().is_empty() && !build.file_name().contains(['/', '\\']), "odd file name");
    fs::create_dir_all(dir)?;
    let dest = dir.join(build.file_name());
    let sidecar = PathBuf::from(format!("{}.sha256", dest.display()));
    let total = build.bytes;
    let write_sidecar = || fs::write(&sidecar, format!("{}  {}\n", build.sha256, build.file_name()));

    if fs::metadata(&dest).is_ok_and(|m| m.len() == total) {
        let sha = sha256_file(&dest, &mut |done| report(Fetch::Checking { done, total }), cancelled)?;
        if sha == build.sha256 {
            write_sidecar()?;
            return Ok(dest);
        }
        fs::remove_file(&dest)?;
    }

    let part = PathBuf::from(format!("{}.part", dest.display()));
    let mut have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if have > total {
        fs::remove_file(&part)?;
        have = 0;
    }
    let mut hasher = Sha256::new();
    if have > 0 {
        // What is already there counts towards the checksum.
        let mut file = File::open(&part)?;
        let mut buffer = vec![0u8; CHUNK];
        let mut done = 0u64;
        while done < have {
            if cancelled() {
                bail!(Cancelled);
            }
            let n = file.read(&mut buffer[..CHUNK.min((have - done) as usize)])?;
            ensure!(n > 0, "the partial download shrank");
            hasher.update(&buffer[..n]);
            done += n as u64;
            report(Fetch::Checking { done, total: have });
        }
    }

    let url = build.url(base);
    let mut out = OpenOptions::new().create(true).write(true).truncate(false).open(&part)?;
    out.seek(SeekFrom::End(0))?;
    if have < total {
        if cancelled() {
            bail!(Cancelled);
        }
        if let Some(response) = parallel::probe(&url, have, total, cancelled)? {
            parallel::fallback(build, &url, response, &mut out, &mut hasher, report, cancelled)?;
        } else {
            parallel::transfer(build, &url, have, &mut out, &mut hasher, report, cancelled)?;
        }
    }
    out.sync_all()?;
    drop(out);
    let sha = hex(&hasher.finalize());
    if sha != build.sha256 {
        let _ = fs::remove_file(&part);
        bail!("the download's SHA-256 is {sha}, not {}; it was deleted", build.sha256);
    }
    fs::rename(&part, &dest)?;
    write_sidecar()?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_published_list() {
        let json = format!(
            r#"{{"builds": [{{"id": "20261013-b9afb36", "revision": "b9afb36ec58d", "committed": "x",
              "uploaded": "2026-10-13T02:51:44Z", "system": "x86_64-linux",
              "iso": "builds/20261013-b9afb36/sekka-nightly-20261013-b9afb36-x86_64-linux.iso",
              "sha256sums": "builds/20261013-b9afb36/SHA256SUMS", "manifest": "m", "flake_lock": "f",
              "bytes": 12157655040, "sha256": "{}"}}]}}"#,
            "3".repeat(64)
        );
        let builds = parse(&json).unwrap();
        assert_eq!(builds[0].file_name(), "sekka-nightly-20261013-b9afb36-x86_64-linux.iso");
        assert_eq!(
            builds[0].url(NIGHTLY),
            "https://downloads.sekka.computer/nightly/builds/20261013-b9afb36/sekka-nightly-20261013-b9afb36-x86_64-linux.iso"
        );
    }

    #[test]
    fn refuses_odd_paths_and_checksums() {
        let entry = |iso: &str, sha: &str| {
            format!(
                r#"{{"builds": [{{"id": "x", "revision": "r", "uploaded": "u", "iso": "{iso}", "bytes": 1, "sha256": "{sha}"}}]}}"#
            )
        };
        let good = "a".repeat(64);
        assert!(parse(&entry("builds/x/a.iso", &good)).is_ok());
        assert!(parse(&entry("../a.iso", &good)).is_err());
        assert!(parse(&entry("/etc/a.iso", &good)).is_err());
        assert!(parse(&entry("builds/x/a.iso", "short")).is_err());
    }

    #[test]
    fn a_complete_download_is_checked_not_fetched() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.iso"), b"abc").unwrap();
        let build = Build {
            id: "x".into(),
            revision: "r".into(),
            uploaded: "u".into(),
            iso: "builds/x/a.iso".into(),
            bytes: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
        };
        // An unreachable base proves nothing is fetched.
        let path = download(&build, "http://127.0.0.1:9", dir.path(), &mut |_| {}, &|| false).unwrap();
        assert_eq!(path, dir.path().join("a.iso"));
        assert!(fs::read_to_string(dir.path().join("a.iso.sha256")).unwrap().starts_with(&build.sha256));
    }
}
