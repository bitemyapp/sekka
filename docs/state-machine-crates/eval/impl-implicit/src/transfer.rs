//! `transfer` in `Worker::place_top`'s shape (transfer.rs:355-446): the
//! locals `replace` and `aside`, the far-away `Worker.trash`, and the merge
//! recursion's `complete` flags, here as fields and a stack because the
//! machine waits for each step's result instead of blocking on it.

use crate::BUGS;
use spec::Sink;
use spec::transfer::{
    AsideId, Config, Effect, Event, Found, ItemId, JobKind, Machine, Mode, Name, Outcome, Placed, Problem, Resolution,
    StateName,
};

pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug)]
pub struct Level {
    pub folder: Option<ItemId>,
    pub next: u32,
    pub end: u32,
    pub complete: bool,
}

/// The machine's fields, in the current code's shape. Readable through
/// the machine (`Deref`); only `handle` changes them.
pub struct Fields {
    pub config: Config,
    /// `Worker.trash`: where replaced items go; `None`: deleted.
    pub trash: Option<()>,
    pub started: bool,
    pub finished: bool,
    pub stack: Vec<Level>,
    pub current: Option<ItemId>,
    /// The conflict sheet is up (`can_merge`).
    pub asked: Option<bool>,
    /// Replace was chosen: setting the target aside.
    pub replace: bool,
    pub aside: Option<AsideId>,
    pub placing: bool,
    pub disposing: bool,
    /// Renaming the aside back; `true`: then cancel.
    pub restoring: Option<bool>,
    pub listing: bool,
}

impl Fields {
    fn reset_item(&mut self) {
        self.current = None;
        self.asked = None;
        self.replace = false;
        self.aside = None;
        self.placing = false;
        self.disposing = false;
        self.restoring = None;
        self.listing = false;
    }

    fn probing(&self, i: ItemId) -> bool {
        self.current == Some(i)
            && self.asked.is_none()
            && !self.replace
            && !self.placing
            && !self.disposing
            && self.restoring.is_none()
            && !self.listing
    }

    fn next(&mut self, out: &mut impl Sink<Effect>) {
        self.reset_item();
        loop {
            let Some(level) = self.stack.last_mut() else {
                out.push(Effect::Finished { cancelled: false });
                self.finished = true;
                return;
            };
            if level.next < level.end {
                let item = ItemId(level.next);
                level.next += 1;
                self.current = Some(item);
                out.push(Effect::Probe { item });
                return;
            }
            let level = self.stack.pop().expect("a level");
            if let Some(folder) = level.folder {
                if self.config.mode == Mode::Move && level.complete {
                    out.push(Effect::RemoveEmptySource { item: folder });
                }
                out.push(Effect::Outcome { item: folder, outcome: Outcome::Done });
            }
        }
    }

    fn done(&mut self, item: ItemId, outcome: Outcome, out: &mut impl Sink<Effect>) {
        out.push(Effect::Outcome { item, outcome });
        if outcome != Outcome::Done
            && let Some(level) = self.stack.last_mut()
        {
            level.complete = false;
        }
        self.next(out);
    }

    fn cancel(&mut self, out: &mut impl Sink<Effect>) {
        self.reset_item();
        self.stack.clear();
        self.finished = true;
        out.push(Effect::Finished { cancelled: true });
    }
}

