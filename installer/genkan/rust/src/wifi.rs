// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only snapshots and parsed, normalized connection profiles. Never log secrets.
use crate::Username;
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};
use zeroize::Zeroizing;

#[cfg(feature = "network")]
mod nm;
#[cfg(feature = "network")]
pub(crate) use nm::snapshot;

// libnm's validated/normalized bytes are retained, not thrown away and parsed
// again at write time. No Debug, Clone, Deserialize, or public raw-data accessor.
struct WifiProfile {
    uuid: String,
    keyfile: Zeroizing<String>,
}
pub struct WifiProfiles(Vec<WifiProfile>);

/// Opt-out cannot coexist with profiles. Copy may contain no saved connections.
/// Only parsing can construct a bounded, duplicate-free WifiProfiles collection.
/// ```compile_fail
/// use calamares_nixos::wifi::WifiProfiles;
/// let profiles = WifiProfiles(Vec::new());
/// ```
pub enum WifiTransfer {
    Skip,
    Copy(WifiProfiles),
}
impl WifiTransfer {
    pub fn parse(enabled: bool, profiles: Vec<String>, username: &Username) -> Result<Self> {
        let profiles = Zeroizing::new(profiles);
        ensure!(
            enabled || profiles.is_empty(),
            "Wi-Fi transfer is disabled but profiles were supplied"
        );
        if !enabled {
            return Ok(Self::Skip);
        }
        ensure!(
            profiles.len() <= 32 && profiles.iter().all(|p| p.len() <= 16384),
            "Too many or oversized Wi-Fi profiles"
        );
        let mut uuids = BTreeSet::new();
        let mut parsed = Vec::with_capacity(profiles.len());
        for profile in profiles.iter() {
            let profile = parse_profile(profile, username)?;
            ensure!(
                uuids.insert(profile.uuid.clone()),
                "Duplicate Wi-Fi profile identity"
            );
            parsed.push(profile);
        }
        Ok(Self::Copy(WifiProfiles(parsed)))
    }

    pub fn enabled(&self) -> bool {
        matches!(self, Self::Copy(_))
    }
    pub fn profile_count(&self) -> usize {
        match self {
            Self::Skip => 0,
            Self::Copy(profiles) => profiles.0.len(),
        }
    }

    /// Each profile's keyfile name on the installed system, and its bytes.
    fn files(&self) -> Vec<(String, &[u8])> {
        let Self::Copy(profiles) = self else {
            return vec![];
        };
        profiles
            .0
            .iter()
            .enumerate()
            .map(|(index, profile)| {
                (
                    format!("installer-wifi-{index}.nmconnection"),
                    profile.keyfile.as_bytes(),
                )
            })
            .collect()
    }

    /// Only I/O remains here: the profile bytes already carry parsing guarantees.
    pub fn write_to(&self, root: &Path) -> Result<()> {
        let files = self.files();
        if files.is_empty() {
            return Ok(());
        }
        let dir = root.join("etc/NetworkManager/system-connections");
        fs::create_dir_all(&dir)?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        for (name, bytes) in files {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(dir.join(name))?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        Ok(())
    }

    /// Make the store of saved networks for the system installed at `root`,
    /// sealed to this computer's TPM, with the profiles written only inside
    /// it (`sekka-network-store install`; DEVELOPERS.md, "Saved network
    /// connections"). They reach it through an anonymous memory file, never
    /// the target disk. Fails, making nothing, where the installed system
    /// keeps no store: no TPM, an encrypted root, removable media.
    pub fn seal_into(&self, root: &Path) -> Result<()> {
        let files = self.files();
        let headers: Vec<String> = files
            .iter()
            .map(|(name, bytes)| format!("{name}\n{}\n", bytes.len()))
            .collect();
        // Sized once: growing it would leave copies of the secrets behind.
        let mut input = Zeroizing::new(Vec::with_capacity(
            files.iter().map(|(_, bytes)| bytes.len()).sum::<usize>()
                + headers.iter().map(String::len).sum::<usize>(),
        ));
        for ((_, bytes), header) in files.iter().zip(&headers) {
            input.extend_from_slice(header.as_bytes());
            input.extend_from_slice(bytes);
        }
        let root = root.to_str().context("Target path encoding")?;
        // Longer than the store's own limits on its tools (a TPM that hangs,
        // say), so it always cleans up after itself: a device left open on
        // the target would keep it from being unmounted.
        crate::process::output_secret(
            "sekka-network-store",
            &["install", "--root", root],
            &input,
            900,
        )?;
        Ok(())
    }

    pub(crate) fn into_raw(self) -> (bool, Vec<String>) {
        match self {
            Self::Skip => (false, vec![]),
            Self::Copy(profiles) => (
                true,
                profiles
                    .0
                    .into_iter()
                    .map(|mut p| std::mem::take(&mut *p.keyfile))
                    .collect(),
            ),
        }
    }
}

#[cfg(test)]
impl WifiTransfer {
    /// Profiles as libnm normalizes them, for tests built without it.
    pub(crate) fn normalized(keyfiles: &[&str]) -> Self {
        Self::Copy(WifiProfiles(
            keyfiles
                .iter()
                .enumerate()
                .map(|(index, keyfile)| WifiProfile {
                    uuid: index.to_string(),
                    keyfile: Zeroizing::new((*keyfile).to_owned()),
                })
                .collect(),
        ))
    }
}

#[cfg(feature = "network")]
fn parse_profile(profile: &str, username: &Username) -> Result<WifiProfile> {
    nm::parse_profile(profile, username)
}

#[cfg(not(feature = "network"))]
fn parse_profile(_: &str, _: &Username) -> Result<WifiProfile> {
    anyhow::bail!("Wi-Fi support was not compiled into this binary")
}
