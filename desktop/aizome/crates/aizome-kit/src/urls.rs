//! Bounded desktop opening. Process creation and reaping stay off the UI thread.

use std::fmt;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::time::Duration;

const LIMIT: usize = 8;
const ACTIVE: usize = 4;
const MAX_TARGET: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenUrlError {
    InvalidTarget,
    Busy,
    Unavailable,
    Spawn(std::io::ErrorKind),
    Exit(Option<i32>),
}

impl fmt::Display for OpenUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTarget => f.write_str("the link or path is empty, too long, or contains control characters"),
            Self::Busy => f.write_str("too many desktop opening requests are still running"),
            Self::Unavailable => f.write_str("desktop opening is unavailable"),
            Self::Spawn(kind) => write!(f, "could not start the desktop opener ({kind})"),
            Self::Exit(Some(code)) => write!(f, "the desktop opener failed (exit status {code})"),
            Self::Exit(None) => f.write_str("the desktop opener was terminated by a signal"),
        }
    }
}
impl std::error::Error for OpenUrlError {}

pub type OpenUrlCompletion = Box<dyn FnOnce(Result<(), OpenUrlError>) + Send>;

pub(crate) fn valid_target(target: &str) -> bool {
    !target.is_empty()
        && target.len() <= MAX_TARGET
        && !target.starts_with('-')
        && !target.chars().any(char::is_control)
}

struct Permit(Arc<AtomicUsize>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Request {
    target: String,
    completion: OpenUrlCompletion,
    _permit: Permit,
}

impl Request {
    fn finish(self, result: Result<(), OpenUrlError>) {
        let Self { completion, _permit, .. } = self;
        // A completion may enqueue another request immediately.
        drop(_permit);
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| completion(result))).is_err() {
            log::error!("Desktop opener completion panicked");
        }
    }
}

struct Active {
    child: Child,
    request: Request,
    failure: Option<OpenUrlError>,
}

pub(crate) struct Launcher {
    tx: SyncSender<Request>,
    pending: Arc<AtomicUsize>,
    stopping: Arc<AtomicBool>,
}

impl Drop for Launcher {
    fn drop(&mut self) {
        // Dropping the sender immediately afterwards also wakes an idle recv.
        // Reaping existing processes continues on the detached worker.
        self.stopping.store(true, Ordering::Release);
    }
}

impl Launcher {
    pub fn new() -> std::io::Result<Self> {
        Self::with_spawner(|target| {
            Command::new("xdg-open")
                .arg(target)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
        })
    }

