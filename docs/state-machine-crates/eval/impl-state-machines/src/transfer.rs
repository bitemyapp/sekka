//! `transfer` with state-machines 0.34.0: `Running` is a superstate whose
//! data is the frame stack (merges push frames; leaving `Running` drops
//! them); each leaf's data is its item (and aside), made by `data:`
//! factories. **next** is a factory too: it pops finished frames, probes
//! the next item and returns it as `Probing`'s data; a `has_next` guard
//! chooses between it and `Finished`. The crate's invoked child machines
//! need its async runtime, so a merge's sub-machine is a frame, as in the
//! reference.

use spec::Sink;
use spec::transfer::{
    AsideId, Config, Effect, Event, Found, ItemId, Machine, Mode, Name, Outcome, Placed, Problem, Resolution, StateName,
};
use state_machines::state_machine;

pub const IMPLEMENTED: bool = true;

pub use machine::{
    Asking, AskingMergeable, AskingUnmergeable, Disposing, Finished, Idle, Job, JobAskingMergeableAnsweredOutcome,
    JobAskingUnmergeableAnsweredOutcome, JobDisposingDisposedOutcome, JobIdleStartOutcome, JobListingListedOutcome,
    JobPlacingPlacedOutcome, JobPlacingReplacementPlacedOutcome, JobProbingProbedOutcome, JobRestoringRestoredOutcome,
    JobSettingAsideSetAsideOutcome, Listing, Placing, PlacingReplacement, Probing, Restoring, Running, SettingAside,
    Transfer,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub folder: Option<ItemId>,
    pub next: u32,
    pub end: u32,
    pub complete: bool,
}

/// `Running`'s data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Frames(Vec<Frame>);

impl Frames {
    fn has_next(&self) -> bool {
        self.0.iter().any(|f| f.next < f.end)
    }

    /// **next** until an item or the end: pops finished frames (a merge
    /// frame reports its folder), probes the next item.
    fn next(&mut self, mode: Mode, out: &mut Vec<Effect>) -> Option<ItemId> {
        while let Some(f) = self.0.last_mut() {
            if f.next < f.end {
                let item = ItemId(f.next);
                f.next += 1;
                out.push(Effect::Probe { item });
                return Some(item);
            }
            let f = self.0.pop().expect("a frame");
            if let Some(folder) = f.folder {
                if mode == Mode::Move && f.complete {
                    out.push(Effect::RemoveEmptySource { item: folder });
                }
                out.push(Effect::Outcome { item: folder, outcome: Outcome::Done });
            }
        }
        None
    }
}

/// `PlacingReplacement`'s and `Disposing`'s data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aside {
    item: ItemId,
    aside: AsideId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    Failed,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Restore {
    item: ItemId,
    aside: AsideId,
    then: After,
}

/// The payloads that name an item.
pub trait Names {
    fn item(&self) -> ItemId;
}

impl<X> Names for (ItemId, X) {
    fn item(&self) -> ItemId {
        self.0
    }
}
impl Names for (ItemId, ItemId, u32) {
    fn item(&self) -> ItemId {
        self.0
    }
}

#[derive(Debug)]
pub struct Ctx {
    config: Config,
    out: Vec<Effect>,
}

impl Ctx {
    pub fn new(config: Config) -> Self {
        Self { config, out: Vec::with_capacity(crate::out::CAPACITY) }
    }
}

/// The generated machines, their callbacks and the facade. The module is
/// private so that the generated `Dynamic*` wrapper (with its
/// `new_init_state`), its event and its state enums aren't exported.
mod machine {
    use super::*;
    use crate::out::drain;

