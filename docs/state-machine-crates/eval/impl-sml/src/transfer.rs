//! `transfer` with stateforward-sml 2.0.0: a flat table whose states carry
//! the awaited item (and the aside) as payloads; guards check that a
//! response names that item. "item done" goes to `Next`, a choice state
//! whose anonymous rows take the next item or finish the job. The merge
//! frames are a stack in the context: a table can't nest itself, so a
//! folder's sub-machine isn't a child table. Effects go through the
//! per-call temporary context, a `dyn Sink`.

use spec::Sink;
use spec::transfer::{
    AsideId, Config, Effect, Event, Found, ItemId, Machine, Mode, Name, Outcome, Placed, Problem, Resolution, StateName,
};

pub const IMPLEMENTED: bool = true;

pub type Out<'a> = &'a mut dyn Sink<Effect>;

/// What the awaited response reports, for one item.
#[derive(Clone, Copy)]
pub struct Reply<T> {
    pub item: ItemId,
    pub value: T,
}

#[derive(Clone, Copy)]
pub struct Listed {
    pub item: ItemId,
    pub first: ItemId,
    pub count: u32,
}

/// `Asking`'s payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ask {
    pub item: ItemId,
    pub can_merge: bool,
}

/// The replaced item's aside, from `PlacingReplacement` to `Disposing` or `Restoring`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aside {
    pub item: ItemId,
    pub aside: AsideId,
}

/// `Restoring`'s payload: the aside, and whether the job goes on or was cancelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Restore {
    pub aside: Aside,
    pub cancel: bool,
}

