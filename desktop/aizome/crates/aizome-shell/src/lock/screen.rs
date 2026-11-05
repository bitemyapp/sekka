//! The lock screen's states as types: unlocked, locking (asked the
//! compositor, its surfaces open), locked (the prompt waiting) and
//! checking (PAM has a password). The shell's handlers in `lock.rs` match
//! on [`Stage`] through `LockState::with_typed` (crate-private) and call
//! the transitions here, which are the only way to change it.
//!
//! - **PAM's answer decides what the person is told.** A check is a
//!   ticket of a [`Generation`], and the PAM thread's answer ([`Checked`])
//!   carries it. [`Checking::pam_done`] takes the answer only proven
//!   fresh ([`Fresh`]), and reads PAM's verdict itself: a router can't
//!   pick a note for it, so a lockout or an expired password isn't told
//!   as a wrong password, and an answer from a check of an earlier lock
//!   can't unlock this one.
//! - **A sleep waits for this lock only.** logind's sleep
//!   ([`SleepCycle`]) is held in [`Locking`] alone, released when the lock
//!   is confirmed or ends, and dropped when the machine resumes without
//!   it; the other states let it go at once.
//!
//! The SHB-12 mistake, telling every refusal as a wrong password, has no
//! method to call:
//!
//! ```compile_fail,E0599
//! use aizome_shell::lock::screen::Checking;
//! fn refused(checking: Checking<'_>) {
//!     let _locked = checking.wrong_password();
//! }
//! ```
//!
//! nor a note to pass:
//!
//! ```compile_fail,E0599
//! use aizome_shell::lock::screen::{Checking, Note};
//! fn refused(checking: Checking<'_>, note: Note) {
//!     let _locked = checking.refused(note);
//! }
//! ```
//!
//! nor a field to write it in, as the shell did (`error` is only read):
//!
//! ```compile_fail,E0615
//! fn refused(shell: &mut aizome_shell::shell::Shell) {
//!     shell.lock.error = Some("That password didn't work. Try again.".into());
//! }
//! ```
//!
//! and the view shows only a note the machine made, which can't be made
//! elsewhere:
//!
//! ```compile_fail,E0423
//! use aizome_shell::lock::LockView;
//! use aizome_shell::lock::screen::Note;
//! fn refused(view: &mut LockView, wrong: Note) {
//!     view.error = Some(Note(wrong.0));
//! }
//! ```
//!
//! ```
//! use aizome_shell::lock::LockView;
//! fn refused(view: &mut LockView, shell: &aizome_shell::shell::Shell) {
//!     view.error = shell.lock.note().cloned();
//! }
//! ```
//!
//! The answer can't be made up instead of PAM's:
//!
//! ```compile_fail,E0451
//! use aizome_shell::lock::screen::Checked;
//! use aizome_shell::pam::{Code, Reply, Step};
//! fn answer(ticket: aizome_typestate::Ticket<aizome_shell::lock::screen::PamCheck>) -> Checked {
//!     let reply = Reply::Refused { step: Step::Authenticate, code: Code::AUTH_ERR, said: vec![], reason: String::new() };
//!     Checked { prompt: 1, ticket, reply }
//! }
//! ```
//!
//! nor applied without the check that it's this check's:
//!
//! ```compile_fail,E0308
//! use aizome_shell::lock::screen::{Checked, Checking, Io};
//! fn pam_done(checking: Checking<'_>, heard: Checked, io: &mut Io<'_, '_>) {
//!     let _ = checking.pam_done(heard, true, io);
//! }
//! ```
//!
//! What compiles hands PAM's answer over, proven fresh:
//!
//! ```
//! use aizome_shell::lock::screen::{Answered, Checked, Checking, Io, PamCheck, Stage};
//! use aizome_typestate::Fresh;
//! fn pam_done<'s>(checking: Checking<'s>, heard: Fresh<PamCheck, Checked>, io: &mut Io<'_, '_>) -> Stage<'s> {
//!     match checking.pam_done(heard, true, io) {
//!         Ok(Answered::Unlocked(unlocked)) => unlocked.into(),
//!         Ok(Answered::Locked(locked)) => locked.into(),
//!         Err(still_checking) => still_checking.into(),
//!     }
//! }
//! ```
//!
//! For SYB-12, a sleep can't be kept past its lock's confirmation to be
//! released by hand later: [`Locking`] keeps it, and [`Locked`] has no
//! place for one.
//!
//! ```compile_fail,E0616
//! use aizome_shell::lock::screen::Locking;
//! use aizome_sys::power::Logind;
//! fn lock_confirmed(locking: Locking<'_>, logind: &Logind) {
//!     if let Some(sleep) = locking.data {
//!         logind.sleep_ready(sleep);
//!     }
//! }
//! ```
//!
//! What compiles confirms the lock, which lets that sleep go:
//!
//! ```
//! use aizome_shell::lock::screen::{Io, Locked, Locking};
//! fn lock_confirmed<'s>(locking: Locking<'s>, io: &mut Io<'_, '_>) -> Locked<'s> {
//!     locking.confirmed(io)
//! }
//! ```

