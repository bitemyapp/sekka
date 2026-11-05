//! Pending operations: what the person asked for, checked against the
//! disk as it will be by then, merged where one supersedes another, shown
//! in the map before they run, and described in plain words.
//!
//! Nothing here touches a disk. Only [`crate::run`] does, after the Apply
//! sheet's confirmation.
//!
//! The rules that keep a plan simple to run safely:
//!
//! - A partition a pending operation will make is changed by editing that
//!   operation (its filesystem, label, size, flags) or undone by deleting
//!   it, so no operation ever names a device that doesn't exist yet.
//! - A device has at most one pending operation that rewrites it (format,
//!   resize, repair, delete); a label or flags may join a format. Deleting
//!   drops the others; formatting again replaces the format.
//! - Nothing in use (mounted, active swap, unlocked) is changed, and a
//!   new partition table drops what was pending for the old partitions.

// Every arm names what it matches: a variant added later is a compile
// error here, not a case quietly taken by `_`.
#![deny(clippy::wildcard_enum_match_arm)]

use aizome_sys::Task;
use aizome_sys::disks::{
    Change, Confirm, CreatePartition, DeletePartition, DisksState, Encryption, Erase, Format, FormatSpec, Holds,
    Identity, NewPartitionTable, PartitionEntry, Relabel, Repair, Resize, TableKind, format_size,
};

use crate::backend::Backend;
use crate::fs;
use crate::layout::{self, Flags, Here, Layout, MIB, Mark, Seg, SegKind, Target};

/// What an operation does.
#[derive(Clone, Debug, PartialEq)]
pub enum OpKind {
    NewTable { kind: TableKind },
    Create(NewPartition),
    Delete,
    Format(NewFs),
    Resize { from: u64, to: u64 },
    Label { label: String },
    Flags { from: Flags, to: Flags },
    Repair,
}

/// A partition to make.
#[derive(Clone, Debug, PartialEq)]
pub struct NewPartition {
    pub offset: u64,
    pub size: u64,
    /// The filesystem, or empty for none.
    pub fs: String,
    pub label: String,
    /// GPT's partition name.
    pub name: String,
    pub encrypt: Option<Encryption>,
    pub flags: Flags,
    /// Inside an extended partition.
    pub logical: bool,
}

/// A filesystem to make on a device that exists.
#[derive(Clone, Debug, PartialEq)]
pub struct NewFs {
    pub fs: String,
    pub label: String,
    pub erase: Erase,
    pub encrypt: Option<Encryption>,
}

/// One pending operation. Its fields are read-only outside this module:
/// what it does and what it acts on decide what the run confirms.
#[derive(Clone, Debug, PartialEq)]
pub struct Op {
    id: u64,
    /// The disk's block device (object path) and its node (`/dev/sdb`).
    disk: String,
    disk_device: String,
    /// The device acted on: a partition, or the disk itself (a new table,
    /// a new partition).
    target: String,
    device: String,
    kind: OpKind,
    /// The device acted on, as it was when the operation was queued: its
    /// confirmations hold the device to this, so it never follows another
    /// device to the same names.
    seen: Identity,
}

impl Op {
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The disk's block device (object path).
    pub fn disk(&self) -> &str {
        &self.disk
    }

    /// The disk's node (`/dev/sdb`).
    pub fn disk_device(&self) -> &str {
        &self.disk_device
    }

    /// The device acted on (object path): a partition, or the disk itself
    /// (a new table, a new partition).
    pub fn target(&self) -> &str {
        &self.target
    }

    /// The device acted on's node (`/dev/sdb1`).
    pub fn device(&self) -> &str {
        &self.device
    }

    pub fn kind(&self) -> &OpKind {
        &self.kind
    }
}

/// The brand of a confirmation of a pending operation's device
/// ([`Op::agreement`]): only this module makes one, from the device as the
/// operation was queued on it.
#[derive(Debug)]
pub struct Queued(());

/// The confirmation for a step of a pending operation, of its device as it
/// was queued, of the one kind of change the operation's kind makes. The
/// backend takes only confirmations branded [`Queued`].
#[derive(Debug)]
pub enum Agreement {
    NewTable(Confirm<NewPartitionTable, Queued>),
    Create(Confirm<CreatePartition, Queued>),
    Delete(Confirm<DeletePartition, Queued>),
    Format(Confirm<Format, Queued>),
    /// Every step of a resize, the check before it included.
    Resize(Confirm<Resize, Queued>),
    Label(Confirm<Relabel, Queued>),
    Flags(Confirm<PartitionEntry, Queued>),
    Repair(Confirm<Repair, Queued>),
}

impl OpKind {
    /// What the operation holds its device to, from queueing it to running
    /// it: what the confirmations of its steps hold it to ([`Op::agreement`]).
    pub fn holds(&self) -> Holds {
        match self {
            OpKind::NewTable { .. } => NewPartitionTable::HOLDS,
            OpKind::Create(_) => CreatePartition::HOLDS,
            OpKind::Delete => DeletePartition::HOLDS,
            OpKind::Format(_) => Format::HOLDS,
            OpKind::Resize { .. } => Resize::HOLDS,
            OpKind::Label { .. } => Relabel::HOLDS,
            OpKind::Flags { .. } => PartitionEntry::HOLDS,
            OpKind::Repair => Repair::HOLDS,
        }
    }
}

/// A new partition being made by a [`OpKind::Create`]'s first step: only
/// [`Op::start_create`] starts one.
pub struct Creating {
    op: u64,
    task: Task<String>,
}

impl Creating {
    pub fn is_done(&self) -> bool {
        self.task.is_done()
    }

    /// Once done: the partition it made, or why it made none.
    pub fn take(&self) -> Option<Result<Made, aizome_sys::Error>> {
        self.task.take().map(|r| r.map(|path| Made { op: self.op, path }))
    }
}

/// A partition an operation's first step made, for its later steps (its
/// flags): only [`Creating::take`] gives one.
#[derive(Clone, Debug, PartialEq)]
pub struct Made {
    op: u64,
    path: String,
}