sml::sml! {
    Transfer[temporary_context: &mut dyn Sink<Effect>] {
        *Idle + Start(u32) / push_top = Next,
         Next [more] / next_item = Probing(ItemId),
         Next / finish = Finished,
         Probing(ItemId) + Probed(Reply<Found>) [free] / place_target = Placing(ItemId),
         Probing(ItemId) + Probed(Reply<Found>) [conflict] / ask = Asking(Ask),
         Probing(ItemId) + Probed(Reply<Found>) [for_item] / probed_done = Next,
         Asking(Ask) + Answered(Reply<Resolution>) [cancels] / cancel_answered = Finished,
         Asking(Ask) + Answered(Reply<Resolution>) [keep_both] / place_free = Placing(ItemId),
         Asking(Ask) + Answered(Reply<Resolution>) [merge] / list = Listing(ItemId),
         Asking(Ask) + Answered(Reply<Resolution>) [replace] / set_aside = SettingAside(ItemId),
         Asking(Ask) + Answered(Reply<Resolution>) [answers] / skipped = Next,
         SettingAside(ItemId) + SetAside(Reply<Option<AsideId>>) [got_aside] / place_replacement = PlacingReplacement(Aside),
         SettingAside(ItemId) + SetAside(Reply<Option<AsideId>>) [aside_reported] / aside_failed = Next,
         Placing(ItemId) + Placed(Reply<Placed>) [placed_cancel] / cancel_placed = Finished,
         Placing(ItemId) + Placed(Reply<Placed>) [placed_for_item] / placed = Next,
         PlacingReplacement(Aside) + Placed(Reply<Placed>) [replaced] / dispose = Disposing(Aside),
         PlacingReplacement(Aside) + Placed(Reply<Placed>) [for_aside] / restore = Restoring(Restore),
         Disposing(Aside) + Disposed(Reply<bool>) [disposal_reported] / disposed = Next,
         Restoring(Restore) + Restored(Reply<bool>) [restored_then_cancel] / restored_cancel = Finished,
         Restoring(Restore) + Restored(Reply<bool>) [restored] / restored_failed = Next,
         Listing(ItemId) + Listed(Listed) [listed] / push_folder = Next,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    folder: Option<ItemId>,
    next: u32,
    end: u32,
    complete: bool,
}

/// The context: the job's choices and the merge frames.
pub struct Job {
    config: Config,
    frames: Vec<Frame>,
}

type R = Result<(), ()>;
type G = Result<bool, ()>;

impl Job {
    fn done(&mut self, out: Out<'_>, item: ItemId, outcome: Outcome) {
        out.push(Effect::Outcome { item, outcome });
        if outcome != Outcome::Done
            && let Some(f) = self.frames.last_mut()
        {
            f.complete = false;
        }
    }

    /// Pops the finished frames: a merge frame reports its folder.
    fn pop_finished(&mut self, out: Out<'_>) {
        while let Some(f) = self.frames.last()
            && f.next >= f.end
        {
            if let Some(folder) = f.folder {
                if self.config.mode == Mode::Move && f.complete {
                    out.push(Effect::RemoveEmptySource { item: folder });
                }
                out.push(Effect::Outcome { item: folder, outcome: Outcome::Done });
            }
            self.frames.pop();
        }
    }

    fn cancelled(&mut self, out: Out<'_>) {
        self.frames.clear();
        out.push(Effect::Finished { cancelled: true });
    }
}

impl TransferStateMachineContext for Job {
    fn push_top(&mut self, _: Out<'_>, items: u32) -> R {
        self.frames.push(Frame { folder: None, next: 0, end: items, complete: true });
        Ok(())
    }

    fn more(&self, _: Out<'_>) -> G {
        Ok(self.frames.iter().any(|f| f.next < f.end))
    }

    fn next_item(&mut self, out: Out<'_>) -> Result<ItemId, ()> {
        self.pop_finished(out);
        let f = self.frames.last_mut().ok_or(())?;
        let item = ItemId(f.next);
        f.next += 1;
        out.push(Effect::Probe { item });
        Ok(item)
    }

    fn finish(&mut self, out: Out<'_>) -> R {
        self.pop_finished(out);
        out.push(Effect::Finished { cancelled: false });
        Ok(())
    }

    fn for_item(&self, _: Out<'_>, item: &ItemId, e: &Reply<Found>) -> G {
        Ok(e.item == *item)
    }

    fn free(&self, _: Out<'_>, item: &ItemId, e: &Reply<Found>) -> G {
        Ok(e.item == *item && e.value == Found::Free)
    }

    fn place_target(&mut self, out: Out<'_>, item: &ItemId, _: Reply<Found>) -> Result<ItemId, ()> {
        out.push(Effect::PlaceAt { item: *item, name: Name::Target });
        Ok(*item)
    }

    fn conflict(&self, _: Out<'_>, item: &ItemId, e: &Reply<Found>) -> G {
        Ok(e.item == *item && matches!(e.value, Found::Conflict { .. }))
    }

    fn ask(&mut self, out: Out<'_>, item: &ItemId, e: Reply<Found>) -> Result<Ask, ()> {
        let Found::Conflict { can_merge } = e.value else { return Err(()) };
        out.push(Effect::Ask { item: *item, can_merge });
        Ok(Ask { item: *item, can_merge })
    }

    /// `Same` or `Refused`.
    fn probed_done(&mut self, out: Out<'_>, item: &ItemId, e: Reply<Found>) -> R {
        self.done(out, *item, if e.value == Found::Same { Outcome::Done } else { Outcome::Failed });
        Ok(())
    }

    fn answers(&self, _: Out<'_>, a: &Ask, e: &Reply<Resolution>) -> G {
        Ok(e.item == a.item)
    }

    fn cancels(&self, _: Out<'_>, a: &Ask, e: &Reply<Resolution>) -> G {
        Ok(e.item == a.item && e.value == Resolution::Cancel)
    }

    fn cancel_answered(&mut self, out: Out<'_>, _: &Ask, _: Reply<Resolution>) -> R {
        self.cancelled(out);
        Ok(())
    }

    fn keep_both(&self, _: Out<'_>, a: &Ask, e: &Reply<Resolution>) -> G {
        Ok(e.item == a.item && e.value == Resolution::KeepBoth)
    }

    fn merge(&self, _: Out<'_>, a: &Ask, e: &Reply<Resolution>) -> G {
        Ok(e.item == a.item && e.value == Resolution::Merge && a.can_merge)
    }

    /// `Replace`, or `Merge` where the conflict can't merge.
    fn replace(&self, _: Out<'_>, a: &Ask, e: &Reply<Resolution>) -> G {
        Ok(e.item == a.item && matches!(e.value, Resolution::Replace | Resolution::Merge))
    }

    fn place_free(&mut self, out: Out<'_>, a: &Ask, _: Reply<Resolution>) -> Result<ItemId, ()> {
        out.push(Effect::PlaceAt { item: a.item, name: Name::Free });
        Ok(a.item)
    }

    fn list(&mut self, out: Out<'_>, a: &Ask, _: Reply<Resolution>) -> Result<ItemId, ()> {
        out.push(Effect::List { item: a.item });
        Ok(a.item)
    }

    fn set_aside(&mut self, out: Out<'_>, a: &Ask, _: Reply<Resolution>) -> Result<ItemId, ()> {
        out.push(Effect::SetAside { item: a.item });
        Ok(a.item)
    }

    /// `Skip`.
    fn skipped(&mut self, out: Out<'_>, a: &Ask, _: Reply<Resolution>) -> R {
        self.done(out, a.item, Outcome::Skipped);
        Ok(())
    }

    fn got_aside(&self, _: Out<'_>, item: &ItemId, e: &Reply<Option<AsideId>>) -> G {
        Ok(e.item == *item && e.value.is_some())
    }

    fn place_replacement(&mut self, out: Out<'_>, item: &ItemId, e: Reply<Option<AsideId>>) -> Result<Aside, ()> {
        out.push(Effect::PlaceAt { item: *item, name: Name::Target });
        Ok(Aside { item: *item, aside: e.value.ok_or(())? })
    }

    fn aside_reported(&self, _: Out<'_>, item: &ItemId, e: &Reply<Option<AsideId>>) -> G {
        Ok(e.item == *item)
    }

    fn aside_failed(&mut self, out: Out<'_>, item: &ItemId, _: Reply<Option<AsideId>>) -> R {
        self.done(out, *item, Outcome::Failed);
        Ok(())
    }

    fn placed_cancel(&self, _: Out<'_>, item: &ItemId, e: &Reply<Placed>) -> G {
        Ok(e.item == *item && e.value == Placed::Cancel)
    }

    fn cancel_placed(&mut self, out: Out<'_>, _: &ItemId, _: Reply<Placed>) -> R {
        self.cancelled(out);
        Ok(())
    }

    fn placed_for_item(&self, _: Out<'_>, item: &ItemId, e: &Reply<Placed>) -> G {
        Ok(e.item == *item)
    }

    fn placed(&mut self, out: Out<'_>, item: &ItemId, e: Reply<Placed>) -> R {
        self.done(out, *item, if e.value == Placed::Ok { Outcome::Done } else { Outcome::Failed });
        Ok(())
    }

    fn replaced(&self, _: Out<'_>, a: &Aside, e: &Reply<Placed>) -> G {
        Ok(e.item == a.item && e.value == Placed::Ok)
    }

    /// Never decided by the job's kind.
    fn dispose(&mut self, out: Out<'_>, a: &Aside, _: Reply<Placed>) -> Result<Aside, ()> {
        let Aside { item, aside } = *a;
        out.push(if self.config.delete_replaced {
            Effect::Delete { item, aside }
        } else {
            Effect::Trash { item, aside }
        });
        Ok(*a)
    }

    fn for_aside(&self, _: Out<'_>, a: &Aside, e: &Reply<Placed>) -> G {
        Ok(e.item == a.item)
    }

    /// `Skip` or `Cancel`: put the original back.
    fn restore(&mut self, out: Out<'_>, a: &Aside, e: Reply<Placed>) -> Result<Restore, ()> {
        out.push(Effect::RestoreAside { item: a.item, aside: a.aside });
        Ok(Restore { aside: *a, cancel: e.value == Placed::Cancel })
    }

    fn disposal_reported(&self, _: Out<'_>, a: &Aside, e: &Reply<bool>) -> G {
        Ok(e.item == a.item)
    }

    fn disposed(&mut self, out: Out<'_>, a: &Aside, e: Reply<bool>) -> R {
        if !e.value {
            let problem =
                if self.config.delete_replaced { Problem::NotAllRemoved } else { Problem::KeptAside(a.aside) };
            out.push(Effect::Problem { item: a.item, problem });
        }
        self.done(out, a.item, Outcome::Done);
        Ok(())
    }

    fn restored(&self, _: Out<'_>, r: &Restore, e: &Reply<bool>) -> G {
        Ok(e.item == r.aside.item)
    }

    fn restored_then_cancel(&self, _: Out<'_>, r: &Restore, e: &Reply<bool>) -> G {
        Ok(e.item == r.aside.item && r.cancel)
    }

    fn restored_cancel(&mut self, out: Out<'_>, r: &Restore, e: Reply<bool>) -> R {
        if !e.value {
            out.push(Effect::Problem { item: r.aside.item, problem: Problem::Stranded(r.aside.aside) });
        }
        self.cancelled(out);
        Ok(())
    }

    fn restored_failed(&mut self, out: Out<'_>, r: &Restore, e: Reply<bool>) -> R {
        if !e.value {
            out.push(Effect::Problem { item: r.aside.item, problem: Problem::Stranded(r.aside.aside) });
        }
        self.done(out, r.aside.item, Outcome::Failed);
        Ok(())
    }

    fn listed(&self, _: Out<'_>, item: &ItemId, e: &Listed) -> G {
        Ok(e.item == *item)
    }

    fn push_folder(&mut self, _: Out<'_>, item: &ItemId, e: Listed) -> R {
        let end = e.first.0.saturating_add(e.count);
        self.frames.push(Frame { folder: Some(*item), next: e.first.0, end, complete: true });
        Ok(())
    }
}

pub struct Transfer {
    /// The generated machine: its `process_event`, `is` and
    /// `visit_current_states` are the typed API.
    pub machine: TransferStateMachine<Job>,
}

impl Machine for Transfer {
    fn new(config: Config) -> Self {
        Self { machine: TransferStateMachine::new(Job { config, frames: Vec::new() }) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        use TransferEvents as E;
        let event = match event {
            Event::Start { items } => E::Start(items),
            Event::Probed { item, found } => E::Probed(Reply { item, value: found }),
            Event::Answered { item, resolution } => E::Answered(Reply { item, value: resolution }),
            Event::SetAside { item, aside } => E::SetAside(Reply { item, value: aside }),
            Event::Placed { item, outcome } => E::Placed(Reply { item, value: outcome }),
            Event::Disposed { item, ok } => E::Disposed(Reply { item, value: ok }),
            Event::Restored { item, ok } => E::Restored(Reply { item, value: ok }),
            Event::Listed { item, first, count } => E::Listed(Listed { item, first, count }),
        };
        // An event no row takes, or whose guards all fail, is an `Err`:
        // ignored, as the spec says.
        let _ = self.machine.process_event(out, event);
    }

    fn state(&self) -> StateName {
        let mut name = StateName::Idle;
        self.machine.visit_current_states(|s| {
            name = match s {
                TransferStates::Idle => StateName::Idle,
                TransferStates::Next => unreachable!("`Next` is left by its anonymous rows"),
                TransferStates::Probing(_) => StateName::Probing,
                TransferStates::Asking(_) => StateName::Asking,
                TransferStates::Placing(_) => StateName::Placing,
                TransferStates::SettingAside(_) => StateName::SettingAside,
                TransferStates::PlacingReplacement(_) => StateName::PlacingReplacement,
                TransferStates::Disposing(_) => StateName::Disposing,
                TransferStates::Restoring(_) => StateName::Restoring,
                TransferStates::Listing(_) => StateName::Listing,
                TransferStates::Finished => StateName::Finished,
            }
        });
        name
    }
}
