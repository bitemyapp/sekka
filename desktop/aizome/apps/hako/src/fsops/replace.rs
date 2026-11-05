//! Replacing an item, in stages that only the stage before makes:
//!
//! 1. [`Occupant`]: the item at the target, as the person saw it when they
//!    chose Replace (its device and inode).
//! 2. [`SetAside`]: it's renamed to a hidden name beside the target
//!    (`.name.hako-aside-…`) and checked to be the item the person saw.
//!    It's put back if its replacement doesn't come
//!    ([`SetAside::put_back`]), and becomes [`Replaced`] only with the
//!    proof that its replacement is in place ([`InPlace`]).
//! 3. [`Replaced`]: it goes as the operation decided ([`Displaced`]): to
//!    the Trash, or deleted for good. Nothing else disposes of it, so
//!    whether it survives can't depend on whether some caller happened to
//!    pass a Trash. One that can't go to the Trash is [`Kept`] under its
//!    hidden name and reported, never deleted instead.

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use aizome_typestate::{Confirmed, Reviewed, SameTarget};

use super::place::InPlace;
use super::trash::Trash;
use super::{delete, describe, names, sys};

/// What becomes of an item a replacement displaces. Each operation
/// decides it where it starts, and the replaced item can't be disposed of
/// without it:
///
/// ```compile_fail,E0308
/// use hako::fsops::replace::Replaced;
/// // Put Back did this: no Trash, so the item it replaced was deleted.
/// fn put_back_replacing(replaced: Replaced) {
///     let _ = replaced.dispose(None);
/// }
/// ```
///
/// ```
/// use hako::fsops::replace::{Displaced, Replaced};
/// use hako::fsops::trash::Trash;
/// fn put_back_replacing(replaced: Replaced, trash: &Trash) {
///     let _ = replaced.dispose(Displaced::ToTrash(trash));
/// }
/// ```
#[derive(Clone, Copy, Debug)]
pub enum Displaced<'t> {
    /// To the Trash, as the item it was: what the conflict sheet promises
    /// ("Replacing moves the one there to the Trash"). Every operation of
    /// Hako's, Put Back and undo included, decides this.
    ToTrash(&'t Trash),
    /// Deleted for good once its replacement is in place, for tools and
    /// tests with no Trash.
    DeleteForGood,
}

/// An item, by what stays with it (its device and inode), and where it
/// was found.
#[derive(Clone, Debug)]
pub struct Found {
    path: PathBuf,
    dev: u64,
    ino: u64,
}

impl Found {
    fn at(path: &Path, meta: &fs::Metadata) -> Found {
        Found { path: path.to_path_buf(), dev: meta.dev(), ino: meta.ino() }
    }
}

impl SameTarget for Found {
    fn same_target(&self, fresh: &Self) -> bool {
        self.dev == fresh.dev && self.ino == fresh.ino
    }
}

/// The brand of a replacement's review: the person was shown the item
/// at the target in the conflict question and chose Replace. Only
/// [`Occupant::seen`], where the conflict is read, makes one, so nothing
/// else can claim the person saw an item:
///
/// ```compile_fail,E0423
/// use hako::fsops::replace::Asked;
/// let _ = Asked(());
/// ```
#[derive(Debug)]
pub struct Asked(());

/// The item at a replacement's target, as the person saw it when they
/// chose Replace.
#[derive(Debug)]
pub struct Occupant {
    seen: Reviewed<Found, Asked>,
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
    /// What's at `target` (`meta`, not followed), as the conflict showed it.
    pub(super) fn seen(target: &Path, meta: &fs::Metadata) -> Occupant {
        Occupant { seen: Reviewed::new(Found::at(target, meta), Asked(())) }
    }

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
            Ok(item) => Ok(SetAside { item, target }),
            Err(_) => {
                let back = sys::rename_noreplace(&aside, &target);
                Err(NotSetAside::Changed(back.err().map(|e| Stranded { at: aside, target, why: describe(&e) })))
            }
        }
    }
}

