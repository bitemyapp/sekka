//! `transfer` with smlang 0.8.0: one state per awaited response, carrying
//! the item (and the aside once there is one); the merge frames are a stack
//! in the context (smlang has no nested machines). A row's target is fixed,
//! so **next** (which ends in `Probing` or, when every frame is used up, in
//! `Finished`) is two rows each time: one guarded by a `more_*` guard that
//! predicts the action's outcome, one to `Finished`. Guards and actions are
//! keyed by (state data, event data) signature, so the "is this response for
//! my item" check is one guard per state and event.

use smlang::statemachine;
use spec::Sink;
use spec::transfer::{
    AsideId, Config, Effect, Event, Found, ItemId, Machine, Mode, Name, Outcome, Placed, Problem, Resolution, StateName,
};

pub const IMPLEMENTED: bool = true;

/// `Asking`'s data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Asked {
    pub item: ItemId,
    pub can_merge: bool,
}

/// `PlacingReplacement` and `Disposing`'s data: the item and its aside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aside {
    pub item: ItemId,
    pub aside: AsideId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    Failed,
    Cancel,
}

/// `Restoring`'s data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Restore {
    pub item: ItemId,
    pub aside: AsideId,
    pub then: After,
}

pub type Pd = (ItemId, Found);
pub type An = (ItemId, Resolution);
pub type Sa = (ItemId, Option<AsideId>);
pub type Pl = (ItemId, Placed);
pub type Ok_ = (ItemId, bool);
/// `Listed`: item, first, count.
pub type Ls = (ItemId, ItemId, u32);

/// The generated machine, kept private: its `new_with_state` and
/// `context_mut` would let callers forge a state.
mod fsm {
    use super::*;

    statemachine! {
        temporary_context: &mut impl Sink<Effect>,
        derive_states: [Clone, Copy, Debug],
        derive_events: [Clone, Copy, Debug],
        transitions: {
            *Idle + Start(u32) [has_items] / start = Probing(ItemId),
            Idle + Start(u32) / start_empty = Finished,
            Probing(ItemId) + Probed(Pd) [probed && free] / place = Placing(ItemId),
            Probing(ItemId) + Probed(Pd) [probed && conflict] / ask = Asking(Asked),
            Probing(ItemId) + Probed(Pd) [probed && more_probed] / probed_next = Probing(ItemId),
            Probing(ItemId) + Probed(Pd) [probed] / probed_last = Finished,
            Asking(Asked) + Answered(An) [answered && cancels] / cancel_asking = Finished,
            Asking(Asked) + Answered(An) [answered && keeps_both] / keep_both = Placing(ItemId),
            Asking(Asked) + Answered(An) [answered && merges] / list = Listing(ItemId),
            Asking(Asked) + Answered(An) [answered && replaces] / set_aside = SettingAside(ItemId),
            Asking(Asked) + Answered(An) [answered && more_answered] / skip_next = Probing(ItemId),
            Asking(Asked) + Answered(An) [answered] / skip_last = Finished,
            SettingAside(ItemId) + SetAside(Sa) [set_aside_for && got_aside] / place_replacement = PlacingReplacement(Aside),
            SettingAside(ItemId) + SetAside(Sa) [set_aside_for && more_set_aside] / no_aside_next = Probing(ItemId),
            SettingAside(ItemId) + SetAside(Sa) [set_aside_for] / no_aside_last = Finished,
            Placing(ItemId) + Placed(Pl) [placed && placed_cancel] / cancel_placing = Finished,
            Placing(ItemId) + Placed(Pl) [placed && more_placed] / placed_next = Probing(ItemId),
            Placing(ItemId) + Placed(Pl) [placed] / placed_last = Finished,
            PlacingReplacement(Aside) + Placed(Pl) [replaced && replaced_ok] / dispose = Disposing(Aside),
            PlacingReplacement(Aside) + Placed(Pl) [replaced] / restore_aside = Restoring(Restore),
            Disposing(Aside) + Disposed(Ok_) [disposed && more_disposed] / disposed_next = Probing(ItemId),
            Disposing(Aside) + Disposed(Ok_) [disposed] / disposed_last = Finished,
            Restoring(Restore) + Restored(Ok_) [restored && restore_cancels] / restored_cancel = Finished,
            Restoring(Restore) + Restored(Ok_) [restored && more_restored] / restored_next = Probing(ItemId),
            Restoring(Restore) + Restored(Ok_) [restored] / restored_last = Finished,
            Listing(ItemId) + Listed(Ls) [listed && more_listed] / listed_next = Probing(ItemId),
            Listing(ItemId) + Listed(Ls) [listed] / listed_last = Finished,
        }
    }
}

