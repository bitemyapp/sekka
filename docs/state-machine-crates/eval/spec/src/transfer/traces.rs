//! Conformance traces for `transfer`.

use super::{
    AsideId, Config, Effect as F, Event as E, Found, ItemId, JobKind, Mode, Name, Outcome, Placed, Problem,
    Resolution as R, StateName as S,
};
use crate::testing::{Driven, Tape, Target, Trace};

/// The `transfer` target.
pub struct Spec;

impl<M: super::Machine> Driven<Spec> for M {
    fn boot(config: Config) -> Self {
        M::new(config)
    }
    fn step(&mut self, event: E, out: &mut Vec<F>) {
        self.handle(event, out);
    }
    fn observe(&self) -> S {
        self.state()
    }
}

impl Target for Spec {
    const NAME: &'static str = "transfer";
    type Config = Config;
    type Event = E;
    type Effect = F;
    type State = S;
    type Env = super::env::Env;

    fn traces() -> Vec<Trace<Self>> {
        traces()
    }
    fn terminal(state: S) -> bool {
        state == S::Finished
    }
    fn configs() -> Vec<Config> {
        let mut v = Vec::new();
        for mode in [Mode::Copy, Mode::Move] {
            for kind in [JobKind::Transfer, JobKind::Duplicate, JobKind::Restore] {
                for delete_replaced in [false, true] {
                    v.push(Config { mode, kind, delete_replaced });
                }
            }
        }
        v
    }
    fn env(_: &Config) -> super::env::Env {
        super::env::Env::default()
    }
    fn next(env: &mut super::env::Env, tape: &mut Tape, state: S, effects: &[F]) -> E {
        env.next(tape, state, effects)
    }
}

const fn i(n: u32) -> ItemId {
    ItemId(n)
}
const A: AsideId = AsideId(7);

fn probed(n: u32, found: Found) -> E {
    E::Probed { item: i(n), found }
}
fn answered(n: u32, resolution: R) -> E {
    E::Answered { item: i(n), resolution }
}
fn placed(n: u32, outcome: Placed) -> E {
    E::Placed { item: i(n), outcome }
}
fn probe(n: u32) -> F {
    F::Probe { item: i(n) }
}
fn place(n: u32) -> F {
    F::PlaceAt { item: i(n), name: Name::Target }
}
fn outcome(n: u32, o: Outcome) -> F {
    F::Outcome { item: i(n), outcome: o }
}
const CONFLICT: Found = Found::Conflict { can_merge: false };
const FOLDERS: Found = Found::Conflict { can_merge: true };
const DONE: F = F::Finished { cancelled: false };
const CANCELLED: F = F::Finished { cancelled: true };

/// Item `n`, in conflict, answered Replace, set aside as `A`, then placed.
fn replace_until_placed(t: Trace<Spec>, n: u32) -> Trace<Spec> {
    t.step(probed(n, CONFLICT), [F::Ask { item: i(n), can_merge: false }], S::Asking)
        .step(answered(n, R::Replace), [F::SetAside { item: i(n) }], S::SettingAside)
        .step(E::SetAside { item: i(n), aside: Some(A) }, [place(n)], S::PlacingReplacement)
}

