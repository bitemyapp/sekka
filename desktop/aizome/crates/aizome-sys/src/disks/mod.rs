//! Disks, through UDisks2: the backend of Tansu (a GParted for Aizome),
//! Kura's volume list and the desktop's drive icons.
//!
//! [`Disks`] follows UDisks's `ObjectManager`, so the [`DisksState`]
//! snapshot always holds the drives (model, serial, size, rotation,
//! removable, ejectable, SMART health), block devices, partition tables and
//! partitions (types and flags by name), filesystems (type, label, UUID,
//! mount points, space), encrypted volumes, swap, loop devices and the jobs
//! in progress with their progress. Drives coming and going (for desktop
//! icons) show as snapshot changes; [`DisksState::volumes`] lists what the
//! desktop shows.
//!
//! # Operations
//!
//! Mounting, unmounting, ejecting, powering off, unlocking and locking,
//! swap, loop devices (mounting `.iso` images), labels, checking, SMART,
//! and creating partitions in free space don't destroy anything. Those that
//! do — formatting (also erasing and encrypting), new partition tables,
//! deleting, resizing and repairing — each take a [`Confirm`] token the UI
//! makes from the device the person agreed to, checked again just before
//! the operation runs (see [`confirm`]). Operations on a device that is in
//! use (mounted, or swap) are refused with a clear error rather than left
//! to fail inside UDisks.
//!
//! Long operations (formatting with erasure, checks, repairs, resizes) run
//! as long as they take: the task finishes when UDisks does, and the job's
//! progress is in the snapshot meanwhile ([`DisksState::jobs_for`]); a
//! cancelable job can be stopped with [`Disks::cancel_job`].
//!
//! Needs `services.udisks2.enable`. Changing system devices asks for
//! administrator rights through polkit; removable drives need none.

pub mod confirm;
mod model;
mod smart;
pub mod types;

pub use confirm::{Confirm, DeletePartition, Destructive, Format, NewPartitionTable, Repair, Resize};
pub use model::{
    BlockDevice, DisksState, Drive, Encrypted, Filesystem, FreeSpace, Health, Job, LoopDevice, Partition,
    PartitionTable, Rotation, Smart, Swap, Usage, Volume, VolumeChanges, format_size, short_name,
    system_volume_from_mounts, usage,
};
pub use smart::SmartAttribute;

use crate::bus::{self, Bus, Event, Objects, Opts, signals_from};
use crate::error::{Error, Result};
use crate::service::{Model, Stop, Task, Wake, service_handle};
use model::*;
use std::collections::HashMap;
use std::os::fd::AsFd;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use zbus::zvariant::{OwnedObjectPath, Value};

/// As long as the hardware takes (erasing a large disk takes hours).
const LONG: Opts = Opts::long(Duration::from_secs(2 * 24 * 3600));
/// Mounting may run a filesystem check first.
const MOUNT: Opts = Opts::long(Duration::from_secs(600));

/// How to erase a device while formatting it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Erase {
    /// Only write the new filesystem's structures (fast).
    #[default]
    Quick,
    /// Write zeros over the whole device (slow).
    Zeros,
    /// The ATA secure erase command (SATA drives that support it).
    AtaSecureErase,
    AtaSecureEraseEnhanced,
}

/// A new filesystem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatSpec {
    /// `ext4`, `vfat`, `exfat`, `ntfs`, `btrfs`, `xfs`, `swap`… or `empty`
    /// (wipe the signatures, leaving nothing).
    pub fs_type: String,
    pub label: String,
    pub erase: Erase,
    /// Encrypt with LUKS under this passphrase (`luks2` unless `luks1`).
    pub encrypt: Option<Encryption>,
    /// Make the person the owner of the new filesystem's root (so they can
    /// write to it without administrator rights).
    pub take_ownership: bool,
    /// Change the partition's type to match the filesystem.
    pub update_partition_type: bool,
}

