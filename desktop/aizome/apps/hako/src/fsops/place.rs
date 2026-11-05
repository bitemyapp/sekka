//! Putting an item where nothing is, and the proof that it's there.
//!
//! An item reaches its target by a rename that refuses to overwrite: the
//! item itself, within a filesystem, or its partial copy, across them.
//! Only those renames, here, make the [`InPlace`] proof that the steps
//! after a placement take: a replaced item goes only once its replacement
//! is in place ([`Replaced`](super::replace::Replaced)).

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
