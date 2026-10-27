//! What UDisks2 reports, as plain data, built from its objects; and the
//! questions Tansu and the desktop ask of it (a drive's devices, a table's
//! partitions and free space, the volumes to show as icons).

use super::types::{filesystem_name, flag_names, partition_type_name};
use crate::bus::{Objects, Props, PropsExt, value_str};
use std::time::Duration;

pub(crate) const UDISKS: &str = "org.freedesktop.UDisks2";
pub(crate) const ROOT: &str = "/org/freedesktop/UDisks2";
pub(crate) const MANAGER_PATH: &str = "/org/freedesktop/UDisks2/Manager";
pub(crate) const IFACE_MANAGER: &str = "org.freedesktop.UDisks2.Manager";
pub(crate) const IFACE_DRIVE: &str = "org.freedesktop.UDisks2.Drive";
pub(crate) const IFACE_ATA: &str = "org.freedesktop.UDisks2.Drive.Ata";
pub(crate) const IFACE_NVME: &str = "org.freedesktop.UDisks2.NVMe.Controller";
pub(crate) const IFACE_BLOCK: &str = "org.freedesktop.UDisks2.Block";
pub(crate) const IFACE_FILESYSTEM: &str = "org.freedesktop.UDisks2.Filesystem";
pub(crate) const IFACE_TABLE: &str = "org.freedesktop.UDisks2.PartitionTable";
pub(crate) const IFACE_PARTITION: &str = "org.freedesktop.UDisks2.Partition";
pub(crate) const IFACE_ENCRYPTED: &str = "org.freedesktop.UDisks2.Encrypted";
pub(crate) const IFACE_SWAP: &str = "org.freedesktop.UDisks2.Swapspace";
pub(crate) const IFACE_LOOP: &str = "org.freedesktop.UDisks2.Loop";
pub(crate) const IFACE_JOB: &str = "org.freedesktop.UDisks2.Job";

const MIB: u64 = 1 << 20;

/// Everything UDisks reports.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisksState {
    pub available: bool,
    pub version: String,
    /// Filesystems this system can make (`ext4`, `vfat`, `btrfs`…).
    pub supported_filesystems: Vec<String>,
    /// Drives in UDisks's order (internal first).
    pub drives: Vec<Drive>,
    /// Every block device, by device name.
    pub blocks: Vec<BlockDevice>,
    /// Operations in progress.
    pub jobs: Vec<Job>,
}

impl DisksState {
    pub fn drive(&self, path: &str) -> Option<&Drive> {
        self.drives.iter().find(|d| d.path == path)
    }

    pub fn block(&self, path: &str) -> Option<&BlockDevice> {
        self.blocks.iter().find(|b| b.path == path)
    }

    /// The block device with this device node (`/dev/sda1`).
    pub fn block_by_device(&self, device: &str) -> Option<&BlockDevice> {
        self.blocks.iter().find(|b| b.device == device || b.symlinks.iter().any(|s| s == device))
    }

    /// A drive's whole-disk block device.
    pub fn drive_block(&self, drive: &str) -> Option<&BlockDevice> {
        self.blocks.iter().find(|b| b.drive.as_deref() == Some(drive) && b.partition.is_none())
    }

    /// The partitions of a partition table's block, by offset.
    pub fn partitions(&self, table: &str) -> Vec<&BlockDevice> {
        let mut parts: Vec<&BlockDevice> =
            self.blocks.iter().filter(|b| b.partition.as_ref().is_some_and(|p| p.table == table)).collect();
        parts.sort_by_key(|b| b.partition.as_ref().map(|p| p.offset).unwrap_or(0));
        parts
    }

    /// The unlocked device of an encrypted block.
    pub fn cleartext(&self, encrypted: &str) -> Option<&BlockDevice> {
        let path = self.block(encrypted)?.encrypted.as_ref()?.cleartext_device.as_deref()?;
        self.block(path)
    }

    /// The blocks that depend on `path`: its partitions (if it has a
    /// table), its unlocked device, theirs in turn.
    pub fn dependents(&self, path: &str) -> Vec<&BlockDevice> {
        let mut found: Vec<&BlockDevice> = Vec::new();
        let mut queue = vec![path.to_owned()];
        while let Some(next) = queue.pop() {
            for block in &self.blocks {
                let child = block.partition.as_ref().is_some_and(|p| p.table == next)
                    || block.crypto_backing_device.as_deref() == Some(next.as_str());
                if child && !found.iter().any(|f| f.path == block.path) {
                    found.push(block);
                    queue.push(block.path.clone());
                }
            }
        }
        found
    }

    /// Whether `path` or anything on it is mounted or in use as swap.
    pub fn in_use(&self, path: &str) -> bool {
        let busy = |b: &BlockDevice| {
            b.filesystem.as_ref().is_some_and(|f| !f.mount_points.is_empty())
                || b.swap.as_ref().is_some_and(|s| s.active)
        };
        self.block(path).is_some_and(busy) || self.dependents(path).into_iter().any(busy)
    }

    /// The jobs working on an object.
    pub fn jobs_for(&self, path: &str) -> impl Iterator<Item = &Job> {
        self.jobs.iter().filter(move |j| j.objects.iter().any(|o| o == path))
    }

    /// Unallocated space in a partition table, aligned to 1 MiB: what a
    /// new partition can use. Space inside an extended (DOS) partition is
    /// reported with `logical` set.
    pub fn free_space(&self, table: &str) -> Vec<FreeSpace> {
        let Some(disk) = self.block(table) else { return Vec::new() };
        let Some(kind) = disk.partition_table.as_ref().map(|t| t.kind.clone()) else { return Vec::new() };
        let parts = self.partitions(table);
        // GPT keeps its backup header in the last 33 sectors.
        let end = if kind == "gpt" { disk.size.saturating_sub(33 * 512) } else { disk.size };
        let primary: Vec<(u64, u64)> = parts
            .iter()
            .filter_map(|b| b.partition.as_ref())
            .filter(|p| !p.is_contained)
            .map(|p| (p.offset, p.offset + p.size))
            .collect();
        let mut free = gaps(MIB, end, &primary, false);
        for container in parts.iter().filter_map(|b| b.partition.as_ref()).filter(|p| p.is_container) {
            let logical: Vec<(u64, u64)> = parts
                .iter()
                .filter_map(|b| b.partition.as_ref())
                .filter(|p| {
                    p.is_contained && p.offset >= container.offset && p.offset < container.offset + container.size
                })
                // Each logical partition has its EBR in the MiB before it.
                .map(|p| (p.offset.saturating_sub(MIB), p.offset + p.size))
                .collect();
            free.extend(gaps(container.offset + MIB, container.offset + container.size, &logical, true));
        }
        free.sort_by_key(|f| f.offset);
        free
    }