    state_machine! {
        name: Job,
        context: Ctx,
        dynamic: true,
        initial: Idle,
        states: [
            Idle,
            superstate Running(Frames) {
                state Probing(ItemId),
                // Whether the two can merge is the state, not data (T6).
                superstate Asking {
                    state AskingMergeable(ItemId),
                    state AskingUnmergeable(ItemId),
                },
                state Placing(ItemId),
                state SettingAside(ItemId),
                state PlacingReplacement(Aside),
                state Disposing(Aside),
                state Restoring(Restore),
                state Listing(ItemId),
            },
            Finished,
        ],
        events {
            start {
                payload: u32,
                branching: true,
                transition: { from: Idle, to: Probing, guards: [has_items], data: first_item, after: [push_top_frame] }
                transition: { from: Idle, to: Finished, fallback: true, before: [finish] }
            }
            probed {
                payload: (ItemId, Found),
                guards: [current],
                branching: true,
                transition: { from: Probing, to: Placing, guards: [is_free], data: keep_item, before: [place_at_target] }
                transition: { from: Probing, to: AskingMergeable, guards: [is_mergeable], data: keep_item, before: [ask] }
                transition: { from: Probing, to: AskingUnmergeable, guards: [is_conflict], data: keep_item, before: [ask] }
                transition: { from: Probing, to: Probing, guards: [is_same, has_next], before: [done], data: next_item }
                transition: { from: Probing, to: Finished, guards: [is_same], before: [done, finish] }
                // Refused.
                transition: { from: Probing, to: Probing, guards: [has_next], before: [failed], data: next_item }
                transition: { from: Probing, to: Finished, fallback: true, before: [failed, finish] }
            }
            answered {
                payload: (ItemId, Resolution),
                guards: [current],
                branching: true,
                transition: { from: Asking, to: Probing, guards: [is_skip, has_next], before: [skipped], data: next_item }
                transition: { from: Asking, to: Finished, guards: [is_skip], before: [skipped, finish] }
                transition: { from: Asking, to: Finished, guards: [is_cancel], before: [cancel] }
                transition: { from: Asking, to: Placing, guards: [is_keep_both], data: keep_item, before: [place_at_free] }
                transition: { from: AskingMergeable, to: Listing, guards: [is_merge], data: keep_item, before: [list] }
                // Replace, or Merge when the two can't merge.
                transition: { from: Asking, to: SettingAside, fallback: true, data: keep_item, before: [rename_aside] }
            }
            set_aside {
                payload: (ItemId, Option<AsideId>),
                guards: [current],
                branching: true,
                transition: { from: SettingAside, to: PlacingReplacement, guards: [was_set_aside], data: aside, before: [place_at_target] }
                transition: { from: SettingAside, to: Probing, guards: [has_next], before: [failed], data: next_item }
                transition: { from: SettingAside, to: Finished, fallback: true, before: [failed, finish] }
            }
            placed {
                payload: (ItemId, Placed),
                guards: [current],
                branching: true,
                transition: { from: Placing, to: Finished, guards: [is_stop], before: [cancel] }
                transition: { from: Placing, to: Probing, guards: [placed_ok, has_next], before: [done], data: next_item }
                transition: { from: Placing, to: Finished, guards: [placed_ok], before: [done, finish] }
                // Skipped after a failure.
                transition: { from: Placing, to: Probing, guards: [has_next], before: [failed], data: next_item }
                transition: { from: Placing, to: Finished, fallback: true, before: [failed, finish] }
                transition: { from: PlacingReplacement, to: Disposing, guards: [placed_ok], data: keep_aside, before: [dispose] }
                transition: { from: PlacingReplacement, to: Restoring, fallback: true, data: restore, before: [put_back] }
            }
            disposed {
                payload: (ItemId, bool),
                guards: [current],
                branching: true,
                transition: { from: Disposing, to: Probing, guards: [has_next], before: [report_disposal, done], data: next_item }
                transition: { from: Disposing, to: Finished, fallback: true, before: [report_disposal, done, finish] }
            }
            restored {
                payload: (ItemId, bool),
                guards: [current],
                branching: true,
                transition: { from: Restoring, to: Finished, guards: [then_cancel], before: [report_restore, cancel] }
                transition: { from: Restoring, to: Probing, guards: [has_next], before: [report_restore, failed], data: next_item }
                transition: { from: Restoring, to: Finished, fallback: true, before: [report_restore, failed, finish] }
            }
            listed {
                payload: (ItemId, ItemId, u32),
                guards: [current],
                branching: true,
                transition: { from: Listing, to: Probing, guards: [lists_more], data: enter_folder }
                transition: { from: Listing, to: Finished, fallback: true, before: [push_folder, finish] }
            }
        }
    }

