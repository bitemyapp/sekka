//! The app's side of a document's worker: it starts `aizome-shiori
//! --worker`, hands it requests, turns its drawings into images off
//! the interface thread, and watches it. A worker that dies, or spends too
//! long on one thing, is replaced; what it was doing is reported, so the
//! app can leave that page (or that search) alone, and the new worker
//! opens the document again.

use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aizome_kit::prelude::Image;

use crate::protocol::{Job, OpenError, Reply, Request, TileKey, read_frame, read_pixels, write_frame};

/// What the engine tells the app.
#[derive(Clone, Debug)]
pub enum Event {
    /// A reply (a drawing comes as `Drawn`).
    Reply(Reply),
    Drawn(TileKey, Image),
    /// The worker died or was stopped while doing `job`; another is
    /// starting (and says `Reply::Opened` again) unless `gave_up`.
    Failed {
        job: Option<Job>,
        why: Failure,
        gave_up: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// It took longer than `limit` allows.
    TooLong,
    /// It crashed (or was killed by something else).
    Crashed,
}

/// How long a job may take before its worker is stopped.
pub fn limit(job: &Job) -> Duration {
    match job {
        Job::Open => Duration::from_secs(30),
        Job::Draw(_) => Duration::from_secs(20),
        Job::Text(_) | Job::Links(_) | Job::Find(_) => Duration::from_secs(10),
    }
}

/// After this many failures in a minute, no worker is started again.
const MOST_FAILURES: usize = 4;

struct Shared {
    stdin: Mutex<Option<BufWriter<ChildStdin>>>,
    child: Mutex<Option<Child>>,
    /// What the worker said it's doing, since when.
    busy: Mutex<Option<(Job, Instant)>>,
    /// The watchdog stopped it.
    stopped: AtomicBool,
    /// The app closed the document.
    closed: AtomicBool,
}

/// A document's worker.
pub struct Engine {
    tx: Sender<Request>,
    shared: Arc<Shared>,
}

impl Engine {
    /// Start a worker (`exe --worker`) for `path`; `sink` hears
    /// everything it says, on threads of the engine's.
    pub fn start(
        exe: PathBuf,
        path: PathBuf,
        password: Option<String>,
        sink: impl Fn(Event) + Send + Sync + 'static,
    ) -> Engine {
        let shared = Arc::new(Shared {
            stdin: Mutex::new(None),
            child: Mutex::new(None),
            busy: Mutex::new(None),
            stopped: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        });
        let (tx, rx) = mpsc::channel::<Request>();
        let sink: Arc<dyn Fn(Event) + Send + Sync> = Arc::new(sink);
        let open = Request::Open { path, password };
        {
            let shared = shared.clone();
            let _ = std::thread::Builder::new()
                .name("shiori-worker".into())
                .spawn(move || supervise(exe, open, shared, sink));
        }
        {
            let shared = shared.clone();
            let _ = std::thread::Builder::new().name("shiori-requests".into()).spawn(move || forward(rx, shared));
        }
        {
            let shared = shared.clone();
            let _ = std::thread::Builder::new().name("shiori-watchdog".into()).spawn(move || watch(shared));
        }
        Engine { tx, shared }
    }

    pub fn send(&self, r: Request) {
        let _ = self.tx.send(r);
    }

    /// The worker's process id (for tests).
    pub fn pid(&self) -> Option<u32> {
        self.shared.child.lock().ok()?.as_ref().map(Child::id)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::SeqCst);
        if let Ok(mut c) = self.shared.child.lock()
            && let Some(child) = c.as_mut()
        {
            let _ = child.kill();
        }
    }
}

fn spawn(exe: &PathBuf) -> std::io::Result<Child> {
    Command::new(exe).arg("--worker").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn()
}

/// Run workers one after another while the document is open.
fn supervise(exe: PathBuf, open: Request, shared: Arc<Shared>, sink: Arc<dyn Fn(Event) + Send + Sync>) {
    let mut failures: Vec<Instant> = Vec::new();
    loop {
        if shared.closed.load(Ordering::SeqCst) {
            return;
        }
        let mut child = match spawn(&exe) {
            Ok(c) => c,
            Err(e) => {
                sink(Event::Reply(Reply::OpenFailed(OpenError::Worker(format!("{}: {e}", exe.display())))));
                return;
            }
        };
        let stdout = child.stdout.take();
        let mut stdin = child.stdin.take().map(BufWriter::new);
        if let Some(s) = stdin.as_mut() {
            let _ = write_frame(s, &open, None);
        }
        *shared.stdin.lock().unwrap_or_else(|e| e.into_inner()) = stdin;
        *shared.child.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
        shared.stopped.store(false, Ordering::SeqCst);
        if let Some(out) = stdout {
            read_replies(BufReader::with_capacity(1 << 20, out), &shared, &*sink);
        }
        // The worker's gone: why, and doing what.
        *shared.stdin.lock().unwrap_or_else(|e| e.into_inner()) = None;
        if let Some(mut c) = shared.child.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        if shared.closed.load(Ordering::SeqCst) {
            return;
        }
        let job = shared.busy.lock().unwrap_or_else(|e| e.into_inner()).take().map(|(j, _)| j);
        let why = if shared.stopped.load(Ordering::SeqCst) { Failure::TooLong } else { Failure::Crashed };
        let now = Instant::now();
        failures.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
        failures.push(now);
        // A document that can't even be opened, or that keeps failing,
        // isn't tried again.
        let gave_up = job == Some(Job::Open) || failures.len() >= MOST_FAILURES;
        log::warn!("the worker stopped ({why:?}) while on {job:?}");
        sink(Event::Failed { job, why, gave_up });
        if gave_up {
            return;
        }
    }
}

fn read_replies(mut out: BufReader<std::process::ChildStdout>, shared: &Shared, sink: &(dyn Fn(Event) + Send + Sync)) {
    loop {
        let reply = match read_frame::<Reply>(&mut out) {
            Ok(Some(r)) => r,
            _ => return,
        };
        match reply {
            Reply::Working(job) => {
                *shared.busy.lock().unwrap_or_else(|e| e.into_inner()) = Some((job, Instant::now()));
            }
            Reply::Drawn(key) => {
                let Ok(px) = read_pixels(&mut out, &key) else { return };
                done(shared);
                match Image::from_rgba(key.w, key.h, &px) {
                    Some(img) => sink(Event::Drawn(key, img)),
                    None => sink(Event::Reply(Reply::DrawFailed(key))),
                }
            }
            r => {
                done(shared);
                sink(Event::Reply(r));
            }
        }
    }
}

fn done(shared: &Shared) {
    *shared.busy.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Hand the app's requests to whichever worker is running.
fn forward(rx: Receiver<Request>, shared: Arc<Shared>) {
    for r in rx {
        let mut stdin = shared.stdin.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = stdin.as_mut()
            && write_frame(s, &r, None).is_err()
        {
            *stdin = None;
        }
    }
}

/// Stop a worker that's been on one job too long.
fn watch(shared: Arc<Shared>) {
    while !shared.closed.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(250));
        let late = shared
            .busy
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|(job, since)| since.elapsed() > limit(job));
        if late {
            shared.stopped.store(true, Ordering::SeqCst);
            if let Some(c) = shared.child.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                let _ = c.kill();
            }
        }
    }
}