    /// The volumes to show on the desktop and in file managers' sidebars:
    /// filesystems (and locked encrypted volumes) on removable or external
    /// drives and loop devices (mounted images), not the system's own.
    pub fn volumes(&self) -> Vec<Volume> {
        let mut volumes: Vec<Volume> = self
            .blocks
            .iter()
            .filter(|b| self.visible_volume(b))
            .filter(|b| {
                let mountable = b.filesystem.is_some();
                let locked = b.encrypted.as_ref().is_some_and(|e| e.cleartext_device.is_none());
                mountable || locked
            })
            // An unlocked volume shows through its cleartext device.
            .filter(|b| b.encrypted.as_ref().is_none_or(|e| e.cleartext_device.is_none()))
            .map(|b| {
                let drive = self.drive_of(b);
                let image = self.loop_of(b);
                Volume {
                    block: b.path.clone(),
                    name: b.display_name(drive),
                    icon_name: b.icon_name(drive, image.is_some()),
                    mount_point: b.filesystem.as_ref().and_then(|f| f.mount_points.first().cloned()),
                    locked: b.encrypted.is_some(),
                    drive: drive.map(|d| d.path.clone()),
                    ejectable: drive.is_some_and(|d| d.ejectable),
                    can_power_off: drive.is_some_and(|d| d.can_power_off),
                    image_file: image.map(|l| l.backing_file.clone()),
                    system: false,
                }
            })
            .collect();
        volumes.sort_by(|a, b| a.name.cmp(&b.name));
        volumes
    }

    fn visible_volume(&self, block: &BlockDevice) -> bool {
        // A dm-crypt cleartext device has no Drive and UDisks can mark it
        // HintSystem even when its encrypted backing volume is removable.
        // Inherit that volume's policy, including nested cleartext mappings.
        // Ordinary partition hints stay authoritative; a hidden partition
        // must not become visible just because its whole disk is external.
        let mut current = block;
        let mut system = block.hint_system;
        let mut inherit_system = true;
        for _ in 0..=self.blocks.len() {
            if current.hint_ignore {
                return false;
            }
            if let Some(backing) = current.crypto_backing_device.as_deref() {
                let Some(parent) = self.block(backing) else { return false };
                current = parent;
                if inherit_system {
                    system = current.hint_system;
                }
            } else if let Some(partition) = &current.partition {
                inherit_system = false;
                let Some(parent) = self.block(&partition.table) else { return false };
                current = parent;
            } else {
                return !system;
            }
        }
        // Incomplete or cyclic ancestry must not expose an unknown device.
        false
    }

    /// The disk the system runs from, as the desktop shows it: the
    /// filesystem mounted at `/` (else the one holding `home`, where `/`
    /// is no disk, as in a container), named for its drive as the boards
    /// name it ("nvme0", "sda"), mounted where it is.
    pub fn system_volume(&self, home: &str) -> Option<Volume> {
        let mount_of = |b: &BlockDevice, want: &dyn Fn(&str) -> bool| -> Option<String> {
            b.filesystem.as_ref()?.mount_points.iter().filter(|m| want(m)).max_by_key(|m| m.len()).cloned()
        };
        let (block, mount) =
            self.blocks.iter().find_map(|b| mount_of(b, &|m| m == "/").map(|m| (b, m))).or_else(|| {
                self.blocks
                    .iter()
                    .filter_map(|b| mount_of(b, &|m| path_within(home, m)).map(|m| (b, m)))
                    .max_by_key(|(_, m)| m.len())
            })?;
        let drive = self.drive_of(block);
        let disk = drive.and_then(|d| self.drive_block(&d.path));
        let name = disk
            .and_then(|d| short_name(&d.device))
            .or_else(|| short_name(&block.device))
            .unwrap_or_else(|| block.display_name(drive));
        Some(Volume {
            block: block.path.clone(),
            name,
            icon_name: "drive-harddisk".into(),
            mount_point: Some(mount),
            drive: drive.map(|d| d.path.clone()),
            system: true,
            ..Volume::default()
        })
    }

    /// What the desktop shows: the system's disk first, then the other
    /// volumes (removable and external ones, and images).
    pub fn desktop_volumes(&self, home: &str) -> Vec<Volume> {
        let system = self.system_volume(home);
        let mut out: Vec<Volume> = system.iter().cloned().collect();
        out.extend(self.volumes().into_iter().filter(|v| system.as_ref().is_none_or(|s| s.block != v.block)));
        out
    }

    /// Volumes that appeared and disappeared since `previous` (a USB stick
    /// plugged in, an image detached), for desktop icons and "mount when
    /// plugged in" (a new volume whose block has `hint_auto`).
    pub fn volume_changes(&self, previous: &DisksState) -> VolumeChanges {
        let now = self.volumes();
        let before = previous.volumes();
        VolumeChanges {
            added: now.iter().filter(|v| !before.iter().any(|b| b.block == v.block)).cloned().collect(),
            removed: before.iter().filter(|b| !now.iter().any(|v| v.block == b.block)).cloned().collect(),
        }
    }

    /// The drive a block is on, through its encrypted backing device and
    /// partition table if need be.
    pub fn drive_of(&self, block: &BlockDevice) -> Option<&Drive> {
        let mut current = block;
        for _ in 0..8 {
            if let Some(drive) = current.drive.as_deref().and_then(|d| self.drive(d)) {
                return Some(drive);
            }
            let next = current
                .crypto_backing_device
                .as_deref()
                .or_else(|| current.partition.as_ref().map(|p| p.table.as_str()))?;
            current = self.block(next)?;
        }
        None
    }