pub fn traces() -> Vec<Trace<Spec>> {
    let copy = Config::default();
    let move_ = Config { mode: Mode::Move, ..copy };
    let delete = Config { delete_replaced: true, ..copy };
    let put_back = Config { mode: Mode::Move, kind: JobKind::Restore, delete_replaced: false };
    vec![
        Trace::new("empty-job", "nothing to place", copy).step(E::Start { items: 0 }, [DONE], S::Finished),
        Trace::new("free-targets-are-placed", "no conflict: placed; a failed placement is left out", copy)
            .step(E::Start { items: 2 }, [probe(0)], S::Probing)
            .step(probed(0, Found::Free), [place(0)], S::Placing)
            .step(placed(0, Placed::Ok), [outcome(0, Outcome::Done), probe(1)], S::Probing)
            .step(probed(1, Found::Free), [place(1)], S::Placing)
            .step(placed(1, Placed::Skip), [outcome(1, Outcome::Failed), DONE], S::Finished),
        Trace::new("conflict-skip-and-keep-both", "Skip leaves it out; Keep Both places it under a free name", copy)
            .step(E::Start { items: 2 }, [probe(0)], S::Probing)
            .step(probed(0, CONFLICT), [F::Ask { item: i(0), can_merge: false }], S::Asking)
            .step(answered(0, R::Skip), [outcome(0, Outcome::Skipped), probe(1)], S::Probing)
            .step(probed(1, FOLDERS), [F::Ask { item: i(1), can_merge: true }], S::Asking)
            .step(answered(1, R::KeepBoth), [F::PlaceAt { item: i(1), name: Name::Free }], S::Placing)
            .step(placed(1, Placed::Ok), [outcome(1, Outcome::Done), DONE], S::Finished),
        replace_until_placed(
            Trace::new(
                "replace-in-a-transfer-trashes",
                "Replace: the old item goes to the Trash once the new one is in place",
                copy,
            )
            .step(E::Start { items: 1 }, [probe(0)], S::Probing),
            0,
        )
        .step(placed(0, Placed::Ok), [F::Trash { item: i(0), aside: A }], S::Disposing)
        .step(E::Disposed { item: i(0), ok: true }, [outcome(0, Outcome::Done), DONE], S::Finished),
        replace_until_placed(
            Trace::new(
                "replace-with-permanent-deletion",
                "permanent deletion was chosen: the old item is deleted",
                delete,
            )
            .step(E::Start { items: 1 }, [probe(0)], S::Probing),
            0,
        )
        .step(placed(0, Placed::Ok), [F::Delete { item: i(0), aside: A }], S::Disposing)
        .step(
            E::Disposed { item: i(0), ok: false },
            [F::Problem { item: i(0), problem: Problem::NotAllRemoved }, outcome(0, Outcome::Done), DONE],
            S::Finished,
        ),
        replace_until_placed(
            Trace::new(
                "trash-failure-keeps-the-replaced-item",
                "if the Trash refuses it, the old item stays under its aside name: never deleted instead",
                copy,
            )
            .step(E::Start { items: 1 }, [probe(0)], S::Probing),
            0,
        )
        .step(placed(0, Placed::Ok), [F::Trash { item: i(0), aside: A }], S::Disposing)
        .step(
            E::Disposed { item: i(0), ok: false },
            [F::Problem { item: i(0), problem: Problem::KeptAside(A) }, outcome(0, Outcome::Done), DONE],
            S::Finished,
        ),
        replace_until_placed(
            Trace::new(
                "failed-placement-restores-the-aside",
                "the replacement failed: the old item is renamed back",
                copy,
            )
            .step(E::Start { items: 2 }, [probe(0)], S::Probing),
            0,
        )
        .step(placed(0, Placed::Skip), [F::RestoreAside { item: i(0), aside: A }], S::Restoring)
        .step(E::Restored { item: i(0), ok: true }, [outcome(0, Outcome::Failed), probe(1)], S::Probing),
        replace_until_placed(
            Trace::new("stranded-aside-is-reported", "renaming back failed too: say where the old item is", copy).step(
                E::Start { items: 1 },
                [probe(0)],
                S::Probing,
            ),
            0,
        )
        .step(placed(0, Placed::Skip), [F::RestoreAside { item: i(0), aside: A }], S::Restoring)
        .step(
            E::Restored { item: i(0), ok: false },
            [F::Problem { item: i(0), problem: Problem::Stranded(A) }, outcome(0, Outcome::Failed), DONE],
            S::Finished,
        ),
        replace_until_placed(
            Trace::new(
                "cancel-while-replacing-restores-first",
                "Stop during the replacement: rename the old item back, then end",
                copy,
            )
            .step(E::Start { items: 2 }, [probe(0)], S::Probing),
            0,
        )
        .step(placed(0, Placed::Cancel), [F::RestoreAside { item: i(0), aside: A }], S::Restoring)
        .step(E::Restored { item: i(0), ok: true }, [CANCELLED], S::Finished)
        .step(E::Start { items: 1 }, [], S::Finished),
        Trace::new(
            "set-aside-failure-leaves-the-target",
            "the old item couldn't be moved aside: nothing replaced",
            copy,
        )
        .step(E::Start { items: 1 }, [probe(0)], S::Probing)
        .step(probed(0, CONFLICT), [F::Ask { item: i(0), can_merge: false }], S::Asking)
        .step(answered(0, R::Replace), [F::SetAside { item: i(0) }], S::SettingAside)
        .step(E::SetAside { item: i(0), aside: None }, [outcome(0, Outcome::Failed), DONE], S::Finished),
        Trace::new("cancel-at-the-prompt", "Stop at the conflict sheet ends the job; later events do nothing", copy)
            .step(E::Start { items: 3 }, [probe(0)], S::Probing)
            .step(probed(0, CONFLICT), [F::Ask { item: i(0), can_merge: false }], S::Asking)
            .step(answered(0, R::Cancel), [CANCELLED], S::Finished)
            .step(probed(1, Found::Free), [], S::Finished)
            .step(placed(0, Placed::Ok), [], S::Finished),
        Trace::new("cancel-while-placing", "Stop during a plain placement ends the job", move_)
            .step(E::Start { items: 2 }, [probe(0)], S::Probing)
            .step(probed(0, Found::Free), [place(0)], S::Placing)
            .step(placed(0, Placed::Cancel), [CANCELLED], S::Finished),
        Trace::new(
            "merge-recurses-into-folders",
            "each child is placed and asked about on its own; a skip keeps the source folder",
            move_,
        )
        .step(E::Start { items: 1 }, [probe(0)], S::Probing)
        .step(probed(0, FOLDERS), [F::Ask { item: i(0), can_merge: true }], S::Asking)
        .step(answered(0, R::Merge), [F::List { item: i(0) }], S::Listing)
        .step(E::Listed { item: i(0), first: i(10), count: 2 }, [probe(10)], S::Probing)
        .step(probed(10, Found::Free), [place(10)], S::Placing)
        .step(placed(10, Placed::Ok), [outcome(10, Outcome::Done), probe(11)], S::Probing)
        .step(probed(11, CONFLICT), [F::Ask { item: i(11), can_merge: false }], S::Asking)
        .step(
            answered(11, R::Skip),
            [outcome(11, Outcome::Skipped), outcome(0, Outcome::Done), DONE],
            S::Finished,
        ),
        Trace::new(
            "merge-complete-move-removes-the-emptied-source",
            "a moved folder whose children all arrived is removed; nested merges too",
            move_,
        )
        .step(E::Start { items: 2 }, [probe(0)], S::Probing)
        .step(probed(0, FOLDERS), [F::Ask { item: i(0), can_merge: true }], S::Asking)
        .step(answered(0, R::Merge), [F::List { item: i(0) }], S::Listing)
        .step(E::Listed { item: i(0), first: i(10), count: 2 }, [probe(10)], S::Probing)
        .step(probed(10, FOLDERS), [F::Ask { item: i(10), can_merge: true }], S::Asking)
        .step(answered(10, R::Merge), [F::List { item: i(10) }], S::Listing)
        .step(
            E::Listed { item: i(10), first: i(20), count: 0 },
            [F::RemoveEmptySource { item: i(10) }, outcome(10, Outcome::Done), probe(11)],
            S::Probing,
        )
        .step(
            probed(11, Found::Same),
            [outcome(11, Outcome::Done), F::RemoveEmptySource { item: i(0) }, outcome(0, Outcome::Done), probe(1)],
            S::Probing,
        )
        .step(probed(1, Found::Refused), [outcome(1, Outcome::Failed), DONE], S::Finished),
        Trace::new("merge-in-a-copy-keeps-the-source", "a copy never removes a source folder", copy)
            .step(E::Start { items: 1 }, [probe(0)], S::Probing)
            .step(probed(0, FOLDERS), [F::Ask { item: i(0), can_merge: true }], S::Asking)
            .step(answered(0, R::Merge), [F::List { item: i(0) }], S::Listing)
            .step(E::Listed { item: i(0), first: i(10), count: 1 }, [probe(10)], S::Probing)
            .step(probed(10, Found::Free), [place(10)], S::Placing)
            .step(placed(10, Placed::Ok), [outcome(10, Outcome::Done), outcome(0, Outcome::Done), DONE], S::Finished),
        Trace::new("cancel-inside-a-merge-ends-everything", "Stop at a child's prompt ends the whole job", copy)
            .step(E::Start { items: 2 }, [probe(0)], S::Probing)
            .step(probed(0, FOLDERS), [F::Ask { item: i(0), can_merge: true }], S::Asking)
            .step(answered(0, R::Merge), [F::List { item: i(0) }], S::Listing)
            .step(E::Listed { item: i(0), first: i(10), count: 3 }, [probe(10)], S::Probing)
            .step(probed(10, CONFLICT), [F::Ask { item: i(10), can_merge: false }], S::Asking)
            .step(answered(10, R::Cancel), [CANCELLED], S::Finished),
        Trace::new("merge-of-non-folders-replaces", "Merge when they aren't both folders means Replace", copy)
            .step(E::Start { items: 1 }, [probe(0)], S::Probing)
            .step(probed(0, CONFLICT), [F::Ask { item: i(0), can_merge: false }], S::Asking)
            .step(answered(0, R::Merge), [F::SetAside { item: i(0) }], S::SettingAside),
        Trace::new(
            "responses-for-other-items-are-ignored",
            "only the awaited response for the current item counts",
            copy,
        )
        .step(probed(0, Found::Free), [], S::Idle)
        .step(E::Start { items: 2 }, [probe(0)], S::Probing)
        .step(E::Start { items: 2 }, [], S::Probing)
        .step(probed(1, Found::Free), [], S::Probing)
        .step(answered(0, R::Skip), [], S::Probing)
        .step(placed(0, Placed::Ok), [], S::Probing)
        .step(E::Listed { item: i(0), first: i(5), count: 1 }, [], S::Probing)
        .step(probed(0, Found::Free), [place(0)], S::Placing)
        .step(E::Disposed { item: i(0), ok: true }, [], S::Placing)
        .step(E::Restored { item: i(0), ok: true }, [], S::Placing)
        .step(E::SetAside { item: i(0), aside: Some(A) }, [], S::Placing)
        .step(placed(1, Placed::Ok), [], S::Placing)
        .step(placed(0, Placed::Ok), [outcome(0, Outcome::Done), probe(1)], S::Probing),
        // Confirmed bug HAKO-05, as the correct behaviour.
        replace_until_placed(
            Trace::new(
                "put-back-replace-goes-to-the-trash",
                "Put Back (or Undo of Move to Trash) and Replace: the newer item goes to the Trash, as the sheet says",
                put_back,
            )
            .bug("HAKO-05")
            .step(E::Start { items: 1 }, [probe(0)], S::Probing),
            0,
        )
        .step(placed(0, Placed::Ok), [F::Trash { item: i(0), aside: A }], S::Disposing)
        .step(E::Disposed { item: i(0), ok: true }, [outcome(0, Outcome::Done), DONE], S::Finished),
        Trace::new(
            "put-back-merge-replaces-into-the-trash",
            "a folder put back over a folder: a file replaced inside the merge goes to the Trash too",
            put_back,
        )
        .bug("HAKO-05")
        .step(E::Start { items: 1 }, [probe(0)], S::Probing)
        .step(probed(0, FOLDERS), [F::Ask { item: i(0), can_merge: true }], S::Asking)
        .step(answered(0, R::Merge), [F::List { item: i(0) }], S::Listing)
        .step(E::Listed { item: i(0), first: i(30), count: 1 }, [probe(30)], S::Probing)
        .step(probed(30, CONFLICT), [F::Ask { item: i(30), can_merge: false }], S::Asking)
        .step(answered(30, R::Replace), [F::SetAside { item: i(30) }], S::SettingAside)
        .step(E::SetAside { item: i(30), aside: Some(AsideId(9)) }, [place(30)], S::PlacingReplacement)
        .step(placed(30, Placed::Ok), [F::Trash { item: i(30), aside: AsideId(9) }], S::Disposing)
        .step(
            E::Disposed { item: i(30), ok: true },
            [outcome(30, Outcome::Done), F::RemoveEmptySource { item: i(0) }, outcome(0, Outcome::Done), DONE],
            S::Finished,
        ),
    ]
}