    fn with_spawner(mut spawn: impl FnMut(&str) -> std::io::Result<Child> + Send + 'static) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<Request>(LIMIT);
        let pending = Arc::new(AtomicUsize::new(0));
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = stopping.clone();
        std::thread::Builder::new().name("aizome-open-url".into()).spawn(move || {
            let mut children: Vec<Active> = Vec::with_capacity(ACTIVE);
            let mut disconnected = false;
            loop {
                let mut index = 0;
                while index < children.len() {
                    let active = &mut children[index];
                    let result = match active.child.try_wait() {
                        Ok(Some(status)) => Some(active.failure.map_or_else(
                            || if status.success() { Ok(()) } else { Err(OpenUrlError::Exit(status.code())) },
                            Err,
                        )),
                        Ok(None) => None,
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => None,
                        // ECHILD proves there is no child left to reap (for
                        // example, an embedding application's SIGCHLD handler).
                        Err(error) if error.raw_os_error() == Some(libc::ECHILD) => {
                            Some(Err(OpenUrlError::Spawn(error.kind())))
                        }
                        Err(error) => {
                            if active.failure.is_none() {
                                active.failure = Some(OpenUrlError::Spawn(error.kind()));
                                let _ = active.child.kill();
                            }
                            // Retain ownership and retry; never drop an
                            // unreaped Child merely because polling failed.
                            None
                        }
                    };
                    if let Some(result) = result {
                        children.swap_remove(index).request.finish(result);
                    } else {
                        index += 1;
                    }
                }
                if stop.load(Ordering::Acquire) {
                    for request in rx.try_iter() {
                        request.finish(Err(OpenUrlError::Unavailable));
                    }
                    disconnected = true;
                }
                if disconnected && children.is_empty() {
                    break;
                }
                if children.len() == ACTIVE || disconnected {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                }
                let request = if children.is_empty() {
                    rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
                } else {
                    rx.recv_timeout(Duration::from_millis(20))
                };
                match request {
                    Ok(request) => {
                        if stop.load(Ordering::Acquire) {
                            request.finish(Err(OpenUrlError::Unavailable));
                            continue;
                        }
                        match spawn(&request.target) {
                            Ok(child) => children.push(Active { child, request, failure: None }),
                            Err(error) => request.finish(Err(OpenUrlError::Spawn(error.kind()))),
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => disconnected = true,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        })?;
        Ok(Self { tx, pending, stopping })
    }

    pub fn open(&self, target: &str, completion: OpenUrlCompletion) -> Result<(), OpenUrlError> {
        if !valid_target(target) {
            return Err(OpenUrlError::InvalidTarget);
        }
        self.pending
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < LIMIT).then_some(n + 1))
            .map_err(|_| OpenUrlError::Busy)?;
        let request = Request { target: target.into(), completion, _permit: Permit(self.pending.clone()) };
        self.tx.try_send(request).map_err(|error| match error {
            TrySendError::Full(_) => OpenUrlError::Busy,
            TrySendError::Disconnected(_) => OpenUrlError::Unavailable,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_admission_recovers_after_spawn_errors() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let launcher = Launcher::with_spawner(move |_| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Err(std::io::ErrorKind::NotFound.into())
        })
        .unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        for n in 0..LIMIT {
            let done = done_tx.clone();
            launcher
                .open(
                    "https://example.test",
                    Box::new(move |r| {
                        done.send(r).unwrap();
                    }),
                )
                .unwrap();
            if n == 0 {
                started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            }
        }
        assert_eq!(
            launcher.open("https://example.test", Box::new(|_| panic!("rejected callback"))),
            Err(OpenUrlError::Busy)
        );
        for _ in 0..LIMIT {
            release_tx.send(()).unwrap();
            assert_eq!(
                done_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                Err(OpenUrlError::Spawn(std::io::ErrorKind::NotFound))
            );
        }
        for invalid in ["", "-option", "https://a/\0", "https://a/\u{009c}"] {
            assert_eq!(
                launcher.open(invalid, Box::new(|_| panic!("rejected callback"))),
                Err(OpenUrlError::InvalidTarget)
            );
        }
        assert!(valid_target("/tmp/space 日本"));
        assert!(valid_target("https://example.test/a?x=$(touch nope)&y='quoted'"));
    }

    #[test]
    fn child_exit_is_reaped_and_reported_without_shell_evaluation() {
        let launcher = Launcher::with_spawner(|target| {
            Command::new("/bin/sh")
                .args(["-c", "test \"$1\" = 'https://example.test/$(false)'", "test-opener", target])
                .spawn()
        })
        .unwrap();
        let (tx, rx) = mpsc::channel();
        launcher
            .open(
                "https://example.test/$(false)",
                Box::new(move |result| {
                    tx.send(result).unwrap();
                }),
            )
            .unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Ok(()));
        let (tx, rx) = mpsc::channel();
        launcher
            .open(
                "https://different.test",
                Box::new(move |result| {
                    tx.send(result).unwrap();
                }),
            )
            .unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Err(OpenUrlError::Exit(Some(1))));
    }

    #[test]
    fn completion_releases_capacity_and_panic_does_not_kill_worker() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let launcher = Arc::new(
            Launcher::with_spawner(move |_| {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Err(std::io::ErrorKind::NotFound.into())
            })
            .unwrap(),
        );
        let weak = Arc::downgrade(&launcher);
        let (tx, rx) = mpsc::channel();
        let retry_tx = tx.clone();
        launcher
            .open(
                "https://first.test",
                Box::new(move |_| {
                    let strong = weak.upgrade().unwrap();
                    let done = retry_tx.clone();
                    retry_tx
                        .send(strong.open(
                            "https://retry.test",
                            Box::new(move |_| {
                                done.send(Ok(())).unwrap();
                            }),
                        ))
                        .unwrap();
                    panic!("fixture completion panic");
                }),
            )
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        for _ in 1..LIMIT {
            launcher.open("https://pending.test", Box::new(|_| {})).unwrap();
        }
        release_tx.send(()).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Ok(()));
        for _ in 0..LIMIT {
            release_tx.send(()).unwrap();
        }
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Ok(()));
    }

    #[test]
    fn dropping_launcher_cancels_queued_requests_without_waiting() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let launcher = Launcher::with_spawner(move |_| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Err(std::io::ErrorKind::NotFound.into())
        })
        .unwrap();
        let (tx, rx) = mpsc::channel();
        let first = tx.clone();
        launcher
            .open(
                "https://first.test",
                Box::new(move |result| {
                    first.send(result).unwrap();
                }),
            )
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        for _ in 1..LIMIT {
            let tx = tx.clone();
            launcher
                .open(
                    "https://cancelled.test",
                    Box::new(move |result| {
                        tx.send(result).unwrap();
                    }),
                )
                .unwrap();
        }
        drop(launcher); // Must return while the spawner is deliberately blocked.
        release_tx.send(()).unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            Err(OpenUrlError::Spawn(std::io::ErrorKind::NotFound))
        );
        for _ in 1..LIMIT {
            assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Err(OpenUrlError::Unavailable));
        }
        assert!(started_rx.try_recv().is_err());
    }
}
