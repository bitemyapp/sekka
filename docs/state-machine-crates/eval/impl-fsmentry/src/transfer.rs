//! `transfer` with fsmentry 0.4.0: one graph of the per-item states, each
//! carrying the item it awaits a response for (and the aside once there is
//! one); the merge frames are a stack beside it, as in the reference: every
//! level of a merge runs the same per-item graph, so there's nothing to nest.
//! Edges are named for their events. A conflict is `AskingFolders` or
//! `AskingFiles` (only folders have `merge`), and a placement `Placing` (at
//! the target, after `free`) or `PlacingCopy` (a free name, after
//! `keep_both`). The spec's **next** is a choice between `Probing` and
//! `Finished` made from the frames, so every state that finishes an item ends
//! in a two-armed `match` over its own handle.

use spec::Sink;
use spec::transfer::{
    AsideId, Config, Effect, Event, Found, ItemId, Machine, Mode, Name, Outcome, Placed, Problem, Resolution, StateName,
};

pub const IMPLEMENTED: bool = true;

/// An item whose target was set aside under `aside`.
#[derive(Debug, PartialEq, Eq)]
pub struct Replaced {
    item: ItemId,
    aside: AsideId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    Failed,
    Cancel,
}

/// Putting the aside back after the replacement failed, then `then`.
#[derive(Debug, PartialEq, Eq)]
pub struct Restore {
    item: ItemId,
    aside: AsideId,
    then: After,
}

impl Replaced {
    pub fn item(&self) -> ItemId {
        self.item
    }
    pub fn aside(&self) -> AsideId {
        self.aside
    }
}

impl Restore {
    pub fn item(&self) -> ItemId {
        self.item
    }
    pub fn aside(&self) -> AsideId {
        self.aside
    }
    pub fn then(&self) -> After {
        self.then
    }
}

pub mod job {
    use super::*;
    fsmentry::fsmentry! {
        #[derive(Debug, PartialEq, Eq)]
        pub enum Job {
            // What the probe found.
            Probing(ItemId) -free-> Placing(ItemId),
            Probing -folders-> AskingFolders(ItemId) -merge-> Listing(ItemId),
            Probing -files-> AskingFiles(ItemId),
            // The person's answers (Merge of files sets aside, as Replace does).
            AskingFolders & AskingFiles -keep_both-> PlacingCopy(ItemId),
            AskingFolders & AskingFiles -replace-> SettingAside(ItemId) -set_aside-> PlacingReplacement(Replaced),
            PlacingReplacement -placed-> Disposing(Replaced),
            PlacingReplacement -failed-> Restoring(Restore),
            // The aside's fate ends the item.
            Disposing -disposed_next-> Probing,
            Disposing -disposed_finish-> Finished,
            Restoring -restored_next-> Probing,
            Restoring -restored_finish-> Finished,
            // **next**: another item, or the end (also a cancel's).
            Idle & Probing & AskingFolders & AskingFiles & SettingAside & Placing & PlacingCopy & Listing -next-> Probing,
            Idle & Probing & AskingFolders & AskingFiles & SettingAside & Placing & PlacingCopy & Listing -finish-> Finished,
        }
    }
}

