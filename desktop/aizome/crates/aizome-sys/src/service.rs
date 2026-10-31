//! What every service handle is made of: a [`Wake`] callback into the UI's
//! main loop, a cached model behind a cheap [`Arc`] snapshot with a change
//! counter, and [`Task`]s for actions that run off the UI thread.
//!
//! The pattern, for a service `S` with model `M`:
//!
//! ```ignore
//! let wake = Wake::new(move || proxy.wake());   // e.g. a sidestep_ui::Proxy
//! let network = Network::new(wake);
//! // In the main loop, after a wake:
//! if network.generation() != seen {
//!     seen = network.generation();
//!     redraw(&network.snapshot());              // Arc<NetworkState>
//! }
//! let task = network.set_wifi_enabled(false);   // returns at once
//! // Later, after another wake:
//! if let Some(result) = task.try_result() { show(result) }
//! ```

use crate::error::{Error, Result};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::Duration;

/// Wakes the UI's main loop from a background thread.
///
/// Services call it whenever their model changes and whenever an action
/// finishes. It must be cheap and must not block: post an event to the main
/// loop (a `sidestep_ui::Proxy`, an eventfd, a channel) and return. It runs
/// on whichever service thread noticed the change. The type is
/// `aizome-apps`'s, so the application index's watcher takes it too.
pub use aizome_apps::Wake;

/// An action running off the UI thread, and its result once it is done.
///
/// Action methods return a `Task` at once. When the action finishes, the
/// service calls its [`Wake`]; the UI then reads the result with
/// [`try_result`](Task::try_result) (or [`take`](Task::take)). Tests and
/// command-line tools can [`wait`](Task::wait) instead. Dropping a task
/// doesn't cancel the action; it only forgets its result.
pub struct Task<T> {
    slot: Arc<Slot<T>>,
}

struct Slot<T> {
    result: Mutex<Option<Result<T>>>,
    done: Condvar,
    finished: AtomicBool,
}

impl<T> Clone for Task<T> {
    fn clone(&self) -> Self {
        Task { slot: self.slot.clone() }
    }
}

impl<T> std::fmt::Debug for Task<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Task").field("done", &self.is_done()).finish()
    }
}

impl<T: Send + 'static> Task<T> {
    /// Run `work` on a new thread; when it returns, store its result and
    /// call `wake`.
    pub(crate) fn spawn(name: &str, wake: &Wake, work: impl FnOnce() -> Result<T> + Send + 'static) -> Task<T> {
        let (task, done) = Task::pending(wake.clone());
        let spawned = std::thread::Builder::new().name(format!("aizome-sys {name}")).spawn(move || done.finish(work()));
        if let Err(error) = spawned {
            // The closure (and the Done inside it) is gone; report on a
            // fresh one.
            return Task::ready(Err(Error::Failed(format!("could not start a thread: {error}"))));
        }
        task
    }
}

impl<T> Task<T> {
    /// A task and the handle that finishes it.
    pub(crate) fn pending(wake: Wake) -> (Task<T>, Done<T>) {
        let slot = Arc::new(Slot { result: Mutex::new(None), done: Condvar::new(), finished: AtomicBool::new(false) });
        (Task { slot: slot.clone() }, Done { slot: Some(slot), wake })
    }

    /// A task that is already done, such as an action refused before it
    /// started.
    pub fn ready(result: Result<T>) -> Task<T> {
        Task {
            slot: Arc::new(Slot {
                result: Mutex::new(Some(result)),
                done: Condvar::new(),
                finished: AtomicBool::new(true),
            }),
        }
    }

    /// Whether the action has finished.
    pub fn is_done(&self) -> bool {
        self.slot.finished.load(Ordering::Acquire)
    }

    /// The result, once, if the action has finished; later calls return
    /// `None`.
    pub fn take(&self) -> Option<Result<T>> {
        lock(&self.slot.result).take()
    }

