//! `transfer` with finny 0.3.0: one flat machine, one state per awaited
//! response, each state struct carrying its item (and the aside). The merge
//! frames can't be sub-machines: a finny sub-machine is a fixed type inside
//! its parent's state storage, so a folder merged inside a merged folder
//! would need the machine to contain itself. The frame stack is a `Vec` in
//! the context instead. "next" is a choice (another item, or the job is
//! done), and finny chooses the target before the action runs, so every
//! transition that finishes an item comes in a guarded pair: to `Probing`
//! while an item is left, else to `Finished`.

use finny::decl::{BuiltFsm, FsmBuilder};
use finny::inspect::null::InspectNull;
use finny::{FsmCurrentState, FsmEventQueueNull, FsmFactory, FsmResult, FsmTimersNull, finny_fsm};
use spec::Sink;
use spec::transfer::{
    AsideId, Config, Effect, Event, Found, ItemId, Machine, Mode, Name, Outcome, Placed, Problem, Resolution, StateName,
};

use crate::Unqueued;

pub const IMPLEMENTED: bool = true;

// Events.
#[derive(Clone, Copy, Debug)]
pub struct Start {
    pub items: u32,
}
#[derive(Clone, Copy, Debug)]
pub struct Probed {
    pub item: ItemId,
    pub found: Found,
}
#[derive(Clone, Copy, Debug)]
pub struct Answered {
    pub item: ItemId,
    pub resolution: Resolution,
}
#[derive(Clone, Copy, Debug)]
pub struct SetAside {
    pub item: ItemId,
    pub aside: Option<AsideId>,
}
#[derive(Clone, Copy, Debug)]
pub struct PlacedEv {
    pub item: ItemId,
    pub outcome: Placed,
}
#[derive(Clone, Copy, Debug)]
pub struct Disposed {
    pub item: ItemId,
    pub ok: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct Restored {
    pub item: ItemId,
    pub ok: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct Listed {
    pub item: ItemId,
    pub first: ItemId,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    Failed,
    Cancel,
}

// States. finny builds every state up front: `ItemId(0)`/`AsideId(0)` stand
// for "not entered yet".
#[derive(Default)]
pub struct Idle;
pub struct Probing {
    pub item: ItemId,
}
pub struct Asking {
    pub item: ItemId,
    pub can_merge: bool,
}
pub struct Placing {
    pub item: ItemId,
}
pub struct SettingAside {
    pub item: ItemId,
}
pub struct PlacingReplacement {
    pub item: ItemId,
    pub aside: AsideId,
}
pub struct Disposing {
    pub item: ItemId,
    pub aside: AsideId,
}
pub struct Restoring {
    pub item: ItemId,
    pub aside: AsideId,
    pub then: After,
}
pub struct Listing {
    pub item: ItemId,
}
#[derive(Default)]
pub struct Finished;

impl Default for Probing {
    fn default() -> Self {
        Self { item: ItemId(0) }
    }
}
impl Default for Asking {
    fn default() -> Self {
        Self { item: ItemId(0), can_merge: false }
    }
}
impl Default for Placing {
    fn default() -> Self {
        Self { item: ItemId(0) }
    }
}
impl Default for SettingAside {
    fn default() -> Self {
        Self { item: ItemId(0) }
    }
}
impl Default for PlacingReplacement {
    fn default() -> Self {
        Self { item: ItemId(0), aside: AsideId(0) }
    }
}
impl Default for Disposing {
    fn default() -> Self {
        Self { item: ItemId(0), aside: AsideId(0) }
    }
}
impl Default for Restoring {
    fn default() -> Self {
        Self { item: ItemId(0), aside: AsideId(0), then: After::Failed }
    }
}
impl Default for Listing {
    fn default() -> Self {
        Self { item: ItemId(0) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub folder: Option<ItemId>,
    pub next: u32,
    pub end: u32,
    pub complete: bool,
}

/// The job: its configuration, the frame stack and the effects. Popping
/// frames emits two effects each, so the buffer is a `Vec` that keeps its
/// capacity (like the frame stack).
pub struct Job {
    config: Config,
    frames: Vec<Frame>,
    out: Vec<Effect>,
}

impl Job {
    /// Whether "next" finds another item (else it finishes the job).
    fn has_more(&self) -> bool {
        self.frames.iter().any(|f| f.next < f.end)
    }

    /// "next": the next item, probed; `None` when the job finished.
    fn next(&mut self) -> Option<ItemId> {
        while let Some(f) = self.frames.last_mut() {
            if f.next < f.end {
                let item = ItemId(f.next);
                f.next += 1;
                self.out.push(Effect::Probe { item });
                return Some(item);
            }
            let f = self.frames.pop().expect("a frame");
            if let Some(folder) = f.folder {
                if self.config.mode == Mode::Move && f.complete {
                    self.out.push(Effect::RemoveEmptySource { item: folder });
                }
                self.out.push(Effect::Outcome { item: folder, outcome: Outcome::Done });
            }
        }
        self.out.push(Effect::Finished { cancelled: false });
        None
    }

    /// "item done": the outcome, then "next".
    fn done(&mut self, item: ItemId, outcome: Outcome) -> Option<ItemId> {
        self.out.push(Effect::Outcome { item, outcome });
        if outcome != Outcome::Done
            && let Some(f) = self.frames.last_mut()
        {
            f.complete = false;
        }
        self.next()
    }

    fn cancel(&mut self) {
        self.frames.clear();
        self.out.push(Effect::Finished { cancelled: true });
    }

    fn push_frame(&mut self, folder: Option<ItemId>, first: u32, count: u32) {
        self.frames.push(Frame { folder, next: first, end: first.saturating_add(count), complete: true });
    }

    /// Never decided by the job's kind.
    fn dispose(&mut self, item: ItemId, aside: AsideId) {
        self.out.push(if self.config.delete_replaced {
            Effect::Delete { item, aside }
        } else {
            Effect::Trash { item, aside }
        });
    }

    fn disposed(&mut self, item: ItemId, aside: AsideId, ok: bool) {
        if !ok {
            let problem = if self.config.delete_replaced { Problem::NotAllRemoved } else { Problem::KeptAside(aside) };
            self.out.push(Effect::Problem { item, problem });
        }
    }

    fn restored(&mut self, item: ItemId, aside: AsideId, ok: bool) {
        if !ok {
            self.out.push(Effect::Problem { item, problem: Problem::Stranded(aside) });
        }
    }
}

fn probed_outcome(found: Found) -> Option<Outcome> {
    match found {
        Found::Same => Some(Outcome::Done),
        Found::Refused => Some(Outcome::Failed),
        Found::Free | Found::Conflict { .. } => None,
    }
}

fn placed_outcome(placed: Placed) -> Option<Outcome> {
    match placed {
        Placed::Ok => Some(Outcome::Done),
        Placed::Skip => Some(Outcome::Failed),
        Placed::Cancel => None,
    }
}

#[finny_fsm]
fn job_fsm(mut fsm: FsmBuilder<JobFsm, Job>) -> BuiltFsm {
    fsm.initial_state::<Idle>();

    fsm.state::<Idle>().on_event::<Start>().transition_to::<Probing>().guard(|ev, _ctx, _s| ev.items > 0).action(
        |ev, ctx, _from, to| {
            ctx.push_frame(None, 0, ev.items);
            to.item = ctx.next().expect("an item");
        },
    );
    fsm.state::<Idle>().on_event::<Start>().transition_to::<Finished>().action(|ev, ctx, _from, _to| {
        ctx.push_frame(None, 0, ev.items);
        ctx.next();
    });

    fsm.state::<Probing>()
        .on_event::<Probed>()
        .transition_to::<Placing>()
        .guard(|ev, _ctx, s| ev.item == s.probing.item && ev.found == Found::Free)
        .action(|_ev, ctx, from, to| {
            ctx.out.push(Effect::PlaceAt { item: from.item, name: Name::Target });
            to.item = from.item;
        });
    fsm.state::<Probing>()
        .on_event::<Probed>()
        .transition_to::<Asking>()
        .guard(|ev, _ctx, s| ev.item == s.probing.item && matches!(ev.found, Found::Conflict { .. }))
        .action(|ev, ctx, from, to| {
            let Found::Conflict { can_merge } = ev.found else { unreachable!() };
            ctx.out.push(Effect::Ask { item: from.item, can_merge });
            *to = Asking { item: from.item, can_merge };
        });
    fsm.state::<Probing>()
        .on_event::<Probed>()
        .internal_transition()
        .guard(|ev, ctx, s| ev.item == s.probing.item && probed_outcome(ev.found).is_some() && ctx.has_more())
        .action(|ev, ctx, s| s.item = ctx.done(s.item, probed_outcome(ev.found).unwrap()).expect("an item"));
    fsm.state::<Probing>()
        .on_event::<Probed>()
        .transition_to::<Finished>()
        .guard(|ev, _ctx, s| ev.item == s.probing.item && probed_outcome(ev.found).is_some())
        .action(|ev, ctx, from, _to| {
            ctx.done(from.item, probed_outcome(ev.found).unwrap());
        });

    fsm.state::<Asking>()
        .on_event::<Answered>()
        .transition_to::<Probing>()
        .guard(|ev, ctx, s| ev.item == s.asking.item && ev.resolution == Resolution::Skip && ctx.has_more())
        .action(|_ev, ctx, from, to| to.item = ctx.done(from.item, Outcome::Skipped).expect("an item"));
    fsm.state::<Asking>()
        .on_event::<Answered>()
        .transition_to::<Finished>()
        .guard(|ev, _ctx, s| ev.item == s.asking.item && ev.resolution == Resolution::Skip)
        .action(|_ev, ctx, from, _to| {
            ctx.done(from.item, Outcome::Skipped);
        });
    fsm.state::<Asking>()
        .on_event::<Answered>()
        .transition_to::<Finished>()
        .guard(|ev, _ctx, s| ev.item == s.asking.item && ev.resolution == Resolution::Cancel)
        .action(|_ev, ctx, _from, _to| ctx.cancel());
    fsm.state::<Asking>()
        .on_event::<Answered>()
        .transition_to::<Placing>()
        .guard(|ev, _ctx, s| ev.item == s.asking.item && ev.resolution == Resolution::KeepBoth)
        .action(|_ev, ctx, from, to| {
            ctx.out.push(Effect::PlaceAt { item: from.item, name: Name::Free });
            to.item = from.item;
        });
    fsm.state::<Asking>()
        .on_event::<Answered>()
        .transition_to::<Listing>()
        .guard(|ev, _ctx, s| ev.item == s.asking.item && ev.resolution == Resolution::Merge && s.asking.can_merge)
        .action(|_ev, ctx, from, to| {
            ctx.out.push(Effect::List { item: from.item });
            to.item = from.item;
        });
    // Replace, or a merge that can't be one.
    fsm.state::<Asking>()
        .on_event::<Answered>()
        .transition_to::<SettingAside>()
        .guard(|ev, _ctx, s| {
            ev.item == s.asking.item && matches!(ev.resolution, Resolution::Merge | Resolution::Replace)
        })
        .action(|_ev, ctx, from, to| {
            ctx.out.push(Effect::SetAside { item: from.item });
            to.item = from.item;
        });

    fsm.state::<SettingAside>()
        .on_event::<SetAside>()
        .transition_to::<PlacingReplacement>()
        .guard(|ev, _ctx, s| ev.item == s.setting_aside.item && ev.aside.is_some())
        .action(|ev, ctx, from, to| {
            ctx.out.push(Effect::PlaceAt { item: from.item, name: Name::Target });
            *to = PlacingReplacement { item: from.item, aside: ev.aside.unwrap() };
        });
    fsm.state::<SettingAside>()
        .on_event::<SetAside>()
        .transition_to::<Probing>()
        .guard(|ev, ctx, s| ev.item == s.setting_aside.item && ctx.has_more())
        .action(|_ev, ctx, from, to| to.item = ctx.done(from.item, Outcome::Failed).expect("an item"));
    fsm.state::<SettingAside>()
        .on_event::<SetAside>()
        .transition_to::<Finished>()
        .guard(|ev, _ctx, s| ev.item == s.setting_aside.item)
        .action(|_ev, ctx, from, _to| {
            ctx.done(from.item, Outcome::Failed);
        });

    fsm.state::<Placing>()
        .on_event::<PlacedEv>()
        .transition_to::<Probing>()
        .guard(|ev, ctx, s| ev.item == s.placing.item && placed_outcome(ev.outcome).is_some() && ctx.has_more())
        .action(|ev, ctx, from, to| {
            to.item = ctx.done(from.item, placed_outcome(ev.outcome).unwrap()).expect("an item");
        });
    fsm.state::<Placing>()
        .on_event::<PlacedEv>()
        .transition_to::<Finished>()
        .guard(|ev, _ctx, s| ev.item == s.placing.item && placed_outcome(ev.outcome).is_some())
        .action(|ev, ctx, from, _to| {
            ctx.done(from.item, placed_outcome(ev.outcome).unwrap());
        });
    fsm.state::<Placing>()
        .on_event::<PlacedEv>()
        .transition_to::<Finished>()
        .guard(|ev, _ctx, s| ev.item == s.placing.item)
        .action(|_ev, ctx, _from, _to| ctx.cancel());

    fsm.state::<PlacingReplacement>()
        .on_event::<PlacedEv>()
        .transition_to::<Disposing>()
        .guard(|ev, _ctx, s| ev.item == s.placing_replacement.item && ev.outcome == Placed::Ok)
        .action(|_ev, ctx, from, to| {
            ctx.dispose(from.item, from.aside);
            *to = Disposing { item: from.item, aside: from.aside };
        });
    fsm.state::<PlacingReplacement>()
        .on_event::<PlacedEv>()
        .transition_to::<Restoring>()
        .guard(|ev, _ctx, s| ev.item == s.placing_replacement.item)
        .action(|ev, ctx, from, to| {
            ctx.out.push(Effect::RestoreAside { item: from.item, aside: from.aside });
            let then = if ev.outcome == Placed::Skip { After::Failed } else { After::Cancel };
            *to = Restoring { item: from.item, aside: from.aside, then };
        });

    fsm.state::<Disposing>()
        .on_event::<Disposed>()
        .transition_to::<Probing>()
        .guard(|ev, ctx, s| ev.item == s.disposing.item && ctx.has_more())
        .action(|ev, ctx, from, to| {
            ctx.disposed(from.item, from.aside, ev.ok);
            to.item = ctx.done(from.item, Outcome::Done).expect("an item");
        });
    fsm.state::<Disposing>()
        .on_event::<Disposed>()
        .transition_to::<Finished>()
        .guard(|ev, _ctx, s| ev.item == s.disposing.item)
        .action(|ev, ctx, from, _to| {
            ctx.disposed(from.item, from.aside, ev.ok);
            ctx.done(from.item, Outcome::Done);
        });

    fsm.state::<Restoring>()
        .on_event::<Restored>()
        .transition_to::<Probing>()
        .guard(|ev, ctx, s| ev.item == s.restoring.item && s.restoring.then == After::Failed && ctx.has_more())
        .action(|ev, ctx, from, to| {
            ctx.restored(from.item, from.aside, ev.ok);
            to.item = ctx.done(from.item, Outcome::Failed).expect("an item");
        });
    fsm.state::<Restoring>()
        .on_event::<Restored>()
        .transition_to::<Finished>()
        .guard(|ev, _ctx, s| ev.item == s.restoring.item)
        .action(|ev, ctx, from, _to| {
            ctx.restored(from.item, from.aside, ev.ok);
            match from.then {
                After::Failed => {
                    ctx.done(from.item, Outcome::Failed);
                }
                After::Cancel => ctx.cancel(),
            }
        });

    fsm.state::<Listing>()
        .on_event::<Listed>()
        .transition_to::<Probing>()
        .guard(|ev, ctx, s| {
            ev.item == s.listing.item && (ev.first.0 < ev.first.0.saturating_add(ev.count) || ctx.has_more())
        })
        .action(|ev, ctx, from, to| {
            ctx.push_frame(Some(from.item), ev.first.0, ev.count);
            to.item = ctx.next().expect("an item");
        });
    fsm.state::<Listing>()
        .on_event::<Listed>()
        .transition_to::<Finished>()
        .guard(|ev, _ctx, s| ev.item == s.listing.item)
        .action(|ev, ctx, from, _to| {
            ctx.push_frame(Some(from.item), ev.first.0, ev.count);
            ctx.next();
        });

    // `Finished` ignores everything.
    fsm.state::<Finished>();

    fsm.build()
}

pub struct Transfer {
    fsm: Unqueued<JobFsm>,
}

impl Transfer {
    pub fn fsm(&self) -> &Unqueued<JobFsm> {
        &self.fsm
    }

    /// finny's `dispatch` of a typed event, then the effects.
    pub fn dispatch(&mut self, event: impl Into<JobFsmEvents>, out: &mut impl Sink<Effect>) -> FsmResult<()> {
        let r = self.fsm.dispatch(event);
        for e in self.fsm.context.out.drain(..) {
            out.push(e);
        }
        r
    }
}

impl Machine for Transfer {
    fn new(config: Config) -> Self {
        let job = Job { config, frames: Vec::new(), out: Vec::new() };
        let mut fsm = JobFsm::new_with(job, FsmEventQueueNull::new(), InspectNull::new(), FsmTimersNull)
            .expect("the states are built from defaults");
        fsm.start().expect("Idle has no entry action");
        Self { fsm }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let ev: JobFsmEvents = match event {
            Event::Start { items } => Start { items }.into(),
            Event::Probed { item, found } => Probed { item, found }.into(),
            Event::Answered { item, resolution } => Answered { item, resolution }.into(),
            Event::SetAside { item, aside } => SetAside { item, aside }.into(),
            Event::Placed { item, outcome } => PlacedEv { item, outcome }.into(),
            Event::Disposed { item, ok } => Disposed { item, ok }.into(),
            Event::Restored { item, ok } => Restored { item, ok }.into(),
            Event::Listed { item, first, count } => Listed { item, first, count }.into(),
        };
        // `Err(NoTransition)`: not the awaited response for the current item.
        let _ = self.dispatch(ev, out);
    }

    fn state(&self) -> StateName {
        use FsmCurrentState::State;
        match self.fsm.get_current_states()[0] {
            State(JobFsmCurrentState::Idle) | FsmCurrentState::Stopped => StateName::Idle,
            State(JobFsmCurrentState::Probing) => StateName::Probing,
            State(JobFsmCurrentState::Asking) => StateName::Asking,
            State(JobFsmCurrentState::Placing) => StateName::Placing,
            State(JobFsmCurrentState::SettingAside) => StateName::SettingAside,
            State(JobFsmCurrentState::PlacingReplacement) => StateName::PlacingReplacement,
            State(JobFsmCurrentState::Disposing) => StateName::Disposing,
            State(JobFsmCurrentState::Restoring) => StateName::Restoring,
            State(JobFsmCurrentState::Listing) => StateName::Listing,
            State(JobFsmCurrentState::Finished) => StateName::Finished,
        }
    }
}
