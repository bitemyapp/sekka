// SPDX-License-Identifier: GPL-3.0-or-later
//! Downloads a release's files into private staging and proves they are the
//! catalog's bytes; and, on the helper's side, imports them from there into
//! the helper's own staging without trusting anything about them.
//!
//! Downloads go in 16 MiB ranged requests. Each part is checked against the
//! `Content-Range` the server returns, appended, and synchronized, so a
//! download interrupted at any point resumes at the last whole part. A
//! stalled part times out (the transport's idle timeout) and is retried a
//! bounded number of times with backoff. Only after the last byte is the
//! whole file hashed against the catalog; until then it lives under a
//! `.partial` name and is never used. That name carries the digest the file
//! must have, so what is left of another release's download is removed,
//! never resumed into this one or counted toward it. A finished file found
//! later is hashed again before it is reused.
//!
//! Nothing here touches a disk other than the staging directory, and
//! nothing here is reachable before the person chooses to download.
#![deny(unsafe_code)]

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sekka_mac_contract::{Artifact, Digest, Hasher, HttpsUrl};

/// The size of one ranged request.
pub const PART: u64 = 16 * 1024 * 1024;

/// What a download must turn out to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expect {
    pub size: u64,
    pub digest: Digest,
}
impl From<&Artifact> for Expect {
    fn from(a: &Artifact) -> Self {
        Self {
            size: a.size,
            digest: a.digest,
        }
    }
}

/// One ranged response, as the transport received it.
pub struct RangeResponse {
    pub status: u16,
    /// The `Content-Range` header, verbatim.
    pub content_range: Option<String>,
    pub body: Vec<u8>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TransportError {
    /// Worth trying again: the network dropped, the request timed out, the
    /// server was briefly unavailable.
    #[error("{0}")]
    Retryable(String),
    /// Trying again won't help: the address is wrong, the certificate is
    /// untrusted.
    #[error("{0}")]
    Fatal(String),
}

/// How bytes arrive. The app uses `NSURLSession` (Apple's trust store,
/// proxies and idle timeouts); tests use a loopback server.
pub trait Transport {
    /// GET bytes `start..=end` of `url`.
    fn get_range(&self, url: &HttpsUrl, start: u64, end: u64) -> Result<RangeResponse, TransportError>;
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StageError {
    #[error("the download was cancelled; what was downloaded is kept for next time")]
    Cancelled,
    #[error(
        "there isn't enough space to download it: {needed} more bytes are needed and {available} are free"
    )]
    NoSpace { needed: u64, available: u64 },
    #[error("the download stopped and didn't resume after {attempts} tries ({last}); what was downloaded is kept")]
    Network { attempts: u32, last: String },
    #[error("the download failed: {0}")]
    Fatal(String),
    #[error("the server sent something other than what was asked for ({0})")]
    BadResponse(String),
    #[error("the downloaded file doesn't match the release catalog (expected {expected}, got {actual}); it was deleted")]
    Corrupt { expected: Digest, actual: Digest },
    #[error("the staging folder isn't private: {0}")]
    UnsafeStaging(String),
    #[error("{0}")]
    Io(String),
}

impl From<io::Error> for StageError {
    fn from(e: io::Error) -> Self {
        if e.raw_os_error() == Some(libc::ENOSPC) {
            return Self::NoSpace {
                needed: 0,
                available: 0,
            };
        }
        Self::Io(e.to_string())
    }
}

/// A file in staging whose size and digest were just checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedFile {
    pub path: PathBuf,
    pub size: u64,
    pub digest: Digest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StageEvent {
    /// `done` of `total` bytes are on disk.
    Progress { done: u64, total: u64 },
    /// A part failed and will be tried again after `wait`.
    Retrying { attempt: u32, wait: Duration, reason: String },
    /// Hashing a finished file.
    Verifying,
    /// A file from an earlier run was checked and reused.
    Reused,
}

/// A directory only this user can read or write, checked when opened.
#[derive(Clone, Debug)]
pub struct StagingDir {
    path: PathBuf,
}

