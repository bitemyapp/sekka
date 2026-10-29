//! Bounded filesystem resolution in a disposable process, never a UI/PTY call.
//! Cancellation kills only our child. A child stuck in kernel I/O retains the
//! single active slot until reaped; neither a hard reap deadline nor a UI join.
use crate::links::Link;
use std::{
    io::{self, Read, Write},
    os::{fd::OwnedFd, unix::net::UnixStream},
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
const MAX_BYTES: usize = 16 * 1024;
const TIMEOUT: Duration = Duration::from_millis(250);
pub const HELPER: &str = "--hotaru-resolve-link";
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Spawn(io::ErrorKind),
    Timeout,
    Cancelled,
    Output,
    Exit,
}
#[derive(Clone)]
pub struct Request {
    pub pane: u64,
    pub link: Link,
    pub cwd: Option<PathBuf>,
}
#[derive(Clone)]
pub struct Response {
    pub request: Arc<Request>,
    pub result: Result<Arc<str>, Error>,
}
#[derive(Default)]
struct Pending {
    job: Option<Arc<Request>>,
    active: Option<Arc<AtomicBool>>,
    stop: bool,
    ready: Option<Response>,
}
pub struct Worker {
    pending: Arc<(Mutex<Pending>, Condvar)>,
}
impl Worker {
    pub fn new(deliver: impl Fn() -> bool + Send + 'static) -> io::Result<Self> {
        let exe = std::env::current_exe()?;
        Self::start(deliver, move |request, cancel| resolve_process(&exe, request, cancel, TIMEOUT))
    }
    fn start(
        deliver: impl Fn() -> bool + Send + 'static,
        mut resolve: impl FnMut(&Request, &AtomicBool) -> Result<Arc<str>, Error> + Send + 'static,
    ) -> io::Result<Self> {
        let pending = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let shared = pending.clone();
        std::thread::Builder::new().name("hotaru-link-path".into()).spawn(move || {
            loop {
                let (request, cancel) = {
                    let mut state = shared.0.lock().unwrap_or_else(|e| e.into_inner());
                    while !state.stop && state.job.is_none() {
                        state = shared.1.wait(state).unwrap_or_else(|e| e.into_inner());
                    }
                    if state.stop {
                        return;
                    }
                    let request = state.job.take().expect("pending path request");
                    let cancel = Arc::new(AtomicBool::new(false));
                    state.active = Some(cancel.clone());
                    (request, cancel)
                };
                let result = resolve(&request, &cancel);
                let (publish, old) = {
                    let mut state = shared.0.lock().unwrap_or_else(|e| e.into_inner());
                    state.active = None;
                    if state.stop || cancel.load(Ordering::Acquire) {
                        (false, None)
                    } else {
                        (true, state.ready.replace(Response { request, result }))
                    }
                };
                drop(old);
                if publish && !deliver() {
                    return;
                }
            }
        })?;
        Ok(Self { pending })
    }
    pub fn submit(&self, request: Arc<Request>) {
        let old = {
            let mut state = self.pending.0.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(active) = &state.active {
                active.store(true, Ordering::Release);
            }
            state.job.replace(request)
        };
        drop(old);
        self.pending.1.notify_one();
    }
    pub fn take_response(&self) -> Option<Response> {
        self.pending.0.lock().unwrap_or_else(|e| e.into_inner()).ready.take()
    }
    pub fn cancel(&self) {
        let old = {
            let mut state = self.pending.0.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(active) = &state.active {
                active.store(true, Ordering::Release);
            }
            (state.job.take(), state.ready.take())
        };
        drop(old);
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let old = {
            let mut state = self.pending.0.lock().unwrap_or_else(|e| e.into_inner());
            state.stop = true;
            if let Some(active) = &state.active {
                active.store(true, Ordering::Release);
            }
            (state.job.take(), state.ready.take())
        };
        drop(old);
        self.pending.1.notify_one();
    }
}
fn resolve_process(exe: &Path, request: &Request, cancel: &AtomicBool, timeout: Duration) -> Result<Arc<str>, Error> {
    let target = &request.link.target;
    if cancel.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    let Some(cwd) = request.cwd.as_ref().filter(|_| !Path::new(target.as_ref()).is_absolute()) else {
        return Ok(target.clone());
    };
    if !cwd.is_absolute()
        || cwd.as_os_str().len() > MAX_BYTES
        || crate::links::valid_target(target.as_bytes()).is_none()
    {
        return Err(Error::Invalid);
    }
    let (reader, writer) = UnixStream::pair().map_err(|e| Error::Spawn(e.kind()))?;
    reader.set_nonblocking(true).map_err(|e| Error::Spawn(e.kind()))?;
    let mut child = Command::new(exe)
        .arg(HELPER)
        .arg(target.as_ref())
        .arg(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::from(OwnedFd::from(writer)))
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error::Spawn(e.kind()))?;
    supervise(&mut child, reader, cancel, timeout)
}
fn supervise(
    child: &mut Child,
    mut reader: UnixStream,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<Arc<str>, Error> {
    let deadline = Instant::now() + timeout;
    let mut bytes = Vec::with_capacity(MAX_BYTES);
    let mut buffer = [0u8; 1024];
    let mut failure = None;
    let mut eof = false;
    let mut status = None;
    loop {
        if failure.is_none() {
            if cancel.load(Ordering::Acquire) {
                failure = Some(Error::Cancelled);
            } else if Instant::now() >= deadline {
                failure = Some(Error::Timeout);
            } else {
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => {
                            eof = true;
                            break;
                        }
                        Ok(n) if n <= MAX_BYTES - bytes.len() => bytes.extend_from_slice(&buffer[..n]),
                        Ok(_) => {
                            failure = Some(Error::Output);
                            break;
                        }
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(_) => {
                            failure = Some(Error::Output);
                            break;
                        }
                    }
                    // Even a faulty helper producing unlimited bytes cannot
                    // prevent cancellation/deadline checks beyond this cap.
                }
            }
            if failure.is_some() {
                let _ = child.kill();
            }
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(Some(done)) => status = Some(done.success()),
                Ok(None) => {}
                Err(e) if e.raw_os_error() == Some(libc::ECHILD) => {
                    status = Some(false);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => {
                    if failure.is_none() {
                        failure = Some(Error::Exit);
                    }
                    let _ = child.kill();
                }
            }
        }
        if let Some(success) = status {
            if let Some(error) = failure {
                return Err(error);
            }
            if !success {
                return Err(Error::Exit);
            }
            if eof {
                return crate::links::valid_target(&bytes).map(Arc::from).ok_or(Error::Output);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
/// Hidden process mode. All arguments are literal; no shell, environment
/// expansion, canonicalization or line/column-suffix interpretation occurs.
pub fn helper(mut args: impl Iterator<Item = std::ffi::OsString>) -> Result<(), &'static str> {
    let target = args.next().ok_or("missing target")?;
    let cwd = args.next().ok_or("missing cwd")?;
    if args.next().is_some() {
        return Err("extra arguments");
    }
    let target = target.to_str().ok_or("non-UTF8 target")?;
    if crate::links::valid_target(target.as_bytes()).is_none() || cwd.len() > MAX_BYTES {
        return Err("invalid argument");
    }
    let result = resolve_literal(target, Path::new(&cwd))?;
    std::io::stdout().lock().write_all(result.as_bytes()).map_err(|_| "output failed")
}
fn resolve_literal(target: &str, cwd: &Path) -> Result<String, &'static str> {
    if !cwd.is_absolute() {
        return Err("relative cwd");
    }
    if Path::new(target).is_absolute() {
        return Ok(target.into());
    }
    let mut resolved = PathBuf::from("/");
    for part in cwd.join(target).components() {
        match part {
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(value) => resolved.push(value),
            Component::Prefix(_) => return Err("unsupported path prefix"),
        }
    }
    if resolved.as_os_str().len() > MAX_BYTES {
        return Err("resolved path too long");
    }
    if resolved.try_exists().unwrap_or(false) {
        let resolved = resolved.to_str().ok_or("non-UTF8 resolved path")?;
        if crate::links::valid_target(resolved.as_bytes()).is_none() {
            return Err("invalid resolved path");
        }
        Ok(resolved.into())
    } else {
        Ok(target.into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn request(pane: u64) -> Arc<Request> {
        let mut terminal = hotaru_core::Terminal::new(8, 2, 0);
        terminal.feed(b"\x1b]8;;https://x\x1b\\x\x1b]8;;\x1b\\");
        let snapshot = Arc::new(terminal.snapshot());
        Arc::new(Request {
            pane,
            link: Link::at(&snapshot, crate::terminal_view::CellPoint { row: 0, col: 0 }).unwrap(),
            cwd: None,
        })
    }
    #[test]
    fn resolver_latest_pending_and_drop_never_join_or_deliver_cancelled_work() {
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = Worker::start(
            move || done_tx.send(()).is_ok(),
            move |r, _| {
                started_tx.send(r.pane).unwrap();
                release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                Ok(r.link.target.clone())
            },
        )
        .unwrap();
        worker.submit(request(1));
        assert_eq!(started_rx.recv_timeout(Duration::from_secs(3)).unwrap(), 1);
        let replaced = request(2);
        let weak = Arc::downgrade(&replaced);
        worker.submit(replaced);
        worker.submit(request(3));
        assert!(weak.upgrade().is_none());
        release_tx.send(()).unwrap();
        assert_eq!(started_rx.recv_timeout(Duration::from_secs(3)).unwrap(), 3);
        assert!(done_rx.try_recv().is_err());
        drop(worker);
        release_tx.send(()).unwrap();
        assert!(done_rx.recv_timeout(Duration::from_secs(3)).is_err());
    }
    #[test]
    fn literal_resolution_normalizes_without_shell_or_symlink_expansion() {
        let root = std::env::temp_dir().join(format!("hotaru-paths-{}", std::process::id()));
        std::fs::create_dir_all(root.join("dir")).unwrap();
        for file in ["file", "$HOME", "~", "file:2:3", "$(false)"] {
            std::fs::write(root.join(file), b"").unwrap();
        }
        for file in ["file", "$HOME", "~", "file:2:3", "$(false)"] {
            assert_eq!(resolve_literal(&format!("dir/../{file}"), &root).unwrap(), root.join(file).to_str().unwrap());
        }
        assert_eq!(resolve_literal("missing", &root).unwrap(), "missing");
        assert_eq!(resolve_literal("https://example.test", &root).unwrap(), "https://example.test");
        assert_eq!(resolve_literal("/does/not/exist", &root).unwrap(), "/does/not/exist");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn stalled_child_is_killed_and_reaped_and_output_is_bounded() {
        for (script, expected) in [("exec sleep 30", Error::Timeout), ("exec head -c 20000 /dev/zero", Error::Output)] {
            let (reader, writer) = UnixStream::pair().unwrap();
            reader.set_nonblocking(true).unwrap();
            let mut child =
                Command::new("sh").arg("-c").arg(script).stdout(Stdio::from(OwnedFd::from(writer))).spawn().unwrap();
            assert_eq!(
                supervise(&mut child, reader, &AtomicBool::new(false), Duration::from_millis(50)),
                Err(expected)
            );
            assert!(child.try_wait().unwrap().is_some());
        }
    }
}
