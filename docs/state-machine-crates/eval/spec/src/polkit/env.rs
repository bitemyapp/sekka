//! A random polkitd, helper and person for the differential property tests.
//! It answers the machine's requests most of the time (the helper's lines
//! for the conversation just started, the timer it armed) and otherwise
//! sends stray, stale or out-of-order events.

use super::{Cookie, Effect, Event, StateName};
use crate::testing::Tape;

#[derive(Default)]
pub struct Env {
    /// The newest conversation the machine started.
    conv: u32,
    armed: bool,
}

impl Env {
    pub fn next(&mut self, t: &mut Tape, _state: StateName, effects: &[Effect]) -> Event {
        for e in effects {
            match e {
                Effect::StartHelper { conv, .. } => self.conv = *conv,
                Effect::ArmRetry { .. } => self.armed = true,
                Effect::CancelRetry => self.armed = false,
                _ => {}
            }
        }
        if self.armed && t.chance(600) {
            self.armed = false;
            return Event::RetryTimer;
        }
        let cookie = Cookie(1 + t.below(3));
        match t.below(100) {
            0..30 => self.helper(t, self.conv),
            30..36 => {
                let back = 1 + t.below(2);
                self.helper(t, self.conv.saturating_sub(back))
            }
            36..52 => Event::Submit,
            52..58 => Event::ChooseIdentity(t.below(3) as u8),
            58..63 => Event::RetryTimer,
            63..68 => Event::Dismiss,
            68..82 => Event::Begin {
                cookie,
                identities: t.below(4) as u8,
                preferred: t.below(4) as u8,
                screen: !t.chance(100),
            },
            82..88 => Event::CancelRequest { cookie },
            88..90 => Event::AuthorityChanged,
            _ => self.helper(t, self.conv + 1),
        }
    }

    fn helper(&self, t: &mut Tape, conv: u32) -> Event {
        match t.below(100) {
            0..35 => Event::Prompt { conv, echo: t.chance(200) },
            35..55 => Event::Failure { conv },
            55..65 => Event::Unreachable { conv },
            65..75 => Event::Success { conv },
            75..90 => Event::PamError { conv },
            _ => Event::PamInfo { conv },
        }
    }
}