impl StagingDir {
    /// Create (mode 0700) or open the directory, refusing a link, another
    /// owner, or access for anyone else.
    pub fn open(path: &Path) -> Result<Self, StageError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        match fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
        let meta = fs::symlink_metadata(path)?;
        if !meta.file_type().is_dir() {
            return Err(StageError::UnsafeStaging(format!("{} isn't a folder", path.display())));
        }
        if meta.uid() != effective_uid() {
            return Err(StageError::UnsafeStaging(format!(
                "{} belongs to someone else",
                path.display()
            )));
        }
        if meta.mode() & 0o077 != 0 {
            return Err(StageError::UnsafeStaging(format!(
                "others can open {}",
                path.display()
            )));
        }
        Ok(Self { path: path.into() })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn file(&self, name: &str) -> Result<PathBuf, StageError> {
        // A `.partial` name is a download in progress, never a file's own.
        let plain = !name.is_empty()
            && !name.starts_with('.')
            && !name.ends_with(".partial")
            && name.len() <= 128
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
        if !plain {
            return Err(StageError::UnsafeStaging(format!("not a plain file name: {name}")));
        }
        Ok(self.path.join(name))
    }

    /// The bytes the files named here still need, given what is already
    /// staged: the peak extra space downloading them takes. A download in
    /// progress counts only toward the digest it was started for; one left
    /// from another release counts for nothing (staging removes it).
    pub fn bytes_needed(&self, files: &[(&str, &Expect)]) -> u64 {
        files
            .iter()
            .map(|(name, expect)| {
                let Ok(path) = self.file(name) else { return expect.size };
                if regular_size(&path) == Some(expect.size) {
                    return 0;
                }
                let partial = partial_name(&path, &expect.digest);
                expect.size - regular_size(&partial).unwrap_or(0).min(expect.size)
            })
            .sum()
    }

    /// Remove every download in progress for `name` but `keep`: those for
    /// another digest, and those whose name doesn't say which. None of them
    /// can become this file, and each may hold gigabytes.
    fn discard_partials(&self, name: &str, keep: &Path) -> Result<(), StageError> {
        for entry in fs::read_dir(&self.path)? {
            let entry = entry?;
            let path = entry.path();
            if path == keep || !is_partial_of(name, &entry.file_name()) {
                continue;
            }
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    /// Hash a finished file from an earlier run and keep it if it is still
    /// right; remove it if not. Never uses a `.partial`.
    pub fn verified(&self, name: &str, expect: &Expect) -> Result<Option<StagedFile>, StageError> {
        let path = self.file(name)?;
        let Some(size) = regular_size(&path) else {
            return Ok(None);
        };
        if size == expect.size {
            let digest = hash_file(&path, &AtomicBool::new(false))?;
            if digest == expect.digest {
                return Ok(Some(StagedFile {
                    path,
                    size,
                    digest,
                }));
            }
        }
        fs::remove_file(&path)?;
        Ok(None)
    }
}

/// Where the download of `path` with this digest waits until it is whole.
fn partial_name(path: &Path, digest: &Digest) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.partial", digest.hex()));
    path.with_file_name(name)
}

/// Whether `file` is a download in progress for `name`: of any digest, or
/// from before a partial's name carried one.
fn is_partial_of(name: &str, file: &OsStr) -> bool {
    let Some(middle) = file
        .to_str()
        .and_then(|f| f.strip_prefix(name))
        .and_then(|rest| rest.strip_suffix(".partial"))
    else {
        return false;
    };
    match middle.strip_prefix('.') {
        Some(hex) => Digest::from_hex(hex).is_ok(),
        None => middle.is_empty(),
    }
}

fn regular_size(path: &Path) -> Option<u64> {
    let meta = fs::symlink_metadata(path).ok()?;
    meta.file_type().is_file().then_some(meta.len())
}

fn hash_file(path: &Path, cancel: &AtomicBool) -> Result<Digest, StageError> {
    let mut f = open_nofollow(path)?;
    let mut hasher = Hasher::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(StageError::Cancelled);
        }
        let n = f.read(&mut buffer)?;
        if n == 0 {
            return Ok(hasher.finish());
        }
        hasher.update(&buffer[..n]);
    }
}

