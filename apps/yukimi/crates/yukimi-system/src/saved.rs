// SPDX-License-Identifier: MIT OR Apache-2.0
//! The configuration each system generation was built from, as Yukimi's
//! helper keeps it: a copy of the configuration's directory in
//! [`SAVED_CONFIGURATIONS`](crate::SAVED_CONFIGURATIONS)`/generation-<n>`.
//!
//! Nix numbers a new generation one past the newest that is left, so once
//! the newest generations are deleted (`nix-collect-garbage -d`), the next
//! build takes the number a deleted one had. So a copy records the system
//! it was kept for, and is found only for a [`GenerationId`] (a number and
//! the system its link points at), whose link [`Snapshot::of`] reads again
//! when it looks. Neither a bare number nor the copy's directory leads to
//! a copy:
//!
//! ```compile_fail,E0308
//! // History reads generation 2's configuration by its number.
//! let root = std::path::Path::new(yukimi_system::SAVED_CONFIGURATIONS);
//! let _ = yukimi_system::saved::Snapshot::of(root, 2);
//! ```
//!
//! ```compile_fail,E0603
//! use yukimi_system::saved::GenerationId;
//! // A reader that finds generation 2's directory, skipping the check.
//! let root = std::path::Path::new(yukimi_system::SAVED_CONFIGURATIONS);
//! let profile = std::path::Path::new(yukimi_system::generations::SYSTEM_PROFILE);
//! let generation = GenerationId::read(profile, 2).unwrap();
//! let _ = yukimi_system::saved::directory(root, &generation);
//! ```
//!
//! ```compile_fail,E0603
//! use yukimi_system::saved::GenerationId;
//! // A migration that labels whatever copy has the number as today's.
//! fn relabel(copy: &std::path::Path, generation: &GenerationId) {
//!     let _ = yukimi_system::saved::mark(copy, generation);
//! }
//! ```
//!
//! ```
//! use yukimi_system::saved::{GenerationId, Saving, Snapshot};
//! let dir = std::env::temp_dir().join(format!("yukimi-saved-doc-{}", std::process::id()));
//! let (profiles, root) = (dir.join("profiles"), dir.join("configurations"));
//! std::fs::create_dir_all(&profiles).unwrap();
//! std::os::unix::fs::symlink("/nix/store/s1", profiles.join("system-2-link")).unwrap();
//! let generation = GenerationId::read(&profiles.join("system"), 2).unwrap();
//! let saving = Saving::begin(&root, &generation).unwrap();
//! std::fs::write(saving.dir().join("flake.lock"), "lock-B").unwrap();
//! saving.finish().unwrap();
//! assert!(Snapshot::of(&root, &generation).is_some());
//! // Generation 2 is deleted, and the next build is generation 2 again.
//! std::fs::remove_file(profiles.join("system-2-link")).unwrap();
//! std::os::unix::fs::symlink("/nix/store/s2", profiles.join("system-2-link")).unwrap();
//! assert!(Snapshot::of(&root, &generation).is_none(), "the id is checked against the link now");
//! let rebuilt = GenerationId::read(&profiles.join("system"), 2).unwrap();
//! assert!(Snapshot::of(&root, &rebuilt).is_none());
//! # std::fs::remove_dir_all(&dir).unwrap();
//! ```
//!
//! # Copies from before copies recorded their system
//!
//! Such a copy is taken for its generation when it was made no earlier
//! than the generation's link (both to the second): the helper wrote it
//! either right after making the link, or later while the generation ran,
//! and a copy for an earlier generation with the number is older than
//! today's link. A clock set back between the two writes can make a good
//! copy look older. Being wrong that way only hides the copy (History shows
//! no revisions for it, and going back doesn't restore it), so the rule
//! never deletes one while its generation exists: [`prune`] keeps it, and a
//! new copy for its generation ([`Saving::finish`]) sets it aside as
//! `generation-<n>.unrecorded`, which nothing reads and `prune` forgets with
//! the generation. The helper records the system of each such copy once it
//! is taken ([`Snapshot::record_system`]), so the times decide it only once.
use std::io::ErrorKind::NotFound;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use aizome_typestate::{SameTarget, Unsettled};

use crate::generations::{Generation, generation_number};

