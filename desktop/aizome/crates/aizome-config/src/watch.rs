//! Settings changes wake the reader; unchanged settings need no timer.

use std::collections::HashSet;
use std::ffi::{CString, OsString};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt, net::UnixStream};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::Duration;

/// Watches the settings file and calls back when it changes.
///
/// Linux file notifications cover writes, atomic replacement, deletion,
/// and settings directories created after startup. The thread blocks
/// until an event or shutdown; dropping the watcher stops and joins it.
/// If notifications cannot be installed, a logged polling fallback keeps
/// settings updates working.
pub struct Watcher {
    stop: Option<UnixStream>,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    pub fn spawn(on_change: impl FnMut() + Send + 'static) -> Watcher {
        Self::spawn_for(crate::Settings::path(), Duration::from_secs(1), on_change)
    }

    /// Watch `path`. `interval` is used only if file notifications fail;
    /// it adds no delay or periodic wakeups while notifications work.
    pub fn spawn_for(path: PathBuf, interval: Duration, mut on_change: impl FnMut() + Send + 'static) -> Watcher {
        // Resolve relative paths before handing them to another thread.
        let path = std::path::absolute(path).expect("resolve the settings path");
        let (read, write) = UnixStream::pair().expect("settings watcher stop socket");
        // Install before returning, so an immediate save cannot be missed.
        let mut watch = notified(&path, true);
        let mut last = stamp(&path);
        let thread = std::thread::Builder::new()
            .name("aizome-config-watch".into())
            .spawn(move || {
                loop {
                    let timeout = watch.is_none().then_some(interval.max(Duration::from_millis(1)));
                    match wait(&read, watch.as_ref(), timeout) {
                        Ok(Ready::Stop) => break,
                        Ok(Ready::Change) => {
                            if let Some(w) = &watch {
                                match w.changed() {
                                    Ok(false) => continue,
                                    Ok(true) => {}
                                    Err(error) => log::warn!("settings notifications: {error}"),
                                }
                            }
                            // Re-arm before inspecting the file. A rename
                            // may have replaced its inode or its directory;
                            // a newly created ancestor lets us watch closer.
                            watch = notified(&path, watch.is_some());
                            let now = stamp(&path);
                            if now != last {
                                last = now;
                                on_change();
                            }
                        }
                        Err(error) => {
                            log::warn!("settings watcher stopped: {error}");
                            break;
                        }
                    }
                }
            })
            .expect("spawn the settings watcher");
        Watcher { stop: Some(write), thread: Some(thread) }
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

// Inode identity detects atomic replacements even when size and mtime
// are preserved. ctime also detects an in-place write with restored mtime.
fn stamp(path: &Path) -> Option<(u64, u64, u64, i64, i64, i64, i64)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.dev(), m.ino(), m.len(), m.mtime(), m.mtime_nsec(), m.ctime(), m.ctime_nsec()))
}

enum Ready {
    Stop,
    Change,
}

fn wait(stop: &UnixStream, watch: Option<&Notifications>, timeout: Option<Duration>) -> io::Result<Ready> {
    let mut fds = [
        libc::pollfd { fd: stop.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd: watch.map_or(-1, |w| w.fd.as_raw_fd()), events: libc::POLLIN, revents: 0 },
    ];
    let timeout = timeout.map_or(-1, |d| d.as_millis().min(i32::MAX as u128) as i32);
    loop {
        // SAFETY: both entries are valid pollfd values for this call.
        if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, timeout) } >= 0 {
            return Ok(if fds[0].revents != 0 { Ready::Stop } else { Ready::Change });
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

struct Notifications {
    fd: OwnedFd,
    // A name filters child events; every entry observes its inode's own
    // events too. Ancestors need only self events, not sibling writes.
    entries: Vec<(i32, Option<OsString>)>,
}

fn notified(path: &Path, report_failure: bool) -> Option<Notifications> {
    match Notifications::new(path) {
        Ok(watch) => Some(watch),
        Err(error) => {
            if report_failure {
                log::warn!("settings: polling {} because file notifications failed: {error}", path.display());
            }
            None
        }
    }
}

impl Notifications {
    fn new(path: &Path) -> io::Result<Self> {
        // SAFETY: plain system call; a successful descriptor is owned below.
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: inotify_init1 returned a fresh descriptor.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let add = |path: &Path, mask| -> io::Result<i32> {
            let path = CString::new(path.as_os_str().as_bytes())?;
            // SAFETY: fd is open and path is a valid, NUL-terminated string.
            let wd = unsafe { libc::inotify_add_watch(fd.as_raw_fd(), path.as_ptr(), mask | libc::IN_MASK_ADD) };
            if wd < 0 { Err(io::Error::last_os_error()) } else { Ok(wd) }
        };
        let changes = libc::IN_CLOSE_WRITE | libc::IN_ATTRIB | libc::IN_DELETE_SELF | libc::IN_MOVE_SELF;
        let mask = changes | libc::IN_CREATE | libc::IN_DELETE | libc::IN_MOVED_FROM | libc::IN_MOVED_TO;
        let mut entries = Vec::new();
        let mut pending = vec![path.to_owned()];
        let mut visited = HashSet::new();
        while let Some(path) = pending.pop() {
            if !visited.insert(path.clone()) {
                continue;
            }
            if visited.len() > 40 {
                return Err(io::Error::other("too many settings symlinks"));
            }
            let mut child = path.as_path();
            let parent = loop {
                let parent = child.parent().ok_or_else(|| io::Error::other("settings path has no directory"))?;
                match add(parent, mask | libc::IN_ONLYDIR) {
                    Ok(wd) => {
                        entries.push((wd, Some(child.file_name().unwrap().to_owned())));
                        break parent;
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => child = parent,
                    Err(error) => return Err(error),
                }
            };
            // Renaming any ancestor can disconnect the watched inode from
            // the configured path. These watches do not observe children.
            for ancestor in parent.ancestors().skip(1) {
                let wd = add(ancestor, libc::IN_MOVE_SELF | libc::IN_DELETE_SELF | libc::IN_ONLYDIR)?;
                entries.push((wd, None));
            }
            // Watch both sides of symlinks, including directory symlinks:
            // the link can be retargeted and its target deleted/recreated.
            for component in path.ancestors() {
                if let Ok(target) = std::fs::read_link(component) {
                    pending.push(component.to_owned());
                    let mut resolved = component.parent().unwrap().join(target);
                    let suffix = path.strip_prefix(component).unwrap();
                    if !suffix.as_os_str().is_empty() {
                        resolved.push(suffix);
                    }
                    pending.push(resolved);
                }
            }
            // Also observe writes made through a different hard link.
            let file_changes = if path.is_dir() { libc::IN_MOVE_SELF | libc::IN_DELETE_SELF } else { changes };
            match add(&path, file_changes) {
                Ok(wd) => entries.push((wd, None)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(Self { fd, entries })
    }

    fn changed(&self) -> io::Result<bool> {
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
                changed |= event.mask & libc::IN_Q_OVERFLOW != 0
                    || self.entries.iter().any(|(wd, child)| {
                        event.wd == *wd && (name.is_empty() || child.as_ref().is_some_and(|c| name == c.as_bytes()))
                    });
                offset = end;
            }
        }
    }
}