/// A replaced item under its hidden name, the one the person saw. Its
/// replacement is on the way; it can't go before that's in place:
///
/// ```compile_fail,E0599
/// use hako::fsops::replace::{Displaced, SetAside};
/// use hako::fsops::trash::Trash;
/// fn early(aside: SetAside, trash: &Trash) {
///     let _ = aside.dispose(Displaced::ToTrash(trash));
/// }
/// ```
///
/// ```
/// use hako::fsops::place::InPlace;
/// use hako::fsops::replace::{Displaced, SetAside};
/// use hako::fsops::trash::Trash;
/// fn after(aside: SetAside, placed: InPlace, trash: &Trash) {
///     let _ = aside.replaced(placed).dispose(Displaced::ToTrash(trash));
/// }
/// ```
#[derive(Debug)]
#[must_use = "a set-aside item is put back, or replaced"]
pub struct SetAside {
    item: Confirmed<Found, Asked>,
    target: PathBuf,
}

impl SetAside {
    /// Its replacement is in place at its target (`placed`): it can go.
    pub fn replaced(self, placed: InPlace) -> Replaced {
        debug_assert_eq!(placed.target(), self.target, "the replacement is in this item's place");
        Replaced { item: self.item, target: self.target }
    }

    /// Its replacement didn't come: back to its name.
    pub fn put_back(self) -> Result<(), Stranded> {
        let at = self.item.into_target().path;
        match sys::rename_noreplace(&at, &self.target) {
            Ok(()) => Ok(()),
            Err(e) => Err(Stranded { at, target: self.target, why: describe(&e) }),
        }
    }
}

/// An item its replacement has taken the place of, under its hidden name.
/// It goes only as the [`Displaced`] decision says. Deleting it any other
/// way doesn't compile:
///
/// ```compile_fail,E0624
/// use hako::fsops::replace::Replaced;
/// fn dispose(replaced: Replaced) {
///     let _ = replaced.delete_for_good();
/// }
/// ```
///
/// ```
/// use hako::fsops::replace::{Displaced, Replaced};
/// fn dispose(replaced: Replaced) {
///     let _ = replaced.dispose(Displaced::DeleteForGood);
/// }
/// ```
#[derive(Debug)]
#[must_use = "a replaced item goes as the operation decided"]
pub struct Replaced {
    item: Confirmed<Found, Asked>,
    target: PathBuf,
}

impl Replaced {
    /// Dispose of it as `how` says.
    pub fn dispose(self, how: Displaced) -> Result<(), Kept> {
        match how {
            Displaced::ToTrash(trash) => self.into_trash(trash),
            Displaced::DeleteForGood => self.delete_for_good(),
        }
    }

    /// To the Trash, as the item at its target. One the Trash refuses is
    /// kept: the decision was the Trash, not deletion.
    fn into_trash(self, trash: &Trash) -> Result<(), Kept> {
        let at = self.item.into_target().path;
        match trash.trash_as(&at, &self.target) {
            Ok(_) => Ok(()),
            Err(e) => {
                Err(Kept { at, target: self.target, why: format!("it couldn't go to the Trash: {}", describe(&e)) })
            }
        }
    }

    fn delete_for_good(self) -> Result<(), Kept> {
        let at = self.item.into_target().path;
        let why = match delete::remove_quietly(&at).failed.first() {
            None => return Ok(()),
            Some((p, why)) => format!("it couldn't all be removed ({}: {why})", p.display()),
        };
        Err(Kept { at, target: self.target, why })
    }
}

/// A replaced item that couldn't go as decided, kept under its hidden name
/// beside its replacement. It can only be reported: when the Trash
/// refuses it, nothing deletes it instead.
///
/// ```compile_fail,E0599
/// use hako::fsops::replace::{Displaced, Replaced};
/// use hako::fsops::trash::Trash;
/// // What `dispose` did: if it can't go to the Trash, delete it.
/// fn dispose(replaced: Replaced, trash: &Trash) {
///     if let Err(kept) = replaced.dispose(Displaced::ToTrash(trash)) {
///         kept.dispose(Displaced::DeleteForGood);
///     }
/// }
/// ```
///
/// ```
/// use hako::fsops::replace::{Displaced, Replaced};
/// use hako::fsops::trash::Trash;
/// fn dispose(replaced: Replaced, trash: &Trash) -> Option<(std::path::PathBuf, String)> {
///     replaced.dispose(Displaced::ToTrash(trash)).err().map(|kept| kept.problem())
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
        let msg = format!("the item it replaced is kept as “{}”: {}", self.at.display(), self.why);
        (self.target, msg)
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
