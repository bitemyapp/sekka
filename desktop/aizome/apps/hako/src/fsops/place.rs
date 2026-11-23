//! Putting an item where nothing is, and the proof that it's there.
//!
//! An item reaches its target by a rename that refuses to overwrite: the
//! item itself, within a filesystem, or its partial copy, across them.
//! Only those renames, here, make the [`InPlace`] proof that the steps
//! after a placement take: a replaced item goes only once its replacement
//! is in place ([`Replaced`](super::replace::Replaced)).
//!
//! A move across filesystems deletes a source only once its copy is
//! committed: a file's data synced, then the folder its copy is in, then
//! each folder above, from the inside out, then the folder the whole copy
//! was renamed into. An [`Item`] is handed out only by the [`Folder`] its
//! copy is made in (or, for the top item, by its [`Move`]), and records its
//! source there; a folder's sources go on to the folder above only through
//! its own [`sync`](Folder::sync), and a move's sources can be deleted only
//! through its [`Committed`] proof. So a folder that couldn't be synced
//! keeps the sources of everything in it.

// Typestate code: every state and every case is named, so a new one
// added later can't fall into an arm written for others.
#![deny(clippy::wildcard_enum_match_arm)]

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::sys;

/// Proof that an item was just put at its target by a rename that
/// refused to overwrite. Only this module's renames make one:
///
/// ```compile_fail,E0451
/// use hako::fsops::place::InPlace;
/// let _ = InPlace { target: "/home/me/a.txt".into() };
/// ```
#[derive(Debug)]
#[must_use = "a placement's proof is for the step after it"]
pub struct InPlace {
    target: PathBuf,
}

impl InPlace {
    /// Where the item is.
    pub fn target(&self) -> &Path {
        &self.target
    }
}

/// Rename `from` to `target`, refusing to overwrite.
pub(super) fn rename(from: &Path, target: &Path) -> io::Result<InPlace> {
    sys::rename_noreplace(from, target)?;
    Ok(InPlace { target: target.to_path_buf() })
}

/// [`rename`] `source` (with `smeta`, not followed), and if it's a
/// read-only folder of ours, which a rename to another folder needs to
/// write to, with write permission for the moment.
pub(super) fn rename_item(source: &Path, target: &Path, smeta: &fs::Metadata) -> io::Result<InPlace> {
    match rename(source, target) {
        Err(e)
            if e.kind() == io::ErrorKind::PermissionDenied
                && smeta.is_dir()
                && smeta.uid() == sys::uid()
                && smeta.mode() & 0o200 == 0 =>
        {
            let mode = smeta.mode() & 0o7777;
            fs::set_permissions(source, fs::Permissions::from_mode(mode | 0o200))?;
            let r = rename(source, target);
            let now = if r.is_ok() { target } else { source };
            let _ = fs::set_permissions(now, fs::Permissions::from_mode(mode));
            r
        }
        r @ (Ok(_) | Err(_)) => r,
    }
}

/// Sources whose copies are made and on disk but for their names in a
/// folder not synced yet: files and links, then folders, each after what
/// it held.
#[derive(Debug, Default)]
struct Sources {
    leaves: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
}

/// Whether `name` names one item in a folder (not a path into another).
fn is_one_name(name: &OsStr) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.as_bytes().contains(&b'/')
}

/// One item to copy: its source, and where its copy is made. In a move it
/// also holds the sources of the folder its copy is made in, where its own
/// goes once its copy is on disk.
#[derive(Debug)]
#[must_use = "an item is copied, or left out"]
pub struct Item<'f> {
    source: PathBuf,
    copy: PathBuf,
    into: Option<&'f mut Sources>,
}

impl Item<'static> {
    /// `source`, to copy to `copy`, in a copy (no source goes).
    pub(super) fn copying(source: &Path, copy: &Path) -> Item<'static> {
        Item { source: source.to_path_buf(), copy: copy.to_path_buf(), into: None }
    }
}

impl<'f> Item<'f> {
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Where its copy is made.
    pub fn copy(&self) -> &Path {
        &self.copy
    }

