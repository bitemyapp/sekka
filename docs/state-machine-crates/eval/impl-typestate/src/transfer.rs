//! `transfer` as hand-written typestate. The conflict sheet is two types
//! (`AskingFiles`, `AskingFolders`: only folders merge); a set-aside item
//! is an `Aside` token that only `PlacingReplacement` (after the new item
//! is in place) can dispose of and only `Restoring` can put back; and
//! disposal reads only the person's choice. The merge frames live in `Job`.

use spec::Sink;
use spec::transfer::{
    AsideId, Config, Effect, Event, Found, ItemId, Machine, Mode, Name, Outcome, Placed, Problem, Resolution, StateName,
};

pub const IMPLEMENTED: bool = true;

struct Frame {
    folder: Option<ItemId>,
    next: u32,
    end: u32,
    complete: bool,
}

/// The job: its configuration and the merge frames.
pub struct Job {
    mode: Mode,
    delete_replaced: bool,
    frames: Vec<Frame>,
}

/// A replaced item set aside under a hidden name.
#[derive(Debug)]
pub struct Aside(AsideId);

#[derive(Debug)]
pub struct Idle(());
#[derive(Debug)]
pub struct Probing(ItemId);
#[derive(Debug)]
pub struct AskingFiles(ItemId);
#[derive(Debug)]
pub struct AskingFolders(ItemId);
#[derive(Debug)]
pub struct Placing(ItemId);
#[derive(Debug)]
pub struct SettingAside(ItemId);
#[derive(Debug)]
pub struct PlacingReplacement {
    item: ItemId,
    aside: Aside,
}
#[derive(Debug)]
pub struct Disposing {
    item: ItemId,
    aside: Aside,
}
#[derive(Debug)]
pub struct Restoring {
    item: ItemId,
    aside: Aside,
    cancel: bool,
}
#[derive(Debug)]
pub struct Listing(ItemId);
#[derive(Debug)]
pub struct Finished(());

/// The runtime wrapper.
#[derive(Debug)]
pub enum Any {
    Idle(Idle),
    Probing(Probing),
    AskingFiles(AskingFiles),
    AskingFolders(AskingFolders),
    Placing(Placing),
    SettingAside(SettingAside),
    PlacingReplacement(PlacingReplacement),
    Disposing(Disposing),
    Restoring(Restoring),
    Listing(Listing),
    Finished(Finished),
}

impl Job {
    /// The next item, or the end of merges and of the job.
    fn next(&mut self, out: &mut impl Sink<Effect>) -> Any {
        while let Some(f) = self.frames.last_mut() {
            if f.next < f.end {
                let item = ItemId(f.next);
                f.next += 1;
                out.push(Effect::Probe { item });
                return Any::Probing(Probing(item));
            }
            let f = self.frames.pop().expect("a frame");
            if let Some(folder) = f.folder {
                if self.mode == Mode::Move && f.complete {
                    out.push(Effect::RemoveEmptySource { item: folder });
                }
                out.push(Effect::Outcome { item: folder, outcome: Outcome::Done });
            }
        }
        out.push(Effect::Finished { cancelled: false });
        Any::Finished(Finished(()))
    }

    fn done(&mut self, item: ItemId, outcome: Outcome, out: &mut impl Sink<Effect>) -> Any {
        out.push(Effect::Outcome { item, outcome });
        if outcome != Outcome::Done
            && let Some(f) = self.frames.last_mut()
        {
            f.complete = false;
        }
        self.next(out)
    }

    fn cancel(&mut self, out: &mut impl Sink<Effect>) -> Finished {
        self.frames.clear();
        out.push(Effect::Finished { cancelled: true });
        Finished(())
    }
}

impl Idle {
    pub fn start(self, job: &mut Job, items: u32, out: &mut impl Sink<Effect>) -> Any {
        job.frames.push(Frame { folder: None, next: 0, end: items, complete: true });
        job.next(out)
    }
}

