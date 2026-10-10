// SPDX-License-Identifier: MIT OR Apache-2.0
//! Channels: where a system without a flake gets Nixpkgs from.
//!
//! `nix-channel --update`, run as root, downloads each of root's channels
//! into the store and links them from root's channels profile, which anyone
//! can read: `nixos` links to `/nix/store/…-nixos-26.05/nixos`, whose
//! `.git-revision` says which Nixpkgs commit it is. The NixOS channels are
//! published at `https://channels.nixos.org/<name>`, under the name the
//! store path carries, with the newest commit in `git-revision`.
use std::path::Path;

use yukimi_store::StorePath;

/// Root's channels profile.
pub const ROOT_CHANNELS: &str = "/nix/var/nix/profiles/per-user/root/channels";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Channel {
    /// What `<…>` calls it: `nixos`.
    pub name: String,
    /// What it was downloaded as: `nixos-26.05`.
    pub release: String,
    /// The Nixpkgs commit it is.
    pub revision: Option<String>,
    /// When the channels were last updated, as Unix time.
    pub updated: i64,
}

impl Channel {
    /// Where the NixOS project publishes this channel, when it is one of
    /// theirs: `https://channels.nixos.org/nixos-26.05`.
    pub fn published_at(&self) -> Option<String> {
        let official = ["nixos-", "nixpkgs-"].iter().any(|p| self.release.starts_with(p))
            && self.release.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b));
        official.then(|| format!("https://channels.nixos.org/{}", self.release))
    }

    /// The first characters of the revision, as Git shows them.
    pub fn short_revision(&self) -> Option<&str> {
        self.revision.as_deref().map(|rev| &rev[..rev.len().min(7)])
    }
}

/// Root's channels.
pub fn root() -> Vec<Channel> {
    list(Path::new(ROOT_CHANNELS))
}

/// The channels in a channels profile.
pub fn list(profile: &Path) -> Vec<Channel> {
    let updated = std::fs::read_link(profile)
        .ok()
        .map(|generation| profile.parent().map(|dir| dir.join(&generation)).unwrap_or(generation))
        .and_then(|link| std::fs::symlink_metadata(link).ok())
        .map(|meta| std::os::unix::fs::MetadataExt::mtime(&meta))
        .unwrap_or(0);
    let Ok(entries) = std::fs::read_dir(profile) else {
        return Vec::new();
    };
    let mut channels: Vec<Channel> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name == "manifest.nix" {
                return None;
            }
            let target = std::fs::read_link(entry.path()).ok()?;
            let release = store_dir(&target)?;
            let revision = std::fs::read_to_string(target.join(".git-revision"))
                .ok()
                .map(|r| r.trim().to_owned())
                .filter(|r| !r.is_empty());
            Some(Channel { name, release, revision, updated })
        })
        .collect();
    channels.sort_by(|a, b| a.name.cmp(&b.name));
    channels
}

/// The name of the store path a channel's link points into:
/// `/nix/store/…-nixos-26.05/nixos` is `nixos-26.05`.
fn store_dir(target: &Path) -> Option<String> {
    Some(StorePath::parse(target.to_str()?)?.full_name().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn channels_in_a_profile() {
        let dir = std::env::temp_dir().join(format!("yukimi-channels-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let generation = dir.join("channels-1-link");
        std::fs::create_dir_all(&generation).unwrap();
        symlink("channels-1-link", dir.join("channels")).unwrap();
        let h = "0123456789abcdfghijklmnpqrsvwxyz";
        symlink(format!("/nix/store/{h}-nixos-26.05/nixos"), generation.join("nixos")).unwrap();
        symlink(format!("/nix/store/{h}-env-manifest.nix"), generation.join("manifest.nix")).unwrap();
        let channels = list(&dir.join("channels"));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(channels.len(), 1);
        assert_eq!((channels[0].name.as_str(), channels[0].release.as_str()), ("nixos", "nixos-26.05"));
        assert_eq!(channels[0].published_at().as_deref(), Some("https://channels.nixos.org/nixos-26.05"));
        let home_manager = Channel { release: "master".into(), ..channels[0].clone() };
        assert_eq!(home_manager.published_at(), None);
    }
}
