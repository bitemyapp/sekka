//! The index of installed applications: every desktop entry in the XDG
//! data directories, by desktop file ID, as the Desktop Entry
//! Specification 1.5 ("Desktop File ID") and the Base Directory
//! Specification define them.
//!
//! - Directories: `$XDG_DATA_HOME/applications`, then each
//!   `$XDG_DATA_DIRS` entry's `applications`, most important first. A
//!   directory reached twice (the same canonical path) is read once.
//! - IDs: the path below `applications/` with `/` turned into `-`
//!   (`kde4/konsole.desktop` → `kde4-konsole.desktop`). The first file with
//!   an ID wins; a `Hidden=true` one deletes the ID. A file that can't be
//!   parsed doesn't count, so a lower one is used.
//! - Change detection: a [`Signature`] of what the index was read from,
//!   cheap enough to recompute every few seconds. On NixOS the data
//!   directories are symlinks into `/nix/store`, where every mtime is 1,
//!   and a rebuild changes where the symlink points, not any mtime. So each
//!   directory is stamped with its canonical path (and device, inode and
//!   mtime); directories outside the store also with their
//!   subdirectories' stamps, and writable ones with every file's. The MIME
//!   and terminal lists under the config directories are stamped file by
//!   file.
//! - [`AppIndex::watch`] polls that signature on a thread (every 2 s, and
//!   at once on inotify events in the writable directories) and calls a
//!   [`Wake`] when it changes; the UI then calls
//!   [`AppIndex::refresh_if_changed`].
//!
//! Window matching ([`AppIndex::match_window`]) tries, in order: the exact
//! desktop file ID, the ID ignoring case, `StartupWMClass` ignoring case,
//! the last component of a reverse-DNS ID (either side), and the `Exec`
//! program's file name (with NixOS's `.foo-wrapped` wrappers unwrapped).
//! Within a step, entries shown in menus win over hidden ones, then more
//! important directories.

use super::entry::{DesktopEntry, EntryKind, Locale};
use super::mime::MimeData;
use super::xdg::XdgDirs;
use crate::Wake;
use std::collections::{HashMap, HashSet};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// An installed application: a desktop entry the index found.
pub type App = DesktopEntry;

/// How deep to follow subdirectories of an `applications` directory.
const MAX_DEPTH: usize = 8;

/// What the index knows about each entry besides the entry itself.
#[derive(Clone, Debug)]
struct Info {
    /// Which application directory (index into `AppIndex::app_dirs`).
    level: usize,
    /// `Type=Application` and `TryExec` satisfied.
    installed: bool,
    /// Shown in menus on the current desktops ([`AppIndex::apps`]).
    shown: bool,
    /// `name` lowercased, for sorting.
    sort_key: String,
    keys: MatchKeys,
}

/// Lowercased names a window's app ID can match.
#[derive(Clone, Debug, Default)]
struct MatchKeys {
    /// The ID without `.desktop`.
    id: String,
    wm_class: Option<String>,
    /// The ID's last dot-separated component.
    last: String,
    /// The `Exec` program's file name, unwrapped.
    exec: Option<String>,
}

/// The installed applications, by desktop file ID.
#[derive(Debug)]
pub struct AppIndex {
    pub(crate) dirs: XdgDirs,
    locale: Locale,
    /// The application directories read, most important first (missing
    /// ones and repeats dropped).
    pub(crate) app_dirs: Vec<PathBuf>,
    entries: Vec<App>,
    info: Vec<Info>,
    by_id: HashMap<String, usize>,
    /// Lowercased MIME type → entries listing it.
    by_mime: HashMap<String, Vec<usize>>,
    visible: Vec<usize>,
    pub(crate) mime: MimeData,
    signature: Signature,
}

impl AppIndex {
    /// Read the applications for this process's environment
    /// ([`XdgDirs::from_env`], [`Locale::from_env`]).
    pub fn load() -> AppIndex {
        AppIndex::load_with(XdgDirs::from_env(), Locale::from_env())
    }

    /// Read the applications under `dirs`, with names for `locale`.
    /// Missing directories and unreadable files are skipped.
    pub fn load_with(dirs: XdgDirs, locale: Locale) -> AppIndex {
        let signature = Signature::compute(&dirs);
        let mut index = AppIndex {
            dirs,
            locale,
            app_dirs: Vec::new(),
            entries: Vec::new(),
            info: Vec::new(),
            by_id: HashMap::new(),
            by_mime: HashMap::new(),
            visible: Vec::new(),
            mime: MimeData::default(),
            signature,
        };
        index.scan();
        index.mime = MimeData::load(&index.dirs, &index.app_dirs);
        index
    }

