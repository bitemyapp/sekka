//! `transfer` with rust-fsm 0.8.0: the ten states are the table (`fsm`); the
//! current item, the aside, and the merge frames live beside it in [`Data`].
//!
//! What the DSL can't hold, and where it went:
//!
//! - the merge frames (a nested sub-machine per folder): a `Vec` beside the
//!   table; pushing and popping them is action code, not table rows;
//! - **next** decides between `Probing` and `Finished` from the frames: the
//!   rows that finish an item go to `Probing`, and **next** feeds a
//!   completion input, `Exhausted`, into the table when the job ends;
//! - the table's states are `spec::transfer::StateName` (the crate's custom
//!   state type), so `state()` reads the table directly;
//! - guards: [`Transfer::input`] drops responses for another item and
//!   splits the rest by payload (`ProbedFree`, `PlacedCancel`, …) and by the
//!   data (`Merge` only with `can_merge`, `RestoredCancel` after a cancel).

use rust_fsm::state_machine;
use spec::Sink;
use spec::transfer::{
    AsideId, Config, Effect, Event, Found, ItemId, Machine, Mode, Name, Outcome, Placed, Problem, Resolution, StateName,
};

pub const IMPLEMENTED: bool = true;

state_machine! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[state_machine(state(spec::transfer::StateName))]
    pub fsm(Idle)

    Idle(Start) => Probing [Begin],
    Probing => {
        ProbedFree => Placing [PlaceTarget],
        ProbedConflict => Asking [Ask],
        ProbedSame => Probing [Done],
        ProbedRefused => Probing [Fail],
        Exhausted => Finished,
    },
    Asking => {
        Skip => Probing [Skipped],
        Cancel => Finished [Cancel],
        KeepBoth => Placing [PlaceFree],
        Merge => Listing [List],
        Replace => SettingAside [SetAside],
    },
    SettingAside => {
        SetAside => PlacingReplacement [PlaceReplacement],
        SetAsideFailed => Probing [Fail],
    },
    Placing => {
        PlacedOk => Probing [Done],
        PlacedSkip => Probing [Fail],
        PlacedCancel => Finished [Cancel],
    },
    PlacingReplacement => {
        PlacedOk => Disposing [Dispose],
        PlacedSkip => Restoring [Restore],
        PlacedCancel => Restoring [Restore],
    },
    Disposing(Disposed) => Probing [Disposed],
    Restoring => {
        Restored => Probing [Restored],
        RestoredCancel => Finished [RestoredCancel],
    },
    Listing(Listed) => Probing [Push],
}

use fsm::{Input, Output};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    Failed,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub folder: Option<ItemId>,
    pub next: u32,
    pub end: u32,
    pub complete: bool,
}

/// The table, the job's config and the data the table can't hold, in one
/// struct (so the table's byte packs with the data). `item` is the current
/// item (stale in `Idle`/`Finished`), `can_merge` meaningful in `Asking`,
/// `aside` from `PlacingReplacement` to `Restoring`, `then` in `Restoring`.
#[derive(Clone, Debug)]
pub struct Data {
    fsm: fsm::StateMachine,
    config: Config,
    pub item: ItemId,
    pub can_merge: bool,
    pub aside: AsideId,
    pub then: After,
    pub frames: Vec<Frame>,
}

pub struct Transfer {
    data: Data,
}

impl Transfer {
    /// The table's state.
    pub fn table(&self) -> &fsm::StateMachine {
        &self.data.fsm
    }

    /// The data beside the table.
    pub fn data(&self) -> &Data {
        &self.data
    }

    /// The guards: a response for the current item, refined by its payload
    /// and the data; `None` for another item's.
    fn input(&self, event: &Event) -> Option<Input> {
        let d = &self.data;
        Some(match *event {
            Event::Start { .. } => Input::Start,
            Event::Probed { item, found } if item == d.item => match found {
                Found::Free => Input::ProbedFree,
                Found::Conflict { .. } => Input::ProbedConflict,
                Found::Same => Input::ProbedSame,
                Found::Refused => Input::ProbedRefused,
            },
            Event::Answered { item, resolution } if item == d.item => match resolution {
                Resolution::Skip => Input::Skip,
                Resolution::Cancel => Input::Cancel,
                Resolution::KeepBoth => Input::KeepBoth,
                Resolution::Merge if d.can_merge => Input::Merge,
                Resolution::Merge | Resolution::Replace => Input::Replace,
            },
            Event::SetAside { item, aside: Some(_) } if item == d.item => Input::SetAside,
            Event::SetAside { item, aside: None } if item == d.item => Input::SetAsideFailed,
            Event::Placed { item, outcome } if item == d.item => match outcome {
                Placed::Ok => Input::PlacedOk,
                Placed::Skip => Input::PlacedSkip,
                Placed::Cancel => Input::PlacedCancel,
            },
            Event::Disposed { item, .. } if item == d.item => Input::Disposed,
            Event::Restored { item, .. } if item == d.item => match d.then {
                After::Failed => Input::Restored,
                After::Cancel => Input::RestoredCancel,
            },
            Event::Listed { item, .. } if item == d.item => Input::Listed,
            _ => return None,
        })
    }

