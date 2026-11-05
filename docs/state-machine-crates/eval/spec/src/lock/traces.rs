//! Conformance traces for `lock`.

use super::{
    Config, DelayPhase as D, Effect as F, Event as E, LockNote, LockPhase as L, PamOutcome, Refusal, StateName,
};
use crate::testing::{Driven, Tape, Target, Trace};

/// The `lock` target.
pub struct Spec;

impl<M: super::Machine> Driven<Spec> for M {
    fn boot(config: Config) -> Self {
        M::new(config)
    }
    fn step(&mut self, event: E, out: &mut Vec<F>) {
        self.handle(event, out);
    }
    fn observe(&self) -> StateName {
        self.state()
    }
}

impl Target for Spec {
    const NAME: &'static str = "lock";
    type Config = Config;
    type Event = E;
    type Effect = F;
    type State = StateName;
    type Env = super::env::Env;

    fn traces() -> Vec<Trace<Self>> {
        traces()
    }
    fn configs() -> Vec<Config> {
        vec![
            Config::default(),
            Config { lock_before_sleep: false, motion: true, outputs: 0b101 },
            Config { lock_before_sleep: true, motion: false, outputs: 0 },
            Config { lock_before_sleep: true, motion: true, outputs: 0b1011 },
        ]
    }
    fn env(_: &Config) -> super::env::Env {
        super::env::Env::default()
    }
    fn next(env: &mut super::env::Env, tape: &mut Tape, state: StateName, effects: &[F]) -> E {
        env.next(tape, state, effects)
    }
}

const fn s(lock: L, delay: D) -> StateName {
    StateName { lock, delay }
}
const TEXT: E = E::Typed { empty: false };
fn done(check: u32, outcome: PamOutcome) -> E {
    E::PamDone { check, outcome }
}

/// Lock and confirm with output 0 and no delay held.
fn locked(name: &'static str, about: &'static str, c: Config) -> Trace<Spec> {
    Trace::new(name, about, c).step(E::Lock, [F::RequestLock, F::OpenSurface(0)], s(L::Locking, D::NotHeld)).step(
        E::LockConfirmed,
        [F::LockedHint(true)],
        s(L::Locked, D::NotHeld),
    )
}

