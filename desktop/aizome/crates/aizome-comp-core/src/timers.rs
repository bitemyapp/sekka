//! Timers for the logic. The logic never puts its own closures on the
//! event loop: one made by a library that has since been swapped out would
//! run its old code against the new library's state. It asks the core
//! instead, with a token, and the core calls whichever logic is loaded when
//! the time comes ([`crate::LogicVTable::timer`]); timers a previous logic
//! asked for are dropped at a reload.

use std::time::Duration;

use smithay::reexports::calloop::RegistrationToken;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};

use crate::state::State;

/// A timer the logic asked for, to cancel it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerId(RegistrationToken);

impl State {
    /// Calls the logic's `timer` with `token` after `after`, if the same
    /// logic is still loaded then.
    pub fn schedule(&mut self, after: Duration, token: u64) -> Option<TimerId> {
        let generation = self.lg.generation();
        self.az
            .lh
            .insert_source(Timer::from_duration(after), move |_, _, state: &mut State| {
                if state.lg.generation() == generation {
                    state.lg_call((), |vt, st| (vt.timer)(st, token));
                }
                TimeoutAction::Drop
            })
            .ok()
            .map(TimerId)
    }

    pub fn cancel_timer(&mut self, id: TimerId) {
        self.az.lh.remove(id.0);
    }
}