/// The file in each copy that names the system the copy was made for. A
/// copy of the configuration never has it: hidden files aren't copied.
const MADE_FOR: &str = ".system";

/// The extension of a copy that recorded no system and was replaced: set
/// aside, read by nothing, and forgotten with its generation.
const UNRECORDED: &str = "unrecorded";

/// A system generation, as it was when read: its number, its link, and
/// the system the link pointed at. [`Snapshot::of`] checks it against the
/// link again, so an id kept while the generation was deleted and rebuilt
/// finds nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenerationId {
    number: u32,
    link: PathBuf,
    system: PathBuf,
}

impl GenerationId {
    /// Generation `number` of `profile` (such as
    /// `/nix/var/nix/profiles/system`), if it exists.
    pub fn read(profile: &Path, number: u32) -> Option<GenerationId> {
        let name = profile.file_name()?.to_str()?;
        let link = profile.with_file_name(format!("{name}-{number}-link"));
        let system = std::fs::read_link(&link).ok()?;
        Some(GenerationId { number, link, system })
    }

    /// The generation `profile` points at.
    pub fn current(profile: &Path) -> Option<GenerationId> {
        let name = profile.file_name()?.to_str()?;
        let target = std::fs::read_link(profile).ok()?;
        GenerationId::read(profile, generation_number(target.file_name()?.to_str()?, name)?)
    }

    /// A generation as [`list`](crate::generations::list) found it. What
    /// it says is checked against its link when used.
    pub fn listed(generation: &Generation) -> GenerationId {
        GenerationId {
            number: generation.number,
            link: generation.link.clone(),
            system: PathBuf::from(generation.target.as_str()),
        }
    }

    pub fn number(&self) -> u32 {
        self.number
    }

    /// The system the generation is.
    pub fn system(&self) -> &Path {
        &self.system
    }

    /// When the generation's link was made, if it is still this
    /// generation: the link for this number, pointing at this system.
    fn linked(&self) -> Option<i64> {
        let name = self.link.file_name()?.to_str()?;
        name.strip_suffix(&format!("-{}-link", self.number))?;
        let fresh = GenerationId { system: std::fs::read_link(&self.link).ok()?, ..self.clone() };
        fresh.same_target(self).then_some(())?;
        Some(std::fs::symlink_metadata(&self.link).ok()?.mtime())
    }
}

/// Two generations are the same when they are the same system. The number
/// is what a later generation takes over.
impl SameTarget for GenerationId {
    fn same_target(&self, fresh: &Self) -> bool {
        self.system == fresh.system
    }
}

fn directory(root: &Path, generation: &GenerationId) -> PathBuf {
    root.join(format!("generation-{}", generation.number))
}

fn mark(copy: &Path, generation: &GenerationId) -> std::io::Result<()> {
    let mut bytes = generation.system.as_os_str().as_bytes().to_vec();
    bytes.push(b'\n');
    std::fs::write(copy.join(MADE_FOR), bytes)
}

/// What a copy records it was made for.
enum MadeFor {
    System(PathBuf),
    /// Made before copies recorded it.
    Unrecorded,
    Unreadable,
}

fn made_for(dir: &Path) -> MadeFor {
    match std::fs::read(dir.join(MADE_FOR)) {
        Ok(bytes) => {
            MadeFor::System(PathBuf::from(std::ffi::OsStr::from_bytes(bytes.strip_suffix(b"\n").unwrap_or(&bytes))))
        }
        Err(e) if e.kind() == NotFound => MadeFor::Unrecorded,
        Err(_) => MadeFor::Unreadable,
    }
}

/// The configuration a generation was built from, as kept for that very
/// generation.
#[derive(Debug)]
pub struct Snapshot {
    dir: PathBuf,
    generation: GenerationId,
    recorded: bool,
}

impl Snapshot {
    /// The copy kept in `root` for `generation`, unless the generation has
    /// changed since it was read, or the copy was made for an earlier
    /// generation that had its number.
    pub fn of(root: &Path, generation: &GenerationId) -> Option<Snapshot> {
        let linked = generation.linked()?;
        let dir = directory(root, generation);
        let recorded = match made_for(&dir) {
            MadeFor::System(system) => {
                GenerationId { system, ..generation.clone() }.same_target(generation).then_some(true)?
            }
            MadeFor::Unrecorded => {
                std::fs::metadata(&dir).is_ok_and(|meta| meta.is_dir() && meta.mtime() >= linked).then_some(false)?
            }
            MadeFor::Unreadable => return None,
        };
        Some(Snapshot { dir, generation: generation.clone(), recorded })
    }

