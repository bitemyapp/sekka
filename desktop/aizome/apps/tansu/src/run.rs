//! Running the plan: each operation in turn, through the backend, with the
//! confirmations of what the person agreed to.
//!
//! As each step starts it gets its [`Confirm`] token
//! ([`plan::agreement`](crate::plan::agreement)), made from the person's
//! agreement to the operation as the Apply sheet showed it: the identity
//! the operation's device had when it was queued; UDisks's client checks it
//! again just before the step runs, so a device that changed meanwhile
//! (another stick in the same port, a partition moved) is refused rather
//! than changed. Each token is held to what the operation's steps keep (a
//! resize keeps the filesystem, not its size), and made when its step
//! starts, so an operation late in a long run isn't refused as too old.
//!
//! Each step waits until UDisks reports its effect before the next starts,
//! so a new partition is made in space a deletion has freed, not in space
//! the cached state still thinks is taken. The first failure stops the
//! run: what didn't run stays pending, with the error beside it.

use std::time::{Duration, Instant};

use aizome_sys::Task;
use aizome_sys::disks::{BlockDevice, DisksState, Erase, FormatSpec};

use crate::backend::Backend;
use crate::fs;
use crate::layout::MIB;
use crate::plan::{self, Agreement, Creating, Made, Op, OpKind, Shown, Words};
use aizome_kit::agree::Agreed;

/// How long a step waits for UDisks to report what it did.
const SETTLE: Duration = Duration::from_secs(15);

/// Where an operation stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Waiting,
    Running(String),
    Done,
    Failed(String),
}

pub struct Item {
    pub words: Words,
    pub status: Status,
    /// The person's agreement to the operation, as the Apply sheet showed
    /// it: each step's confirmation is made from it.
    agreed: Agreed<Shown>,
    /// The device's record as it was agreed to.
    block: Option<BlockDevice>,
    /// What a step made (a new partition).
    made: Option<Made>,
}

impl Item {
    /// The operation, as the person agreed to it.
    pub fn op(&self) -> &Op {
        self.agreed.target().op()
    }
}

enum InFlight {
    Unit(Task<()>),
    Bool(Task<bool>),
    Made(Creating),
}

/// What the next step waits to see.
enum Expect {
    Gone(String),
    Present(String),
    Table { disk: String, kind: String },
    Fs { block: String, fs: String, old_uuid: String },
    Size { block: String, size: u64 },
    Label { block: String, label: String },
    Partition { block: String, type_id: Option<String>, flags: Option<u64> },
}

impl Expect {
    fn met(&self, s: &DisksState) -> bool {
        match self {
            Expect::Gone(p) => s.block(p).is_none(),
            Expect::Present(p) => s.block(p).is_some(),
            Expect::Table { disk, kind } => {
                s.block(disk).and_then(|b| b.partition_table.as_ref()).is_some_and(|t| &t.kind == kind)
            }
            Expect::Fs { block, fs, old_uuid } => {
                s.block(block).is_some_and(|b| &b.id_type == fs && (old_uuid.is_empty() || &b.id_uuid != old_uuid))
            }
            Expect::Size { block, size } => {
                s.block(block).and_then(|b| b.partition.as_ref()).is_some_and(|p| p.size.abs_diff(*size) <= MIB)
            }
            Expect::Label { block, label } => s.block(block).is_some_and(|b| &b.id_label == label),
            Expect::Partition { block, type_id, flags } => {
                s.block(block).and_then(|b| b.partition.as_ref()).is_some_and(|p| {
                    type_id.as_ref().is_none_or(|t| p.type_id.eq_ignore_ascii_case(t))
                        && flags.is_none_or(|f| p.flags == f)
                })
            }
        }
    }
}

/// How the run ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Done,
    Failed { op: String, error: String },
    Stopped,
}

pub struct Run {
    pub items: Vec<Item>,
    pub current: usize,
    step: usize,
    inflight: Option<InFlight>,
    /// What the step in flight must show once it is done.
    after: Option<Expect>,
    expect: Option<(Expect, Instant)>,
    pub outcome: Option<Outcome>,
    /// Stop once the operation running now is done.
    pub stop_requested: bool,
    /// Held where it is (for pictures of a run under way).
    pub held: bool,
    pub started: Instant,
}

/// What a step does.
enum Step {
    Start(InFlight, Option<Expect>, String),
    /// Nothing more to do for this operation.
    Finished,
}