    /// **next**: probe the next item, finishing frames (and the job) on the way.
    fn next(&mut self, out: &mut impl Sink<Effect>) {
        let frames = &mut self.data.frames;
        while let Some(f) = frames.last_mut() {
            if f.next < f.end {
                let item = ItemId(f.next);
                f.next += 1;
                out.push(Effect::Probe { item });
                self.data.item = item;
                return;
            }
            let f = frames.pop().expect("a frame");
            if let Some(folder) = f.folder {
                if self.data.config.mode == Mode::Move && f.complete {
                    out.push(Effect::RemoveEmptySource { item: folder });
                }
                out.push(Effect::Outcome { item: folder, outcome: Outcome::Done });
            }
        }
        out.push(Effect::Finished { cancelled: false });
        // Always `Ok`: **next** runs only from rows that lead to `Probing`.
        let r = self.data.fsm.consume(&Input::Exhausted);
        debug_assert!(r.is_ok());
    }

    /// **item done**.
    fn done(&mut self, outcome: Outcome, out: &mut impl Sink<Effect>) {
        out.push(Effect::Outcome { item: self.data.item, outcome });
        if outcome != Outcome::Done
            && let Some(f) = self.data.frames.last_mut()
        {
            f.complete = false;
        }
        self.next(out);
    }

    fn cancel(&mut self, out: &mut impl Sink<Effect>) {
        self.data.frames.clear();
        out.push(Effect::Finished { cancelled: true });
    }

    /// The actions the outputs name. Exhaustive on the output, so a new
    /// output is a compile error; arms that need a payload take it from
    /// their event (the table pairs every output with one kind of event).
    fn act(&mut self, action: Output, event: Event, out: &mut impl Sink<Effect>) {
        let item = self.data.item;
        match action {
            Output::Begin => {
                let Event::Start { items } = event else { unreachable!() };
                self.data.frames.push(Frame { folder: None, next: 0, end: items, complete: true });
                self.next(out);
            }
            Output::PlaceTarget => out.push(Effect::PlaceAt { item, name: Name::Target }),
            Output::Ask => {
                let Event::Probed { found: Found::Conflict { can_merge }, .. } = event else { unreachable!() };
                self.data.can_merge = can_merge;
                out.push(Effect::Ask { item, can_merge });
            }
            Output::Done => self.done(Outcome::Done, out),
            Output::Fail => self.done(Outcome::Failed, out),
            Output::Skipped => self.done(Outcome::Skipped, out),
            Output::Cancel => self.cancel(out),
            Output::PlaceFree => out.push(Effect::PlaceAt { item, name: Name::Free }),
            Output::List => out.push(Effect::List { item }),
            Output::SetAside => out.push(Effect::SetAside { item }),
            Output::PlaceReplacement => {
                let Event::SetAside { aside: Some(aside), .. } = event else { unreachable!() };
                self.data.aside = aside;
                out.push(Effect::PlaceAt { item, name: Name::Target });
            }
            Output::Dispose => {
                let aside = self.data.aside;
                out.push(if self.data.config.delete_replaced {
                    Effect::Delete { item, aside }
                } else {
                    Effect::Trash { item, aside }
                });
            }
            Output::Restore => {
                let Event::Placed { outcome, .. } = event else { unreachable!() };
                self.data.then = if outcome == Placed::Cancel { After::Cancel } else { After::Failed };
                out.push(Effect::RestoreAside { item, aside: self.data.aside });
            }
            Output::Disposed => {
                let Event::Disposed { ok, .. } = event else { unreachable!() };
                if !ok {
                    let problem = if self.data.config.delete_replaced {
                        Problem::NotAllRemoved
                    } else {
                        Problem::KeptAside(self.data.aside)
                    };
                    out.push(Effect::Problem { item, problem });
                }
                self.done(Outcome::Done, out);
            }
            Output::Restored | Output::RestoredCancel => {
                let Event::Restored { ok, .. } = event else { unreachable!() };
                if !ok {
                    out.push(Effect::Problem { item, problem: Problem::Stranded(self.data.aside) });
                }
                if action == Output::Restored {
                    self.done(Outcome::Failed, out);
                } else {
                    self.cancel(out);
                }
            }
            Output::Push => {
                let Event::Listed { first, count, .. } = event else { unreachable!() };
                let end = first.0.saturating_add(count);
                self.data.frames.push(Frame { folder: Some(item), next: first.0, end, complete: true });
                self.next(out);
            }
        }
    }
}

impl Machine for Transfer {
    fn new(config: Config) -> Self {
        let fsm = fsm::StateMachine::new();
        let (item, aside, then, frames) = (ItemId(0), AsideId(0), After::Failed, Vec::new());
        Self { data: Data { fsm, config, item, can_merge: false, aside, then, frames } }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let Some(input) = self.input(&event) else { return };
        // `Err`: no row for this (state, input): the spec ignores the event.
        if let Ok(Some(action)) = self.data.fsm.consume(&input) {
            self.act(action, event, out);
        }
    }

    fn state(&self) -> StateName {
        *self.data.fsm.state()
    }
}