#![deny(clippy::wildcard_enum_match_arm)]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_sys::power::{Logind, SleepCycle};
use aizome_typestate::{Answer, Fresh, Generation, Ticket};

use super::{LockMsg, Phase};
use crate::pam::{Authenticator, Refusal, Reply, Verdict};
use crate::shell::{Msg, Surface};

/// The kind of request PAM's checks are counted in.
pub enum PamCheck {}

aizome_typestate::machine! {
    /// The lock screen, over what the prompt shows ([`Prompt`]).
    pub struct LockState over Prompt {
        #[derive(Debug)]
        stored Stored;
        /// The lock screen's state, typed.
        typed Stage;
        states {
            /// No lock asked for.
            Unlocked,
            /// Asked the compositor; the lock surfaces are open. Holds the
            /// sleep logind announced, if one waits for this lock.
            Locking(Option<SleepCycle>),
            /// Locked, the prompt waiting; what the last check said.
            Locked(Option<Note>),
            /// PAM is checking a password, under this check's ticket.
            Checking(Ticket<PamCheck>),
        }
        // Where PAM's answer leaves the lock, and where submitting the
        // prompt does (the macro takes no attributes on a branch).
        branches {
            Answered = Unlocked | Locked;
            Submitted = Locked | Checking;
        }
        access(private);
        new(private);
        with_typed(pub(crate));
    }
}

/// What the prompt shows, shared by the states.
#[derive(Debug)]
pub struct Prompt {
    /// This lock screen's number (`PROMPTS`): checks are counted per lock
    /// screen, so an answer to another's is never this one's.
    prompt: u64,
    checks: Generation<PamCheck>,
    password: String,
    caps: bool,
    /// The shake's step (0: still).
    shake: u32,
    /// When it locked, seconds since the epoch (notifications since show).
    since: u64,
    /// Each output's lock surface.
    surfaces: Vec<(WindowKey, u32)>,
}

/// Numbers each lock screen in the process. A lock screen made again (in
/// place of one with a check running) counts its checks from 1 too, and
/// the old check's answer must not pass for the new one's.
static PROMPTS: AtomicU64 = AtomicU64::new(0);

/// What the prompt says after a check that didn't unlock: only
/// [`Checking::pam_done`] makes one, from PAM's verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note(Told);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Told {
    WrongPassword,
    /// PAM refused for another reason, in its words.
    Refused(Refusal),
}

impl Note {
    pub fn text(&self) -> &str {
        match &self.0 {
            Told::WrongPassword => "That password didn't work. Try again.",
            Told::Refused(r) => r.text(),
        }
    }
}

/// PAM's answer to one check: only [`Locked::submit`]'s thread makes one.
#[derive(Clone, Debug, PartialEq)]
pub struct Checked {
    prompt: u64,
    ticket: Ticket<PamCheck>,
    reply: Reply,
}

impl Answer<PamCheck> for Checked {
    fn ticket(&self) -> Ticket<PamCheck> {
        self.ticket
    }
}

/// What the lock's transitions act through: the kit (the session lock,
/// PAM's thread, the shake's timer, focus), logind (the locked hint and
/// the sleep it waits for) and the shell's surfaces (which forget the lock
/// surfaces when it ends).
pub struct Io<'a, 'c> {
    cx: &'a mut Cx<'c, Msg>,
    logind: Option<&'a Logind>,
    surfaces: &'a mut HashMap<WindowKey, Surface>,
}

impl<'a, 'c> Io<'a, 'c> {
    pub(crate) fn new(
        cx: &'a mut Cx<'c, Msg>,
        logind: Option<&'a Logind>,
        surfaces: &'a mut HashMap<WindowKey, Surface>,
    ) -> Io<'a, 'c> {
        Io { cx, logind, surfaces }
    }

    /// Let logind sleep: the screen is locked for `sleep`, or won't be.
    fn let_sleep(&self, sleep: SleepCycle) {
        if let Some(l) = self.logind {
            l.sleep_ready(sleep);
        }
    }
}