    /// It was a file, copied to [`copy`](Self::copy) as `file`. In a move,
    /// its data is synced and its source recorded in its folder; if the
    /// sync fails, or `file` isn't what's at its copy, the item comes back.
    pub fn file(self, file: &fs::File) -> Result<(), (Item<'f>, io::Error)> {
        if self.into.is_some() {
            let made = |m: &fs::Metadata| (m.dev(), m.ino());
            let is_copy = match (file.metadata(), fs::symlink_metadata(&self.copy)) {
                (Ok(a), Ok(b)) => made(&a) == made(&b),
                (Err(_), _) | (_, Err(_)) => false,
            };
            if !is_copy {
                return Err((self, io::Error::new(io::ErrorKind::InvalidInput, "it isn't the copy made")));
            }
            if let Err(e) = file.sync_all() {
                return Err((self, e));
            }
        }
        if let Some(into) = self.into {
            into.leaves.push(self.source);
        }
        Ok(())
    }

    /// It was a link, made at [`copy`](Self::copy): all of it is its name.
    /// In a move, its source is recorded only if a link is there.
    pub fn link(self) -> io::Result<()> {
        if let Some(into) = self.into {
            if !fs::symlink_metadata(&self.copy)?.file_type().is_symlink() {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "no link was made"));
            }
            into.leaves.push(self.source);
        }
        Ok(())
    }

    /// It's a folder, made at [`copy`](Self::copy): what's copied into it
    /// is recorded in it, until it's synced.
    pub fn folder(self) -> Folder<'f> {
        let inside = self.into.is_some().then(Sources::default);
        Folder { source: self.source, copy: self.copy, inside, into: self.into }
    }
}

/// A folder being copied, and the sources of what's been copied into it.
/// Its items' sources go on to the folder it's in only once it's synced,
/// and while it's being filled, that folder can't take anything else: one
/// list shared by the whole copy, as the old copy kept, doesn't compile.
///
/// ```compile_fail,E0499
/// use hako::fsops::place::Folder;
/// // The top folder takes a file of the inner one while the inner one is
/// // being copied: its source would skip the inner folder's sync.
/// fn one_list(top: &mut Folder<'_>) {
///     let mut inner = top.child("inner".as_ref()).folder();
///     top.child("x.txt".as_ref()).link();
///     inner.child("y.txt".as_ref()).link();
///     let _ = inner.sync();
/// }
/// ```
///
/// ```
/// use hako::fsops::place::Folder;
/// fn in_turn(top: &mut Folder<'_>) -> std::io::Result<()> {
///     let mut inner = top.child("inner".as_ref()).folder();
///     inner.child("y.txt".as_ref()).link()?;
///     inner.sync()?;
///     top.child("x.txt".as_ref()).link()?;
///     Ok(())
/// }
/// ```
#[derive(Debug)]
#[must_use = "a folder's sources go on only once it's synced"]
pub struct Folder<'f> {
    source: PathBuf,
    copy: PathBuf,
    inside: Option<Sources>,
    into: Option<&'f mut Sources>,
}

impl<'f> Folder<'f> {
    /// The item `name` in it: its copy is made in this folder. A name, not
    /// a path into another folder (that panics).
    pub fn child(&mut self, name: &OsStr) -> Item<'_> {
        assert!(is_one_name(name), "a folder's child is named, not a path: {name:?}");
        Item { source: self.source.join(name), copy: self.copy.join(name), into: self.inside.as_mut() }
    }

    /// Where its copy is.
    pub fn copy(&self) -> &Path {
        &self.copy
    }

    /// It's full. In a move, its copy is synced, and then its sources, and
    /// it, go on to the folder it's in. On an error they stay, with the
    /// sources of all it holds.
    pub fn sync(self) -> io::Result<()> {
        if let (Some(mut inside), Some(into)) = (self.inside, self.into) {
            sys::sync_dir(&self.copy)?;
            inside.dirs.push(self.source);
            into.leaves.extend(inside.leaves);
            into.dirs.extend(inside.dirs);
        }
        Ok(())
    }
}

