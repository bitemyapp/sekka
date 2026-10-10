// SPDX-License-Identifier: GPL-3.0-or-later
pub mod applications;
pub mod config;
pub mod desktop;
pub mod disk;
pub mod filesystem;
pub mod github;
pub mod graphics;
pub mod install;
pub mod memory;
pub mod nixlog;
pub mod plan;
pub mod power;
pub mod precache;
pub mod process;
pub mod session;
pub use filesystem::Filesystem;
pub use plan::{ConfirmedInstall, Hostname, InstallPlan, RawRequest, Username};
pub mod timezone;
pub mod wifi;
#[cfg(feature = "gui")]
pub mod zonemap;
pub use desktop::Desktop;
#[cfg(test)]
mod plan_tests;

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, os::unix::fs::MetadataExt, path::Path};

pub const SETTINGS: &str = "/etc/calamares-nixos/settings.json";
fn enabled() -> bool {
    true
}
/// Hardware-friendly GUI default; the review page and request retain an opt-out.
pub const DEFAULT_ALLOW_UNFREE: bool = true;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub template_dir: String,
    pub zoneinfo: String,
    pub state_version: String,
    pub kernel: Kernel,
    // Set only by a root-owned VM test configuration, never by the UI request.
    #[serde(default)]
    pub test_diagnostics: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kernel {
    Lts,
    Latest,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Firmware {
    Uefi,
    Bios,
}
impl Firmware {
    pub fn current() -> Self {
        if Path::new("/sys/firmware/efi").exists() {
            Self::Uefi
        } else {
            Self::Bios
        }
    }
}

pub const LOCALES: &[&str] = &[
    "en_US.UTF-8",
    "en_GB.UTF-8",
    "de_DE.UTF-8",
    "fr_FR.UTF-8",
    "es_ES.UTF-8",
    "it_IT.UTF-8",
    "ja_JP.UTF-8",
    "pt_BR.UTF-8",
];
pub const KEYBOARDS: &[&str] = &["us", "gb", "de", "fr", "es", "it", "jp", "br"];

fn trusted_component(path: &Path, uid: u32, mode: u32, directory: bool) -> bool {
    // Nix's store root is root:nixbld 1775 on the live image. The sticky
    // directory prevents builders from replacing root-owned store entries.
    // Every referenced entry below it is still checked for root ownership
    // and no group/other writes. Do not extend this exception to arbitrary
    // shared directories or to a store without the sticky bit.
    let store_root = path == Path::new("/nix/store") && directory && mode & 0o1777 == 0o1775;
    uid == 0 && (mode & 0o022 == 0 || store_root)
}

pub fn read_trusted(path: &Path) -> Result<Vec<u8>> {
    // All parents, including symlink targets, must be owned by root and not
    // writable by another user. NixOS /etc links into /nix/store are expected.
    let resolved = path.canonicalize()?;
    for p in path.ancestors().chain(resolved.ancestors()) {
        let m = fs::metadata(p)?;
        ensure!(
            fs::symlink_metadata(p)?.uid() == 0
                && trusted_component(p, m.uid(), m.mode(), m.is_dir()),
            "Untrusted installer configuration: {}",
            p.display()
        );
    }
    ensure!(
        fs::metadata(&resolved)?.is_file(),
        "Not a regular settings file"
    );
    ensure!(
        fs::metadata(&resolved)?.len() < 1024 * 1024,
        "Settings too large"
    );
    Ok(fs::read(resolved)?)
}

impl Settings {
    pub fn load() -> Result<Self> {
        let settings: Self = serde_json::from_slice(&read_trusted(Path::new(SETTINGS))?)?;
        ensure!(
            matches!(settings.state_version.as_str(), "26.05" | "26.11"),
            "Unsupported state version"
        );
        ensure!(
            Path::new(&settings.template_dir).is_absolute()
                && Path::new(&settings.zoneinfo).is_absolute(),
            "Settings paths must be absolute"
        );
        Ok(settings)
    }
}

#[cfg(test)]
mod trust_tests {
    use super::*;
    #[test]
    fn sticky_nix_store_is_the_only_shared_directory_exception() {
        assert!(trusted_component(Path::new("/nix/store"), 0, 0o41775, true));
        assert!(trusted_component(
            Path::new("/nix/store/root-owned-entry"),
            0,
            0o40755,
            true
        ));
        assert!(!trusted_component(
            Path::new("/nix/store"),
            0,
            0o40775,
            true
        ));
        assert!(!trusted_component(
            Path::new("/nix/store"),
            1000,
            0o41775,
            true
        ));
        assert!(!trusted_component(
            Path::new("/nix/store"),
            0,
            0o101775,
            false
        ));
        assert!(!trusted_component(Path::new("/tmp"), 0, 0o41777, true));
        assert!(!trusted_component(
            Path::new("/nix/store/writable-entry"),
            0,
            0o40775,
            true
        ));
    }
}