impl Made {
    /// Its block device's object path.
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl Op {
    /// The device acted on, as it was when the operation was queued.
    pub fn seen(&self) -> &Identity {
        &self.seen
    }

    /// Whether the device it acts on is still the one it was queued for.
    pub fn still_there(&self, state: &DisksState) -> bool {
        state.identity(&self.target).is_some_and(|now| self.seen.same(&now, self.kind.holds()))
    }

    /// The confirmation for a step of it, made now, of its device as it was
    /// queued, of the one kind of change this operation's kind makes. The
    /// run makes one as each step starts, so an operation late in a long run
    /// isn't refused as too old.
    ///
    /// The backend takes only confirmations branded [`Queued`], which only
    /// this module makes, so the run can't confirm whatever has the device's
    /// names when the person applies (TSH-25: a stick swapped for another of
    /// the same model got the first one's format):
    ///
    /// ```compile_fail,E0423
    /// use aizome_sys::disks::{Confirm, DisksState, Format};
    /// use tansu::plan::{Op, Queued};
    /// fn confirm_what_is_there(state: &DisksState, op: &Op) -> Confirm<Format, Queued> {
    ///     Confirm::new(state.identity(&op.target).unwrap(), Queued(()))
    /// }
    /// ```
    ///
    /// A label's review can't become a format's confirmation, neither as the
    /// review once was:
    ///
    /// ```compile_fail,E0599
    /// use aizome_sys::disks::{Confirm, Format, FormatSpec};
    /// use tansu::backend::Backend;
    /// use tansu::plan::Op;
    /// fn f(backend: &dyn Backend, label_op: &Op) {
    ///     let _ = backend.format(Confirm::<Format, _>::new(label_op.review()), FormatSpec::new("ext4", ""));
    /// }
    /// ```
    ///
    /// nor as the agreement is:
    ///
    /// ```compile_fail,E0308
    /// use aizome_sys::disks::FormatSpec;
    /// use tansu::backend::Backend;
    /// use tansu::plan::Op;
    /// fn f(backend: &dyn Backend, label_op: &Op) {
    ///     let _ = backend.format(label_op.agreement(), FormatSpec::new("ext4", ""));
    /// }
    /// ```
    ///
    /// ```
    /// use aizome_sys::disks::FormatSpec;
    /// use tansu::backend::Backend;
    /// use tansu::plan::{Agreement, Op};
    /// fn format(backend: &dyn Backend, op: &Op) {
    ///     if let Agreement::Format(c) = op.agreement() {
    ///         let _ = backend.format(c, FormatSpec::new("ext4", ""));
    ///     }
    /// }
    /// ```
    pub fn agreement(&self) -> Agreement {
        let seen = || self.seen.clone();
        match self.kind {
            OpKind::NewTable { .. } => Agreement::NewTable(Confirm::new(seen(), Queued(()))),
            OpKind::Create(_) => Agreement::Create(Confirm::new(seen(), Queued(()))),
            OpKind::Delete => Agreement::Delete(Confirm::new(seen(), Queued(()))),
            OpKind::Format(_) => Agreement::Format(Confirm::new(seen(), Queued(()))),
            OpKind::Resize { .. } => Agreement::Resize(Confirm::new(seen(), Queued(()))),
            OpKind::Label { .. } => Agreement::Label(Confirm::new(seen(), Queued(()))),
            OpKind::Flags { .. } => Agreement::Flags(Confirm::new(seen(), Queued(()))),
            OpKind::Repair => Agreement::Repair(Confirm::new(seen(), Queued(()))),
        }
    }

    /// Starts making the new partition of a [`OpKind::Create`], on its disk
    /// as it was queued.
    pub fn start_create(&self, backend: &dyn Backend, state: &DisksState) -> Result<Creating, String> {
        let OpKind::Create(p) = &self.kind else { return Err("It doesn't make a partition.".into()) };
        let table = state
            .block(&self.target)
            .and_then(|b| b.partition_table.as_ref())
            .map(|t| t.kind.clone())
            .unwrap_or_default();
        let (type_id, _) = p.flags.apply(&table, &p.fs, "", 0);
        let format =
            (!p.fs.is_empty()).then(|| FormatSpec { encrypt: p.encrypt.clone(), ..FormatSpec::new(&p.fs, &p.label) });
        let confirm = Confirm::new(self.seen.clone(), Queued(()));
        let task = backend.create_partition(confirm, p.offset, p.size, Some(&type_id), &p.name, format);
        Ok(Creating { op: self.id, task })
    }

    /// The confirmation for setting the flags of the partition this
    /// operation made. No one saw it before it was made, so it is taken as
    /// it is, if it is where this operation made it: in the space it asked
    /// for, on its disk, which is still the one it was queued for. Only
    /// what the create step gave can name it, not any partition:
    ///
    /// ```compile_fail,E0599
    /// use aizome_sys::disks::{Confirm, DeletePartition, DisksState};
    /// use tansu::backend::Backend;
    /// use tansu::plan::Op;
    /// fn f(backend: &dyn Backend, state: &DisksState, create_op: &Op, other_partition: &str) {
    ///     let c: Confirm<DeletePartition, _> = Confirm::new(create_op.review_made(state, other_partition).unwrap());
    ///     let _ = backend.delete_partition(c);
    /// }
    /// ```
    ///
    /// ```compile_fail,E0308
    /// use aizome_sys::disks::DisksState;
    /// use tansu::backend::Backend;
    /// use tansu::plan::Op;
    /// fn f(backend: &dyn Backend, state: &DisksState, create_op: &Op, other_partition: &str) {
    ///     let c = create_op.made_flags(state, other_partition).unwrap();
    ///     let _ = backend.set_partition_flags(c, 0x80);
    /// }
    /// ```
    ///
    /// ```compile_fail,E0451
    /// use tansu::plan::Made;
    /// let _ = Made { op: 1, path: "/org/freedesktop/UDisks2/block_devices/sda1".into() };
    /// ```
    pub fn made_flags(&self, state: &DisksState, made: &Made) -> Option<Confirm<PartitionEntry, Queued>> {
        let OpKind::Create(p) = &self.kind else { return None };
        if made.op != self.id {
            return None;
        }
        let part = state.block(&made.path)?.partition.as_ref()?;
        let in_place = part.table == self.target && part.offset >= p.offset && part.offset < p.offset + p.size;
        let disk_same = state.identity(&self.target).is_some_and(|now| self.seen.same(&now, Holds::Place));
        let made = state.identity(&made.path).filter(|_| in_place && disk_same)?;
        Some(Confirm::new(made, Queued(())))
    }

    /// Whether it needs a confirmation token ([`aizome_sys::disks::Confirm`])
    /// to run: whether it can lose data.
    pub fn is_destructive(&self) -> bool {
        matches!(
            self.kind,
            OpKind::NewTable { .. } | OpKind::Delete | OpKind::Format(_) | OpKind::Resize { .. } | OpKind::Repair
        )
    }

    fn rewrites(&self) -> bool {
        matches!(self.kind, OpKind::Delete | OpKind::Format(_) | OpKind::Resize { .. } | OpKind::Repair)
    }
}

/// What the person can do to a segment (or its disk).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Mount,
    Unmount,
    Unlock,
    Lock,
    SwapOn,
    SwapOff,
    New,
    Delete,
    Format,
    Resize,
    Label,
    Flags,
    Check,
    Repair,
}

/// The pending operations, in the order they will run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub ops: Vec<Op>,
    next: u64,
}

/// Whether a plan's words warn of losing data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Loss {
    None,
    /// It can lose data if it goes wrong: back up first.
    Risk(String),
    /// It erases data.
    Destroys(String),
}

/// An operation in plain words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Words {
    pub title: String,
    pub detail: String,
    pub loss: Loss,
}

type Check = Result<(), String>;

fn quote(s: &str) -> String {
    format!("“{s}”")
}

/// "Ext4 “home”", "FAT", "unformatted".
fn what_is_on(seg: &Seg) -> String {
    let fs = seg.shown_fs();
    let name = if fs.is_empty() { "unformatted".to_owned() } else { fs::name(fs) };
    let label = seg.shown_label();
    let mut s = if label.is_empty() { name } else { format!("{name} {}", quote(label)) };
    if seg.is_locked() {
        s = format!("{s}, locked");
    }
    if let Some(used) = seg.shown_used() {
        s = format!("{s}, {} used", format_size(used));
    }
    s
}

