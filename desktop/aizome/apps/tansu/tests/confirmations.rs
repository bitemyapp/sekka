//! When the run makes each operation's confirmation: as the operation
//! starts, so one late in a long run isn't refused as too old (a stick
//! erased, then its boot flag set). The in-memory mock behind a stand-in
//! whose format takes a while and that notes how old each confirmation it
//! gets is. No real device is involved.

use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aizome_sys::Task;
use aizome_sys::disks::{
    Confirm, CreatePartition, DeletePartition, DisksState, Erase, Format, FormatSpec, NewPartitionTable,
    PartitionEntry, Relabel, Repair, Resize, SelfTest, SmartAttribute, TableKind,
};
use tansu::app::Opening;
use tansu::backend::Backend;
use tansu::layout::{Flags, Layout, Sel, Volume};
use tansu::mock::Mock;
use tansu::plan::{NewFs, Plan, Queued};
use tansu::run::{Outcome, Run};

/// The run of `plan`, as if the person pressed Apply on the sheet listing it
/// (the kit's test agreement stands for their press).
fn agree(plan: &Plan, state: &aizome_sys::disks::DisksState) -> Run {
    Run::agree(aizome_kit::agree::testing::agreed(plan.shown(state)), plan, state).expect("the plan as shown")
}

/// The mock, with a format that takes `slow` and a note of each
/// confirmation's age when it arrives.
struct Slow {
    inner: Mock,
    slow: Duration,
    ages: RefCell<Vec<(&'static str, Duration)>>,
}

impl Slow {
    fn note(&self, what: &'static str, age: Duration) {
        self.ages.borrow_mut().push((what, age));
    }
}

impl Backend for Slow {
    fn snapshot(&self) -> Arc<DisksState> {
        self.inner.snapshot()
    }
    fn generation(&self) -> u64 {
        self.inner.generation()
    }
    fn mount(&self, block: &str) -> Task<String> {
        self.inner.mount(block)
    }
    fn unmount(&self, block: &str) -> Task<()> {
        self.inner.unmount(block)
    }
    fn unlock(&self, volume: Volume<'_>, passphrase: &str) -> Task<String> {
        self.inner.unlock(volume, passphrase)
    }
    fn lock(&self, block: &str) -> Task<()> {
        self.inner.lock(block)
    }
    fn set_swap(&self, block: &str, on: bool) -> Task<()> {
        self.inner.set_swap(block, on)
    }
    fn power_off(&self, drive: &str) -> Task<()> {
        self.inner.power_off(drive)
    }
    fn loop_setup(&self, file: &Path, read_only: bool) -> Task<String> {
        self.inner.loop_setup(file, read_only)
    }
    fn loop_delete(&self, block: &str) -> Task<()> {
        self.inner.loop_delete(block)
    }
    fn check(&self, block: &str) -> Task<bool> {
        self.inner.check(block)
    }
    fn smart_update(&self, drive: &str) -> Task<()> {
        self.inner.smart_update(drive)
    }
    fn smart_attributes(&self, drive: &str) -> Task<Vec<SmartAttribute>> {
        self.inner.smart_attributes(drive)
    }
    fn smart_selftest(&self, drive: &str, test: SelfTest) -> Task<()> {
        self.inner.smart_selftest(drive, test)
    }
    fn cancel_job(&self, job: &str) -> Task<()> {
        self.inner.cancel_job(job)
    }
    fn can_format(&self, fs: &str) -> Task<(bool, String)> {
        self.inner.can_format(fs)
    }
    fn create_partition_table(&self, confirm: Confirm<NewPartitionTable, Queued>, kind: TableKind) -> Task<()> {
        self.inner.create_partition_table(confirm, kind)
    }
    fn create_partition(
        &self,
        confirm: Confirm<CreatePartition, Queued>,
        offset: u64,
        size: u64,
        type_id: Option<&str>,
        name: &str,
        format: Option<FormatSpec>,
        flags: u64,
    ) -> Task<String> {
        self.inner.create_partition(confirm, offset, size, type_id, name, format, flags)
    }
    fn delete_partition(&self, confirm: Confirm<DeletePartition, Queued>) -> Task<()> {
        self.inner.delete_partition(confirm)
    }
    fn format(&self, confirm: Confirm<Format, Queued>, spec: FormatSpec) -> Task<()> {
        self.note("format", confirm.age());
        let task = self.inner.format(confirm, spec);
        // Erasing every byte first takes as long as the stick is big.
        std::thread::sleep(self.slow);
        task
    }
    fn resize_partition(&self, confirm: Confirm<Resize, Queued>, size: u64) -> Task<()> {
        self.inner.resize_partition(confirm, size)
    }
    fn resize_filesystem(&self, confirm: Confirm<Resize, Queued>, size: u64) -> Task<()> {
        self.inner.resize_filesystem(confirm, size)
    }
    fn repair(&self, confirm: Confirm<Repair, Queued>) -> Task<bool> {
        self.inner.repair(confirm)
    }
    fn repair_before_resize(&self, confirm: Confirm<Resize, Queued>) -> Task<bool> {
        self.inner.repair_before_resize(confirm)
    }
    fn set_label(&self, confirm: Confirm<Relabel, Queued>, label: &str) -> Task<()> {
        self.note("label", confirm.age());
        self.inner.set_label(confirm, label)
    }
    fn set_partition_type(&self, confirm: Confirm<PartitionEntry, Queued>, type_id: &str) -> Task<()> {
        self.note("type", confirm.age());
        self.inner.set_partition_type(confirm, type_id)
    }
    fn set_partition_flags(&self, confirm: Confirm<PartitionEntry, Queued>, flags: u64) -> Task<()> {
        self.note("flags", confirm.age());
        self.inner.set_partition_flags(confirm, flags)
    }
}

const STICK: &str = "/org/freedesktop/UDisks2/block_devices/sdc";

fn at<'l>(state: &DisksState, layout: &'l Layout, name: &str) -> tansu::layout::Here<'l> {
    let seg = layout.segs.iter().find(|s| s.short_name() == name).expect(name);
    layout.locate(state, &layout.choose(state, &Sel::of(seg), &Opening::for_tests()).expect(name)).expect(name)
}

