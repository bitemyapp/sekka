//! What the kernel answered when asked for a watch, as values only this
//! module makes.
//!
//! Placing the watch goes wrong when a step acts on something that is no
//! longer true: a folder taken for absent that has since been made, a watch
//! taken to be on the settings path whose folder has since moved, a missing
//! folder taken for notifications that don't work. Each of those facts is a
//! type here with private fields, so the placing ([`super::placing`]) can
//! hold one only by having asked, in the order that makes it true.

use std::ffi::{CString, OsString};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use aizome_typestate::{Confirmed, Reviewed, SameTarget};

/// Changes to a file or folder itself: written, replaced, moved, removed.
pub(super) const CHANGES: u32 = libc::IN_CLOSE_WRITE | libc::IN_ATTRIB | libc::IN_DELETE_SELF | libc::IN_MOVE_SELF;
/// A folder on the way to the settings file: itself, and what comes and goes
/// in it.
pub(super) const FOLDER: u32 =
    CHANGES | libc::IN_CREATE | libc::IN_DELETE | libc::IN_MOVED_FROM | libc::IN_MOVED_TO | libc::IN_ONLYDIR;
/// A folder above that: only its own move or removal, which takes the
/// settings path with it.
pub(super) const ABOVE: u32 = libc::IN_MOVE_SELF | libc::IN_DELETE_SELF | libc::IN_ONLYDIR;
/// The settings path when it is a folder: its own move or removal.
pub(super) const MOVED: u32 = libc::IN_MOVE_SELF | libc::IN_DELETE_SELF;

/// The brand of a watch asked for here. Only [`Inotify`] makes one, so no
/// other code can say the kernel gave it a watch:
///
/// ```compile_fail,E0423
/// use aizome_config::watch::kernel::Placement;
/// let _ = Placement(());
/// ```
#[derive(Debug)]
pub struct Placement(());

/// A path a watch was asked for on, and the watch the kernel gave. Whether
/// the watch is on what is at that path now is another question
/// ([`Inotify::again`]).
#[derive(Debug)]
pub struct Asked<'p> {
    path: &'p Path,
    mask: u32,
    wd: i32,
}

/// The watch the kernel gave when asked a second time. Only
/// [`Inotify::again`] makes one:
///
/// ```compile_fail,E0451
/// use aizome_config::watch::kernel::Landed;
/// let _ = Landed { wd: 1 };
/// ```
#[derive(Debug)]
pub struct Landed {
    wd: i32,
}

impl Landed {
    pub(super) fn wd(&self) -> i32 {
        self.wd
    }
}

impl SameTarget<Landed> for Asked<'_> {
    /// The kernel names a watch by the file it is on, not by the path it
    /// was asked for with: the same number is the same file.
    fn same_target(&self, fresh: &Landed) -> bool {
        self.wd == fresh.wd
    }
}

/// What the kernel answered when asked for a watch.
#[must_use = "a watch that isn't kept reports nothing"]
#[derive(Debug)]
pub enum Placed<'p> {
    /// A watch, on whatever was at the path when the kernel looked.
    Watching(Reviewed<Asked<'p>, Placement>),
    /// Nothing is at the path. The folders changed, or the settings folder
    /// isn't made yet: look above, or place again. Never a reason to poll.
    Gone,
    /// The kernel refused for another reason.
    Unavailable(Unavailable),
}

/// File notifications can't be had: the kernel refused one for a reason
/// other than a missing path, or the folders wouldn't hold still
/// ([`Budget`]). Only those make one, so a folder that merely went can't
/// send the watcher to polling:
///
/// ```compile_fail,E0451
/// use aizome_config::watch::kernel::Unavailable;
/// let _ = Unavailable { why: std::io::Error::other("the folder went") };
/// ```
#[derive(Debug)]
pub struct Unavailable {
    why: io::Error,
}

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.why.fmt(f)
    }
}

/// Why a step of a placing didn't hold.
#[derive(Debug)]
pub enum Step {
    /// The folders changed under it: place again, on what is there now.
    Again,
    /// Notifications can't be had.
    Unavailable(Unavailable),
}