impl FormatSpec {
    /// A filesystem of `fs_type` labelled `label`, owned by the person.
    pub fn new(fs_type: &str, label: &str) -> FormatSpec {
        FormatSpec {
            fs_type: fs_type.to_owned(),
            label: label.to_owned(),
            erase: Erase::Quick,
            encrypt: None,
            take_ownership: true,
            update_partition_type: true,
        }
    }

    fn options(&self) -> HashMap<&'static str, Value<'_>> {
        let mut options: HashMap<&'static str, Value<'_>> = HashMap::new();
        if !self.label.is_empty() {
            options.insert("label", Value::from(self.label.as_str()));
        }
        if self.take_ownership && self.fs_type != "swap" && self.fs_type != "empty" {
            options.insert("take-ownership", Value::from(true));
        }
        if self.update_partition_type {
            options.insert("update-partition-type", Value::from(true));
        }
        match self.erase {
            Erase::Quick => {}
            Erase::Zeros => {
                options.insert("erase", Value::from("zero"));
            }
            Erase::AtaSecureErase => {
                options.insert("erase", Value::from("ata-secure-erase"));
            }
            Erase::AtaSecureEraseEnhanced => {
                options.insert("erase", Value::from("ata-secure-erase-enhanced"));
            }
        }
        if let Some(encrypt) = &self.encrypt {
            options.insert("encrypt.passphrase", Value::from(encrypt.passphrase.as_str()));
            options.insert("encrypt.type", Value::from(if encrypt.luks1 { "luks1" } else { "luks2" }));
        }
        options
    }

    fn check(&self, supported: &[String]) -> Result<()> {
        if self.fs_type != "empty" && !supported.contains(&self.fs_type) {
            return Err(Error::InvalidArgument(format!(
                "This system can't make {} filesystems (it can make {})",
                self.fs_type,
                supported.join(", ")
            )));
        }
        if let Some(limit) = label_limit(&self.fs_type)
            && self.label.len() > limit
        {
            return Err(Error::InvalidArgument(format!("A {} label has at most {limit} bytes", self.fs_type)));
        }
        if self.encrypt.as_ref().is_some_and(|e| e.passphrase.is_empty()) {
            return Err(Error::InvalidArgument("The passphrase is empty".into()));
        }
        Ok(())
    }
}

/// LUKS encryption for a new filesystem.
#[derive(Clone, PartialEq, Eq)]
pub struct Encryption {
    pub passphrase: String,
    /// LUKS1 (for old bootloaders) rather than LUKS2.
    pub luks1: bool,
}

impl std::fmt::Debug for Encryption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Encryption").field("luks1", &self.luks1).finish_non_exhaustive()
    }
}

/// The longest label a filesystem takes, in bytes.
pub fn label_limit(fs_type: &str) -> Option<usize> {
    match fs_type {
        "vfat" => Some(11),
        "ext2" | "ext3" | "ext4" | "nilfs2" => Some(16),
        "xfs" => Some(12),
        "exfat" => Some(15),
        "ntfs" => Some(128),
        "btrfs" => Some(255),
        "f2fs" => Some(512),
        _ => None,
    }
}

/// A partition table's kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TableKind {
    /// GUID Partition Table: any size, 128 partitions.
    #[default]
    Gpt,
    /// Master Boot Record ("DOS"): up to 2 TiB, 4 primary partitions.
    Dos,
}

impl TableKind {
    fn id(self) -> &'static str {
        match self {
            TableKind::Gpt => "gpt",
            TableKind::Dos => "dos",
        }
    }
}

/// A self-test of a drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelfTest {
    Short,
    Extended,
}

/// UDisks2.
pub struct Disks {
    bus: Option<Bus>,
    model: Arc<Model<DisksState>>,
    stop: Stop,
}

service_handle!(Disks, DisksState);

fn empty() -> HashMap<&'static str, Value<'static>> {
    HashMap::new()
}

impl Disks {
    /// Follow UDisks on the system bus (starting it if need be).
    pub fn new(wake: Wake) -> Disks {
        Disks::with_bus(Bus::system().ok(), wake)
    }