#[test]
fn each_operation_is_confirmed_as_it_starts_not_at_apply() {
    let slow = Duration::from_millis(400);
    let backend = Slow { inner: Mock::default(), slow, ages: RefCell::default() };
    let state = backend.snapshot();
    let mut plan = Plan::default();
    let layout = plan.preview(&state, STICK).unwrap();
    let erase = NewFs { fs: "vfat".into(), label: "BOOT".into(), erase: Erase::Zeros, encrypt: None };
    plan.format(at(&state, &layout, "sdc1"), erase).unwrap();
    let layout = plan.preview(&state, STICK).unwrap();
    plan.flags(at(&state, &layout, "sdc1"), Flags { hidden: true, ..Flags::of("dos", "0x0c", 0x80) }).unwrap();

    let applied = Instant::now();
    let mut run = agree(&plan, &state);
    while run.is_running() && applied.elapsed() < Duration::from_secs(10) {
        run.advance(&backend);
    }
    assert_eq!(run.outcome, Some(Outcome::Done));
    let ages = backend.ages.borrow().clone();
    let (what, age) = *ages.last().expect("the flags step");
    assert!(what == "type" || what == "flags", "{ages:?}");
    assert!(applied.elapsed() >= slow);
    assert!(
        age < slow / 2,
        "the flags' confirmation was {age:?} old when it arrived, made at Apply rather than when it started: {ages:?}"
    );
}

/// An agreement to the plan as the sheet listed it runs only that plan: one
/// whose operations changed since (one added, or one dropped as its device
/// went) is refused.
#[test]
fn an_agreement_runs_only_the_plan_it_was_given_for() {
    let mock = Mock::default();
    let state = mock.snapshot();
    let mut plan = Plan::default();
    let layout = plan.preview(&state, STICK).unwrap();
    let erase = NewFs { fs: "vfat".into(), label: "BOOT".into(), erase: Erase::Quick, encrypt: None };
    plan.format(at(&state, &layout, "sdc1"), erase).unwrap();
    let agreed = aizome_kit::agree::testing::agreed(plan.shown(&state));
    let mut changed = plan.clone();
    let layout = changed.preview(&state, STICK).unwrap();
    changed.flags(at(&state, &layout, "sdc1"), Flags { hidden: true, ..Flags::of("dos", "0x0c", 0x80) }).unwrap();
    assert_eq!(Run::agree(agreed, &changed, &state).err(), Some(tansu::run::PlanChanged));
    let agreed = aizome_kit::agree::testing::agreed(plan.shown(&state));
    assert!(Run::agree(agreed, &plan, &state).is_ok());
}

/// Nor does it run against a plan put together elsewhere with the same
/// operations: it names the plan it was given for.
#[test]
fn an_agreement_runs_only_against_the_plan_it_listed() {
    let mock = Mock::default();
    let state = mock.snapshot();
    let mut plan = Plan::default();
    let layout = plan.preview(&state, STICK).unwrap();
    let erase = NewFs { fs: "vfat".into(), label: "BOOT".into(), erase: Erase::Quick, encrypt: None };
    plan.format(at(&state, &layout, "sdc1"), erase).unwrap();
    let agreed = aizome_kit::agree::testing::agreed(plan.shown(&state));
    let mut own = Plan::default();
    own.ops = plan.ops.clone();
    assert_eq!(Run::agree(agreed, &own, &state).err(), Some(tansu::run::PlanChanged));
    // A clone of the plan it listed (the undo history keeps them) is that plan.
    let agreed = aizome_kit::agree::testing::agreed(plan.shown(&state));
    assert!(Run::agree(agreed, &plan.clone(), &state).is_ok());
}
