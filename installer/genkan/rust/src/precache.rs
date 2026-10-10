// SPDX-License-Identifier: GPL-3.0-or-later
//! Warm the page cache with Nix store paths before the disk is erased.
//!
//! The live store is a squashfs image on the installation USB stick. Reading a
//! file once leaves its decompressed pages in RAM, so copying the installed
//! system after confirmation is limited by the target disk instead of the
//! stick. Page cache is reclaimable: warming never pins memory. It reads at
//! most what was available when it started, less a reserve, so it does not
//! push the live session into reclaim or evict its own earlier work. The
//! compressed image pages read along the way are released immediately.
use crate::memory;
use anyhow::{Context, Result};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Bytes read during this call.
    pub bytes: u64,
    /// Bytes of regular files below the requested roots.
    pub total: u64,
    pub files: u64,
    /// Warming stopped early to keep the memory reserve free.
    pub limited: bool,
}

struct File {
    path: PathBuf,
    bytes: u64,
}

/// The live store's compressed image on the installation media.
const SQUASHFS: &str = "/iso/nix-store.squashfs";

fn drop_cached(file: Option<&fs::File>) {
    // Only clean, unmapped pages are released; a failure only loses memory.
    if let Some(file) = file {
        unsafe {
            libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
        }
    }
}

/// Only absolute, normalized paths directly inside /nix/store are accepted;
/// path lists come from Nix or root-owned installation media.
pub fn store_path(line: &str) -> Option<PathBuf> {
    let path = Path::new(line.trim());
    let name = path.strip_prefix("/nix/store/").ok()?;
    let mut components = name.components();
    match (components.next(), components.next()) {
        (Some(std::path::Component::Normal(_)), None) => Some(path.to_path_buf()),
        _ => None,
    }
}

/// Store path lists shipped on the installation media's ISO 9660 filesystem
/// (outside the Nix store, so they add no references to the live system):
/// the closures of prebuilt reference systems for each desktop and of each
/// application.
pub const REFERENCE_LISTS: &str = "/iso/calamares/closures";

/// Existing reference lists for a selection. Warming these while the user is
/// still choosing caches nearly the whole eventual system: only small
/// per-machine derivations (configuration, initrd) differ.
pub fn reference_lists(desktops: &[crate::Desktop], applications: &[String]) -> Vec<PathBuf> {
    let dir = Path::new(REFERENCE_LISTS);
    desktops
        .iter()
        .map(|desktop| dir.join(format!("desktop-{}.paths", desktop.id())))
        .chain(
            applications
                .iter()
                .filter(|id| {
                    id.bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                })
                .map(|id| dir.join(format!("application-{id}.paths"))),
        )
        .filter(|path| path.is_file())
        .collect()
}

/// Read newline-separated store path lists (e.g. `closureInfo` output) and
/// return their unique valid entries.
pub fn read_lists(files: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut paths = BTreeSet::new();
    for file in files {
        let text = fs::read_to_string(file)
            .with_context(|| format!("Reading store path list {}", file.display()))?;
        paths.extend(text.lines().filter_map(store_path));
    }
    Ok(paths.into_iter().collect())
}

fn collect(root: &Path, files: &mut Vec<File>, cancel: &AtomicBool) {
    // Never follow symlinks: store paths link to each other and to /run.
    let Ok(meta) = fs::symlink_metadata(root) else {
        return;
    };
    if meta.is_file() {
        files.push(File {
            path: root.to_path_buf(),
            bytes: meta.size(),
        });
    } else if meta.is_dir() {
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            collect(&entry.path(), files, cancel);
        }
    }
}

fn read_file(path: &Path, buffer: &mut [u8], read: &AtomicU64) -> std::io::Result<()> {
    let mut file = fs::File::open(path)?;
    // Hint large sequential readahead; a failure only loses the hint.
    unsafe {
        libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_SEQUENTIAL);
    }
    loop {
        let n = file.read(buffer)?;
        if n == 0 {
            return Ok(());
        }
        read.fetch_add(n as u64, Ordering::Relaxed);
    }
}

