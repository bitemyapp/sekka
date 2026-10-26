//! A one-value channel whose receiver is a future: served D-Bus methods
//! (the Bluetooth pairing agent, the polkit agent, notification actions)
//! await the UI's answer with it, without blocking zbus's executor.

use crate::service::lock;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

struct State<T> {
    value: Option<T>,
    waker: Option<Waker>,
    closed: bool,
}

/// Sends the value. Dropping it unsent makes the receiver yield `None`.
pub(crate) struct Sender<T> {
    state: Arc<Mutex<State<T>>>,
}

/// Resolves to the value, or `None` if the sender was dropped.
pub(crate) struct Receiver<T> {
    state: Arc<Mutex<State<T>>>,
}

pub(crate) fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let state = Arc::new(Mutex::new(State { value: None, waker: None, closed: false }));
    (Sender { state: state.clone() }, Receiver { state })
}

impl<T> Sender<T> {
    pub(crate) fn send(self, value: T) {
        let mut state = lock(&self.state);
        state.value = Some(value);
        if let Some(waker) = state.waker.take() {
            waker.wake();
        }
        // Drop runs next and only marks it closed.
    }

    /// Whether the receiver is gone (the caller gave up).
    pub(crate) fn is_closed(&self) -> bool {
        Arc::strong_count(&self.state) < 2
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let mut state = lock(&self.state);
        state.closed = true;
        if let Some(waker) = state.waker.take() {
            waker.wake();
        }
    }
}

impl<T> Future for Receiver<T> {
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let mut state = lock(&self.state);
        if let Some(value) = state.value.take() {
            return Poll::Ready(Some(value));
        }
        if state.closed {
            return Poll::Ready(None);
        }
        state.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_and_drops_arrive() {
        let (tx, rx) = channel();
        std::thread::spawn(move || tx.send(5));
        assert_eq!(futures_lite::future::block_on(rx), Some(5));
        let (tx, rx) = channel::<u8>();
        drop(tx);
        assert_eq!(futures_lite::future::block_on(rx), None);
    }
}
