//! The program's session lock, as the main thread keeps it: not asked
//! for, asked for, or held. [`Cx`](crate::Cx) drives it from
//! [`Cx::lock_session`](crate::Cx::lock_session),
//! [`Cx::unlock_session`](crate::Cx::unlock_session) and what the render
//! thread hears from the compositor.
//!
//! Each request is a ticket of a [`Generation`], sent with
//! `ToRender::LockSession`, and the render thread's answers name the
//! request they answer. A transition that applies an answer takes it only
//! as [`Fresh`], which only the comparison with the current request makes,
//! so the compositor's `finished` for a lock the program has withdrawn
//! can't end the lock it asks for next. Unlocking retires the request, so
//! nothing heard about it afterwards is fresh.
//!
//! Ending a lock without checking which request the answer is for, as
//! `SessionLockFinished` used to, doesn't compile:
//!
//! ```compile_fail,E0061
//! use sidestep_ui::session_lock::Asked;
//! fn session_lock_finished(asked: Asked<'_>) {
//!     let _unlocked = asked.ended();
//! }
//! ```
//!
//! nor with the request's ticket in place of the proof:
//!
//! ```compile_fail,E0308
//! use sidestep_ui::session_lock::{Asked, LockTicket};
//! fn session_lock_finished(asked: Asked<'_>, lock: LockTicket) {
//!     let _unlocked = asked.ended(lock);
//! }
//! ```
//!
//! nor with a confirmation's proof:
//!
//! ```compile_fail,E0308
//! use aizome_typestate::Fresh;
//! use sidestep_ui::session_lock::{Asked, Confirmed, SessionLockRequest};
//! fn session_lock_finished(asked: Asked<'_>, heard: Fresh<SessionLockRequest, Confirmed>) {
//!     let _unlocked = asked.ended(heard);
//! }
//! ```
//!
//! and an answer can't be made outside this module:
//!
//! ```compile_fail,E0423
//! use sidestep_ui::session_lock::{Ended, LockTicket};
//! fn heard(lock: LockTicket) -> Ended {
//!     Ended(lock)
//! }
//! ```
//!
//! What compiles is ending the lock with the answer proven to be this
//! request's:
//!
//! ```
//! use aizome_typestate::Fresh;
//! use sidestep_ui::session_lock::{Asked, Ended, SessionLockRequest, Stage};
//! fn session_lock_finished<'s>(asked: Asked<'s>, heard: Fresh<SessionLockRequest, Ended>) -> Stage<'s> {
//!     match asked.ended(heard) {
//!         Ok(unlocked) => unlocked.into(),
//!         Err(still_asked) => still_asked.into(),
//!     }
//! }
//! ```

#![deny(clippy::wildcard_enum_match_arm)]

use aizome_typestate::{Answer, Fresh, Generation};
use sidestep_engine::protocol::ToRender;
pub use sidestep_engine::protocol::{LockTicket, SessionLockRequest};
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use crate::window::send;

aizome_typestate::machine! {
    /// The session lock's state, over the generation of lock requests.
    pub struct SessionLock over Generation<SessionLockRequest> {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        stored(Copy) Stored;
        /// The session lock's state, typed: what [`Cx`](crate::Cx)'s
        /// transitions match on.
        typed Stage;
        states {
            /// Neither asked for nor held.
            Unlocked,
            /// Asked for under this request, not confirmed yet.
            Asked(LockTicket),
            /// The compositor locked the session for this request.
            Locked(LockTicket),
        }
        access(private);
        new(pub(crate));
        with_typed(pub(crate));
    }
}

/// The compositor locked the session for a request (`SessionLocked`).
#[derive(Debug)]
pub struct Confirmed(LockTicket);

/// The compositor refused or ended the lock asked for under a request
/// (`SessionLockFinished`).
#[derive(Debug)]
pub struct Ended(LockTicket);

impl Answer<SessionLockRequest> for Confirmed {
    fn ticket(&self) -> LockTicket {
        self.0
    }
}

