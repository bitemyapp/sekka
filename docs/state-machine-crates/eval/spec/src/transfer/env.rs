//! A random file system and person for the differential property tests:
//! most events answer the machine's last request, with random results;
//! some are stray (another item, another kind of response).

use super::{AsideId, Effect, Event, Found, ItemId, Placed, Resolution, StateName};
use crate::testing::Tape;

#[derive(Default)]
pub struct Env {
    /// Ids handed out to merged folders' children.
    next_child: u32,
    asides: u32,
    /// The last request awaiting an answer.
    pending: Option<Effect>,
}

fn found(t: &mut Tape) -> Found {
    match t.below(10) {
        0..4 => Found::Free,
        4..6 => Found::Conflict { can_merge: false },
        6..8 => Found::Conflict { can_merge: true },
        8 => Found::Same,
        _ => Found::Refused,
    }
}

fn resolution(t: &mut Tape) -> Resolution {
    match t.below(20) {
        0..3 => Resolution::Skip,
        3 => Resolution::Cancel,
        4..7 => Resolution::KeepBoth,
        7..12 => Resolution::Merge,
        _ => Resolution::Replace,
    }
}

fn placed(t: &mut Tape) -> Placed {
    match t.below(40) {
        0..31 => Placed::Ok,
        31..38 => Placed::Skip,
        _ => Placed::Cancel,
    }
}

impl Env {
    pub fn next(&mut self, t: &mut Tape, state: StateName, effects: &[Effect]) -> Event {
        if let Some(e) = effects
            .iter()
            .rev()
            .find(|e| !matches!(e, Effect::Outcome { .. } | Effect::Problem { .. } | Effect::RemoveEmptySource { .. }))
        {
            self.pending = Some(*e);
        }
        if state == StateName::Idle && !t.chance(100) {
            return Event::Start { items: 1 + t.below(6) };
        }
        if t.chance(120) {
            return self.stray(t);
        }
        match self.pending {
            Some(Effect::Probe { item }) => Event::Probed { item, found: found(t) },
            Some(Effect::Ask { item, .. }) => Event::Answered { item, resolution: resolution(t) },
            Some(Effect::PlaceAt { item, .. }) => Event::Placed { item, outcome: placed(t) },
            Some(Effect::SetAside { item }) => {
                self.asides += 1;
                Event::SetAside { item, aside: (!t.chance(150)).then_some(AsideId(self.asides)) }
            }
            Some(Effect::Trash { item, .. } | Effect::Delete { item, .. }) => {
                Event::Disposed { item, ok: !t.chance(150) }
            }
            Some(Effect::RestoreAside { item, .. }) => Event::Restored { item, ok: !t.chance(200) },
            Some(Effect::List { item }) => {
                let count = t.below(4);
                let first = ItemId(1000 + self.next_child);
                self.next_child += count;
                Event::Listed { item, first, count }
            }
            _ => self.stray(t),
        }
    }

    fn stray(&mut self, t: &mut Tape) -> Event {
        let item = ItemId(t.below(3));
        match t.below(8) {
            0 => Event::Start { items: t.below(3) },
            1 => Event::Probed { item, found: found(t) },
            2 => Event::Answered { item, resolution: resolution(t) },
            3 => Event::SetAside { item, aside: Some(AsideId(99)) },
            4 => Event::Placed { item, outcome: placed(t) },
            5 => Event::Disposed { item, ok: t.chance(500) },
            6 => Event::Restored { item, ok: t.chance(500) },
            _ => Event::Listed { item, first: ItemId(5000 + t.below(10)), count: t.below(2) },
        }
    }
}
