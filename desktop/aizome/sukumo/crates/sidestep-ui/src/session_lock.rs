//! The program's session lock, as the main thread keeps it: not asked
//! for, asked for, or held. [`Cx`](crate::Cx) drives it from
//! [`Cx::lock_session`](crate::Cx::lock_session),
//! [`Cx::unlock_session`](crate::Cx::unlock_session) and what the render
//! thread hears from the compositor.
//!
//! Each request is a ticket of a [`Generation`], sent with
//! `ToRender::LockSession` as a [`LockRequest`], and the render thread's
//! answers are [`LockAnswer`]s naming the request they answer. Only the
//! render thread makes an answer, so the main thread can't apply one for
//! a request it picks itself. A transition that applies an answer takes it
//! only as [`Fresh`], which only the comparison with the current request
//! makes, so the compositor's `finished` for a lock the program has
//! withdrawn can't end the lock it asks for next. Unlocking retires the
//! request, so nothing heard about it afterwards is fresh.
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
//! nor with the request in place of the proof:
//!
//! ```compile_fail,E0308
//! use sidestep_ui::session_lock::{Asked, LockRequest};
//! fn session_lock_finished(asked: Asked<'_>, lock: LockRequest) {
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
//! An answer can't be made outside this module:
//!
//! ```compile_fail,E0423
//! use sidestep_ui::session_lock::{Ended, LockAnswer};
//! fn heard(lock: LockAnswer) -> Ended {
//!     Ended(lock)
//! }
//! ```
//!
//! and the render thread's answer can't be made for a request the router
//! picks, such as the one it asks for now: taking a finished for the
//! current lock's is the UIW-27 mistake.
//!
//! ```compile_fail,E0624
//! use sidestep_ui::session_lock::{LockAnswer, LockRequest};
//! fn finished_for(current: LockRequest) -> LockAnswer {
//!     LockAnswer::heard(current)
//! }
//! ```
//!
//! What compiles takes the answer the render thread sent:
//!
//! ```
//! use sidestep_engine::protocol::FromRender;
//! use sidestep_ui::session_lock::LockAnswer;
//! fn finished(message: FromRender) -> Option<LockAnswer> {
//!     match message {
//!         FromRender::SessionLockFinished { lock } => Some(lock),
//!         _ => None,
//!     }
//! }
//! ```
//!
//! and ends the lock with the answer proven to be this request's:
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
//!
//! (With the `testing` feature, which only tests enable,
//! `LockAnswer::forged` makes any answer: tests play the compositor.)

#![deny(clippy::wildcard_enum_match_arm)]

use std::sync::atomic::{AtomicU64, Ordering};

use aizome_typestate::{Answer, Fresh, Generation, Ticket};
use sidestep_engine::protocol::ToRender;
pub use sidestep_engine::protocol::{LockAnswer, LockRequest, SessionLockRequest};
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use crate::window::send;

aizome_typestate::machine! {
    /// The session lock's state, over this machine's lock requests.
    pub struct SessionLock over Requests {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        stored(Copy) Stored;
        /// The session lock's state, typed: what [`Cx`](crate::Cx)'s
        /// transitions match on.
        typed Stage;
        states {
            /// Neither asked for nor held.
            Unlocked,
            /// Asked for under this request, not confirmed yet.
            Asked(LockRequest),
            /// The compositor locked the session for this request.
            Locked(LockRequest),
        }
        access(private);
        new(private);
        with_typed(pub(crate));
    }
}

/// The requests of one lock machine: its number in the process, and its
/// tickets.
pub struct Requests {
    machine: u64,
    tickets: Generation<SessionLockRequest>,
}

/// Numbers each lock machine in the process. One made again counts its
/// tickets from 1 too, and an answer to the old one's must not pass for
/// the new one's.
static MACHINES: AtomicU64 = AtomicU64::new(0);

/// The compositor locked the session for a request (`SessionLocked`).
#[derive(Debug)]
pub struct Confirmed(LockAnswer);

/// The compositor refused or ended the lock asked for under a request
/// (`SessionLockFinished`).
#[derive(Debug)]
pub struct Ended(LockAnswer);

impl Answer<SessionLockRequest> for Confirmed {
    fn ticket(&self) -> Ticket<SessionLockRequest> {
        self.0.request().ticket
    }
}

impl Answer<SessionLockRequest> for Ended {
    fn ticket(&self) -> Ticket<SessionLockRequest> {
        self.0.request().ticket
    }
}

