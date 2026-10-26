//! Running the plan: each operation in turn, through the backend, with the
//! confirmations the person gave on the Apply sheet.
//!
//! When the person applies, each destructive operation gets its
//! [`Confirm`] token made from the device as the sheet showed it; UDisks
//! checks it again just before the operation runs, so a device that
//! changed meanwhile (another stick in the same port, a partition moved)
//! is refused rather than overwritten. Operations made of several steps
//! (a resize: check, shrink the filesystem, move the partition's end, fill
//! it again) make the tokens for their later steps from the device as
//! their earlier steps left it, after checking it is the one they changed.
//!
//! Each step waits until UDisks reports its effect before the next starts,
//! so a new partition is made in space a deletion has freed, not in space
//! the cached state still thinks is taken. The first failure stops the
//! run: what didn't run stays pending, with the error beside it.

use std::time::{Duration, Instant};

use aizome_sys::Task;
use aizome_sys::disks::{
    BlockDevice, Confirm, DeletePartition, DisksState, Erase, Format, FormatSpec, NewPartitionTable, Repair, Resize,
};

use crate::backend::Backend;
use crate::fs;
use crate::layout::MIB;
use crate::plan::{Op, OpKind, Plan, Words};

/// How long a step waits for UDisks to report what it did.
const SETTLE: Duration = Duration::from_secs(15);

/// A confirmation the person gave, for one operation's first step.
enum Token {
    Table(Confirm<NewPartitionTable>),
    Delete(Confirm<DeletePartition>),
    Format(Confirm<Format>),
    Resize(Confirm<Resize>),
    Repair(Confirm<Repair>),
}

/// Where an operation stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Waiting,
    Running(String),
    Done,
    Failed(String),
}

pub struct Item {
    pub op: Op,
    pub words: Words,
    pub status: Status,
    token: Option<Token>,
    /// The device as it was agreed to.
    agreed: Option<BlockDevice>,
    /// What a step made (a new partition's path).
    made: Option<String>,
}