pub use fsm::{Error, Events, States};
use fsm::{StateMachine, StateMachineContext};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub folder: Option<ItemId>,
    pub next: u32,
    pub end: u32,
    pub complete: bool,
}

/// The context: the job's configuration and the frame stack.
pub struct Job {
    config: Config,
    frames: Vec<Frame>,
}

impl Job {
    fn new(config: Config) -> Self {
        Self { config, frames: Vec::new() }
    }

    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    /// Whether **next** will probe another item (the `more_*` guards).
    fn more(&self) -> bool {
        self.frames.iter().any(|f| f.next < f.end)
    }

    /// **next**: the item it probes, or `None` once it emitted `Finished`.
    fn next(&mut self, out: &mut impl Sink<Effect>) -> Option<ItemId> {
        while let Some(f) = self.frames.last_mut() {
            if f.next < f.end {
                let item = ItemId(f.next);
                f.next += 1;
                out.push(Effect::Probe { item });
                return Some(item);
            }
            let f = self.frames.pop().expect("a frame");
            if let Some(folder) = f.folder {
                if self.config.mode == Mode::Move && f.complete {
                    out.push(Effect::RemoveEmptySource { item: folder });
                }
                out.push(Effect::Outcome { item: folder, outcome: Outcome::Done });
            }
        }
        out.push(Effect::Finished { cancelled: false });
        None
    }

    /// **item done**.
    fn done(&mut self, item: ItemId, outcome: Outcome, out: &mut impl Sink<Effect>) -> Option<ItemId> {
        out.push(Effect::Outcome { item, outcome });
        if outcome != Outcome::Done
            && let Some(f) = self.frames.last_mut()
        {
            f.complete = false;
        }
        self.next(out)
    }

    fn cancel(&mut self, out: &mut impl Sink<Effect>) -> R {
        self.frames.clear();
        out.push(Effect::Finished { cancelled: true });
        Ok(())
    }

    fn push(&mut self, folder: Option<ItemId>, first: u32, count: u32) {
        self.frames.push(Frame { folder, next: first, end: first.saturating_add(count), complete: true });
    }
}

fn probed_outcome(found: Found) -> Outcome {
    if found == Found::Same { Outcome::Done } else { Outcome::Failed }
}

fn placed_outcome(p: Placed) -> Outcome {
    if p == Placed::Ok { Outcome::Done } else { Outcome::Failed }
}

fn disposal_problem(job: &Job, a: &Aside, ok: bool, out: &mut impl Sink<Effect>) {
    if !ok {
        let problem = if job.config.delete_replaced { Problem::NotAllRemoved } else { Problem::KeptAside(a.aside) };
        out.push(Effect::Problem { item: a.item, problem });
    }
}

fn restore_problem(r: &Restore, ok: bool, out: &mut impl Sink<Effect>) {
    if !ok {
        out.push(Effect::Problem { item: r.item, problem: Problem::Stranded(r.aside) });
    }
}

type R<T = ()> = Result<T, ()>;