impl Fields {
    fn new(config: Config) -> Self {
        // BUG(HAKO-05): `restore` and `duplicate` build their Worker with
        // `trash: None`, so what they replace is deleted.
        let trash = if BUGS {
            (config.kind == JobKind::Transfer && !config.delete_replaced).then_some(())
        } else {
            (!config.delete_replaced).then_some(())
        };
        Self {
            config,
            trash,
            started: false,
            finished: false,
            stack: Vec::new(),
            current: None,
            asked: None,
            replace: false,
            aside: None,
            placing: false,
            disposing: false,
            restoring: None,
            listing: false,
        }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        if self.finished {
            return;
        }
        match event {
            Event::Start { items } => {
                if !self.started {
                    self.started = true;
                    self.stack.push(Level { folder: None, next: 0, end: items, complete: true });
                    self.next(out);
                }
            }
            Event::Probed { item, found } => {
                if !self.probing(item) {
                    return;
                }
                match found {
                    Found::Free => {
                        self.placing = true;
                        out.push(Effect::PlaceAt { item, name: Name::Target });
                    }
                    Found::Same => self.done(item, Outcome::Done, out),
                    Found::Refused => self.done(item, Outcome::Failed, out),
                    Found::Conflict { can_merge } => {
                        self.asked = Some(can_merge);
                        out.push(Effect::Ask { item, can_merge });
                    }
                }
            }
            Event::Answered { item, resolution } => {
                let Some(can_merge) = self.asked else { return };
                if self.current != Some(item) {
                    return;
                }
                self.asked = None;
                match resolution {
                    Resolution::Skip => self.done(item, Outcome::Skipped, out),
                    Resolution::Cancel => self.cancel(out),
                    Resolution::KeepBoth => {
                        self.placing = true;
                        out.push(Effect::PlaceAt { item, name: Name::Free });
                    }
                    Resolution::Merge if can_merge => {
                        self.listing = true;
                        out.push(Effect::List { item });
                    }
                    Resolution::Merge | Resolution::Replace => {
                        self.replace = true;
                        out.push(Effect::SetAside { item });
                    }
                }
            }
            Event::SetAside { item, aside } => {
                if !(self.replace && self.aside.is_none() && !self.placing && self.current == Some(item)) {
                    return;
                }
                match aside {
                    Some(a) => {
                        self.aside = Some(a);
                        self.placing = true;
                        out.push(Effect::PlaceAt { item, name: Name::Target });
                    }
                    None => self.done(item, Outcome::Failed, out),
                }
            }
            Event::Placed { item, outcome } => {
                if !(self.placing && self.current == Some(item)) {
                    return;
                }
                self.placing = false;
                match (self.aside, outcome) {
                    (None, Placed::Ok) => self.done(item, Outcome::Done, out),
                    (None, Placed::Skip) => self.done(item, Outcome::Failed, out),
                    (None, Placed::Cancel) => self.cancel(out),
                    (Some(aside), Placed::Ok) => {
                        self.disposing = true;
                        out.push(match self.trash {
                            Some(()) => Effect::Trash { item, aside },
                            None => Effect::Delete { item, aside },
                        });
                    }
                    (Some(aside), _) => {
                        self.restoring = Some(outcome == Placed::Cancel);
                        out.push(Effect::RestoreAside { item, aside });
                    }
                }
            }
            Event::Disposed { item, ok } => {
                if !(self.disposing && self.current == Some(item)) {
                    return;
                }
                if !ok {
                    // SPEC-FIX: a Trash refusal keeps the item aside (the
                    // current code falls back to deleting it).
                    let problem = match (self.trash, self.aside) {
                        (Some(()), Some(a)) => Problem::KeptAside(a),
                        _ => Problem::NotAllRemoved,
                    };
                    out.push(Effect::Problem { item, problem });
                }
                self.done(item, Outcome::Done, out);
            }
            Event::Restored { item, ok } => {
                let Some(then_cancel) = self.restoring else { return };
                if self.current != Some(item) {
                    return;
                }
                if !ok && let Some(a) = self.aside {
                    out.push(Effect::Problem { item, problem: Problem::Stranded(a) });
                }
                if then_cancel {
                    self.cancel(out);
                } else {
                    self.done(item, Outcome::Failed, out);
                }
            }
            Event::Listed { item, first, count } => {
                if !(self.listing && self.current == Some(item)) {
                    return;
                }
                self.stack.push(Level {
                    folder: Some(item),
                    next: first.0,
                    end: first.0.saturating_add(count),
                    complete: true,
                });
                self.next(out);
            }
        }
    }

    fn state(&self) -> StateName {
        if self.finished {
            StateName::Finished
        } else if !self.started {
            StateName::Idle
        } else if self.listing {
            StateName::Listing
        } else if self.restoring.is_some() {
            StateName::Restoring
        } else if self.disposing {
            StateName::Disposing
        } else if self.placing && self.aside.is_some() {
            StateName::PlacingReplacement
        } else if self.replace && self.aside.is_none() {
            StateName::SettingAside
        } else if self.placing {
            StateName::Placing
        } else if self.asked.is_some() {
            StateName::Asking
        } else {
            StateName::Probing
        }
    }
}

/// The machine: [`Fields`] read through `Deref`, never written from outside.
pub struct Transfer(Fields);

impl std::ops::Deref for Transfer {
    type Target = Fields;
    fn deref(&self) -> &Fields {
        &self.0
    }
}

impl Machine for Transfer {
    fn new(config: Config) -> Self {
        Transfer(Fields::new(config))
    }
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        self.0.handle(event, out);
    }
    fn state(&self) -> StateName {
        self.0.state()
    }
}