impl LockState {
    /// No lock yet: for the shell's start, and only there. Made again in
    /// place of a lock, it forgets that lock (the compositor keeps it);
    /// the old check's answer is refused, as another lock screen's.
    pub(crate) fn unlocked() -> LockState {
        LockState::new(Prompt {
            prompt: PROMPTS.fetch_add(1, Ordering::Relaxed) + 1,
            checks: Generation::new(),
            password: String::new(),
            caps: false,
            shake: 0,
            since: 0,
            surfaces: Vec::new(),
        })
    }

    /// Where locking is, to read (the states change only through the
    /// transitions).
    pub fn phase(&self) -> Phase {
        match self.stored() {
            Stored::Unlocked(()) => Phase::Unlocked,
            Stored::Locking(_) => Phase::Locking,
            Stored::Locked(_) => Phase::Locked,
            Stored::Checking(_) => Phase::Checking,
        }
    }

    /// The password typed so far, one for every output's prompt.
    pub fn password(&self) -> &str {
        &self.ctx().password
    }

    /// What the prompt says under the field: the last check's note.
    pub fn note(&self) -> Option<&Note> {
        match self.stored() {
            Stored::Locked(note) => note.as_ref(),
            Stored::Unlocked(()) | Stored::Locking(_) | Stored::Checking(_) => None,
        }
    }

    /// The note's words.
    pub fn error(&self) -> Option<&str> {
        self.note().map(Note::text)
    }

    /// Whether a sleep waits for this lock to be confirmed.
    pub fn holds_sleep(&self) -> bool {
        match self.stored() {
            Stored::Locking(sleep) => sleep.is_some(),
            Stored::Unlocked(()) | Stored::Locked(_) | Stored::Checking(_) => false,
        }
    }

    pub fn caps(&self) -> bool {
        self.ctx().caps
    }

    pub fn shake(&self) -> u32 {
        self.ctx().shake
    }

    pub fn since(&self) -> u64 {
        self.ctx().since
    }

    /// Each output's lock surface, and the output.
    pub fn surfaces(&self) -> &[(WindowKey, u32)] {
        &self.ctx().surfaces
    }

    /// The lock surfaces to follow the outputs with, while there's a lock.
    pub(crate) fn surfaces_mut(&mut self) -> Option<&mut Vec<(WindowKey, u32)>> {
        match self.phase() {
            Phase::Unlocked => None,
            Phase::Locking | Phase::Locked | Phase::Checking => Some(&mut self.ctx.surfaces),
        }
    }

    pub(crate) fn set_caps(&mut self, on: bool) {
        self.ctx.caps = on;
    }

    pub(crate) fn set_shake(&mut self, step: u32) {
        self.ctx.shake = step;
    }

    /// PAM's answer, proven to answer the check running now; back again
    /// when a newer check or the lock's end made it stale.
    pub(crate) fn check(&self, heard: Checked) -> Result<Fresh<PamCheck, Checked>, Checked> {
        if heard.prompt != self.ctx().prompt {
            return Err(heard);
        }
        self.ctx().checks.check_answer(heard)
    }
}

impl<'s> Unlocked<'s> {
    /// Ask the compositor to lock.
    pub fn lock(self, io: &mut Io<'_, '_>) -> Locking<'s> {
        self.locking(None, io)
    }

    /// Lock for the sleep logind announced, which waits until the lock is
    /// confirmed (or ends).
    pub fn lock_for_sleep(self, sleep: SleepCycle, io: &mut Io<'_, '_>) -> Locking<'s> {
        self.locking(Some(sleep), io)
    }

    /// The sleep logind announced, without locking first.
    pub fn let_sleep(&self, sleep: SleepCycle, io: &Io<'_, '_>) {
        io.let_sleep(sleep);
    }

    fn locking(self, sleep: Option<SleepCycle>, io: &mut Io<'_, '_>) -> Locking<'s> {
        io.cx.lock_session();
        let p = &mut *self.ctx;
        p.password.clear();
        p.since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        Locking::enter(self.ctx, sleep)
    }
}

impl<'s> Locking<'s> {
    /// The compositor locked: the sleep waiting for it may go.
    pub fn confirmed(self, io: &mut Io<'_, '_>) -> Locked<'s> {
        if let Some(l) = io.logind {
            let _ = l.set_locked_hint(true);
        }
        if let Some(sleep) = self.data {
            io.let_sleep(sleep);
        }
        io.cx.focus("lock-password");
        Locked::enter(self.ctx, None)
    }

    /// Another sleep announced before the lock was confirmed: it waits
    /// instead (one before it is over).
    pub fn sleep(self, sleep: SleepCycle) -> Locking<'s> {
        Locking::enter(self.ctx, Some(sleep))
    }

    /// The machine woke: the sleep that waited (logind stops waiting
    /// after its limit) is over.
    pub fn resumed(self) -> Locking<'s> {
        Locking::enter(self.ctx, None)
    }

    pub fn typed(self, password: String) -> Locking<'s> {
        self.ctx.password = password;
        self
    }

    /// The compositor refused or ended the lock.
    pub fn ended(self, io: &mut Io<'_, '_>) -> Unlocked<'s> {
        if let Some(sleep) = self.data {
            io.let_sleep(sleep);
        }
        ended(self.ctx, io)
    }

    /// Withdraw the lock.
    pub fn unlock(self, io: &mut Io<'_, '_>) -> Unlocked<'s> {
        io.cx.unlock_session();
        self.ended(io)
    }
}

