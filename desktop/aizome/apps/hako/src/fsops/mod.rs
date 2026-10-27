//! File operations, with no toolkit: copying, moving, the Trash,
//! deleting from it, archives, names, and undo.
//!
//! Everything here is written to lose nothing:
//!
//! - **Nothing appears half-made.** A file or folder being copied is
//!   written under a hidden temporary name beside its destination
//!   (`.name.hako-part-…`) and renamed into place only once it's whole; a
//!   cancelled or failed copy removes what it made.
//! - **A move deletes only what arrived.** Within a filesystem a move is
//!   a rename (atomic). Across filesystems it's a copy, then `fsync` of
//!   every file and folder made, then the rename into place and an
//!   `fsync` of the folder it's in, and only then the deletion of the
//!   source. An item whose copy skipped or failed anything stays where it
//!   was.
//! - **Nothing is replaced unasked.** Renames refuse to overwrite
//!   (`renameat2(RENAME_NOREPLACE)`); an existing name is a [`Conflict`]
//!   the [`Asker`] answers (replace, skip, keep both, merge folders). A
//!   replaced item is set aside and removed only after its replacement
//!   is in place, and put back if that fails.
//! - **Deleting is for the Trash.** [`delete`] removes items only as
//!   [`trash`] lets it: what's in a trash, emptied or deleted from it.
//!
//! Work runs on a thread of the caller's: it checks a [`Control`]
//! between chunks (to pause and cancel), reports [`Progress`], and asks
//! questions through an [`Asker`], which may block until someone
//! answers.

pub mod archive;
pub mod control;
pub mod delete;
pub mod names;
pub mod sys;
pub mod transfer;
pub mod trash;
pub mod undo;

use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub use control::{Cancelled, Control};

/// Two items want one name.
#[derive(Clone, Debug, PartialEq)]
pub struct Conflict {
    /// What's coming.
    pub source: PathBuf,
    /// What's there.
    pub target: PathBuf,
    pub source_is_dir: bool,
    pub target_is_dir: bool,
    pub source_size: u64,
    pub target_size: u64,
    pub source_modified: Option<SystemTime>,
    pub target_modified: Option<SystemTime>,
}

impl Conflict {
    /// Folders on both sides: merging them is an answer.
    pub fn can_merge(&self) -> bool {
        self.source_is_dir && self.target_is_dir
    }
}

/// What to do about a [`Conflict`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Resolution {
    /// The new item takes the name; the old one goes (only once the new
    /// one is in place).
    Replace,
    /// Leave both as they are; the new one doesn't come.
    Skip,
    /// The new one comes under a name of its own ("name 2.txt").
    KeepBoth,
    /// Folders: the new one's contents go into the old one, each a
    /// question of its own if it conflicts.
    Merge,
    /// Stop the whole operation.
    Cancel,
}

/// What to do about an error on one item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorChoice {
    /// Leave it and go on (a move then keeps its source).
    Skip,
    Retry,
    Cancel,
}

/// An error on one item, as the person is asked about it.
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub path: PathBuf,
    /// What was being done: "copy", "read", "make the folder".
    pub action: &'static str,
    pub message: String,
    /// The error is one a retry may fix (not a missing source, say).
    pub can_retry: bool,
}

impl Failure {
    pub fn new(path: &Path, action: &'static str, error: &std::io::Error) -> Failure {
        let can_retry = !matches!(error.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidInput);
        Failure { path: path.to_path_buf(), action, message: describe(error), can_retry }
    }

    pub fn because(path: &Path, action: &'static str, message: impl Into<String>) -> Failure {
        Failure { path: path.to_path_buf(), action, message: message.into(), can_retry: false }
    }
}

/// An I/O error in words for people: "Permission denied" rather than
/// "Permission denied (os error 13)".
pub fn describe(error: &std::io::Error) -> String {
    let s = error.to_string();
    match s.find(" (os error") {
        Some(i) => s[..i].to_owned(),
        None => s,
    }
}

/// Answers an operation's questions. The UI's asks the person (blocking
/// the worker until they answer, and remembering "apply to all");
/// tests' answer at once.
pub trait Asker {
    fn conflict(&mut self, conflict: &Conflict) -> Resolution;
    fn error(&mut self, failure: &Failure) -> ErrorChoice;
}

/// An [`Asker`] that always gives the same answers.
#[derive(Clone, Copy, Debug)]
pub struct Always(pub Resolution, pub ErrorChoice);

impl Asker for Always {
    fn conflict(&mut self, c: &Conflict) -> Resolution {
        if self.0 == Resolution::Merge && !c.can_merge() { Resolution::KeepBoth } else { self.0 }
    }

    fn error(&mut self, _: &Failure) -> ErrorChoice {
        self.1
    }
}

/// How far an operation has come.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub items_done: u64,
    pub items_total: u64,
    /// The item being worked on.
    pub current: Option<PathBuf>,
    /// Still counting what there is to do.
    pub preparing: bool,
}

