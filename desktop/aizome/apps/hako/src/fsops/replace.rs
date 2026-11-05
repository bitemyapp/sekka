//! Replacing an item, in stages that only the stage before makes:
//!
//! 1. [`Occupant`]: the item at the target, as the person saw it when they
//!    answered its conflict with Replace (its device and inode). Only
//!    [`ask`], given that answer, makes one.
//! 2. [`SetAside`]: it's renamed to a hidden name beside the target
//!    (`.name.hako-aside-…`) and checked to be the item the person saw.
//!    It's put back if its replacement doesn't come
//!    ([`SetAside::put_back`]), or if it's dropped on the way (an early
//!    return, a panic). It becomes [`Replaced`] only with the proof that
//!    its replacement is in place at its own target ([`InPlace`]).
//! 3. [`Replaced`]: it goes to the Trash, as the conflict sheet promises
//!    ("Replacing moves the one there to the Trash"). There's no other way
//!    for it to go: no operation can choose to delete it. One the Trash
//!    refuses is [`Kept`] beside its replacement under a visible name
//!    (`a (replaced).txt`) and reported.

// Typestate code: every state and every case is named, so a new one
// added later can't fall into an arm written for others.
#![deny(clippy::wildcard_enum_match_arm)]

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use aizome_typestate::{Confirmed, Reviewed, SameTarget, Unsettled};

use super::place::InPlace;
use super::trash::Trash;
use super::{Asker, Choice, Conflict, Consent, Resolution, describe, names, sys};

/// An item, by what stays with it (its device and inode), and where it
/// was found.
#[derive(Clone, Debug)]
pub struct Found {
    path: PathBuf,
    dev: u64,
    ino: u64,
    is_dir: bool,
}

impl Found {
    fn at(path: &Path, meta: &fs::Metadata) -> Found {
        Found { path: path.to_path_buf(), dev: meta.dev(), ino: meta.ino(), is_dir: meta.is_dir() }
    }
}

impl SameTarget for Found {
    fn same_target(&self, fresh: &Self) -> bool {
        self.dev == fresh.dev && self.ino == fresh.ino
    }
}

/// The brand of a replacement's review: the person was shown the item at
/// the target and pressed Replace. Only [`ask`] makes one, from their
/// [`Consent`], which only their press makes, so nothing else can claim the
/// person chose to replace an item:
///
/// ```compile_fail,E0423
/// use hako::fsops::replace::Asked;
/// let _ = Asked(());
/// ```
#[derive(Debug)]
pub struct Asked(Consent);

/// The item at a replacement's target, as the person saw it when they
/// answered Replace.
#[derive(Debug)]
pub struct Occupant {
    seen: Reviewed<Found, Asked>,
}

/// The answer to a conflict, with the [`Occupant`] when it's Replace.
#[derive(Debug)]
pub(super) enum Answer {
    Skip,
    Cancel,
    KeepBoth,
    Merge,
    Replace(Occupant),
}

/// Ask about `conflict`, `tmeta` being what's at its target (not
/// followed). Merge is an answer only for folders: for anything else it
/// keeps both (replacing takes the person's consent, which Merge isn't).
pub(super) fn ask(asker: &mut dyn Asker, conflict: &Conflict, tmeta: &fs::Metadata) -> Answer {
    match asker.conflict(conflict) {
        Choice::Other(Resolution::Skip) => Answer::Skip,
        Choice::Other(Resolution::Cancel) => Answer::Cancel,
        Choice::Other(Resolution::KeepBoth) => Answer::KeepBoth,
        Choice::Other(Resolution::Merge) if conflict.can_merge() => Answer::Merge,
        Choice::Other(Resolution::Merge) => Answer::KeepBoth,
        Choice::Replace(consent) => {
            Answer::Replace(Occupant { seen: Reviewed::new(Found::at(&conflict.target, tmeta), Asked(consent)) })
        }
    }
}