    /// The loop device a block is on (a mounted image).
    pub fn loop_of<'a>(&'a self, block: &'a BlockDevice) -> Option<&'a LoopDevice> {
        let mut current = block;
        for _ in 0..8 {
            if let Some(found) = current.loop_device.as_ref() {
                return Some(found);
            }
            let next = current
                .crypto_backing_device
                .as_deref()
                .or_else(|| current.partition.as_ref().map(|p| p.table.as_str()))?;
            current = self.block(next)?;
        }
        None
    }
}

/// Unallocated stretches between `used` ranges in `[start, end)`, aligned
/// to 1 MiB and at least 1 MiB long.
fn gaps(start: u64, end: u64, used: &[(u64, u64)], logical: bool) -> Vec<FreeSpace> {
    let mut used = used.to_vec();
    used.sort();
    let mut free = Vec::new();
    let mut cursor = start;
    let mut push = |from: u64, to: u64| {
        let from = from.div_ceil(MIB) * MIB;
        let to = to / MIB * MIB;
        if to > from && to - from >= MIB {
            free.push(FreeSpace { offset: from, size: to - from, logical });
        }
    };
    for (from, to) in used {
        if from > cursor {
            push(cursor, from.min(end));
        }
        cursor = cursor.max(to);
    }
    if end > cursor {
        push(cursor, end);
    }
    free
}

/// What [`DisksState::volume_changes`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VolumeChanges {
    pub added: Vec<Volume>,
    pub removed: Vec<Volume>,
}

/// Room for a new partition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FreeSpace {
    pub offset: u64,
    pub size: u64,
    /// Inside an extended (DOS) partition: a new partition there is logical.
    pub logical: bool,
}

/// A volume for the desktop.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Volume {
    /// The block device to mount (or unlock).
    pub block: String,
    pub name: String,
    pub icon_name: String,
    pub mount_point: Option<String>,
    /// Encrypted and not unlocked.
    pub locked: bool,
    pub drive: Option<String>,
    pub ejectable: bool,
    pub can_power_off: bool,
    /// For a mounted image, its file.
    pub image_file: Option<String>,
    /// The disk the system runs from: opened at its mount point, never
    /// unmounted or ejected.
    pub system: bool,
}

/// A drive.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drive {
    pub path: String,
    pub vendor: String,
    pub model: String,
    pub revision: String,
    pub serial: String,
    pub wwn: String,
    pub id: String,
    pub size: u64,
    pub rotation: Rotation,
    /// The media can be removed (a card reader, an optical drive).
    pub media_removable: bool,
    pub media_available: bool,
    /// The drive itself can be unplugged (USB).
    pub removable: bool,
    pub ejectable: bool,
    pub can_power_off: bool,
    /// `usb`, `sdio`, `ieee1394`, or empty for internal.
    pub connection_bus: String,
    pub optical: bool,
    pub optical_blank: bool,
    /// UDisks's order.
    pub sort_key: String,
    pub smart: Option<Smart>,
}

impl Drive {
    /// "Samsung MZVL8512HFLU-00BLL" from vendor and model.
    pub fn display_name(&self) -> String {
        match (self.vendor.trim(), self.model.trim()) {
            ("", "") => "Drive".into(),
            ("", model) => model.replace('_', " "),
            (vendor, "") => vendor.to_owned(),
            (vendor, model) if model.to_lowercase().starts_with(&vendor.to_lowercase()) => model.replace('_', " "),
            (vendor, model) => format!("{vendor} {}", model.replace('_', " ")),
        }
    }

    /// External (USB and the like) rather than built in.
    pub fn external(&self) -> bool {
        self.removable || !self.connection_bus.is_empty()
    }
}

/// How a drive spins.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rotation {
    #[default]
    Unknown,
    /// No moving parts (SSD, NVMe, flash).
    SolidState,
    /// Rotations per minute.
    Rpm(u32),
}

/// SMART health.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Smart {
    pub supported: bool,
    pub enabled: bool,
    /// The drive predicts its own failure.
    pub failing: bool,
    pub temperature_celsius: Option<f64>,
    pub power_on_hours: Option<u64>,
    /// ATA: bad sectors; attributes failing now and in the past.
    pub bad_sectors: Option<u64>,
    pub attributes_failing: Option<u64>,
    pub attributes_failed_in_past: Option<u64>,
    /// NVMe critical warnings (`spare`, `temperature`, `degraded`…).
    pub critical_warnings: Vec<String>,
    /// The last self-test's result (`success`, `inprogress`, `aborted`…).
    pub selftest_status: String,
    pub selftest_percent_remaining: Option<u32>,
    /// When the data was read (seconds since 1970).
    pub updated: u64,
}

/// A summary of [`Smart`] for a badge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Health {
    #[default]
    Unknown,
    Good,
    /// Something to watch: bad sectors, attributes that failed before.
    Warning,
    /// The drive says it is failing: back up now.
    Failing,
}

impl Smart {
    pub fn health(&self) -> Health {
        if !self.supported || !self.enabled {
            return Health::Unknown;
        }
        if self.failing || self.attributes_failing.is_some_and(|n| n > 0) || !self.critical_warnings.is_empty() {
            return Health::Failing;
        }
        if self.bad_sectors.is_some_and(|n| n > 0) || self.attributes_failed_in_past.is_some_and(|n| n > 0) {
            return Health::Warning;
        }
        Health::Good
    }
}

