//! `lock` as two enums, one per orthogonal region; the lock region's
//! variants carry the field and shake data of their superstates.

use spec::Sink;
use spec::lock::{Config, DelayPhase, Effect, Event, LockNote, LockPhase, Machine, PamOutcome, StateName};

pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Unlocked,
    Locking,
    Locked { has_text: bool, shake: u8 },
    Checking { shake: u8, check: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delay {
    NotHeld,
    Held,
    SleepPending,
}

/// The machine. Its fields are private: the state changes only through
/// `Machine::handle`; [`Lock::typed`] reads it.
pub struct Lock {
    config: Config,
    outputs: u32,
    last_check: u32,
    phase: Phase,
    delay: Delay,
}

impl Lock {
    /// The current state, read-only.
    pub fn typed(&self) -> (&Phase, &Delay) {
        (&self.phase, &self.delay)
    }
}

impl Lock {
    fn enter_lockscreen(&mut self, out: &mut impl Sink<Effect>) {
        out.push(Effect::RequestLock);
        let mut m = self.outputs;
        while m != 0 {
            let o = m.trailing_zeros() as u8;
            out.push(Effect::OpenSurface(o));
            m &= m - 1;
        }
        self.phase = Phase::Locking;
    }

    fn leave_lockscreen(&mut self, release: bool, out: &mut impl Sink<Effect>) {
        if let Phase::Locked { shake, .. } | Phase::Checking { shake, .. } = self.phase
            && shake > 0
        {
            out.push(Effect::CancelShake);
        }
        if release {
            out.push(Effect::ReleaseLock);
        }
        out.push(Effect::LockedHint(false));
        if self.delay == Delay::SleepPending {
            out.push(Effect::ReleaseDelay);
            self.delay = Delay::NotHeld;
        }
        self.phase = Phase::Unlocked;
    }

    fn take_delay(&mut self, out: &mut impl Sink<Effect>) {
        if self.config.lock_before_sleep && self.delay == Delay::NotHeld {
            out.push(Effect::TakeDelay);
            self.delay = Delay::Held;
        }
    }

    fn refused(&mut self, note: LockNote, out: &mut impl Sink<Effect>) {
        out.push(Effect::ShowNote(note));
        let shake = if self.config.motion {
            out.push(Effect::ShakeStep(1));
            out.push(Effect::ArmShake);
            1
        } else {
            0
        };
        self.phase = Phase::Locked { has_text: false, shake };
    }
}

impl Machine for Lock {
    fn new(config: Config) -> Self {
        Self { config, outputs: config.outputs, last_check: 0, phase: Phase::Unlocked, delay: Delay::NotHeld }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let shown = self.phase != Phase::Unlocked;
        match event {
            Event::Lock => {
                if self.phase == Phase::Unlocked {
                    self.enter_lockscreen(out);
                }
            }
            Event::LockConfirmed => {
                if self.phase == Phase::Locking {
                    out.push(Effect::LockedHint(true));
                    if self.delay == Delay::SleepPending {
                        out.push(Effect::ReleaseDelay);
                        self.delay = Delay::NotHeld;
                    }
                    self.phase = Phase::Locked { has_text: false, shake: 0 };
                }
            }
            Event::LockEnded => {
                if shown {
                    self.leave_lockscreen(false, out);
                }
            }
            Event::UnlockRequested => {
                if shown {
                    self.leave_lockscreen(true, out);
                }
            }
            Event::OutputAdded(o) => {
                let bit = 1u32 << (o & 31);
                if self.outputs & bit == 0 {
                    self.outputs |= bit;
                    if shown {
                        out.push(Effect::OpenSurface(o));
                    }
                }
            }
            Event::OutputRemoved(o) => {
                let bit = 1u32 << (o & 31);
                if self.outputs & bit != 0 {
                    self.outputs &= !bit;
                    if shown {
                        out.push(Effect::CloseSurface(o));
                    }
                }
            }
            Event::Typed { empty } => {
                if let Phase::Locked { has_text, .. } = &mut self.phase {
                    *has_text = !empty;
                }
            }
            Event::Escape => {
                if let Phase::Locked { has_text, .. } = &mut self.phase {
                    *has_text = false;
                }
            }
            Event::Submit => {
                if let Phase::Locked { has_text: true, shake } = self.phase {
                    self.last_check += 1;
                    out.push(Effect::StartCheck { check: self.last_check });
                    self.phase = Phase::Checking { shake, check: self.last_check };
                }
            }
            Event::PamDone { check, outcome } => {
                if let Phase::Checking { check: c, .. } = self.phase
                    && c == check
                {
                    match outcome {
                        PamOutcome::Accepted => self.leave_lockscreen(true, out),
                        PamOutcome::WrongPassword => self.refused(LockNote::WrongPassword, out),
                        PamOutcome::Refused(r) => self.refused(LockNote::Refused(r), out),
                    }
                }
            }
            Event::ShakeTick => {
                if let Phase::Locked { shake, .. } | Phase::Checking { shake, .. } = &mut self.phase
                    && *shake > 0
                {
                    *shake += 1;
                    if *shake >= 9 {
                        *shake = 0;
                        out.push(Effect::ShakeStep(0));
                    } else {
                        out.push(Effect::ShakeStep(*shake));
                        out.push(Effect::ArmShake);
                    }
                }
            }
            Event::SessionStarted | Event::LogindAppeared => self.take_delay(out),
            Event::PrepareForSleep => {
                if !self.config.lock_before_sleep {
                    return;
                }
                match self.phase {
                    Phase::Unlocked | Phase::Locking => {
                        if self.phase == Phase::Unlocked {
                            self.enter_lockscreen(out);
                        }
                        if self.delay == Delay::Held {
                            self.delay = Delay::SleepPending;
                        }
                    }
                    Phase::Locked { .. } | Phase::Checking { .. } => {
                        if self.delay == Delay::Held {
                            out.push(Effect::ReleaseDelay);
                            self.delay = Delay::NotHeld;
                        }
                    }
                }
            }
            Event::Resumed => match self.delay {
                Delay::SleepPending => self.delay = Delay::Held,
                Delay::NotHeld => self.take_delay(out),
                Delay::Held => {}
            },
            Event::LogindVanished => self.delay = Delay::NotHeld,
        }
    }

    fn state(&self) -> StateName {
        StateName {
            lock: match self.phase {
                Phase::Unlocked => LockPhase::Unlocked,
                Phase::Locking => LockPhase::Locking,
                Phase::Locked { .. } => LockPhase::Locked,
                Phase::Checking { .. } => LockPhase::Checking,
            },
            delay: match self.delay {
                Delay::NotHeld => DelayPhase::NotHeld,
                Delay::Held => DelayPhase::Held,
                Delay::SleepPending => DelayPhase::SleepPending,
            },
        }
    }
}
