//! `lock` in the shell's shape: `LockState { phase, surfaces, busy, error,
//! shake, sleep_pending }` (lock.rs:47-66, with its `Phase` enum: partly
//! explicit already) and the logind follower's held delay (`delay:
//! Option<OwnedFd>`, logind.rs:143), changed by each handler's `if`s.

use crate::BUGS;
use spec::Sink;
use spec::lock::{Config, DelayPhase, Effect, Event, LockNote, LockPhase, Machine, PamOutcome, StateName};

pub const IMPLEMENTED: bool = true;

/// lock.rs's `Phase`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Unlocked,
    Locking,
    Locked,
}

/// The machine's fields, in the current code's shape. Readable through
/// the machine (`Deref`); only `handle` changes them.
pub struct Fields {
    pub config: Config,
    pub phase: Phase,
    /// Outputs (bit n), each with a lock surface while not unlocked.
    pub outputs: u32,
    pub has_text: bool,
    /// A PAM thread runs.
    pub busy: bool,
    /// SPEC-FIX: the check the result must name (the shell takes any).
    pub check: u32,
    pub error: Option<LockNote>,
    pub shake: u32,
    /// Holding logind's sleep until locked.
    pub sleep_pending: bool,
    /// The follower's inhibitor fd.
    pub delay_held: bool,
}

impl Fields {
    fn lock(&mut self, out: &mut impl Sink<Effect>) {
        if self.phase != Phase::Unlocked {
            return;
        }
        out.push(Effect::RequestLock);
        self.phase = Phase::Locking;
        self.has_text = false;
        self.error = None;
        for o in 0..32u8 {
            if self.outputs & (1 << o) != 0 {
                out.push(Effect::OpenSurface(o));
            }
        }
    }

    fn sleep_ready(&mut self, out: &mut impl Sink<Effect>) {
        if self.delay_held {
            out.push(Effect::ReleaseDelay);
            self.delay_held = false;
        }
    }

    /// `lock_ended`, after `unlock_session` when `release`.
    fn lock_ended(&mut self, release: bool, out: &mut impl Sink<Effect>) {
        // SPEC-FIX: the shake stops with the lock screen.
        if self.shake > 0 && self.phase == Phase::Locked {
            out.push(Effect::CancelShake);
        }
        self.shake = 0;
        if release {
            out.push(Effect::ReleaseLock);
        }
        self.phase = Phase::Unlocked;
        self.has_text = false;
        self.busy = false;
        self.error = None;
        out.push(Effect::LockedHint(false));
        if self.sleep_pending {
            self.sleep_pending = false;
            self.sleep_ready(out);
        }
    }

    fn take_sleep_delay(&mut self, out: &mut impl Sink<Effect>) {
        if self.config.lock_before_sleep && !self.delay_held {
            out.push(Effect::TakeDelay);
            self.delay_held = true;
        }
    }
}