impl Run {
    /// Run what the person agreed to with the Apply sheet's button: its
    /// operations, as it showed them. Each step's confirmation is made when
    /// it starts, from that agreement: of the operation's device as it was
    /// queued (not whatever has its names now).
    ///
    /// Only the person's press makes the agreement, so the plan can't run as
    /// the sheet merely lists it:
    ///
    /// ```compile_fail,E0308
    /// use aizome_sys::disks::DisksState;
    /// use tansu::plan::Plan;
    /// use tansu::run::Run;
    /// fn apply_unasked(plan: &Plan, state: &DisksState) -> Run {
    ///     Run::agree(plan.shown(state), state)
    /// }
    /// ```
    ///
    /// nor as it once did, from the plan alone:
    ///
    /// ```compile_fail,E0308
    /// use aizome_sys::disks::DisksState;
    /// use tansu::plan::Plan;
    /// use tansu::run::Run;
    /// fn apply_unasked(plan: &Plan, state: &DisksState) -> Run {
    ///     Run::agree(plan, state)
    /// }
    /// ```
    ///
    /// One press runs once: the agreement is spent by the run it starts.
    ///
    /// ```compile_fail,E0382
    /// use aizome_kit::agree::Agreed;
    /// use aizome_sys::disks::DisksState;
    /// use tansu::plan::Shown;
    /// use tansu::run::Run;
    /// fn apply_twice(agreed: Agreed<Vec<Shown>>, state: &DisksState) -> (Run, Run) {
    ///     (Run::agree(agreed, state), Run::agree(agreed, state))
    /// }
    /// ```
    ///
    /// ```
    /// use aizome_kit::agree::Agreed;
    /// use aizome_sys::disks::DisksState;
    /// use tansu::plan::Shown;
    /// use tansu::run::Run;
    /// fn apply(agreed: Agreed<Vec<Shown>>, state: &DisksState) -> Run {
    ///     Run::agree(agreed, state)
    /// }
    /// ```
    pub fn agree(agreed: Agreed<Vec<Shown>>, state: &DisksState) -> Run {
        let items = agreed
            .split()
            .into_iter()
            .map(|agreed| Item {
                words: agreed.target().words().clone(),
                status: Status::Waiting,
                block: state.block(agreed.target().op().target()).cloned(),
                agreed,
                made: None,
            })
            .collect();
        Run {
            items,
            current: 0,
            step: 0,
            inflight: None,
            after: None,
            expect: None,
            outcome: None,
            stop_requested: false,
            held: false,
            started: Instant::now(),
        }
    }

    pub fn is_running(&self) -> bool {
        self.outcome.is_none()
    }

    /// The ids of the operations that ran to the end.
    pub fn done_ids(&self) -> Vec<u64> {
        self.items.iter().filter(|i| i.status == Status::Done).map(|i| i.op().id()).collect()
    }

    /// The device the running operation works on (to find its UDisks job).
    pub fn working_on(&self) -> Option<&str> {
        let item = self.items.get(self.current)?;
        Some(item.made.as_ref().map_or(item.op().target(), |m| m.path()))
    }

    /// Move the run along as far as it can go now: collect finished
    /// steps, wait for their effects, start the next. Whether anything
    /// changed.
    pub fn advance(&mut self, backend: &dyn Backend) -> bool {
        let mut changed = false;
        if self.held {
            return false;
        }
        for _ in 0..64 {
            if self.outcome.is_some() {
                return changed;
            }
            if let Some(f) = &self.inflight {
                let done = match f {
                    InFlight::Unit(t) => t.is_done(),
                    InFlight::Bool(t) => t.is_done(),
                    InFlight::Made(c) => c.is_done(),
                };
                if !done {
                    return changed;
                }
                let result: Result<(), String> = match self.inflight.take() {
                    Some(InFlight::Unit(t)) => t.take().unwrap_or(Ok(())).map_err(|e| e.to_string()),
                    Some(InFlight::Bool(t)) => match t.take().unwrap_or(Ok(true)) {
                        Ok(true) => Ok(()),
                        Ok(false) => Err("The filesystem still has errors the repair couldn't fix.".into()),
                        Err(e) => Err(e.to_string()),
                    },
                    Some(InFlight::Made(c)) => match c.take().unwrap_or(Err(aizome_sys::Error::Cancelled)) {
                        Ok(made) => {
                            self.after = Some(Expect::Present(made.path().to_owned()));
                            self.items[self.current].made = Some(made);
                            Ok(())
                        }
                        Err(e) => Err(e.to_string()),
                    },
                    None => Ok(()),
                };
                changed = true;
                if let Err(e) = result {
                    self.fail(e);
                    return changed;
                }
                // What the step did must show before the next one starts.
                self.expect = self.after.take().map(|e| (e, Instant::now()));
                self.step += 1;
            }
            if let Some((expect, since)) = &self.expect {
                let state = backend.snapshot();
                if !expect.met(&state) {
                    if since.elapsed() < SETTLE {
                        return changed;
                    }
                    log::warn!("tansu: UDisks didn't report the step's effect in {SETTLE:?}; going on");
                }
                self.expect = None;
            }
            match self.next_step(backend) {
                Ok(Step::Start(f, expect, what)) => {
                    self.items[self.current].status = Status::Running(what);
                    self.inflight = Some(f);
                    self.after = expect;
                }
                Ok(Step::Finished) => {
                    self.items[self.current].status = Status::Done;
                    self.current += 1;
                    self.step = 0;
                    if self.current == self.items.len() {
                        self.outcome = Some(Outcome::Done);
                    } else if self.stop_requested {
                        self.outcome = Some(Outcome::Stopped);
                    }
                }
                Err(e) => self.fail(e),
            }
            changed = true;
        }
        changed
    }

