// SPDX-License-Identifier: MIT OR Apache-2.0
//! The configuration each system generation was built from, as Yukimi's
//! helper keeps it: a copy of the configuration's directory in
//! [`SAVED_CONFIGURATIONS`](crate::SAVED_CONFIGURATIONS)`/generation-<n>`.
//!
//! Nix numbers a new generation one past the newest that is left, so once
//! the newest generations are deleted (`nix-collect-garbage -d`), the next
//! build takes the number a deleted one had. A copy is therefore found by
//! the generation as it is now, [`GenerationId`]: its number *and* the
//! system its link points at, which each copy records. Looking a copy up by
//! the number alone doesn't compile:
//!
//! ```compile_fail,E0308
//! // History reads generation 2's configuration by its number.
//! let root = std::path::Path::new(yukimi_system::SAVED_CONFIGURATIONS);
//! let _ = yukimi_system::saved::Snapshot::of(root, 2);
//! ```
//!
//! ```
//! use yukimi_system::saved::{GenerationId, Snapshot};
//! let dir = std::env::temp_dir().join(format!("yukimi-saved-doc-{}", std::process::id()));
//! let (profiles, root) = (dir.join("profiles"), dir.join("configurations"));
//! std::fs::create_dir_all(&profiles).unwrap();
//! std::os::unix::fs::symlink("/nix/store/s1", profiles.join("system-2-link")).unwrap();
//! let generation = GenerationId::read(&profiles.join("system"), 2).unwrap();
//! let copy = yukimi_system::saved::directory(&root, &generation);
//! std::fs::create_dir_all(&copy).unwrap();
//! yukimi_system::saved::mark(&copy, &generation).unwrap();
//! assert!(Snapshot::of(&root, &generation).is_some());
//! // Generation 2 is deleted, and the next build is generation 2 again.
//! std::fs::remove_file(profiles.join("system-2-link")).unwrap();
//! std::os::unix::fs::symlink("/nix/store/s2", profiles.join("system-2-link")).unwrap();
//! let rebuilt = GenerationId::read(&profiles.join("system"), 2).unwrap();
//! assert!(Snapshot::of(&root, &rebuilt).is_none());
//! # std::fs::remove_dir_all(&dir).unwrap();
//! ```
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use aizome_typestate::SameTarget;

use crate::generations::{Generation, generation_number};

/// The file in each copy that names the system the copy was made for. A
/// copy of the configuration never has it: hidden files aren't copied.
const MADE_FOR: &str = ".system";

/// A system generation as it is now. Only reading the profile (or a
/// generation [`list`](crate::generations::list)ed from it) makes one, so
/// it always carries the system that the number names today.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenerationId {
    number: u32,
    /// What the generation's link points at.
    system: PathBuf,
    /// When its link was made, as Unix time.
    linked: i64,
}

impl GenerationId {
    /// Generation `number` of `profile` (such as
    /// `/nix/var/nix/profiles/system`), if it exists.
    pub fn read(profile: &Path, number: u32) -> Option<GenerationId> {
        let name = profile.file_name()?.to_str()?;
        let link = profile.with_file_name(format!("{name}-{number}-link"));
        let system = std::fs::read_link(&link).ok()?;
        let linked = std::fs::symlink_metadata(&link).ok()?.mtime();
        Some(GenerationId { number, system, linked })
    }

    /// The generation `profile` points at.
    pub fn current(profile: &Path) -> Option<GenerationId> {
        let name = profile.file_name()?.to_str()?;
        let target = std::fs::read_link(profile).ok()?;
        GenerationId::read(profile, generation_number(target.file_name()?.to_str()?, name)?)
    }

    /// A generation as [`list`](crate::generations::list) found it.
    pub fn listed(generation: &Generation) -> GenerationId {
        GenerationId {
            number: generation.number,
            system: PathBuf::from(generation.target.as_str()),
            linked: generation.created,
        }
    }

    pub fn number(&self) -> u32 {
        self.number
    }

    /// The system the generation is.
    pub fn system(&self) -> &Path {
        &self.system
    }
}

/// Two generations are the same when they are the same system. The number
/// is what a later generation takes over.
impl SameTarget for GenerationId {
    fn same_target(&self, fresh: &Self) -> bool {
        self.system == fresh.system
    }
}