/// Why an [`Occupant`] wasn't set aside.
#[derive(Debug)]
pub enum NotSetAside {
    /// The rename failed: it's where it was (try again, or leave it).
    Failed(Occupant, io::Error),
    /// Another item had taken its place since the person saw it. That one
    /// was put back, or, if it couldn't be, is [`Stranded`].
    Changed(Option<Stranded>),
}

impl Occupant {
    /// Where it is.
    pub fn target(&self) -> &Path {
        &self.seen.target().path
    }

    /// Rename it to a hidden name beside it, until its replacement is in
    /// place, and check that what moved is the item the person saw.
    pub fn set_aside(self) -> Result<SetAside, NotSetAside> {
        let target = self.target().to_path_buf();
        let dir = target.parent().unwrap_or(Path::new("/"));
        let aside = loop {
            let aside = dir.join(names::aside_name(target.file_name().unwrap_or_default()));
            match sys::rename_noreplace(&target, &aside) {
                Ok(()) => break aside,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(NotSetAside::Failed(self, e)),
            }
        };
        let Ok(meta) = fs::symlink_metadata(&aside) else { return Err(NotSetAside::Changed(None)) };
        match self.seen.confirm(Found::at(&aside, &meta)) {
            Ok(item) => Ok(SetAside { item: Unsettled::new((item, target), put_back_quietly) }),
            Err(_) => {
                let back = sys::rename_noreplace(&aside, &target);
                Err(NotSetAside::Changed(back.err().map(|e| Stranded { at: aside, target, why: describe(&e) })))
            }
        }
    }
}

/// An item set aside, and its target.
type Aside = (Confirmed<Found, Asked>, PathBuf);

/// A set-aside item dropped on the way goes back to its name, if that's
/// free.
fn put_back_quietly((item, target): Aside) {
    let _ = sys::rename_noreplace(&item.target().path, &target);
}

/// A replaced item dropped before it went is kept beside its replacement.
fn keep_quietly((item, target): Aside) {
    let _ = keep(item.target(), &target);
}

/// Rename a replaced item at `item` to a visible name beside its
/// replacement at `target`: where it is now.
fn keep(item: &Found, target: &Path) -> PathBuf {
    let dir = target.parent().unwrap_or(Path::new("/"));
    for _ in 0..8 {
        let name = names::replaced_name(dir, target.file_name().unwrap_or_default(), item.is_dir);
        let to = dir.join(name);
        match sys::rename_noreplace(&item.path, &to) {
            Ok(()) => return to,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => break,
        }
    }
    item.path.clone()
}

/// A replaced item under its hidden name, the one the person saw. Its
/// replacement is on the way; it can't go before that's in place:
///
/// ```compile_fail,E0599
/// use hako::fsops::replace::SetAside;
/// use hako::fsops::trash::Trash;
/// fn early(aside: SetAside, trash: &Trash) {
///     let _ = aside.dispose(trash);
/// }
/// ```
///
/// ```
/// use hako::fsops::place::InPlace;
/// use hako::fsops::replace::SetAside;
/// use hako::fsops::trash::Trash;
/// fn after(aside: SetAside, placed: InPlace, trash: &Trash) {
///     match aside.replaced(placed) {
///         Ok(replaced) => drop(replaced.dispose(trash)),
///         Err(aside) => drop(aside.put_back()),
///     }
/// }
/// ```
#[derive(Debug)]
#[must_use = "a set-aside item is put back, or replaced"]
pub struct SetAside {
    item: Unsettled<Aside, fn(Aside)>,
}

impl SetAside {
    /// Its replacement is in place (`placed`): it can go. A placement at
    /// another target doesn't do: the item comes back, to be put back.
    pub fn replaced(self, placed: InPlace) -> Result<Replaced, SetAside> {
        if placed.target() != self.item.get().1 {
            return Err(self);
        }
        Ok(Replaced { item: Unsettled::new(self.item.settle(), keep_quietly) })
    }