impl Answer<SessionLockRequest> for Ended {
    fn ticket(&self) -> LockTicket {
        self.0
    }
}

impl SessionLock {
    /// No lock yet.
    pub(crate) fn unlocked() -> SessionLock {
        SessionLock::new(Generation::new())
    }

    /// The render thread's `SessionLocked` for `lock`, if it answers the
    /// current request; `None` for a request since withdrawn.
    pub(crate) fn confirmed(&self, lock: LockTicket) -> Option<Fresh<SessionLockRequest, Confirmed>> {
        self.ctx().check_answer(Confirmed(lock)).ok()
    }

    /// The render thread's `SessionLockFinished` for `lock`, if it answers
    /// the current request; `None` for a request since withdrawn.
    pub(crate) fn ended(&self, lock: LockTicket) -> Option<Fresh<SessionLockRequest, Ended>> {
        self.ctx().check_answer(Ended(lock)).ok()
    }

    /// Whether the compositor holds the lock for the current request.
    pub(crate) fn is_locked(&self) -> bool {
        match self.stored() {
            Stored::Locked(_) => true,
            Stored::Unlocked(()) | Stored::Asked(_) => false,
        }
    }

    /// The request asked for or held (`testing`).
    pub(crate) fn request(&self) -> Option<LockTicket> {
        match *self.stored() {
            Stored::Unlocked(()) => None,
            Stored::Asked(lock) | Stored::Locked(lock) => Some(lock),
        }
    }
}

impl<'s> Unlocked<'s> {
    /// Ask the compositor for the lock, under a new request.
    pub fn lock(self, tx: &Sender<ToRender>) -> Asked<'s> {
        let lock = self.ctx.issue();
        send(tx, ToRender::LockSession { lock });
        Asked::enter(self.ctx, lock)
    }
}

impl<'s> Asked<'s> {
    /// The compositor locked the session for this request; a proof made
    /// for an earlier request gives the state back.
    pub fn confirmed(self, heard: Fresh<SessionLockRequest, Confirmed>) -> Result<Locked<'s>, Asked<'s>> {
        if self.answers(heard.answer()) { Ok(Locked::enter(self.ctx, self.data)) } else { Err(self) }
    }

    /// The compositor refused the lock asked for under this request; a
    /// proof made for an earlier request gives the state back.
    pub fn ended(self, heard: Fresh<SessionLockRequest, Ended>) -> Result<Unlocked<'s>, Asked<'s>> {
        if self.answers(heard.answer()) { Ok(withdrawn(self.ctx)) } else { Err(self) }
    }

    /// Stop asking: the render thread withdraws the lock, and whatever
    /// it hears about this request from now on is stale.
    pub fn unlock(self, tx: &Sender<ToRender>) -> Unlocked<'s> {
        send(tx, ToRender::UnlockSession);
        withdrawn(self.ctx)
    }

    fn answers(&self, heard: &impl Answer<SessionLockRequest>) -> bool {
        // A proof is fresh when it's made; one kept until a later request
        // isn't this request's.
        heard.ticket() == self.data && self.ctx.is_current(self.data)
    }
}

impl<'s> Locked<'s> {
    /// The compositor ended the lock held for this request; a proof made
    /// for an earlier request gives the state back.
    pub fn ended(self, heard: Fresh<SessionLockRequest, Ended>) -> Result<Unlocked<'s>, Locked<'s>> {
        if heard.answer().ticket() == self.data && self.ctx.is_current(self.data) {
            Ok(withdrawn(self.ctx))
        } else {
            Err(self)
        }
    }

    /// Unlock: whatever the render thread hears about this request from
    /// now on is stale.
    pub fn unlock(self, tx: &Sender<ToRender>) -> Unlocked<'s> {
        send(tx, ToRender::UnlockSession);
        withdrawn(self.ctx)
    }
}

/// Unlocked, with the request over.
fn withdrawn(requests: &mut Generation<SessionLockRequest>) -> Unlocked<'_> {
    requests.retire();
    Unlocked::enter(requests, ())
}
