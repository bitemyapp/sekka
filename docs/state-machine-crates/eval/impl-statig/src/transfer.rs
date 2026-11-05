//! `transfer` with statig 0.4.1: one leaf per awaited response, carrying the
//! item (and the aside, once there is one) as state-local storage; each
//! leaf's entry action sends its request, so every way into a state asks
//! for what it awaits. The merge frames are a stack in the shared storage:
//! statig's hierarchy is fixed at compile time, so the nested per-folder
//! machine can't be a superstate.

use spec::Sink;
use spec::transfer::{
    AsideId, Config, Effect, Event, Found, ItemId, Machine, Mode, Name, Outcome as ItemOutcome, Placed, Problem,
    Resolution, StateName,
};
use statig::prelude::*;

use crate::{Nowhere, Out};

pub const IMPLEMENTED: bool = true;

/// What a failed placement over a set-aside item ends in, once it is back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    Failed,
    Cancel,
}

/// One level of the merge: the items still to place in a folder (the top
/// frame has no folder).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Frame {
    folder: Option<ItemId>,
    next: u32,
    end: u32,
    complete: bool,
}

/// The shared storage.
pub struct Job {
    config: Config,
    frames: Vec<Frame>,
}

impl Job {
    /// next: the next item, finishing frames (merged folders) on the way.
    fn next(&mut self, out: &mut Out<'_, Effect>) -> Outcome<State> {
        while let Some(f) = self.frames.last_mut() {
            if f.next < f.end {
                let item = ItemId(f.next);
                f.next += 1;
                return Transition(State::probing(item));
            }
            let f = self.frames.pop().expect("a frame");
            if let Some(folder) = f.folder {
                if self.config.mode == Mode::Move && f.complete {
                    out.push(Effect::RemoveEmptySource { item: folder });
                }
                out.push(Effect::Outcome { item: folder, outcome: ItemOutcome::Done });
            }
        }
        out.push(Effect::Finished { cancelled: false });
        Transition(State::finished())
    }

    /// item done(item, o).
    fn done(&mut self, item: ItemId, outcome: ItemOutcome, out: &mut Out<'_, Effect>) -> Outcome<State> {
        out.push(Effect::Outcome { item, outcome });
        if outcome != ItemOutcome::Done
            && let Some(f) = self.frames.last_mut()
        {
            f.complete = false;
        }
        self.next(out)
    }

    fn cancel(&mut self, out: &mut Out<'_, Effect>) -> Outcome<State> {
        self.frames.clear();
        out.push(Effect::Finished { cancelled: true });
        Transition(State::finished())
    }
}

#[state_machine(initial = "State::idle()", state(derive(Debug, Clone, Copy, PartialEq, Eq)))]
impl Job {
    #[state]
    fn idle(&mut self, context: &mut Out<'_, Effect>, event: &Event) -> Outcome<State> {
        match *event {
            Event::Start { items } => {
                self.frames.push(Frame { folder: None, next: 0, end: items, complete: true });
                self.next(context)
            }
            _ => Handled,
        }
    }

    #[state(entry_action = "probe")]
    fn probing(&mut self, item: &mut ItemId, context: &mut Out<'_, Effect>, event: &Event) -> Outcome<State> {
        match *event {
            Event::Probed { item: i, found } if i == *item => match found {
                Found::Free => Transition(State::placing(*item, Name::Target)),
                Found::Same => self.done(*item, ItemOutcome::Done, context),
                Found::Refused => self.done(*item, ItemOutcome::Failed, context),
                Found::Conflict { can_merge } => Transition(State::asking(*item, can_merge)),
            },
            _ => Handled,
        }
    }

    #[action]
    fn probe(item: &mut ItemId, context: &mut Out<'_, Effect>) {
        context.push(Effect::Probe { item: *item });
    }

    #[state(entry_action = "ask")]
    fn asking(
        &mut self,
        item: &mut ItemId,
        can_merge: &mut bool,
        context: &mut Out<'_, Effect>,
        event: &Event,
    ) -> Outcome<State> {
        match *event {
            Event::Answered { item: i, resolution } if i == *item => match resolution {
                Resolution::Skip => self.done(*item, ItemOutcome::Skipped, context),
                Resolution::Cancel => self.cancel(context),
                Resolution::KeepBoth => Transition(State::placing(*item, Name::Free)),
                Resolution::Merge if *can_merge => Transition(State::listing(*item)),
                Resolution::Merge | Resolution::Replace => Transition(State::setting_aside(*item)),
            },
            _ => Handled,
        }
    }

    #[action]
    fn ask(item: &mut ItemId, can_merge: &mut bool, context: &mut Out<'_, Effect>) {
        context.push(Effect::Ask { item: *item, can_merge: *can_merge });
    }

    /// Placing under `name` (the target's, or a free one beside it).
    #[state(entry_action = "place", local_storage("name: Name"))]
    fn placing(&mut self, item: &mut ItemId, context: &mut Out<'_, Effect>, event: &Event) -> Outcome<State> {
        match *event {
            Event::Placed { item: i, outcome } if i == *item => match outcome {
                Placed::Ok => self.done(*item, ItemOutcome::Done, context),
                Placed::Skip => self.done(*item, ItemOutcome::Failed, context),
                Placed::Cancel => self.cancel(context),
            },
            _ => Handled,
        }
    }

