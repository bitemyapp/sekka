//! A random helper environment and GUI for the differential property
//! tests: usually the running stage's own result, sometimes a failure, a
//! cancel, a confirmation, or an out-of-order event.

use super::{Event, GIB, StateName};
use crate::testing::Tape;

fn size(t: &mut Tape) -> u64 {
    u64::from(t.below(120)) * GIB / 2
}

fn expected(t: &mut Tape, state: StateName) -> Event {
    match state {
        StateName::Idle => Event::Begin,
        StateName::Checks => Event::Checked { root_mib: 20 * GIB + size(t) },
        StateName::Configuration => Event::Configured,
        StateName::Evaluation => {
            Event::Evaluated { download_mib: size(t), unpacked_mib: size(t) * 3, budget_mib: size(t) }
        }
        StateName::Build => Event::Built { closure_mib: size(t) / 2 },
        StateName::Cache => Event::Cached,
        StateName::Prepared => Event::Confirm { phrase_ok: !t.chance(100) },
        _ => Event::StepDone,
    }
}

pub fn event(t: &mut Tape, state: StateName) -> Event {
    match t.below(100) {
        0..82 => expected(t, state),
        82..86 => Event::StageFailed,
        86..90 => Event::Cancel,
        90..92 => Event::Confirm { phrase_ok: t.chance(800) },
        _ => {
            let other = *t.pick(&[
                StateName::Idle,
                StateName::Checks,
                StateName::Configuration,
                StateName::Evaluation,
                StateName::Build,
                StateName::Cache,
                StateName::Erase,
            ]);
            expected(t, other)
        }
    }
}
