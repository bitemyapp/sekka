//! `transfer` with statum 0.9.0: one `#[state]` per awaited response, each
//! with its own sealed payload type (statum keys `transition_with` by
//! payload type, so two targets of one state can't share one), the job's configuration and merge
//! frames as the `#[machine] Job`'s fields. The conflict sheet is two states
//! (only `AskingFolders` merges), a set-aside item is an `Aside` token that
//! only `PlacingReplacement` (after the new item is in place) can dispose
//! of and only `Restoring` can put back, and disposal reads only the
//! person's choice. Moving on to the next item ("next", which may finish
//! merges and the job) is one generic function over the edges each state
//! declares.

use crate::{SEAL, Seal};
use spec::Sink;
use spec::transfer::{
    AsideId, Config, Effect, Event, Found, ItemId, Machine, Mode, Name, Outcome, Placed, Problem, Resolution, StateName,
};
use statum::{Branch, CanTransitionWith, machine, state, transition};

pub const IMPLEMENTED: bool = true;

#[derive(Debug)]
struct Frame {
    folder: Option<ItemId>,
    next: u32,
    end: u32,
    complete: bool,
}

/// A replaced item set aside under a hidden name.
#[derive(Debug)]
pub struct Aside(AsideId);

tokens!(IdleTok, FinishedTok);

/// The payloads that hold only the awaited item.
macro_rules! item_data {
    ($($d:ident),*) => {$(
        #[derive(Debug)]
        pub struct $d {
            item: ItemId,
        }

        impl $d {
            pub fn item(&self) -> ItemId {
                self.item
            }
        }
    )*};
}
item_data!(ProbingData, AskingFilesData, AskingFoldersData, PlacingData, SettingAsideData, ListingData);

#[derive(Debug)]
pub struct PlacingReplacementData {
    item: ItemId,
    aside: Aside,
}

#[derive(Debug)]
pub struct DisposingData {
    item: ItemId,
    aside: Aside,
}

#[derive(Debug)]
pub struct RestoringData {
    item: ItemId,
    aside: Aside,
    cancel: bool,
}

#[state]
#[derive(Debug)]
pub enum TransferState {
    Idle(IdleTok),
    Probing(ProbingData),
    AskingFiles(AskingFilesData),
    AskingFolders(AskingFoldersData),
    Placing(PlacingData),
    SettingAside(SettingAsideData),
    PlacingReplacement(PlacingReplacementData),
    Disposing(DisposingData),
    Restoring(RestoringData),
    Listing(ListingData),
    Finished(FinishedTok),
}

/// The job: its configuration (not its kind) and the merge frames.
#[machine]
#[derive(Debug)]
pub struct Job<TransferState> {
    mode: Mode,
    delete_replaced: bool,
    frames: Vec<Frame>,
    seal: Seal,
}

/// The next item, or the end of the job.
pub type Next = ::statum::Branch<Job<Probing>, Job<Finished>>;

/// The next item, or the end of merges and of the job.
fn next<S: TransferStateTrait>(mut job: Job<S>, out: &mut impl Sink<Effect>) -> Next
where
    Job<S>:
        CanTransitionWith<ProbingData, Output = Job<Probing>> + CanTransitionWith<FinishedTok, Output = Job<Finished>>,
{
    while let Some(f) = job.frames.last_mut() {
        if f.next < f.end {
            let item = ItemId(f.next);
            f.next += 1;
            out.push(Effect::Probe { item });
            return Branch::First(job.transition_with_data(ProbingData { item }));
        }
        let f = job.frames.pop().expect("a frame");
        if let Some(folder) = f.folder {
            if job.mode == Mode::Move && f.complete {
                out.push(Effect::RemoveEmptySource { item: folder });
            }
            out.push(Effect::Outcome { item: folder, outcome: Outcome::Done });
        }
    }
    out.push(Effect::Finished { cancelled: false });
    Branch::Second(job.transition_with_data(FinishedTok(())))
}