    fn scan(&mut self) {
        let mut app_dirs = Vec::new();
        let mut canonical_seen = HashSet::new();
        let mut consumed: HashSet<String> = HashSet::new();
        let mut found: Vec<(App, usize)> = Vec::new();
        for dir in self.dirs.application_dirs() {
            let Ok(canonical) = std::fs::canonicalize(&dir) else {
                continue;
            };
            if !canonical.is_dir() || !canonical_seen.insert(canonical) {
                continue;
            }
            let level = app_dirs.len();
            app_dirs.push(dir.clone());
            scan_dir(&dir, "", 0, &mut |id, path| {
                if consumed.contains(id) {
                    return;
                }
                match DesktopEntry::load(path, id, &self.locale) {
                    Ok(entry) => {
                        consumed.insert(id.to_owned());
                        if !entry.hidden {
                            found.push((entry, level));
                        }
                    }
                    Err(error) => log::debug!("skipping {}: {error}", path.display()),
                }
            });
        }
        found.sort_by(|a, b| a.0.id.cmp(&b.0.id));

        let desktops = &self.dirs.desktops;
        self.app_dirs = app_dirs;
        self.entries = Vec::with_capacity(found.len());
        self.info = Vec::with_capacity(found.len());
        self.by_id.clear();
        self.by_mime.clear();
        for (index, (entry, level)) in found.into_iter().enumerate() {
            let installed = entry.kind == EntryKind::Application && entry.try_exec_found();
            let info = Info {
                level,
                installed,
                shown: installed && entry.should_show(desktops),
                sort_key: entry.name.to_lowercase(),
                keys: MatchKeys::of(&entry),
            };
            self.by_id.insert(entry.id.clone(), index);
            for mime in &entry.mime_types {
                let list = self.by_mime.entry(mime.to_ascii_lowercase()).or_default();
                if !list.contains(&index) {
                    list.push(index);
                }
            }
            self.entries.push(entry);
            self.info.push(info);
        }
        let mut visible: Vec<usize> = (0..self.entries.len()).filter(|&i| self.info[i].shown).collect();
        visible.sort_by(|&a, &b| {
            (self.info[a].sort_key.as_str(), self.entries[a].id.as_str())
                .cmp(&(self.info[b].sort_key.as_str(), self.entries[b].id.as_str()))
        });
        self.visible = visible;
    }

    /// The applications to show in menus and launchers, sorted by name:
    /// `Type=Application`, not `NoDisplay`/`Hidden`, allowed on the current
    /// desktops, with `TryExec` satisfied and something to run.
    pub fn apps(&self) -> Vec<&App> {
        self.visible.iter().filter_map(|&i| self.entries.get(i)).collect()
    }

    /// Every entry found (any type, including `NoDisplay` ones and ones
    /// whose `TryExec` fails, but not `Hidden` ones), sorted by ID.
    pub fn all(&self) -> &[App] {
        &self.entries
    }

    /// The entry with this desktop file ID (`firefox.desktop`, or just
    /// `firefox`).
    pub fn get(&self, id: &str) -> Option<&App> {
        self.position(id).and_then(|i| self.entries.get(i))
    }

    pub(crate) fn position(&self, id: &str) -> Option<usize> {
        if let Some(&i) = self.by_id.get(id) {
            return Some(i);
        }
        if id.ends_with(".desktop") {
            return None;
        }
        self.by_id.get(&format!("{id}.desktop")).copied()
    }

    /// Whether `id` is an installed application: present, `Type=Application`
    /// and its `TryExec` satisfied. (`NoDisplay` ones count: they are
    /// often URL or MIME handlers.)
    pub fn is_installed(&self, id: &str) -> bool {
        self.position(id).is_some_and(|i| self.info[i].installed)
    }

    /// Whether `id` is shown in menus ([`apps`](Self::apps) lists it).
    pub fn is_shown(&self, id: &str) -> bool {
        self.position(id).is_some_and(|i| self.info[i].shown)
    }

    /// The application directory an entry came from, as an index into
    /// `app_dirs` (0 is the most important).
    pub(crate) fn level_of(&self, index: usize) -> Option<usize> {
        self.info.get(index).map(|info| info.level)
    }

    pub(crate) fn installed_at(&self, index: usize) -> bool {
        self.info.get(index).is_some_and(|info| info.installed)
    }

    pub(crate) fn shown_at(&self, index: usize) -> bool {
        self.info.get(index).is_some_and(|info| info.shown)
    }

    pub(crate) fn entry_at(&self, index: usize) -> Option<&App> {
        self.entries.get(index)
    }