enum InFlight {
    Unit(Task<()>),
    Bool(Task<bool>),
    Path(Task<String>),
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
    /// Agree to `plan` as `state` shows it: words for each operation, and
    /// the confirmation tokens for those that destroy.
    pub fn agree(plan: &Plan, state: &DisksState) -> Run {
        let items = plan
            .ops
            .iter()
            .map(|op| {
                let agreed = state.block(&op.target).cloned();
                let token = agreed.as_ref().and_then(|b| match &op.kind {
                    OpKind::NewTable { .. } => Some(Token::Table(Confirm::new(b))),
                    OpKind::Delete => Some(Token::Delete(Confirm::new(b))),
                    OpKind::Format(_) => Some(Token::Format(Confirm::new(b))),
                    OpKind::Resize { .. } if fs::check_before_resize(&b.id_type) => {
                        Some(Token::Repair(Confirm::new(b)))
                    }
                    OpKind::Resize { .. } => Some(Token::Resize(Confirm::new(b))),
                    OpKind::Repair => Some(Token::Repair(Confirm::new(b))),
                    _ => None,
                });
                Item {
                    op: op.clone(),
                    words: plan.words(op, state),
                    status: Status::Waiting,
                    token,
                    agreed,
                    made: None,
                }
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
        self.items.iter().filter(|i| i.status == Status::Done).map(|i| i.op.id).collect()
    }

    /// The device the running operation works on (to find its UDisks job).
    pub fn working_on(&self) -> Option<&str> {
        let item = self.items.get(self.current)?;
        Some(item.made.as_deref().unwrap_or(&item.op.target))
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
                    InFlight::Path(t) => t.is_done(),
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
                    Some(InFlight::Path(t)) => match t.take().unwrap_or(Err(aizome_sys::Error::Cancelled)) {
                        Ok(path) => {
                            self.items[self.current].made = Some(path.clone());
                            self.after = Some(Expect::Present(path));
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

    /// The current operation's next step, started.
    fn next_step(&mut self, backend: &dyn Backend) -> Result<Step, String> {
        let state = backend.snapshot();
        let step = self.step;
        let item = &mut self.items[self.current];
        let target = item.op.target.clone();
        let unit = |t: Task<()>, e: Option<Expect>, what: &str| Ok(Step::Start(InFlight::Unit(t), e, what.to_owned()));
        match item.op.kind.clone() {
            OpKind::NewTable { kind } => match step {
                0 => {
                    let Some(Token::Table(c)) = item.token.take() else { return Err(no_token(&item.op)) };
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
                    let table = state
                        .block(&target)
                        .and_then(|b| b.partition_table.as_ref())
                        .map(|t| t.kind.clone())
                        .unwrap_or_default();
                    let (type_id, _) = p.flags.apply(&table, &p.fs, "", 0);
                    let format = (!p.fs.is_empty())
                        .then(|| FormatSpec { encrypt: p.encrypt.clone(), ..FormatSpec::new(&p.fs, &p.label) });
                    let task = backend.create_partition(&target, p.offset, p.size, Some(&type_id), &p.name, format);
                    Ok(Step::Start(InFlight::Path(task), None, "Making the partition…".into()))
                }
                1 => {
                    let made = item.made.clone().ok_or("The new partition didn't appear.")?;
                    let table = state
                        .block(&target)
                        .and_then(|b| b.partition_table.as_ref())
                        .map(|t| t.kind.clone())
                        .unwrap_or_default();
                    let (_, bits) = p.flags.apply(&table, &p.fs, "", 0);
                    if bits == 0 {
                        return Ok(Step::Finished);
                    }
                    unit(
                        backend.set_partition_flags(&made, bits),
                        Some(Expect::Partition { block: made.clone(), type_id: None, flags: Some(bits) }),
                        "Setting its flags…",
                    )
                }
                _ => Ok(Step::Finished),
            },
            OpKind::Delete => match step {
                0 => {
                    let Some(Token::Delete(c)) = item.token.take() else { return Err(no_token(&item.op)) };
                    unit(backend.delete_partition(c), Some(Expect::Gone(target)), "Deleting…")
                }
                _ => Ok(Step::Finished),
            },
            OpKind::Format(f) => match step {
                0 => {
                    let Some(Token::Format(c)) = item.token.take() else { return Err(no_token(&item.op)) };
                    let keep = item
                        .agreed
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
                    let old = item.agreed.as_ref().map(|b| b.id_uuid.clone()).unwrap_or_default();
                    let what = if f.erase == Erase::Quick { "Formatting…" } else { "Erasing, then formatting…" };
                    unit(backend.format(c, spec), Some(Expect::Fs { block: target, fs, old_uuid: old }), what)
                }
                _ => Ok(Step::Finished),
            },
            OpKind::Resize { from, to } => {
                let fs_type = item.agreed.as_ref().map(|b| b.id_type.clone()).unwrap_or_default();
                let has_fs = !fs_type.is_empty();
                // Later steps confirm the device as the earlier ones left it.
                let again = |state: &DisksState| -> Result<Confirm<Resize>, String> {
                    let b = state.block(&target).ok_or("The partition is gone.")?;
                    let agreed = item.agreed.as_ref().ok_or("The partition is gone.")?;
                    let same = b.device == agreed.device
                        && b.id_uuid == agreed.id_uuid
                        && b.partition.as_ref().map(|p| p.offset) == agreed.partition.as_ref().map(|p| p.offset);
                    if !same {
                        return Err(format!("{} changed while it was being resized: stopped.", agreed.device));
                    }
                    Ok(Confirm::new(b))
                };
                if to > from {
                    // Check ext before changing either size, then grow the
                    // partition and the filesystem into the added space.
                    let steps: &[&str] = if fs::check_before_resize(&fs_type) {
                        &["repair", "part", "fs"]
                    } else if has_fs {
                        &["part", "fs"]
                    } else {
                        &["part"]
                    };
                    match steps.get(step).copied() {
                        Some("repair") => {
                            let Some(Token::Repair(c)) = item.token.take() else { return Err(no_token(&item.op)) };
                            Ok(Step::Start(InFlight::Bool(backend.repair(c)), None, "Checking the filesystem…".into()))
                        }
                        Some("part") => {
                            let c = match item.token.take() {
                                Some(Token::Resize(c)) => c,
                                _ => again(&state)?,
                            };
                            unit(
                                backend.resize_partition(c, to),
                                Some(Expect::Size { block: target, size: to }),
                                "Moving the partition's end…",
                            )
                        }
                        Some("fs") => {
                            unit(backend.resize_filesystem(again(&state)?, 0), None, "Growing the filesystem…")
                        }
                        _ => Ok(Step::Finished),
                    }
                } else {
                    // Shrink: check (ext), the filesystem to a MiB under the
                    // new end, the partition, then the filesystem to fill it.
                    let check = fs::check_before_resize(&fs_type);
                    let steps: Vec<&str> = match (has_fs, check) {
                        (false, _) => vec!["part"],
                        (true, true) => vec!["repair", "fs", "part", "fill"],
                        (true, false) => vec!["fs", "part", "fill"],
                    };
                    match steps.get(step).copied() {
                        Some("repair") => {
                            let Some(Token::Repair(c)) = item.token.take() else { return Err(no_token(&item.op)) };
                            Ok(Step::Start(InFlight::Bool(backend.repair(c)), None, "Checking the filesystem…".into()))
                        }
                        Some("fs") => {
                            let c = match item.token.take() {
                                Some(Token::Resize(c)) => c,
                                _ => again(&state)?,
                            };
                            unit(
                                backend.resize_filesystem(c, to.saturating_sub(MIB)),
                                None,
                                "Shrinking the filesystem…",
                            )
                        }
                        Some("part") => {
                            let c = match item.token.take() {
                                Some(Token::Resize(c)) => c,
                                _ => again(&state)?,
                            };
                            unit(
                                backend.resize_partition(c, to),
                                Some(Expect::Size { block: target, size: to }),
                                "Moving the partition's end…",
                            )
                        }
                        Some("fill") => unit(
                            backend.resize_filesystem(again(&state)?, 0),
                            None,
                            "Fitting the filesystem to the partition…",
                        ),
                        _ => Ok(Step::Finished),
                    }
                }
            }
            OpKind::Label { label } => match step {
                0 => unit(
                    backend.set_label(&target, &label),
                    Some(Expect::Label { block: target.clone(), label: label.clone() }),
                    "Labelling…",
                ),
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
                if step >= 2 {
                    Ok(Step::Finished)
                } else if !type_id.eq_ignore_ascii_case(&p.type_id) {
                    unit(
                        backend.set_partition_type(&target, &type_id),
                        Some(Expect::Partition { block: target.clone(), type_id: Some(type_id.clone()), flags: None }),
                        "Changing the partition's type…",
                    )
                } else if bits != p.flags {
                    unit(
                        backend.set_partition_flags(&target, bits),
                        Some(Expect::Partition { block: target.clone(), type_id: None, flags: Some(bits) }),
                        "Setting the flags…",
                    )
                } else {
                    Ok(Step::Finished)
                }
            }
            OpKind::Repair => match step {
                0 => {
                    let Some(Token::Repair(c)) = item.token.take() else { return Err(no_token(&item.op)) };
                    Ok(Step::Start(InFlight::Bool(backend.repair(c)), None, "Checking and repairing…".into()))
                }
                _ => Ok(Step::Finished),
            },
        }
    }
}

fn no_token(op: &Op) -> String {
    format!("{} wasn't confirmed as it is now: review and apply again.", op.device)
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
