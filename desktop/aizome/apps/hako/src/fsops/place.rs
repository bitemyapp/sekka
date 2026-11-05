//! Putting an item where nothing is, and the proof that it's there.
//!
//! An item reaches its target by a rename that refuses to overwrite: the
//! item itself, within a filesystem, or its partial copy, across them.
//! Only those renames, here, make the [`InPlace`] proof that the steps
//! after a placement take: a replaced item goes only once its replacement
//! is in place ([`Replaced`](super::replace::Replaced)).
//!
//! A move across filesystems deletes a source only once its copy is
//! committed: the file's data synced ([`Copied`]), then each folder it's
//! in, from the inside out ([`Unsynced`]), then the folder the whole copy
//! was renamed into ([`Committed`]). Each stage is a type that only the
//! sync before it makes, so a folder that couldn't be synced keeps the
//! sources of everything in it.

use std::fs;
use std::io;
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
        r => r,
    }
}

/// Sources of a move across filesystems whose copies are made and on
/// disk, all but their names in the folder that holds them: a file's data
/// synced, a link made, a folder synced with all it holds. Only syncing
/// that folder ([`Unsynced::sync`]), and at the top the folder the copy
/// was renamed into ([`Copied::commit`]), takes them further. A folder
/// that couldn't be synced takes its sources with it, so they stay.
///
/// Sources can't be deleted from here:
///
/// ```compile_fail,E0599
/// use hako::fsops::place::Copied;
/// fn delete_early(copied: Copied) {
///     let _ = copied.delete_sources();
/// }
/// ```
///
/// ```
/// use hako::fsops::place::{Copied, InPlace};
/// fn delete_once_committed(copied: Copied, placed: &InPlace) -> std::io::Result<()> {
///     let _problems = copied.commit(placed)?.delete_sources();
///     Ok(())
/// }
/// ```
#[derive(Debug, Default)]
#[must_use = "a copy's sources go only once it's committed"]
pub struct Copied {
    /// Files and links.
    leaves: Vec<PathBuf>,
    /// Folders, each after what it held: removed once they're empty.
    dirs: Vec<PathBuf>,
}

impl Copied {
    /// `source`, a file copied to `copy`: its data synced.
    pub(super) fn file(source: &Path, copy: &fs::File) -> io::Result<Copied> {
        copy.sync_all()?;
        Ok(Copied { leaves: vec![source.to_path_buf()], dirs: Vec::new() })
    }

    /// `source`, a link copied: it's all in its name.
    pub(super) fn link(source: &Path) -> Copied {
        Copied { leaves: vec![source.to_path_buf()], dirs: Vec::new() }
    }

    /// These copies are in the item just put in place (`placed`): sync the
    /// folder it was renamed into, and the move is committed.
    pub fn commit(self, placed: &InPlace) -> io::Result<Committed> {
        sys::sync_dir(placed.target().parent().unwrap_or(Path::new("/")))?;
        Ok(Committed(self))
    }
}

/// A folder of a move being copied, and the sources of what's been copied
/// into it. Their copies' names in it are on disk only once it's synced,
/// so a nested folder's sources go up to the folder above only through
/// [`sync`](Self::sync):
///
/// ```compile_fail,E0308
/// use hako::fsops::place::Unsynced;
/// // What the copy did: the inner folder's files were the move's to
/// // delete before the inner folder was synced.
/// fn copied_folder(parent: &mut Unsynced, inner: Unsynced) {
///     parent.add(inner);
/// }
/// ```
///
/// ```
/// use hako::fsops::place::Unsynced;
/// fn copied_folder(parent: &mut Unsynced, inner: Unsynced) -> std::io::Result<()> {
///     parent.add(inner.sync()?);
///     Ok(())
/// }
/// ```
#[derive(Debug)]
#[must_use = "a folder's sources go up only once it's synced"]
pub struct Unsynced {
    source: PathBuf,
    copy: PathBuf,
    inside: Copied,
}

impl Unsynced {
    /// `source`, a folder, being copied to `copy` (made, and empty).
    pub(super) fn new(source: &Path, copy: &Path) -> Unsynced {
        Unsynced { source: source.to_path_buf(), copy: copy.to_path_buf(), inside: Copied::default() }
    }

    /// Something copied into it.
    pub fn add(&mut self, copied: Copied) {
        self.inside.leaves.extend(copied.leaves);
        self.inside.dirs.extend(copied.dirs);
    }

    /// Sync the folder's copy: then what's in it, and the folder, are on
    /// disk but for the folder's own name. On an error the sources stay.
    pub fn sync(self) -> io::Result<Copied> {
        sys::sync_dir(&self.copy)?;
        let mut copied = self.inside;
        copied.dirs.push(self.source);
        Ok(copied)
    }
}

/// Proof that a move's copies are in place and on disk. Only
/// [`Copied::commit`] makes one, and only it deletes sources:
///
/// ```compile_fail,E0423
/// use hako::fsops::place::{Committed, Copied};
/// fn forge(copied: Copied) -> Committed {
///     Committed(copied)
/// }
/// ```
#[derive(Debug)]
#[must_use = "a committed move deletes its sources"]
pub struct Committed(Copied);

impl Committed {
    /// Delete the sources: files and links, then the folders left empty
    /// (those with something skipped in them stay). What couldn't be
    /// deleted comes back as problems for the report.
    pub fn delete_sources(self) -> Vec<(PathBuf, String)> {
        let Committed(copied) = self;
        let mut problems = Vec::new();
        for leaf in copied.leaves {
            if let Err(e) = fs::remove_file(&leaf)
                && e.kind() != io::ErrorKind::NotFound
            {
                let why = format!("arrived, but the original couldn't be removed: {}", super::describe(&e));
                problems.push((leaf, why));
            }
        }
        for dir in copied.dirs {
            let _ = fs::remove_dir(dir);
        }
        problems
    }
}