    impl<S> Job<S> {
        /// The frame stack (set on entering `Running`; empty outside it).
        fn frames(&mut self) -> &mut Frames {
            self.__state_data_running.get_or_insert_default()
        }

        /// The current item: the data of whichever leaf this is.
        fn item(&self) -> Option<ItemId> {
            let item = self.state_data_probing().or(self.state_data_placing());
            let item = item.or(self.state_data_setting_aside()).or(self.state_data_listing());
            let item = item.or(self.state_data_asking_mergeable()).or(self.state_data_asking_unmergeable()).copied();
            let aside = self.state_data_placing_replacement().or(self.state_data_disposing());
            item.or(aside.map(|a| a.item)).or(self.state_data_restoring().map(|r| r.item))
        }

        fn the_item(&self) -> ItemId {
            self.item().expect("entered with its item")
        }

        fn the_aside(&self) -> AsideId {
            let aside = self.state_data_placing_replacement().or(self.state_data_disposing()).map(|a| a.aside);
            aside.or(self.state_data_restoring().map(|r| r.aside)).expect("entered with its aside")
        }

        fn outcome(&mut self, outcome: Outcome) {
            let item = self.the_item();
            self.ctx.out.push(Effect::Outcome { item, outcome });
            if outcome != Outcome::Done
                && let Some(f) = self.frames().0.last_mut()
            {
                f.complete = false;
            }
        }

        // Guards.

        fn current<P: Names>(&self, _: &Ctx, p: &P) -> bool {
            self.item() == Some(p.item())
        }

        fn has_next<P>(&self, _: &Ctx, _: &P) -> bool {
            self.state_data_running().is_some_and(Frames::has_next)
        }

        fn has_items(&self, _: &Ctx, &items: &u32) -> bool {
            items > 0
        }

        fn is_free(&self, _: &Ctx, p: &(ItemId, Found)) -> bool {
            p.1 == Found::Free
        }

        fn is_mergeable(&self, _: &Ctx, p: &(ItemId, Found)) -> bool {
            p.1 == Found::Conflict { can_merge: true }
        }

        fn is_conflict(&self, _: &Ctx, p: &(ItemId, Found)) -> bool {
            matches!(p.1, Found::Conflict { .. })
        }

        fn is_same(&self, _: &Ctx, p: &(ItemId, Found)) -> bool {
            p.1 == Found::Same
        }

        fn is_skip(&self, _: &Ctx, p: &(ItemId, Resolution)) -> bool {
            p.1 == Resolution::Skip
        }

        fn is_cancel(&self, _: &Ctx, p: &(ItemId, Resolution)) -> bool {
            p.1 == Resolution::Cancel
        }

        fn is_keep_both(&self, _: &Ctx, p: &(ItemId, Resolution)) -> bool {
            p.1 == Resolution::KeepBoth
        }

        fn is_merge(&self, _: &Ctx, p: &(ItemId, Resolution)) -> bool {
            p.1 == Resolution::Merge
        }

        fn was_set_aside(&self, _: &Ctx, p: &(ItemId, Option<AsideId>)) -> bool {
            p.1.is_some()
        }

        fn is_stop(&self, _: &Ctx, p: &(ItemId, Placed)) -> bool {
            p.1 == Placed::Cancel
        }

        fn placed_ok(&self, _: &Ctx, p: &(ItemId, Placed)) -> bool {
            p.1 == Placed::Ok
        }

        fn then_cancel(&self, _: &Ctx, _: &(ItemId, bool)) -> bool {
            self.state_data_restoring().is_some_and(|r| r.then == After::Cancel)
        }

        fn lists_more(&self, _: &Ctx, &(_, _, count): &(ItemId, ItemId, u32)) -> bool {
            count > 0 || self.state_data_running().is_some_and(Frames::has_next)
        }

