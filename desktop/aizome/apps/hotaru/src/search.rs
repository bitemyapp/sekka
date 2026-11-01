//! One cancellable whole-history search worker for the active pane.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use aizome_kit::{MsgSender, TimerHandle};
use hotaru_core::{ContentSnapshot, search};

use crate::app::Msg;

#[derive(Clone)]
pub struct Cache {
    pub results: Arc<search::Results>,
    pub selected: Option<usize>,
}

pub struct State {
    pub worker: Option<Worker>,
    pub pane: Option<u64>,
    pub token: u64,
    pub cancel: Arc<AtomicBool>,
    pub in_flight: bool,
    pub dirty: bool,
    pub timer: Option<TimerHandle>,
    pub caches: [Option<Cache>; 2],
    pub navigation: u64,
    pub pending_navigation: Option<(u64, Arc<search::Results>, usize)>,
    pub retry_navigation: bool,
    pub copy: Option<(Arc<search::Results>, usize)>,
    pub error: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            worker: None,
            pane: None,
            token: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            in_flight: false,
            dirty: false,
            timer: None,
            caches: [None, None],
            navigation: 0,
            pending_navigation: None,
            retry_navigation: false,
            copy: None,
            error: None,
        }
    }
}

impl State {
    pub fn invalidate(&mut self, clear: bool) {
        self.cancel.store(true, Ordering::Release);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.token = self.token.wrapping_add(1);
        self.in_flight = false;
        self.dirty = true;
        self.copy = None;
        self.pending_navigation = None;
        self.retry_navigation = false;
        self.error = None;
        if clear {
            self.caches = [None, None];
        }
    }

    pub fn label(&self, alternate: bool) -> String {
        if let Some(error) = &self.error {
            return error.clone();
        }
        let Some(cache) = &self.caches[usize::from(alternate)] else {
            return if self.in_flight || self.dirty { "Searching…" } else { "0 matches" }.into();
        };
        let count = cache.results.total_matches;
        let selected = cache.selected.map_or(0, |index| index + 1);
        if cache.results.truncated() {
            format!("{selected} of {} shown ({count} total)", cache.results.matches.len())
        } else if count == 0 {
            "0 matches".into()
        } else {
            format!("{selected} of {count}")
        }
    }
}

pub struct Job {
    pub token: u64,
    pub pane: u64,
    pub navigation: u64,
    pub snapshot: ContentSnapshot,
    pub needle: Vec<u8>,
    pub preserve: Option<(Arc<search::Results>, Option<usize>)>,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Default)]
struct Pending {
    job: Option<Job>,
    active: Option<Arc<AtomicBool>>,
    stop: bool,
}

pub struct Worker {
    pending: Arc<(Mutex<Pending>, Condvar)>,
}

impl Worker {
    pub fn new(tx: MsgSender<Msg>) -> std::io::Result<Self> {
        let pending = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let shared = pending.clone();
        std::thread::Builder::new().name("hotaru-search".into()).spawn(move || {
            loop {
                let job = {
                    let (lock, wake) = &*shared;
                    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                    while !state.stop && state.job.is_none() {
                        state = wake.wait(state).unwrap_or_else(|e| e.into_inner());
                    }
                    if state.stop {
                        return;
                    }
                    let job = state.job.take().expect("pending search job");
                    state.active = Some(job.cancel.clone());
                    job
                };
                let retained = job.preserve.as_ref().and_then(|(results, index)| {
                    index
                        .and_then(|index| results.resolve_match(index, &job.snapshot.position, job.snapshot.generation))
                });
                let message =
                    match search::find(job.snapshot, &job.needle, search::Options { max_matches: 10_000 }, &job.cancel)
                    {
                        Ok(results) => {
                            let selected = if job.preserve.is_some() {
                                retained.and_then(|old| results.matches.iter().position(|found| *found == old))
                            } else {
                                (!results.matches.is_empty()).then_some(0)
                            };
                            Some(Msg::SearchReady(
                                job.token,
                                job.pane,
                                job.navigation,
                                Cache { results: Arc::new(results), selected },
                            ))
                        }
                        Err(search::Error::Cancelled) => None,
                        Err(error) => Some(Msg::SearchFailed(job.token, job.pane, error.to_string())),
                    };
                shared.0.lock().unwrap_or_else(|e| e.into_inner()).active = None;
                if !job.cancel.load(Ordering::Acquire)
                    && let Some(message) = message
                    && !tx.send(message)
                {
                    return;
                }
            }
        })?;
        Ok(Self { pending })
    }