fn open_nofollow(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}

#[allow(unsafe_code)]
fn effective_uid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

/// Free bytes on the file system holding `path`.
#[allow(unsafe_code)]
pub fn free_space(path: &Path) -> io::Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path and `s` a writable struct.
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(s.f_bavail as u64 * s.f_frsize as u64)
}

/// How many times a failing part is tried, and how long to wait between.
#[derive(Clone, Copy, Debug)]
pub struct RetryPolicy {
    pub attempts: u32,
    pub first_wait: Duration,
    pub max_wait: Duration,
}
impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 6,
            first_wait: Duration::from_secs(2),
            max_wait: Duration::from_secs(60),
        }
    }
}

/// Downloads into one staging directory.
pub struct Stager<'a, T: Transport> {
    pub dir: &'a StagingDir,
    pub transport: T,
    pub retry: RetryPolicy,
    pub cancel: &'a AtomicBool,
    /// Waiting between attempts (tests don't wait).
    pub sleep: fn(Duration),
    /// Free space (tests pretend).
    pub space: fn(&Path) -> io::Result<u64>,
}

impl<'a, T: Transport> Stager<'a, T> {
    pub fn new(dir: &'a StagingDir, transport: T, cancel: &'a AtomicBool) -> Self {
        Self {
            dir,
            transport,
            retry: RetryPolicy::default(),
            cancel,
            sleep: std::thread::sleep,
            space: free_space,
        }
    }

    /// Download `url` into `name` until it is exactly the expected bytes.
    pub fn stage(
        &self,
        name: &str,
        url: &HttpsUrl,
        expect: &Expect,
        on: &mut dyn FnMut(StageEvent),
    ) -> Result<StagedFile, StageError> {
        let path = self.dir.file(name)?;
        let partial = partial_name(&path, &expect.digest);
        self.dir.discard_partials(name, &partial)?;
        if let Some(file) = self.dir.verified(name, expect)? {
            on(StageEvent::Reused);
            return Ok(file);
        }
        // A corrupt result gets one fresh download: a bad byte from a
        // flaky connection shouldn't strand the person, but a server that
        // keeps serving the wrong file must not be fetched forever.
        for fresh in [false, true] {
            if fresh {
                let _ = fs::remove_file(&partial);
            }
            self.download(url, &partial, expect, on)?;
            on(StageEvent::Verifying);
            let digest = hash_file(&partial, self.cancel)?;
            if digest == expect.digest {
                File::open(&partial)?.sync_all()?;
                fs::rename(&partial, &path)?;
                File::open(self.dir.path())?.sync_all()?;
                return Ok(StagedFile {
                    path,
                    size: expect.size,
                    digest,
                });
            }
            if fresh {
                let _ = fs::remove_file(&partial);
                return Err(StageError::Corrupt {
                    expected: expect.digest,
                    actual: digest,
                });
            }
        }
        unreachable!()
    }