    /// Entries whose `MimeType` lists `mime` (lowercase).
    pub(crate) fn listing_mime(&self, mime: &str) -> &[usize] {
        self.by_mime.get(mime).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The directories and desktop names the index reads.
    pub fn dirs(&self) -> &XdgDirs {
        &self.dirs
    }

    /// The locale names are chosen for.
    pub fn locale(&self) -> &Locale {
        &self.locale
    }

    /// Re-read everything if anything it was read from changed since the
    /// last load or refresh; `true` if it did. Cheap when nothing changed
    /// (a few hundred `stat`s at most), so it can run every few seconds.
    pub fn refresh_if_changed(&mut self) -> bool {
        let now = Signature::compute(&self.dirs);
        if now == self.signature {
            return false;
        }
        if now.apps != self.signature.apps {
            self.signature = now;
            self.scan();
        } else {
            self.signature = now;
        }
        self.mime = MimeData::load(&self.dirs, &self.app_dirs);
        true
    }

    /// Re-read the MIME and terminal lists after writing one, so the write
    /// isn't reported as an outside change.
    pub(crate) fn reload_lists(&mut self) {
        self.signature.lists = Signature::compute_lists(&self.dirs);
        self.mime = MimeData::load(&self.dirs, &self.app_dirs);
    }

    /// Watch for changes on a background thread: every 2 seconds (and soon
    /// after an inotify event in a writable directory) the signature is
    /// recomputed, and `wake` called if it differs from this index's. The
    /// UI then calls [`refresh_if_changed`](Self::refresh_if_changed).
    /// Dropping the [`Watcher`] stops the thread.
    pub fn watch(&self, wake: Wake) -> Watcher {
        Watcher::start(self.dirs.clone(), self.signature.clone(), wake, Duration::from_secs(2))
    }

    /// The application a window belongs to, from its Wayland app ID or X11
    /// `WM_CLASS`; see the module documentation for the rules.
    pub fn match_window(&self, app_id_or_wm_class: &str) -> Option<&App> {
        let wanted = app_id_or_wm_class.trim();
        if wanted.is_empty() {
            return None;
        }
        // 1. The exact desktop file ID.
        if let Some(i) = self.by_id.get(&format!("{wanted}.desktop")).or_else(|| self.by_id.get(wanted)) {
            return self.entries.get(*i);
        }
        let lower = wanted.to_lowercase();
        let lower = lower.strip_suffix(".desktop").unwrap_or(&lower);
        let last = lower.rsplit('.').next().unwrap_or(lower);
        let unwrapped = unwrap_program_name(lower);
        let steps: [&dyn Fn(&MatchKeys) -> bool; 5] = [
            // 2. The ID, ignoring case.
            &|keys: &MatchKeys| keys.id == lower,
            // 3. StartupWMClass, ignoring case.
            &|keys: &MatchKeys| keys.wm_class.as_deref() == Some(lower),
            // 4. Reverse-DNS: `nautilus` ↔ `org.gnome.Nautilus`, either way.
            &|keys: &MatchKeys| keys.last == lower || (lower.contains('.') && keys.id == last),
            &|keys: &MatchKeys| lower.contains('.') && keys.id.contains('.') && keys.last == last,
            // 5. The program's file name.
            &|keys: &MatchKeys| keys.exec.as_deref() == Some(unwrapped),
        ];
        for step in steps {
            let best = (0..self.entries.len())
                .filter(|&i| step(&self.info[i].keys))
                .min_by_key(|&i| (!self.info[i].shown, self.info[i].level, i));
            if let Some(i) = best {
                return self.entries.get(i);
            }
        }
        None
    }
}

impl MatchKeys {
    fn of(entry: &DesktopEntry) -> MatchKeys {
        let id = entry.app_id().to_lowercase();
        let last = id.rsplit('.').next().unwrap_or(&id).to_owned();
        MatchKeys {
            wm_class: entry.startup_wm_class.as_ref().map(|class| class.to_lowercase()),
            exec: entry.exec.as_deref().and_then(exec_program_name),
            last,
            id,
        }
    }
}

/// The file name of the program an `Exec` line runs, lowercased and
/// unwrapped, looking through `env [-u NAME] [-C DIR] VAR=value …`.
fn exec_program_name(exec: &str) -> Option<String> {
    let parsed = super::exec::Exec::parse(exec).ok()?;
    let argv = parsed.expand(&[], &super::exec::ExecContext::default());
    let mut words = argv.iter();
    let mut program = words.next()?;
    if file_name(program) == "env" {
        program = loop {
            let word = words.next()?;
            if word == "-u" || word == "-C" {
                words.next();
            } else if !word.contains('=') && !word.starts_with('-') {
                break word;
            }
        };
    }
    let name = file_name(program).to_lowercase();
    Some(unwrap_program_name(&name).to_owned()).filter(|name| !name.is_empty())
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `.firefox-wrapped` → `firefox`: NixOS's `wrapProgram` renames the
/// real binary, and some programs take their app ID from `argv[0]`.
fn unwrap_program_name(name: &str) -> &str {
    let mut name = name;
    while let Some(inner) = name.strip_prefix('.').and_then(|n| n.strip_suffix("-wrapped")) {
        name = inner;
    }
    name
}

/// Call `found(id, path)` for each `.desktop` file under `dir`, in name
/// order, with IDs prefixed by `prefix` (the parent directories joined by
/// `-`).
fn scan_dir(dir: &Path, prefix: &str, depth: usize, found: &mut dyn FnMut(&str, &Path)) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<(String, std::fs::DirEntry)> = read
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| Some((entry.file_name().into_string().ok()?, entry)))
        .collect();
    names.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, entry) in names {
        let path = entry.path();
        if name.ends_with(".desktop") && name.len() > ".desktop".len() {
            found(&format!("{prefix}{name}"), &path);
            continue;
        }
        if depth >= MAX_DEPTH || name.starts_with('.') {
            continue;
        }
        let is_dir = match entry.file_type() {
            Ok(kind) if kind.is_dir() => true,
            Ok(kind) if kind.is_symlink() => path.is_dir(),
            _ => false,
        };
        if is_dir {
            scan_dir(&path, &format!("{prefix}{name}-"), depth + 1, found);
        }
    }
}