/// A block device.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockDevice {
    pub path: String,
    /// `/dev/nvme0n1p2`.
    pub device: String,
    pub preferred_device: String,
    pub symlinks: Vec<String>,
    pub size: u64,
    pub read_only: bool,
    pub drive: Option<String>,
    /// `filesystem`, `crypto`, `raid`, `other`, or empty.
    pub id_usage: String,
    /// `ext4`, `vfat`, `crypto_LUKS`, `swap`…
    pub id_type: String,
    pub id_version: String,
    pub id_label: String,
    pub id_uuid: String,
    /// Part of the system (UDisks asks for administrator rights).
    pub hint_system: bool,
    /// Not to be shown.
    pub hint_ignore: bool,
    /// To be mounted automatically when it appears.
    pub hint_auto: bool,
    pub hint_name: String,
    pub hint_icon_name: String,
    /// For an unlocked device, the encrypted one.
    pub crypto_backing_device: Option<String>,
    pub filesystem: Option<Filesystem>,
    pub partition_table: Option<PartitionTable>,
    pub partition: Option<Partition>,
    pub encrypted: Option<Encrypted>,
    pub swap: Option<Swap>,
    pub loop_device: Option<LoopDevice>,
}

impl BlockDevice {
    /// The name to show: the label, the hint, else size and kind
    /// ("32 GB Volume").
    pub fn display_name(&self, drive: Option<&Drive>) -> String {
        if !self.hint_name.is_empty() {
            return self.hint_name.clone();
        }
        if !self.id_label.is_empty() {
            return self.id_label.clone();
        }
        if let Some(partition) = &self.partition
            && !partition.name.is_empty()
        {
            return partition.name.clone();
        }
        let size = format_size(self.size);
        match (&self.encrypted, drive) {
            (Some(_), _) => format!("{size} Encrypted"),
            (None, Some(drive)) if drive.media_removable && self.partition.is_none() => {
                format!("{size} {}", drive.display_name())
            }
            _ => format!("{size} Volume"),
        }
    }

    /// The icon: UDisks's hint, else from the drive (removable media,
    /// optical, USB) or the image.
    pub fn icon_name(&self, drive: Option<&Drive>, image: bool) -> String {
        if !self.hint_icon_name.is_empty() {
            return self.hint_icon_name.clone();
        }
        if image {
            return "media-optical".into();
        }
        match drive {
            Some(d) if d.optical => "media-optical".into(),
            Some(d) if d.media_removable => "media-removable".into(),
            Some(d) if d.external() => "drive-removable-media".into(),
            _ => "drive-harddisk".into(),
        }
    }

    /// What the content is, for people ("Ext4", "LUKS encryption").
    pub fn content(&self) -> String {
        if let Some(table) = &self.partition_table {
            return if table.kind == "gpt" { "GUID partition table".into() } else { "Master boot record".into() };
        }
        if self.id_type.is_empty() {
            return if self.id_usage.is_empty() { "Unknown".into() } else { self.id_usage.clone() };
        }
        filesystem_name(&self.id_type).to_owned()
    }
}

/// A mountable filesystem.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filesystem {
    pub mount_points: Vec<String>,
    /// The filesystem's own size, when UDisks knows it.
    pub size: Option<u64>,
    /// Space, when mounted.
    pub usage: Option<Usage>,
}

/// A mounted filesystem's space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub total: u64,
    pub used: u64,
    /// Free for anyone.
    pub free: u64,
    /// Free for unprivileged users.
    pub available: u64,
}

impl Usage {
    pub fn used_fraction(&self) -> f64 {
        if self.total == 0 { 0.0 } else { self.used as f64 / self.total as f64 }
    }
}

/// Space on the filesystem mounted at `mount_point` (`statvfs`).
pub fn usage(mount_point: &str) -> Option<Usage> {
    let path = std::ffi::CString::new(mount_point).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: a valid C string and a statvfs struct to fill.
    if unsafe { libc::statvfs(path.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let fragment = st.f_frsize as u64;
    let total = st.f_blocks as u64 * fragment;
    let free = st.f_bfree as u64 * fragment;
    Some(Usage { total, used: total.saturating_sub(free), free, available: st.f_bavail as u64 * fragment })
}

/// A partition table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PartitionTable {
    /// `gpt` or `dos`.
    pub kind: String,
    pub partitions: Vec<String>,
}

/// A partition.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Partition {
    pub number: u32,
    /// The type GUID (GPT) or byte (`0x83`, DOS).
    pub type_id: String,
    pub type_name: Option<&'static str>,
    pub flags: u64,
    pub flag_names: Vec<&'static str>,
    pub offset: u64,
    pub size: u64,
    /// The GPT partition name.
    pub name: String,
    pub uuid: String,
    /// The block with the table.
    pub table: String,
    /// A DOS extended partition.
    pub is_container: bool,
    /// A DOS logical partition.
    pub is_contained: bool,
}

/// An encrypted (LUKS) device.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Encrypted {
    /// The unlocked device, when unlocked.
    pub cleartext_device: Option<String>,
    /// `luks1`, `luks2`, `bitlk`…
    pub hint_type: String,
}

/// A swap area.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Swap {
    pub active: bool,
}

/// A loop device (an image file attached as a disk).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoopDevice {
    pub backing_file: String,
    /// Detached by itself once nothing uses it.
    pub autoclear: bool,
    pub setup_by_uid: u32,
}

/// An operation in progress.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Job {
    pub path: String,
    /// `format-mkfs`, `filesystem-mount`, `partition-delete`…
    pub operation: String,
    /// 0.0–1.0, when known.
    pub progress: Option<f64>,
    /// Bytes processed, and the rate in bytes per second.
    pub bytes: u64,
    pub rate: u64,
    /// Expected time left, when known.
    pub remaining: Option<Duration>,
    /// The objects it works on.
    pub objects: Vec<String>,
    pub cancelable: bool,
    pub started_by_uid: u32,
}

/// "512 GB" (decimal units, as drives are sold).
/// Whether `path` is `dir` or inside it.
fn path_within(path: &str, dir: &str) -> bool {
    path == dir || dir == "/" || path.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'))
}

