//! Kura 蔵 (Disk Usage): what fills a disk, for people and for agents.
//!
//! The library is everything that works without a display: the parallel
//! [`scan`]ner into a compact [`tree`], the [`hints`] about what could be
//! freed, the agents' [`index`] (`index.json`) and its [`report`]
//! (`index.md`), the freedesktop [`trash`], [`mounts`] and [`nix`]. The
//! window (`ui`) and the command line (`cli`) are built on it.

pub mod cli;
pub mod dupes;
pub mod format;
pub mod hints;
pub mod index;
pub mod kinds;
pub mod mounts;
pub mod nix;
pub mod report;
pub mod scan;
pub mod time;
pub mod trash;
pub mod tree;
#[cfg(feature = "gui")]
pub mod ui;

use std::path::{Path, PathBuf};

pub const APP_ID: &str = "org.aizome.Kura";

/// The home folder.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// An XDG base directory: `$var` if set to an absolute path, else
/// `~/<fallback>`.
pub fn xdg_dir(var: &str, fallback: &str) -> Option<PathBuf> {
    match std::env::var_os(var).map(PathBuf::from) {
        Some(p) if p.is_absolute() => Some(p),
        _ => home_dir().map(|h| h.join(fallback)),
    }
}

/// The Downloads folder: `XDG_DOWNLOAD_DIR` from `user-dirs.dirs`, else
/// `~/Downloads`.
pub fn downloads_dir() -> Option<PathBuf> {
    let home = home_dir()?;
    let dirs = xdg_dir("XDG_CONFIG_HOME", ".config").map(|c| c.join("user-dirs.dirs"));
    if let Some(text) = dirs.and_then(|d| std::fs::read_to_string(d).ok()) {
        for line in text.lines() {
            if let Some(v) = line.trim().strip_prefix("XDG_DOWNLOAD_DIR=") {
                let v = v.trim().trim_matches('"').replace("$HOME", &home.to_string_lossy());
                let p = PathBuf::from(v);
                if p.is_absolute() && p != home {
                    return Some(p);
                }
            }
        }
    }
    Some(home.join("Downloads"))
}

/// Write `bytes` to `path` through a temporary file beside it, so readers
/// see the old file or the new one, never half of one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(),
        std::process::id()
    ));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Everything after a scan: the hints, the index, and (if `dir` is given)
/// `index.json` and `index.md` written there.
pub fn analyse(scan: &scan::Scan, cx: &hints::Context, limits: Option<index::TreeLimits>) -> index::Index {
    let hints = hints::detect(scan, cx);
    index::build(scan, hints, cx, limits)
}

/// A context for hints with Nix's profiles and (if `measure_store`) the
/// store's size, cached in `cache_dir`.
pub fn context(measure_store: bool, cache_dir: Option<&Path>) -> hints::Context {
    let mut cx = hints::Context::from_env();
    if nix::has_store() {
        cx.nix_profiles = nix::profiles();
        if measure_store {
            let cache = cache_dir.map(|d| d.join("nix-store.json"));
            cx.nix_store = nix::store_size(cache.as_deref());
        }
    }
    cx
}

/// A fresh folder for a test, under the system's temporary folder.
#[cfg(test)]
pub(crate) fn testdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kura-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a test folder");
    std::fs::canonicalize(&dir).expect("its real path")
}