/// A file's or directory's identity and modification stamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    dev: u64,
    ino: u64,
    mtime: i64,
    mtime_nsec: i64,
    size: u64,
}

impl Stamp {
    fn of(meta: &std::fs::Metadata) -> Stamp {
        Stamp {
            dev: meta.dev(),
            ino: meta.ino(),
            mtime: meta.mtime(),
            mtime_nsec: meta.mtime_nsec(),
            size: meta.size(),
        }
    }

    fn of_path(path: &Path) -> Option<Stamp> {
        std::fs::metadata(path).ok().map(|meta| Stamp::of(&meta))
    }
}

/// A directory tree's stamp: where it really is, and (outside the Nix
/// store) its subdirectories' stamps, and (when writable) every file's.
#[derive(Clone, Debug, PartialEq, Eq)]
struct DirStamp {
    path: PathBuf,
    canonical: Option<PathBuf>,
    stamp: Option<Stamp>,
    inside: Vec<(PathBuf, Stamp)>,
}

impl DirStamp {
    fn of(path: &Path) -> DirStamp {
        let canonical = std::fs::canonicalize(path).ok();
        let stamp = canonical.as_deref().and_then(Stamp::of_path);
        let mut inside = Vec::new();
        if let Some(real) = &canonical
            && !real.starts_with("/nix/store/")
            && stamp.is_some()
        {
            let files = is_writable(real);
            walk_stamps(real, Path::new(""), files, 0, &mut inside);
        }
        DirStamp { path: path.to_owned(), canonical, stamp, inside }
    }
}

fn walk_stamps(dir: &Path, relative: &Path, files: bool, depth: usize, out: &mut Vec<(PathBuf, Stamp)>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut items: Vec<_> = read.filter_map(|entry| entry.ok()).collect();
    items.sort_by_key(|entry| entry.file_name());
    for entry in items {
        let path = entry.path();
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        let relative = relative.join(entry.file_name());
        if meta.is_dir() {
            out.push((relative.clone(), Stamp::of(&meta)));
            if depth < MAX_DEPTH {
                walk_stamps(&path, &relative, files, depth + 1, out);
            }
        } else if files {
            out.push((relative, Stamp::of(&meta)));
        }
    }
}

/// Whether this process may write in `path`.
fn is_writable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: c_path is a valid NUL-terminated string for the call.
    unsafe { libc::access(c_path.as_ptr(), libc::W_OK) == 0 }
}

/// Everything the index was read from, stamped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Signature {
    /// The application directories.
    apps: Vec<DirStamp>,
    /// The MIME and terminal lists outside them.
    lists: Vec<ListStamp>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ListStamp {
    File(PathBuf, Option<Stamp>),
    Dir(DirStamp),
}

impl Signature {
    pub(crate) fn compute(dirs: &XdgDirs) -> Signature {
        Signature {
            apps: dirs.application_dirs().iter().map(|dir| DirStamp::of(dir)).collect(),
            lists: Signature::compute_lists(dirs),
        }
    }

    fn compute_lists(dirs: &XdgDirs) -> Vec<ListStamp> {
        let mut lists = Vec::new();
        for path in super::mime::config_list_paths(dirs) {
            let stamp = Stamp::of_path(&path);
            lists.push(ListStamp::File(path, stamp));
        }
        for dir in dirs.data_subdirs("xdg-terminal-exec") {
            lists.push(ListStamp::Dir(DirStamp::of(&dir)));
        }
        lists
    }
}