    pub fn submit(&self, job: Job) {
        let (old, active) = {
            let mut state = self.pending.0.lock().unwrap_or_else(|e| e.into_inner());
            (state.job.replace(job), state.active.clone())
        };
        if let Some(active) = active {
            active.store(true, Ordering::Release);
        }
        if let Some(old) = old {
            old.cancel.store(true, Ordering::Release);
        }
        self.pending.1.notify_one();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let mut state = self.pending.0.lock().unwrap_or_else(|e| e.into_inner());
        state.stop = true;
        if let Some(job) = &state.job {
            job.cancel.store(true, Ordering::Release);
        }
        if let Some(cancel) = &state.active {
            cancel.store(true, Ordering::Release);
        }
        drop(state);
        self.pending.1.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Hotaru;
    use aizome_kit::testing::Harness;

    #[test]
    fn refresh_started_before_navigation_cannot_overwrite_its_selection() {
        let mut h = Harness::new(Hotaru::preview());
        let id = h.program().active_pane().unwrap().id;
        let captured = ContentSnapshot::from((*h.program().active_pane().unwrap().snapshot).clone());
        let results = Arc::new(search::find(captured, b"Hotaru", Default::default(), &AtomicBool::new(false)).unwrap());
        assert_eq!(results.matches.len(), 2);
        let app = h.program_mut().front_mut().unwrap();
        app.find = true;
        app.search = "Hotaru".into();
        app.find_state.pane = Some(id);
        app.find_state.token = 42;
        app.find_state.navigation = 7;
        app.find_state.in_flight = true;
        app.find_state.caches[0] = Some(Cache { results: results.clone(), selected: Some(1) });
        app.find_state.copy = Some((results.clone(), 1));
        h.send(Msg::SearchReady(42, id, 6, Cache { results, selected: Some(0) }));
        assert_eq!(h.program().front().unwrap().find_state.caches[0].as_ref().unwrap().selected, Some(1));
        assert_eq!(h.program().front().unwrap().find_state.copy.as_ref().unwrap().1, 1);
        assert!(!h.program().front().unwrap().find_state.in_flight);
        assert!(h.program().front().unwrap().find_state.dirty);
        h.send(Msg::Select(id, crate::terminal_view::Selection::default()));
        assert!(h.program().front().unwrap().find_state.copy.is_none());
        assert!(h.program().front().unwrap().find_state.caches[0].as_ref().unwrap().selected.is_none());
    }

    #[test]
    fn late_refresh_cannot_restore_expired_active_copy_and_fatal_actor_cancels_capture() {
        let mut h = Harness::new(Hotaru::preview());
        let id = h.program().active_pane().unwrap().id;
        let mut terminal = hotaru_core::Terminal::new(20, 2, 0);
        terminal.feed(b"aaa");
        let results = Arc::new(
            search::find(terminal.content_snapshot(), b"a", Default::default(), &AtomicBool::new(false)).unwrap(),
        );
        terminal.feed(b"X");
        let app = h.program_mut();
        app.panes[0].snapshot = Arc::new(terminal.snapshot());
        let app = app.front_mut().unwrap();
        app.find = true;
        app.search = "a".into();
        app.find_state.pane = Some(id);
        app.find_state.token = 42;
        app.find_state.in_flight = true;
        app.find_state.caches[0] = Some(Cache { results: results.clone(), selected: Some(0) });
        app.find_state.copy = Some((results.clone(), 0));
        h.send(Msg::SearchReady(42, id, 0, Cache { results, selected: Some(0) }));
        assert!(h.program().front().unwrap().find_state.copy.is_none());
        // The logical index belongs to the result, while copy requires a
        // currently valid display anchor. The next worker refresh decides
        // whether the selected match itself survived.
        assert_eq!(h.program().front().unwrap().find_state.caches[0].as_ref().unwrap().selected, Some(0));
        h.program_mut().front_mut().unwrap().find_state.in_flight = true;
        let cancellation = h.program().front().unwrap().find_state.cancel.clone();
        h.send(Msg::Failed(id, "injected worker failure".into()));
        assert!(!h.program().front().unwrap().find_state.in_flight);
        assert!(cancellation.load(Ordering::Acquire));
        assert!(h.program().front().unwrap().find_state.error.is_some());
    }

    #[test]
    fn newer_capture_keeps_logical_selection_until_its_frame_arrives() {
        let mut h = Harness::new(Hotaru::preview());
        let id = h.program().active_pane().unwrap().id;
        let mut terminal = hotaru_core::Terminal::new(10, 2, 2);
        terminal.feed(b"Mark0\r\nMark1\r\nMark2\r\n");
        let old = Arc::new(
            search::find(terminal.content_snapshot(), b"Mark", Default::default(), &AtomicBool::new(false)).unwrap(),
        );
        let old_frame = Arc::new(terminal.snapshot());
        terminal.feed(b"Mark3\r\nMark4\r\n");
        let results = Arc::new(
            search::find(terminal.content_snapshot(), b"Mark", Default::default(), &AtomicBool::new(false)).unwrap(),
        );
        let selected = 0;
        assert!(results.resolve_match(selected, &old_frame.position, old_frame.generation).is_none());
        let app = h.program_mut();
        app.panes[0].snapshot = old_frame;
        let app = app.front_mut().unwrap();
        app.find = true;
        app.search = "Mark".into();
        app.find_state.pane = Some(id);
        app.find_state.token = 42;
        app.find_state.in_flight = true;
        app.find_state.caches[0] = Some(Cache { results: old.clone(), selected: Some(0) });
        app.find_state.copy = Some((old, 0));
        h.send(Msg::SearchReady(42, id, 0, Cache { results, selected: Some(selected) }));
        assert_eq!(h.program().front().unwrap().find_state.caches[0].as_ref().unwrap().selected, Some(selected));
        assert!(h.program().front().unwrap().find_state.copy.is_none());
        h.send(Msg::Frame(id, Arc::new(terminal.snapshot())));
        assert_eq!(h.program().front().unwrap().find_state.copy.as_ref().unwrap().1, selected);
        h.send(Msg::Select(id, crate::terminal_view::Selection::default()));
        h.send(Msg::Frame(id, Arc::new(terminal.snapshot())));
        assert!(h.program().front().unwrap().find_state.copy.is_none());
    }
}
