//! Where Yukimi reads the system from and keeps its own files.
//!
//! The system is read where NixOS keeps it: `/etc/nixos` and Yukimi's
//! module's facts in `/etc/yukimi/system.json`. For development, and for
//! pictures of another system, `AIZOME_YUKIMI_ETC` names a directory to
//! read in place of `/etc` (a host's, mounted elsewhere); Yukimi then only
//! looks, and changes nothing, since the helper acts on the real `/etc`.
use std::path::{Path, PathBuf};

use yukimi_system::setup::Facts;

/// Where the system's configuration and facts are read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    /// Stands for `/etc`.
    pub etc: PathBuf,
    /// Read from somewhere other than `/etc`: look, don't change.
    pub elsewhere: bool,
}

impl Default for Paths {
    fn default() -> Paths {
        Paths { etc: PathBuf::from("/etc"), elsewhere: false }
    }
}

impl Paths {
    /// This system's, or the one `AIZOME_YUKIMI_ETC` names.
    pub fn from_env() -> Paths {
        match std::env::var_os("AIZOME_YUKIMI_ETC").filter(|v| !v.is_empty()) {
            Some(dir) if Path::new(&dir) != Path::new("/etc") => Paths { etc: PathBuf::from(dir), elsewhere: true },
            _ => Paths::default(),
        }
    }

    /// The directory a NixOS system is configured in.
    pub fn etc_nixos(&self) -> PathBuf {
        self.etc.join("nixos")
    }

    /// What Yukimi's NixOS module recorded about the running system, as
    /// [`Facts::read`] reads `/etc/yukimi/system.json`.
    pub fn facts(&self) -> Facts {
        let file = self.etc.join("yukimi/system.json");
        let mut facts: Facts =
            std::fs::read_to_string(file).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        // A channel's Nixpkgs is recorded where it was found, a link into the
        // store; and a path that is gone (collected) is no use.
        facts.nixpkgs = facts
            .nixpkgs
            .and_then(|path| std::fs::canonicalize(path).ok())
            .map(|path| path.to_string_lossy().into_owned());
        facts
    }
}

/// `$XDG_CACHE_HOME`, or `~/.cache`.
fn cache_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
}

/// Yukimi's cache: the package index, the last check for updates and an
/// update being got ready. `~/.cache/aizome/yukimi`.
pub fn cache_dir() -> Option<PathBuf> {
    Some(cache_home()?.join("aizome/yukimi"))
}

/// The GTK Yukimi's cache, `~/.cache/yukimi`: its package index and last
/// check serve until Yukimi has its own, so a system that already has one
/// needn't spend a minute (and gigabytes) making it again.
pub fn shared_cache_dir() -> Option<PathBuf> {
    Some(cache_home()?.join("yukimi"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_come_from_the_etc_given() {
        let dir = std::env::temp_dir().join(format!("aizome-yukimi-paths-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("yukimi")).unwrap();
        std::fs::write(
            dir.join("yukimi/system.json"),
            r#"{"allowUnfree":true,"catalogs":[],"configuration":null,"nixpkgs":"/nonexistent-nixpkgs"}"#,
        )
        .unwrap();
        let paths = Paths { etc: dir.clone(), elsewhere: true };
        let facts = paths.facts();
        assert_eq!(facts.allow_unfree, Some(true));
        // A Nixpkgs that isn't there any more is no use.
        assert_eq!(facts.nixpkgs, None);
        assert_eq!(paths.etc_nixos(), dir.join("nixos"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
