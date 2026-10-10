// SPDX-License-Identifier: MIT OR Apache-2.0
//! Generations: every version of the system Nix keeps, so any of them can
//! be returned to.
//!
//! A profile (`/nix/var/nix/profiles/system`, or a user's packages) is a
//! symlink to its current generation, `system-42-link`, which points at a
//! build in the store. Each generation's link was made when it was built,
//! and the build says which NixOS release and kernel it holds.
use std::path::{Path, PathBuf};

use yukimi_store::StorePath;

/// The system profile.
pub const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Generation {
    pub number: u32,
    pub link: PathBuf,
    /// The build this generation is.
    pub target: StorePath,
    /// When it was made, as Unix time.
    pub created: i64,
    /// For system generations: the NixOS release, such as `26.11.20261001.c59305b`.
    pub nixos_version: Option<String>,
    /// For system generations: the kernel version.
    pub kernel_version: Option<String>,
    /// The profile points at this one.
    pub current: bool,
    /// The machine started with this one.
    pub booted: bool,
}

/// The generations of a profile, oldest first. `profile` is the profile's
/// symlink, such as `/nix/var/nix/profiles/system`; `booted` is the store
/// path the machine started with, if known.
pub fn list(profile: &Path, booted: Option<&str>) -> Vec<Generation> {
    let (Some(dir), Some(name)) = (profile.parent(), profile.file_name().and_then(|n| n.to_str())) else {
        return Vec::new();
    };
    let current = std::fs::read_link(profile)
        .ok()
        .and_then(|target| target.file_name().map(|n| n.to_string_lossy().into_owned()));
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut generations: Vec<Generation> = entries
        .flatten()
        .filter_map(|entry| {
            let file = entry.file_name().to_string_lossy().into_owned();
            let number = generation_number(&file, name)?;
            let link = entry.path();
            let target = std::fs::read_link(&link).ok()?;
            let target = StorePath::parse(target.to_str()?)?;
            let created = std::fs::symlink_metadata(&link).map(|m| mtime(&m)).unwrap_or(0);
            let nixos_version = std::fs::read_to_string(Path::new(target.as_str()).join("nixos-version"))
                .ok()
                .map(|v| v.trim().to_owned());
            let kernel_version = std::fs::read_link(Path::new(target.as_str()).join("kernel"))
                .ok()
                .and_then(|k| StorePath::parse(k.to_str()?).map(|p| p.version().to_owned()))
                .filter(|v| !v.is_empty());
            Some(Generation {
                current: current.as_deref() == Some(file.as_str()),
                booted: booted == Some(target.as_str()),
                number,
                link,
                target,
                created,
                nixos_version,
                kernel_version,
            })
        })
        .collect();
    generations.sort_by_key(|g| g.number);
    generations
}

/// The system's generations, with the running and started ones marked.
pub fn system() -> Vec<Generation> {
    let booted = std::fs::read_link("/run/booted-system")
        .ok()
        .and_then(|p| p.to_str().and_then(StorePath::parse))
        .map(|p| p.as_str().to_owned());
    list(Path::new(SYSTEM_PROFILE), booted.as_deref())
}

/// `system-42-link` is generation 42 of `system`.
pub fn generation_number(file: &str, profile: &str) -> Option<u32> {
    file.strip_prefix(profile)?.strip_prefix('-')?.strip_suffix("-link")?.parse().ok()
}

fn mtime(meta: &std::fs::Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    meta.mtime()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn numbers_from_link_names() {
        assert_eq!(generation_number("system-42-link", "system"), Some(42));
        assert_eq!(generation_number("profile-3-link", "profile"), Some(3));
        assert_eq!(generation_number("system", "system"), None);
        assert_eq!(generation_number("system-boot-link", "system"), None);
        assert_eq!(generation_number("system-1-link", "profile"), None);
    }

    #[test]
    fn generations_in_a_profile_directory() {
        let dir = std::env::temp_dir().join(format!("yukimi-gens-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let h = "0123456789abcdfghijklmnpqrsvwxyz";
        symlink(format!("/nix/store/{h}-nixos-system-acer-26.11.1"), dir.join("system-1-link")).unwrap();
        symlink(format!("/nix/store/{h}-nixos-system-acer-26.11.2"), dir.join("system-2-link")).unwrap();
        symlink("system-2-link", dir.join("system")).unwrap();
        let booted = format!("/nix/store/{h}-nixos-system-acer-26.11.1");
        let gens = list(&dir.join("system"), Some(&booted));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(gens.iter().map(|g| g.number).collect::<Vec<_>>(), vec![1, 2]);
        assert!(gens[1].current && !gens[0].current);
        assert!(gens[0].booted && !gens[1].booted);
        assert_eq!(gens[1].target.version(), "26.11.2");
    }
}
