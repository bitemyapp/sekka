//! The system as Settei sees it: `aizome-sys`'s services when live, and
//! a sample system (fixtures that change only in memory) for `--sample`
//! and the tests, so developing and testing never change the machine.
//!
//! The window shares the accounts ([`Accounts`]: the sidebar's card and
//! Users & Groups). Panes keep the other services they use themselves,
//! made in their `shown`; each service here pairs a live handle with its
//! sample in the same shape (a snapshot, and actions returning a
//! [`Task`]), so a pane is written once.

pub mod accounts;
pub mod sample;

use std::sync::{Arc, Mutex};

use aizome_sys::{Task, Wake};

pub use accounts::Accounts;

/// The services the window shares.
pub struct System {
    pub sample: bool,
    pub accounts: Accounts,
}

impl System {
    /// Nothing started yet: what `--list-settings` and the first view see.
    pub fn empty(sample: bool) -> System {
        System { sample, accounts: Accounts::empty() }
    }

    /// Start the shared services, waking the window on their changes.
    pub fn start(sample: bool, wake: Wake) -> System {
        let accounts = if sample { Accounts::sample(wake) } else { Accounts::live(wake) };
        System { sample, accounts }
    }
}

/// A sample service's state: changed in memory, waking the window like a
/// daemon's signal would.
pub struct Cell<S> {
    state: Mutex<Arc<S>>,
    wake: Wake,
}

impl<S: Clone> Cell<S> {
    pub fn new(state: S, wake: Wake) -> Cell<S> {
        Cell { state: Mutex::new(Arc::new(state)), wake }
    }

    pub fn get(&self) -> Arc<S> {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Change the state and wake the window: a finished task.
    pub fn update(&self, f: impl FnOnce(&mut S)) -> Task<()> {
        {
            let mut g = self.state.lock().unwrap_or_else(|e| e.into_inner());
            f(Arc::make_mut(&mut g));
        }
        self.wake.wake();
        Task::ready(Ok(()))
    }

    /// Refuse, as the live daemon would.
    pub fn refuse<T>(&self, e: aizome_sys::Error) -> Task<T> {
        Task::ready(Err(e))
    }
}
