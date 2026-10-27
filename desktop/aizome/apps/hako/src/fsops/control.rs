//! Pausing and cancelling an operation from another thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};

/// The operation was cancelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cancelled")
    }
}

impl std::error::Error for Cancelled {}

/// Shared between an operation's worker (which calls
/// [`checkpoint`](Control::checkpoint) between pieces of work) and the
/// UI (which pauses, resumes and cancels it).
#[derive(Debug, Default)]
pub struct Control {
    cancelled: AtomicBool,
    paused: Mutex<bool>,
    wake: Condvar,
}

impl Control {
    pub fn new() -> Control {
        Control::default()
    }

    /// Stop at the next checkpoint (a paused operation wakes to stop).
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        let _g = self.paused.lock().unwrap_or_else(PoisonError::into_inner);
        self.wake.notify_all();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn set_paused(&self, paused: bool) {
        *self.paused.lock().unwrap_or_else(PoisonError::into_inner) = paused;
        self.wake.notify_all();
    }

    pub fn is_paused(&self) -> bool {
        *self.paused.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Between pieces of work: wait while paused, and say whether to stop.
    pub fn checkpoint(&self) -> Result<(), Cancelled> {
        if self.is_cancelled() {
            return Err(Cancelled);
        }
        let mut paused = self.paused.lock().unwrap_or_else(PoisonError::into_inner);
        while *paused && !self.is_cancelled() {
            paused = self.wake.wait(paused).unwrap_or_else(PoisonError::into_inner);
        }
        if self.is_cancelled() { Err(Cancelled) } else { Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn pauses_until_resumed_or_cancelled() {
        let c = Arc::new(Control::new());
        assert_eq!(c.checkpoint(), Ok(()));
        c.set_paused(true);
        let worker = {
            let c = c.clone();
            std::thread::spawn(move || c.checkpoint())
        };
        std::thread::sleep(Duration::from_millis(30));
        assert!(!worker.is_finished(), "a paused worker waits");
        c.set_paused(false);
        assert_eq!(worker.join().unwrap(), Ok(()));

        c.set_paused(true);
        let worker = {
            let c = c.clone();
            std::thread::spawn(move || c.checkpoint())
        };
        std::thread::sleep(Duration::from_millis(30));
        c.cancel();
        assert_eq!(worker.join().unwrap(), Err(Cancelled));
        assert_eq!(c.checkpoint(), Err(Cancelled));
    }
}