impl Plan {
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn on_disk<'a>(&'a self, disk: &'a str) -> impl Iterator<Item = &'a Op> + 'a {
        self.ops.iter().filter(move |o| o.disk == disk)
    }

    /// The disk as it will be after the pending operations.
    pub fn preview(&self, state: &DisksState, disk: &str) -> Option<Layout> {
        let mut l = layout::current(state, disk)?;
        for op in self.on_disk(disk) {
            apply(&mut l, op);
        }
        Some(l)
    }

    fn id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    // --- What can be done -----------------------------------------------

    /// Whether `action` can be done to segment `i` of `layout` (a preview),
    /// and if not, why, for the toolbar's tooltip and the status line.
    pub fn can(&self, layout: &Layout, i: usize, action: Action) -> Check {
        let Some(seg) = layout.segs.get(i) else { return Err("Select a partition.".into()) };
        let kind = seg.kind();
        let pending_here = !seg.pending.is_empty();
        let is_new = matches!(seg.target, Some(Target::New(_)));
        let in_use = || -> Check {
            if seg.read_only {
                return Err(format!("{} is read-only.", seg.short_name()));
            }
            if let Some(m) = seg.shown_mount() {
                return Err(format!("{} is mounted at {m}: unmount it first.", seg.short_name()));
            }
            if seg.fs == "swap" && seg.in_use {
                return Err(format!("{} is in use as swap: stop using it first.", seg.short_name()));
            }
            if seg.is_unlocked() {
                return Err(format!("{} is unlocked: lock it first.", seg.short_name()));
            }
            if seg.in_use {
                return Err(format!("{} is in use.", seg.short_name()));
            }
            Ok(())
        };
        let pending_first = |what: &str| -> Check {
            if pending_here || is_new || layout.new_table {
                Err(format!("Apply the pending operations before {what} {}.", seg.short_name()))
            } else {
                Ok(())
            }
        };
        let rewrite_pending = |marks: &[Mark]| -> Check {
            match seg.pending.iter().find(|m| marks.contains(m)) {
                Some(m) => Err(format!(
                    "{} already has a pending {}: apply or undo it first.",
                    seg.short_name(),
                    mark_word(*m)
                )),
                None => Ok(()),
            }
        };
        match action {
            Action::Mount => {
                pending_first("mounting")?;
                if seg.is_locked() {
                    return Err("Unlock it first.".into());
                }
                if seg.shown_mount().is_some() {
                    return Err("It's mounted already.".into());
                }
                let fs = seg.shown_fs();
                if fs.is_empty()
                    || fs == "swap"
                    || fs == "crypto_LUKS"
                    || !matches!(kind, SegKind::Partition | SegKind::Whole)
                {
                    return Err("There's no filesystem to mount.".into());
                }
                Ok(())
            }
            Action::Unmount => {
                if seg.shown_mount().is_none() {
                    return Err("It isn't mounted.".into());
                }
                Ok(())
            }
            Action::Unlock => {
                pending_first("unlocking")?;
                if !seg.is_locked() {
                    return Err("It isn't a locked encrypted volume.".into());
                }
                Ok(())
            }
            Action::Lock => {
                if !seg.is_unlocked() {
                    return Err("It isn't unlocked.".into());
                }
                Ok(())
            }
            Action::SwapOn | Action::SwapOff => {
                pending_first("changing")?;
                if seg.fs != "swap" {
                    return Err("It isn't swap.".into());
                }
                match (action == Action::SwapOn, seg.in_use) {
                    (true, true) => Err("It's in use as swap already.".into()),
                    (false, false) => Err("It isn't in use.".into()),
                    (true, false) | (false, true) => Ok(()),
                }
            }
            Action::New => {
                if kind != SegKind::Free {
                    return Err("Select unallocated space to make a partition in.".into());
                }
                if layout.table.is_empty() {
                    return Err("Create a partition table first.".into());
                }
                if layout.table == "dos" && !seg.logical && layout.primaries() >= 4 {
                    return Err("An MBR disk holds four primary partitions, and this one has four.".into());
                }
                if seg.size < MIB {
                    return Err("There's too little room here.".into());
                }
                Ok(())
            }
            Action::Delete => {
                if !matches!(kind, SegKind::Partition | SegKind::Extended) {
                    return Err("Select a partition to delete.".into());
                }
                if is_new {
                    return Ok(());
                }
                in_use()?;
                if kind == SegKind::Extended && layout.segs.iter().any(|s| s.logical && !s.is_free()) {
                    return Err("Delete the logical partitions inside it first.".into());
                }
                Ok(())
            }
            Action::Format => {
                match kind {
                    SegKind::Free => return Err("Make a partition here first.".into()),
                    SegKind::Extended => {
                        return Err("An extended partition holds others; it can't hold a filesystem.".into());
                    }
                    SegKind::Partition | SegKind::Whole | SegKind::Blank => {}
                }
                if is_new {
                    return Ok(());
                }
                in_use()?;
                rewrite_pending(&[Mark::Resize, Mark::Repair])
            }
            Action::Resize => {
                if kind != SegKind::Partition {
                    return Err("Select a partition to resize.".into());
                }
                let r = fs::resizable(&seg.fs);
                if !r.grow && !r.shrink {
                    return Err(r.why_not.unwrap_or("It can't be resized.").into());
                }
                if is_new {
                    return Ok(());
                }
                in_use()?;
                rewrite_pending(&[Mark::Format, Mark::Resize, Mark::Repair])?;
                let room = layout.room_after(i);
                if room < MIB && !r.shrink {
                    return Err("There's no free space after it to grow into, and its filesystem can't shrink.".into());
                }
                if room < MIB && seg.size <= 2 * MIB {
                    return Err("There's no free space after it, and it's too small to shrink.".into());
                }
                Ok(())
            }
            Action::Label => {
                let fs_now = seg.shown_fs();
                if !matches!(kind, SegKind::Partition | SegKind::Whole) || fs_now.is_empty() {
                    return Err("There's no filesystem to label.".into());
                }
                if seg.is_unlocked() || seg.is_locked() {
                    return Err("Changing labels on encrypted filesystems isn't supported here.".into());
                }
                if is_new || seg.pending.contains(&Mark::Format) {
                    return Ok(());
                }
                if !fs::can_label(fs_now) {
                    return Err(format!("{} labels can't be changed here.", fs::name(fs_now)));
                }
                if seg.read_only {
                    return Err(format!("{} is read-only.", seg.short_name()));
                }
                if seg.mount.is_some() && !fs::labels_while_mounted(fs_now) {
                    return Err(format!("{} labels change only while unmounted: unmount it first.", fs::name(fs_now)));
                }
                Ok(())
            }
            Action::Flags => {
                if kind != SegKind::Partition {
                    return Err("Select a partition to set its flags.".into());
                }
                if seg.read_only {
                    return Err(format!("{} is read-only.", seg.short_name()));
                }
                Ok(())
            }
            Action::Check | Action::Repair => {
                pending_first(if action == Action::Check { "checking" } else { "repairing" })?;
                if !matches!(kind, SegKind::Partition | SegKind::Whole) || !fs::can_repair(&seg.fs) {
                    return Err(if seg.fs.is_empty() || seg.fs == "crypto_LUKS" {
                        "There's no filesystem to check here.".into()
                    } else {
                        format!("{} filesystems can't be checked here.", fs::name(&seg.fs))
                    });
                }
                in_use()
            }
        }
    }

    /// Whether the disk can take a new partition table.
    pub fn can_new_table(&self, state: &DisksState, layout: &Layout) -> Check {
        let disk = state.block(&layout.disk).ok_or("The disk is gone.")?;
        if disk.read_only {
            return Err("The disk is read-only.".into());
        }
        if disk.partition.is_some() {
            return Err("A partition table goes on a whole disk.".into());
        }
        if state.in_use(&layout.disk) {
            return Err("Something on this disk is mounted or in use: unmount it first.".into());
        }
        if state
            .dependents(&layout.disk)
            .iter()
            .any(|b| b.encrypted.as_ref().is_some_and(|e| e.cleartext_device.is_some()))
        {
            return Err("Something on this disk is unlocked: lock it first.".into());
        }
        Ok(())
    }

    // --- Queueing ---------------------------------------------------------

    /// An operation on `on` (the disk of `layout`, or a device on it), as
    /// it is now.
    fn op(&mut self, layout: &Layout, on: &Identity, kind: OpKind) -> Op {
        Op {
            id: self.id(),
            disk: layout.disk.clone(),
            disk_device: layout.device.clone(),
            target: on.path().to_owned(),
            device: on.device().to_owned(),
            kind,
            seen: on.clone(),
        }
    }

    /// The pending operation that makes the new partition `id`.
    fn creation(&mut self, id: u64) -> Option<&mut NewPartition> {
        self.ops.iter_mut().find(|o| o.id == id).and_then(|o| match &mut o.kind {
            OpKind::Create(p) => Some(p),
            OpKind::NewTable { .. }
            | OpKind::Delete
            | OpKind::Format(_)
            | OpKind::Resize { .. }
            | OpKind::Label { .. }
            | OpKind::Flags { .. }
            | OpKind::Repair => None,
        })
    }

    /// A new partition table on the disk `at` is on: what was pending there
    /// goes.
    pub fn new_table(&mut self, state: &DisksState, at: Here<'_>, kind: TableKind) -> Check {
        let layout = at.layout();
        self.can_new_table(state, layout)?;
        self.ops.retain(|o| o.disk != layout.disk);
        let op = self.op(layout, at.disk(), OpKind::NewTable { kind });
        self.ops.push(op);
        Ok(())
    }

    /// A partition in the free space `at`.
    pub fn create(&mut self, at: Here<'_>, p: NewPartition) -> Result<u64, String> {
        let (layout, i) = located(&at)?;
        self.can(layout, i, Action::New)?;
        let free = &layout.segs[i];
        if p.size < MIB || p.offset < free.offset || p.offset + p.size > free.end() {
            return Err("The new partition must fit in the free space.".into());
        }
        check_label(&p.fs, &p.label)?;
        check_encryption(&p.fs, p.encrypt.as_ref())?;
        if !p.name.is_empty() && layout.table != "gpt" {
            return Err("Only GPT partitions have names.".into());
        }
        let p = NewPartition { logical: free.logical, ..p };
        let op = self.op(layout, at.disk(), OpKind::Create(p));
        let id = op.id;
        self.ops.push(op);
        Ok(id)
    }

    /// Delete the partition at `i` (or undo the one that would make it).
    pub fn delete(&mut self, at: Here<'_>) -> Check {
        let (layout, i) = located(&at)?;
        self.can(layout, i, Action::Delete)?;
        let seg = &layout.segs[i];
        match seg.target.clone() {
            Some(Target::New(id)) => {
                self.ops.retain(|o| o.id != id);
                Ok(())
            }
            Some(Target::Block(path)) => {
                self.ops.retain(|o| o.target != path);
                let op = self.op(layout, device(&at)?, OpKind::Delete);
                self.ops.push(op);
                Ok(())
            }
            None => Err("Select a partition to delete.".into()),
        }
    }

    /// Format the device at `i` (or change what a pending new partition
    /// will hold).
    pub fn format(&mut self, at: Here<'_>, spec: NewFs) -> Check {
        let (layout, i) = located(&at)?;
        self.can(layout, i, Action::Format)?;
        check_label(&spec.fs, &spec.label)?;
        check_encryption(&spec.fs, spec.encrypt.as_ref())?;
        let seg = &layout.segs[i];
        match seg.target.clone() {
            Some(Target::New(id)) => {
                let p = self.creation(id).ok_or("The new partition is gone.")?;
                p.fs = spec.fs;
                p.label = spec.label;
                p.encrypt = spec.encrypt;
                Ok(())
            }
            Some(Target::Block(path)) => {
                // A new format replaces the old; the label goes with it.
                self.ops.retain(|o| !(o.target == path && matches!(o.kind, OpKind::Format(_) | OpKind::Label { .. })));
                let op = self.op(layout, device(&at)?, OpKind::Format(spec));
                self.ops.push(op);
                Ok(())
            }
            None => Err("Make a partition here first.".into()),
        }
    }

    /// The largest the partition at `i` can grow to, and the least it can
    /// shrink to (`used` is the filesystem's bytes in use, when known).
    pub fn resize_range(&self, layout: &Layout, i: usize) -> (u64, u64) {
        self.resize_range_knowing(layout, i, None)
    }

    /// [`resize_range`](Plan::resize_range), knowing the filesystem had
    /// `used` bytes in use when it was last seen mounted.
    pub fn resize_range_knowing(&self, layout: &Layout, i: usize, used: Option<u64>) -> (u64, u64) {
        let Some(seg) = layout.segs.get(i) else { return (0, 0) };
        let r = fs::resizable(&seg.fs);
        let max = if r.grow { seg.size + layout.room_after(i) } else { seg.size };
        let min = if r.shrink {
            // What's in use, with a tenth more and 16 MiB of room for the
            // filesystem's own structures; whole MiB.
            let floor = seg.used.or(used).map_or(16 * MIB, |u| u + u / 10 + 16 * MIB);
            floor.div_ceil(MIB) * MIB
        } else {
            seg.size
        };
        (min.min(seg.size), max)
    }

    /// Move the end of the partition at `i` so it is `to` bytes long.
    pub fn resize(&mut self, at: Here<'_>, to: u64) -> Check {
        let (layout, i) = located(&at)?;
        self.can(layout, i, Action::Resize)?;
        let seg = &layout.segs[i];
        let to = to / MIB * MIB;
        let (min, max) = self.resize_range(layout, i);
        if to < min {
            return Err(format!("It can't shrink below {}.", format_size(min)));
        }
        if to > max {
            return Err(format!("It can grow to {} at most.", format_size(max)));
        }
        if to == seg.size {
            return Err("That's the size it is.".into());
        }
        match seg.target.clone() {
            Some(Target::New(id)) => {
                self.creation(id).ok_or("The new partition is gone.")?.size = to;
                Ok(())
            }
            Some(Target::Block(_)) => {
                let op = self.op(layout, device(&at)?, OpKind::Resize { from: seg.size, to });
                self.ops.push(op);
                Ok(())
            }
            None => Err("Select a partition to resize.".into()),
        }
    }

    /// Change the label of the filesystem at `i` (`state` tells the label
    /// it has now: setting that one again only drops a pending change).
    pub fn label(&mut self, state: &DisksState, at: Here<'_>, label: &str) -> Check {
        let (layout, i) = located(&at)?;
        self.can(layout, i, Action::Label)?;
        let seg = &layout.segs[i];
        check_label(&seg.fs, label)?;
        match seg.target.clone() {
            Some(Target::New(id)) => {
                self.creation(id).ok_or("The new partition is gone.")?.label = label.to_owned();
                Ok(())
            }
            Some(Target::Block(path)) => {
                if let Some(OpKind::Format(f)) = self
                    .ops
                    .iter_mut()
                    .find(|o| o.target == path && matches!(o.kind, OpKind::Format(_)))
                    .map(|o| &mut o.kind)
                {
                    f.label = label.to_owned();
                    return Ok(());
                }
                self.ops.retain(|o| !(o.target == path && matches!(o.kind, OpKind::Label { .. })));
                let now = state.block(&path).map(|b| b.id_label.as_str());
                if now != Some(label) {
                    let op = self.op(layout, device(&at)?, OpKind::Label { label: label.to_owned() });
                    self.ops.push(op);
                }
                Ok(())
            }
            None => Err("There's no filesystem to label.".into()),
        }
    }

    /// Set the flags of the partition at `i`.
    pub fn flags(&mut self, at: Here<'_>, to: Flags) -> Check {
        let (layout, i) = located(&at)?;
        self.can(layout, i, Action::Flags)?;
        let seg = &layout.segs[i];
        if to.bios_grub && layout.table == "dos" {
            return Err("BIOS boot partitions are for GPT disks.".into());
        }
        if to.esp && to.bios_grub {
            return Err("A partition is an EFI System Partition or a BIOS boot partition, not both.".into());
        }
        match seg.target.clone() {
            Some(Target::New(id)) => {
                self.creation(id).ok_or("The new partition is gone.")?.flags = to;
                Ok(())
            }
            Some(Target::Block(path)) => {
                // The flags as the disk has them, before any pending change.
                let pending = self.ops.iter().find(|o| o.target == path && matches!(o.kind, OpKind::Flags { .. }));
                let from = match pending.map(|o| &o.kind) {
                    Some(OpKind::Flags { from, .. }) => *from,
                    Some(
                        OpKind::NewTable { .. }
                        | OpKind::Create(_)
                        | OpKind::Delete
                        | OpKind::Format(_)
                        | OpKind::Resize { .. }
                        | OpKind::Label { .. }
                        | OpKind::Repair,
                    )
                    | None => Flags::of(&layout.table, &seg.type_id, seg.flag_bits),
                };
                self.ops.retain(|o| !(o.target == path && matches!(o.kind, OpKind::Flags { .. })));
                if from != to {
                    let op = self.op(layout, device(&at)?, OpKind::Flags { from, to });
                    self.ops.push(op);
                }
                Ok(())
            }
            None => Err("Select a partition to set its flags.".into()),
        }
    }

    /// Repair the filesystem at `i`.
    pub fn repair(&mut self, at: Here<'_>) -> Check {
        let (layout, i) = located(&at)?;
        self.can(layout, i, Action::Repair)?;
        let seg = &layout.segs[i];
        let path = seg.block().ok_or("There's no filesystem to repair here.")?.to_owned();
        if self.ops.iter().any(|o| o.target == path && o.rewrites()) {
            return Err(format!("{} already has a pending change: apply or undo it first.", seg.short_name()));
        }
        let op = self.op(layout, device(&at)?, OpKind::Repair);
        self.ops.push(op);
        Ok(())
    }

    /// Drop operations whose disk or device is gone (unplugged, detached)
    /// or isn't the one they were queued for (another stick at the same
    /// names, another tool's new filesystem): the words for those dropped.
    pub fn prune(&mut self, state: &DisksState) -> Vec<String> {
        let mut dropped = Vec::new();
        self.ops.retain(|o| {
            let ok = state.block(&o.disk).is_some() && o.still_there(state);
            if !ok {
                dropped.push(format!("{} {}", verb(&o.kind), o.device));
            }
            ok
        });
        dropped
    }

    /// Why the plan can't be applied now (a device got mounted since it
    /// was queued, a disk changed), if it can't.
    pub fn problems(&self, state: &DisksState) -> Vec<String> {
        let mut out = Vec::new();
        for o in &self.ops {
            let Some(target) = state.block(&o.target) else {
                out.push(format!("{} is gone.", o.device));
                continue;
            };
            if !o.still_there(state) {
                out.push(format!("{} isn't the device this was queued for: undo it.", o.device));
                continue;
            }
            let busy = match &o.kind {
                OpKind::NewTable { .. } => state.in_use(&o.target),
                OpKind::Create(_) | OpKind::Flags { .. } => false,
                OpKind::Label { .. } => {
                    let mounted = target.filesystem.as_ref().is_some_and(|f| !f.mount_points.is_empty());
                    mounted && !fs::labels_while_mounted(&target.id_type)
                }
                OpKind::Delete | OpKind::Format(_) | OpKind::Resize { .. } | OpKind::Repair => {
                    state.in_use(&o.target) || target.encrypted.as_ref().is_some_and(|e| e.cleartext_device.is_some())
                }
            };
            if busy {
                out.push(format!("{} is in use now: unmount or lock it first.", o.device));
            }
        }
        out.dedup();
        out
    }

    // --- Words ---------------------------------------------------------------

    /// An operation in plain words, with what it puts at risk, from the
    /// disk as it is now.
    pub fn words(&self, op: &Op, state: &DisksState) -> Words {
        let now = layout::current(state, &op.disk);
        let seg = now.as_ref().and_then(|l| l.segs.iter().find(|s| s.block() == Some(op.target.as_str())));
        let on = seg.map(what_is_on).unwrap_or_default();
        let dev = op.device.as_str();
        let disk = op.disk_device.as_str();
        match &op.kind {
            OpKind::NewTable { kind } => {
                let (name, why) = match kind {
                    TableKind::Gpt => ("GPT", "any size, up to 128 partitions"),
                    TableKind::Dos => ("MBR", "up to 2 TB and four primary partitions, for old computers"),
                };
                let parts: Vec<String> = now
                    .as_ref()
                    .map(|l| l.partitions().map(|s| format!("{} ({})", s.short_name(), what_is_on(s))).collect())
                    .unwrap_or_default();
                let loss =
                    match (parts.is_empty(), now.as_ref().map(|l| l.segs.iter().any(|s| s.kind() == SegKind::Whole))) {
                        (false, _) => {
                            Loss::Destroys(format!("Every partition on {disk} will be lost: {}.", parts.join(", ")))
                        }
                        (true, Some(true)) => Loss::Destroys(format!(
                            "The filesystem on {disk} ({}) will be lost.",
                            seg_on_whole(now.as_ref())
                        )),
                        (true, Some(false) | None) => Loss::Destroys(format!("Anything on {disk} will be lost.")),
                    };
                Words {
                    title: format!("Create a new {name} partition table on {disk}"),
                    detail: format!("{name}: {why}."),
                    loss,
                }
            }
            OpKind::Create(p) => {
                let what = if p.fs.is_empty() { "unformatted".to_owned() } else { fs::name(&p.fs) };
                let mut title = format!("Create a {} {} partition on {disk}", format_size(p.size), what);
                if !p.label.is_empty() {
                    title.push_str(&format!(" labelled {}", quote(&p.label)));
                }
                let mut detail = format!("In the free space at {}", format_size(p.offset));
                if p.logical {
                    detail.push_str(", as a logical partition");
                }
                if p.encrypt.is_some() {
                    detail.push_str(", encrypted with LUKS");
                }
                let flags = p.flags.names();
                if !flags.is_empty() {
                    detail.push_str(&format!(", flagged {}", flags.join(" and ")));
                }
                detail.push('.');
                Words { title, detail, loss: Loss::None }
            }
            OpKind::Delete => Words {
                title: format!("Delete partition {dev}"),
                detail: format!(
                    "{} on {disk}; its space becomes unallocated.",
                    seg.map_or(String::new(), |s| format_size(s.size))
                ),
                loss: Loss::Destroys(format!("Everything on {dev} ({on}) will be lost.")),
            },
            OpKind::Format(f) => {
                let mut title = format!("Format {dev} as {}", fs::name(&f.fs));
                if !f.label.is_empty() {
                    title.push_str(&format!(" labelled {}", quote(&f.label)));
                }
                let mut detail = Vec::new();
                if f.encrypt.is_some() {
                    detail.push("Encrypted with LUKS: it will ask for its passphrase to unlock.".to_owned());
                }
                match f.erase {
                    Erase::Quick => {}
                    Erase::Zeros => {
                        detail.push("Every byte is overwritten with zeros first, which takes a while.".to_owned())
                    }
                    Erase::AtaSecureErase | Erase::AtaSecureEraseEnhanced => {
                        detail.push("The drive erases itself first (ATA Secure Erase).".to_owned())
                    }
                }
                if detail.is_empty() {
                    detail.push(format!("{} on {disk}.", seg.map_or(String::new(), |s| format_size(s.size))));
                }
                Words {
                    title,
                    detail: detail.join(" "),
                    loss: Loss::Destroys(format!("Everything on {dev} ({on}) will be erased.")),
                }
            }
            OpKind::Resize { from, to } => {
                let grow = to > from;
                let fs_now = seg.map(|s| s.fs.clone()).unwrap_or_default();
                let title = format!(
                    "{} {dev} from {} to {}",
                    if grow { "Grow" } else { "Shrink" },
                    format_size(*from),
                    format_size(*to)
                );
                let how = match (grow, fs_now.is_empty()) {
                    (_, true) => "Its end moves; its start stays where it is.".to_owned(),
                    (true, false) if fs::check_before_resize(&fs_now) => {
                        "The filesystem is checked and repaired, the partition's end moves into the free space after it, then the filesystem grows to fill it.".to_owned()
                    }
                    (true, false) => {
                        "Its end moves into the free space after it, then the filesystem grows to fill it.".to_owned()
                    }
                    (false, false) if fs::check_before_resize(&fs_now) => {
                        "The filesystem is checked and repaired, shrunk, and then the partition's end moves in."
                            .to_owned()
                    }
                    (false, false) => "The filesystem shrinks, then the partition's end moves in.".to_owned(),
                };
                Words {
                    title,
                    detail: how,
                    loss: Loss::Risk(format!(
                        "Resizing rewrites {}'s filesystem. If it's interrupted (a power cut, an unplugged cable), data can be lost: back it up first.",
                        dev
                    )),
                }
            }
            OpKind::Label { label } => Words {
                title: if label.is_empty() {
                    format!("Remove {dev}'s label")
                } else {
                    format!("Label {dev} {}", quote(label))
                },
                detail: seg.map_or(String::new(), |s| format!("Now {}.", what_is_on(s))),
                loss: Loss::None,
            },
            OpKind::Flags { from, to } => {
                let on: Vec<&str> = to.names().into_iter().filter(|n| !from.names().contains(n)).collect();
                let off: Vec<&str> = from.names().into_iter().filter(|n| !to.names().contains(n)).collect();
                let mut parts = Vec::new();
                if !on.is_empty() {
                    parts.push(format!("set {}", on.join(", ")));
                }
                if !off.is_empty() {
                    parts.push(format!("clear {}", off.join(", ")));
                }
                Words {
                    title: format!("Flags on {dev}: {}", parts.join("; ")),
                    detail: "The partition's type and attributes change; its contents don't.".into(),
                    loss: Loss::None,
                }
            }
            OpKind::Repair => Words {
                title: format!("Check and repair {dev}"),
                detail: format!("{}.", on),
                loss: Loss::Destroys(format!(
                    "A repair can discard what it can't save on {dev}: files it can't place may end up in lost+found, or go."
                )),
            },
        }
    }
}