    pub fn with_bus(bus: Option<Bus>, wake: Wake) -> Disks {
        let model = Arc::new(Model::new(DisksState::default(), wake));
        let stop = Stop::default();
        if let Some(bus) = bus.clone() {
            let follower = model.clone();
            let mut objects = Objects::default();
            let mut present = false;
            let mut dirty = false;
            bus::follow(bus, UDISKS, vec![signals_from(UDISKS)], true, stop.clone(), move |bus, event| match event {
                Event::Appeared => {
                    present = true;
                    objects = Objects::load(bus, UDISKS, ROOT).unwrap_or_else(|error| {
                        log::warn!("aizome-sys: reading UDisks's objects: {error}");
                        Objects::default()
                    });
                    dirty = true;
                }
                Event::Vanished => {
                    present = false;
                    objects = Objects::default();
                    follower.set_if_changed(DisksState::default());
                }
                Event::Signal(msg) => {
                    let applied = objects.apply(&msg);
                    if !applied.invalidated.is_empty() {
                        objects.refetch(bus, UDISKS, applied.invalidated);
                    }
                    dirty |= applied.changed;
                }
                Event::Idle => {
                    if present && dirty {
                        follower.set_if_changed(build(&objects));
                        dirty = false;
                    }
                }
            });
        }
        Disks { bus, model, stop }
    }

    /// Whether UDisks is running.
    pub fn available(&self) -> bool {
        self.model.get().available
    }

