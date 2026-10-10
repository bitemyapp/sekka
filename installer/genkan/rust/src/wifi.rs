// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only snapshots and parsed, normalized connection profiles. Never log secrets.
use crate::Username;
use anyhow::{Result, ensure};
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

    /// Only I/O remains here: the profile bytes already carry parsing guarantees.
    pub fn write_to(&self, root: &Path) -> Result<()> {
        let Self::Copy(profiles) = self else {
            return Ok(());
        };
        if profiles.0.is_empty() {
            return Ok(());
        }
        let dir = root.join("etc/NetworkManager/system-connections");
        fs::create_dir_all(&dir)?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        for (index, profile) in profiles.0.iter().enumerate() {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(dir.join(format!("installer-wifi-{index}.nmconnection")))?;
            file.write_all(profile.keyfile.as_bytes())?;
            file.sync_all()?;
        }
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

#[cfg(feature = "network")]
fn parse_profile(profile: &str, username: &Username) -> Result<WifiProfile> {
    nm::parse_profile(profile, username)
}

#[cfg(not(feature = "network"))]
fn parse_profile(_: &str, _: &Username) -> Result<WifiProfile> {
    anyhow::bail!("Wi-Fi support was not compiled into this binary")
}