/// Watches what an [`AppIndex`] was read from; see [`AppIndex::watch`].
/// Dropping it stops the thread.
#[derive(Debug)]
pub struct Watcher {
    /// Closing this end of a pipe stops the thread.
    stop: Option<OwnedFd>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Watcher {
    fn start(dirs: XdgDirs, baseline: Signature, wake: Wake, every: Duration) -> Watcher {
        let Some((stop_read, stop_write)) = pipe() else {
            log::warn!("apps: can't watch for changes: no pipe");
            return Watcher { stop: None, thread: None };
        };
        // Watch before returning, so no change after this call is missed.
        let mut watched = dirs.application_dirs();
        watched.push(dirs.config_home.clone());
        let inotify = Inotify::new(watched.iter().filter(|dir| is_writable(dir)));
        let spawned = std::thread::Builder::new()
            .name("aizome-sys apps watcher".into())
            .stack_size(256 * 1024)
            .spawn(move || watch_loop(&dirs, baseline, &wake, every, &stop_read, inotify));
        match spawned {
            Ok(thread) => Watcher { stop: Some(stop_write), thread: Some(thread) },
            Err(error) => {
                log::warn!("apps: can't watch for changes: {error}");
                Watcher { stop: None, thread: None }
            }
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn watch_loop(
    dirs: &XdgDirs,
    baseline: Signature,
    wake: &Wake,
    every: Duration,
    stop: &OwnedFd,
    inotify: Option<Inotify>,
) {
    let mut last = baseline;
    let mut next_poll = Instant::now() + every;
    loop {
        let timeout = next_poll.saturating_duration_since(Instant::now());
        match wait(stop, inotify.as_ref(), timeout) {
            Waited::Stop => return,
            Waited::Event => {
                // Let a burst of writes settle before looking.
                if let Some(inotify) = &inotify {
                    inotify.drain();
                }
                if matches!(wait(stop, None, Duration::from_millis(150)), Waited::Stop) {
                    return;
                }
                if let Some(inotify) = &inotify {
                    inotify.drain();
                }
            }
            Waited::Timeout => next_poll = Instant::now() + every,
        }
        let now = Signature::compute(dirs);
        if now != last {
            last = now;
            wake.wake();
        }
    }
}

enum Waited {
    Stop,
    Event,
    Timeout,
}

/// Wait up to `timeout` for the stop pipe to close or an inotify event.
fn wait(stop: &OwnedFd, inotify: Option<&Inotify>, timeout: Duration) -> Waited {
    let mut fds = [
        libc::pollfd { fd: stop.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd: inotify.map_or(-1, |i| i.fd.as_raw_fd()), events: libc::POLLIN, revents: 0 },
    ];
    let millis = libc::c_int::try_from(timeout.as_millis()).unwrap_or(libc::c_int::MAX);
    // SAFETY: fds is a valid array of two pollfd structures for the call.
    let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, millis) };
    if ready < 0 {
        // EINTR and friends: try again soon.
        std::thread::sleep(Duration::from_millis(10));
        return Waited::Timeout;
    }
    if fds[0].revents != 0 {
        return Waited::Stop;
    }
    if fds[1].revents != 0 {
        return Waited::Event;
    }
    Waited::Timeout
}

fn pipe() -> Option<(OwnedFd, OwnedFd)> {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: fds has room for the two descriptors pipe2 writes.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return None;
    }
    // SAFETY: pipe2 succeeded, so both are open descriptors we now own.
    unsafe { Some((OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1]))) }
}

/// An inotify instance watching a few directories for entries changing.
struct Inotify {
    fd: OwnedFd,
}