impl Probing {
    pub fn item(&self) -> ItemId {
        self.0
    }
    pub fn probed(self, job: &mut Job, found: Found, out: &mut impl Sink<Effect>) -> Any {
        let item = self.0;
        match found {
            Found::Free => {
                out.push(Effect::PlaceAt { item, name: Name::Target });
                Any::Placing(Placing(item))
            }
            Found::Same => job.done(item, Outcome::Done, out),
            Found::Refused => job.done(item, Outcome::Failed, out),
            Found::Conflict { can_merge: false } => {
                out.push(Effect::Ask { item, can_merge: false });
                Any::AskingFiles(AskingFiles(item))
            }
            Found::Conflict { can_merge: true } => {
                out.push(Effect::Ask { item, can_merge: true });
                Any::AskingFolders(AskingFolders(item))
            }
        }
    }
}

macro_rules! asking {
    ($t:ident) => {
        impl $t {
            pub fn item(&self) -> ItemId {
                self.0
            }
            pub fn skip(self, job: &mut Job, out: &mut impl Sink<Effect>) -> Any {
                job.done(self.0, Outcome::Skipped, out)
            }
            pub fn cancel(self, job: &mut Job, out: &mut impl Sink<Effect>) -> Finished {
                job.cancel(out)
            }
            pub fn keep_both(self, out: &mut impl Sink<Effect>) -> Placing {
                out.push(Effect::PlaceAt { item: self.0, name: Name::Free });
                Placing(self.0)
            }
            pub fn replace(self, out: &mut impl Sink<Effect>) -> SettingAside {
                out.push(Effect::SetAside { item: self.0 });
                SettingAside(self.0)
            }
        }
    };
}
asking!(AskingFiles);
asking!(AskingFolders);

impl AskingFolders {
    pub fn merge(self, out: &mut impl Sink<Effect>) -> Listing {
        out.push(Effect::List { item: self.0 });
        Listing(self.0)
    }
}

impl Placing {
    pub fn item(&self) -> ItemId {
        self.0
    }
    pub fn placed(self, job: &mut Job, outcome: Placed, out: &mut impl Sink<Effect>) -> Any {
        match outcome {
            Placed::Ok => job.done(self.0, Outcome::Done, out),
            Placed::Skip => job.done(self.0, Outcome::Failed, out),
            Placed::Cancel => Any::Finished(job.cancel(out)),
        }
    }
}

impl SettingAside {
    pub fn item(&self) -> ItemId {
        self.0
    }
    pub fn set_aside(self, aside: AsideId, out: &mut impl Sink<Effect>) -> PlacingReplacement {
        out.push(Effect::PlaceAt { item: self.0, name: Name::Target });
        PlacingReplacement { item: self.0, aside: Aside(aside) }
    }
    pub fn failed(self, job: &mut Job, out: &mut impl Sink<Effect>) -> Any {
        job.done(self.0, Outcome::Failed, out)
    }
}

impl PlacingReplacement {
    pub fn item(&self) -> ItemId {
        self.item
    }
    /// The new item is in place: now the old one goes, as the person chose.
    pub fn placed(self, job: &Job, out: &mut impl Sink<Effect>) -> Disposing {
        let (item, aside) = (self.item, self.aside.0);
        out.push(if job.delete_replaced { Effect::Delete { item, aside } } else { Effect::Trash { item, aside } });
        Disposing { item, aside: self.aside }
    }
    /// It failed (skipped, or stopped): put the old one back.
    pub fn failed(self, cancel: bool, out: &mut impl Sink<Effect>) -> Restoring {
        out.push(Effect::RestoreAside { item: self.item, aside: self.aside.0 });
        Restoring { item: self.item, aside: self.aside, cancel }
    }
}

impl Disposing {
    pub fn item(&self) -> ItemId {
        self.item
    }
    pub fn disposed(self, job: &mut Job, ok: bool, out: &mut impl Sink<Effect>) -> Any {
        if !ok {
            let problem = if job.delete_replaced { Problem::NotAllRemoved } else { Problem::KeptAside(self.aside.0) };
            out.push(Effect::Problem { item: self.item, problem });
        }
        job.done(self.item, Outcome::Done, out)
    }
}