pub use job::{Job, JobEntry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Frame {
    folder: Option<ItemId>,
    next: u32,
    end: u32,
    complete: bool,
}

/// The merge frames, beside the graph.
struct Frames(Vec<Frame>);

impl Frames {
    /// **next**: `Some(item)` once its `Probe` is out, `None` once `Finished` is.
    fn next(&mut self, mode: Mode, out: &mut impl Sink<Effect>) -> Option<ItemId> {
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
        out.push(Effect::Finished { cancelled: false });
        None
    }

    /// **item done**, then **next**.
    fn done(&mut self, mode: Mode, item: ItemId, outcome: Outcome, out: &mut impl Sink<Effect>) -> Option<ItemId> {
        out.push(Effect::Outcome { item, outcome });
        if outcome != Outcome::Done
            && let Some(f) = self.0.last_mut()
        {
            f.complete = false;
        }
        self.next(mode, out)
    }

    fn cancel(&mut self, out: &mut impl Sink<Effect>) -> Option<ItemId> {
        self.0.clear();
        out.push(Effect::Finished { cancelled: true });
        None
    }

    /// A placement's result: the item done (or the job cancelled), then **next**.
    fn placed(&mut self, mode: Mode, item: ItemId, placed: Placed, out: &mut impl Sink<Effect>) -> Option<ItemId> {
        match placed {
            Placed::Ok => self.done(mode, item, Outcome::Done, out),
            Placed::Skip => self.done(mode, item, Outcome::Failed, out),
            Placed::Cancel => self.cancel(out),
        }
    }
}

pub struct Transfer {
    job: Job,
    config: Config,
    frames: Frames,
}

impl Transfer {
    pub fn job(&self) -> &Job {
        &self.job
    }

    /// fsmentry's transition API on the live machine: only the current
    /// state's legal edges (and none of `handle`'s effects).
    pub fn entry(&mut self) -> JobEntry<'_> {
        self.job.entry()
    }
}