impl Inotify {
    fn new<'a>(dirs: impl Iterator<Item = &'a PathBuf>) -> Option<Inotify> {
        use std::os::unix::ffi::OsStrExt;
        // SAFETY: plain system call with flags.
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if fd < 0 {
            return None;
        }
        // SAFETY: inotify_init1 returned a new descriptor we own.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let mask = libc::IN_CREATE
            | libc::IN_DELETE
            | libc::IN_MOVED_FROM
            | libc::IN_MOVED_TO
            | libc::IN_CLOSE_WRITE
            | libc::IN_ATTRIB
            | libc::IN_DELETE_SELF
            | libc::IN_MOVE_SELF;
        let mut any = false;
        for dir in dirs {
            let Ok(path) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else {
                continue;
            };
            // SAFETY: fd is an inotify descriptor and path a valid C string.
            any |= unsafe { libc::inotify_add_watch(fd.as_raw_fd(), path.as_ptr(), mask) } >= 0;
        }
        any.then_some(Inotify { fd })
    }

    /// Read and discard pending events.
    fn drain(&self) {
        let mut buffer = [0u8; 4096];
        loop {
            // SAFETY: the buffer is valid for writes of its length.
            let read = unsafe { libc::read(self.fd.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
            if read <= 0 {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::TempDir;
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn app(name: &str, extra: &str) -> String {
        format!("[Desktop Entry]\nType=Application\nName={name}\nExec={}\n{extra}", name.to_lowercase())
    }

    fn fixture() -> (TempDir, XdgDirs) {
        let tmp = TempDir::new("index");
        let dirs = XdgDirs::rooted(tmp.path());
        let user = "home/.local/share/applications";
        let local = "usr/local/share/applications";
        let system = "usr/share/applications";
        tmp.write(&format!("{system}/firefox.desktop"), &app("Firefox", "StartupWMClass=firefox\n"));
        tmp.write(&format!("{system}/org.gnome.Nautilus.desktop"), &app("Files", "Exec=nautilus --new-window %U\n"));
        tmp.write(&format!("{system}/kde4/konsole.desktop"), &app("Konsole", ""));
        tmp.write(&format!("{system}/hidden-by-user.desktop"), &app("Doomed", ""));
        tmp.write(&format!("{system}/overridden.desktop"), &app("System Version", ""));
        tmp.write(&format!("{system}/nodisplay.desktop"), &app("Handler", "NoDisplay=true\n"));
        tmp.write(&format!("{system}/gnome-only.desktop"), &app("Gnome Thing", "OnlyShowIn=GNOME;\n"));
        tmp.write(&format!("{system}/tryexec.desktop"), &app("Missing", "TryExec=/no/such/binary\n"));
        tmp.write(&format!("{system}/link.desktop"), "[Desktop Entry]\nType=Link\nName=Web\nURL=https://x.org\n");
        tmp.write(&format!("{system}/broken-user.desktop"), &app("Fallback", ""));
        tmp.write(&format!("{system}/notes.txt"), "not an entry");
        tmp.write(&format!("{local}/overridden.desktop"), &app("Local Version", ""));
        tmp.write(&format!("{user}/hidden-by-user.desktop"), "[Desktop Entry]\nHidden=true\n");
        tmp.write(&format!("{user}/broken-user.desktop"), "this is not a desktop file");
        tmp.write(&format!("{user}/alpha.desktop"), &app("alpha", ""));
        (tmp, dirs)
    }

    #[test]
    fn ids_precedence_and_visibility() {
        let (_tmp, dirs) = fixture();
        let index = AppIndex::load_with(dirs, Locale::default());
        let ids: Vec<&str> = index.all().iter().map(|e| e.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "alpha.desktop",
                "broken-user.desktop",
                "firefox.desktop",
                "gnome-only.desktop",
                "kde4-konsole.desktop",
                "link.desktop",
                "nodisplay.desktop",
                "org.gnome.Nautilus.desktop",
                "overridden.desktop",
                "tryexec.desktop",
            ]
        );
        assert_eq!(index.get("overridden.desktop").unwrap().name, "Local Version");
        assert_eq!(index.get("overridden").unwrap().name, "Local Version");
        assert!(index.get("hidden-by-user.desktop").is_none());
        // An unparsable file doesn't hide the one below it.
        assert_eq!(index.get("broken-user").unwrap().name, "Fallback");
        let shown: Vec<&str> = index.apps().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(shown, ["alpha", "Fallback", "Files", "Firefox", "Konsole", "Local Version"]);
        assert!(index.is_installed("nodisplay.desktop"));
        assert!(!index.is_shown("nodisplay.desktop"));
        assert!(!index.is_installed("tryexec.desktop"));
        assert!(!index.is_installed("link.desktop"));
        assert!(!index.is_installed("nope"));
        assert_eq!(index.app_dirs.len(), 3);
    }

    #[test]
    fn desktops_change_what_is_shown() {
        let (_tmp, mut dirs) = fixture();
        dirs.desktops = vec!["GNOME".into(), "Aizome".into()];
        let index = AppIndex::load_with(dirs, Locale::default());
        assert!(index.is_shown("gnome-only.desktop"));
    }

    #[test]
    fn missing_directories_give_an_empty_index() {
        let tmp = TempDir::new("index-empty");
        let mut index = AppIndex::load_with(XdgDirs::rooted(tmp.path()), Locale::default());
        assert!(index.all().is_empty());
        assert!(index.apps().is_empty());
        assert!(!index.refresh_if_changed());
        tmp.write("usr/share/applications/new.desktop", &app("New", ""));
        assert!(index.refresh_if_changed());
        assert_eq!(index.apps().len(), 1);
    }

    #[test]
    fn writable_directories_notice_edits() {
        let (tmp, dirs) = fixture();
        let mut index = AppIndex::load_with(dirs, Locale::default());
        assert!(!index.refresh_if_changed());
        // Rewrite a file in place (same directory mtime is possible).
        std::thread::sleep(Duration::from_millis(20));
        tmp.write("home/.local/share/applications/alpha.desktop", &app("Alpha Two", ""));
        assert!(index.refresh_if_changed());
        assert_eq!(index.get("alpha").unwrap().name, "Alpha Two");
        assert!(!index.refresh_if_changed());
        // A new file in a subdirectory.
        tmp.write("usr/share/applications/sub/dir/x.desktop", &app("Deep", ""));
        assert!(index.refresh_if_changed());
        assert!(index.get("sub-dir-x.desktop").is_some());
        // A MIME list in the config directory.
        tmp.write("home/.config/mimeapps.list", "[Default Applications]\n");
        assert!(index.refresh_if_changed());
        assert!(!index.refresh_if_changed());
    }

    #[test]
    fn a_retargeted_symlink_is_a_change_even_with_equal_mtimes() {
        // NixOS: /run/current-system/sw/share is a symlink into the store,
        // where every mtime is 1; a rebuild only retargets the symlink.
        let tmp = TempDir::new("index-symlink");
        let generation = |n: u32, name: &str| {
            let dir = format!("store/gen{n}/share/applications");
            tmp.write(&format!("{dir}/app.desktop"), &app(name, ""));
            set_mtime(&tmp.path().join(&dir).join("app.desktop"), 1);
            set_mtime(&tmp.path().join(&dir), 1);
            tmp.path().join(format!("store/gen{n}/share"))
        };
        let first = generation(1, "First");
        let second = generation(2, "Second");
        let link = tmp.path().join("sw-share");
        std::os::unix::fs::symlink(&first, &link).unwrap();
        let mut dirs = XdgDirs::rooted(tmp.path());
        dirs.data_dirs = vec![link.clone()];
        let mut index = AppIndex::load_with(dirs, Locale::default());
        assert_eq!(index.get("app").unwrap().name, "First");
        assert!(!index.refresh_if_changed());
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&second, &link).unwrap();
        assert!(index.refresh_if_changed());
        assert_eq!(index.get("app").unwrap().name, "Second");
    }

    fn set_mtime(path: &Path, seconds: i64) {
        use std::os::unix::ffi::OsStrExt;
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        let times = [libc::timespec { tv_sec: seconds, tv_nsec: 0 }; 2];
        // SAFETY: valid path and two timespecs.
        let status = unsafe { libc::utimensat(libc::AT_FDCWD, c_path.as_ptr(), times.as_ptr(), 0) };
        assert_eq!(status, 0);
    }

    #[test]
    fn the_watcher_wakes_on_changes_and_stops_when_dropped() {
        let (tmp, dirs) = fixture();
        let index = AppIndex::load_with(dirs.clone(), Locale::default());
        let woken = Arc::new(AtomicUsize::new(0));
        let counter = woken.clone();
        let wake = Wake::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        // A long poll interval: the inotify path must notice the user dir.
        let watcher = Watcher::start(dirs.clone(), index.signature.clone(), wake.clone(), Duration::from_secs(60));
        tmp.write("home/.local/share/applications/new.desktop", &app("New", ""));
        let deadline = Instant::now() + Duration::from_secs(5);
        while woken.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(woken.load(Ordering::SeqCst), 1);
        let started = Instant::now();
        drop(watcher);
        assert!(started.elapsed() < Duration::from_secs(1));
        // Polling notices non-writable or unwatched changes.
        let watcher = Watcher::start(dirs, index.signature.clone(), wake, Duration::from_millis(50));
        let deadline = Instant::now() + Duration::from_secs(5);
        while woken.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(woken.load(Ordering::SeqCst), 2, "the baseline differs from now");
        drop(watcher);
    }

    fn matcher() -> (TempDir, AppIndex) {
        let tmp = TempDir::new("match");
        let dir = "usr/share/applications";
        let write = |id: &str, text: &str| tmp.write(&format!("{dir}/{id}"), text);
        let entry = |name: &str, exec: &str, extra: &str| {
            format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n{extra}")
        };
        write("foot.desktop", &entry("Foot", "foot", ""));
        write("footclient.desktop", &entry("Foot Client", "footclient", ""));
        write("firefox.desktop", &entry("Firefox", "firefox --name firefox %U", "StartupWMClass=firefox\n"));
        write("org.gnome.Nautilus.desktop", &entry("Files", "nautilus --new-window %U", ""));
        write("code.desktop", &entry("Visual Studio Code", "code %F", "StartupWMClass=Code\n"));
        write("code-url-handler.desktop", &entry("VS Code URL Handler", "code --open-url %U", "NoDisplay=true\n"));
        write("signal.desktop", &entry("Signal", "signal-desktop --no-sandbox %U", "StartupWMClass=Signal\n"));
        write("bitwarden.desktop", &entry("Bitwarden", "bitwarden %U", ""));
        write("org.telegram.desktop.desktop", &entry("Telegram", "telegram-desktop -- %u", ""));
        write(
            "google-chrome.desktop",
            &entry(
                "Google Chrome",
                "/nix/store/abc-chrome/bin/google-chrome-stable %U",
                "StartupWMClass=google-chrome\n",
            ),
        );
        write("chrome-abcdef-Default.desktop", &entry("Some PWA", "chrome --app-id=abcdef", ""));
        write("mpv.desktop", &entry("mpv", "env LC_ALL=C /nix/store/x/bin/.mpv-wrapped %U", ""));
        write("dev.zed.Zed.desktop", &entry("Zed", "zeditor %U", ""));
        let index = AppIndex::load_with(XdgDirs::rooted(tmp.path()), Locale::default());
        (tmp, index)
    }

    #[test]
    fn windows_match_their_apps() {
        let (_tmp, index) = matcher();
        let id = |window: &str| index.match_window(window).map(|app| app.id.as_str());
        assert_eq!(id("foot"), Some("foot.desktop"));
        assert_eq!(id("foot.desktop"), Some("foot.desktop"));
        assert_eq!(id("footclient"), Some("footclient.desktop"));
        assert_eq!(id("firefox"), Some("firefox.desktop"));
        assert_eq!(id("Firefox"), Some("firefox.desktop"));
        assert_eq!(id("org.gnome.Nautilus"), Some("org.gnome.Nautilus.desktop"));
        assert_eq!(id("nautilus"), Some("org.gnome.Nautilus.desktop"));
        assert_eq!(id("Nautilus"), Some("org.gnome.Nautilus.desktop"));
        // Electron apps' app ID is their WM class.
        assert_eq!(id("Code"), Some("code.desktop"));
        assert_eq!(id("code"), Some("code.desktop"));
        assert_eq!(id("code-url-handler"), Some("code-url-handler.desktop"));
        assert_eq!(id("Signal"), Some("signal.desktop"));
        assert_eq!(id("Bitwarden"), Some("bitwarden.desktop"));
        // An app ID that itself ends in ".desktop".
        assert_eq!(id("org.telegram.desktop"), Some("org.telegram.desktop.desktop"));
        assert_eq!(id("google-chrome"), Some("google-chrome.desktop"));
        assert_eq!(id("google-chrome-stable"), Some("google-chrome.desktop"));
        assert_eq!(id("chrome-abcdef-Default"), Some("chrome-abcdef-Default.desktop"));
        // Wrapped programs, through env.
        assert_eq!(id(".mpv-wrapped"), Some("mpv.desktop"));
        assert_eq!(id("zeditor"), Some("dev.zed.Zed.desktop"));
        assert_eq!(id("zed"), Some("dev.zed.Zed.desktop"));
        assert_eq!(id("com.example.Zed"), Some("dev.zed.Zed.desktop"));
        assert_eq!(id("unknown-app"), None);
        assert_eq!(id(""), None);
        assert_eq!(id("   "), None);
    }

    #[test]
    fn program_names() {
        assert_eq!(exec_program_name("foot").as_deref(), Some("foot"));
        assert_eq!(exec_program_name("/usr/bin/Foo --x").as_deref(), Some("foo"));
        assert_eq!(exec_program_name("env A=1 B=2 prog %U").as_deref(), Some("prog"));
        assert_eq!(exec_program_name("env -u X").as_deref(), None);
        assert_eq!(exec_program_name("env -u X -i -C /tmp A=b prog").as_deref(), Some("prog"));
        assert_eq!(exec_program_name("/x/.prog-wrapped").as_deref(), Some("prog"));
        assert_eq!(exec_program_name("\"unterminated").as_deref(), None);
        assert_eq!(unwrap_program_name("..a-wrapped-wrapped"), "a");
        assert_eq!(unwrap_program_name(".hidden"), ".hidden");
    }

    #[test]
    #[ignore = "reads this machine's applications"]
    fn live_index_loads_quickly() {
        let started = Instant::now();
        let index = AppIndex::load();
        let load = started.elapsed();
        let started = Instant::now();
        let mut index = index;
        let unchanged = !index.refresh_if_changed();
        let refresh = started.elapsed();
        println!(
            "indexed {} entries ({} shown) from {} directories in {load:?}; unchanged check {refresh:?}",
            index.all().len(),
            index.apps().len(),
            index.app_dirs.len(),
        );
        assert!(unchanged);
        assert!(!index.apps().is_empty());
        for window in ["foot", "firefox", "org.gnome.Nautilus", "Code", "google-chrome", "kitty"] {
            println!("  window {window:?} → {:?}", index.match_window(window).map(|app| &app.id));
        }
        for app in index.apps().iter().take(8) {
            println!("  {} ({}) exec={:?} icon={:?}", app.name, app.id, app.exec, app.icon);
        }
        assert!(load < Duration::from_millis(500), "loading took {load:?}");
    }
}