/// Where the copy for `generation` goes.
pub fn directory(root: &Path, generation: &GenerationId) -> PathBuf {
    root.join(format!("generation-{}", generation.number))
}

/// Records in `copy`, a configuration being saved, which generation it is
/// for.
pub fn mark(copy: &Path, generation: &GenerationId) -> std::io::Result<()> {
    let mut bytes = generation.system.as_os_str().as_bytes().to_vec();
    bytes.push(b'\n');
    std::fs::write(copy.join(MADE_FOR), bytes)
}

/// The configuration a generation was built from, as kept for that very
/// generation.
#[derive(Debug)]
pub struct Snapshot {
    dir: PathBuf,
}

impl Snapshot {
    /// The copy kept in `root` for `generation`, unless it was made for an
    /// earlier generation that had the same number.
    pub fn of(root: &Path, generation: &GenerationId) -> Option<Snapshot> {
        let dir = directory(root, generation);
        let made_for = match std::fs::read(dir.join(MADE_FOR)) {
            Ok(bytes) => {
                let system = Path::new(std::ffi::OsStr::from_bytes(bytes.strip_suffix(b"\n").unwrap_or(&bytes)));
                GenerationId { number: generation.number, system: system.to_owned(), linked: 0 }.same_target(generation)
            }
            // Copies made before they recorded their system: one made
            // after this generation's link is this generation's.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::metadata(&dir).is_ok_and(|meta| meta.is_dir() && meta.mtime() >= generation.linked)
            }
            Err(_) => false,
        };
        made_for.then_some(Snapshot { dir })
    }

    /// The copy's directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        dir: PathBuf,
    }

    impl Fixture {
        fn new(tag: &str) -> Fixture {
            let dir = std::env::temp_dir().join(format!("yukimi-saved-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("profiles")).unwrap();
            std::fs::create_dir_all(dir.join("configurations")).unwrap();
            Fixture { dir }
        }
        fn profile(&self) -> PathBuf {
            self.dir.join("profiles/system")
        }
        fn root(&self) -> PathBuf {
            self.dir.join("configurations")
        }
        fn link(&self, number: u32, system: &str) -> GenerationId {
            let link = self.dir.join(format!("profiles/system-{number}-link"));
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(system, &link).unwrap();
            GenerationId::read(&self.profile(), number).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn a_copy_belongs_to_the_system_it_was_made_for() {
        let f = Fixture::new("system");
        let first = f.link(2, "/nix/store/s1");
        let copy = directory(&f.root(), &first);
        std::fs::create_dir_all(&copy).unwrap();
        mark(&copy, &first).unwrap();
        assert_eq!(Snapshot::of(&f.root(), &first).unwrap().dir(), copy);
        // Deleted and rebuilt with the same number: another system.
        let rebuilt = f.link(2, "/nix/store/s2");
        assert_eq!(rebuilt.number(), first.number());
        assert!(Snapshot::of(&f.root(), &rebuilt).is_none());
        assert!(Snapshot::of(&f.root(), &f.link(3, "/nix/store/s1")).is_none(), "another number");
    }

    #[test]
    fn an_unmarked_copy_counts_only_if_made_after_the_link() {
        let f = Fixture::new("unmarked");
        let generation = f.link(4, "/nix/store/s4");
        let copy = directory(&f.root(), &generation);
        std::fs::create_dir_all(&copy).unwrap();
        assert!(Snapshot::of(&f.root(), &generation).is_some());
        let later = GenerationId { linked: generation.linked + 60, ..generation };
        assert!(Snapshot::of(&f.root(), &later).is_none());
    }

    #[test]
    fn the_current_generation_is_read_through_the_profile() {
        let f = Fixture::new("current");
        f.link(7, "/nix/store/s7");
        std::os::unix::fs::symlink("system-7-link", f.profile()).unwrap();
        let current = GenerationId::current(&f.profile()).unwrap();
        assert_eq!((current.number(), current.system()), (7, Path::new("/nix/store/s7")));
        assert!(GenerationId::read(&f.profile(), 8).is_none());
    }
}