impl StateMachineContext for Job {
    fn has_items(&self, _: &mut impl Sink<Effect>, items: &u32) -> R<bool> {
        Ok(*items > 0)
    }
    fn probed(&self, _: &mut impl Sink<Effect>, item: &ItemId, e: &Pd) -> R<bool> {
        Ok(e.0 == *item)
    }
    fn free(&self, _: &mut impl Sink<Effect>, _: &ItemId, e: &Pd) -> R<bool> {
        Ok(e.1 == Found::Free)
    }
    fn conflict(&self, _: &mut impl Sink<Effect>, _: &ItemId, e: &Pd) -> R<bool> {
        Ok(matches!(e.1, Found::Conflict { .. }))
    }
    fn more_probed(&self, _: &mut impl Sink<Effect>, _: &ItemId, _: &Pd) -> R<bool> {
        Ok(self.more())
    }
    fn answered(&self, _: &mut impl Sink<Effect>, a: &Asked, e: &An) -> R<bool> {
        Ok(e.0 == a.item)
    }
    fn cancels(&self, _: &mut impl Sink<Effect>, _: &Asked, e: &An) -> R<bool> {
        Ok(e.1 == Resolution::Cancel)
    }
    fn keeps_both(&self, _: &mut impl Sink<Effect>, _: &Asked, e: &An) -> R<bool> {
        Ok(e.1 == Resolution::KeepBoth)
    }
    fn merges(&self, _: &mut impl Sink<Effect>, a: &Asked, e: &An) -> R<bool> {
        Ok(e.1 == Resolution::Merge && a.can_merge)
    }
    fn replaces(&self, _: &mut impl Sink<Effect>, _: &Asked, e: &An) -> R<bool> {
        Ok(matches!(e.1, Resolution::Merge | Resolution::Replace))
    }
    fn more_answered(&self, _: &mut impl Sink<Effect>, _: &Asked, _: &An) -> R<bool> {
        Ok(self.more())
    }
    fn set_aside_for(&self, _: &mut impl Sink<Effect>, item: &ItemId, e: &Sa) -> R<bool> {
        Ok(e.0 == *item)
    }
    fn got_aside(&self, _: &mut impl Sink<Effect>, _: &ItemId, e: &Sa) -> R<bool> {
        Ok(e.1.is_some())
    }
    fn more_set_aside(&self, _: &mut impl Sink<Effect>, _: &ItemId, _: &Sa) -> R<bool> {
        Ok(self.more())
    }
    fn placed(&self, _: &mut impl Sink<Effect>, item: &ItemId, e: &Pl) -> R<bool> {
        Ok(e.0 == *item)
    }
    fn placed_cancel(&self, _: &mut impl Sink<Effect>, _: &ItemId, e: &Pl) -> R<bool> {
        Ok(e.1 == Placed::Cancel)
    }
    fn more_placed(&self, _: &mut impl Sink<Effect>, _: &ItemId, _: &Pl) -> R<bool> {
        Ok(self.more())
    }
    fn replaced(&self, _: &mut impl Sink<Effect>, a: &Aside, e: &Pl) -> R<bool> {
        Ok(e.0 == a.item)
    }
    fn replaced_ok(&self, _: &mut impl Sink<Effect>, _: &Aside, e: &Pl) -> R<bool> {
        Ok(e.1 == Placed::Ok)
    }
    fn disposed(&self, _: &mut impl Sink<Effect>, a: &Aside, e: &Ok_) -> R<bool> {
        Ok(e.0 == a.item)
    }
    fn more_disposed(&self, _: &mut impl Sink<Effect>, _: &Aside, _: &Ok_) -> R<bool> {
        Ok(self.more())
    }
    fn restored(&self, _: &mut impl Sink<Effect>, r: &Restore, e: &Ok_) -> R<bool> {
        Ok(e.0 == r.item)
    }
    fn restore_cancels(&self, _: &mut impl Sink<Effect>, r: &Restore, _: &Ok_) -> R<bool> {
        Ok(r.then == After::Cancel)
    }
    fn more_restored(&self, _: &mut impl Sink<Effect>, _: &Restore, _: &Ok_) -> R<bool> {
        Ok(self.more())
    }
    fn listed(&self, _: &mut impl Sink<Effect>, item: &ItemId, e: &Ls) -> R<bool> {
        Ok(e.0 == *item)
    }
    fn more_listed(&self, _: &mut impl Sink<Effect>, _: &ItemId, e: &Ls) -> R<bool> {
        // The new frame isn't pushed yet.
        Ok(e.2 > 0 || self.more())
    }