impl Restoring {
    pub fn item(&self) -> ItemId {
        self.item
    }
    pub fn restored(self, job: &mut Job, ok: bool, out: &mut impl Sink<Effect>) -> Any {
        if !ok {
            out.push(Effect::Problem { item: self.item, problem: Problem::Stranded(self.aside.0) });
        }
        if self.cancel { Any::Finished(job.cancel(out)) } else { job.done(self.item, Outcome::Failed, out) }
    }
}

impl Listing {
    pub fn item(&self) -> ItemId {
        self.0
    }
    pub fn listed(self, job: &mut Job, first: ItemId, count: u32, out: &mut impl Sink<Effect>) -> Any {
        job.frames.push(Frame {
            folder: Some(self.0),
            next: first.0,
            end: first.0.saturating_add(count),
            complete: true,
        });
        job.next(out)
    }
}

pub struct Transfer {
    job: Job,
    state: Any,
}

impl Transfer {
    pub fn typed(&self) -> &Any {
        &self.state
    }
}

impl Machine for Transfer {
    fn new(config: Config) -> Self {
        Self {
            job: Job { mode: config.mode, delete_replaced: config.delete_replaced, frames: Vec::new() },
            state: Any::Idle(Idle(())),
        }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        use Any as A;
        let job = &mut self.job;
        let state = std::mem::replace(&mut self.state, A::Finished(Finished(())));
        self.state = match (state, event) {
            (A::Idle(s), Event::Start { items }) => s.start(job, items, out),
            (A::Probing(s), Event::Probed { item, found }) if item == s.item() => s.probed(job, found, out),
            (A::AskingFiles(s), Event::Answered { item, resolution }) if item == s.item() => match resolution {
                Resolution::Skip => s.skip(job, out),
                Resolution::Cancel => A::Finished(s.cancel(job, out)),
                Resolution::KeepBoth => A::Placing(s.keep_both(out)),
                Resolution::Merge | Resolution::Replace => A::SettingAside(s.replace(out)),
            },
            (A::AskingFolders(s), Event::Answered { item, resolution }) if item == s.item() => match resolution {
                Resolution::Skip => s.skip(job, out),
                Resolution::Cancel => A::Finished(s.cancel(job, out)),
                Resolution::KeepBoth => A::Placing(s.keep_both(out)),
                Resolution::Merge => A::Listing(s.merge(out)),
                Resolution::Replace => A::SettingAside(s.replace(out)),
            },
            (A::SettingAside(s), Event::SetAside { item, aside }) if item == s.item() => match aside {
                Some(a) => A::PlacingReplacement(s.set_aside(a, out)),
                None => s.failed(job, out),
            },
            (A::Placing(s), Event::Placed { item, outcome }) if item == s.item() => s.placed(job, outcome, out),
            (A::PlacingReplacement(s), Event::Placed { item, outcome }) if item == s.item() => match outcome {
                Placed::Ok => A::Disposing(s.placed(job, out)),
                Placed::Skip => A::Restoring(s.failed(false, out)),
                Placed::Cancel => A::Restoring(s.failed(true, out)),
            },
            (A::Disposing(s), Event::Disposed { item, ok }) if item == s.item() => s.disposed(job, ok, out),
            (A::Restoring(s), Event::Restored { item, ok }) if item == s.item() => s.restored(job, ok, out),
            (A::Listing(s), Event::Listed { item, first, count }) if item == s.item() => {
                s.listed(job, first, count, out)
            }
            (s, _) => s,
        };
    }

    fn state(&self) -> StateName {
        match self.state {
            Any::Idle(_) => StateName::Idle,
            Any::Probing(_) => StateName::Probing,
            Any::AskingFiles(_) | Any::AskingFolders(_) => StateName::Asking,
            Any::Placing(_) => StateName::Placing,
            Any::SettingAside(_) => StateName::SettingAside,
            Any::PlacingReplacement(_) => StateName::PlacingReplacement,
            Any::Disposing(_) => StateName::Disposing,
            Any::Restoring(_) => StateName::Restoring,
            Any::Listing(_) => StateName::Listing,
            Any::Finished(_) => StateName::Finished,
        }
    }
}