fn done<S: TransferStateTrait>(mut job: Job<S>, item: ItemId, outcome: Outcome, out: &mut impl Sink<Effect>) -> Next
where
    Job<S>:
        CanTransitionWith<ProbingData, Output = Job<Probing>> + CanTransitionWith<FinishedTok, Output = Job<Finished>>,
{
    out.push(Effect::Outcome { item, outcome });
    if outcome != Outcome::Done
        && let Some(f) = job.frames.last_mut()
    {
        f.complete = false;
    }
    next(job, out)
}

fn cancel<S: TransferStateTrait>(mut job: Job<S>, out: &mut impl Sink<Effect>) -> Job<Finished>
where
    Job<S>: CanTransitionWith<FinishedTok, Output = Job<Finished>>,
{
    job.frames.clear();
    out.push(Effect::Finished { cancelled: true });
    job.transition_with_data(FinishedTok(()))
}

impl Job<Idle> {
    #[inline]
    pub fn new(config: Config) -> Self {
        Job::<Idle>::builder()
            .state_data(IdleTok(()))
            .mode(config.mode)
            .delete_replaced(config.delete_replaced)
            .frames(Vec::new())
            .seal(SEAL)
            .build()
    }
}

#[transition]
impl Job<Idle> {
    pub fn start(mut self, items: u32, out: &mut impl Sink<Effect>) -> Next {
        self.frames.push(Frame { folder: None, next: 0, end: items, complete: true });
        next(self, out)
    }
}

/// Where a probe leads (more targets than `Branch` holds).
pub enum Probed {
    Placing(Job<Placing>),
    AskingFiles(Job<AskingFiles>),
    AskingFolders(Job<AskingFolders>),
    Next(Next),
}

#[transition]
impl Job<Probing> {
    fn free(self, out: &mut impl Sink<Effect>) -> Job<Placing> {
        out.push(Effect::PlaceAt { item: self.state_data.item, name: Name::Target });
        self.transition_map(|ProbingData { item }| PlacingData { item })
    }
    fn ask(
        self,
        can_merge: bool,
        out: &mut impl Sink<Effect>,
    ) -> ::statum::Branch<Job<AskingFiles>, Job<AskingFolders>> {
        let item = self.state_data.item;
        out.push(Effect::Ask { item, can_merge });
        if can_merge {
            Branch::Second(self.transition_with(AskingFoldersData { item }))
        } else {
            Branch::First(self.transition_with(AskingFilesData { item }))
        }
    }
    fn skip(self, outcome: Outcome, out: &mut impl Sink<Effect>) -> Next {
        let item = self.state_data.item;
        done(self, item, outcome, out)
    }
}

impl Job<Probing> {
    pub fn probed(self, found: Found, out: &mut impl Sink<Effect>) -> Probed {
        match found {
            Found::Free => Probed::Placing(self.free(out)),
            Found::Same => Probed::Next(self.skip(Outcome::Done, out)),
            Found::Refused => Probed::Next(self.skip(Outcome::Failed, out)),
            Found::Conflict { can_merge } => match self.ask(can_merge, out) {
                Branch::First(s) => Probed::AskingFiles(s),
                Branch::Second(s) => Probed::AskingFolders(s),
            },
        }
    }
}

/// The answers both conflict sheets take, in the same block as the sheet's own.
macro_rules! asking {
    ($s:ident { $($own:tt)* }) => {
        #[transition]
        impl Job<$s> {
            pub fn skip(self, out: &mut impl Sink<Effect>) -> Next {
                let item = self.state_data.item;
                done(self, item, Outcome::Skipped, out)
            }
            pub fn cancel(self, out: &mut impl Sink<Effect>) -> Job<Finished> {
                cancel(self, out)
            }
            pub fn keep_both(self, out: &mut impl Sink<Effect>) -> Job<Placing> {
                let item = self.state_data.item;
                out.push(Effect::PlaceAt { item, name: Name::Free });
                self.transition_with(PlacingData { item })
            }
            pub fn replace(self, out: &mut impl Sink<Effect>) -> Job<SettingAside> {
                let item = self.state_data.item;
                out.push(Effect::SetAside { item });
                self.transition_with(SettingAsideData { item })
            }
            $($own)*
        }
    };
}