/// Read every regular file below `roots` until done, cancelled, or free memory
/// falls to `memory::reserve`. `progress(read, total)` runs on the calling
/// thread at most a few times per second.
pub fn warm(
    roots: &[PathBuf],
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
) -> Result<Outcome> {
    let mut files = Vec::new();
    for root in roots {
        collect(root, &mut files, cancel);
    }
    let total = files.iter().map(|f| f.bytes).sum();
    let info = memory::read()?;
    let reserve = memory::reserve(info.total);
    let budget = info.available.saturating_sub(reserve);
    // Reading through the loop-mounted squashfs also caches its compressed
    // backing pages. Once decompressed they are redundant: drop them as we go
    // so the budget holds decompressed data. Missing on non-ISO systems.
    let compressed = fs::File::open(SQUASHFS).ok();
    let next = AtomicUsize::new(0);
    let read = AtomicU64::new(0);
    let done = AtomicUsize::new(0);
    let limited = AtomicBool::new(false);
    let stop = AtomicBool::new(false);
    let first_error = Mutex::new(None::<std::io::Error>);
    // Squashfs is mounted with threads=multi: parallel readers decompress in
    // parallel. More readers than this only add USB queueing.
    let workers = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 8);
    let active = AtomicUsize::new(workers);
    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                let mut buffer = vec![0u8; 1 << 20];
                loop {
                    if stop.load(Ordering::Relaxed) || cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(file) = files.get(index) else {
                        break;
                    };
                    // A store path can disappear (garbage collection) or be
                    // unreadable; warming is an optimization, never a failure.
                    if let Err(error) = read_file(&file.path, &mut buffer, &read)
                        && error.kind() != std::io::ErrorKind::NotFound
                    {
                        first_error.lock().unwrap().get_or_insert(error);
                    }
                    done.fetch_add(1, Ordering::Relaxed);
                }
                active.fetch_sub(1, Ordering::Release);
            });
        }
        let mut last = Instant::now();
        while active.load(Ordering::Acquire) > 0 {
            thread::sleep(Duration::from_millis(50));
            // Stop at the budget, or sooner if the live session's own memory
            // use (tmpfs store writes, applications) grew in the meantime.
            let over_budget = read.load(Ordering::Relaxed) >= budget;
            let squeezed = memory::read().map_or(true, |now| now.available <= reserve);
            if (over_budget || squeezed) && !stop.swap(true, Ordering::Relaxed) {
                limited.store(true, Ordering::Relaxed);
            }
            if last.elapsed() >= Duration::from_millis(250) {
                drop_cached(compressed.as_ref());
                progress(read.load(Ordering::Relaxed), total);
                last = Instant::now();
            }
        }
    });
    drop_cached(compressed.as_ref());
    let outcome = Outcome {
        bytes: read.load(Ordering::Relaxed),
        total,
        files: done.load(Ordering::Relaxed) as u64,
        limited: limited.load(Ordering::Relaxed),
    };
    progress(outcome.bytes, total);
    if let Some(error) = first_error.into_inner().unwrap()
        && outcome.files == 0
    {
        return Err(error).context("Could not read any store files for caching");
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_top_level_store_paths_are_accepted() {
        assert_eq!(
            store_path("/nix/store/abc-hello\n"),
            Some(PathBuf::from("/nix/store/abc-hello"))
        );
        for bad in [
            "",
            "/nix/store",
            "/nix/store/",
            "/nix/store/abc/def",
            "/nix/store/../etc",
            "nix/store/abc",
            "/etc/passwd",
        ] {
            assert_eq!(store_path(bad), None, "{bad}");
        }
    }
    #[test]
    fn warms_regular_files_without_following_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("a"), vec![1u8; 3000]).unwrap();
        fs::write(dir.path().join("sub/b"), vec![2u8; 5000]).unwrap();
        std::os::unix::fs::symlink("/", dir.path().join("root-link")).unwrap();
        let cancel = AtomicBool::new(false);
        let mut reports = 0;
        let outcome = warm(&[dir.path().to_path_buf()], &cancel, |_, total| {
            assert_eq!(total, 8000);
            reports += 1;
        })
        .unwrap();
        assert_eq!(outcome.total, 8000);
        assert!(outcome.limited || (outcome.bytes == 8000 && outcome.files == 2));
        assert!(reports >= 1);
        cancel.store(true, Ordering::Relaxed);
        let cancelled = warm(&[dir.path().to_path_buf()], &cancel, |_, _| {}).unwrap();
        assert_eq!(cancelled.bytes, 0);
    }
}