    /// Its replacement didn't come: back to its name.
    pub fn put_back(self) -> Result<(), Stranded> {
        let (item, target) = self.item.settle();
        let at = item.into_target().path;
        match sys::rename_noreplace(&at, &target) {
            Ok(()) => Ok(()),
            Err(e) => Err(Stranded { at, target, why: describe(&e) }),
        }
    }
}

/// An item its replacement has taken the place of, under its hidden name.
/// It goes to the Trash. Nothing can delete it instead, or choose for the
/// operation that it should be deleted:
///
/// ```compile_fail,E0308
/// use hako::fsops::replace::Replaced;
/// // Put Back did this: no Trash, so the item it replaced was deleted.
/// fn put_back_replacing(replaced: Replaced) {
///     let _ = replaced.dispose(None);
/// }
/// ```
///
/// ```compile_fail,E0599
/// use hako::fsops::replace::Replaced;
/// fn put_back_replacing(replaced: Replaced) {
///     let _ = replaced.delete_for_good();
/// }
/// ```
///
/// ```
/// use hako::fsops::replace::Replaced;
/// use hako::fsops::trash::Trash;
/// fn put_back_replacing(replaced: Replaced, trash: &Trash) {
///     let _ = replaced.dispose(trash);
/// }
/// ```
#[derive(Debug)]
#[must_use = "a replaced item goes to the Trash"]
pub struct Replaced {
    item: Unsettled<Aside, fn(Aside)>,
}

impl Replaced {
    /// To the Trash, as the item at its target. One the Trash refuses is
    /// kept beside its replacement under a visible name.
    pub fn dispose(self, trash: &Trash) -> Result<(), Kept> {
        let (item, target) = self.item.settle();
        let found = item.into_target();
        match trash.trash_as(&found.path, &target) {
            Ok(_) => Ok(()),
            Err(e) => {
                let at = keep(&found, &target);
                Err(Kept { at, target, why: format!("it couldn't go to the Trash: {}", describe(&e)) })
            }
        }
    }
}

/// A replaced item the Trash refused, kept beside its replacement. It can
/// only be reported: nothing deletes it instead.
///
/// ```compile_fail,E0599
/// use hako::fsops::replace::Replaced;
/// use hako::fsops::trash::Trash;
/// // What `dispose` did: if it can't go to the Trash, delete it.
/// fn dispose(replaced: Replaced, trash: &Trash) {
///     if let Err(kept) = replaced.dispose(trash) {
///         kept.delete_for_good();
///     }
/// }
/// ```
///
/// ```
/// use hako::fsops::replace::Replaced;
/// use hako::fsops::trash::Trash;
/// fn dispose(replaced: Replaced, trash: &Trash) -> Option<(std::path::PathBuf, String)> {
///     replaced.dispose(trash).err().map(|kept| kept.problem())
/// }
/// ```
#[derive(Debug)]
pub struct Kept {
    at: PathBuf,
    target: PathBuf,
    why: String,
}

impl Kept {
    /// The report's problem: the item's target, and where the old one is.
    pub fn problem(self) -> (PathBuf, String) {
        let beside = self.at.parent() == self.target.parent();
        let name = match self.at.file_name() {
            Some(n) if beside && !names::is_temporary(n) => names::display(n),
            Some(_) | None => self.at.display().to_string(),
        };
        (self.target, format!("the item it replaced is kept beside it as “{name}”: {}", self.why))
    }
}

/// A set-aside item that couldn't be put back, left under its hidden name.
#[derive(Debug)]
pub struct Stranded {
    at: PathBuf,
    target: PathBuf,
    why: String,
}

impl Stranded {
    /// The report's problem: the item's target, and where the old one is.
    pub fn problem(self) -> (PathBuf, String) {
        let msg = format!("the item it was to replace is kept as “{}” ({})", self.at.display(), self.why);
        (self.target, msg)
    }
}