asking!(AskingFiles {});
asking!(AskingFolders {
    pub fn merge(self, out: &mut impl Sink<Effect>) -> Job<Listing> {
        let item = self.state_data.item;
        out.push(Effect::List { item });
        self.transition_with(ListingData { item })
    }
});

#[transition]
impl Job<Placing> {
    pub fn placed(self, outcome: Placed, out: &mut impl Sink<Effect>) -> Next {
        let item = self.state_data.item;
        match outcome {
            Placed::Ok => done(self, item, Outcome::Done, out),
            Placed::Skip => done(self, item, Outcome::Failed, out),
            Placed::Cancel => Branch::Second(cancel(self, out)),
        }
    }
}

#[transition]
impl Job<SettingAside> {
    pub fn set_aside(self, aside: AsideId, out: &mut impl Sink<Effect>) -> Job<PlacingReplacement> {
        let item = self.state_data.item;
        out.push(Effect::PlaceAt { item, name: Name::Target });
        self.transition_with(PlacingReplacementData { item, aside: Aside(aside) })
    }
    pub fn failed(self, out: &mut impl Sink<Effect>) -> Next {
        let item = self.state_data.item;
        done(self, item, Outcome::Failed, out)
    }
}

#[transition]
impl Job<PlacingReplacement> {
    /// The new item is in place: now the old one goes, as the person chose.
    pub fn placed(self, out: &mut impl Sink<Effect>) -> Job<Disposing> {
        let (item, aside) = (self.state_data.item, self.state_data.aside.0);
        out.push(if self.delete_replaced { Effect::Delete { item, aside } } else { Effect::Trash { item, aside } });
        self.transition_map(|PlacingReplacementData { item, aside }| DisposingData { item, aside })
    }
    /// It failed (skipped, or stopped): put the old one back.
    pub fn failed(self, cancel: bool, out: &mut impl Sink<Effect>) -> Job<Restoring> {
        out.push(Effect::RestoreAside { item: self.state_data.item, aside: self.state_data.aside.0 });
        self.transition_map(|PlacingReplacementData { item, aside }| RestoringData { item, aside, cancel })
    }
}

#[transition]
impl Job<Disposing> {
    pub fn disposed(self, ok: bool, out: &mut impl Sink<Effect>) -> Next {
        let DisposingData { item, aside: Aside(aside) } = self.state_data;
        if !ok {
            let problem = if self.delete_replaced { Problem::NotAllRemoved } else { Problem::KeptAside(aside) };
            out.push(Effect::Problem { item, problem });
        }
        done(self, item, Outcome::Done, out)
    }
}

#[transition]
impl Job<Restoring> {
    pub fn restored(self, ok: bool, out: &mut impl Sink<Effect>) -> Next {
        let RestoringData { item, aside: Aside(aside), cancel: then_cancel } = self.state_data;
        if !ok {
            out.push(Effect::Problem { item, problem: Problem::Stranded(aside) });
        }
        if then_cancel { Branch::Second(cancel(self, out)) } else { done(self, item, Outcome::Failed, out) }
    }
}

#[transition]
impl Job<Listing> {
    pub fn listed(mut self, first: ItemId, count: u32, out: &mut impl Sink<Effect>) -> Next {
        let folder = Some(self.state_data.item);
        self.frames.push(Frame { folder, next: first.0, end: first.0.saturating_add(count), complete: true });
        next(self, out)
    }
}

