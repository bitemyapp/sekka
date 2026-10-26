//! The system calls the operations need beyond `std::fs`: renames that
//! refuse to overwrite, in-kernel copies, syncing folders.

use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::io::AsRawFd;
use std::path::Path;

fn cstr(p: &Path) -> io::Result<CString> {
    CString::new(p.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a path with a NUL byte"))
}

/// Rename `from` to `to`, failing with `AlreadyExists` if `to` is there
/// (`renameat2(RENAME_NOREPLACE)`). Filesystems without it (some network
/// ones) get a check and a plain rename, which a race could still
/// overwrite; it's the best they allow.
pub fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    let (f, t) = (cstr(from)?, cstr(to)?);
    // SAFETY: both are valid NUL-terminated paths; AT_FDCWD resolves them
    // as `rename` would.
    let r = unsafe { libc::renameat2(libc::AT_FDCWD, f.as_ptr(), libc::AT_FDCWD, t.as_ptr(), libc::RENAME_NOREPLACE) };
    if r == 0 {
        return Ok(());
    }
    let e = io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::EINVAL) | Some(libc::ENOSYS) | Some(libc::EOPNOTSUPP) => {
            if std::fs::symlink_metadata(to).is_ok() {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, "the name is taken"));
            }
            std::fs::rename(from, to)
        }
        _ => Err(e),
    }
}

/// Whether an error is a rename across filesystems.
pub fn is_cross_device(e: &io::Error) -> bool {
    e.raw_os_error() == Some(libc::EXDEV)
}

/// The device a path is on (not following a link at the end).
pub fn device(path: &Path) -> io::Result<u64> {
    Ok(std::fs::symlink_metadata(path)?.dev())
}

/// Flush a folder's entries (names made, renamed, removed) to its disk.
pub fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

/// The size of the chunks copied between checks for pause and cancel.
pub const CHUNK: usize = 4 << 20;

/// Copy what's left of `from` into `to` in chunks, in the kernel where
/// it can (`copy_file_range`, which reflinks on filesystems that share
/// blocks), else through a buffer. `each` hears the bytes of each chunk
/// and may stop the copy by returning an error.
pub fn copy_contents(from: &mut File, to: &mut File, each: &mut dyn FnMut(u64) -> io::Result<()>) -> io::Result<u64> {
    let mut total = 0u64;
    let mut kernel = true;
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let n = if kernel {
            // SAFETY: both descriptors are open files of ours; null offsets
            // use and advance their own.
            let r = unsafe {
                libc::copy_file_range(
                    from.as_raw_fd(),
                    std::ptr::null_mut(),
                    to.as_raw_fd(),
                    std::ptr::null_mut(),
                    CHUNK,
                    0,
                )
            };
            if r < 0 {
                let e = io::Error::last_os_error();
                match e.raw_os_error() {
                    // Not between these files: copy through a buffer from
                    // where it got to.
                    Some(libc::EXDEV | libc::EINVAL | libc::ENOSYS | libc::EOPNOTSUPP | libc::EBADF | libc::EPERM)
                        if total == 0 =>
                    {
                        kernel = false;
                        continue;
                    }
                    Some(libc::EINTR) => continue,
                    _ => return Err(e),
                }
            }
            r as usize
        } else {
            if buf.is_empty() {
                buf = vec![0u8; 1 << 20];
            }
            let n = match from.read(&mut buf) {
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            to.write_all(&buf[..n])?;
            n
        };
        if n == 0 {
            return Ok(total);
        }
        total += n as u64;
        each(n as u64)?;
    }
}

/// A short random word for temporary names.
pub fn random_word() -> String {
    let mut b = [0u8; 6];
    let ok = File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).is_ok();
    if !ok {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let x = t as u64 ^ (u64::from(std::process::id()) << 32);
        b.copy_from_slice(&x.to_le_bytes()[..6]);
    }
    b.iter().map(|v| format!("{v:02x}")).collect()
}

/// This process's user ID.
pub fn uid() -> u32 {
    // SAFETY: getuid has no preconditions.
    unsafe { libc::getuid() }
}

/// The free and total bytes of the filesystem a path is on.
pub fn space(path: &Path) -> Option<(u64, u64)> {
    let p = cstr(path).ok()?;
    // SAFETY: `st` is written by statvfs only.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: a valid path and a valid out-pointer.
    if unsafe { libc::statvfs(p.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let block = st.f_frsize as u64;
    Some((st.f_bavail as u64 * block, st.f_blocks as u64 * block))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsops::testing::TempDir;

    #[test]
    fn renames_refuse_to_overwrite() {
        let t = TempDir::new("noreplace");
        let a = t.file("a", "A");
        let b = t.file("b", "B");
        let e = rename_noreplace(&a, &b).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "B");
        rename_noreplace(&a, &t.join("c")).unwrap();
        assert!(!a.exists());
        assert_eq!(std::fs::read_to_string(t.join("c")).unwrap(), "A");
    }

    #[test]
    fn copies_contents_in_chunks() {
        let t = TempDir::new("contents");
        let data: Vec<u8> = (0..(CHUNK * 2 + 123)).map(|i| (i % 251) as u8).collect();
        std::fs::write(t.join("big"), &data).unwrap();
        let mut from = File::open(t.join("big")).unwrap();
        let mut to = File::create(t.join("copy")).unwrap();
        let mut chunks = 0;
        let n = copy_contents(&mut from, &mut to, &mut |_| {
            chunks += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(n as usize, data.len());
        assert!(chunks >= 3);
        assert_eq!(std::fs::read(t.join("copy")).unwrap(), data);
        // A chunk's hearer can stop it.
        let mut from = File::open(t.join("big")).unwrap();
        let mut to = File::create(t.join("stopped")).unwrap();
        let e = copy_contents(&mut from, &mut to, &mut |_| Err(io::Error::other("stop"))).unwrap_err();
        assert_eq!(e.to_string(), "stop");
    }
}