    fn start(&mut self, out: &mut impl Sink<Effect>, items: u32) -> R<ItemId> {
        self.push(None, 0, items);
        self.next(out).ok_or(())
    }
    fn start_empty(&mut self, out: &mut impl Sink<Effect>, items: u32) -> R {
        self.push(None, 0, items);
        self.next(out);
        Ok(())
    }
    fn place(&mut self, out: &mut impl Sink<Effect>, item: &ItemId, _: Pd) -> R<ItemId> {
        out.push(Effect::PlaceAt { item: *item, name: Name::Target });
        Ok(*item)
    }
    fn ask(&mut self, out: &mut impl Sink<Effect>, item: &ItemId, (_, found): Pd) -> R<Asked> {
        let can_merge = matches!(found, Found::Conflict { can_merge: true });
        out.push(Effect::Ask { item: *item, can_merge });
        Ok(Asked { item: *item, can_merge })
    }
    fn probed_next(&mut self, out: &mut impl Sink<Effect>, item: &ItemId, (_, found): Pd) -> R<ItemId> {
        self.done(*item, probed_outcome(found), out).ok_or(())
    }
    fn probed_last(&mut self, out: &mut impl Sink<Effect>, item: &ItemId, (_, found): Pd) -> R {
        self.done(*item, probed_outcome(found), out);
        Ok(())
    }
    fn cancel_asking(&mut self, out: &mut impl Sink<Effect>, _: &Asked, _: An) -> R {
        self.cancel(out)
    }
    fn keep_both(&mut self, out: &mut impl Sink<Effect>, a: &Asked, _: An) -> R<ItemId> {
        out.push(Effect::PlaceAt { item: a.item, name: Name::Free });
        Ok(a.item)
    }
    fn list(&mut self, out: &mut impl Sink<Effect>, a: &Asked, _: An) -> R<ItemId> {
        out.push(Effect::List { item: a.item });
        Ok(a.item)
    }
    fn set_aside(&mut self, out: &mut impl Sink<Effect>, a: &Asked, _: An) -> R<ItemId> {
        out.push(Effect::SetAside { item: a.item });
        Ok(a.item)
    }
    fn skip_next(&mut self, out: &mut impl Sink<Effect>, a: &Asked, _: An) -> R<ItemId> {
        self.done(a.item, Outcome::Skipped, out).ok_or(())
    }
    fn skip_last(&mut self, out: &mut impl Sink<Effect>, a: &Asked, _: An) -> R {
        self.done(a.item, Outcome::Skipped, out);
        Ok(())
    }
    fn place_replacement(&mut self, out: &mut impl Sink<Effect>, item: &ItemId, (_, aside): Sa) -> R<Aside> {
        out.push(Effect::PlaceAt { item: *item, name: Name::Target });
        Ok(Aside { item: *item, aside: aside.ok_or(())? })
    }
    fn no_aside_next(&mut self, out: &mut impl Sink<Effect>, item: &ItemId, _: Sa) -> R<ItemId> {
        self.done(*item, Outcome::Failed, out).ok_or(())
    }
    fn no_aside_last(&mut self, out: &mut impl Sink<Effect>, item: &ItemId, _: Sa) -> R {
        self.done(*item, Outcome::Failed, out);
        Ok(())
    }
    fn cancel_placing(&mut self, out: &mut impl Sink<Effect>, _: &ItemId, _: Pl) -> R {
        self.cancel(out)
    }
    fn placed_next(&mut self, out: &mut impl Sink<Effect>, item: &ItemId, (_, p): Pl) -> R<ItemId> {
        self.done(*item, placed_outcome(p), out).ok_or(())
    }
    fn placed_last(&mut self, out: &mut impl Sink<Effect>, item: &ItemId, (_, p): Pl) -> R {
        self.done(*item, placed_outcome(p), out);
        Ok(())
    }
    fn dispose(&mut self, out: &mut impl Sink<Effect>, a: &Aside, _: Pl) -> R<Aside> {
        let (item, aside) = (a.item, a.aside);
        out.push(if self.config.delete_replaced {
            Effect::Delete { item, aside }
        } else {
            Effect::Trash { item, aside }
        });
        Ok(*a)
    }
    fn restore_aside(&mut self, out: &mut impl Sink<Effect>, a: &Aside, (_, p): Pl) -> R<Restore> {
        out.push(Effect::RestoreAside { item: a.item, aside: a.aside });
        let then = if p == Placed::Skip { After::Failed } else { After::Cancel };
        Ok(Restore { item: a.item, aside: a.aside, then })
    }
    fn disposed_next(&mut self, out: &mut impl Sink<Effect>, a: &Aside, (_, ok): Ok_) -> R<ItemId> {
        disposal_problem(self, a, ok, out);
        self.done(a.item, Outcome::Done, out).ok_or(())
    }
    fn disposed_last(&mut self, out: &mut impl Sink<Effect>, a: &Aside, (_, ok): Ok_) -> R {
        disposal_problem(self, a, ok, out);
        self.done(a.item, Outcome::Done, out);
        Ok(())
    }
    fn restored_cancel(&mut self, out: &mut impl Sink<Effect>, r: &Restore, (_, ok): Ok_) -> R {
        restore_problem(r, ok, out);
        self.cancel(out)
    }
    fn restored_next(&mut self, out: &mut impl Sink<Effect>, r: &Restore, (_, ok): Ok_) -> R<ItemId> {
        restore_problem(r, ok, out);
        self.done(r.item, Outcome::Failed, out).ok_or(())
    }
    fn restored_last(&mut self, out: &mut impl Sink<Effect>, r: &Restore, (_, ok): Ok_) -> R {
        restore_problem(r, ok, out);
        self.done(r.item, Outcome::Failed, out);
        Ok(())
    }
    fn listed_next(&mut self, out: &mut impl Sink<Effect>, item: &ItemId, (_, first, count): Ls) -> R<ItemId> {
        self.push(Some(*item), first.0, count);
        self.next(out).ok_or(())
    }
    fn listed_last(&mut self, out: &mut impl Sink<Effect>, item: &ItemId, (_, first, count): Ls) -> R {
        self.push(Some(*item), first.0, count);
        self.next(out);
        Ok(())
    }
}

