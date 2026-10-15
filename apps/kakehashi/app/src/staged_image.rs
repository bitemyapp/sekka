//! macOS administrator authorization does not grant the helper the GUI's
//! access to Downloads, Documents or removable volumes. Prepare an independent
//! image in the job's private temporary directory, as the user, before elevation.

use anyhow::{Context, Result, bail, ensure};
use kakehashi_core::image::{Cancelled, sha256_file};
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub fn prepare(source: &Path, sha: &str, dir: &Path, cancelled: &dyn Fn() -> bool) -> Result<PathBuf> {
    let dest = dir.join("image.iso");
    ensure!(!dest.try_exists()?, "the temporary image already exists");
    let result = (|| {
        if cancelled() {
            bail!(Cancelled);
        }
        ensure!(fs::metadata(source)?.is_file(), "the image is not a regular file");
        if !clone_image(source, &dest)? {
            copy_image(source, &dest, cancelled)?;
        }
        // Cloning also copies permissions. Restrict the staged image to its owner.
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o600))?;
        let actual = sha256_file(&dest, &mut |_| {}, cancelled)?;
        ensure!(actual == sha, "the image changed since it was selected; select it again before writing");
        Ok(dest.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&dest);
    }
    result.with_context(|| format!("could not prepare {} for writing", source.display()))
}

/// APFS clones have independent contents without allocating another ISO's worth
/// of storage. Unlike a hard link, later edits to the original cannot change it.
fn clone_image(source: &Path, dest: &Path) -> Result<bool> {
    let source = CString::new(source.as_os_str().as_bytes())?;
    let dest = CString::new(dest.as_os_str().as_bytes())?;
    // SAFETY: both paths are NUL-terminated and live through the call.
    if unsafe { libc::clonefile(source.as_ptr(), dest.as_ptr(), 0) } == 0 {
        return Ok(true);
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        Some(libc::EXDEV | libc::ENOTSUP | libc::ENOSYS) => Ok(false),
        _ => Err(err.into()),
    }
}

/// Images on another filesystem need a real copy. Bound each read so Stop
/// works during preparation, before any administrator prompt or drive access.
fn copy_image(source: &Path, dest: &Path, cancelled: &dyn Fn() -> bool) -> Result<()> {
    let mut input = File::open(source)?;
    let mut output = OpenOptions::new().write(true).create_new(true).mode(0o600).open(dest)?;
    let mut buffer = vec![0; 8 * 1024 * 1024];
    loop {
        if cancelled() {
            bail!(Cancelled);
        }
        let n = input.read(&mut buffer)?;
        if n == 0 {
            return Ok(());
        }
        output.write_all(&buffer[..n])?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::os::unix::fs::MetadataExt;

    const SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn cancelled_preparation_never_opens_the_source() {
        let job = tempfile::tempdir().unwrap();
        let err = prepare(&job.path().join("missing.iso"), SHA, job.path(), &|| true).unwrap_err();
        assert!(err.is::<Cancelled>());
        assert_eq!(fs::read_dir(job.path()).unwrap().count(), 0);
    }

    #[test]
    fn preparation_never_overwrites_an_existing_file() {
        let job = tempfile::tempdir().unwrap();
        let dest = job.path().join("image.iso");
        fs::write(&dest, b"keep").unwrap();
        assert!(prepare(&job.path().join("missing.iso"), SHA, job.path(), &|| false).is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"keep");
    }

    #[test]
    fn staged_image_survives_original_changes_and_is_private() {
        let source_dir = tempfile::tempdir().unwrap();
        let source = source_dir.path().join("selected.iso");
        fs::write(&source, b"abc").unwrap();
        let job = tempfile::tempdir().unwrap();
        let staged = prepare(&source, SHA, job.path(), &|| false).unwrap();
        assert_ne!(fs::metadata(&source).unwrap().ino(), fs::metadata(&staged).unwrap().ino());
        assert_eq!(fs::metadata(&staged).unwrap().permissions().mode() & 0o777, 0o600);
        fs::write(&source, b"changed").unwrap();
        drop(source_dir);
        assert_eq!(fs::read(&staged).unwrap(), b"abc");
        drop(job);
        assert!(!staged.exists());
    }

    #[test]
    fn changed_image_is_rejected_and_removed_before_elevation() {
        let source_dir = tempfile::tempdir().unwrap();
        let source = source_dir.path().join("selected.iso");
        fs::write(&source, b"changed").unwrap();
        let job = tempfile::tempdir().unwrap();
        let err = prepare(&source, SHA, job.path(), &|| false).unwrap_err();
        assert!(format!("{err:#}").contains("image changed"));
        assert_eq!(fs::read_dir(job.path()).unwrap().count(), 0);
    }

    #[test]
    fn cancellation_during_verification_removes_the_copy() {
        let source_dir = tempfile::tempdir().unwrap();
        let source = source_dir.path().join("selected.iso");
        fs::write(&source, b"abc").unwrap();
        let job = tempfile::tempdir().unwrap();
        let err = prepare(&source, SHA, job.path(), &|| job.path().join("image.iso").exists()).unwrap_err();
        assert!(err.is::<Cancelled>());
        assert_eq!(fs::read_dir(job.path()).unwrap().count(), 0);
    }

    #[test]
    fn fallback_copies_bytes_and_can_be_cancelled_between_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let dest = dir.path().join("copy");
        fs::write(&source, vec![7; 8 * 1024 * 1024 + 1]).unwrap();
        let calls = Cell::new(0);
        let err = copy_image(&source, &dest, &|| {
            calls.set(calls.get() + 1);
            calls.get() == 2
        })
        .unwrap_err();
        assert!(err.is::<Cancelled>());
        assert_eq!(fs::metadata(&dest).unwrap().len(), 8 * 1024 * 1024);
        fs::remove_file(&dest).unwrap();
        copy_image(&source, &dest, &|| false).unwrap();
        assert_eq!(fs::read(&source).unwrap(), fs::read(&dest).unwrap());
    }
}