/// A disk's short name, as the desktop labels it: `/dev/nvme0n1p2` →
/// `nvme0`, `/dev/sda2` → `sda`, `/dev/mmcblk0p1` → `mmcblk0`; other
/// devices by their own name (`/dev/mapper/root` → `root`).
pub fn short_name(device: &str) -> Option<String> {
    let dev = device.strip_prefix("/dev/")?;
    let dev = dev.rsplit('/').next().unwrap_or(dev);
    if dev.is_empty() {
        return None;
    }
    for prefix in ["nvme", "mmcblk"] {
        if let Some(rest) = dev.strip_prefix(prefix) {
            let n: String = rest.chars().take_while(char::is_ascii_digit).collect();
            return Some(format!("{prefix}{n}"));
        }
    }
    if ["sd", "vd", "hd", "xvd"].iter().any(|p| dev.starts_with(p)) {
        return Some(dev.trim_end_matches(|c: char| c.is_ascii_digit()).to_owned());
    }
    Some(dev.to_owned())
}

/// The system's disk without UDisks, from `/proc/self/mountinfo`'s text:
/// the device mounted at `/` (else the one holding `home`), its name
/// found through sysfs (`/sys/class/block`, the root given) past device
/// mapper layers (LUKS, LVM) to the disk.
pub fn system_volume_from_mounts(mountinfo: &str, home: &str, sys_block: &std::path::Path) -> Option<Volume> {
    // Fields: id parent major:minor root mount-point options… - type source …
    let mounts: Vec<(&str, &str)> = mountinfo
        .lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let mount = f.nth(4)?;
            let mut rest = f.skip_while(|x| *x != "-");
            rest.next()?;
            rest.next()?;
            Some((mount, rest.next()?))
        })
        .filter(|(_, source)| source.starts_with("/dev/"))
        .collect();
    let (mount, source) = mounts
        .iter()
        .find(|(m, _)| *m == "/")
        .or_else(|| mounts.iter().filter(|(m, _)| path_within(home, m)).max_by_key(|(m, _)| m.len()))?;
    let name = disk_of(source, sys_block).or_else(|| short_name(source))?;
    Some(Volume {
        block: (*source).to_owned(),
        name,
        icon_name: "drive-harddisk".into(),
        mount_point: Some((*mount).to_owned()),
        system: true,
        ..Volume::default()
    })
}

/// The disk under a device, through sysfs: a device mapper device's
/// `slaves`, then a partition's parent.
fn disk_of(device: &str, sys_block: &std::path::Path) -> Option<String> {
    let mut name = std::fs::canonicalize(device)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| device.rsplit('/').next().unwrap_or(device).to_owned());
    for _ in 0..8 {
        let slaves = sys_block.join(&name).join("slaves");
        match std::fs::read_dir(&slaves).ok().and_then(|mut d| d.next()).and_then(Result::ok) {
            Some(s) => name = s.file_name().to_string_lossy().into_owned(),
            None => break,
        }
    }
    let entry = sys_block.join(&name);
    if entry.join("partition").exists()
        && let Ok(real) = std::fs::canonicalize(&entry)
        && let Some(parent) = real.parent().and_then(|p| p.file_name())
    {
        name = parent.to_string_lossy().into_owned();
    }
    short_name(&format!("/dev/{name}"))
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "kB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 || value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn drive(path: &str, p: &Props, ata: Option<&Props>, nvme: Option<&Props>) -> Drive {
    let rotation = match p.opt_i64("RotationRate") {
        Some(0) => Rotation::SolidState,
        Some(n) if n > 0 => Rotation::Rpm(u32::try_from(n).unwrap_or(u32::MAX)),
        _ if nvme.is_some() => Rotation::SolidState,
        _ => Rotation::Unknown,
    };
    let kelvin = |k: f64| (k > 0.0).then_some(k - 273.15);
    let smart = if let Some(a) = ata {
        Some(Smart {
            supported: a.bool("SmartSupported"),
            enabled: a.bool("SmartEnabled"),
            failing: a.bool("SmartFailing"),
            temperature_celsius: kelvin(a.f64("SmartTemperature")),
            power_on_hours: Some(a.u64("SmartPowerOnSeconds") / 3600).filter(|&h| h > 0),
            bad_sectors: a.opt_i64("SmartNumBadSectors").and_then(|n| u64::try_from(n).ok()),
            attributes_failing: a.opt_i64("SmartNumAttributesFailing").and_then(|n| u64::try_from(n).ok()),
            attributes_failed_in_past: a
                .opt_i64("SmartNumAttributesFailedInThePast")
                .and_then(|n| u64::try_from(n).ok()),
            critical_warnings: Vec::new(),
            selftest_status: a.str("SmartSelftestStatus"),
            selftest_percent_remaining: a.opt_i64("SmartSelftestPercentRemaining").and_then(|n| u32::try_from(n).ok()),
            updated: a.u64("SmartUpdated"),
        })
    } else {
        nvme.map(|n| Smart {
            supported: true,
            enabled: true,
            failing: false,
            temperature_celsius: kelvin(n.f64("SmartTemperature")),
            power_on_hours: n.opt_i64("SmartPowerOnHours").and_then(|h| u64::try_from(h).ok()),
            bad_sectors: None,
            attributes_failing: None,
            attributes_failed_in_past: None,
            critical_warnings: n.strings("SmartCriticalWarning"),
            selftest_status: n.str("SmartSelftestStatus"),
            selftest_percent_remaining: n.opt_i64("SmartSelftestPercentRemaining").and_then(|p| u32::try_from(p).ok()),
            updated: n.u64("SmartUpdated"),
        })
    };
    Drive {
        path: path.to_owned(),
        vendor: p.str("Vendor"),
        model: p.str("Model"),
        revision: p.str("Revision"),
        serial: p.str("Serial"),
        wwn: p.str("WWN"),
        id: p.str("Id"),
        size: p.u64("Size"),
        rotation,
        media_removable: p.bool("MediaRemovable"),
        media_available: p.bool("MediaAvailable"),
        removable: p.bool("Removable"),
        ejectable: p.bool("Ejectable"),
        can_power_off: p.bool("CanPowerOff"),
        connection_bus: p.str("ConnectionBus"),
        optical: p.bool("Optical"),
        optical_blank: p.bool("OpticalBlank"),
        sort_key: p.str("SortKey"),
        smart,
    }
}