    #[action]
    fn place(item: &mut ItemId, name: &mut Name, context: &mut Out<'_, Effect>) {
        context.push(Effect::PlaceAt { item: *item, name: *name });
    }

    #[state(entry_action = "set_aside")]
    fn setting_aside(&mut self, item: &mut ItemId, context: &mut Out<'_, Effect>, event: &Event) -> Outcome<State> {
        match *event {
            Event::SetAside { item: i, aside } if i == *item => match aside {
                Some(aside) => Transition(State::placing_replacement(*item, aside)),
                None => self.done(*item, ItemOutcome::Failed, context),
            },
            _ => Handled,
        }
    }

    #[action]
    fn set_aside(item: &mut ItemId, context: &mut Out<'_, Effect>) {
        context.push(Effect::SetAside { item: *item });
    }

    #[state(entry_action = "place_replacement")]
    fn placing_replacement(item: &mut ItemId, aside: &mut AsideId, event: &Event) -> Outcome<State> {
        match *event {
            Event::Placed { item: i, outcome } if i == *item => match outcome {
                Placed::Ok => Transition(State::disposing(*item, *aside)),
                Placed::Skip => Transition(State::restoring(*item, *aside, After::Failed)),
                Placed::Cancel => Transition(State::restoring(*item, *aside, After::Cancel)),
            },
            _ => Handled,
        }
    }

    #[action]
    fn place_replacement(item: &mut ItemId, context: &mut Out<'_, Effect>) {
        context.push(Effect::PlaceAt { item: *item, name: Name::Target });
    }

    #[state(entry_action = "dispose")]
    fn disposing(
        &mut self,
        item: &mut ItemId,
        aside: &mut AsideId,
        context: &mut Out<'_, Effect>,
        event: &Event,
    ) -> Outcome<State> {
        match *event {
            Event::Disposed { item: i, ok } if i == *item => {
                if !ok {
                    let problem =
                        if self.config.delete_replaced { Problem::NotAllRemoved } else { Problem::KeptAside(*aside) };
                    context.push(Effect::Problem { item: *item, problem });
                }
                self.done(*item, ItemOutcome::Done, context)
            }
            _ => Handled,
        }
    }

    /// dispose: the person's choice alone decides (never the job's kind).
    #[action]
    fn dispose(&self, item: &mut ItemId, aside: &mut AsideId, context: &mut Out<'_, Effect>) {
        let (item, aside) = (*item, *aside);
        context.push(if self.config.delete_replaced {
            Effect::Delete { item, aside }
        } else {
            Effect::Trash { item, aside }
        });
    }

    #[state(entry_action = "restore")]
    fn restoring(
        &mut self,
        item: &mut ItemId,
        aside: &mut AsideId,
        then: &mut After,
        context: &mut Out<'_, Effect>,
        event: &Event,
    ) -> Outcome<State> {
        match *event {
            Event::Restored { item: i, ok } if i == *item => {
                if !ok {
                    context.push(Effect::Problem { item: *item, problem: Problem::Stranded(*aside) });
                }
                match then {
                    After::Failed => self.done(*item, ItemOutcome::Failed, context),
                    After::Cancel => self.cancel(context),
                }
            }
            _ => Handled,
        }
    }

    #[action]
    fn restore(item: &mut ItemId, aside: &mut AsideId, context: &mut Out<'_, Effect>) {
        context.push(Effect::RestoreAside { item: *item, aside: *aside });
    }

    #[state(entry_action = "list")]
    fn listing(&mut self, item: &mut ItemId, context: &mut Out<'_, Effect>, event: &Event) -> Outcome<State> {
        match *event {
            Event::Listed { item: i, first, count } if i == *item => {
                let end = first.0.saturating_add(count);
                self.frames.push(Frame { folder: Some(*item), next: first.0, end, complete: true });
                self.next(context)
            }
            _ => Handled,
        }
    }

    #[action]
    fn list(item: &mut ItemId, context: &mut Out<'_, Effect>) {
        context.push(Effect::List { item: *item });
    }

    #[state]
    fn finished() -> Outcome<State> {
        Handled
    }
}

pub struct Transfer {
    machine: statig::blocking::InitializedStateMachine<Job>,
}

impl Transfer {
    pub fn state_enum(&self) -> &State {
        self.machine.state()
    }
}

impl Machine for Transfer {
    fn new(config: Config) -> Self {
        let machine = Job { config, frames: Vec::new() }.uninitialized_state_machine();
        Self { machine: machine.init_with_context(&mut Out(&mut Nowhere)) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        self.machine.handle_with_context(&event, &mut Out(out));
    }

    fn state(&self) -> StateName {
        match self.machine.state() {
            State::Idle {} => StateName::Idle,
            State::Probing { .. } => StateName::Probing,
            State::Asking { .. } => StateName::Asking,
            State::Placing { .. } => StateName::Placing,
            State::SettingAside { .. } => StateName::SettingAside,
            State::PlacingReplacement { .. } => StateName::PlacingReplacement,
            State::Disposing { .. } => StateName::Disposing,
            State::Restoring { .. } => StateName::Restoring,
            State::Listing { .. } => StateName::Listing,
            State::Finished {} => StateName::Finished,
        }
    }
}
