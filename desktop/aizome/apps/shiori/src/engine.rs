//! The app's side of a document's worker: it starts `aizome-shiori
//! --worker`, hands it requests, turns its drawings into images off
//! the interface thread, and watches it. A worker that dies, or spends too
//! long on one thing, is replaced; what it was doing is reported, so the
//! app can leave that page (or that search) alone, and the new worker
//! opens the document again.

use std::io::{BufReader, BufWriter, Read};
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
        {
            // The document may have closed while this one started, with
            // `drop` finding no worker to stop: it's stopped here instead.
            // Looked at with the lock held: either `drop` finds this
            // worker, or this sees that the document closed.
            let mut c = shared.child.lock().unwrap_or_else(|e| e.into_inner());
            if shared.closed.load(Ordering::SeqCst) {
                drop(c);
                let _ = child.kill();
                let _ = child.wait();
                *shared.stdin.lock().unwrap_or_else(|e| e.into_inner()) = None;
                return;
            }
            *c = Some(child);
        }
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

/// Whether a worker that opened a document of `pages` pages (`None` if it
/// hasn't opened one) could have made the drawing `key`: of a page it has,
/// and a part of that page no larger than the app asks for. A worker reads
/// what a document gives it, so the app takes neither on its word.
fn could_draw(key: &TileKey, pages: Option<usize>) -> bool {
    key.plausible() && pages.is_some_and(|n| (key.page as usize) < n)
}

/// Hand on what the worker says until it goes. A drawing it couldn't
/// have made (`could_draw`) means it's gone wrong, and what follows can't
/// be trusted or even read: it's stopped, as if it crashed.
fn read_replies(mut out: impl Read, shared: &Shared, sink: &(dyn Fn(Event) + Send + Sync)) {
    let mut pages = None;
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
                if !could_draw(&key, pages) {
                    log::warn!("the worker sent a drawing it couldn't have made, {key:?}, of {pages:?} pages");
                    return;
                }
                let Ok(px) = read_pixels(&mut out, &key) else { return };
                done(shared);
                // Drawn on white, every pixel opaque: premultiplied as
                // they are.
                match Image::from_premultiplied_rgba(key.w, key.h, px) {
                    Some(img) => sink(Event::Drawn(key, img)),
                    None => sink(Event::Reply(Reply::DrawFailed(key))),
                }
            }
            Reply::Text { page, mut text } => {
                done(shared);
                text.agree();
                sink(Event::Reply(Reply::Text { page, text }));
            }
            Reply::Opened(info) => {
                pages = Some(info.pages.len());
                done(shared);
                sink(Event::Reply(Reply::Opened(info)));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::DocInfo;

    /// What the engine hands on of `stream`, a worker's replies.
    fn heard(stream: &[u8]) -> Vec<Event> {
        let shared = Shared {
            stdin: Mutex::new(None),
            child: Mutex::new(None),
            busy: Mutex::new(None),
            stopped: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        };
        let events = Mutex::new(Vec::<Event>::new());
        read_replies(stream, &shared, &|e: Event| events.lock().unwrap().push(e));
        events.into_inner().unwrap()
    }

    fn drawn(out: &mut Vec<u8>, key: TileKey) {
        write_frame(out, &Reply::Drawn(key), Some(&vec![255; key.bytes()][..])).unwrap();
    }

    #[test]
    fn a_drawing_the_worker_couldnt_have_made_stops_it() {
        let tile = TileKey { page: 2, size: (100, 100), at: (0, 0), w: 10, h: 10 };
        // Before it's opened a document, a worker has drawn nothing.
        let mut early = Vec::new();
        drawn(&mut early, tile);
        assert!(heard(&early).is_empty());
        let opened = |out: &mut Vec<u8>| {
            let info = DocInfo { pages: vec![(612.0, 792.0); 3], ..DocInfo::default() };
            write_frame(out, &Reply::Opened(Box::new(info)), None).unwrap();
        };
        for wrong in [
            // A page the document hasn't.
            TileKey { page: 9999, ..tile },
            // A part past the page's edge, or past the end of a `u32`.
            TileKey { at: (95, 0), ..tile },
            TileKey { at: (u32::MAX - 5, 0), ..tile },
        ] {
            let mut stream = Vec::new();
            opened(&mut stream);
            drawn(&mut stream, tile);
            drawn(&mut stream, wrong);
            write_frame(&mut stream, &Reply::Links { page: 0, links: Vec::new() }, None).unwrap();
            let events = heard(&stream);
            // The document opened and the right drawing came; nothing
            // after the wrong one did.
            assert_eq!(events.len(), 2, "{wrong:?}: {events:?}");
            assert!(matches!(events[0], Event::Reply(Reply::Opened(_))));
            assert!(matches!(&events[1], Event::Drawn(k, _) if *k == tile));
        }
    }
}