impl From<Unavailable> for Step {
    fn from(why: Unavailable) -> Step {
        Step::Unavailable(why)
    }
}

/// How many more times something may be done before the watcher gives up on
/// notifications. Each further placing needs another change at just the
/// wrong moment, so running out means the folders won't hold still.
#[derive(Debug)]
pub struct Budget {
    left: usize,
    spent_on: &'static str,
}

impl Budget {
    /// For placing one path's watches again.
    pub(super) fn placings() -> Budget {
        Budget { left: 16, spent_on: "the folders above the settings file kept changing" }
    }

    /// For the paths that the settings path's symbolic links lead to.
    pub(super) fn symlinks() -> Budget {
        Budget { left: 40, spent_on: "too many settings symlinks" }
    }

    /// One more, or [`Unavailable`] when none is left.
    pub(super) fn spend(&mut self) -> Result<(), Unavailable> {
        match self.left.checked_sub(1) {
            Some(left) => {
                self.left = left;
                Ok(())
            }
            None => Err(Unavailable { why: io::Error::other(self.spent_on) }),
        }
    }
}

/// The folder nearest the settings file that exists, watched: the file's
/// own folder, or one above it whose next folder down was absent when looked
/// for through this watch. Only [`Nearest::below`] makes one, and it takes
/// the watch first:
///
/// ```compile_fail,E0451
/// use aizome_config::watch::kernel::{Asked, Nearest, Placement};
/// use aizome_typestate::Reviewed;
/// use std::path::Path;
/// // Found missing before the folder above it was watched: no proof.
/// fn assume<'p>(watch: Reviewed<Asked<'p>, Placement>, missing: &'p Path) -> Nearest<'p> {
///     Nearest { watch, child: missing }
/// }
/// ```
#[derive(Debug)]
pub struct Nearest<'p> {
    watch: Reviewed<Asked<'p>, Placement>,
    child: &'p Path,
}

/// What lies below a watched folder, on the way to the settings file.
#[derive(Debug)]
pub enum Below<'p> {
    /// Nothing that isn't watched for: the settings file itself, or a
    /// folder that isn't there.
    Nothing(Nearest<'p>),
    /// A folder, at this path. It wasn't there when its own watch was asked
    /// for, and was made before the watch above it was placed: nothing
    /// reported it. The watch above comes back with it.
    Folder(Reviewed<Asked<'p>, Placement>, &'p Path),
}

impl<'p> Nearest<'p> {
    /// Looks below `watched`, on the way to `file`. The folder is watched
    /// already, so whatever is made below it from now on is reported.
    pub fn below(watched: Reviewed<Asked<'p>, Placement>, file: &'p Path) -> Below<'p> {
        let folder = watched.target().path;
        let child = file
            .ancestors()
            .find(|path| path.parent() == Some(folder))
            .expect("a folder watched for the settings file is above it");
        if child != file && child.is_dir() {
            Below::Folder(watched, child)
        } else {
            Below::Nothing(Nearest { watch: watched, child })
        }
    }

    /// The folder.
    pub fn folder(&self) -> &'p Path {
        self.watch.target().path
    }

    /// Its watch, and the name to listen for in it.
    pub(super) fn into_watch(self) -> (Reviewed<Asked<'p>, Placement>, OsString) {
        (self.watch, name(self.child))
    }
}

/// The last part of `path`, which every path below a folder has.
pub(super) fn name(path: &Path) -> OsString {
    path.file_name().map(OsString::from).unwrap_or_default()
}

/// The kernel's file notifications: the only code that asks for a watch.
#[derive(Debug)]
pub struct Inotify {
    fd: OwnedFd,
}

impl AsRawFd for Inotify {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl Inotify {
    pub(super) fn new() -> Result<Inotify, Unavailable> {
        // SAFETY: plain system call; a successful descriptor is owned below.
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if fd < 0 {
            return Err(Unavailable { why: io::Error::last_os_error() });
        }
        // SAFETY: inotify_init1 returned a fresh descriptor.
        Ok(Inotify { fd: unsafe { OwnedFd::from_raw_fd(fd) } })
    }