    fn download(
        &self,
        url: &HttpsUrl,
        partial: &Path,
        expect: &Expect,
        on: &mut dyn FnMut(StageEvent),
    ) -> Result<(), StageError> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(partial)?;
        let meta = file.metadata()?;
        if !meta.file_type().is_file() || meta.nlink() != 1 {
            return Err(StageError::UnsafeStaging(format!("{} isn't a plain file", partial.display())));
        }
        // Resume at the last whole part; anything after it may be torn.
        let mut done = (meta.len().min(expect.size) / PART) * PART;
        if meta.len() == expect.size {
            done = expect.size;
        }
        file.set_len(done)?;
        let available = (self.space)(self.dir.path())?;
        let needed = expect.size - done;
        if needed > available {
            return Err(StageError::NoSpace { needed, available });
        }
        on(StageEvent::Progress {
            done,
            total: expect.size,
        });
        let mut attempt = 0;
        while done < expect.size {
            if self.cancel.load(Ordering::Relaxed) {
                file.sync_all()?;
                return Err(StageError::Cancelled);
            }
            let end = (done + PART).min(expect.size) - 1;
            match self.part(url, done, end, expect.size) {
                Ok(bytes) => {
                    file.seek(SeekFrom::Start(done))?;
                    file.write_all(&bytes)?;
                    file.sync_data()?;
                    done = end + 1;
                    attempt = 0;
                    on(StageEvent::Progress {
                        done,
                        total: expect.size,
                    });
                }
                Err(PartError::Fatal(e)) => return Err(e),
                Err(PartError::Retry(reason)) => {
                    attempt += 1;
                    if attempt >= self.retry.attempts {
                        return Err(StageError::Network {
                            attempts: attempt,
                            last: reason,
                        });
                    }
                    let wait = self
                        .retry
                        .first_wait
                        .saturating_mul(1 << (attempt - 1).min(16))
                        .min(self.retry.max_wait);
                    on(StageEvent::Retrying {
                        attempt,
                        wait,
                        reason,
                    });
                    (self.sleep)(wait);
                }
            }
        }
        Ok(())
    }

    fn part(&self, url: &HttpsUrl, start: u64, end: u64, total: u64) -> Result<Vec<u8>, PartError> {
        let response = match self.transport.get_range(url, start, end) {
            Ok(r) => r,
            Err(TransportError::Retryable(e)) => return Err(PartError::Retry(e)),
            Err(TransportError::Fatal(e)) => return Err(PartError::Fatal(StageError::Fatal(e))),
        };
        match response.status {
            206 => {}
            408 | 429 | 500..=599 => {
                return Err(PartError::Retry(format!("the server answered {}", response.status)));
            }
            200 => {
                return Err(PartError::Fatal(StageError::BadResponse(
                    "the server ignored the byte range; Sekka's download hosts support ranges".into(),
                )));
            }
            other => {
                return Err(PartError::Fatal(StageError::Fatal(format!(
                    "the server answered {other}"
                ))));
            }
        }
        let expected = format!("bytes {start}-{end}/{total}");
        if response.content_range.as_deref() != Some(expected.as_str()) {
            return Err(PartError::Fatal(StageError::BadResponse(format!(
                "asked for {expected}, got {}",
                response.content_range.as_deref().unwrap_or("no range")
            ))));
        }
        if response.body.len() as u64 != end - start + 1 {
            // A short body is a dropped connection.
            return Err(PartError::Retry(format!(
                "a part ended after {} of {} bytes",
                response.body.len(),
                end - start + 1
            )));
        }
        Ok(response.body)
    }
}

enum PartError {
    Retry(String),
    Fatal(StageError),
}