    fn fail(&mut self, error: String) {
        let item = &mut self.items[self.current];
        item.status = Status::Failed(error.clone());
        self.outcome = Some(Outcome::Failed { op: item.words.title.clone(), error });
        self.inflight = None;
        self.expect = None;
    }

    /// The current operation's next step, started, with its confirmation
    /// made now.
    fn next_step(&mut self, backend: &dyn Backend) -> Result<Step, String> {
        let state = backend.snapshot();
        let step = self.step;
        let item = &mut self.items[self.current];
        let target = item.op().target().to_owned();
        let unit = |t: Task<()>, e: Option<Expect>, what: &str| Ok(Step::Start(InFlight::Unit(t), e, what.to_owned()));
        let unconfirmed = || no_token(item.op());
        match item.op().kind().clone() {
            OpKind::NewTable { kind } => match step {
                0 => {
                    let Agreement::NewTable(c) = plan::agreement(&item.agreed) else { return Err(unconfirmed()) };
                    let k = if kind == aizome_sys::disks::TableKind::Gpt { "gpt" } else { "dos" };
                    unit(
                        backend.create_partition_table(c, kind),
                        Some(Expect::Table { disk: target, kind: k.into() }),
                        "Writing the partition table…",
                    )
                }
                _ => Ok(Step::Finished),
            },
            OpKind::Create(p) => match step {
                0 => {
                    let creating = plan::start_create(&item.agreed, backend, &state)?;
                    Ok(Step::Start(InFlight::Made(creating), None, "Making the partition…".into()))
                }
                1 => {
                    let made = item.made.as_ref().ok_or("The new partition didn't appear.")?;
                    let table = state
                        .block(&target)
                        .and_then(|b| b.partition_table.as_ref())
                        .map(|t| t.kind.clone())
                        .unwrap_or_default();
                    let (_, bits) = p.flags.apply(&table, &p.fs, "", 0);
                    if bits == 0 {
                        return Ok(Step::Finished);
                    }
                    // What the first step made, where it made it.
                    let c = plan::made_flags(&item.agreed, &state, made)
                        .ok_or("The new partition isn't where it was made.")?;
                    let block = made.path().to_owned();
                    unit(
                        backend.set_partition_flags(c, bits),
                        Some(Expect::Partition { block, type_id: None, flags: Some(bits) }),
                        "Setting its flags…",
                    )
                }
                _ => Ok(Step::Finished),
            },
            OpKind::Delete => match step {
                0 => {
                    let Agreement::Delete(c) = plan::agreement(&item.agreed) else { return Err(unconfirmed()) };
                    unit(backend.delete_partition(c), Some(Expect::Gone(target)), "Deleting…")
                }
                _ => Ok(Step::Finished),
            },
            OpKind::Format(f) => match step {
                0 => {
                    let Agreement::Format(c) = plan::agreement(&item.agreed) else { return Err(unconfirmed()) };
                    let keep = item
                        .block
                        .as_ref()
                        .and_then(|b| b.partition.as_ref())
                        .is_some_and(|p| crate::plan::keeps_type(&p.type_id, &f.fs));
                    let spec = FormatSpec {
                        erase: f.erase,
                        encrypt: f.encrypt.clone(),
                        update_partition_type: !keep,
                        ..FormatSpec::new(&f.fs, &f.label)
                    };
                    let fs = if f.encrypt.is_some() { "crypto_LUKS".to_owned() } else { f.fs.clone() };
                    let old = item.block.as_ref().map(|b| b.id_uuid.clone()).unwrap_or_default();
                    let what = if f.erase == Erase::Quick { "Formatting…" } else { "Erasing, then formatting…" };
                    unit(backend.format(c, spec), Some(Expect::Fs { block: target, fs, old_uuid: old }), what)
                }
                _ => Ok(Step::Finished),
            },
            OpKind::Resize { from, to } => {
                let fs_type = item.block.as_ref().map(|b| b.id_type.clone()).unwrap_or_default();
                let has_fs = !fs_type.is_empty();
                let check = fs::check_before_resize(&fs_type);
                // Every step, the check before it included, is held as the
                // resize is: to the partition's place and filesystem, and to
                // a size of `from` or `to`, between which the steps (or an
                // earlier run's) move it. At another size, another tool
                // resized it, and growing it from `from` could shrink it.
                let steps: &[&str] = match (to > from, has_fs, check) {
                    // Grow: check ext before changing either size, then the
                    // partition and the filesystem into the added space.
                    (true, true, true) => &["repair", "part", "fs"],
                    (true, true, false) => &["part", "fs"],
                    (true, false, _) => &["part"],
                    // Shrink: check (ext), the filesystem to a MiB under the
                    // new end, the partition, then the filesystem to fill it.
                    (false, true, true) => &["repair", "fs", "part", "fill"],
                    (false, true, false) => &["fs", "part", "fill"],
                    (false, false, _) => &["part"],
                };
                let Some(what) = steps.get(step).copied() else { return Ok(Step::Finished) };
                let Agreement::Resize(c) = plan::agreement(&item.agreed) else { return Err(unconfirmed()) };
                match what {
                    "repair" => Ok(Step::Start(
                        InFlight::Bool(backend.repair_before_resize(c)),
                        None,
                        "Checking the filesystem…".into(),
                    )),
                    "part" => unit(
                        backend.resize_partition(c, to),
                        Some(Expect::Size { block: target, size: to }),
                        "Moving the partition's end…",
                    ),
                    "fs" if to > from => unit(backend.resize_filesystem(c, 0), None, "Growing the filesystem…"),
                    "fs" => {
                        unit(backend.resize_filesystem(c, to.saturating_sub(MIB)), None, "Shrinking the filesystem…")
                    }
                    _ => unit(backend.resize_filesystem(c, 0), None, "Fitting the filesystem to the partition…"),
                }
            }
            OpKind::Label { label } => match step {
                0 => {
                    let Agreement::Label(c) = plan::agreement(&item.agreed) else { return Err(unconfirmed()) };
                    unit(
                        backend.set_label(c, &label),
                        Some(Expect::Label { block: target.clone(), label: label.clone() }),
                        "Labelling…",
                    )
                }
                _ => Ok(Step::Finished),
            },
            OpKind::Flags { to, .. } => {
                let b = state.block(&target).ok_or("The partition is gone.")?;
                let p = b.partition.as_ref().ok_or("It isn't a partition.")?;
                let table = state
                    .block(&p.table)
                    .and_then(|t| t.partition_table.as_ref())
                    .map(|t| t.kind.clone())
                    .unwrap_or_default();
                // Each step does what's still to do: the type, then the bits.
                let (type_id, bits) = to.apply(&table, &b.id_type, &p.type_id, p.flags);
                if step >= 2 || (type_id.eq_ignore_ascii_case(&p.type_id) && bits == p.flags) {
                    return Ok(Step::Finished);
                }
                let Agreement::Flags(c) = plan::agreement(&item.agreed) else { return Err(unconfirmed()) };
                if !type_id.eq_ignore_ascii_case(&p.type_id) {
                    unit(
                        backend.set_partition_type(c, &type_id),
                        Some(Expect::Partition { block: target.clone(), type_id: Some(type_id.clone()), flags: None }),
                        "Changing the partition's type…",
                    )
                } else {
                    unit(
                        backend.set_partition_flags(c, bits),
                        Some(Expect::Partition { block: target.clone(), type_id: None, flags: Some(bits) }),
                        "Setting the flags…",
                    )
                }
            }
            OpKind::Repair => match step {
                0 => {
                    let Agreement::Repair(c) = plan::agreement(&item.agreed) else { return Err(unconfirmed()) };
                    Ok(Step::Start(InFlight::Bool(backend.repair(c)), None, "Checking and repairing…".into()))
                }
                _ => Ok(Step::Finished),
            },
        }
    }
}

fn no_token(op: &Op) -> String {
    format!("{} wasn't confirmed as it is now: review and apply again.", op.device())
}

impl Run {
    /// Show the run as if the operation at `current` were doing `what`
    /// (those before it done), and hold it there: for pictures.
    pub fn picture(&mut self, current: usize, what: &str) {
        for (i, item) in self.items.iter_mut().enumerate() {
            item.status = match i.cmp(&current) {
                std::cmp::Ordering::Less => Status::Done,
                std::cmp::Ordering::Equal => Status::Running(what.to_owned()),
                std::cmp::Ordering::Greater => Status::Waiting,
            };
        }
        self.current = current;
        self.held = true;
    }
}