impl Progress {
    /// Done, from 0 to 1, by bytes where there are any, else by items.
    pub fn fraction(&self) -> f64 {
        if self.bytes_total > 0 {
            (self.bytes_done as f64 / self.bytes_total as f64).clamp(0.0, 1.0)
        } else if self.items_total > 0 {
            (self.items_done as f64 / self.items_total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// Hears progress. The UI's sends it on (at most so often); tests keep
/// the last.
pub trait Reporter {
    fn report(&mut self, progress: &Progress);
}

impl Reporter for () {
    fn report(&mut self, _: &Progress) {}
}

impl<F: FnMut(&Progress)> Reporter for F {
    fn report(&mut self, progress: &Progress) {
        self(progress)
    }
}

/// What an operation did, item by item (each a top-level item it was
/// given).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// Each item done, and where it is now.
    pub done: Vec<(PathBuf, PathBuf)>,
    /// Items left as they were by choice (a conflict skipped).
    pub skipped: Vec<PathBuf>,
    /// Items not done, and why. For a move across filesystems, an item
    /// copied only in part is here, its source untouched.
    pub failed: Vec<(PathBuf, String)>,
    /// Things inside the items that were left out (a file that couldn't
    /// be read, skipped), and why; their items are in `done`.
    pub problems: Vec<(PathBuf, String)>,
    /// Stopped before the end.
    pub cancelled: bool,
}

impl Report {
    pub fn is_clean(&self) -> bool {
        self.failed.is_empty() && self.problems.is_empty() && !self.cancelled
    }

    /// The report's results after another's (one operation done in
    /// parts).
    pub fn absorb(&mut self, other: Report) {
        self.done.extend(other.done);
        self.skipped.extend(other.skipped);
        self.failed.extend(other.failed);
        self.problems.extend(other.problems);
        self.cancelled |= other.cancelled;
    }
}

/// What an operation is told to work with.
pub struct Env<'a> {
    pub control: &'a Control,
    pub asker: &'a mut dyn Asker,
    pub reporter: &'a mut dyn Reporter,
    pub progress: Progress,
}

impl<'a> Env<'a> {
    pub fn new(control: &'a Control, asker: &'a mut dyn Asker, reporter: &'a mut dyn Reporter) -> Env<'a> {
        Env { control, asker, reporter, progress: Progress::default() }
    }

    pub(crate) fn report(&mut self) {
        self.reporter.report(&self.progress);
    }

    /// Ask what to do about `failure`, as many times as it's retried: the
    /// answer, never `Retry` (the caller retries by calling again).
    pub(crate) fn ask_error(&mut self, failure: Failure) -> ErrorChoice {
        if self.control.is_cancelled() {
            return ErrorChoice::Cancel;
        }
        let choice = self.asker.error(&failure);
        if choice == ErrorChoice::Retry && !failure.can_retry { ErrorChoice::Skip } else { choice }
    }
}

/// Whether `inner` is `outer` or inside it (by their paths, made
/// absolute and without `.` and `..`, not following links).
pub fn is_within(inner: &Path, outer: &Path) -> bool {
    let (i, o) = (names::normalize(inner), names::normalize(outer));
    i.starts_with(&o)
}

#[cfg(test)]
pub(crate) mod testing {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fresh folder for a test, under the build's (or the system's)
    /// temporary folder, removed when dropped. Tests never touch anything
    /// outside these.
    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new(name: &str) -> TempDir {
            static N: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::var_os("CARGO_TARGET_TMPDIR")
                .or_else(|| std::env::var_os("CARGO_TARGET_DIR"))
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(std::env::temp_dir)
                .join("hako-tests");
            let dir = root.join(format!("{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("a test folder");
            TempDir(std::fs::canonicalize(&dir).expect("its real path"))
        }

        /// A folder on another filesystem than [`TempDir::new`]'s (tmpfs
        /// at `/dev/shm`), for moves across devices; `None` where there's
        /// none.
        pub fn elsewhere(name: &str) -> Option<TempDir> {
            use std::os::unix::fs::MetadataExt;
            let shm = Path::new("/dev/shm");
            let here = TempDir::new("probe");
            let dev = std::fs::metadata(here.path()).ok()?.dev();
            if std::fs::metadata(shm).ok()?.dev() == dev {
                return None;
            }
            static N: AtomicUsize = AtomicUsize::new(0);
            let dir =
                shm.join(format!("hako-test-{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&dir).ok()?;
            Some(TempDir(dir))
        }

        pub fn path(&self) -> &Path {
            &self.0
        }

        pub fn join(&self, p: impl AsRef<Path>) -> PathBuf {
            self.0.join(p)
        }

        /// Write `text` to `rel`, making folders on the way.
        pub fn file(&self, rel: &str, text: &str) -> PathBuf {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, text).unwrap();
            p
        }

        pub fn dir(&self, rel: &str) -> PathBuf {
            let p = self.0.join(rel);
            std::fs::create_dir_all(&p).unwrap();
            p
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            // Read-only folders made by tests would stop the removal.
            fn writable(p: &Path) {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(m) = std::fs::symlink_metadata(p)
                    && m.is_dir()
                {
                    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755));
                    if let Ok(rd) = std::fs::read_dir(p) {
                        for e in rd.flatten() {
                            writable(&e.path());
                        }
                    }
                }
            }
            writable(&self.0);
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Whether the tests run as root (permission tests mean nothing then).
    pub fn is_root() -> bool {
        // SAFETY: geteuid has no preconditions.
        unsafe { libc::geteuid() == 0 }
    }

    pub fn read(p: impl AsRef<Path>) -> String {
        std::fs::read_to_string(p).unwrap()
    }
}