fn block(objects: &Objects, path: &str, p: &Props) -> BlockDevice {
    let get = |iface: &str| objects.get(path, iface);
    let table_kind = get(IFACE_PARTITION)
        .and_then(|part| part.path("Table"))
        .and_then(|t| objects.get(&t, IFACE_TABLE))
        .map(|t| t.str("Type"))
        .unwrap_or_default();
    BlockDevice {
        path: path.to_owned(),
        device: p.bytestring("Device"),
        preferred_device: p.bytestring("PreferredDevice"),
        symlinks: p.bytestrings("Symlinks"),
        size: p.u64("Size"),
        read_only: p.bool("ReadOnly"),
        drive: p.path("Drive"),
        id_usage: p.str("IdUsage"),
        id_type: p.str("IdType"),
        id_version: p.str("IdVersion"),
        id_label: p.str("IdLabel"),
        id_uuid: p.str("IdUUID"),
        hint_system: p.bool("HintSystem"),
        hint_ignore: p.bool("HintIgnore"),
        hint_auto: p.bool("HintAuto"),
        hint_name: p.str("HintName"),
        hint_icon_name: p.str("HintIconName"),
        crypto_backing_device: p.path("CryptoBackingDevice"),
        filesystem: get(IFACE_FILESYSTEM).map(|f| {
            let mount_points = f.bytestrings("MountPoints");
            Filesystem {
                usage: mount_points.first().and_then(|m| usage(m)),
                size: Some(f.u64("Size")).filter(|&s| s > 0),
                mount_points,
            }
        }),
        partition_table: get(IFACE_TABLE)
            .map(|t| PartitionTable { kind: t.str("Type"), partitions: t.strings("Partitions") }),
        partition: get(IFACE_PARTITION).map(|part| {
            let type_id = part.str("Type");
            let flags = part.u64("Flags");
            Partition {
                number: part.u32("Number"),
                type_name: partition_type_name(&type_id),
                type_id,
                flags,
                flag_names: flag_names(&table_kind, flags),
                offset: part.u64("Offset"),
                size: part.u64("Size"),
                name: part.str("Name"),
                uuid: part.str("UUID"),
                table: part.path("Table").unwrap_or_default(),
                is_container: part.bool("IsContainer"),
                is_contained: part.bool("IsContained"),
            }
        }),
        encrypted: get(IFACE_ENCRYPTED)
            .map(|e| Encrypted { cleartext_device: e.path("CleartextDevice"), hint_type: e.str("HintEncryptionType") }),
        swap: get(IFACE_SWAP).map(|s| Swap { active: s.bool("Active") }),
        loop_device: get(IFACE_LOOP).map(|l| LoopDevice {
            backing_file: l.bytestring("BackingFile"),
            autoclear: l.bool("Autoclear"),
            setup_by_uid: l.u32("SetupByUID"),
        }),
    }
}

fn job(path: &str, p: &Props) -> Job {
    let now =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros() as u64).unwrap_or(0);
    let end = p.u64("ExpectedEndTime");
    Job {
        path: path.to_owned(),
        operation: p.str("Operation"),
        progress: p.bool("ProgressValid").then(|| p.f64("Progress").clamp(0.0, 1.0)),
        bytes: p.u64("Bytes"),
        rate: p.u64("Rate"),
        remaining: (end > now).then(|| Duration::from_micros(end - now)),
        objects: p.strings("Objects"),
        cancelable: p.bool("Cancelable"),
        started_by_uid: p.u32("StartedByUID"),
    }
}

/// Build the state from UDisks's objects.
pub(crate) fn build(objects: &Objects) -> DisksState {
    let empty = Props::new();
    let manager = objects.get(MANAGER_PATH, IFACE_MANAGER).unwrap_or(&empty);
    let mut drives: Vec<Drive> = objects
        .with(IFACE_DRIVE)
        .map(|(path, p)| drive(path, p, objects.get(path, IFACE_ATA), objects.get(path, IFACE_NVME)))
        .collect();
    drives.sort_by(|a, b| a.sort_key.cmp(&b.sort_key).then_with(|| a.path.cmp(&b.path)));
    let mut blocks: Vec<BlockDevice> = objects.with(IFACE_BLOCK).map(|(path, p)| block(objects, path, p)).collect();
    blocks.sort_by(|a, b| natural(&a.device, &b.device));
    let jobs = objects.with(IFACE_JOB).map(|(path, p)| job(path, p)).collect();
    DisksState {
        available: true,
        version: manager.str("Version"),
        supported_filesystems: manager.strings("SupportedFilesystems"),
        drives,
        blocks,
        jobs,
    }
}

/// `sda2` before `sda10`.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let split = |s: &str| {
        let digits = s.len() - s.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        let (head, tail) = s.split_at(s.len() - digits);
        (head.to_owned(), tail.parse::<u64>().unwrap_or(0))
    };
    split(a).cmp(&split(b))
}