        // Factories.

        fn first_item(&mut self, _: &mut u32) -> ItemId {
            self.ctx.out.push(Effect::Probe { item: ItemId(0) });
            ItemId(0)
        }

        fn next_item<P>(&mut self, _: &mut P) -> ItemId {
            let frames = self.__state_data_running.get_or_insert_default();
            frames.next(self.ctx.config.mode, &mut self.ctx.out).expect("guarded by has_next")
        }

        fn keep_item<P>(&mut self, _: &mut P) -> ItemId {
            self.the_item()
        }

        fn aside(&mut self, p: &mut (ItemId, Option<AsideId>)) -> Aside {
            Aside { item: p.0, aside: p.1.expect("guarded by was_set_aside") }
        }

        fn keep_aside(&mut self, _: &mut (ItemId, Placed)) -> Aside {
            Aside { item: self.the_item(), aside: self.the_aside() }
        }

        fn restore(&mut self, p: &mut (ItemId, Placed)) -> Restore {
            let then = if p.1 == Placed::Skip { After::Failed } else { After::Cancel };
            Restore { item: self.the_item(), aside: self.the_aside(), then }
        }

        fn enter_folder(&mut self, p: &mut (ItemId, ItemId, u32)) -> ItemId {
            self.push_folder(p);
            self.next_item(p)
        }

        // Callbacks.

        /// `Start`: the top frame, its first item already probed by `first_item`
        /// (`Running`'s data exists only once the machine is in it).
        fn push_top_frame(&mut self, &items: &u32) {
            self.frames().0.push(Frame { folder: None, next: 1, end: items, complete: true });
        }

        fn push_folder(&mut self, &(folder, first, count): &(ItemId, ItemId, u32)) {
            let end = first.0.saturating_add(count);
            self.frames().0.push(Frame { folder: Some(folder), next: first.0, end, complete: true });
        }

        /// **next** with nothing left: the remaining frames end, then the job.
        fn finish<P>(&mut self, _: &P) {
            if let Some(frames) = self.__state_data_running.as_mut() {
                let none = frames.next(self.ctx.config.mode, &mut self.ctx.out);
                debug_assert!(none.is_none(), "guarded by has_next");
            }
            self.ctx.out.push(Effect::Finished { cancelled: false });
        }

        fn cancel<P>(&mut self, _: &P) {
            self.ctx.out.push(Effect::Finished { cancelled: true });
        }

        fn done<P>(&mut self, _: &P) {
            self.outcome(Outcome::Done);
        }

        fn failed<P>(&mut self, _: &P) {
            self.outcome(Outcome::Failed);
        }

        fn skipped<P>(&mut self, _: &P) {
            self.outcome(Outcome::Skipped);
        }

        fn place_at_target<P: Names>(&mut self, p: &P) {
            self.ctx.out.push(Effect::PlaceAt { item: p.item(), name: Name::Target });
        }

        fn place_at_free(&mut self, p: &(ItemId, Resolution)) {
            self.ctx.out.push(Effect::PlaceAt { item: p.0, name: Name::Free });
        }

        fn ask(&mut self, p: &(ItemId, Found)) {
            let can_merge = matches!(p.1, Found::Conflict { can_merge: true });
            self.ctx.out.push(Effect::Ask { item: p.0, can_merge });
        }

        fn list(&mut self, p: &(ItemId, Resolution)) {
            self.ctx.out.push(Effect::List { item: p.0 });
        }

        fn rename_aside(&mut self, p: &(ItemId, Resolution)) {
            self.ctx.out.push(Effect::SetAside { item: p.0 });
        }

        /// Never decided by the job's kind (HAKO-05).
        fn dispose(&mut self, _: &(ItemId, Placed)) {
            let (item, aside) = (self.the_item(), self.the_aside());
            let effect = if self.ctx.config.delete_replaced {
                Effect::Delete { item, aside }
            } else {
                Effect::Trash { item, aside }
            };
            self.ctx.out.push(effect);
        }