    fn task<T: Send + 'static>(&self, work: impl FnOnce(&Bus) -> Result<T> + Send + 'static) -> Task<T> {
        bus::spawn(&self.bus, "UDisks", self.model.wake(), work)
    }

    /// Re-read the state now (after an operation, so the next step sees
    /// its result without waiting for signals).
    fn refreshed(bus: &Bus) -> DisksState {
        Objects::load(bus, UDISKS, ROOT).map(|o| build(&o)).unwrap_or_default()
    }

    // --- Using volumes ----------------------------------------------------

    /// Mount a filesystem where UDisks chooses (`/run/media/USER/LABEL`);
    /// the result is the mount point.
    pub fn mount(&self, block: &str) -> Task<String> {
        let block = block.to_owned();
        self.task(move |bus| bus.call_with(MOUNT, UDISKS, &block, IFACE_FILESYSTEM, "Mount", &(empty(),)))
    }

    /// Unmount a filesystem; `force` detaches it even while files are open
    /// (a lazy unmount).
    pub fn unmount(&self, block: &str, force: bool) -> Task<()> {
        let block = block.to_owned();
        self.task(move |bus| unmount(bus, &block, force))
    }

    /// Unlock an encrypted volume; the result is its unlocked device.
    pub fn unlock(&self, block: &str, passphrase: &str) -> Task<String> {
        let block = block.to_owned();
        let passphrase = passphrase.to_owned();
        self.task(move |bus| {
            let unlocked: OwnedObjectPath =
                bus.call_with(Opts::ACTION, UDISKS, &block, IFACE_ENCRYPTED, "Unlock", &(passphrase, empty()))?;
            Ok(unlocked.to_string())
        })
    }

    /// Lock an encrypted volume, unmounting what is on it first.
    pub fn lock(&self, block: &str) -> Task<()> {
        let block = block.to_owned();
        self.task(move |bus| {
            let state = Disks::refreshed(bus);
            if let Some(cleartext) = state.cleartext(&block) {
                for mounted in std::iter::once(cleartext).chain(state.dependents(&cleartext.path)) {
                    if mounted.filesystem.as_ref().is_some_and(|f| !f.mount_points.is_empty()) {
                        unmount(bus, &mounted.path, false)?;
                    }
                }
            }
            bus.call_with(Opts::ACTION, UDISKS, &block, IFACE_ENCRYPTED, "Lock", &(empty(),))
        })
    }

    /// Change an encrypted volume's passphrase.
    pub fn change_passphrase(&self, block: &str, old: &str, new: &str) -> Task<()> {
        if new.is_empty() {
            return Task::ready(Err(Error::InvalidArgument("The passphrase is empty".into())));
        }
        let args = (old.to_owned(), new.to_owned(), empty());
        let block = block.to_owned();
        self.task(move |bus| bus.call_with(LONG, UDISKS, &block, IFACE_ENCRYPTED, "ChangePassphrase", &args))
    }

    /// Start or stop using a swap area.
    pub fn set_swap(&self, block: &str, on: bool) -> Task<()> {
        let block = block.to_owned();
        self.task(move |bus| {
            bus.call_with(Opts::ACTION, UDISKS, &block, IFACE_SWAP, if on { "Start" } else { "Stop" }, &(empty(),))
        })
    }

    /// Change a filesystem's label.
    pub fn set_label(&self, block: &str, label: &str) -> Task<()> {
        let state = self.snapshot();
        if let Some(limit) = state.block(block).and_then(|b| label_limit(&b.id_type))
            && label.len() > limit
        {
            return Task::ready(Err(Error::InvalidArgument(format!("This label has at most {limit} bytes"))));
        }
        let block = block.to_owned();
        let label = label.to_owned();
        self.task(move |bus| bus.call_with(LONG, UDISKS, &block, IFACE_FILESYSTEM, "SetLabel", &(label, empty())))
    }

    /// Eject a drive's media, unmounting and locking what is on it first.
    pub fn eject(&self, drive: &str) -> Task<()> {
        let drive = drive.to_owned();
        self.task(move |bus| {
            release_drive(bus, &drive)?;
            bus.call_with(Opts::ACTION, UDISKS, &drive, IFACE_DRIVE, "Eject", &(empty(),))
        })
    }

    /// "Safely remove": unmount and lock everything on the drive, then
    /// power it off so it can be unplugged.
    pub fn power_off(&self, drive: &str) -> Task<()> {
        let drive = drive.to_owned();
        self.task(move |bus| {
            release_drive(bus, &drive)?;
            bus.call_with(Opts::ACTION, UDISKS, &drive, IFACE_DRIVE, "PowerOff", &(empty(),))
        })
    }

    /// Attach an image file as a loop device (read-only unless asked).
    /// It remains attached until explicitly detached with [`Self::loop_delete`].
    pub fn loop_setup(&self, file: &Path, read_only: bool) -> Task<String> {
        let file = file.to_owned();
        self.task(move |bus| loop_setup(bus, &file, read_only))
    }

    /// Attach an image (an `.iso`) read-only and mount its filesystem; the
    /// result is the mount point. Images with partitions are attached and
    /// their first filesystem mounted. Once mounted, the loop device is
    /// detached automatically on unmount. Failed attempts release their loop.
    pub fn mount_image(&self, file: &Path) -> Task<String> {
        let file = file.to_owned();
        self.task(move |bus| {
            let device = loop_setup(bus, &file, true)?;
            let mut mounted_by_us = None;
            let result = (|| {
                // The kernel and UDisks need a moment to probe what is on it.
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    let state = Disks::refreshed(bus);
                    let candidates = std::iter::once(state.block(&device)).flatten().chain(state.dependents(&device));
                    if let Some(fs) = candidates.into_iter().find(|b| b.filesystem.is_some()) {
                        let mounted = if let Some(mounted) = fs.filesystem.as_ref().and_then(|f| f.mount_points.first())
                        {
                            mounted.clone()
                        } else {
                            let point =
                                bus.call_with(MOUNT, UDISKS, &fs.path, IFACE_FILESYSTEM, "Mount", &(empty(),))?;
                            mounted_by_us = Some(fs.path.clone());
                            point
                        };
                        // Autoclear drops a loop on its last close. The mount
                        // must hold it first, or it disappears before probing.
                        bus.call_with::<_, ()>(
                            Opts::ACTION,
                            UDISKS,
                            &device,
                            IFACE_LOOP,
                            "SetAutoclear",
                            &(true, empty()),
                        )?;
                        return Ok(mounted);
                    }
                    if Instant::now() > deadline {
                        return Err(Error::Failed("No filesystem found in the image".into()));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            })();
            result.map_err(|error| cleanup_failed_image(bus, &device, mounted_by_us.as_deref(), error))
        })
    }

    /// Detach a loop device.
    pub fn loop_delete(&self, block: &str) -> Task<()> {
        let block = block.to_owned();
        self.task(move |bus| bus.call_with(Opts::ACTION, UDISKS, &block, IFACE_LOOP, "Delete", &(empty(),)))
    }

    /// Check a filesystem without changing it (it must not be mounted);
    /// the result is whether it is consistent.
    pub fn check(&self, block: &str) -> Task<bool> {
        if self.snapshot().in_use(block) {
            return Task::ready(Err(in_use(block)));
        }
        let block = block.to_owned();
        self.task(move |bus| bus.call_with(LONG, UDISKS, &block, IFACE_FILESYSTEM, "Check", &(empty(),)))
    }

    /// Whether this system can make (and check, repair, resize) a
    /// filesystem: `(true, "")`, or `(false, "the missing program")`.
    pub fn can_format(&self, fs_type: &str) -> Task<(bool, String)> {
        let fs_type = fs_type.to_owned();
        self.task(move |bus| bus.call(UDISKS, MANAGER_PATH, IFACE_MANAGER, "CanFormat", &(fs_type,)))
    }

    /// Read a drive's SMART data again.
    pub fn smart_update(&self, drive: &str) -> Task<()> {
        let drive = drive.to_owned();
        let nvme =
            self.snapshot().drive(&drive).and_then(|d| d.smart.as_ref()).is_some_and(|s| s.bad_sectors.is_none());
        self.task(move |bus| {
            let iface = if nvme { IFACE_NVME } else { IFACE_ATA };
            bus.call_with(Opts::ACTION, UDISKS, &drive, iface, "SmartUpdate", &(empty(),))
        })
    }

    /// A drive's SMART attributes: ATA's numbered attributes, or NVMe's
    /// health log. Reading them changes nothing on the drive.
    pub fn smart_attributes(&self, drive: &str) -> Task<Vec<SmartAttribute>> {
        let drive = drive.to_owned();
        let nvme =
            self.snapshot().drive(&drive).and_then(|d| d.smart.as_ref()).is_some_and(|s| s.bad_sectors.is_none());
        self.task(move |bus| {
            if nvme {
                let log: HashMap<String, zbus::zvariant::OwnedValue> =
                    bus.call_with(Opts::ACTION, UDISKS, &drive, IFACE_NVME, "SmartGetAttributes", &(empty(),))?;
                Ok(smart::nvme(&log))
            } else {
                let rows: Vec<smart::AtaRow> =
                    bus.call_with(Opts::ACTION, UDISKS, &drive, IFACE_ATA, "SmartGetAttributes", &(empty(),))?;
                Ok(smart::ata(rows))
            }
        })
    }

    /// Start a drive self-test; its progress shows in the drive's SMART
    /// data.
    pub fn smart_selftest(&self, drive: &str, test: SelfTest) -> Task<()> {
        let drive = drive.to_owned();
        let nvme =
            self.snapshot().drive(&drive).and_then(|d| d.smart.as_ref()).is_some_and(|s| s.bad_sectors.is_none());
        self.task(move |bus| {
            let (iface, kind) = match (nvme, test) {
                (true, SelfTest::Short) => (IFACE_NVME, "short"),
                (true, SelfTest::Extended) => (IFACE_NVME, "extended"),
                (false, SelfTest::Short) => (IFACE_ATA, "short"),
                (false, SelfTest::Extended) => (IFACE_ATA, "extended"),
            };
            bus.call_with(Opts::ACTION, UDISKS, &drive, iface, "SmartSelftestStart", &(kind, empty()))
        })
    }

    /// Stop a job that can be stopped.
    pub fn cancel_job(&self, job: &str) -> Task<()> {
        let job = job.to_owned();
        self.task(move |bus| bus.call_with(Opts::ACTION, UDISKS, &job, IFACE_JOB, "Cancel", &(empty(),)))
    }

    // --- Partitions --------------------------------------------------------

    /// Make a partition in free space of the table on `table` (`offset`
    /// and `size` in bytes, inside one of [`DisksState::free_space`]), of
    /// `type_id` (a GUID or MBR type; by default one fitting `format`'s
    /// filesystem), named `name` (GPT only), and format it as `format`.
    /// The result is the new partition.
    pub fn create_partition(
        &self,
        table: &str,
        offset: u64,
        size: u64,
        type_id: Option<&str>,
        name: &str,
        format: Option<FormatSpec>,
    ) -> Task<String> {
        let state = self.snapshot();
        let Some(disk) = state.block(table) else {
            return Task::ready(Err(Error::NotFound(table.to_owned())));
        };
        let Some(kind) = disk.partition_table.as_ref().map(|t| t.kind.clone()) else {
            return Task::ready(Err(Error::InvalidArgument(format!("{} has no partition table", disk.device))));
        };
        let fits = state.free_space(table).iter().any(|f| offset >= f.offset && offset + size <= f.offset + f.size);
        if size == 0 || !fits {
            return Task::ready(Err(Error::InvalidArgument("The new partition must fit in free space".into())));
        }
        if let Some(spec) = &format
            && let Err(error) = spec.check(&state.supported_filesystems)
        {
            return Task::ready(Err(error));
        }
        if !name.is_empty() && kind != "gpt" {
            return Task::ready(Err(Error::InvalidArgument("Only GPT partitions have names".into())));
        }
        let type_id = type_id.map(str::to_owned).unwrap_or_else(|| {
            types::type_for_filesystem(&kind, format.as_ref().map_or("ext4", |f| f.fs_type.as_str())).to_owned()
        });
        let table = table.to_owned();
        let name = name.to_owned();
        self.task(move |bus| {
            let created: OwnedObjectPath = match &format {
                Some(spec) => bus.call_with(
                    LONG,
                    UDISKS,
                    &table,
                    IFACE_TABLE,
                    "CreatePartitionAndFormat",
                    &(offset, size, type_id.as_str(), name.as_str(), empty(), spec.fs_type.as_str(), spec.options()),
                )?,
                None => bus.call_with(
                    Opts::ACTION,
                    UDISKS,
                    &table,
                    IFACE_TABLE,
                    "CreatePartition",
                    &(offset, size, type_id.as_str(), name.as_str(), empty()),
                )?,
            };
            Ok(created.to_string())
        })
    }

    /// Change a partition's type (a GUID or MBR type).
    pub fn set_partition_type(&self, block: &str, type_id: &str) -> Task<()> {
        let args = (type_id.to_owned(), empty());
        let block = block.to_owned();
        self.task(move |bus| bus.call_with(Opts::ACTION, UDISKS, &block, IFACE_PARTITION, "SetType", &args))
    }

    /// Change a GPT partition's name.
    pub fn set_partition_name(&self, block: &str, name: &str) -> Task<()> {
        let args = (name.to_owned(), empty());
        let block = block.to_owned();
        self.task(move |bus| bus.call_with(Opts::ACTION, UDISKS, &block, IFACE_PARTITION, "SetName", &args))
    }

    /// Change a partition's flags (see [`types::flags`]).
    pub fn set_partition_flags(&self, block: &str, flags: u64) -> Task<()> {
        let block = block.to_owned();
        self.task(move |bus| {
            bus.call_with(Opts::ACTION, UDISKS, &block, IFACE_PARTITION, "SetFlags", &(flags, empty()))
        })
    }

    // --- Destructive --------------------------------------------------------

    /// Check a confirmation against the device now, and that nothing on it
    /// is in use; then run `work` on its path.
    fn destructive<Op: Destructive + 'static, T: Send + 'static>(
        &self,
        confirm: Confirm<Op>,
        work: impl FnOnce(&Bus, String, &DisksState) -> Result<T> + Send + 'static,
    ) -> Task<T> {
        self.task(move |bus| {
            // Decide on the device as it is now, not the cached snapshot.
            let state = Disks::refreshed(bus);
            let block = confirm.check(&state)?;
            if state.in_use(&block.path) {
                return Err(in_use(&block.device));
            }
            work(bus, block.path.clone(), &state)
        })
    }

    /// Make a new filesystem on a device (or wipe it, with `empty`), as
    /// confirmed: everything on it is lost.
    pub fn format(&self, confirm: Confirm<Format>, spec: FormatSpec) -> Task<()> {
        self.destructive(confirm, move |bus, block, state| {
            spec.check(&state.supported_filesystems)?;
            bus.call_with(LONG, UDISKS, &block, IFACE_BLOCK, "Format", &(spec.fs_type.as_str(), spec.options()))
        })
    }

    /// Make a new, empty partition table on a disk, as confirmed: every
    /// partition on it is lost.
    pub fn create_partition_table(&self, confirm: Confirm<NewPartitionTable>, kind: TableKind) -> Task<()> {
        self.destructive(confirm, move |bus, block, state| {
            if state.block(&block).is_some_and(|b| b.partition.is_some()) {
                return Err(Error::InvalidArgument("A partition table goes on a whole disk, not a partition".into()));
            }
            bus.call_with(LONG, UDISKS, &block, IFACE_BLOCK, "Format", &(kind.id(), empty()))
        })
    }

    /// Delete a partition, as confirmed.
    pub fn delete_partition(&self, confirm: Confirm<DeletePartition>) -> Task<()> {
        self.destructive(confirm, move |bus, block, state| {
            if state.block(&block).is_none_or(|b| b.partition.is_none()) {
                return Err(Error::InvalidArgument("Not a partition".into()));
            }
            bus.call_with(Opts::ACTION, UDISKS, &block, IFACE_PARTITION, "Delete", &(empty(),))
        })
    }

    /// Resize a partition to `size` bytes, as confirmed. Growing needs free
    /// space after it; shrinking loses what lies past the new end, so
    /// shrink the filesystem first.
    pub fn resize_partition(&self, confirm: Confirm<Resize>, size: u64) -> Task<()> {
        self.destructive(confirm, move |bus, block, state| {
            let partition = state
                .block(&block)
                .and_then(|b| b.partition.clone())
                .ok_or_else(|| Error::InvalidArgument("Not a partition".into()))?;
            if size > partition.size {
                let room = state
                    .free_space(&partition.table)
                    .iter()
                    .find(|f| {
                        f.offset >= partition.offset + partition.size
                            && f.offset <= partition.offset + partition.size + (1 << 20)
                    })
                    .map_or(0, |f| f.offset + f.size - (partition.offset + partition.size));
                if size - partition.size > room {
                    return Err(Error::InvalidArgument("There isn't that much free space after the partition".into()));
                }
            }
            bus.call_with(LONG, UDISKS, &block, IFACE_PARTITION, "Resize", &(size, empty()))
        })
    }

    /// Resize a filesystem to `size` bytes (0: to fill its partition), as
    /// confirmed.
    pub fn resize_filesystem(&self, confirm: Confirm<Resize>, size: u64) -> Task<()> {
        self.destructive(confirm, move |bus, block, _| {
            bus.call_with(LONG, UDISKS, &block, IFACE_FILESYSTEM, "Resize", &(size, empty()))
        })
    }

    /// Repair a filesystem, as confirmed; the result is whether it is
    /// consistent afterwards.
    pub fn repair(&self, confirm: Confirm<Repair>) -> Task<bool> {
        self.destructive(confirm, move |bus, block, _| {
            bus.call_with(LONG, UDISKS, &block, IFACE_FILESYSTEM, "Repair", &(empty(),))
        })
    }
}

