//! Applications: what's installed, how to start it, its icons, and which
//! one opens what.
//!
//! - [`DesktopEntry`] parses `.desktop` files (Desktop Entry Specification
//!   1.5), with localised values for a [`Locale`].
//! - [`AppIndex`] finds every entry in the XDG data directories, keeps
//!   them by desktop file ID, notices changes cheaply (NixOS-aware), and
//!   matches windows to apps ([`AppIndex::match_window`]).
//! - [`Exec`] expands `Exec` lines into argument vectors; `aizome-sys`
//!   (`aizome_sys::apps::launch`) runs them in a systemd scope.
//! - [`IconLookup`] finds icon files (Icon Theme Specification), caching
//!   what it reads.
//! - [`AppIndex::default_for`] and friends answer and change default
//!   applications (MIME Applications Associations Specification), by MIME
//!   type or by [`DefaultRole`]; the default terminal follows the
//!   xdg-terminal-exec convention.
//!
//! Everything that reads the file system takes its directories from an
//! [`XdgDirs`], so tests and tools can point it at any tree. Nothing here
//! talks to D-Bus or blocks for long, so the compositor can ask it which
//! terminal ❖Return starts: the same answer Settei shows.

mod entry;
mod error;
mod exec;
mod icons;
mod index;
mod keyfile;
mod mime;
mod xdg;

pub use entry::{Action, DesktopEntry, EntryKind, Locale};
pub use error::{Error, Result};
pub use exec::{Exec, ExecContext, file_uri_to_path, path_to_file_uri};
pub use icons::{IconLookup, IconTheme};
pub use index::{App, AppIndex, Watcher};
pub use mime::DefaultRole;
pub use xdg::{DESKTOP_NAME, XdgDirs, find_program, parse_desktops};

use std::sync::{Arc, Mutex};

/// Wakes a main loop from a background thread (the index's watcher).
///
/// It must be cheap and must not block: post an event to the main loop
/// and return. `aizome-sys`'s services take the same type.
#[derive(Clone)]
pub struct Wake(Arc<dyn Fn() + Send + Sync>);

impl Wake {
    /// A wake that calls `wake`.
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Wake {
        Wake(Arc::new(wake))
    }

    /// A wake that does nothing, for callers that poll the services'
    /// `generation()` counters or wait on tasks instead.
    pub fn none() -> Wake {
        Wake(Arc::new(|| {}))
    }

    /// Wake the main loop.
    pub fn wake(&self) {
        (self.0)()
    }
}

impl std::fmt::Debug for Wake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Wake")
    }
}

impl Default for Wake {
    fn default() -> Wake {
        Wake::none()
    }
}

/// Lock a mutex, ignoring poisoning: a panicked worker must not take the
/// UI down with it.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// `bytes` random bytes as lowercase hex, from `/dev/urandom` (or, if that
/// can't be read, from the clock, process ID and a counter): `aizome-sys`'s
/// launch tokens and unit names.
#[doc(hidden)]
pub fn random_hex(bytes: usize) -> String {
    use std::io::Read;
    let mut buffer = vec![0u8; bytes];
    let read = std::fs::File::open("/dev/urandom").and_then(|mut file| file.read_exact(&mut buffer));
    if read.is_err() {
        use std::hash::{BuildHasher, Hasher};
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u128(
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos(),
        );
        hasher.write_u32(std::process::id());
        hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
        let mut state = hasher.finish();
        for byte in &mut buffer {
            *byte = state as u8;
            state = state.rotate_right(8) ^ 0x9e37_79b9_7f4a_7c15;
        }
    }
    buffer.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Test fixtures: directories under the build directory, removed when
/// dropped.
#[cfg(any(test, feature = "test-util"))]
#[doc(hidden)]
pub mod testutil {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub struct TempDir(PathBuf);

    impl TempDir {
        /// A new empty directory under `$CARGO_TARGET_DIR/test-tmp` (or the
        /// system's temporary directory).
        pub fn new(name: &str) -> TempDir {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .filter(|dir| dir.is_absolute())
                .unwrap_or_else(std::env::temp_dir)
                .join("test-tmp");
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!("{name}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create a test directory");
            TempDir(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }

        /// Write `contents` to `relative` (creating directories); returns
        /// the full path.
        pub fn write(&self, relative: &str, contents: &str) -> PathBuf {
            let path = self.0.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create fixture directories");
            }
            std::fs::write(&path, contents).expect("write a fixture");
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn random_hex_is_hex_and_varies() {
        let a = super::random_hex(8);
        let b = super::random_hex(8);
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_ne!(a, b);
    }
}