pub struct Transfer {
    sm: StateMachine<Job>,
}

impl Transfer {
    /// The generated `process_event`: the typed transition API.
    pub fn process_event(&mut self, out: &mut impl Sink<Effect>, event: Events) -> Result<&States, Error> {
        self.sm.process_event(out, event)
    }

    /// The generated state.
    pub fn state_enum(&self) -> &States {
        self.sm.state()
    }

    /// The context, read-only.
    pub fn context(&self) -> &Job {
        self.sm.context()
    }
}

impl Machine for Transfer {
    fn new(config: Config) -> Self {
        Self { sm: StateMachine::new(Job::new(config)) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let event = match event {
            Event::Start { items } => Events::Start(items),
            Event::Probed { item, found } => Events::Probed((item, found)),
            Event::Answered { item, resolution } => Events::Answered((item, resolution)),
            Event::SetAside { item, aside } => Events::SetAside((item, aside)),
            Event::Placed { item, outcome } => Events::Placed((item, outcome)),
            Event::Disposed { item, ok } => Events::Disposed((item, ok)),
            Event::Restored { item, ok } => Events::Restored((item, ok)),
            Event::Listed { item, first, count } => Events::Listed((item, first, count)),
        };
        // Responses not awaited here, or for another item, are `Err`: ignored.
        let _ = self.sm.process_event(out, event);
    }

    fn state(&self) -> StateName {
        match self.sm.state() {
            States::Idle => StateName::Idle,
            States::Probing(_) => StateName::Probing,
            States::Asking(_) => StateName::Asking,
            States::Placing(_) => StateName::Placing,
            States::SettingAside(_) => StateName::SettingAside,
            States::PlacingReplacement(_) => StateName::PlacingReplacement,
            States::Disposing(_) => StateName::Disposing,
            States::Restoring(_) => StateName::Restoring,
            States::Listing(_) => StateName::Listing,
            States::Finished => StateName::Finished,
        }
    }
}
