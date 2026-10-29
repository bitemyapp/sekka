//! One bounded latest-request matcher for the native window. No UI teardown join.
pub mod mapping;
use hotaru_core::Snapshot;
use hotaru_link_regex::{CellPoint, Limits, Match, Matcher};
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
const SNAPSHOT_ADMISSION: usize = 8 * 1024 * 1024;
#[derive(Debug)]
pub struct Request {
    pub pane: u64,
    pub snapshot: Arc<Snapshot>,
    pub cell: CellPoint,
    pub retained_bytes: usize,
}
impl Request {
    pub fn same_location(&self, pane: u64, snapshot: &Arc<Snapshot>, cell: CellPoint) -> bool {
        self.pane == pane && Arc::ptr_eq(&self.snapshot, snapshot) && self.cell == cell
    }
}
#[derive(Clone, Debug)]
pub enum Error {
    Admission,
    Mapping(mapping::Error),
    Regex(hotaru_link_regex::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Admission => f.write_str("displayed frame exceeds link-worker admission"),
            Self::Mapping(error) => write!(f, "logical line mapping: {error:?}"),
            Self::Regex(error) => write!(f, "{error}"),
        }
    }
}
#[derive(Clone)]
pub struct Response {
    pub request: Arc<Request>,
    pub result: Result<Option<Match>, Error>,
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
    pub fn new(deliver: impl Fn() -> bool + Send + 'static) -> std::io::Result<Self> {
        Self::start(deliver, || {
            let mut matcher = Matcher::new();
            move |request: &Request, cancel: &AtomicBool| {
                let deadline = Instant::now() + Duration::from_millis(25);
                let mapped =
                    mapping::extract(&request.snapshot, request.cell, cancel, deadline).map_err(Error::Mapping)?;
                if request.cell < mapped.start || request.cell > mapped.end {
                    return Err(Error::Mapping(mapping::Error::Invalid));
                }
                let matcher = matcher.as_mut().map_err(|e| Error::Regex(*e))?;
                matcher
                    .find_at(&mapped.source, request.cell, &Limits { deadline, ..Default::default() }, cancel)
                    .map_err(Error::Regex)
            }
        })
    }
    fn start<P>(
        deliver: impl Fn() -> bool + Send + 'static,
        initialize: impl FnOnce() -> P + Send + 'static,
    ) -> std::io::Result<Self>
    where
        P: FnMut(&Request, &AtomicBool) -> Result<Option<Match>, Error> + 'static,
    {
        let pending = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let shared = pending.clone();
        std::thread::Builder::new().name("hotaru-links".into()).spawn(move || {
            let mut process = initialize();
            loop {
                let (request, cancel) = {
                    let mut state = shared.0.lock().unwrap_or_else(|e| e.into_inner());
                    while !state.stop && state.job.is_none() {
                        state = shared.1.wait(state).unwrap_or_else(|e| e.into_inner());
                    }
                    if state.stop {
                        return;
                    }
                    let job = state.job.take().expect("pending automatic-link request");
                    let cancel = Arc::new(AtomicBool::new(false));
                    state.active = Some(cancel.clone());
                    (job, cancel)
                };
                let result = process(&request, &cancel);
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
    /// O(1) admission from the font worker's conservative complete-frame charge.
    /// One active, one pending and one unread completed capture can retain at most24MiB
    /// by this estimate, plus bounded matcher output/engine allocations. It is
    /// neither RSS nor a promise that a C call can be interrupted at25ms.
    pub fn submit(&self, request: Arc<Request>) -> Result<(), Error> {
        if request.retained_bytes > SNAPSHOT_ADMISSION {
            self.cancel();
            return Err(Error::Admission);
        }
        let old = {
            let mut state = self.pending.0.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(active) = &state.active {
                active.store(true, Ordering::Release);
            }
            state.job.replace(request)
        };
        drop(old);
        self.pending.1.notify_one();
        Ok(())
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
        // The detached owner releases C/row state after the bounded call returns.
        // Never join a foreign search, filesystem operation or background owner here.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    fn request(pane: u64) -> Arc<Request> {
        Arc::new(Request {
            pane,
            snapshot: Arc::new(hotaru_core::Terminal::new(8, 2, 0).snapshot()),
            cell: CellPoint { row: 0, col: 0 },
            retained_bytes: 1024,
        })
    }
    #[test]
    fn one_active_latest_pending_cancellation_and_drop_do_not_join() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = Worker::start(
            move || done_tx.send(()).is_ok(),
            move || {
                move |r: &Request, _: &AtomicBool| {
                    started_tx.send(r.pane).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                    Ok(None)
                }
            },
        )
        .unwrap();
        worker.submit(request(1)).unwrap();
        assert_eq!(started_rx.recv_timeout(Duration::from_secs(3)).unwrap(), 1);
        let replaced = request(2);
        let weak = Arc::downgrade(&replaced);
        worker.submit(replaced).unwrap();
        worker.submit(request(3)).unwrap();
        assert!(weak.upgrade().is_none());
        release_tx.send(()).unwrap();
        assert_eq!(started_rx.recv_timeout(Duration::from_secs(3)).unwrap(), 3);
        assert!(done_rx.try_recv().is_err());
        drop(worker);
        release_tx.send(()).unwrap();
        assert!(done_rx.recv_timeout(Duration::from_secs(3)).is_err());
    }
    #[test]
    fn unread_completion_is_replaced_without_queueing_owned_frames() {
        let (tx, rx) = mpsc::channel();
        let worker = Worker::start(move || tx.send(()).is_ok(), || |_: &Request, _: &AtomicBool| Ok(None)).unwrap();
        let first = request(1);
        let weak = Arc::downgrade(&first);
        worker.submit(first).unwrap();
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(weak.upgrade().is_some());
        worker.submit(request(2)).unwrap();
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(weak.upgrade().is_none());
        assert_eq!(worker.take_response().unwrap().request.pane, 2);
        assert!(worker.take_response().is_none());
    }
    #[test]
    fn frame_ownership_admission_does_not_start_a_job() {
        let (tx, rx) = mpsc::channel();
        let worker = Worker::start(
            move || true,
            move || {
                move |_: &Request, _: &AtomicBool| {
                    tx.send(()).unwrap();
                    Ok(None)
                }
            },
        )
        .unwrap();
        let mut job = request(1);
        Arc::get_mut(&mut job).unwrap().retained_bytes = SNAPSHOT_ADMISSION + 1;
        assert!(matches!(worker.submit(job), Err(Error::Admission)));
        assert!(rx.try_recv().is_err());
        drop(worker);
    }
}