#[inline]
fn next_state(n: Next) -> job::SomeState {
    match n {
        Branch::First(s) => job::SomeState::Probing(s),
        Branch::Second(s) => job::SomeState::Finished(s),
    }
}

pub struct Transfer {
    /// `None` only while `handle` has moved the state out (statum's builder
    /// makes no cheap placeholder).
    state: Option<job::SomeState>,
}

impl Transfer {
    pub fn typed(&self) -> &job::SomeState {
        self.state.as_ref().expect("a state")
    }
}

impl Machine for Transfer {
    fn new(config: Config) -> Self {
        Self { state: Some(job::SomeState::Idle(Job::<Idle>::new(config))) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        use job::SomeState as S;
        let state = self.state.take().expect("a state");
        self.state = Some(match (state, event) {
            (S::Idle(s), Event::Start { items }) => next_state(s.start(items, out)),
            (S::Probing(s), Event::Probed { item, found }) if item == s.state_data.item => match s.probed(found, out) {
                Probed::Placing(s) => S::Placing(s),
                Probed::AskingFiles(s) => S::AskingFiles(s),
                Probed::AskingFolders(s) => S::AskingFolders(s),
                Probed::Next(n) => next_state(n),
            },
            (S::AskingFiles(s), Event::Answered { item, resolution }) if item == s.state_data.item => {
                match resolution {
                    Resolution::Skip => next_state(s.skip(out)),
                    Resolution::Cancel => S::Finished(s.cancel(out)),
                    Resolution::KeepBoth => S::Placing(s.keep_both(out)),
                    Resolution::Merge | Resolution::Replace => S::SettingAside(s.replace(out)),
                }
            }
            (S::AskingFolders(s), Event::Answered { item, resolution }) if item == s.state_data.item => {
                match resolution {
                    Resolution::Skip => next_state(s.skip(out)),
                    Resolution::Cancel => S::Finished(s.cancel(out)),
                    Resolution::KeepBoth => S::Placing(s.keep_both(out)),
                    Resolution::Merge => S::Listing(s.merge(out)),
                    Resolution::Replace => S::SettingAside(s.replace(out)),
                }
            }
            (S::SettingAside(s), Event::SetAside { item, aside }) if item == s.state_data.item => match aside {
                Some(a) => S::PlacingReplacement(s.set_aside(a, out)),
                None => next_state(s.failed(out)),
            },
            (S::Placing(s), Event::Placed { item, outcome }) if item == s.state_data.item => {
                next_state(s.placed(outcome, out))
            }
            (S::PlacingReplacement(s), Event::Placed { item, outcome }) if item == s.state_data.item => match outcome {
                Placed::Ok => S::Disposing(s.placed(out)),
                Placed::Skip => S::Restoring(s.failed(false, out)),
                Placed::Cancel => S::Restoring(s.failed(true, out)),
            },
            (S::Disposing(s), Event::Disposed { item, ok }) if item == s.state_data.item => {
                next_state(s.disposed(ok, out))
            }
            (S::Restoring(s), Event::Restored { item, ok }) if item == s.state_data.item => {
                next_state(s.restored(ok, out))
            }
            (S::Listing(s), Event::Listed { item, first, count }) if item == s.state_data.item => {
                next_state(s.listed(first, count, out))
            }
            (s, _) => s,
        });
    }

    fn state(&self) -> StateName {
        use job::SomeState as S;
        match self.typed() {
            S::Idle(_) => StateName::Idle,
            S::Probing(_) => StateName::Probing,
            S::AskingFiles(_) | S::AskingFolders(_) => StateName::Asking,
            S::Placing(_) => StateName::Placing,
            S::SettingAside(_) => StateName::SettingAside,
            S::PlacingReplacement(_) => StateName::PlacingReplacement,
            S::Disposing(_) => StateName::Disposing,
            S::Restoring(_) => StateName::Restoring,
            S::Listing(_) => StateName::Listing,
            S::Finished(_) => StateName::Finished,
        }
    }
}