impl Drop for Disks {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

fn in_use(device: &str) -> Error {
    Error::InvalidArgument(format!("{device} is in use: unmount it first"))
}

fn unmount(bus: &Bus, block: &str, force: bool) -> Result<()> {
    let mut options = empty();
    if force {
        options.insert("force", Value::from(true));
    }
    bus.call_with(Opts::ACTION, UDISKS, block, IFACE_FILESYSTEM, "Unmount", &(options,))
}

/// Unmount every filesystem on a drive and lock its encrypted volumes,
/// innermost first.
fn release_drive(bus: &Bus, drive: &str) -> Result<()> {
    let state = Disks::refreshed(bus);
    let mut on_drive: Vec<&BlockDevice> =
        state.blocks.iter().filter(|b| state.drive_of(b).is_some_and(|d| d.path == drive)).collect();
    // Unlocked devices and partitions before what holds them.
    on_drive.sort_by_key(|b| state.dependents(&b.path).len());
    for block in &on_drive {
        if block.filesystem.as_ref().is_some_and(|f| !f.mount_points.is_empty()) {
            unmount(bus, &block.path, false)?;
        }
        if block.swap.as_ref().is_some_and(|s| s.active) {
            bus.call_with::<_, ()>(Opts::ACTION, UDISKS, &block.path, IFACE_SWAP, "Stop", &(empty(),))?;
        }
    }
    for block in &on_drive {
        if block.encrypted.as_ref().is_some_and(|e| e.cleartext_device.is_some()) {
            bus.call_with::<_, ()>(Opts::ACTION, UDISKS, &block.path, IFACE_ENCRYPTED, "Lock", &(empty(),))?;
        }
    }
    Ok(())
}

fn loop_setup(bus: &Bus, file: &Path, read_only: bool) -> Result<String> {
    let handle = std::fs::OpenOptions::new().read(true).write(!read_only).open(file)?;
    if !handle.metadata()?.is_file() {
        return Err(Error::InvalidArgument(format!("{} is not a file", file.display())));
    }
    let mut options: HashMap<&str, Value> = HashMap::new();
    options.insert("read-only", Value::from(read_only));
    let fd = zbus::zvariant::Fd::from(handle.as_fd());
    let device: OwnedObjectPath =
        bus.call_with(Opts::ACTION, UDISKS, MANAGER_PATH, IFACE_MANAGER, "LoopSetup", &(fd, options))?;
    Ok(device.to_string())
}

/// Release only the loop this attempt created, and only a mount it made.
/// Keep the operation's error; also report any cleanup that needs attention.
fn cleanup_failed_image(bus: &Bus, device: &str, mounted_by_us: Option<&str>, error: Error) -> Error {
    let mut failures = Vec::new();
    if let Some(filesystem) = mounted_by_us
        && let Err(cleanup) = unmount(bus, filesystem, false)
    {
        failures.push(format!("could not unmount the image: {cleanup}"));
    }
    if let Err(cleanup) = bus.call_with::<_, ()>(Opts::ACTION, UDISKS, device, IFACE_LOOP, "Delete", &(empty(),))
        && !matches!(cleanup, Error::NotFound(_))
    {
        failures.push(format!("could not detach {device}: {cleanup}"));
    }
    if failures.is_empty() { error } else { Error::Failed(format!("{}; {}", error, failures.join("; "))) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_options_and_checks() {
        let mut spec = FormatSpec::new("vfat", "PHOTOS");
        spec.erase = Erase::Zeros;
        let options = spec.options();
        assert_eq!(options["label"], Value::from("PHOTOS"));
        assert_eq!(options["erase"], Value::from("zero"));
        assert_eq!(options["take-ownership"], Value::from(true));
        let supported = vec!["ext4".to_owned(), "vfat".to_owned()];
        assert!(spec.check(&supported).is_ok());
        spec.label = "MUCH TOO LONG".into();
        assert!(spec.check(&supported).is_err(), "FAT labels have 11 bytes");
        assert!(FormatSpec::new("zfs", "").check(&supported).is_err());
        assert!(FormatSpec::new("empty", "").check(&supported).is_ok());
        let mut secret = FormatSpec::new("ext4", "vault");
        secret.encrypt = Some(Encryption { passphrase: "hunter2".into(), luks1: false });
        assert_eq!(secret.options()["encrypt.type"], Value::from("luks2"));
        assert!(!format!("{secret:?}").contains("hunter2"), "passphrases stay out of logs");
    }
}
