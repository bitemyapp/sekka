//! `transfer` as an enum: one variant per awaited response, carrying the
//! item (and the aside, once there is one); merges push frames.

use spec::Sink;
use spec::transfer::{
    AsideId, Config, Effect, Event, Found, ItemId, Machine, Mode, Name, Outcome, Placed, Problem, Resolution, StateName,
};

pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    Failed,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Probing { item: ItemId },
    Asking { item: ItemId, can_merge: bool },
    Placing { item: ItemId },
    SettingAside { item: ItemId },
    PlacingReplacement { item: ItemId, aside: AsideId },
    Disposing { item: ItemId, aside: AsideId },
    Restoring { item: ItemId, aside: AsideId, then: After },
    Listing { item: ItemId },
    Finished,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub folder: Option<ItemId>,
    pub next: u32,
    pub end: u32,
    pub complete: bool,
}

/// The machine. Its fields are private: the state changes only through
/// `Machine::handle`; [`Transfer::typed`] reads it.
pub struct Transfer {
    config: Config,
    state: State,
    frames: Vec<Frame>,
}

impl Transfer {
    /// The current state, read-only.
    pub fn typed(&self) -> &State {
        &self.state
    }
}

impl Transfer {
    fn next(&mut self, out: &mut impl Sink<Effect>) {
        while let Some(f) = self.frames.last_mut() {
            if f.next < f.end {
                let item = ItemId(f.next);
                f.next += 1;
                out.push(Effect::Probe { item });
                self.state = State::Probing { item };
                return;
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
        self.state = State::Finished;
    }

    fn done(&mut self, item: ItemId, outcome: Outcome, out: &mut impl Sink<Effect>) {
        out.push(Effect::Outcome { item, outcome });
        if outcome != Outcome::Done
            && let Some(f) = self.frames.last_mut()
        {
            f.complete = false;
        }
        self.next(out);
    }

    fn cancel(&mut self, out: &mut impl Sink<Effect>) {
        self.frames.clear();
        out.push(Effect::Finished { cancelled: true });
        self.state = State::Finished;
    }
}

impl Machine for Transfer {
    fn new(config: Config) -> Self {
        Self { config, state: State::Idle, frames: Vec::new() }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        match (self.state, event) {
            (State::Idle, Event::Start { items }) => {
                self.frames.push(Frame { folder: None, next: 0, end: items, complete: true });
                self.next(out);
            }
            (State::Probing { item }, Event::Probed { item: i, found }) if i == item => match found {
                Found::Free => {
                    out.push(Effect::PlaceAt { item, name: Name::Target });
                    self.state = State::Placing { item };
                }
                Found::Same => self.done(item, Outcome::Done, out),
                Found::Refused => self.done(item, Outcome::Failed, out),
                Found::Conflict { can_merge } => {
                    out.push(Effect::Ask { item, can_merge });
                    self.state = State::Asking { item, can_merge };
                }
            },
            (State::Asking { item, can_merge }, Event::Answered { item: i, resolution }) if i == item => {
                match resolution {
                    Resolution::Skip => self.done(item, Outcome::Skipped, out),
                    Resolution::Cancel => self.cancel(out),
                    Resolution::KeepBoth => {
                        out.push(Effect::PlaceAt { item, name: Name::Free });
                        self.state = State::Placing { item };
                    }
                    Resolution::Merge if can_merge => {
                        out.push(Effect::List { item });
                        self.state = State::Listing { item };
                    }
                    Resolution::Merge | Resolution::Replace => {
                        out.push(Effect::SetAside { item });
                        self.state = State::SettingAside { item };
                    }
                }
            }
            (State::SettingAside { item }, Event::SetAside { item: i, aside }) if i == item => match aside {
                Some(aside) => {
                    out.push(Effect::PlaceAt { item, name: Name::Target });
                    self.state = State::PlacingReplacement { item, aside };
                }
                None => self.done(item, Outcome::Failed, out),
            },
            (State::Placing { item }, Event::Placed { item: i, outcome }) if i == item => match outcome {
                Placed::Ok => self.done(item, Outcome::Done, out),
                Placed::Skip => self.done(item, Outcome::Failed, out),
                Placed::Cancel => self.cancel(out),
            },
            (State::PlacingReplacement { item, aside }, Event::Placed { item: i, outcome }) if i == item => {
                match outcome {
                    Placed::Ok => {
                        out.push(if self.config.delete_replaced {
                            Effect::Delete { item, aside }
                        } else {
                            Effect::Trash { item, aside }
                        });
                        self.state = State::Disposing { item, aside };
                    }
                    Placed::Skip | Placed::Cancel => {
                        out.push(Effect::RestoreAside { item, aside });
                        let then = if outcome == Placed::Skip { After::Failed } else { After::Cancel };
                        self.state = State::Restoring { item, aside, then };
                    }
                }
            }
            (State::Disposing { item, aside }, Event::Disposed { item: i, ok }) if i == item => {
                if !ok {
                    let problem =
                        if self.config.delete_replaced { Problem::NotAllRemoved } else { Problem::KeptAside(aside) };
                    out.push(Effect::Problem { item, problem });
                }
                self.done(item, Outcome::Done, out);
            }
            (State::Restoring { item, aside, then }, Event::Restored { item: i, ok }) if i == item => {
                if !ok {
                    out.push(Effect::Problem { item, problem: Problem::Stranded(aside) });
                }
                match then {
                    After::Failed => self.done(item, Outcome::Failed, out),
                    After::Cancel => self.cancel(out),
                }
            }
            (State::Listing { item }, Event::Listed { item: i, first, count }) if i == item => {
                self.frames.push(Frame {
                    folder: Some(item),
                    next: first.0,
                    end: first.0.saturating_add(count),
                    complete: true,
                });
                self.next(out);
            }
            _ => {}
        }
    }

    fn state(&self) -> StateName {
        match self.state {
            State::Idle => StateName::Idle,
            State::Probing { .. } => StateName::Probing,
            State::Asking { .. } => StateName::Asking,
            State::Placing { .. } => StateName::Placing,
            State::SettingAside { .. } => StateName::SettingAside,
            State::PlacingReplacement { .. } => StateName::PlacingReplacement,
            State::Disposing { .. } => StateName::Disposing,
            State::Restoring { .. } => StateName::Restoring,
            State::Listing { .. } => StateName::Listing,
            State::Finished => StateName::Finished,
        }
    }
}