    /// Asks for a watch on what is at `at`, without recording anything.
    fn add(&self, at: &Path, mask: u32) -> io::Result<i32> {
        let at = CString::new(at.as_os_str().as_bytes())?;
        // SAFETY: fd is open and the path is a valid, NUL-terminated string.
        let wd = unsafe { libc::inotify_add_watch(self.fd.as_raw_fd(), at.as_ptr(), mask | libc::IN_MASK_ADD) };
        if wd < 0 { Err(io::Error::last_os_error()) } else { Ok(wd) }
    }

    /// Asks for a watch on `path`.
    pub fn watch<'p>(&self, path: &'p Path, mask: u32) -> Placed<'p> {
        self.watch_landing(path, None, mask)
    }

    /// Asks for a watch on `path`. A test that has just moved what was
    /// there names where it went, and the watch lands on it there: as it
    /// does when something is moved after the kernel has found it and
    /// before it is watched.
    pub(super) fn watch_landing<'p>(&self, path: &'p Path, landing: Option<&Path>, mask: u32) -> Placed<'p> {
        match self.add(landing.unwrap_or(path), mask) {
            Ok(wd) => Placed::Watching(Reviewed::new(Asked { path, mask, wd }, Placement(()))),
            Err(why) if why.kind() == io::ErrorKind::NotFound => Placed::Gone,
            Err(why) => Placed::Unavailable(Unavailable { why }),
        }
    }

    /// Asks for the same watch again. A folder can be moved or replaced
    /// after the kernel has found it and before the watch lands on it; the
    /// watch then sits on something that is no longer at the path, and the
    /// move it would have reported has already happened. The same watch
    /// comes back only while the path still leads to what it is on, and
    /// from then any move is reported.
    pub fn again(&self, asked: Reviewed<Asked<'_>, Placement>) -> Result<Confirmed<Landed, Placement>, Step> {
        let Asked { path, mask, .. } = *asked.target();
        match self.add(path, mask) {
            Ok(wd) => asked.confirm(Landed { wd }).map_err(|_| Step::Again),
            Err(why) if why.kind() == io::ErrorKind::NotFound => Err(Step::Again),
            Err(why) => Err(Step::Unavailable(Unavailable { why })),
        }
    }

    /// Reads what has been reported, giving each report's watch, kind and
    /// name to `ours`: whether any was one of ours. A queue that overflowed
    /// counts, since what it dropped may have been.
    pub(super) fn reported(&self, mut ours: impl FnMut(i32, &[u8]) -> bool) -> io::Result<bool> {
        let mut changed = false;
        let mut bytes = [0u8; 8192];
        loop {
            // SAFETY: bytes is writable for its full length.
            let n = unsafe { libc::read(self.fd.as_raw_fd(), bytes.as_mut_ptr().cast(), bytes.len()) };
            if n < 0 {
                let error = io::Error::last_os_error();
                match error.kind() {
                    io::ErrorKind::WouldBlock => return Ok(changed),
                    io::ErrorKind::Interrupted => continue,
                    _ => return Err(error),
                }
            }
            if n == 0 {
                return Ok(changed);
            }
            let mut offset = 0;
            let header = std::mem::size_of::<libc::inotify_event>();
            while offset + header <= n as usize {
                // SAFETY: the header is within the initialized bytes; the
                // byte array need not have inotify_event's alignment.
                let event = unsafe { bytes.as_ptr().add(offset).cast::<libc::inotify_event>().read_unaligned() };
                let end = offset + header + event.len as usize;
                if end > n as usize {
                    return Err(io::Error::other("incomplete settings notification"));
                }
                let name = &bytes[offset + header..end];
                let name = &name[..name.iter().position(|b| *b == 0).unwrap_or(name.len())];
                changed |= event.mask & libc::IN_Q_OVERFLOW != 0 || ours(event.wd, name);
                offset = end;
            }
        }
    }
}
