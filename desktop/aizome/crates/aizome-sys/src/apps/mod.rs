//! Applications: what's installed, how to start it, its icons, and which
//! one opens what.
//!
//! - [`DesktopEntry`] parses `.desktop` files (Desktop Entry Specification
//!   1.5), with localised values for a [`Locale`].
//! - [`AppIndex`] finds every entry in the XDG data directories, keeps
//!   them by desktop file ID, notices changes cheaply (NixOS-aware), and
//!   matches windows to apps ([`AppIndex::match_window`]).
//! - [`Exec`] expands `Exec` lines into argument vectors; [`launch()`] runs
//!   them in a systemd scope (or directly, as a fallback).
//! - [`IconLookup`] finds icon files (Icon Theme Specification), caching
//!   what it reads.
//! - [`AppIndex::default_for`] and friends answer and change default
//!   applications (MIME Applications Associations Specification), by MIME
//!   type or by [`DefaultRole`]; the default terminal follows the
//!   xdg-terminal-exec convention.
//!
//! Everything that reads the file system takes its directories from an
//! [`XdgDirs`], so tests and tools can point it at any tree. Nothing here
//! talks to D-Bus; nothing blocks for long except where documented
//! ([`launch()`] waits a few milliseconds for systemd).

mod entry;
mod exec;
mod icons;
mod index;
mod keyfile;
mod launch;
mod mime;
mod xdg;

pub use entry::{Action, DesktopEntry, EntryKind, Locale};
pub use exec::{Exec, ExecContext, file_uri_to_path, path_to_file_uri};
pub use icons::{IconLookup, IconTheme};
pub use index::{App, AppIndex, Watcher};
pub use launch::{
    LaunchEvent, LaunchObserver, LaunchOptions, Launched, activate_dbus, command_lines, dbus_object_path, launch,
    launch_command_observed, launch_in_background, launch_observed, systemd_escape, unit_name,
};
pub use mime::DefaultRole;
pub use xdg::{DESKTOP_NAME, XdgDirs, find_program, parse_desktops};

/// `bytes` random bytes as lowercase hex, from `/dev/urandom` (or, if that
/// can't be read, from the clock, process ID and a counter).
pub(crate) fn random_hex(bytes: usize) -> String {
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
#[cfg(test)]
pub(crate) mod testutil {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub(crate) struct TempDir(PathBuf);

    impl TempDir {
        /// A new empty directory under `$CARGO_TARGET_DIR/test-tmp` (or the
        /// system's temporary directory).
        pub(crate) fn new(name: &str) -> TempDir {
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

        pub(crate) fn path(&self) -> &Path {
            &self.0
        }

        /// Write `contents` to `relative` (creating directories); returns
        /// the full path.
        pub(crate) fn write(&self, relative: &str, contents: &str) -> PathBuf {
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