/// Copy a file the app staged into the helper's own staging, trusting
/// nothing about it: the path must lie inside `user_root` with no link
/// anywhere along it; the file must be a plain file with one link, owned by
/// `owner`, the size the catalog says, and hash to the catalog's digest as
/// it is copied. The helper calls this as root; the app's word about the
/// file's digest is never used.
pub fn import(
    source: &Path,
    user_root: &Path,
    owner: u32,
    dest: &StagingDir,
    name: &str,
    expect: &Expect,
    cancel: &AtomicBool,
) -> Result<StagedFile, StageError> {
    let unsafe_path = |why: &str| StageError::UnsafeStaging(format!("{}: {why}", source.display()));
    if !source.is_absolute()
        || source
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(unsafe_path("not a plain absolute path"));
    }
    let relative = source
        .strip_prefix(user_root)
        .map_err(|_| unsafe_path("outside the app's staging folder"))?;
    // Every component from the staging root down: no links, nothing others
    // may write.
    let mut at = user_root.to_path_buf();
    let check = |p: &Path, last: bool| -> Result<fs::Metadata, StageError> {
        let m = fs::symlink_metadata(p)?;
        if m.file_type().is_symlink() {
            return Err(unsafe_path("a link on the way"));
        }
        if m.mode() & 0o022 != 0 || (m.uid() != owner && m.uid() != 0) {
            return Err(unsafe_path("others can change it"));
        }
        if last && (!m.file_type().is_file() || m.nlink() != 1 || m.uid() != owner) {
            return Err(unsafe_path("not a plain file of the requesting user"));
        }
        Ok(m)
    };
    check(&at, false)?;
    let count = relative.components().count();
    let mut last_meta = None;
    for (i, c) in relative.components().enumerate() {
        at.push(c);
        last_meta = Some(check(&at, i + 1 == count)?);
    }
    let before = last_meta.ok_or_else(|| unsafe_path("names the folder itself"))?;
    let mut src = open_nofollow(source)?;
    let after = src.metadata()?;
    if (after.dev(), after.ino()) != (before.dev(), before.ino()) || after.len() != expect.size {
        return Err(unsafe_path("it changed while being opened, or is the wrong size"));
    }

    let path = dest.file(name)?;
    let tmp = dest.path().join(format!(".import-{name}"));
    let _ = fs::remove_file(&tmp);
    // On APFS the helper takes a clone: no second copy of a payload's
    // gigabytes, so macOS can still give what the plan asked of it. The
    // clone is the helper's own file, hashed below; what the app does to its
    // copy afterwards can't change it.
    let cloned = clone_into(&src, dest.path(), &format!(".import-{name}")).is_ok();
    let (copied, digest) = if cloned {
        let mut out = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&tmp)?;
        std::os::unix::fs::fchown(&out, Some(effective_uid()), None).ok();
        out.set_permissions(fs::Permissions::from_mode(0o600))?;
        let mut hasher = Hasher::new();
        let mut buffer = vec![0u8; 1 << 20];
        let mut copied = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = fs::remove_file(&tmp);
                return Err(StageError::Cancelled);
            }
            let n = out.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            copied += n as u64;
            hasher.update(&buffer[..n]);
        }
        out.sync_all()?;
        (copied, hasher.finish())
    } else {
        copy_hashing(&mut src, &tmp, expect, cancel, &unsafe_path)?
    };
    if copied != expect.size || digest != expect.digest {
        let _ = fs::remove_file(&tmp);
        return Err(StageError::Corrupt {
            expected: expect.digest,
            actual: digest,
        });
    }
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    fs::rename(&tmp, &path)?;
    File::open(dest.path())?.sync_all()?;
    Ok(StagedFile {
        path,
        size: copied,
        digest,
    })
}

/// Copy while hashing, where a clone isn't possible.
fn copy_hashing(
    src: &mut File,
    tmp: &Path,
    expect: &Expect,
    cancel: &AtomicBool,
    unsafe_path: &dyn Fn(&str) -> StageError,
) -> Result<(u64, Digest), StageError> {
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(tmp)?;
    let mut hasher = Hasher::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut copied = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = fs::remove_file(tmp);
            return Err(StageError::Cancelled);
        }
        let n = src.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        copied += n as u64;
        if copied > expect.size {
            let _ = fs::remove_file(tmp);
            return Err(unsafe_path("it grew while being copied"));
        }
        hasher.update(&buffer[..n]);
        out.write_all(&buffer[..n])?;
    }
    out.sync_all()?;
    Ok((copied, hasher.finish()))
}

/// An APFS clone of an open file into a directory, without following links.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn clone_into(src: &File, dir: &Path, name: &str) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let dir = File::open(dir)?;
    let name = std::ffi::CString::new(name).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // CLONE_NOFOLLOW (0x1): never clone through a link.
    // SAFETY: two open descriptors and a NUL-terminated name.
    if unsafe { libc::fclonefileat(src.as_raw_fd(), dir.as_raw_fd(), name.as_ptr(), 0x0001) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn clone_into(_: &File, _: &Path, _: &str) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

#[cfg(test)]
mod tests;
