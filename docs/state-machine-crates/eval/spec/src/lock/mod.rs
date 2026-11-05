//! `lock`: the shell's lock screen (Locking, Locked, the PAM check, the
//! refusal shake) with logind's sleep-delay inhibitor as an orthogonal
//! region (catalog SHB-12, SYB-12). See `SPEC.md`.

#[cfg(feature = "testing")]
pub mod env;
#[cfg(feature = "testing")]
pub mod traces;

use crate::Sink;

/// The highest output id (outputs are a `u32` mask).
pub const MAX_OUTPUT: u8 = 31;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Config {
    /// Lock before sleeping, holding logind's sleep delay until locked.
    pub lock_before_sleep: bool,
    /// Animations allowed: a refused password shakes the field.
    pub motion: bool,
    /// The outputs present at the start (bit `n`: output `n`).
    pub outputs: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self { lock_before_sleep: true, motion: true, outputs: 0b1 }
    }
}

/// Why PAM refused, as the check reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// faillock: too many failures.
    AccountLocked,
    /// The password expired: a new one is required.
    PasswordExpired,
    /// The account expired.
    AccountExpired,
    /// Anything else PAM said.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PamOutcome {
    Accepted,
    /// The password was wrong (`PAM_AUTH_ERR`).
    WrongPassword,
    /// PAM refused for another reason, which it named.
    Refused(Refusal),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Event {
    /// The shell started in a real session (`start_lock`).
    SessionStarted,
    /// Lock now (the person, idle, or logind's `Lock`).
    Lock,
    /// logind's `Unlock` (`loginctl unlock-session`).
    UnlockRequested,
    /// The compositor confirmed the session lock.
    LockConfirmed,
    /// The compositor ended (or refused) the lock.
    LockEnded,
    OutputAdded(u8),
    OutputRemoved(u8),
    /// The password field's text changed.
    Typed {
        empty: bool,
    },
    /// Escape: clears the field.
    Escape,
    /// Return in the field.
    Submit,
    /// The PAM check `check` finished.
    PamDone {
        check: u32,
        outcome: PamOutcome,
    },
    /// The shake's 30 ms timer fired.
    ShakeTick,
    /// logind's `PrepareForSleep(true)`.
    PrepareForSleep,
    /// logind's `PrepareForSleep(false)`.
    Resumed,
    /// logind left the bus (the inhibitor fd is gone with it).
    LogindVanished,
    /// logind is back.
    LogindAppeared,
}

/// What the lock surface says under the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LockNote {
    /// "That password didn't work. Try again."
    WrongPassword,
    /// PAM's own reason (locked out, expired…).
    Refused(Refusal),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Effect {
    /// Ask the compositor for a session lock (`lock_session`).
    RequestLock,
    /// End the session lock (`unlock_session`).
    ReleaseLock,
    OpenSurface(u8),
    CloseSurface(u8),
    /// logind's `LockedHint`.
    LockedHint(bool),
    /// Start PAM check `check` with the field's text, and clear the field.
    StartCheck {
        check: u32,
    },
    ShowNote(LockNote),
    /// The field's shake offset step (0: still).
    ShakeStep(u8),
    /// Arm (or re-arm) the shake's 30 ms timer.
    ArmShake,
    CancelShake,
    /// Take logind's sleep-delay inhibitor (`Inhibit("sleep", "delay")`).
    TakeDelay,
    /// Close the inhibitor fd: logind may sleep now (`sleep_ready`).
    ReleaseDelay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LockPhase {
    Unlocked,
    /// Asked the compositor; surfaces open; not confirmed.
    Locking,
    /// Locked, the field takes input.
    Locked,
    /// Locked, a PAM check running.
    Checking,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DelayPhase {
    NotHeld,
    Held,
    /// logind is waiting to sleep; the delay is held until the lock is confirmed.
    SleepPending,
}

/// The two orthogonal regions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StateName {
    pub lock: LockPhase,
    pub delay: DelayPhase,
}

/// An implementation of the lock machine.
pub trait Machine: Sized {
    fn new(config: Config) -> Self;
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>);
    fn state(&self) -> StateName;
}