/// The layout and the segment an operation is queued at.
fn located<'l>(at: &Here<'l>) -> Result<(&'l Layout, usize), String> {
    Ok((at.layout(), at.index().ok_or("Select a partition or unallocated space.")?))
}

/// The device an operation is queued on, as it is now.
fn device<'a>(at: &'a Here<'_>) -> Result<&'a Identity, String> {
    at.device().ok_or_else(|| "Select a partition.".to_owned())
}

fn seg_on_whole(l: Option<&Layout>) -> String {
    l.and_then(|l| l.segs.iter().find(|s| s.kind() == SegKind::Whole)).map(what_is_on).unwrap_or_default()
}

fn mark_word(m: Mark) -> &'static str {
    match m {
        Mark::New => "creation",
        Mark::Format => "format",
        Mark::Resize => "resize",
        Mark::Label => "label",
        Mark::Flags => "flags change",
        Mark::Repair => "repair",
    }
}

/// The verb for an operation, for short lists.
pub fn verb(kind: &OpKind) -> &'static str {
    match kind {
        OpKind::NewTable { .. } => "New partition table on",
        OpKind::Create(_) => "New partition on",
        OpKind::Delete => "Delete",
        OpKind::Format(_) => "Format",
        OpKind::Resize { .. } => "Resize",
        OpKind::Label { .. } => "Label",
        OpKind::Flags { .. } => "Flags on",
        OpKind::Repair => "Repair",
    }
}