pub fn traces() -> Vec<Trace<Spec>> {
    let c = Config::default();
    let still = Config { motion: false, ..c };
    let mut v = vec![
        locked("lock-and-unlock", "lock, confirm, the right password unlocks", c)
            .step(TEXT, [], s(L::Locked, D::NotHeld))
            .step(E::Submit, [F::StartCheck { check: 1 }], s(L::Checking, D::NotHeld))
            .step(done(1, PamOutcome::Accepted), [F::ReleaseLock, F::LockedHint(false)], s(L::Unlocked, D::NotHeld)),
        locked("no-shake-with-reduced-motion", "a wrong password, animations off: just the note", still)
            .step(TEXT, [], s(L::Locked, D::NotHeld))
            .step(E::Submit, [F::StartCheck { check: 1 }], s(L::Checking, D::NotHeld))
            .step(done(1, PamOutcome::WrongPassword), [F::ShowNote(LockNote::WrongPassword)], s(L::Locked, D::NotHeld))
            .step(E::ShakeTick, [], s(L::Locked, D::NotHeld)),
        locked("unlock-cancels-the-shake", "leaving the lock screen cancels a running shake", c)
            .step(TEXT, [], s(L::Locked, D::NotHeld))
            .step(E::Submit, [F::StartCheck { check: 1 }], s(L::Checking, D::NotHeld))
            .step(
                done(1, PamOutcome::WrongPassword),
                [F::ShowNote(LockNote::WrongPassword), F::ShakeStep(1), F::ArmShake],
                s(L::Locked, D::NotHeld),
            )
            .step(TEXT, [], s(L::Locked, D::NotHeld))
            .step(E::Submit, [F::StartCheck { check: 2 }], s(L::Checking, D::NotHeld))
            .step(E::ShakeTick, [F::ShakeStep(2), F::ArmShake], s(L::Checking, D::NotHeld))
            .step(
                done(2, PamOutcome::Accepted),
                [F::CancelShake, F::ReleaseLock, F::LockedHint(false)],
                s(L::Unlocked, D::NotHeld),
            )
            .step(E::ShakeTick, [], s(L::Unlocked, D::NotHeld)),
        Trace::new(
            "submit-needs-text-and-a-confirmed-lock",
            "the field works only once locked, not while a check runs",
            c,
        )
        .step(E::Lock, [F::RequestLock, F::OpenSurface(0)], s(L::Locking, D::NotHeld))
        .step(TEXT, [], s(L::Locking, D::NotHeld))
        .step(E::Submit, [], s(L::Locking, D::NotHeld))
        .step(E::LockConfirmed, [F::LockedHint(true)], s(L::Locked, D::NotHeld))
        .step(E::Submit, [], s(L::Locked, D::NotHeld))
        .step(TEXT, [], s(L::Locked, D::NotHeld))
        .step(E::Escape, [], s(L::Locked, D::NotHeld))
        .step(E::Submit, [], s(L::Locked, D::NotHeld))
        .step(TEXT, [], s(L::Locked, D::NotHeld))
        .step(E::Typed { empty: true }, [], s(L::Locked, D::NotHeld))
        .step(E::Submit, [], s(L::Locked, D::NotHeld))
        .step(TEXT, [], s(L::Locked, D::NotHeld))
        .step(E::Submit, [F::StartCheck { check: 1 }], s(L::Checking, D::NotHeld))
        .step(TEXT, [], s(L::Checking, D::NotHeld))
        .step(E::Submit, [], s(L::Checking, D::NotHeld))
        .step(
            done(1, PamOutcome::WrongPassword),
            [F::ShowNote(LockNote::WrongPassword), F::ShakeStep(1), F::ArmShake],
            s(L::Locked, D::NotHeld),
        )
        .step(E::Submit, [], s(L::Locked, D::NotHeld)),
        locked("stale-pam-result-after-relock", "a check from an ended lock doesn't act on the next one", c)
            .step(TEXT, [], s(L::Locked, D::NotHeld))
            .step(E::Submit, [F::StartCheck { check: 1 }], s(L::Checking, D::NotHeld))
            .step(E::LockEnded, [F::LockedHint(false)], s(L::Unlocked, D::NotHeld))
            .step(E::Lock, [F::RequestLock, F::OpenSurface(0)], s(L::Locking, D::NotHeld))
            .step(E::LockConfirmed, [F::LockedHint(true)], s(L::Locked, D::NotHeld))
            .step(done(1, PamOutcome::Accepted), [], s(L::Locked, D::NotHeld))
            .step(TEXT, [], s(L::Locked, D::NotHeld))
            .step(E::Submit, [F::StartCheck { check: 2 }], s(L::Checking, D::NotHeld))
            .step(done(1, PamOutcome::WrongPassword), [], s(L::Checking, D::NotHeld))
            .step(done(2, PamOutcome::Accepted), [F::ReleaseLock, F::LockedHint(false)], s(L::Unlocked, D::NotHeld))
            .step(done(2, PamOutcome::Accepted), [], s(L::Unlocked, D::NotHeld)),
        Trace::new(
            "outputs-follow-while-locked",
            "surfaces open and close with outputs, only while locking or locked",
            Config { outputs: 0b011, ..c },
        )
        .step(E::Lock, [F::RequestLock, F::OpenSurface(0), F::OpenSurface(1)], s(L::Locking, D::NotHeld))
        .step(E::OutputRemoved(1), [F::CloseSurface(1)], s(L::Locking, D::NotHeld))
        .step(E::OutputRemoved(1), [], s(L::Locking, D::NotHeld))
        .step(E::OutputAdded(2), [F::OpenSurface(2)], s(L::Locking, D::NotHeld))
        .step(E::OutputAdded(2), [], s(L::Locking, D::NotHeld))
        .step(E::LockConfirmed, [F::LockedHint(true)], s(L::Locked, D::NotHeld))
        .step(E::UnlockRequested, [F::ReleaseLock, F::LockedHint(false)], s(L::Unlocked, D::NotHeld))
        .step(E::OutputAdded(3), [], s(L::Unlocked, D::NotHeld))
        .step(E::OutputRemoved(0), [], s(L::Unlocked, D::NotHeld))
        .step(E::Lock, [F::RequestLock, F::OpenSurface(2), F::OpenSurface(3)], s(L::Locking, D::NotHeld)),
        Trace::new("sleep-locks-first", "the delay is held until the lock is confirmed, then taken again on resume", c)
            .step(E::SessionStarted, [F::TakeDelay], s(L::Unlocked, D::Held))
            .step(E::PrepareForSleep, [F::RequestLock, F::OpenSurface(0)], s(L::Locking, D::SleepPending))
            .step(E::LockConfirmed, [F::LockedHint(true), F::ReleaseDelay], s(L::Locked, D::NotHeld))
            .step(E::Resumed, [F::TakeDelay], s(L::Locked, D::Held))
            .step(E::PrepareForSleep, [F::ReleaseDelay], s(L::Locked, D::NotHeld))
            .step(E::Resumed, [F::TakeDelay], s(L::Locked, D::Held))
            .step(E::SessionStarted, [], s(L::Locked, D::Held)),
        Trace::new(
            "sleep-while-locking-waits-for-the-lock",
            "logind asks to sleep before the lock is confirmed: hold the delay",
            c,
        )
        .step(E::SessionStarted, [F::TakeDelay], s(L::Unlocked, D::Held))
        .step(E::Lock, [F::RequestLock, F::OpenSurface(0)], s(L::Locking, D::Held))
        .step(E::PrepareForSleep, [], s(L::Locking, D::SleepPending))
        .step(E::PrepareForSleep, [], s(L::Locking, D::SleepPending))
        .step(E::LockConfirmed, [F::LockedHint(true), F::ReleaseDelay], s(L::Locked, D::NotHeld)),
        Trace::new("lock-ended-releases-a-pending-sleep", "the compositor refused the lock: let logind sleep", c)
            .step(E::SessionStarted, [F::TakeDelay], s(L::Unlocked, D::Held))
            .step(E::PrepareForSleep, [F::RequestLock, F::OpenSurface(0)], s(L::Locking, D::SleepPending))
            .step(E::LockEnded, [F::LockedHint(false), F::ReleaseDelay], s(L::Unlocked, D::NotHeld))
            .step(E::Resumed, [F::TakeDelay], s(L::Unlocked, D::Held)),
        Trace::new(
            "unlock-request-while-a-sleep-waits",
            "logind's Unlock during a pending sleep unlocks and lets it sleep",
            c,
        )
        .step(E::SessionStarted, [F::TakeDelay], s(L::Unlocked, D::Held))
        .step(E::PrepareForSleep, [F::RequestLock, F::OpenSurface(0)], s(L::Locking, D::SleepPending))
        .step(
            E::UnlockRequested,
            [F::ReleaseLock, F::LockedHint(false), F::ReleaseDelay],
            s(L::Unlocked, D::NotHeld),
        ),
        Trace::new(
            "no-delay-when-not-locking-before-sleep",
            "with lock-before-sleep off nothing is held or locked",
            Config { lock_before_sleep: false, ..c },
        )
        .step(E::SessionStarted, [], s(L::Unlocked, D::NotHeld))
        .step(E::PrepareForSleep, [], s(L::Unlocked, D::NotHeld))
        .step(E::Resumed, [], s(L::Unlocked, D::NotHeld))
        .step(E::LogindAppeared, [], s(L::Unlocked, D::NotHeld)),
        Trace::new("logind-goes-and-comes-back", "a vanished logind took the fd; its return retakes it", c)
            .step(E::SessionStarted, [F::TakeDelay], s(L::Unlocked, D::Held))
            .step(E::LogindVanished, [], s(L::Unlocked, D::NotHeld))
            .step(E::PrepareForSleep, [F::RequestLock, F::OpenSurface(0)], s(L::Locking, D::NotHeld))
            .step(E::LogindAppeared, [F::TakeDelay], s(L::Locking, D::Held))
            .step(E::LogindAppeared, [], s(L::Locking, D::Held))
            .step(E::LockConfirmed, [F::LockedHint(true)], s(L::Locked, D::Held)),
        Trace::new("vanished-during-a-pending-sleep", "nothing to release once logind took the fd away", c)
            .step(E::SessionStarted, [F::TakeDelay], s(L::Unlocked, D::Held))
            .step(E::PrepareForSleep, [F::RequestLock, F::OpenSurface(0)], s(L::Locking, D::SleepPending))
            .step(E::LogindVanished, [], s(L::Locking, D::NotHeld))
            .step(E::LockConfirmed, [F::LockedHint(true)], s(L::Locked, D::NotHeld)),
        Trace::new("late-and-stray-compositor-events", "confirmations and ends outside their phase change nothing", c)
            .step(E::LockConfirmed, [], s(L::Unlocked, D::NotHeld))
            .step(E::LockEnded, [], s(L::Unlocked, D::NotHeld))
            .step(E::UnlockRequested, [], s(L::Unlocked, D::NotHeld))
            .step(done(1, PamOutcome::Accepted), [], s(L::Unlocked, D::NotHeld))
            .step(E::Lock, [F::RequestLock, F::OpenSurface(0)], s(L::Locking, D::NotHeld))
            .step(E::Lock, [], s(L::Locking, D::NotHeld))
            .step(E::LockConfirmed, [F::LockedHint(true)], s(L::Locked, D::NotHeld))
            .step(E::LockConfirmed, [], s(L::Locked, D::NotHeld))
            .step(E::Lock, [], s(L::Locked, D::NotHeld)),
        // Confirmed bugs, as the correct behaviour.
        locked(
            "a-lockout-says-why",
            "faillock refuses the right password: say the account is locked, not that it was wrong",
            c,
        )
        .bug("SHB-12")
        .step(TEXT, [], s(L::Locked, D::NotHeld))
        .step(E::Submit, [F::StartCheck { check: 1 }], s(L::Checking, D::NotHeld))
        .step(
            done(1, PamOutcome::Refused(Refusal::AccountLocked)),
            [F::ShowNote(LockNote::Refused(Refusal::AccountLocked)), F::ShakeStep(1), F::ArmShake],
            s(L::Locked, D::NotHeld),
        ),
        locked("an-expired-password-says-why", "PAM requires a new password: say so", still)
            .bug("SHB-12")
            .step(TEXT, [], s(L::Locked, D::NotHeld))
            .step(E::Submit, [F::StartCheck { check: 1 }], s(L::Checking, D::NotHeld))
            .step(
                done(1, PamOutcome::Refused(Refusal::PasswordExpired)),
                [F::ShowNote(LockNote::Refused(Refusal::PasswordExpired))],
                s(L::Locked, D::NotHeld),
            ),
        Trace::new(
            "lock-confirmed-after-resume-keeps-the-next-delay",
            "the lock wasn't confirmed before logind slept; after resume the confirmation must not drop the delay",
            c,
        )
        .bug("SYB-12")
        .step(E::SessionStarted, [F::TakeDelay], s(L::Unlocked, D::Held))
        .step(E::PrepareForSleep, [F::RequestLock, F::OpenSurface(0)], s(L::Locking, D::SleepPending))
        .step(E::Resumed, [], s(L::Locking, D::Held))
        .step(E::LockConfirmed, [F::LockedHint(true)], s(L::Locked, D::Held))
        .step(E::PrepareForSleep, [F::ReleaseDelay], s(L::Locked, D::NotHeld)),
    ];
    // The full shake: steps 1..8, then still.
    let mut t = locked("wrong-password-shakes", "a refused password shakes the field over nine 30 ms steps", c)
        .step(TEXT, [], s(L::Locked, D::NotHeld))
        .step(E::Submit, [F::StartCheck { check: 1 }], s(L::Checking, D::NotHeld))
        .step(
            done(1, PamOutcome::WrongPassword),
            [F::ShowNote(LockNote::WrongPassword), F::ShakeStep(1), F::ArmShake],
            s(L::Locked, D::NotHeld),
        );
    for step in 2..9 {
        t = t.step(E::ShakeTick, [F::ShakeStep(step), F::ArmShake], s(L::Locked, D::NotHeld));
    }
    t = t.step(E::ShakeTick, [F::ShakeStep(0)], s(L::Locked, D::NotHeld)).step(
        E::ShakeTick,
        [],
        s(L::Locked, D::NotHeld),
    );
    v.push(t);
    v
}