impl<'s> Locked<'s> {
    /// The person typed (or cleared the field): the last note goes.
    pub fn typed(self, password: String) -> Locked<'s> {
        self.ctx.password = password;
        Locked::enter(self.ctx, None)
    }

    /// Check the password typed with PAM, on a thread; nothing to check
    /// without one.
    pub fn submit(self, user: &str, auth: Arc<dyn Authenticator>, io: &mut Io<'_, '_>) -> Submitted<'s> {
        if self.ctx.password.is_empty() {
            return Submitted::Locked(self);
        }
        let ticket = self.ctx.checks.issue();
        let prompt = self.ctx.prompt;
        let user = user.to_owned();
        let password = std::mem::take(&mut self.ctx.password);
        io.cx.spawn(move || {
            let reply = auth.authenticate(&user, &password);
            Msg::Lock(LockMsg::Checked(Checked { prompt, ticket, reply }))
        });
        Submitted::Checking(Checking::enter(self.ctx, ticket))
    }

    /// The sleep logind announced: the screen is locked already.
    pub fn let_sleep(&self, sleep: SleepCycle, io: &Io<'_, '_>) {
        io.let_sleep(sleep);
    }

    /// The compositor ended the lock.
    pub fn ended(self, io: &mut Io<'_, '_>) -> Unlocked<'s> {
        ended(self.ctx, io)
    }

    pub fn unlock(self, io: &mut Io<'_, '_>) -> Unlocked<'s> {
        io.cx.unlock_session();
        ended(self.ctx, io)
    }
}

impl<'s> Checking<'s> {
    /// PAM answered this check: unlock, or say why not. The verdict is
    /// PAM's ([`Reply::verdict`]); `motion` is whether the field may shake.
    /// A proof made for an earlier check gives the state back.
    pub fn pam_done(
        self,
        heard: Fresh<PamCheck, Checked>,
        motion: bool,
        io: &mut Io<'_, '_>,
    ) -> Result<Answered<'s>, Checking<'s>> {
        // A proof is fresh when it's made; one kept until a later check
        // isn't this check's.
        let answer = heard.answer();
        if answer.prompt != self.ctx.prompt || answer.ticket != self.data || !self.ctx.checks.is_current(self.data) {
            return Err(self);
        }
        self.ctx.checks.retire();
        let reply = heard.into_answer().reply;
        let note = match reply.verdict() {
            Verdict::Accepted => {
                io.cx.unlock_session();
                return Ok(Answered::Unlocked(ended(self.ctx, io)));
            }
            Verdict::WrongPassword => Note(Told::WrongPassword),
            Verdict::Refused(r) => Note(Told::Refused(r)),
        };
        // What PAM said, as it said it (the step, code, pam_strerror and
        // the modules' messages; never the password).
        log::info!("unlocking: refused, {reply:?}");
        if motion {
            self.ctx.shake = 1;
            io.cx.after(Duration::from_millis(30), Msg::Lock(LockMsg::Shake(2)));
        }
        io.cx.focus("lock-password");
        Ok(Answered::Locked(Locked::enter(self.ctx, Some(note))))
    }

    /// The sleep logind announced: the screen is locked already.
    pub fn let_sleep(&self, sleep: SleepCycle, io: &Io<'_, '_>) {
        io.let_sleep(sleep);
    }

    /// The compositor ended the lock: whatever PAM answers now is stale.
    pub fn ended(self, io: &mut Io<'_, '_>) -> Unlocked<'s> {
        ended(self.ctx, io)
    }

    pub fn unlock(self, io: &mut Io<'_, '_>) -> Unlocked<'s> {
        io.cx.unlock_session();
        ended(self.ctx, io)
    }
}

/// The lock is over: its surfaces, password and checks go.
fn ended<'s>(p: &'s mut Prompt, io: &mut Io<'_, '_>) -> Unlocked<'s> {
    for (key, _) in std::mem::take(&mut p.surfaces) {
        io.surfaces.remove(&key);
    }
    p.password.clear();
    p.checks.retire();
    if let Some(l) = io.logind {
        let _ = l.set_locked_hint(false);
    }
    Unlocked::enter(p, ())
}
