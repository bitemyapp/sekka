//! The scanner: walks a folder on several threads at once into a [`Tree`].
//!
//! - It counts allocated size (`st_blocks` × 512, what deleting frees) as
//!   well as apparent size (`st_size`).
//! - A file with several hard links counts once, the first time it's seen;
//!   its other names show with size zero.
//! - It stays on the root's filesystem unless asked
//!   ([`ScanOptions::cross_filesystems`]), and even then never enters
//!   pseudo-filesystems (`/proc`, `/sys`, …).
//! - It never follows a symlink: a link is a small node of its own.
//! - A folder it may not read is marked ([`flags::ERROR`]) and counted
//!   in [`Progress::errors`]; the scan goes on.
//! - It reports progress through [`Progress`] (atomics any thread can
//!   read), and stops soon after [`Progress::cancel`].
//!
//! Workers share a stack of folders still to read (depth first, so it
//! stays short) and append each folder's entries to the arena in one go,
//! taking its lock once per folder.

use std::collections::HashSet;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::mounts;
use crate::tree::{Entry, Kind, NodeId, Tree, flags};

/// What to scan, and how.
#[derive(Clone, Debug)]
pub struct ScanOptions {
    pub root: PathBuf,
    /// Enter folders on other filesystems (never pseudo-filesystems).
    pub cross_filesystems: bool,
    /// Worker threads; 0 picks from the CPUs.
    pub threads: usize,
    /// Folders not to enter (shown, marked [`flags::SKIPPED`]).
    pub skip: Vec<PathBuf>,
}

impl ScanOptions {
    pub fn new(root: impl Into<PathBuf>) -> ScanOptions {
        ScanOptions { root: root.into(), cross_filesystems: false, threads: 0, skip: Vec::new() }
    }
}

/// A scan under way, as any thread sees it.
#[derive(Debug, Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    /// Allocated bytes seen so far.
    pub bytes: AtomicU64,
    pub errors: AtomicU64,
    cancel: AtomicBool,
    current: Mutex<PathBuf>,
}

impl Progress {
    pub fn new() -> Arc<Progress> {
        Arc::new(Progress::default())
    }

    /// Ask the scan to stop.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// A folder being read lately.
    pub fn current(&self) -> PathBuf {
        self.current.lock().map(|p| p.clone()).unwrap_or_default()
    }

    pub fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            files: self.files.load(Ordering::Relaxed),
            dirs: self.dirs.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            current: self.current(),
        }
    }
}

/// Progress at a moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProgressSnapshot {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub errors: u64,
    pub current: PathBuf,
}

/// A path that couldn't be read, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct ScanError {
    pub path: PathBuf,
    pub error: String,
}

/// What a finished scan found.
#[derive(Clone, Debug)]
pub struct Scan {
    pub tree: Tree,
    pub options: ScanOptions,
    /// When it started, as Unix time.
    pub started: i64,
    pub duration: Duration,
    /// Every error is counted; the first [`MAX_ERRORS`] are kept.
    pub error_count: u64,
    pub errors: Vec<ScanError>,
    /// Folders on other filesystems it didn't enter.
    pub other_filesystems: Vec<PathBuf>,
    /// Files counted once though they have more names.
    pub hard_links: u64,
    pub threads: usize,
}

/// How many errors a scan keeps (it counts them all).
pub const MAX_ERRORS: usize = 200;