impl Fields {
    fn new(config: Config) -> Self {
        Self {
            config,
            phase: Phase::Unlocked,
            outputs: config.outputs,
            has_text: false,
            busy: false,
            check: 0,
            error: None,
            shake: 0,
            sleep_pending: false,
            delay_held: false,
        }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        match event {
            Event::Lock => self.lock(out),
            Event::LockConfirmed => {
                // SPEC-FIX: only while locking (the shell relies on the kit
                // ignoring a late SessionLocked).
                if self.phase != Phase::Locking {
                    return;
                }
                self.phase = Phase::Locked;
                out.push(Effect::LockedHint(true));
                if self.sleep_pending {
                    self.sleep_pending = false;
                    // BUG(SYB-12): after a resume, this drops the delay just
                    // taken for the next sleep.
                    self.sleep_ready(out);
                }
            }
            Event::LockEnded => {
                if self.phase != Phase::Unlocked {
                    self.lock_ended(false, out);
                }
            }
            Event::UnlockRequested => {
                if self.phase != Phase::Unlocked {
                    self.lock_ended(true, out);
                }
            }
            Event::OutputAdded(o) => {
                let bit = 1u32 << (o & 31);
                if self.outputs & bit == 0 {
                    self.outputs |= bit;
                    if self.phase != Phase::Unlocked {
                        out.push(Effect::OpenSurface(o));
                    }
                }
            }
            Event::OutputRemoved(o) => {
                let bit = 1u32 << (o & 31);
                if self.outputs & bit != 0 {
                    self.outputs &= !bit;
                    if self.phase != Phase::Unlocked {
                        out.push(Effect::CloseSurface(o));
                    }
                }
            }
            Event::Typed { empty } => {
                // SPEC-FIX: the field takes input once locked.
                if !self.busy && self.phase == Phase::Locked {
                    self.has_text = !empty;
                }
            }
            Event::Escape => {
                if !self.busy && self.phase == Phase::Locked {
                    self.has_text = false;
                }
            }
            Event::Submit => {
                if self.busy || !self.has_text || self.phase != Phase::Locked {
                    return;
                }
                self.busy = true;
                self.has_text = false;
                self.check += 1;
                out.push(Effect::StartCheck { check: self.check });
            }
            Event::PamDone { check, outcome } => {
                if !self.busy || check != self.check {
                    return;
                }
                self.busy = false;
                let note = match outcome {
                    PamOutcome::Accepted => return self.lock_ended(true, out),
                    PamOutcome::WrongPassword => LockNote::WrongPassword,
                    // BUG(SHB-12): whatever PAM said becomes "That password
                    // didn't work".
                    PamOutcome::Refused(r) => {
                        if BUGS {
                            LockNote::WrongPassword
                        } else {
                            LockNote::Refused(r)
                        }
                    }
                };
                self.error = Some(note);
                out.push(Effect::ShowNote(note));
                if self.config.motion {
                    self.shake = 1;
                    out.push(Effect::ShakeStep(1));
                    out.push(Effect::ArmShake);
                }
            }
            Event::ShakeTick => {
                if self.shake == 0 || self.phase != Phase::Locked {
                    return;
                }
                self.shake += 1;
                if self.shake >= 9 {
                    self.shake = 0;
                    out.push(Effect::ShakeStep(0));
                } else {
                    out.push(Effect::ShakeStep(self.shake as u8));
                    out.push(Effect::ArmShake);
                }
            }
            Event::SessionStarted | Event::LogindAppeared => self.take_sleep_delay(out),
            Event::PrepareForSleep => {
                if !self.config.lock_before_sleep {
                    return;
                }
                match self.phase {
                    Phase::Unlocked | Phase::Locking => {
                        // SPEC-FIX: also while locking (the shell lets it sleep at
                        // once), and only with a delay to hold.
                        self.sleep_pending = self.delay_held;
                        self.lock(out);
                    }
                    Phase::Locked => self.sleep_ready(out),
                }
            }
            Event::Resumed => {
                if BUGS {
                    // BUG(SYB-12): the shell takes the delay again even while
                    // one is held, and the pending sleep stays pending.
                    if self.config.lock_before_sleep {
                        out.push(Effect::TakeDelay);
                        self.delay_held = true;
                    }
                } else if self.sleep_pending {
                    self.sleep_pending = false;
                } else {
                    self.take_sleep_delay(out);
                }
            }
            Event::LogindVanished => {
                self.delay_held = false;
                self.sleep_pending = false;
            }
        }
    }

    fn state(&self) -> StateName {
        StateName {
            lock: match self.phase {
                Phase::Unlocked => LockPhase::Unlocked,
                Phase::Locking => LockPhase::Locking,
                Phase::Locked if self.busy => LockPhase::Checking,
                Phase::Locked => LockPhase::Locked,
            },
            delay: if self.delay_held && self.sleep_pending {
                DelayPhase::SleepPending
            } else if self.delay_held {
                DelayPhase::Held
            } else {
                DelayPhase::NotHeld
            },
        }
    }
}

/// The machine: [`Fields`] read through `Deref`, never written from outside.
pub struct Lock(Fields);

impl std::ops::Deref for Lock {
    type Target = Fields;
    fn deref(&self) -> &Fields {
        &self.0
    }
}

impl Machine for Lock {
    fn new(config: Config) -> Self {
        Lock(Fields::new(config))
    }
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        self.0.handle(event, out);
    }
    fn state(&self) -> StateName {
        self.0.state()
    }
}
