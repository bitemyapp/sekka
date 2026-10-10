// SPDX-License-Identifier: GPL-3.0-or-later
//! Parse at process boundaries; keep the result, not just a successful check.
//! Plans are immutable and deliberately neither Deserialize nor Debug/Clone.
//! They describe reviewed intent, not a promise that a disk is still safe.
use crate::{
    Filesystem, Firmware, KEYBOARDS, LOCALES, Settings, desktop::DesktopSelection, disk,
    timezone::TimeZone, wifi::WifiTransfer,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;
use zeroize::{Zeroize, Zeroizing};

/// Untrusted form/wire data. Older requests default to ext4.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRequest {
    pub disk: disk::Identity,
    pub firmware: Firmware,
    #[serde(default)]
    pub filesystem: Filesystem,
    pub hostname: String,
    pub username: String,
    pub full_name: String,
    pub password: String,
    pub locale: String,
    pub timezone: String,
    pub keyboard: String,
    pub desktops: Vec<crate::Desktop>,
    pub default_desktop: crate::Desktop,
    #[serde(default = "crate::applications::default_selection")]
    pub applications: Vec<String>,
    pub copy_wifi: bool,
    pub wifi_profiles: Vec<String>,
    pub allow_unfree: bool,
    /// Swap partition matched to installed RAM, fronted by zswap.
    #[serde(default = "crate::enabled")]
    pub swap: bool,
    /// CachyOS-inspired kernel, memory, I/O and service defaults.
    #[serde(default = "crate::enabled")]
    pub tuning: bool,
    /// NVIDIA GPUs the GUI found, for NVIDIA's driver and PRIME offload.
    #[serde(default)]
    pub graphics: crate::graphics::Graphics,
    /// GitHub step (github.rs): the account whose public keys are
    /// authorized, the keys, the SSH server and a Git identity. All optional.
    #[serde(default)]
    pub github_user: String,
    #[serde(default)]
    pub ssh_keys: Vec<String>,
    #[serde(default)]
    pub ssh_server: bool,
    #[serde(default)]
    pub git_name: String,
    #[serde(default)]
    pub git_email: String,
    pub confirmation: String,
}
impl Drop for RawRequest {
    fn drop(&mut self) {
        self.password.zeroize();
        self.wifi_profiles.zeroize();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hostname(String);
impl Hostname {
    pub fn parse(value: &str) -> Result<Self> {
        let bytes = value.as_bytes();
        ensure!(
            !bytes.is_empty()
                && bytes.len() <= 63
                && bytes[0].is_ascii_alphanumeric()
                && bytes[bytes.len() - 1].is_ascii_alphanumeric()
                && bytes
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || *b == b'-'),
            "Hostname must be one DNS label (letters, numbers and interior hyphens)"
        );
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Username(String);
impl Username {
    pub fn parse(value: &str) -> Result<Self> {
        let bytes = value.as_bytes();
        ensure!(
            !bytes.is_empty()
                && bytes.len() <= 31
                && bytes[0].is_ascii_lowercase()
                && bytes.iter().all(|b| b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || *b == b'-'
                    || *b == b'_'),
            "Username must start with a lowercase letter and contain only lowercase letters, digits, hyphens or underscores"
        );
        ensure!(
            ![
                "root",
                "nixos",
                "daemon",
                "nobody",
                "systemd-network",
                "messagebus",
                "sshd",
                "polkituser",
                "sddm",
                "nixbld"
            ]
            .contains(&value)
                && !value.starts_with("nixbld"),
            "Reserved username"
        );
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Parsed intent. Only constructors in this module can establish its invariants.
/// Configuration generation cannot accept wire data:
/// ```compile_fail
/// use calamares_nixos::{RawRequest, config};
/// fn bypass(raw: RawRequest) { config::configuration(&raw); }
/// ```
/// A serialized GUI plan is not proof of validation in the privileged process:
/// ```compile_fail
/// use calamares_nixos::InstallPlan;
/// let plan: InstallPlan = serde_json::from_str("{}").unwrap();
/// ```
/// Read-only access cannot change the selected disk after confirmation:
/// ```compile_fail
/// use calamares_nixos::InstallPlan;
/// fn retarget(plan: &mut InstallPlan) { plan.disk().path.clear(); }
/// ```
pub struct InstallPlan {
    settings: Settings,
    disk: disk::Identity,
    firmware: Firmware,
    filesystem: Filesystem,
    hostname: Hostname,
    username: Username,
    full_name: String,
    password: Zeroizing<String>,
    locale: &'static str,
    timezone: TimeZone,
    keyboard: &'static str,
    desktops: DesktopSelection,
    applications: crate::applications::ApplicationSelection,
    wifi: WifiTransfer,
    allow_unfree: bool,
    swap: bool,
    tuning: bool,
    graphics: crate::graphics::Graphics,
    onboarding: crate::github::Onboarding,
}

impl RawRequest {
    /// Parse for review; confirmation is intentionally a separate transition.
    /// Called on a worker: timezone and Wi-Fi parsing can perform I/O/FFI.
    pub fn parse(mut self, settings: &Settings) -> Result<InstallPlan> {
        let hostname = Hostname::parse(&self.hostname)?;
        let username = Username::parse(&self.username)?;
        ensure!(
            self.full_name.len() <= 128
                && !self.full_name.contains(':')
                && !self.full_name.chars().any(char::is_control),
            "Invalid full name"
        );
        ensure!(
            self.password.chars().count() >= 12
                && self.password.len() <= 1024
                && !self.password.chars().any(char::is_control),
            "Password must have at least 12 characters, at most 1024 bytes, and no control characters"
        );
        // These closed UI choices need no separate wrapper types: store only
        // the matching static value, never the unchecked input string.
        let locale = LOCALES
            .iter()
            .copied()
            .find(|v| *v == self.locale)
            .ok_or_else(|| anyhow::anyhow!("Unsupported locale"))?;
        let keyboard = KEYBOARDS
            .iter()
            .copied()
            .find(|v| *v == self.keyboard)
            .ok_or_else(|| anyhow::anyhow!("Unsupported keyboard layout"))?;
        let timezone = TimeZone::parse(&self.timezone, Path::new(&settings.zoneinfo))?;
        let desktops =
            DesktopSelection::parse(std::mem::take(&mut self.desktops), self.default_desktop)?;
        let applications = crate::applications::ApplicationSelection::parse(
            std::mem::take(&mut self.applications),
            self.allow_unfree,
        )?;
        let wifi = WifiTransfer::parse(
            self.copy_wifi,
            std::mem::take(&mut self.wifi_profiles),
            &username,
        )?;
        self.graphics.check()?;
        let onboarding = crate::github::Onboarding::parse(
            &self.github_user,
            &self.ssh_keys,
            self.ssh_server,
            &self.git_name,
            &self.git_email,
        )?;
        Ok(InstallPlan {
            settings: settings.clone(),
            disk: self.disk.clone(),
            firmware: self.firmware,
            filesystem: self.filesystem,
            hostname,
            username,
            full_name: std::mem::take(&mut self.full_name),
            password: Zeroizing::new(std::mem::take(&mut self.password)),
            locale,
            timezone,
            keyboard,
            desktops,
            applications,
            wifi,
            allow_unfree: self.allow_unfree,
            swap: self.swap,
            tuning: self.tuning,
            graphics: std::mem::take(&mut self.graphics),
            onboarding,
        })
    }

    /// The helper independently parses every byte received over stdin.
    pub fn parse_confirmed(mut self, settings: &Settings) -> Result<ConfirmedInstall> {
        let phrase = std::mem::take(&mut self.confirmation);
        self.parse(settings)?.confirm(&phrase)
    }
}

impl InstallPlan {
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    pub fn disk(&self) -> &disk::Identity {
        &self.disk
    }
    pub fn firmware(&self) -> Firmware {
        self.firmware
    }
    pub fn filesystem(&self) -> Filesystem {
        self.filesystem
    }
    pub fn hostname(&self) -> &Hostname {
        &self.hostname
    }
    pub fn username(&self) -> &Username {
        &self.username
    }
    pub fn full_name(&self) -> &str {
        &self.full_name
    }
    pub fn locale(&self) -> &str {
        self.locale
    }
    pub fn timezone(&self) -> &TimeZone {
        &self.timezone
    }
    pub fn keyboard(&self) -> &str {
        self.keyboard
    }
    pub fn desktops(&self) -> &DesktopSelection {
        &self.desktops
    }
    pub fn wifi(&self) -> &WifiTransfer {
        &self.wifi
    }
    pub fn applications(&self) -> &crate::applications::ApplicationSelection {
        &self.applications
    }
    pub fn allow_unfree(&self) -> bool {
        self.allow_unfree
    }
    pub fn swap(&self) -> bool {
        self.swap
    }
    pub fn tuning(&self) -> bool {
        self.tuning
    }
    pub fn graphics(&self) -> &crate::graphics::Graphics {
        &self.graphics
    }
    pub fn onboarding(&self) -> &crate::github::Onboarding {
        &self.onboarding
    }
    pub(crate) fn take_password(&mut self) -> Zeroizing<String> {
        Zeroizing::new(std::mem::take(&mut *self.password))
    }

    /// Populate the reviewed plan from the live user before privilege elevation.
    #[cfg(feature = "network")]
    pub fn snapshot_wifi(mut self) -> Result<Self> {
        if self.wifi.enabled() {
            self.wifi = WifiTransfer::parse(true, crate::wifi::snapshot()?, &self.username)?;
        }
        Ok(self)
    }

    /// Downgrade at IPC for a preparation session. The privileged helper
    /// parses it again and accepts a separate confirmation phrase later.
    pub fn into_request(self) -> RawRequest {
        let mut request = ConfirmedInstall(self).into_request();
        request.confirmation.clear();
        request
    }

    /// Pure, cheap comparison: safe in a GUI callback. This is not authorization
    /// or a disk-state proof; the helper repeats parsing and the live checks.
    pub fn confirm(self, phrase: &str) -> Result<ConfirmedInstall> {
        ensure!(
            phrase == format!("ERASE {}", self.disk.path),
            "Type ERASE followed by the selected device path to confirm"
        );
        Ok(ConfirmedInstall(self))
    }
}

/// Only a matching erase phrase can produce this value.
/// ```compile_fail
/// use calamares_nixos::{InstallPlan, install::{self, InstallMode}};
/// fn bypass(plan: InstallPlan) { install::install(plan, InstallMode::Execute).unwrap(); }
/// ```
/// ```compile_fail
/// use calamares_nixos::ConfirmedInstall;
/// let confirmed: ConfirmedInstall = serde_json::from_str("{}").unwrap();
/// ```
pub struct ConfirmedInstall(InstallPlan);
impl ConfirmedInstall {
    pub(crate) fn into_plan(self) -> InstallPlan {
        self.0
    }

    /// Downgrade at IPC: the receiver must parse and confirm it again. Secrets
    /// remain in zeroizing owners until moved into the short-lived wire DTO.
    pub fn into_request(self) -> RawRequest {
        let mut plan = self.0;
        let (copy_wifi, wifi_profiles) = plan.wifi.into_raw();
        let (desktops, default_desktop) = plan.desktops.into_raw();
        let (github_user, ssh_keys, ssh_server, git_name, git_email) = plan.onboarding.into_raw();
        RawRequest {
            confirmation: format!("ERASE {}", plan.disk.path),
            disk: plan.disk,
            firmware: plan.firmware,
            filesystem: plan.filesystem,
            hostname: plan.hostname.0,
            username: plan.username.0,
            full_name: plan.full_name,
            password: std::mem::take(&mut *plan.password),
            locale: plan.locale.into(),
            timezone: plan.timezone.as_str().into(),
            keyboard: plan.keyboard.into(),
            desktops,
            default_desktop,
            applications: plan.applications.ids(),
            copy_wifi,
            wifi_profiles,
            allow_unfree: plan.allow_unfree,
            swap: plan.swap,
            tuning: plan.tuning,
            graphics: plan.graphics,
            github_user,
            ssh_keys,
            ssh_server,
            git_name,
            git_email,
        }
    }
}