/// Why a scan didn't finish.
#[derive(Debug)]
pub enum Failure {
    Cancelled,
    /// The root itself couldn't be read.
    Root(std::io::Error),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Cancelled => f.write_str("the scan was stopped"),
            Failure::Root(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Failure {}

/// A folder waiting to be read.
struct Job {
    node: NodeId,
    path: PathBuf,
}

/// What the workers share.
struct Shared {
    tree: Mutex<Tree>,
    queue: Mutex<Queue>,
    wake: Condvar,
    progress: Arc<Progress>,
    root_dev: u64,
    cross: bool,
    pseudo: HashSet<PathBuf>,
    skip: HashSet<PathBuf>,
    inodes: Vec<Mutex<HashSet<(u64, u64)>>>,
    hard_links: AtomicU64,
    errors: Mutex<Vec<ScanError>>,
    other_fs: Mutex<Vec<PathBuf>>,
}

struct Queue {
    jobs: Vec<Job>,
    /// Workers reading a folder now.
    busy: usize,
    done: bool,
}

const INODE_SHARDS: usize = 64;

/// Scan `options.root`, reporting to `progress`.
pub fn scan(options: ScanOptions, progress: Arc<Progress>) -> Result<Scan, Failure> {
    let start = Instant::now();
    let started = crate::time::now_unix();
    let root = fs::canonicalize(&options.root).unwrap_or_else(|_| options.root.clone());
    let meta = fs::symlink_metadata(&root).map_err(Failure::Root)?;
    let kind = kind_of(&meta.file_type());
    let mut tree = Tree::new(root.clone(), kind, alloc_of(&meta), meta.size(), meta.mtime());
    let threads = match options.threads {
        0 => std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 16),
        n => n,
    };
    if kind != Kind::Dir {
        progress.files.fetch_add(1, Ordering::Relaxed);
        progress.bytes.fetch_add(alloc_of(&meta), Ordering::Relaxed);
        tree.finish();
        return Ok(Scan {
            tree,
            options: ScanOptions { root, ..options },
            started,
            duration: start.elapsed(),
            error_count: 0,
            errors: Vec::new(),
            other_filesystems: Vec::new(),
            hard_links: 0,
            threads: 1,
        });
    }
    progress.dirs.fetch_add(1, Ordering::Relaxed);
    progress.bytes.fetch_add(alloc_of(&meta), Ordering::Relaxed);

    let shared = Arc::new(Shared {
        tree: Mutex::new(tree),
        queue: Mutex::new(Queue { jobs: vec![Job { node: NodeId::ROOT, path: root.clone() }], busy: 0, done: false }),
        wake: Condvar::new(),
        progress: progress.clone(),
        root_dev: meta.dev(),
        cross: options.cross_filesystems,
        pseudo: mounts::pseudo_mount_points(),
        skip: options.skip.iter().cloned().collect(),
        inodes: (0..INODE_SHARDS).map(|_| Mutex::new(HashSet::new())).collect(),
        hard_links: AtomicU64::new(0),
        errors: Mutex::new(Vec::new()),
        other_fs: Mutex::new(Vec::new()),
    });

    let handles: Vec<_> = (0..threads)
        .map(|i| {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name(format!("kura-scan-{i}"))
                .spawn(move || worker(&shared))
                .expect("a scan thread")
        })
        .collect();
    for h in handles {
        let _ = h.join();
    }
    if progress.is_cancelled() {
        return Err(Failure::Cancelled);
    }
    let shared = Arc::try_unwrap(shared).ok().expect("the workers are done");
    let mut tree = shared.tree.into_inner().unwrap_or_else(|e| e.into_inner());
    tree.finish();
    let mut errors = shared.errors.into_inner().unwrap_or_else(|e| e.into_inner());
    errors.sort_by(|a, b| a.path.cmp(&b.path));
    let mut other_filesystems = shared.other_fs.into_inner().unwrap_or_else(|e| e.into_inner());
    other_filesystems.sort();
    Ok(Scan {
        tree,
        options: ScanOptions { root, ..options },
        started,
        duration: start.elapsed(),
        error_count: progress.errors.load(Ordering::Relaxed),
        errors,
        other_filesystems,
        hard_links: shared.hard_links.load(Ordering::Relaxed),
        threads,
    })
}

fn worker(shared: &Shared) {
    let mut entries: Vec<Entry> = Vec::with_capacity(256);
    let mut subdirs: Vec<(usize, PathBuf)> = Vec::new();
    loop {
        let job = {
            let mut q = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if q.done {
                    return;
                }
                if shared.progress.is_cancelled() {
                    q.done = true;
                    q.jobs.clear();
                    shared.wake.notify_all();
                    return;
                }
                if let Some(job) = q.jobs.pop() {
                    q.busy += 1;
                    break job;
                }
                if q.busy == 0 {
                    q.done = true;
                    shared.wake.notify_all();
                    return;
                }
                q = shared.wake.wait(q).unwrap_or_else(|e| e.into_inner());
            }
        };
        entries.clear();
        subdirs.clear();
        read_dir(shared, &job, &mut entries, &mut subdirs);
        let first = {
            let mut tree = shared.tree.lock().unwrap_or_else(|e| e.into_inner());
            if entries.is_empty() { 0 } else { tree.push_children(job.node, &entries) }
        };
        let mut q = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        for (i, path) in subdirs.drain(..) {
            q.jobs.push(Job { node: NodeId(first + i as u32), path });
        }
        q.busy -= 1;
        // One waiter per new folder, or everyone if this was the last.
        if q.jobs.is_empty() && q.busy == 0 {
            shared.wake.notify_all();
        } else {
            for _ in 0..q.jobs.len().min(16) {
                shared.wake.notify_one();
            }
        }
    }
}