    /// The copy's directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Records the system of a copy made before copies recorded it, so
    /// that later readings don't depend on the times of the copy and the
    /// link. Nothing to do for a copy that records it already.
    pub fn record_system(&self) -> std::io::Result<()> {
        if self.recorded { Ok(()) } else { mark(&self.dir, &self.generation) }
    }
}

/// A copy being saved for a generation: written into a directory of its
/// own, which [`finish`](Self::finish) labels and puts in place. Dropped
/// unfinished, it is removed.
#[derive(Debug)]
#[must_use = "a copy is kept only once it is finished"]
pub struct Saving {
    partial: Unsettled<PathBuf, fn(PathBuf)>,
    dir: PathBuf,
    generation: GenerationId,
}

fn remove_partial(partial: PathBuf) {
    let _ = std::fs::remove_dir_all(partial);
}

impl Saving {
    /// Starts a copy in `root` for `generation`.
    pub fn begin(root: &Path, generation: &GenerationId) -> std::io::Result<Saving> {
        let dir = directory(root, generation);
        let partial = dir.with_extension("new");
        let _ = std::fs::remove_dir_all(&partial);
        std::fs::create_dir_all(&partial)?;
        Ok(Saving { partial: Unsettled::new(partial, remove_partial), dir, generation: generation.clone() })
    }

    /// Where to write the copy.
    pub fn dir(&self) -> &Path {
        self.partial.get()
    }

    /// Labels the copy with its generation's system and puts it in place
    /// of any copy kept for the number. The generation must still be the
    /// one the copy began for.
    pub fn finish(self) -> std::io::Result<Snapshot> {
        if self.generation.linked().is_none() {
            return Err(std::io::Error::other(format!(
                "generation {} changed while its configuration was being kept",
                self.generation.number
            )));
        }
        mark(self.partial.get(), &self.generation)?;
        let partial = self.partial.settle();
        // A copy that records no system may have been hidden by the time
        // rule only: set it aside rather than lose it.
        if matches!(made_for(&self.dir), MadeFor::Unrecorded) && self.dir.is_dir() {
            let aside = self.dir.with_extension(UNRECORDED);
            let _ = std::fs::remove_dir_all(&aside);
            let _ = std::fs::rename(&self.dir, &aside);
        }
        let _ = std::fs::remove_dir_all(&self.dir);
        if let Err(e) = std::fs::rename(&partial, &self.dir) {
            remove_partial(partial);
            return Err(e);
        }
        Ok(Snapshot { dir: self.dir, generation: self.generation, recorded: true })
    }
}

