//! Watching the folders on screen for changes (inotify): one descriptor,
//! a thread that waits on it, and the set of folders kept to those
//! showing.

use std::collections::HashMap;
use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

/// What changes are heard: entries made, removed, renamed, written or
/// changed, and the folder itself going.
const MASK: u32 = libc::IN_CREATE
    | libc::IN_DELETE
    | libc::IN_MOVED_FROM
    | libc::IN_MOVED_TO
    | libc::IN_CLOSE_WRITE
    | libc::IN_ATTRIB
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF
    | libc::IN_ONLYDIR;

/// Watches folders; tells `on_change` the folder that changed (often
/// several times for one change: debounce).
pub struct Watcher {
    fd: Arc<OwnedFd>,
    stop: OwnedFd,
    watches: Arc<Mutex<HashMap<i32, PathBuf>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Watcher {
    pub fn new(on_change: impl Fn(PathBuf) + Send + 'static) -> io::Result<Watcher> {
        // SAFETY: plain system calls; the descriptors are owned at once.
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: as above.
        let fd = Arc::new(unsafe { OwnedFd::from_raw_fd(fd) });
        let mut pipe = [0; 2];
        // SAFETY: as above.
        if unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: as above.
        let (wake_r, stop) = unsafe { (OwnedFd::from_raw_fd(pipe[0]), OwnedFd::from_raw_fd(pipe[1])) };
        let watches: Arc<Mutex<HashMap<i32, PathBuf>>> = Arc::default();
        let thread = {
            let (fd, watches) = (fd.clone(), watches.clone());
            std::thread::Builder::new()
                .name("hako-watch".into())
                .spawn(move || read_events(&fd, &wake_r, &watches, &on_change))?
        };
        Ok(Watcher { fd, stop, watches, thread: Some(thread) })
    }

    /// Watch exactly `dirs` from now on.
    pub fn set(&self, dirs: &[PathBuf]) {
        let mut w = self.watches.lock().unwrap_or_else(PoisonError::into_inner);
        let gone: Vec<i32> = w.iter().filter(|(_, p)| !dirs.contains(p)).map(|(wd, _)| *wd).collect();
        for wd in gone {
            // SAFETY: a watch of ours on our descriptor.
            unsafe { libc::inotify_rm_watch(self.fd.as_raw_fd(), wd) };
            w.remove(&wd);
        }
        for d in dirs {
            if w.values().any(|p| p == d) {
                continue;
            }
            if let Some(wd) = add_watch(&self.fd, d) {
                w.insert(wd, d.clone());
            }
        }
    }

    /// The folders watched.
    pub fn watched(&self) -> Vec<PathBuf> {
        self.watches.lock().unwrap_or_else(PoisonError::into_inner).values().cloned().collect()
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // SAFETY: writing a byte to our own pipe.
        unsafe { libc::write(self.stop.as_raw_fd(), b"x".as_ptr().cast(), 1) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn add_watch(fd: &OwnedFd, dir: &Path) -> Option<i32> {
    let c = CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: a valid path on our descriptor.
    let wd = unsafe { libc::inotify_add_watch(fd.as_raw_fd(), c.as_ptr(), MASK) };
    (wd >= 0).then_some(wd)
}

fn read_events(fd: &OwnedFd, stop: &OwnedFd, watches: &Mutex<HashMap<i32, PathBuf>>, on_change: &dyn Fn(PathBuf)) {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let mut fds = [
            libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: stop.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        ];
        // SAFETY: two valid pollfds.
        let r = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if r < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        if fds[1].revents != 0 {
            return;
        }
        // SAFETY: reading into our buffer.
        let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if n <= 0 {
            continue;
        }
        let mut changed: Vec<PathBuf> = Vec::new();
        let mut i = 0usize;
        let header = std::mem::size_of::<libc::inotify_event>();
        while i + header <= n as usize {
            // SAFETY: the kernel wrote whole events; read one unaligned.
            let ev: libc::inotify_event = unsafe { std::ptr::read_unaligned(buf[i..].as_ptr().cast()) };
            i += header + ev.len as usize;
            let w = watches.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(p) = w.get(&ev.wd)
                && !changed.contains(p)
            {
                changed.push(p.clone());
            }
        }
        for p in changed {
            on_change(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsops::testing::TempDir;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn hears_changes_in_folders_watched() {
        let t = TempDir::new("watch");
        let (a, b) = (t.dir("a"), t.dir("b"));
        let (tx, rx) = mpsc::channel();
        let w = Watcher::new(move |p| {
            let _ = tx.send(p);
        })
        .unwrap();
        w.set(std::slice::from_ref(&a));
        std::fs::write(a.join("new.txt"), "x").unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), a);
        while rx.try_recv().is_ok() {}
        // Only what's watched.
        w.set(std::slice::from_ref(&b));
        std::fs::write(a.join("other.txt"), "x").unwrap();
        std::fs::rename(a.join("new.txt"), b.join("moved.txt")).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), b);
        assert_eq!(w.watched(), vec![b]);
        drop(w);
    }
}