        fn put_back(&mut self, _: &(ItemId, Placed)) {
            let (item, aside) = (self.the_item(), self.the_aside());
            self.ctx.out.push(Effect::RestoreAside { item, aside });
        }

        fn report_disposal(&mut self, &(item, ok): &(ItemId, bool)) {
            if !ok {
                let problem = if self.ctx.config.delete_replaced {
                    Problem::NotAllRemoved
                } else {
                    Problem::KeptAside(self.the_aside())
                };
                self.ctx.out.push(Effect::Problem { item, problem });
            }
        }

        fn report_restore(&mut self, &(item, ok): &(ItemId, bool)) {
            if !ok {
                let problem = Problem::Stranded(self.the_aside());
                self.ctx.out.push(Effect::Problem { item, problem });
            }
        }
    }

    /// The `Machine` facade over the generated runtime wrapper.
    pub struct Transfer {
        machine: DynamicJob,
    }

    /// The context inside the wrapper (`None` once a panicking callback
    /// poisoned it). `inner` and `AnyJobState` are private generated
    /// items, visible here because the macro expanded in this module.
    impl DynamicJob {
        fn ctx(&mut self) -> Option<&mut Ctx> {
            Some(match self.inner.as_mut()? {
                AnyJobState::Idle(m) => &mut m.ctx,
                AnyJobState::Probing(m) => &mut m.ctx,
                AnyJobState::AskingMergeable(m) => &mut m.ctx,
                AnyJobState::AskingUnmergeable(m) => &mut m.ctx,
                AnyJobState::Placing(m) => &mut m.ctx,
                AnyJobState::SettingAside(m) => &mut m.ctx,
                AnyJobState::PlacingReplacement(m) => &mut m.ctx,
                AnyJobState::Disposing(m) => &mut m.ctx,
                AnyJobState::Restoring(m) => &mut m.ctx,
                AnyJobState::Listing(m) => &mut m.ctx,
                AnyJobState::Finished(m) => &mut m.ctx,
            })
        }
    }

    impl<S> Job<S> {
        /// Takes the effects the callbacks buffered, for driving the typed
        /// machine directly (the compile-fail probes do).
        pub fn take_effects(&mut self) -> Vec<Effect> {
            std::mem::take(&mut self.ctx.out)
        }
    }

    impl Machine for Transfer {
        fn new(config: Config) -> Self {
            Self { machine: DynamicJob::new(Ctx::new(config)) }
        }

        fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
            let event = match event {
                Event::Start { items } => JobEvent::Start(items),
                Event::Probed { item, found } => JobEvent::Probed((item, found)),
                Event::Answered { item, resolution } => JobEvent::Answered((item, resolution)),
                Event::SetAside { item, aside } => JobEvent::SetAside((item, aside)),
                Event::Placed { item, outcome } => JobEvent::Placed((item, outcome)),
                Event::Disposed { item, ok } => JobEvent::Disposed((item, ok)),
                Event::Restored { item, ok } => JobEvent::Restored((item, ok)),
                Event::Listed { item, first, count } => JobEvent::Listed((item, first, count)),
            };
            // `Err`: no transition from here, or not the awaited item: ignored.
            let _ = self.machine.handle(event);
            if let Some(ctx) = self.machine.ctx() {
                drain(&mut ctx.out, out);
            }
        }

        fn state(&self) -> StateName {
            match self.machine.current_state() {
                JobState::Idle => StateName::Idle,
                JobState::Probing => StateName::Probing,
                JobState::AskingMergeable | JobState::AskingUnmergeable => StateName::Asking,
                JobState::Placing => StateName::Placing,
                JobState::SettingAside => StateName::SettingAside,
                JobState::PlacingReplacement => StateName::PlacingReplacement,
                JobState::Disposing => StateName::Disposing,
                JobState::Restoring => StateName::Restoring,
                JobState::Listing => StateName::Listing,
                JobState::Finished => StateName::Finished,
            }
        }
    }
}