/// The string in a variant, for options read back in tests.
#[allow(dead_code)]
pub(crate) fn as_str(v: &zbus::zvariant::Value<'_>) -> Option<String> {
    value_str(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::owned;
    use zbus::zvariant::{ObjectPath, OwnedValue};

    fn put(objects: &mut Objects, path: &str, iface: &str, entries: Vec<(&str, OwnedValue)>) {
        objects
            .map
            .entry(path.to_owned())
            .or_default()
            .insert(iface.to_owned(), entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect());
    }

    fn op(p: &str) -> OwnedValue {
        owned(ObjectPath::try_from(p.to_owned()).unwrap())
    }

    fn bytes(s: &str) -> OwnedValue {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        owned(v)
    }

    const GIB: u64 = 1 << 30;

    /// A 100 GiB USB disk: GPT with a 512 MiB ESP and a 50 GiB ext4 data
    /// partition (mounted), then free space; and a 64 GiB internal NVMe.
    fn fixture() -> Objects {
        let mut o = Objects::default();
        let b = "/org/freedesktop/UDisks2/block_devices/";
        let d = "/org/freedesktop/UDisks2/drives/";
        put(
            &mut o,
            MANAGER_PATH,
            IFACE_MANAGER,
            vec![("Version", owned("2.11.2")), ("SupportedFilesystems", owned(vec!["ext4", "vfat"]))],
        );
        put(
            &mut o,
            &format!("{d}usb"),
            IFACE_DRIVE,
            vec![
                ("Vendor", owned("SanDisk")),
                ("Model", owned("Extreme")),
                ("Size", owned(100 * GIB)),
                ("Removable", owned(true)),
                ("Ejectable", owned(true)),
                ("CanPowerOff", owned(true)),
                ("ConnectionBus", owned("usb")),
                ("SortKey", owned("01hotplug")),
            ],
        );
        put(
            &mut o,
            &format!("{d}nvme"),
            IFACE_DRIVE,
            vec![("Model", owned("Samsung SSD")), ("SortKey", owned("00coldplug"))],
        );
        put(
            &mut o,
            &format!("{d}nvme"),
            IFACE_NVME,
            vec![
                ("SmartTemperature", owned(318u16)),
                ("SmartPowerOnHours", owned(172u64)),
                ("SmartCriticalWarning", owned(Vec::<String>::new())),
            ],
        );
        put(
            &mut o,
            &format!("{b}sdb"),
            IFACE_BLOCK,
            vec![("Device", bytes("/dev/sdb")), ("Size", owned(100 * GIB)), ("Drive", op(&format!("{d}usb")))],
        );
        put(&mut o, &format!("{b}sdb"), IFACE_TABLE, vec![("Type", owned("gpt"))]);
        put(
            &mut o,
            &format!("{b}sdb1"),
            IFACE_BLOCK,
            vec![
                ("Device", bytes("/dev/sdb1")),
                ("Size", owned(512 * MIB)),
                ("IdType", owned("vfat")),
                ("IdUsage", owned("filesystem")),
                ("IdLabel", owned("ESP")),
            ],
        );
        put(
            &mut o,
            &format!("{b}sdb1"),
            IFACE_PARTITION,
            vec![
                ("Number", owned(1u32)),
                ("Type", owned("c12a7328-f81f-11d2-ba4b-00a0c93ec93b")),
                ("Flags", owned(1u64)),
                ("Offset", owned(MIB)),
                ("Size", owned(512 * MIB)),
                ("Table", op(&format!("{b}sdb"))),
            ],
        );
        put(&mut o, &format!("{b}sdb1"), IFACE_FILESYSTEM, vec![("MountPoints", owned(Vec::<Vec<u8>>::new()))]);
        put(
            &mut o,
            &format!("{b}sdb10"),
            IFACE_BLOCK,
            vec![
                ("Device", bytes("/dev/sdb10")),
                ("Size", owned(50 * GIB)),
                ("IdType", owned("ext4")),
                ("IdUsage", owned("filesystem")),
            ],
        );
        put(
            &mut o,
            &format!("{b}sdb10"),
            IFACE_PARTITION,
            vec![
                ("Number", owned(10u32)),
                ("Type", owned("0fc63daf-8483-4772-8e79-3d69d8477de4")),
                ("Offset", owned(513 * MIB)),
                ("Size", owned(50 * GIB)),
                ("Table", op(&format!("{b}sdb"))),
            ],
        );
        let mut mount = b"/".to_vec();
        mount.push(0);
        put(&mut o, &format!("{b}sdb10"), IFACE_FILESYSTEM, vec![("MountPoints", owned(vec![mount]))]);
        put(
            &mut o,
            &format!("{b}sdb2"),
            IFACE_BLOCK,
            vec![
                ("Device", bytes("/dev/sdb2")),
                ("Size", owned(GIB)),
                ("IdType", owned("crypto_LUKS")),
                ("IdUsage", owned("crypto")),
            ],
        );
        put(
            &mut o,
            &format!("{b}sdb2"),
            IFACE_PARTITION,
            vec![
                ("Number", owned(2u32)),
                ("Offset", owned(60 * GIB)),
                ("Size", owned(GIB)),
                ("Table", op(&format!("{b}sdb"))),
            ],
        );
        put(&mut o, &format!("{b}sdb2"), IFACE_ENCRYPTED, vec![("CleartextDevice", op("/"))]);
        put(
            &mut o,
            "/org/freedesktop/UDisks2/jobs/7",
            IFACE_JOB,
            vec![
                ("Operation", owned("format-mkfs")),
                ("Progress", owned(0.25f64)),
                ("ProgressValid", owned(true)),
                ("Objects", owned(vec![ObjectPath::try_from(format!("{b}sdb2")).unwrap()])),
                ("Cancelable", owned(true)),
            ],
        );
        o
    }

    #[test]
    fn drives_blocks_and_partitions() {
        let state = build(&fixture());
        assert_eq!(state.version, "2.11.2");
        let names: Vec<String> = state.drives.iter().map(|d| d.display_name()).collect();
        assert_eq!(names, ["Samsung SSD", "SanDisk Extreme"], "UDisks's order");
        let nvme = &state.drives[0];
        assert_eq!(nvme.rotation, Rotation::SolidState);
        let smart = nvme.smart.as_ref().unwrap();
        assert!((smart.temperature_celsius.unwrap() - 44.85).abs() < 0.01);
        assert_eq!(smart.health(), Health::Good);
        let devices: Vec<&str> = state.blocks.iter().map(|b| b.device.as_str()).collect();
        assert_eq!(devices, ["/dev/sdb", "/dev/sdb1", "/dev/sdb2", "/dev/sdb10"], "natural order");
        let disk = state.drive_block("/org/freedesktop/UDisks2/drives/usb").unwrap();
        assert_eq!(disk.content(), "GUID partition table");
        let parts = state.partitions(&disk.path);
        assert_eq!(parts.iter().map(|p| p.partition.as_ref().unwrap().number).collect::<Vec<_>>(), [1, 10, 2]);
        let esp = parts[0].partition.as_ref().unwrap();
        assert_eq!(esp.type_name, Some("EFI System"));
        assert_eq!(esp.flag_names, ["system"]);
        assert!(state.in_use(&disk.path), "a partition is mounted");
        assert!(!state.in_use(&parts[0].path));
        assert_eq!(state.jobs_for(&parts[2].path).next().unwrap().progress, Some(0.25));
        assert_eq!(state.drive_of(parts[1]).unwrap().model, "Extreme");
    }

    #[test]
    fn free_space_between_partitions() {
        let state = build(&fixture());
        let free = state.free_space("/org/freedesktop/UDisks2/block_devices/sdb");
        // From the end of sdb10 (50.5 GiB) to the LUKS partition at 60 GiB,
        // and from 61 GiB to the end, short of GPT's backup header.
        assert_eq!(free.len(), 2, "{free:?}");
        assert_eq!(free[0].offset, 513 * MIB + 50 * GIB);
        assert_eq!(free[0].offset + free[0].size, 60 * GIB);
        assert_eq!(free[1].offset, 61 * GIB);
        assert_eq!(free[1].offset + free[1].size, 100 * GIB - MIB, "aligned down to whole MiB");
        assert!(free.iter().all(|f| !f.logical && f.offset % MIB == 0));
    }

    #[test]
    fn volumes_for_the_desktop() {
        let state = build(&fixture());
        let mut unplugged = fixture();
        unplugged.map.retain(|path, _| !path.contains("sdb"));
        let changes = state.volume_changes(&build(&unplugged));
        assert_eq!(changes.added.len(), 3);
        assert!(changes.removed.is_empty());
        assert_eq!(build(&unplugged).volume_changes(&state).removed.len(), 3);
        let volumes = state.volumes();
        let names: Vec<&str> = volumes.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["1.1 GB Encrypted", "53.7 GB Volume", "ESP"]);
        let data = volumes.iter().find(|v| v.name == "53.7 GB Volume").unwrap();
        assert_eq!(data.mount_point.as_deref(), Some("/"));
        assert!(data.ejectable && data.can_power_off);
        assert_eq!(data.icon_name, "drive-removable-media");
        assert!(volumes.iter().find(|v| v.name.ends_with("Encrypted")).unwrap().locked);
        assert!(volumes.iter().all(|v| !v.system));
    }

    #[test]
    fn the_system_disk_by_its_drive() {
        // Here sdb10 holds `/`: the system's disk is shown first, as
        // "sdb", once, and never as something to eject.
        let state = build(&fixture());
        let system = state.system_volume("/home/u").expect("the system's disk");
        assert_eq!(system.name, "sdb");
        assert_eq!(system.mount_point.as_deref(), Some("/"));
        assert!(system.system && !system.ejectable && !system.can_power_off);
        assert_eq!(system.drive.as_deref(), Some("/org/freedesktop/UDisks2/drives/usb"));
        let shown = state.desktop_volumes("/home/u");
        assert_eq!(shown[0], system);
        assert_eq!(shown.iter().filter(|v| v.block == system.block).count(), 1);
        assert_eq!(shown.len(), 3);
        // Nothing at `/` (a container's tmpfs): the one holding home.
        let mut moved = fixture();
        let mut home = b"/home".to_vec();
        home.push(0);
        let b = "/org/freedesktop/UDisks2/block_devices/sdb10";
        moved
            .map
            .get_mut(b)
            .unwrap()
            .get_mut(IFACE_FILESYSTEM)
            .unwrap()
            .insert("MountPoints".into(), owned(vec![home]));
        let system = build(&moved).system_volume("/home/u").unwrap();
        assert_eq!((system.name.as_str(), system.mount_point.as_deref()), ("sdb", Some("/home")));
        assert!(build(&moved).system_volume("/srv/u").is_none());
    }

    #[test]
    fn disks_short_names() {
        assert_eq!(short_name("/dev/nvme0n1p2").as_deref(), Some("nvme0"));
        assert_eq!(short_name("/dev/nvme1n1").as_deref(), Some("nvme1"));
        assert_eq!(short_name("/dev/sda2").as_deref(), Some("sda"));
        assert_eq!(short_name("/dev/mmcblk0p1").as_deref(), Some("mmcblk0"));
        assert_eq!(short_name("/dev/mapper/cryptroot").as_deref(), Some("cryptroot"));
        assert_eq!(short_name("tmpfs"), None);
    }

    #[test]
    fn the_system_disk_without_udisks() {
        // `/` on LUKS on nvme0n1p2, as sysfs shows it.
        let root = std::env::temp_dir().join(format!("aizome-sys-sysblock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let disk = root.join("devices/nvme0n1");
        std::fs::create_dir_all(disk.join("nvme0n1p2")).unwrap();
        std::fs::write(disk.join("nvme0n1p2/partition"), "2\n").unwrap();
        std::fs::create_dir_all(root.join("devices/dm-0/slaves")).unwrap();
        std::fs::create_dir_all(root.join("devices/dm-0/slaves/nvme0n1p2")).unwrap();
        let class = root.join("class");
        std::fs::create_dir_all(&class).unwrap();
        std::os::unix::fs::symlink(root.join("devices/dm-0"), class.join("dm-0")).unwrap();
        std::os::unix::fs::symlink(disk.join("nvme0n1p2"), class.join("nvme0n1p2")).unwrap();
        let info = "22 1 254:0 / / rw,relatime shared:1 - ext4 /dev/dm-0 rw\n\
                    40 22 0:5 / /proc rw - proc proc rw\n";
        let v = system_volume_from_mounts(info, "/home/u", &class).unwrap();
        assert_eq!((v.name.as_str(), v.mount_point.as_deref(), v.system), ("nvme0", Some("/"), true));
        // `/` a tmpfs (a container): the disk holding home.
        let info = "603 171 0:61 /newroot / rw - tmpfs tmpfs rw\n\
                    646 603 259:2 /home /home rw - ext4 /dev/nvme0n1p2 rw\n";
        let v = system_volume_from_mounts(info, "/home/u", &class).unwrap();
        assert_eq!((v.name.as_str(), v.mount_point.as_deref()), ("nvme0", Some("/home")));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn sizes_read_like_drive_labels() {
        assert_eq!(format_size(512_110_190_592), "512 GB");
        assert_eq!(format_size(1_500_000_000), "1.5 GB");
        assert_eq!(format_size(999), "999 B");
    }
}