/// Read one folder's entries; the folders among them to read next go in
/// `subdirs` (by their place in `entries`).
fn read_dir(shared: &Shared, job: &Job, entries: &mut Vec<Entry>, subdirs: &mut Vec<(usize, PathBuf)>) {
    let progress = &shared.progress;
    if let Ok(mut current) = progress.current.try_lock() {
        current.clone_from(&job.path);
    }
    let rd = match fs::read_dir(&job.path) {
        Ok(rd) => rd,
        Err(e) => {
            error(shared, &job.path, &e);
            shared.tree.lock().unwrap_or_else(|e| e.into_inner()).add_flag(job.node, flags::ERROR);
            return;
        }
    };
    let (mut files, mut dirs, mut bytes) = (0u64, 0u64, 0u64);
    for item in rd {
        if progress.is_cancelled() {
            return;
        }
        let item = match item {
            Ok(i) => i,
            Err(e) => {
                error(shared, &job.path, &e);
                shared.tree.lock().unwrap_or_else(|e| e.into_inner()).add_flag(job.node, flags::ERROR);
                continue;
            }
        };
        let name = item.file_name();
        let meta = match item.metadata() {
            Ok(m) => m,
            Err(e) => {
                let path = job.path.join(&name);
                error(shared, &path, &e);
                entries.push(Entry {
                    name: name.as_bytes().to_vec(),
                    kind: Kind::Other,
                    flags: flags::ERROR,
                    alloc: 0,
                    apparent: 0,
                    mtime: 0,
                });
                files += 1;
                continue;
            }
        };
        let kind = kind_of(&meta.file_type());
        let mut entry = Entry {
            name: name.as_bytes().to_vec(),
            kind,
            flags: 0,
            alloc: alloc_of(&meta),
            apparent: meta.size(),
            mtime: meta.mtime(),
        };
        match kind {
            Kind::Dir => {
                dirs += 1;
                let path = job.path.join(&name);
                if shared.skip.contains(&path) {
                    entry.flags |= flags::SKIPPED;
                } else if meta.dev() != shared.root_dev && (!shared.cross || shared.pseudo.contains(&path)) {
                    entry.flags |= flags::MOUNT;
                    // A mount point's own inode belongs to the other
                    // filesystem: it frees nothing here.
                    entry.alloc = 0;
                    entry.apparent = 0;
                    if let Ok(mut v) = shared.other_fs.lock() {
                        v.push(path);
                    }
                } else {
                    subdirs.push((entries.len(), path));
                }
            }
            _ => {
                files += 1;
                if kind == Kind::File && meta.nlink() > 1 && !first_sight(shared, meta.dev(), meta.ino()) {
                    entry.flags |= flags::HARDLINK;
                    entry.alloc = 0;
                    entry.apparent = 0;
                    shared.hard_links.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        bytes += entry.alloc;
        entries.push(entry);
    }
    progress.files.fetch_add(files, Ordering::Relaxed);
    progress.dirs.fetch_add(dirs, Ordering::Relaxed);
    progress.bytes.fetch_add(bytes, Ordering::Relaxed);
}

/// Whether this is the first name of a hard-linked file the scan meets.
fn first_sight(shared: &Shared, dev: u64, ino: u64) -> bool {
    let shard = (ino as usize ^ (dev as usize).rotate_left(17)) % INODE_SHARDS;
    shared.inodes[shard].lock().unwrap_or_else(|e| e.into_inner()).insert((dev, ino))
}

fn error(shared: &Shared, path: &Path, e: &std::io::Error) {
    shared.progress.errors.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut v) = shared.errors.lock()
        && v.len() < MAX_ERRORS
    {
        v.push(ScanError { path: path.to_path_buf(), error: e.to_string() });
    }
}

pub fn kind_of(t: &fs::FileType) -> Kind {
    if t.is_symlink() {
        Kind::Symlink
    } else if t.is_dir() {
        Kind::Dir
    } else if t.is_file() {
        Kind::File
    } else {
        Kind::Other
    }
}

/// Bytes the filesystem gave it.
pub fn alloc_of(m: &fs::Metadata) -> u64 {
    m.blocks() * 512
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_a_fixture() {
        let dir = crate::testdir("scan-fixture");
        fs::create_dir_all(dir.join("a/b/c")).unwrap();
        fs::write(dir.join("a/one"), vec![1u8; 10_000]).unwrap();
        fs::write(dir.join("a/b/two"), vec![2u8; 50_000]).unwrap();
        fs::write(dir.join("a/b/c/three"), vec![3u8; 5]).unwrap();
        fs::hard_link(dir.join("a/b/two"), dir.join("a/two-again")).unwrap();
        std::os::unix::fs::symlink("/", dir.join("a/to-root")).unwrap();
        std::os::unix::fs::symlink(dir.join("a"), dir.join("a/b/loop")).unwrap();

        let progress = Progress::new();
        let s = scan(ScanOptions::new(&dir), progress.clone()).unwrap();
        let t = &s.tree;
        let root = t.root();
        assert_eq!(t.node(root).items, 9);
        assert_eq!(s.hard_links, 1);
        let apparent = t.node(root).apparent;
        // The link's target counts once; symlinks count their own length.
        let links = dir.join("a").as_os_str().len() as u64 + 1;
        let dirs: u64 =
            ["", "a", "a/b", "a/b/c"].iter().map(|d| fs::symlink_metadata(dir.join(d)).unwrap().size()).sum();
        assert_eq!(apparent, 10_000 + 50_000 + 5 + links + dirs);
        // Whichever name of the hard-linked file came first holds its size.
        let again = t.node(t.find(&dir.join("a/two-again")).unwrap()).apparent;
        let two = t.node(t.find(&dir.join("a/b/two")).unwrap()).apparent;
        assert_eq!(again + two, 50_000);
        let a = t.child_named(root, "a").unwrap();
        let first = t.children(a).next().unwrap();
        assert!(t.name(first) == "b" || t.name(first) == "two-again");
        let hardlinked: Vec<_> = t.nodes().iter().filter(|n| n.has(flags::HARDLINK)).collect();
        assert_eq!(hardlinked.len(), 1);
        assert_eq!(progress.snapshot().files, 6);
        assert_eq!(progress.snapshot().dirs, 4);
        assert!(t.node(t.find(&dir.join("a/to-root")).unwrap()).kind == Kind::Symlink);
    }

    #[test]
    fn unreadable_folders_are_marked_not_fatal() {
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::testdir("scan-denied");
        fs::create_dir_all(dir.join("open")).unwrap();
        fs::create_dir_all(dir.join("shut/inner")).unwrap();
        fs::write(dir.join("open/f"), b"x").unwrap();
        fs::set_permissions(dir.join("shut"), fs::Permissions::from_mode(0o000)).unwrap();
        let s = scan(ScanOptions::new(&dir), Progress::new());
        fs::set_permissions(dir.join("shut"), fs::Permissions::from_mode(0o755)).unwrap();
        let s = s.unwrap();
        // Root can read anything; elsewhere the folder is marked.
        if unsafe { libc::geteuid() } != 0 {
            let shut = s.tree.find(&dir.join("shut")).unwrap();
            assert!(s.tree.node(shut).has(flags::ERROR));
            assert_eq!(s.error_count, 1);
            assert_eq!(s.errors[0].path, dir.join("shut"));
        }
        assert!(s.tree.find(&dir.join("open/f")).is_some());
    }

    #[test]
    fn cancelling_stops_the_scan() {
        let dir = crate::testdir("scan-cancel");
        for i in 0..50 {
            fs::create_dir_all(dir.join(format!("d{i}/e"))).unwrap();
        }
        let p = Progress::new();
        p.cancel();
        assert!(matches!(scan(ScanOptions::new(&dir), p), Err(Failure::Cancelled)));
    }

    #[test]
    fn skips_what_it_is_told() {
        let dir = crate::testdir("scan-skip");
        fs::create_dir_all(dir.join("keep")).unwrap();
        fs::create_dir_all(dir.join("skip/deep")).unwrap();
        fs::write(dir.join("skip/deep/big"), vec![0u8; 100_000]).unwrap();
        let mut o = ScanOptions::new(&dir);
        o.skip.push(fs::canonicalize(&dir).unwrap().join("skip"));
        let s = scan(o, Progress::new()).unwrap();
        let skip = s.tree.find(&fs::canonicalize(&dir).unwrap().join("skip")).unwrap();
        assert!(s.tree.node(skip).has(flags::SKIPPED));
        assert_eq!(s.tree.node(skip).count, 0);
    }
}