    /// Block until the action finishes and return its result.
    pub fn wait(self) -> Result<T> {
        let mut result = lock(&self.slot.result);
        loop {
            if let Some(result) = result.take() {
                return result;
            }
            if self.is_done() {
                return Err(Error::Failed("the result was already taken".into()));
            }
            result = self.slot.done.wait(result).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Block up to `timeout` for the action to finish; `None` if it hasn't.
    pub fn wait_timeout(&self, timeout: Duration) -> Option<Result<T>> {
        let deadline = std::time::Instant::now() + timeout;
        let mut result = lock(&self.slot.result);
        loop {
            if let Some(result) = result.take() {
                return Some(result);
            }
            let left = deadline.checked_duration_since(std::time::Instant::now())?;
            result =
                self.slot.done.wait_timeout(result, left).map(|(guard, _)| guard).unwrap_or_else(|e| e.into_inner().0);
        }
    }
}

impl<T: Clone> Task<T> {
    /// A copy of the result, if the action has finished, leaving it in
    /// place for later calls.
    pub fn try_result(&self) -> Option<Result<T>> {
        lock(&self.slot.result).clone()
    }
}

/// Finishes a [`Task`]: stores the result and wakes the UI. Dropped without
/// a result, it finishes the task with [`Error::Cancelled`].
pub(crate) struct Done<T> {
    slot: Option<Arc<Slot<T>>>,
    wake: Wake,
}

impl<T> Done<T> {
    pub(crate) fn finish(mut self, result: Result<T>) {
        self.complete(result);
    }

    fn complete(&mut self, result: Result<T>) {
        if let Some(slot) = self.slot.take() {
            *lock(&slot.result) = Some(result);
            slot.finished.store(true, Ordering::Release);
            // Wake before notifying, so whoever waits sees the wake done.
            self.wake.wake();
            slot.done.notify_all();
        }
    }
}

impl<T> Drop for Done<T> {
    fn drop(&mut self) {
        self.complete(Err(Error::Cancelled));
    }
}

/// A service's cached model: an `Arc` snapshot swapped whole on each
/// change, a generation counter, and the wakes to call.
pub(crate) struct Model<M> {
    current: RwLock<Arc<M>>,
    generation: AtomicU64,
    wakes: Mutex<Vec<Wake>>,
}

impl<M> Model<M> {
    pub(crate) fn new(initial: M, wake: Wake) -> Model<M> {
        Model { current: RwLock::new(Arc::new(initial)), generation: AtomicU64::new(0), wakes: Mutex::new(vec![wake]) }
    }

    /// The current snapshot (an `Arc` clone).
    pub(crate) fn get(&self) -> Arc<M> {
        self.current.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Replace the snapshot, count a change and wake everyone.
    pub(crate) fn set(&self, model: M) {
        *self.current.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(model);
        self.changed();
    }

    /// Replace the snapshot only if it differs from the current one.
    pub(crate) fn set_if_changed(&self, model: M)
    where
        M: PartialEq,
    {
        if *self.get() != model {
            self.set(model);
        }
    }

    /// Change a copy of the snapshot and install it.
    pub(crate) fn update(&self, change: impl FnOnce(&mut M))
    where
        M: Clone,
    {
        let mut current = self.current.write().unwrap_or_else(|e| e.into_inner());
        let mut model = M::clone(&current);
        change(&mut model);
        *current = Arc::new(model);
        drop(current);
        self.changed();
    }

    /// Count a change and wake everyone (for state kept outside the
    /// snapshot).
    pub(crate) fn changed(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        let wakes = lock(&self.wakes).clone();
        for wake in wakes {
            wake.wake();
        }
    }

    /// How many times the model has changed; compare with the last value
    /// seen to know whether to redraw.
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Also call `wake` on each change.
    pub(crate) fn on_change(&self, wake: Wake) {
        lock(&self.wakes).push(wake);
    }

    /// The wake to call when an action finishes: the first one given.
    pub(crate) fn wake(&self) -> Wake {
        lock(&self.wakes).first().cloned().unwrap_or_default()
    }
}

/// Lock a mutex, ignoring poisoning: a panicked worker must not take the
/// UI down with it.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// A flag a service's background threads check to know the handle was
/// dropped.
#[derive(Clone, Default)]
pub(crate) struct Stop(Arc<AtomicBool>);

impl Stop {
    pub(crate) fn stop(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub(crate) fn stopped(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Implements the service-handle methods shared by every service whose
/// struct has `model: Arc<Model<M>>`.
macro_rules! service_handle {
    ($service:ty, $model:ty) => {
        impl $service {
            /// The current state: cheap, from the cache that signal
            /// subscriptions keep current.
            pub fn snapshot(&self) -> std::sync::Arc<$model> {
                self.model.get()
            }

            /// How many times the state has changed. Compare with the value
            /// seen last to know whether to redraw.
            pub fn generation(&self) -> u64 {
                self.model.generation()
            }

            /// Also call `wake` whenever the state changes.
            pub fn on_change(&self, wake: impl Fn() + Send + Sync + 'static) {
                self.model.on_change($crate::Wake::new(wake));
            }
        }
    };
}
pub(crate) use service_handle;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn a_task_wakes_once_and_keeps_its_result() {
        let woken = Arc::new(AtomicUsize::new(0));
        let counter = woken.clone();
        let wake = Wake::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let task = Task::spawn("test", &wake, || Ok(42));
        assert_eq!(task.wait_timeout(Duration::from_secs(5)), Some(Ok(42)));
        assert!(task.is_done());
        assert_eq!(woken.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_dropped_completion_cancels() {
        let (task, done) = Task::<()>::pending(Wake::none());
        drop(done);
        assert_eq!(task.wait(), Err(Error::Cancelled));
    }

    #[test]
    fn models_count_changes() {
        let model = Model::new(1, Wake::none());
        let first = model.generation();
        model.set_if_changed(1);
        assert_eq!(model.generation(), first);
        model.update(|n| *n += 1);
        assert_eq!(*model.get(), 2);
        assert!(model.generation() > first);
    }
}
