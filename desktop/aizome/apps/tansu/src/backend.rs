//! What Tansu asks of UDisks2: [`aizome_sys::disks::Disks`] on the system
//! bus, or a [`Mock`](crate::mock::Mock) with sample disks for tests,
//! pictures and `--sample`.

use std::path::Path;
use std::sync::Arc;

use aizome_sys::Task;
use aizome_sys::disks::{
    Confirm, DeletePartition, Disks, DisksState, Format, FormatSpec, NewPartitionTable, Repair, Resize, SelfTest,
    SmartAttribute, TableKind,
};

/// The operations Tansu uses. Every destructive one takes its
/// [`Confirm`] token, made from the device as the person agreed to it.
pub trait Backend {
    fn snapshot(&self) -> Arc<DisksState>;
    fn generation(&self) -> u64;

    // Done at once.
    fn mount(&self, block: &str) -> Task<String>;
    fn unmount(&self, block: &str) -> Task<()>;
    fn unlock(&self, block: &str, passphrase: &str) -> Task<String>;
    fn lock(&self, block: &str) -> Task<()>;
    fn set_swap(&self, block: &str, on: bool) -> Task<()>;
    fn power_off(&self, drive: &str) -> Task<()>;
    fn loop_setup(&self, file: &Path, read_only: bool) -> Task<String>;
    fn loop_delete(&self, block: &str) -> Task<()>;
    fn check(&self, block: &str) -> Task<bool>;
    fn smart_update(&self, drive: &str) -> Task<()>;
    fn smart_attributes(&self, drive: &str) -> Task<Vec<SmartAttribute>>;
    fn smart_selftest(&self, drive: &str, test: SelfTest) -> Task<()>;
    fn cancel_job(&self, job: &str) -> Task<()>;
    /// Whether this system can make `fs` filesystems: `(true, "")`, or
    /// `(false, "the missing program")`.
    fn can_format(&self, fs: &str) -> Task<(bool, String)>;

    // Queued, and run after the Apply sheet.
    fn create_partition_table(&self, confirm: Confirm<NewPartitionTable>, kind: TableKind) -> Task<()>;
    fn create_partition(
        &self,
        table: &str,
        offset: u64,
        size: u64,
        type_id: Option<&str>,
        name: &str,
        format: Option<FormatSpec>,
    ) -> Task<String>;
    fn delete_partition(&self, confirm: Confirm<DeletePartition>) -> Task<()>;
    fn format(&self, confirm: Confirm<Format>, spec: FormatSpec) -> Task<()>;
    fn resize_partition(&self, confirm: Confirm<Resize>, size: u64) -> Task<()>;
    fn resize_filesystem(&self, confirm: Confirm<Resize>, size: u64) -> Task<()>;
    fn repair(&self, confirm: Confirm<Repair>) -> Task<bool>;
    fn set_label(&self, block: &str, label: &str) -> Task<()>;
    fn set_partition_type(&self, block: &str, type_id: &str) -> Task<()>;
    fn set_partition_flags(&self, block: &str, flags: u64) -> Task<()>;
}

impl Backend for Disks {
    fn snapshot(&self) -> Arc<DisksState> {
        Disks::snapshot(self)
    }
    fn generation(&self) -> u64 {
        Disks::generation(self)
    }
    fn mount(&self, block: &str) -> Task<String> {
        Disks::mount(self, block)
    }
    fn unmount(&self, block: &str) -> Task<()> {
        Disks::unmount(self, block, false)
    }
    fn unlock(&self, block: &str, passphrase: &str) -> Task<String> {
        Disks::unlock(self, block, passphrase)
    }
    fn lock(&self, block: &str) -> Task<()> {
        Disks::lock(self, block)
    }
    fn set_swap(&self, block: &str, on: bool) -> Task<()> {
        Disks::set_swap(self, block, on)
    }
    fn power_off(&self, drive: &str) -> Task<()> {
        Disks::power_off(self, drive)
    }
    fn loop_setup(&self, file: &Path, read_only: bool) -> Task<String> {
        Disks::loop_setup(self, file, read_only)
    }
    fn loop_delete(&self, block: &str) -> Task<()> {
        Disks::loop_delete(self, block)
    }
    fn check(&self, block: &str) -> Task<bool> {
        Disks::check(self, block)
    }
    fn smart_update(&self, drive: &str) -> Task<()> {
        Disks::smart_update(self, drive)
    }
    fn smart_attributes(&self, drive: &str) -> Task<Vec<SmartAttribute>> {
        Disks::smart_attributes(self, drive)
    }
    fn smart_selftest(&self, drive: &str, test: SelfTest) -> Task<()> {
        Disks::smart_selftest(self, drive, test)
    }
    fn cancel_job(&self, job: &str) -> Task<()> {
        Disks::cancel_job(self, job)
    }
    fn can_format(&self, fs: &str) -> Task<(bool, String)> {
        Disks::can_format(self, fs)
    }
    fn create_partition_table(&self, confirm: Confirm<NewPartitionTable>, kind: TableKind) -> Task<()> {
        Disks::create_partition_table(self, confirm, kind)
    }
    fn create_partition(
        &self,
        table: &str,
        offset: u64,
        size: u64,
        type_id: Option<&str>,
        name: &str,
        format: Option<FormatSpec>,
    ) -> Task<String> {
        Disks::create_partition(self, table, offset, size, type_id, name, format)
    }
    fn delete_partition(&self, confirm: Confirm<DeletePartition>) -> Task<()> {
        Disks::delete_partition(self, confirm)
    }
    fn format(&self, confirm: Confirm<Format>, spec: FormatSpec) -> Task<()> {
        Disks::format(self, confirm, spec)
    }
    fn resize_partition(&self, confirm: Confirm<Resize>, size: u64) -> Task<()> {
        Disks::resize_partition(self, confirm, size)
    }
    fn resize_filesystem(&self, confirm: Confirm<Resize>, size: u64) -> Task<()> {
        Disks::resize_filesystem(self, confirm, size)
    }
    fn repair(&self, confirm: Confirm<Repair>) -> Task<bool> {
        Disks::repair(self, confirm)
    }
    fn set_label(&self, block: &str, label: &str) -> Task<()> {
        Disks::set_label(self, block, label)
    }
    fn set_partition_type(&self, block: &str, type_id: &str) -> Task<()> {
        Disks::set_partition_type(self, block, type_id)
    }
    fn set_partition_flags(&self, block: &str, flags: u64) -> Task<()> {
        Disks::set_partition_flags(self, block, flags)
    }
}