fn check_label(fs: &str, label: &str) -> Check {
    if label.is_empty() {
        return Ok(());
    }
    if fs.is_empty() {
        return Err("An unformatted partition has no label.".into());
    }
    match fs::label_limit(fs) {
        Some(limit) if label.len() > limit => Err(format!("A {} label has at most {limit} bytes.", fs::name(fs))),
        Some(_) | None if label.contains('\0') || label.contains('/') && fs == "vfat" => {
            Err("That label has a character FAT doesn't take.".into())
        }
        Some(_) | None => Ok(()),
    }
}

fn check_encryption(fs: &str, encrypt: Option<&Encryption>) -> Check {
    match encrypt {
        None => Ok(()),
        Some(_) if fs.is_empty() || fs == "swap" => Err("Encryption needs a filesystem inside it.".into()),
        Some(e) if e.passphrase.is_empty() => Err("Choose a passphrase.".into()),
        Some(_) => Ok(()),
    }
}

/// What an operation does to a preview.
fn apply(l: &mut Layout, op: &Op) {
    let find = |l: &mut Layout| l.segs.iter().position(|s| s.block() == Some(op.target.as_str()));
    match &op.kind {
        OpKind::NewTable { kind } => {
            l.table = match kind {
                TableKind::Gpt => "gpt".into(),
                TableKind::Dos => "dos".into(),
            };
            l.segs.clear();
            l.new_table = true;
            l.fill_free();
        }
        OpKind::Create(p) => {
            let (type_id, flag_bits) = p.flags.apply(&l.table, &p.fs, "", 0);
            l.segs.push(Seg {
                kind: Some(SegKind::Partition),
                offset: p.offset,
                size: p.size,
                target: Some(Target::New(op.id)),
                fs: if p.encrypt.is_some() { "crypto_LUKS".into() } else { p.fs.clone() },
                label: if p.encrypt.is_some() { String::new() } else { p.label.clone() },
                name: p.name.clone(),
                type_id,
                flag_bits,
                pending: vec![Mark::New],
                logical: p.logical,
                ..Default::default()
            });
            l.fill_free();
        }
        OpKind::Delete => {
            if let Some(i) = find(l) {
                l.segs.remove(i);
                l.fill_free();
            }
        }
        OpKind::Format(f) => {
            if let Some(i) = find(l) {
                let s = &mut l.segs[i];
                s.fs = if f.encrypt.is_some() { "crypto_LUKS".into() } else { f.fs.clone() };
                s.label = if f.encrypt.is_some() { String::new() } else { f.label.clone() };
                s.crypt = None;
                s.used = None;
                s.mount = None;
                if s.kind() == SegKind::Partition && !keeps_type(&s.type_id, &s.fs) {
                    s.type_id = aizome_sys::disks::types::type_for_filesystem(&l.table, &s.fs).to_owned();
                }
                if l.segs[i].kind() == SegKind::Blank {
                    l.segs[i].kind = Some(SegKind::Whole);
                }
                l.segs[i].pending.push(Mark::Format);
            }
        }
        OpKind::Resize { to, .. } => {
            if let Some(i) = find(l) {
                l.segs[i].size = *to;
                l.segs[i].pending.push(Mark::Resize);
                l.fill_free();
            }
        }
        OpKind::Label { label } => {
            if let Some(i) = find(l) {
                l.segs[i].label = label.clone();
                l.segs[i].pending.push(Mark::Label);
            }
        }
        OpKind::Flags { to, .. } => {
            if let Some(i) = find(l) {
                let table = l.table.clone();
                let s = &mut l.segs[i];
                let (t, b) = to.apply(&table, &s.fs, &s.type_id, s.flag_bits);
                s.type_id = t;
                s.flag_bits = b;
                s.pending.push(Mark::Flags);
            }
        }
        OpKind::Repair => {
            if let Some(i) = find(l) {
                l.segs[i].pending.push(Mark::Repair);
            }
        }
    }
}