impl Machine for Transfer {
    fn new(config: Config) -> Self {
        Self { job: Job::Idle, config, frames: Frames(Vec::new()) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let (frames, mode) = (&mut self.frames, self.config.mode);
        match (self.job.entry(), event) {
            (JobEntry::Idle(s), Event::Start { items }) => {
                frames.0.push(Frame { folder: None, next: 0, end: items, complete: true });
                match frames.next(mode, out) {
                    Some(i) => s.next(i),
                    None => s.finish(),
                }
            }
            (JobEntry::Probing(s), Event::Probed { item, found }) if *s.as_ref() == item => match found {
                Found::Free => {
                    out.push(Effect::PlaceAt { item, name: Name::Target });
                    s.free(item);
                }
                Found::Conflict { can_merge } => {
                    out.push(Effect::Ask { item, can_merge });
                    _ = if can_merge { s.folders(item) } else { s.files(item) };
                }
                Found::Same | Found::Refused => {
                    let outcome = if found == Found::Same { Outcome::Done } else { Outcome::Failed };
                    _ = match frames.done(mode, item, outcome, out) {
                        Some(i) => s.next(i),
                        None => s.finish(),
                    };
                }
            },
            (JobEntry::AskingFolders(s), Event::Answered { item, resolution }) if *s.as_ref() == item => {
                _ = match resolution {
                    Resolution::Skip => match frames.done(mode, item, Outcome::Skipped, out) {
                        Some(i) => s.next(i),
                        None => s.finish(),
                    },
                    Resolution::Cancel => {
                        frames.cancel(out);
                        s.finish()
                    }
                    Resolution::KeepBoth => {
                        out.push(Effect::PlaceAt { item, name: Name::Free });
                        s.keep_both(item)
                    }
                    Resolution::Merge => {
                        out.push(Effect::List { item });
                        s.merge(item)
                    }
                    Resolution::Replace => {
                        out.push(Effect::SetAside { item });
                        s.replace(item)
                    }
                };
            }
            (JobEntry::AskingFiles(s), Event::Answered { item, resolution }) if *s.as_ref() == item => {
                _ = match resolution {
                    Resolution::Skip => match frames.done(mode, item, Outcome::Skipped, out) {
                        Some(i) => s.next(i),
                        None => s.finish(),
                    },
                    Resolution::Cancel => {
                        frames.cancel(out);
                        s.finish()
                    }
                    Resolution::KeepBoth => {
                        out.push(Effect::PlaceAt { item, name: Name::Free });
                        s.keep_both(item)
                    }
                    Resolution::Merge | Resolution::Replace => {
                        out.push(Effect::SetAside { item });
                        s.replace(item)
                    }
                };
            }
            (JobEntry::SettingAside(s), Event::SetAside { item, aside }) if *s.as_ref() == item => {
                _ = match aside {
                    Some(aside) => {
                        out.push(Effect::PlaceAt { item, name: Name::Target });
                        s.set_aside(Replaced { item, aside })
                    }
                    None => match frames.done(mode, item, Outcome::Failed, out) {
                        Some(i) => s.next(i),
                        None => s.finish(),
                    },
                };
            }
            (JobEntry::Placing(s), Event::Placed { item, outcome }) if *s.as_ref() == item => {
                _ = match frames.placed(mode, item, outcome, out) {
                    Some(i) => s.next(i),
                    None => s.finish(),
                };
            }
            (JobEntry::PlacingCopy(s), Event::Placed { item, outcome }) if *s.as_ref() == item => {
                _ = match frames.placed(mode, item, outcome, out) {
                    Some(i) => s.next(i),
                    None => s.finish(),
                };
            }
            (JobEntry::PlacingReplacement(s), Event::Placed { item, outcome }) if s.as_ref().item == item => {
                let aside = s.as_ref().aside;
                _ = match outcome {
                    Placed::Ok => {
                        // **dispose**: the person's choice, never the job's kind.
                        out.push(if self.config.delete_replaced {
                            Effect::Delete { item, aside }
                        } else {
                            Effect::Trash { item, aside }
                        });
                        s.placed(Replaced { item, aside })
                    }
                    Placed::Skip | Placed::Cancel => {
                        out.push(Effect::RestoreAside { item, aside });
                        let then = if outcome == Placed::Skip { After::Failed } else { After::Cancel };
                        s.failed(Restore { item, aside, then })
                    }
                };
            }
            (JobEntry::Disposing(s), Event::Disposed { item, ok }) if s.as_ref().item == item => {
                if !ok {
                    let aside = s.as_ref().aside;
                    let problem =
                        if self.config.delete_replaced { Problem::NotAllRemoved } else { Problem::KeptAside(aside) };
                    out.push(Effect::Problem { item, problem });
                }
                _ = match frames.done(mode, item, Outcome::Done, out) {
                    Some(i) => s.disposed_next(i),
                    None => s.disposed_finish(),
                };
            }
            (JobEntry::Restoring(s), Event::Restored { item, ok }) if s.as_ref().item == item => {
                if !ok {
                    out.push(Effect::Problem { item, problem: Problem::Stranded(s.as_ref().aside) });
                }
                let next = match s.as_ref().then {
                    After::Failed => frames.done(mode, item, Outcome::Failed, out),
                    After::Cancel => frames.cancel(out),
                };
                _ = match next {
                    Some(i) => s.restored_next(i),
                    None => s.restored_finish(),
                };
            }
            (JobEntry::Listing(s), Event::Listed { item, first, count }) if *s.as_ref() == item => {
                let end = first.0.saturating_add(count);
                frames.0.push(Frame { folder: Some(item), next: first.0, end, complete: true });
                _ = match frames.next(mode, out) {
                    Some(i) => s.next(i),
                    None => s.finish(),
                };
            }
            _ => {}
        }
    }

    fn state(&self) -> StateName {
        match self.job {
            Job::Idle => StateName::Idle,
            Job::Probing(_) => StateName::Probing,
            Job::AskingFolders(_) | Job::AskingFiles(_) => StateName::Asking,
            Job::Placing(_) | Job::PlacingCopy(_) => StateName::Placing,
            Job::SettingAside(_) => StateName::SettingAside,
            Job::PlacingReplacement(_) => StateName::PlacingReplacement,
            Job::Disposing(_) => StateName::Disposing,
            Job::Restoring(_) => StateName::Restoring,
            Job::Listing(_) => StateName::Listing,
            Job::Finished => StateName::Finished,
        }
    }
}
