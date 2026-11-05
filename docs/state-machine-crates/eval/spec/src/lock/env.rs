//! A random person, compositor, PAM and logind for the differential
//! property tests.

use super::{Effect, Event, PamOutcome, Refusal, StateName};
use crate::testing::Tape;

#[derive(Default)]
pub struct Env {
    check: u32,
    shaking: bool,
}

fn outcome(t: &mut Tape) -> PamOutcome {
    match t.below(10) {
        0..4 => PamOutcome::Accepted,
        4..7 => PamOutcome::WrongPassword,
        7 => PamOutcome::Refused(Refusal::AccountLocked),
        8 => PamOutcome::Refused(Refusal::PasswordExpired),
        _ => PamOutcome::Refused(*t.pick(&[Refusal::AccountExpired, Refusal::Other])),
    }
}

impl Env {
    pub fn next(&mut self, t: &mut Tape, _state: StateName, effects: &[Effect]) -> Event {
        for e in effects {
            match e {
                Effect::StartCheck { check } => self.check = *check,
                Effect::ArmShake => self.shaking = true,
                Effect::CancelShake | Effect::ShakeStep(0) => self.shaking = false,
                _ => {}
            }
        }
        if self.shaking && t.chance(400) {
            return Event::ShakeTick;
        }
        match t.below(100) {
            0..8 => Event::Lock,
            8..16 => Event::LockConfirmed,
            16..20 => Event::LockEnded,
            20..23 => Event::UnlockRequested,
            23..27 => Event::OutputAdded(t.below(5) as u8),
            27..31 => Event::OutputRemoved(t.below(5) as u8),
            31..42 => Event::Typed { empty: t.chance(200) },
            42..45 => Event::Escape,
            45..55 => Event::Submit,
            55..66 => Event::PamDone { check: self.check.saturating_sub(t.below(2)), outcome: outcome(t) },
            66..70 => Event::ShakeTick,
            70..78 => Event::PrepareForSleep,
            78..86 => Event::Resumed,
            86..90 => Event::SessionStarted,
            90..95 => Event::LogindVanished,
            _ => Event::LogindAppeared,
        }
    }
}