/// Whether formatting a partition of `type_id` as `fs` keeps its type
/// rather than changing it to match: an EFI System Partition stays one
/// when it's given FAT again.
pub fn keeps_type(type_id: &str, fs: &str) -> bool {
    let t = type_id.to_ascii_lowercase();
    (t == layout::ESP_GPT || t == layout::ESP_DOS || t == "0xef") && fs == "vfat"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::sample;

    const B: &str = "/org/freedesktop/UDisks2/block_devices/";
    const GIB: u64 = 1 << 30;

    fn disk(name: &str) -> String {
        format!("{B}{name}")
    }

    fn at(l: &Layout, short: &str) -> usize {
        l.segs
            .iter()
            .position(|s| s.short_name() == short)
            .unwrap_or_else(|| panic!("{short} in {:?}", l.segs.iter().map(|s| s.short_name()).collect::<Vec<_>>()))
    }

    /// Segment `i` of `l`, chosen and found now.
    fn here<'l>(s: &DisksState, l: &'l Layout, i: usize) -> Here<'l> {
        let opening = crate::app::Opening::for_tests();
        let chosen =
            l.choose(s, &crate::layout::Sel::of(&l.segs[i]), &opening).unwrap_or_else(|| panic!("segment {i}"));
        l.locate(s, &chosen).expect("found")
    }

    /// The disk of `l`, chosen and found now.
    fn disk_here<'l>(s: &DisksState, l: &'l Layout) -> Here<'l> {
        l.locate(s, &l.choose_disk(s, &crate::app::Opening::for_tests()).expect("the disk")).expect("the disk")
    }

    /// The largest stretch of free space.
    fn free(l: &Layout) -> usize {
        (0..l.segs.len()).filter(|&i| l.segs[i].is_free()).max_by_key(|&i| l.segs[i].size).expect("free space")
    }

    fn ext4(label: &str) -> NewFs {
        NewFs { fs: "ext4".into(), label: label.into(), erase: Erase::Quick, encrypt: None }
    }

    #[test]
    fn nothing_in_use_is_changed() {
        let s = sample();
        let p = Plan::default();
        let l = p.preview(&s, &disk("sdc")).unwrap();
        let share = at(&l, "sdc2");
        let why = p.can(&l, share, Action::Format).unwrap_err();
        assert!(why.contains("mounted at /run/media/callen/SHARE"), "{why}");
        assert!(p.can(&l, share, Action::Delete).is_err());
        assert!(p.can(&l, share, Action::Unmount).is_ok());
        let nvme = p.preview(&s, &disk("nvme0n1")).unwrap();
        let root = at(&nvme, "nvme0n1p2");
        assert!(
            p.can(&nvme, root, Action::Format).unwrap_err().contains("mounted at /"),
            "the unlocked root shows its mount"
        );
        assert!(p.can(&nvme, at(&nvme, "nvme0n1p3"), Action::Delete).unwrap_err().contains("swap"));
        assert!(p.can_new_table(&s, &nvme).is_err(), "the system disk is in use");
        assert!(p.can(&nvme, root, Action::Lock).is_ok());
    }

    #[test]
    fn a_pending_partition_is_edited_not_targeted() {
        let s = sample();
        let mut p = Plan::default();
        let l = p.preview(&s, &disk("sda")).unwrap();
        let f = free(&l);
        let room = l.segs[f].clone();
        let id = p
            .create(
                here(&s, &l, f),
                NewPartition {
                    offset: room.offset,
                    size: 10 * GIB,
                    fs: "ext4".into(),
                    label: "data".into(),
                    name: String::new(),
                    encrypt: None,
                    flags: Flags::default(),
                    logical: false,
                },
            )
            .unwrap();
        let l = p.preview(&s, &disk("sda")).unwrap();
        let new = l.find(&Target::New(id)).unwrap();
        assert_eq!(l.segs[new].pending, [Mark::New]);
        assert!(l.segs.iter().any(|s| s.is_free() && s.offset == room.offset + 10 * GIB), "the rest stays free");
        // Formatting, labelling, resizing and flagging it edit the one operation.
        p.format(here(&s, &l, new), NewFs { fs: "xfs".into(), ..ext4("") }).unwrap();
        p.label(&s, here(&s, &l, new), "media").unwrap();
        p.resize(here(&s, &l, new), 12 * GIB).unwrap();
        p.flags(here(&s, &l, new), Flags { hidden: true, ..Flags::default() }).unwrap();
        assert_eq!(p.len(), 1);
        let OpKind::Create(made) = &p.ops[0].kind else { panic!() };
        assert_eq!(
            (made.fs.as_str(), made.label.as_str(), made.size, made.flags.hidden),
            ("xfs", "media", 12 * GIB, true)
        );
        // Deleting it undoes it.
        let l = p.preview(&s, &disk("sda")).unwrap();
        p.delete(here(&s, &l, l.find(&Target::New(id)).unwrap())).unwrap();
        assert!(p.is_empty());
    }

    #[test]
    fn one_rewrite_per_device() {
        let s = sample();
        let mut p = Plan::default();
        let l = p.preview(&s, &disk("sdc")).unwrap();
        let stick = at(&l, "sdc1");
        p.format(here(&s, &l, stick), ext4("one")).unwrap();
        let l = p.preview(&s, &disk("sdc")).unwrap();
        // A second format replaces the first; a label joins it.
        p.format(here(&s, &l, stick), ext4("two")).unwrap();
        p.label(&s, here(&s, &l, stick), "three").unwrap();
        assert_eq!(p.len(), 1);
        let OpKind::Format(f) = &p.ops[0].kind else { panic!() };
        assert_eq!(f.label, "three");
        // A resize waits for the format.
        let l = p.preview(&s, &disk("sdc")).unwrap();
        assert!(p.resize(here(&s, &l, stick), 8 * GIB).unwrap_err().contains("pending format"));
        // Deleting drops the format.
        p.delete(here(&s, &l, stick)).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p.ops[0].kind, OpKind::Delete);
        let l = p.preview(&s, &disk("sdc")).unwrap();
        assert!(l.segs.iter().all(|s| s.short_name() != "sdc1"), "the preview shows it gone");
    }

    #[test]
    fn labels_and_flags_that_change_nothing_drop_out() {
        let s = sample();
        let mut p = Plan::default();
        let l = p.preview(&s, &disk("sdc")).unwrap();
        let stick = at(&l, "sdc1");
        p.label(&s, here(&s, &l, stick), "NEW").unwrap();
        assert_eq!(p.len(), 1);
        p.label(&s, here(&s, &l, stick), "STICK").unwrap();
        assert!(p.is_empty(), "back to the label it has");
        assert!(p.label(&s, here(&s, &l, stick), "MUCH TOO LONG FOR FAT").unwrap_err().contains("11 bytes"));
        let was = Flags::of("dos", "0x0c", 0x80);
        assert!(was.boot);
        p.flags(here(&s, &l, stick), Flags { boot: false, ..was }).unwrap();
        let l = p.preview(&s, &disk("sdc")).unwrap();
        p.flags(here(&s, &l, stick), was).unwrap();
        assert!(p.is_empty());
        assert!(p.flags(here(&s, &l, stick), Flags { bios_grub: true, ..was }).unwrap_err().contains("GPT"));
    }

    #[test]
    fn a_new_table_wipes_the_preview_and_the_plan_for_that_disk() {
        let s = sample();
        let mut p = Plan::default();
        let l = p.preview(&s, &disk("mmcblk0")).unwrap();
        p.label(&s, here(&s, &l, at(&l, "mmcblk0p2")), "root").unwrap();
        let sdc = p.preview(&s, &disk("sdc")).unwrap();
        p.label(&s, here(&s, &sdc, at(&sdc, "sdc1")), "KEEP").unwrap();
        p.new_table(&s, disk_here(&s, &l), TableKind::Gpt).unwrap();
        assert_eq!(p.len(), 2, "the other disk's operation stays");
        let l = p.preview(&s, &disk("mmcblk0")).unwrap();
        assert!(l.new_table && l.table == "gpt");
        assert_eq!(l.segs.len(), 1);
        assert!(l.segs[0].is_free());
        let w = p.words(p.ops.iter().find(|o| matches!(o.kind, OpKind::NewTable { .. })).unwrap(), &s);
        assert!(matches!(&w.loss, Loss::Destroys(m) if m.contains("mmcblk0p1") && m.contains("mmcblk0p2")), "{w:?}");
    }

    #[test]
    fn resizing_moves_only_the_end_within_what_fits() {
        let s = sample();
        let mut p = Plan::default();
        let l = p.preview(&s, &disk("mmcblk0")).unwrap();
        let root = at(&l, "mmcblk0p2");
        let (min, max) = p.resize_range(&l, root);
        assert_eq!(max, l.segs[root].size + l.room_after(root));
        assert!(min < l.segs[root].size);
        assert!(p.resize(here(&s, &l, root), max + GIB).unwrap_err().contains("at most"));
        p.resize(here(&s, &l, root), 16 * GIB).unwrap();
        let l = p.preview(&s, &disk("mmcblk0")).unwrap();
        assert_eq!(l.segs[at(&l, "mmcblk0p2")].size, 16 * GIB);
        let w = p.words(&p.ops[0], &s);
        assert!(w.title.starts_with("Grow mmcblk0p2") || w.title.starts_with("Grow /dev/mmcblk0p2"), "{}", w.title);
        assert!(matches!(w.loss, Loss::Risk(_)));
        // Btrfs and XFS don't resize unmounted.
        let hdd = p.preview(&s, &disk("sdb")).unwrap();
        assert!(p.can(&hdd, at(&hdd, "sdb1"), Action::Resize).unwrap_err().contains("Btrfs"));
    }

    #[test]
    fn words_say_what_is_lost() {
        let s = sample();
        let mut p = Plan::default();
        let l = p.preview(&s, &disk("sda")).unwrap();
        p.format(
            here(&s, &l, at(&l, "sda2")),
            NewFs { encrypt: Some(Encryption { passphrase: "x".into(), luks1: false }), ..ext4("vault") },
        )
        .unwrap();
        let w = p.words(&p.ops[0], &s);
        assert_eq!(w.title, "Format /dev/sda2 as Ext4 labelled “vault”");
        assert!(w.detail.contains("LUKS"));
        assert_eq!(w.loss, Loss::Destroys("Everything on /dev/sda2 (NTFS “Windows”) will be erased.".into()));
        let l = p.preview(&s, &disk("sda")).unwrap();
        assert_eq!(l.segs[at(&l, "sda2")].fs, "crypto_LUKS", "the preview shows the encryption");
    }

    #[test]
    fn mbr_holds_four_primaries() {
        let s = sample();
        let mut p = Plan::default();
        let mut l = p.preview(&s, &disk("mmcblk0")).unwrap();
        for _ in 0..2 {
            let f = free(&l);
            let room = l.segs[f].clone();
            p.create(
                here(&s, &l, f),
                NewPartition {
                    offset: room.offset,
                    size: GIB,
                    fs: String::new(),
                    label: String::new(),
                    name: String::new(),
                    encrypt: None,
                    flags: Flags::default(),
                    logical: false,
                },
            )
            .unwrap();
            l = p.preview(&s, &disk("mmcblk0")).unwrap();
        }
        assert_eq!(l.primaries(), 4);
        assert!(p.can(&l, free(&l), Action::New).unwrap_err().contains("four primary"));
        assert!(
            p.create(
                here(&s, &l, free(&l)),
                NewPartition {
                    offset: l.segs[free(&l)].offset,
                    size: GIB,
                    fs: String::new(),
                    label: String::new(),
                    name: "x".into(),
                    encrypt: None,
                    flags: Flags::default(),
                    logical: false
                }
            )
            .is_err()
        );
    }

    #[test]
    fn prune_drops_what_is_gone() {
        let mut s = sample();
        let mut p = Plan::default();
        let l = p.preview(&s, &disk("sdc")).unwrap();
        p.label(&s, here(&s, &l, at(&l, "sdc1")), "X").unwrap();
        s.blocks.retain(|b| !b.device.starts_with("/dev/sdc"));
        assert_eq!(p.prune(&s), ["Label /dev/sdc1"]);
        assert!(p.is_empty());
    }

    #[test]
    fn prune_drops_what_another_device_took_the_place_of() {
        let mut s = sample();
        let mut p = Plan::default();
        let l = p.preview(&s, &disk("sdc")).unwrap();
        p.format(here(&s, &l, at(&l, "sdc1")), ext4("one")).unwrap();
        let l = p.preview(&s, &disk("sdc")).unwrap();
        p.flags(here(&s, &l, at(&l, "sdc1")), Flags { hidden: true, ..Flags::of("dos", "0x0c", 0x80) }).unwrap();
        let l = p.preview(&s, &disk("sda")).unwrap();
        let free_on_sda = free(&l);
        p.create(
            here(&s, &l, free_on_sda),
            NewPartition {
                offset: l.segs[free_on_sda].offset,
                size: GIB,
                fs: String::new(),
                label: String::new(),
                name: String::new(),
                encrypt: None,
                flags: Flags::default(),
                logical: false,
            },
        )
        .unwrap();
        // A new label is no new device: nothing goes.
        s.blocks.iter_mut().find(|b| b.device == "/dev/sda3").unwrap().id_label = "renamed".into();
        assert!(p.prune(&s).is_empty());
        assert!(p.problems(&s).is_empty());
        // Another tool makes a new filesystem on sdc1: the format and the
        // flags were for the partition, the format for its filesystem too.
        s.blocks.iter_mut().find(|b| b.device == "/dev/sdc1").unwrap().id_uuid = "another".into();
        assert_eq!(p.problems(&s), ["/dev/sdc1 isn't the device this was queued for: undo it."]);
        assert_eq!(p.prune(&s), ["Format /dev/sdc1"]);
        // Another stick of the same model comes up at the same names.
        s.drives.iter_mut().find(|d| d.model == "Cruzer Blade").unwrap().time_detected += 1;
        assert_eq!(p.prune(&s), ["Flags on /dev/sdc1"]);
        assert_eq!(p.len(), 1, "the new partition on the SSD stays");
    }
}
