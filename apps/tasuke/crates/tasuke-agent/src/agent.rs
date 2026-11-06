//! Starting a turn and talking to it while it runs.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::event::{Decision, Event};
use crate::ids::CallId;
use crate::options::Options;
use crate::session::Session;

pub struct Agent;

impl Agent {
    /// Run one user turn of `session` on its own thread. Events arrive on
    /// [`TurnHandle::events`] and are written to the session as they happen.
    /// The session must not have another turn running.
    pub fn start(session: &Session, user_text: &str, options: Options) -> TurnHandle {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Shared::default());
        let ctl = Control { shared: shared.clone(), tx };
        let session = session.clone();
        let text = user_text.to_owned();
        let thread = std::thread::Builder::new()
            .name("tasuke-turn".into())
            .spawn(move || {
                crate::turn::run(&session, &text, options, &ctl);
                ctl.shared.finished.store(true, Ordering::SeqCst);
                ctl.wake();
            })
            .expect("spawn the turn's thread");
        TurnHandle { events: rx, shared, thread: Some(thread) }
    }
}

type Waker = Box<dyn Fn() + Send + 'static>;

#[derive(Default)]
pub(crate) struct Shared {
    cancelled: AtomicBool,
    finished: AtomicBool,
    decisions: Mutex<HashMap<CallId, Decision>>,
    decided: Condvar,
    waker: Mutex<Option<Waker>>,
}

/// A running turn.
pub struct TurnHandle {
    events: Receiver<Event>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl TurnHandle {
    /// The turn's events, in order. The channel closes when the turn ends.
    pub fn events(&self) -> &Receiver<Event> {
        &self.events
    }

    /// Answer a [`Event::ToolCall`] that asked for approval.
    pub fn decide(&self, id: &CallId, decision: Decision) {
        self.shared.decisions.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone(), decision);
        self.shared.decided.notify_all();
    }

    /// Stop the turn: the model's stream is closed, a running command's
    /// process group is killed, a pending approval is denied. The turn
    /// still ends with [`Event::TurnFinished`].
    pub fn cancel(&self) {
        self.shared.cancelled.store(true, Ordering::SeqCst);
        self.shared.decided.notify_all();
    }

    /// Called (on the turn's thread) after each event is sent and when the
    /// turn ends, so a UI can wake its loop. Called once at once, for any
    /// events already waiting.
    pub fn set_waker(&self, waker: impl Fn() + Send + 'static) {
        let mut w = self.shared.waker.lock().unwrap_or_else(|e| e.into_inner());
        waker();
        *w = Some(Box::new(waker));
    }

    /// Whether the turn's thread has ended (events may still be queued).
    pub fn is_finished(&self) -> bool {
        self.shared.finished.load(Ordering::SeqCst)
    }

    /// Wait for the turn's thread to end.
    pub fn join(mut self) {
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The turn's side of the handle.
pub(crate) struct Control {
    pub(crate) shared: Arc<Shared>,
    tx: Sender<Event>,
}

impl Control {
    pub(crate) fn send(&self, event: Event) {
        let _ = self.tx.send(event);
        self.wake();
    }

    fn wake(&self) {
        if let Some(w) = self.shared.waker.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            w();
        }
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.shared.cancelled.load(Ordering::SeqCst)
    }

    pub(crate) fn cancel_flag(&self) -> &AtomicBool {
        &self.shared.cancelled
    }

    /// Wait for the person's decision on `id`; `None` when cancelled.
    pub(crate) fn wait_decision(&self, id: &CallId) -> Option<Decision> {
        let mut d = self.shared.decisions.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(decision) = d.remove(id) {
                return Some(decision);
            }
            if self.cancelled() {
                return None;
            }
            d = self.shared.decided.wait_timeout(d, Duration::from_millis(500)).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> (Control, TurnHandle) {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Shared::default());
        (Control { shared: shared.clone(), tx }, TurnHandle { events: rx, shared, thread: None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn decisions_reach_the_waiting_turn() {
        let (ctl, handle) = Control::for_test();
        let id = CallId::new("c1");
        let waiter = {
            let id = id.clone();
            std::thread::spawn(move || ctl.wait_decision(&id))
        };
        std::thread::sleep(Duration::from_millis(50));
        handle.decide(&id, Decision::Allow);
        assert_eq!(waiter.join().unwrap(), Some(Decision::Allow));
    }

    #[test]
    fn cancel_ends_a_wait() {
        let (ctl, handle) = Control::for_test();
        let waiter = std::thread::spawn(move || ctl.wait_decision(&CallId::new("c1")));
        std::thread::sleep(Duration::from_millis(50));
        handle.cancel();
        assert_eq!(waiter.join().unwrap(), None);
    }

    #[test]
    fn the_waker_runs_on_registration_and_each_event() {
        let (ctl, handle) = Control::for_test();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        handle.set_waker(move || {
            c.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(count.load(Ordering::SeqCst), 1);
        ctl.send(Event::MemoryChanged);
        ctl.send(Event::MemoryChanged);
        assert_eq!(count.load(Ordering::SeqCst), 3);
        assert_eq!(handle.events().try_iter().count(), 2);
    }
}