/// Forgets, in `root`, the copies of `profile`'s generations that no longer
/// exist, those recorded for another system than their generation's, and
/// unfinished ones. A copy that records no system is kept while its
/// generation exists (see [the rule](self#copies-from-before-copies-recorded-their-system));
/// one that the rule takes for its generation records it from now on.
pub fn prune(root: &Path, profile: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let number = name.strip_prefix("generation-");
        // Set aside: kept with its generation.
        if let Some(n) = number.and_then(|n| n.strip_suffix(&format!(".{UNRECORDED}"))) {
            if n.parse::<u32>().ok().and_then(|n| GenerationId::read(profile, n)).is_none() {
                let _ = std::fs::remove_dir_all(entry.path());
            }
            continue;
        }
        let number = number.and_then(|n| n.parse::<u32>().ok());
        let keep = match number.and_then(|n| GenerationId::read(profile, n)) {
            None => false,
            Some(generation) => match Snapshot::of(root, &generation) {
                Some(snapshot) => {
                    let _ = snapshot.record_system();
                    true
                }
                None => matches!(made_for(&entry.path()), MadeFor::Unrecorded),
            },
        };
        if !keep {
            let _ = std::fs::remove_dir_all(entry.path());
        }
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
        fn save(&self, generation: &GenerationId) -> Snapshot {
            Saving::begin(&self.root(), generation).unwrap().finish().unwrap()
        }
        fn names(&self) -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(self.root())
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
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
        let saved = f.save(&first);
        assert_eq!(Snapshot::of(&f.root(), &first).unwrap().dir(), saved.dir());
        // Deleted and rebuilt with the same number: another system.
        let rebuilt = f.link(2, "/nix/store/s2");
        assert_eq!(rebuilt.number(), first.number());
        assert!(Snapshot::of(&f.root(), &rebuilt).is_none());
        assert!(Snapshot::of(&f.root(), &first).is_none(), "the old id no longer matches its link");
        assert!(Snapshot::of(&f.root(), &f.link(3, "/nix/store/s1")).is_none(), "another number");
    }

    #[test]
    fn an_unrecorded_copy_counts_only_if_made_after_the_link_and_is_then_recorded() {
        let f = Fixture::new("unrecorded");
        let generation = f.link(4, "/nix/store/s4");
        let copy = directory(&f.root(), &generation);
        std::fs::create_dir_all(&copy).unwrap();
        let snapshot = Snapshot::of(&f.root(), &generation).unwrap();
        snapshot.record_system().unwrap();
        assert!(matches!(made_for(&copy), MadeFor::System(s) if s == generation.system()));
        // A link made after the copy: today's generation 4 isn't the copy's.
        std::fs::remove_file(copy.join(MADE_FOR)).unwrap();
        crate::sys::set_link_modified(&f.dir.join("profiles/system-4-link"), 60);
        assert!(Snapshot::of(&f.root(), &generation).is_none());
    }

    #[test]
    fn a_copy_saved_for_a_generation_that_changed_meanwhile_is_dropped() {
        let f = Fixture::new("changed");
        let generation = f.link(5, "/nix/store/s5");
        let saving = Saving::begin(&f.root(), &generation).unwrap();
        f.link(5, "/nix/store/s5-rebuilt");
        assert!(saving.finish().is_err());
        assert!(f.names().is_empty(), "{:?}", f.names());
        drop(Saving::begin(&f.root(), &generation).unwrap());
        assert!(f.names().is_empty(), "an unfinished copy is removed");
    }

    #[test]
    fn pruning_forgets_deleted_and_replaced_generations_but_not_unrecorded_ones() {
        let f = Fixture::new("prune");
        for (n, system) in [(1, "/nix/store/s1"), (2, "/nix/store/s2"), (3, "/nix/store/s3")] {
            f.save(&f.link(n, system));
        }
        std::fs::create_dir_all(f.root().join("generation-4.new")).unwrap();
        // An unrecorded copy of an existing generation, made before its link.
        let six = f.link(6, "/nix/store/s6");
        std::fs::create_dir_all(directory(&f.root(), &six)).unwrap();
        crate::sys::set_link_modified(&f.dir.join("profiles/system-6-link"), 60);
        std::fs::remove_file(f.dir.join("profiles/system-1-link")).unwrap();
        f.link(2, "/nix/store/s2-rebuilt");
        prune(&f.root(), &f.profile());
        assert_eq!(f.names(), ["generation-3", "generation-6"]);
    }

    #[test]
    fn a_hidden_unrecorded_copy_is_set_aside_not_lost_when_replaced() {
        let f = Fixture::new("aside");
        // A copy that records no system, hidden by a clock set back.
        let generation = f.link(6, "/nix/store/s6");
        let copy = directory(&f.root(), &generation);
        std::fs::create_dir_all(&copy).unwrap();
        std::fs::write(copy.join("flake.lock"), "made by hand long ago").unwrap();
        crate::sys::set_link_modified(&f.dir.join("profiles/system-6-link"), 60);
        assert!(Snapshot::of(&f.root(), &generation).is_none());
        // The next change keeps the configuration as it is now.
        let saving = Saving::begin(&f.root(), &generation).unwrap();
        std::fs::write(saving.dir().join("flake.lock"), "now").unwrap();
        saving.finish().unwrap();
        let aside = f.root().join("generation-6.unrecorded/flake.lock");
        assert_eq!(std::fs::read_to_string(aside).unwrap(), "made by hand long ago");
        prune(&f.root(), &f.profile());
        assert_eq!(f.names(), ["generation-6", "generation-6.unrecorded"]);
        std::fs::remove_file(f.dir.join("profiles/system-6-link")).unwrap();
        prune(&f.root(), &f.profile());
        assert!(f.names().is_empty(), "{:?}", f.names());
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
