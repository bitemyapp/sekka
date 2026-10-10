// SPDX-License-Identifier: MIT OR Apache-2.0
//! A NixOS system as a person sees it.
//!
//! - [`setup`]: how the system is configured: a flake or channels, where,
//!   and which file Yukimi adds its own to.
//! - [`info`]: this computer: its name, NixOS release, kernel, and whether a
//!   restart would finish an update.
//! - [`generations`]: every version of the system (and of a user's
//!   packages) that can be returned to.
//! - [`diff`]: what changed between two of them, package by package.
//! - [`profile`]: packages a user installed for themselves.
//! - [`catalog`]: well-known applications to offer, Yukimi's and the
//!   system's.
//! - [`channels`]: where a system without a flake gets Nixpkgs from.
//! - [`updates`]: whether the system's sources have newer versions, and
//!   getting an update ready without administrator rights.
//! - [`index`]: every package in Nixpkgs, searchable.
//! - [`log`]: Nix's progress while it downloads and builds.
//! - [`nix`]: running Nix itself.
pub mod catalog;
pub mod channels;
pub mod diff;
pub mod generations;
pub mod index;
pub mod info;
pub mod log;
pub mod nix;
pub mod profile;
pub mod setup;
pub mod updates;

/// The directory a NixOS system is configured in.
pub const CONFIG_DIR: &str = "/etc/nixos";

/// Where Yukimi's helper keeps a copy of the configuration each system
/// generation was built from, as `generation-<n>`.
pub const SAVED_CONFIGURATIONS: &str = "/var/lib/yukimi/configurations";

/// The copy of generation `generation`'s configuration.
pub fn saved_configuration(generation: u32) -> std::path::PathBuf {
    std::path::Path::new(SAVED_CONFIGURATIONS).join(format!("generation-{generation}"))
}

/// Bytes as a person reads them: `1.4 GB`, `312 MB`, `40 kB`.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

/// How long ago something happened: `just now`, `5 minutes ago`,
/// `3 days ago`, `2 months ago`.
pub fn human_age(then: i64, now: i64) -> String {
    let seconds = (now - then).max(0);
    let (amount, unit) = match seconds {
        0..60 => return "just now".to_owned(),
        60..3600 => (seconds / 60, "minute"),
        3600..86400 => (seconds / 3600, "hour"),
        86400..2_592_000 => (seconds / 86400, "day"),
        2_592_000..31_536_000 => (seconds / 2_592_000, "month"),
        _ => (seconds / 31_536_000, "year"),
    };
    format!("{amount} {unit}{} ago", if amount == 1 { "" } else { "s" })
}

/// Unix time now.
pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_ages() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(40_960), "41 kB");
        assert_eq!(human_size(1_400_000_000), "1.4 GB");
        assert_eq!(human_size(312_000_000), "312 MB");
        assert_eq!(human_age(100, 110), "just now");
        assert_eq!(human_age(0, 300), "5 minutes ago");
        assert_eq!(human_age(0, 3600), "1 hour ago");
        assert_eq!(human_age(0, 3 * 86400), "3 days ago");
        assert_eq!(human_age(0, 70 * 86400), "2 months ago");
    }
}