/// A move across filesystems: its item copied under a partial name beside
/// its target, and the sources of what came. They're deleted only once
/// it's committed:
///
/// ```compile_fail,E0599
/// use hako::fsops::place::Move;
/// fn delete_early(copied: Move) {
///     let _ = copied.delete_sources();
/// }
/// ```
///
/// ```
/// use hako::fsops::place::{InPlace, Move};
/// fn delete_once_committed(copied: Move, placed: &InPlace) -> std::io::Result<()> {
///     let _problems = copied.commit(placed)?.delete_sources();
///     Ok(())
/// }
/// ```
#[derive(Debug)]
#[must_use = "a move's sources go only once it's committed"]
pub struct Move {
    source: PathBuf,
    partial: PathBuf,
    came: Sources,
}

impl Move {
    /// `source`, to copy to `partial`, beside its target.
    pub(super) fn new(source: &Path, partial: &Path) -> Move {
        Move { source: source.to_path_buf(), partial: partial.to_path_buf(), came: Sources::default() }
    }

    /// Its item, to copy.
    pub fn item(&mut self) -> Item<'_> {
        Item { source: self.source.clone(), copy: self.partial.clone(), into: Some(&mut self.came) }
    }

    /// The copy is in place (`placed`: the partial renamed to its target):
    /// sync the folder it's in, and the move is committed. Another
    /// folder's placement doesn't do.
    pub fn commit(self, placed: &InPlace) -> io::Result<Committed> {
        let dir = placed.target().parent();
        if dir.is_none() || dir != self.partial.parent() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "it isn't where the copy was made"));
        }
        // A move inside another copy (in its partial) isn't in place till
        // that one is: its sources can't go yet.
        if placed.target().ancestors().skip(1).any(|a| a.file_name().is_some_and(super::names::is_temporary)) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "it's inside a copy not in place yet"));
        }
        sys::sync_dir(dir.unwrap_or(Path::new("/")))?;
        Ok(Committed(self.came))
    }
}

/// Proof that a move's copies are in place and on disk. Only
/// [`Move::commit`] makes one, and only it deletes sources:
///
/// ```compile_fail,E0423
/// use hako::fsops::place::Committed;
/// fn forge() -> Committed {
///     Committed(Default::default())
/// }
/// ```
#[derive(Debug)]
#[must_use = "a committed move deletes its sources"]
pub struct Committed(Sources);

impl Committed {
    /// Delete the sources: files and links, then the folders left empty
    /// (those with something skipped in them stay). What couldn't be
    /// deleted comes back as problems for the report.
    pub fn delete_sources(self) -> Vec<(PathBuf, String)> {
        let Committed(sources) = self;
        let mut problems = Vec::new();
        for leaf in sources.leaves {
            if let Err(e) = fs::remove_file(&leaf)
                && e.kind() != io::ErrorKind::NotFound
            {
                let why = format!("arrived, but the original couldn't be removed: {}", super::describe(&e));
                problems.push((leaf, why));
            }
        }
        for dir in sources.dirs {
            let _ = fs::remove_dir(dir);
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsops::testing::TempDir;

    #[test]
    fn an_item_records_only_the_copy_made_at_its_place() {
        let t = TempDir::new("place-item");
        let src = t.file("src/a.txt", "a");
        let partial = t.dir("dest").join(".a.txt.hako-part-x");
        let mut moving = Move::new(&src, &partial);
        assert!(moving.item().link().is_err(), "no link was made there");
        fs::write(&partial, "a").unwrap();
        let elsewhere = fs::File::open(&src).unwrap();
        assert!(moving.item().file(&elsewhere).is_err(), "the source's handle isn't its copy");
        assert!(moving.came.leaves.is_empty());
        let copy = fs::File::open(&partial).unwrap();
        assert!(moving.item().file(&copy).is_ok());
        assert_eq!(moving.came.leaves, vec![src]);
    }

    #[test]
    fn a_move_inside_another_copy_doesn_t_commit() {
        let t = TempDir::new("place-nested");
        let outer = t.dir("dest/.f.hako-part-x");
        let src = t.file("src/a.txt", "a");
        let partial = outer.join(".a.txt.hako-part-y");
        fs::write(&partial, "a").unwrap();
        let moving = Move::new(&src, &partial);
        let placed = rename(&partial, &outer.join("a.txt")).unwrap();
        assert!(moving.commit(&placed).is_err(), "the outer copy isn't in place");
    }
}