impl SessionLock {
    /// No lock yet: for `App::run`'s `Cx`, and only there. Made again in
    /// place of a lock asked for or held, it forgets that lock (the
    /// compositor keeps it, its surfaces close) and the program believes
    /// it unlocked, which is UIW-27's outcome; routing a `finished` to
    /// `unlock_session` is too. What the render thread says about the old
    /// machine's requests is refused: they're another machine's.
    pub(crate) fn unlocked() -> SessionLock {
        SessionLock::new(Requests { machine: MACHINES.fetch_add(1, Ordering::Relaxed) + 1, tickets: Generation::new() })
    }

    /// The render thread's `SessionLocked`, if it answers the current
    /// request; `None` for one since withdrawn or another machine's.
    pub(crate) fn confirmed(&self, heard: LockAnswer) -> Option<Fresh<SessionLockRequest, Confirmed>> {
        if heard.request().machine != self.ctx().machine {
            return None;
        }
        self.ctx().tickets.check_answer(Confirmed(heard)).ok()
    }

    /// The render thread's `SessionLockFinished`, if it answers the
    /// current request; `None` for one since withdrawn or another
    /// machine's.
    pub(crate) fn ended(&self, heard: LockAnswer) -> Option<Fresh<SessionLockRequest, Ended>> {
        if heard.request().machine != self.ctx().machine {
            return None;
        }
        self.ctx().tickets.check_answer(Ended(heard)).ok()
    }

    /// Whether the compositor holds the lock for the current request.
    pub(crate) fn is_locked(&self) -> bool {
        match self.stored() {
            Stored::Locked(_) => true,
            Stored::Unlocked(()) | Stored::Asked(_) => false,
        }
    }
}

impl<'s> Unlocked<'s> {
    /// Ask the compositor for the lock, under a new request.
    pub fn lock(self, tx: &Sender<ToRender>) -> Asked<'s> {
        let lock = LockRequest { machine: self.ctx.machine, ticket: self.ctx.tickets.issue() };
        send(tx, ToRender::LockSession { lock });
        Asked::enter(self.ctx, lock)
    }
}

impl<'s> Asked<'s> {
    /// The compositor locked the session for this request; a proof made
    /// for an earlier request gives the state back.
    pub fn confirmed(self, heard: Fresh<SessionLockRequest, Confirmed>) -> Result<Locked<'s>, Asked<'s>> {
        if answers(self.ctx, self.data, &heard.answer().0) { Ok(Locked::enter(self.ctx, self.data)) } else { Err(self) }
    }

    /// The compositor refused the lock asked for under this request; a
    /// proof made for an earlier request gives the state back.
    pub fn ended(self, heard: Fresh<SessionLockRequest, Ended>) -> Result<Unlocked<'s>, Asked<'s>> {
        if answers(self.ctx, self.data, &heard.answer().0) { Ok(withdrawn(self.ctx)) } else { Err(self) }
    }

    /// Stop asking: the render thread withdraws the lock, and whatever
    /// it hears about this request from now on is stale.
    pub fn unlock(self, tx: &Sender<ToRender>) -> Unlocked<'s> {
        send(tx, ToRender::UnlockSession);
        withdrawn(self.ctx)
    }
}

impl<'s> Locked<'s> {
    /// The compositor ended the lock held for this request; a proof made
    /// for an earlier request gives the state back.
    pub fn ended(self, heard: Fresh<SessionLockRequest, Ended>) -> Result<Unlocked<'s>, Locked<'s>> {
        if answers(self.ctx, self.data, &heard.answer().0) { Ok(withdrawn(self.ctx)) } else { Err(self) }
    }

    /// Unlock: whatever the render thread hears about this request from
    /// now on is stale.
    pub fn unlock(self, tx: &Sender<ToRender>) -> Unlocked<'s> {
        send(tx, ToRender::UnlockSession);
        withdrawn(self.ctx)
    }
}

/// Whether `heard` answers `request`, the state's, and that's still the
/// current request: a proof is fresh when it's made, and one kept until a
/// later request isn't this request's.
fn answers(requests: &Requests, request: LockRequest, heard: &LockAnswer) -> bool {
    heard.request() == request && requests.tickets.is_current(request.ticket)
}

/// Unlocked, with the request over.
fn withdrawn(requests: &mut Requests) -> Unlocked<'_> {
    requests.tickets.retire();
    Unlocked::enter(requests, ())
}
