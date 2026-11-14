// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{process::output, storage::Program};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::Path,
};

#[allow(clippy::unnecessary_cast)] // dev_t is u64 on Linux, i32 on Darwin.
fn device_number(meta: &fs::Metadata) -> libc::dev_t {
    meta.rdev() as libc::dev_t
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub path: String,
    pub major_minor: String,
    pub bytes: u64,
    pub serial: String,
    pub wwn: String,
    pub model: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Disk {
    pub identity: Identity,
    pub blocked: Option<String>,
}

fn busy(node: &Value) -> bool {
    node["mountpoints"]
        .as_array()
        .is_some_and(|a| a.iter().any(|m| m.as_str().is_some_and(|s| !s.is_empty())))
        || node["children"]
            .as_array()
            .is_some_and(|a| a.iter().any(busy))
}

pub fn parse(text: &str) -> Result<Vec<Disk>> {
    let data: Value = serde_json::from_str(text)?;
    let nodes = data["blockdevices"]
        .as_array()
        .context("lsblk did not return block devices")?;
    ensure!(
        !nodes.iter().any(|n| n["type"] == "part"),
        "lsblk returned a flat list, not the required device tree"
    );
    let mut disks = Vec::new();
    for node in nodes {
        if node["type"] != "disk" {
            continue;
        }
        let s = |key: &str| node[key].as_str().unwrap_or("").trim().to_string();
        // The live session's compressed RAM swap is never a target; listing
        // it as an unavailable disk only confuses.
        if s("path").starts_with("/dev/zram") {
            continue;
        }
        let bytes = node["size"].as_u64().context("Missing disk size")?;
        let blocked = if node["ro"] != false {
            Some("Read-only device")
        } else if busy(node) {
            Some("Contains a mounted filesystem or active swap (including live media)")
        } else if bytes < 24 * 1024u64.pow(3) {
            Some("Needs at least 24 GiB")
        } else if !s("path").starts_with("/dev/") {
            Some("Invalid device path")
        } else {
            None
        };
        disks.push(Disk {
            identity: Identity {
                path: s("path"),
                major_minor: s("maj:min"),
                bytes,
                serial: s("serial"),
                wwn: s("wwn"),
                model: s("model"),
            },
            blocked: blocked.map(str::to_owned),
        });
    }
    Ok(disks)
}

/// Every disk, for choosing one. A disk whose node or sysfs entry
/// misbehaves while it's checked (unplugged, re-enumerated) is listed as
/// unavailable rather than failing the others' listing.
pub fn discover() -> Result<Vec<Disk>> {
    let firmware = crate::Firmware::current();
    let mut disks = list(None)?;
    for disk in &mut disks {
        if let Err(error) = inspect(disk, firmware, &[]) {
            disk.blocked = Some(format!("Could not be checked: {error:#}"));
        }
    }
    Ok(disks)
}

/// lsblk's device tree: every disk, or only `device`'s.
fn list(device: Option<&str>) -> Result<Vec<Disk>> {
    let mut args = vec![
        "--json",
        "--tree",
        "--bytes",
        "--paths",
        "--output",
        "PATH,TYPE,SIZE,RO,MODEL,SERIAL,WWN,MAJ:MIN,MOUNTPOINTS",
    ];
    args.extend(device);
    parse(&output(Program::LSBLK, &args, 30)?)
}

/// The checks lsblk can't make: the node is the disk lsblk named, and its
/// sysfs entry says it's a usable target. Holders other than `allowed`
/// (sysfs paths of this installation's own mappings) block it.
fn inspect(
    disk: &mut Disk,
    firmware: crate::Firmware,
    allowed: &[std::path::PathBuf],
) -> Result<()> {
    let p = Path::new(&disk.identity.path);
    let meta = fs::metadata(p)?;
    ensure!(
        meta.file_type().is_block_device() && p.canonicalize()? == p,
        "Noncanonical block device"
    );
    let major = libc::major(device_number(&meta));
    let minor = libc::minor(device_number(&meta));
    ensure!(
        disk.identity.major_minor == format!("{major}:{minor}"),
        "Disk changed during discovery"
    );
    let sys = Path::new("/sys/dev/block")
        .join(&disk.identity.major_minor)
        .canonicalize()?;
    let logical_sector_size = fs::read_to_string(sys.join("queue/logical_block_size"))?
        .trim()
        .parse::<u64>()?;
    if let Some(reason) = boot_sector_block(firmware, logical_sector_size) {
        disk.blocked = Some(reason.into());
    }
    if sys.starts_with("/sys/devices/virtual/block") {
        disk.blocked =
            Some("Virtual memory or logical block device is not an installation target".into());
    }
    // Holders (LVM, dm-crypt, RAID) must block the whole drive even when
    // no filesystem on the logical device is currently mounted.
    for node in std::iter::once(sys.clone()).chain(
        fs::read_dir(&sys)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.join("partition").exists()),
    ) {
        let holders = fs::read_dir(node.join("holders"))?
            .map(|entry| Ok(entry?.path().canonicalize()?))
            .collect::<Result<Vec<_>>>()?;
        if holders.iter().any(|holder| !allowed.contains(holder)) {
            disk.blocked = Some("Device or partition is used by LVM, encryption or RAID".into());
        }
    }
    Ok(())
}

fn boot_sector_block(firmware: crate::Firmware, logical_sector_size: u64) -> Option<&'static str> {
    // GRUB's BIOS bootstrap encodes its next stage in 512-byte sectors.
    // A native 4 KiB BIOS read lands elsewhere even when grub-install succeeds.
    // Check logical size only: 512e disks remain valid BIOS targets.
    (firmware == crate::Firmware::Bios && logical_sector_size != 512).then_some(
        "BIOS installation requires 512-byte logical sectors. Restart the installation media in UEFI mode to use this disk.",
    )
}

/// Two readings name the same disk only if everything about it matches.
/// The path alone never does (another disk can take it over), and a disk
/// with no serial, WWN or model still has its device numbers and size.
impl aizome_typestate::SameTarget<Reading> for Identity {
    fn same_target(&self, fresh: &Reading) -> bool {
        *self == fresh.disk.identity
    }
}
impl aizome_typestate::SameTarget for Reading {
    fn same_target(&self, fresh: &Self) -> bool {
        self.disk.identity == fresh.disk.identity
    }
}

/// One disk as it is now: the only thing [`Reviewed::confirm`] accepts.
/// Only [`read`] makes one, from that disk alone.
#[derive(Debug)]
pub struct Reading {
    disk: Disk,
}

impl Reading {
    pub fn identity(&self) -> &Identity {
        &self.disk.identity
    }
    pub fn blocked(&self) -> Option<&str> {
        self.disk.blocked.as_deref()
    }
}

/// Reads the disk at `path`, and only it: lsblk for that device, then its
/// own node and sysfs entry. Nothing about another disk can make it fail.
pub fn read(path: &str) -> Result<Reading> {
    read_allowing(path, &[])
}

fn read_allowing(path: &str, allowed: &[std::path::PathBuf]) -> Result<Reading> {
    let mut disk = list(Some(path))
        .context("Selected disk disappeared")?
        .into_iter()
        .find(|d| d.identity.path == path)
        .context("Selected disk disappeared")?;
    inspect(&mut disk, crate::Firmware::current(), allowed)?;
    Ok(Reading { disk })
}

/// The brand of a disk's review: only this module makes one, so a
/// [`Confirmed`] disk comes only from [`Reviewed::confirm`]. A review is
/// confirmed again before each phase of the installation, so it can be kept.
#[derive(Clone, Debug)]
pub struct DiskReview(());

/// The disk the person reviewed, which the erase phrase names: a parsed
/// plan's. Every check before and during the installation compares a fresh
/// reading of that disk alone with it (GEN-03). Only a parsed plan makes a
/// review, so other code can't claim one for a disk it picked:
///
/// ```compile_fail,E0624
/// use calamares_nixos::disk::{Identity, Reviewed};
/// fn claim(disk: Identity) -> Reviewed {
///     Reviewed::new(disk)
/// }
/// ```
///
/// ```
/// use calamares_nixos::{InstallPlan, disk::Reviewed};
/// fn review(plan: &InstallPlan) -> Reviewed {
///     Reviewed::of(plan)
/// }
/// ```
#[derive(Clone, Debug)]
pub struct Reviewed(aizome_typestate::Reviewed<Identity, DiskReview>);

impl Reviewed {
    /// The plan's disk: what the review showed and the erase phrase names.
    pub fn of(plan: &crate::InstallPlan) -> Self {
        Self::new(plan.disk().clone())
    }
    pub(crate) fn new(identity: Identity) -> Self {
        Self(aizome_typestate::Reviewed::new(identity, DiskReview(())))
    }
    pub fn identity(&self) -> &Identity {
        self.0.target()
    }

    /// Locks the reviewed disk for its first writes: opens it by the reviewed
    /// path, takes the partition-table lock, reads it again, and checks that
    /// the locked node is the confirmed disk. Only this makes the [`Locked`]
    /// disk the writes take, so they can't act on a proof made earlier.
    pub fn lock(&self) -> Result<Locked> {
        let device = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.identity().path)?;
        ensure!(
            crate::sys::try_lock_exclusive(&device),
            "Disk is locked by another installer"
        );
        let disk = self.revalidate()?;
        let rdev = device_number(&device.metadata()?);
        ensure!(
            format!("{}:{}", libc::major(rdev), libc::minor(rdev)) == disk.identity().major_minor,
            "The locked device is not the confirmed disk"
        );
        Ok(Locked { device, disk })
    }

    /// Reads the reviewed disk again and checks it's the same disk and
    /// still usable.
    pub fn revalidate(&self) -> Result<Confirmed> {
        self.confirm(read(&self.identity().path)?)
    }

    /// Checks a fresh reading against what the person reviewed. It takes a
    /// [`Reading`] of one disk, which [`read`] makes from that disk alone,
    /// so a check of the selected disk can't fail because another disk was
    /// unplugged or renumbered meanwhile, as a full rediscovery could. Once
    /// the disk has been partitioned, that failure abandoned a half-written
    /// installation (GEN-03):
    ///
    /// ```compile_fail,E0308
    /// use calamares_nixos::disk;
    /// fn recheck(reviewed: &disk::Reviewed) -> anyhow::Result<disk::Confirmed> {
    ///     let fresh = disk::discover()?
    ///         .into_iter()
    ///         .find(|d| d.identity.path == reviewed.identity().path)
    ///         .ok_or_else(|| anyhow::anyhow!("Selected disk disappeared"))?;
    ///     reviewed.confirm(fresh)
    /// }
    /// ```
    ///
    /// ```
    /// use calamares_nixos::disk;
    /// fn recheck(reviewed: &disk::Reviewed) -> anyhow::Result<disk::Confirmed> {
    ///     reviewed.confirm(disk::read(&reviewed.identity().path)?)
    /// }
    /// ```
    pub fn confirm(&self, fresh: Reading) -> Result<Confirmed> {
        let now = self
            .0
            .clone()
            .confirm(fresh)
            .map_err(|_| anyhow::anyhow!("Disk identity changed; rescan and review again"))?;
        Confirmed::usable(now)
    }
}

/// Proof that the disk is the one the person reviewed, read again just now.
/// Only [`Reviewed::confirm`] makes one, and the destructive steps take it
/// (or a [`Device`] only it yields): they act on the disk it carries, never
/// on a path passed beside it.
#[derive(Debug)]
pub struct Confirmed(aizome_typestate::Confirmed<Reading, DiskReview>);

impl Confirmed {
    fn usable(now: aizome_typestate::Confirmed<Reading, DiskReview>) -> Result<Self> {
        let blocked = now.target().blocked();
        ensure!(
            blocked.is_none(),
            "Disk is unavailable: {}",
            blocked.unwrap_or("")
        );
        Ok(Self(now))
    }

    pub fn identity(&self) -> &Identity {
        self.0.target().identity()
    }
    pub fn path(&self) -> &str {
        &self.identity().path
    }

    /// Reads the confirmed disk alone again after a write, accepting
    /// `mappings` (this installation's own device-mapper nodes) as holders.
    pub fn recheck(self, mappings: &[String]) -> Result<Confirmed> {
        let allowed: Vec<_> = mappings
            .iter()
            .map(|path| -> Result<_> {
                let meta = fs::metadata(path)?;
                ensure!(
                    meta.file_type().is_block_device(),
                    "Mapping is not a block device"
                );
                Ok(Path::new("/sys/dev/block")
                    .join(format!(
                        "{}:{}",
                        libc::major(device_number(&meta)),
                        libc::minor(device_number(&meta))
                    ))
                    .canonicalize()?)
            })
            .collect::<Result<_>>()?;
        let fresh = read_allowing(self.path(), &allowed)?;
        let now = self
            .0
            .recheck(fresh)
            .map_err(|_| anyhow::anyhow!("Disk identity changed during installation"))?;
        Self::usable(now)
    }

    /// Partition `index` of this disk.
    pub fn partition(&self, index: u8) -> Device {
        Device(partition(self.path(), index))
    }
}

/// The confirmed disk, opened and holding the partition-table lock, read
/// again after the lock was taken: what the first writes take. Only
/// [`Reviewed::lock`] makes one.
#[derive(Debug)]
pub struct Locked {
    device: fs::File,
    disk: Confirmed,
}

impl Locked {
    pub fn disk(&self) -> &Confirmed {
        &self.disk
    }

    /// Reads the disk alone again while it's locked, after a write.
    pub fn recheck(self, mappings: &[String]) -> Result<Locked> {
        let Locked { device, disk } = self;
        Ok(Locked {
            device,
            disk: disk.recheck(mappings)?,
        })
    }

    /// Flushes and releases the lock (udev holds back its events for a
    /// locked disk): the confirmed disk, for what follows.
    pub fn release(self) -> Result<Confirmed> {
        self.device.sync_all()?;
        Ok(self.disk)
    }
}

/// A block device on the confirmed disk: one of its partitions, or an
/// encrypted mapping over one. Only [`Confirmed::partition`] makes one, so
/// formatting can't be pointed at a device named from a path passed beside
/// the proof.
#[derive(Clone, Debug)]
pub struct Device(String);

impl Device {
    pub fn path(&self) -> &str {
        &self.0
    }
    /// The device-mapper node `name`, opened over this device.
    pub(crate) fn mapping(&self, name: &str) -> Device {
        Device(format!("/dev/mapper/{name}"))
    }
    /// A disposable test device (a loop device, a VM disk's partition).
    #[cfg(test)]
    pub(crate) fn for_test(path: &str) -> Device {
        Device(path.into())
    }
}

impl Layout {
    /// Erases the locked disk's signatures and writes this layout's GPT,
    /// then has the kernel read it. The first writes of an installation:
    /// they take the disk locked and confirmed just now ([`Locked`]), so they
    /// can't erase a disk named any other way, nor reuse a proof made before
    /// the lock:
    ///
    /// ```compile_fail,E0308
    /// use calamares_nixos::{Filesystem, Firmware, InstallPlan, disk::Layout};
    /// fn erase(plan: &InstallPlan, layout: &Layout) -> anyhow::Result<()> {
    ///     layout.write(&plan.disk().path, Firmware::Uefi, Filesystem::Ext4)
    /// }
    /// ```
    ///
    /// ```compile_fail,E0308
    /// use calamares_nixos::{Filesystem, Firmware, disk::{Layout, Reviewed}};
    /// fn erase(reviewed: &Reviewed, layout: &Layout) -> anyhow::Result<()> {
    ///     // Confirmed at review, without the lock, then written to later.
    ///     let confirmed = reviewed.revalidate()?;
    ///     layout.write(&confirmed, Firmware::Uefi, Filesystem::Ext4)
    /// }
    /// ```
    ///
    /// ```
    /// use calamares_nixos::{Filesystem, Firmware, InstallPlan, disk::{Layout, Reviewed}};
    /// fn erase(plan: &InstallPlan, layout: &Layout) -> anyhow::Result<()> {
    ///     layout.write(&Reviewed::of(plan).lock()?, Firmware::Uefi, Filesystem::Ext4)
    /// }
    /// ```
    pub fn write(
        &self,
        disk: &Locked,
        firmware: crate::Firmware,
        filesystem: crate::Filesystem,
    ) -> Result<()> {
        let dev = disk.disk().path();
        output(Program::WIPEFS, &["--all", "--force", dev], 60)?;
        let script = self.parted_script(firmware, filesystem.name());
        let mut args = vec!["--script", dev];
        args.extend(script.iter().map(String::as_str));
        output(Program::PARTED, &args, 60)?;
        // Keep udev from observing partially written superblocks. Partition nodes
        // come from the kernel/devtmpfs; do not wait for udev while holding its lock.
        output(Program::BLOCKDEV, &["--rereadpt", dev], 30)?;
        Ok(())
    }
}

pub fn partition(path: &str, index: u8) -> String {
    format!(
        "{path}{}{index}",
        if path.ends_with(|c: char| c.is_ascii_digit()) {
            "p"
        } else {
            ""
        }
    )
}

/// Test diagnostics are only available with our marked, disposable QEMU disk.
/// Both transports must satisfy the same serial and size checks.
pub fn vm_test_disk() -> Result<Identity> {
    let mut matches = discover()?.into_iter().filter(|d| {
        ["/dev/vda", "/dev/nvme0n1"].contains(&d.identity.path.as_str())
            && d.identity.serial == "RESPIN_TEST_ONLY"
            && [40 * 1024u64.pow(3), 80 * 1024u64.pow(3)].contains(&d.identity.bytes)
    });
    let disk = matches
        .next()
        .context("Missing marked 40 or 80 GiB VM disk")?;
    ensure!(matches.next().is_none(), "Multiple VM test disks");
    Ok(disk.identity)
}

const MIB: u64 = 1024 * 1024;
/// The installed root filesystem must keep at least this much space.
pub const MIN_ROOT_BYTES: u64 = 20 * 1024 * MIB;

/// Exact GPT geometry in MiB: boot (ESP or BIOS boot), root, optional swap at
/// the end. Explicit aligned boundaries let the kernel's view be checked
/// exactly instead of trusting that partition names exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub boot: (u64, u64),
    pub root: (u64, u64),
    pub swap: Option<(u64, u64)>,
    pub bios_boot: Option<(u64, u64)>,
    pub encrypted: bool,
}
impl Layout {
    pub fn new(
        disk_bytes: u64,
        firmware: crate::Firmware,
        swap_bytes: Option<u64>,
    ) -> Result<Self> {
        Self::for_install(disk_bytes, firmware, swap_bytes, false)
    }
    pub fn for_install(
        disk_bytes: u64,
        firmware: crate::Firmware,
        swap_bytes: Option<u64>,
        encrypted: bool,
    ) -> Result<Self> {
        let boot = if firmware == crate::Firmware::Uefi {
            (1, 1025)
        } else {
            (1, 3)
        };
        // Leave the final MiB for the backup GPT (33 sectors at 512 bytes,
        // 5 at 4096) and partition alignment.
        let end = (disk_bytes / MIB)
            .checked_sub(1)
            .context("Disk is too small")?;
        let bios_boot = (encrypted && firmware == crate::Firmware::Bios).then_some((3, 1027));
        let root_start = bios_boot.map_or(boot.1, |(_, end)| end);
        let overhead = if encrypted {
            crate::encryption::HEADER_BYTES
        } else {
            0
        };
        let swap_mib = swap_bytes
            .map(|bytes| {
                bytes
                    .checked_add(overhead)
                    .context("Swap size overflow")
                    .map(|bytes| bytes.div_ceil(MIB))
            })
            .transpose()?;
        let root_end = end
            .checked_sub(swap_mib.unwrap_or(0))
            .filter(|root_end| *root_end > root_start + overhead / MIB)
            .context("Disk is too small for the swap partition")?;
        let layout = Self {
            boot,
            root: (root_start, root_end),
            bios_boot,
            encrypted,
            swap: swap_mib.map(|_| (root_end, end)),
        };
        ensure!(
            layout.root_bytes() >= MIN_ROOT_BYTES,
            "The root filesystem would have only {:.1} GiB; at least {} GiB is required{}",
            layout.root_bytes() as f64 / 1024f64.powi(3),
            MIN_ROOT_BYTES / 1024u64.pow(3),
            if swap_bytes.is_some() {
                ". Disable the RAM-sized swap partition or choose a larger disk"
            } else {
                ""
            }
        );
        Ok(layout)
    }
    pub fn root_bytes(&self) -> u64 {
        (self.root.1 - self.root.0) * MIB - self.header_bytes()
    }
    pub fn swap_bytes(&self) -> Option<u64> {
        self.swap
            .map(|(start, end)| (end - start) * MIB - self.header_bytes())
    }
    fn header_bytes(&self) -> u64 {
        if self.encrypted {
            crate::encryption::HEADER_BYTES
        } else {
            0
        }
    }
    pub fn bios_boot_partition(&self) -> Option<u8> {
        self.bios_boot
            .map(|_| if self.swap.is_some() { 4 } else { 3 })
    }
    /// One parted invocation: fewer partition-table rereads and udev events.
    pub(crate) fn parted_script(&self, firmware: crate::Firmware, filesystem: &str) -> Vec<String> {
        // Binary units are exact in parted; decimal units are rounded.
        let mut script = vec!["mklabel".into(), "gpt".into()];
        let mut part = |name: &str, kind: &str, (start, end): (u64, u64)| {
            script.extend([
                "mkpart".into(),
                name.into(),
                kind.into(),
                format!("{start}MiB"),
                format!("{end}MiB"),
            ]);
        };
        if firmware == crate::Firmware::Uefi {
            part("ESP", "fat32", self.boot);
        } else {
            part("BIOS", "", self.boot);
        }
        part("root", filesystem, self.root);
        if let Some(swap) = self.swap {
            part("swap", "linux-swap", swap);
        }
        if let Some(boot) = self.bios_boot {
            part("boot", "ext4", boot);
        }
        script.retain(|word| !word.is_empty());
        script.extend(
            if firmware == crate::Firmware::Uefi {
                ["set", "1", "esp", "on"]
            } else {
                ["set", "1", "bios_grub", "on"]
            }
            .map(String::from),
        );
        script
    }
    fn partitions(&self) -> Vec<(u8, (u64, u64))> {
        let mut partitions = vec![(1, self.boot), (2, self.root)];
        partitions.extend(self.swap.map(|swap| (3, swap)));
        if let Some(boot) = self.bios_boot {
            partitions.push((self.bios_boot_partition().unwrap(), boot));
        }
        partitions
    }
}

/// Require the kernel's actual partition geometry, not merely existing names.
/// sysfs start/size are always in 512-byte sectors, including on 4Kn NVMe.
pub(crate) fn verify_layout(disk: &Identity, layout: &Layout) -> Result<()> {
    let parent = Path::new("/sys/dev/block")
        .join(&disk.major_minor)
        .canonicalize()?;
    let expected = layout.partitions();
    // A leftover partition 4+ would mean the table was not fully replaced.
    let count = fs::read_dir(&parent)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().join("partition").exists())
        .count();
    ensure!(
        count == expected.len(),
        "Expected {} partitions, the kernel reports {count}",
        expected.len()
    );
    for (index, (start_mib, end_mib)) in expected {
        let device = partition(&disk.path, index);
        let meta = fs::metadata(&device)?;
        ensure!(
            meta.file_type().is_block_device()
                && Path::new(&device).canonicalize()? == Path::new(&device),
            "Invalid partition node {device}"
        );
        let node = Path::new("/sys/dev/block")
            .join(format!(
                "{}:{}",
                libc::major(device_number(&meta)),
                libc::minor(device_number(&meta))
            ))
            .canonicalize()?;
        ensure!(
            node.parent() == Some(parent.as_path()),
            "Partition {device} belongs to another disk"
        );
        let number = |name: &str| -> Result<u64> {
            Ok(fs::read_to_string(node.join(name))?.trim().parse()?)
        };
        ensure!(
            number("partition")? == u64::from(index),
            "Wrong partition number for {device}"
        );
        let start = number("start")?
            .checked_mul(512)
            .context("Partition start overflow")?;
        let size = number("size")?
            .checked_mul(512)
            .context("Partition size overflow")?;
        ensure!(
            start == start_mib * MIB && size == (end_mib - start_mib) * MIB,
            "Unexpected partition geometry for {device}: start {start}, size {size}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bios_requires_512_logical_sectors_but_uefi_accepts_4kn() {
        assert!(boot_sector_block(crate::Firmware::Bios, 512).is_none());
        assert!(boot_sector_block(crate::Firmware::Bios, 4096).is_some());
        assert!(boot_sector_block(crate::Firmware::Uefi, 4096).is_none());
    }
    #[test]
    #[ignore = "requires the marked disposable disks in the storage VM"]
    fn real_bios_sector_discovery_and_revalidation() {
        assert_eq!(crate::Firmware::current(), crate::Firmware::Bios);
        let disks = discover().unwrap();
        let native = disks
            .iter()
            .find(|d| d.identity.serial == "SEKKA_BIOS4KN_TEST")
            .unwrap();
        let emulated = disks
            .iter()
            .find(|d| d.identity.serial == "SEKKA_BIOS512E_TEST")
            .unwrap();
        for disk in [native, emulated] {
            assert_eq!(disk.identity.bytes, 40 * 1024u64.pow(3));
        }
        assert!(
            native
                .blocked
                .as_deref()
                .unwrap()
                .contains("Restart the installation media in UEFI mode")
        );
        assert!(
            Reviewed::new(native.identity.clone())
                .revalidate()
                .unwrap_err()
                .to_string()
                .contains("512-byte logical sectors")
        );
        assert!(emulated.blocked.is_none());
        Reviewed::new(emulated.identity.clone())
            .revalidate()
            .unwrap();
    }
    #[test]
    fn layout_reserves_ram_sized_swap_at_the_end() {
        use crate::Firmware;
        let gib = 1024u64.pow(3);
        let layout = Layout::new(512 * gib, Firmware::Uefi, Some(32 * gib)).unwrap();
        assert_eq!(layout.boot, (1, 1025));
        assert_eq!(layout.root.0, 1025);
        assert_eq!(layout.swap_bytes(), Some(32 * gib));
        assert_eq!(layout.swap.unwrap().1, 512 * 1024 - 1);
        assert_eq!(layout.root.1, layout.swap.unwrap().0);
        let bios = Layout::new(40 * gib, Firmware::Bios, None).unwrap();
        assert_eq!(bios.boot, (1, 3));
        assert_eq!(bios.root, (3, 40 * 1024 - 1));
        assert!(bios.swap.is_none());
        // Swap may not squeeze root below its minimum.
        assert!(Layout::new(40 * gib, Firmware::Uefi, Some(32 * gib)).is_err());
        assert!(Layout::new(gib / 2, Firmware::Uefi, None).is_err());
        assert_eq!(
            layout.parted_script(Firmware::Uefi, "ext4").join(" "),
            format!(
                "mklabel gpt mkpart ESP fat32 1MiB 1025MiB mkpart root ext4 1025MiB {}MiB mkpart swap linux-swap {}MiB {}MiB set 1 esp on",
                layout.root.1,
                layout.root.1,
                layout.swap.unwrap().1
            )
        );
        assert_eq!(
            bios.parted_script(Firmware::Bios, "xfs").join(" "),
            format!(
                "mklabel gpt mkpart BIOS 1MiB 3MiB mkpart root xfs 3MiB {}MiB set 1 bios_grub on",
                bios.root.1
            )
        );
    }
    #[test]
    fn live_zram_is_not_listed() {
        let disks = parse(r#"{"blockdevices":[{"type":"disk","path":"/dev/zram0","size":16000000000,"ro":false,"mountpoints":["[SWAP]"]},{"type":"disk","path":"/dev/vda","size":42949672960,"ro":false}]}"#).unwrap();
        assert_eq!(disks.len(), 1);
        assert_eq!(disks[0].identity.path, "/dev/vda");
    }
    #[test]
    fn flat_device_list_is_rejected() {
        assert!(parse(r#"{"blockdevices":[{"type":"disk","size":42949672960,"ro":false,"path":"/dev/vda"},{"type":"part","mountpoints":["/"]}]}"#).is_err());
    }
    #[test]
    fn partition_names() {
        assert_eq!(partition("/dev/vda", 2), "/dev/vda2");
        assert_eq!(partition("/dev/nvme0n1", 2), "/dev/nvme0n1p2");
        assert_eq!(partition("/dev/mmcblk0", 2), "/dev/mmcblk0p2");
    }
    #[test]
    fn refuses_live_swap_small_and_readonly() {
        for extra in [
            r#""ro":true"#,
            r#""ro":false,"mountpoints":["[SWAP]"]"#,
            r#""ro":false,"children":[{"mountpoints":["/iso"]}]"#,
        ] {
            let json = format!(
                r#"{{"blockdevices":[{{"type":"disk","path":"/dev/vda","size":42949672960,{extra}}}]}}"#
            );
            assert!(parse(&json).unwrap()[0].blocked.is_some());
        }
        assert!(
            parse(r#"{"blockdevices":[{"type":"disk","path":"/dev/vda","size":1000,"ro":false}]}"#)
                .unwrap()[0]
                .blocked
                .is_some()
        );
    }
}

#[cfg(test)]
mod encryption_tests {
    use super::*;
    #[test]
    fn encrypted_layout_reserves_headers_and_a_plain_bios_boot_filesystem() {
        for firmware in [crate::Firmware::Uefi, crate::Firmware::Bios] {
            for swap in [None, Some(4 * 1024 * MIB)] {
                let layout = Layout::for_install(40 * 1024 * MIB, firmware, swap, true).unwrap();
                assert_eq!(layout.swap_bytes(), swap);
                assert_eq!(
                    layout.root_bytes() + crate::encryption::HEADER_BYTES,
                    (layout.root.1 - layout.root.0) * MIB
                );
                let mut spans = layout.partitions();
                spans.sort_by_key(|(_, (start, _))| *start);
                for pair in spans.windows(2) {
                    assert_eq!(pair[0].1.1, pair[1].1.0);
                }
                assert_eq!(
                    layout.bios_boot.is_some(),
                    firmware == crate::Firmware::Bios
                );
                assert_eq!(
                    layout.bios_boot_partition(),
                    (firmware == crate::Firmware::Bios).then_some(if swap.is_some() {
                        4
                    } else {
                        3
                    })
                );
                if firmware == crate::Firmware::Bios {
                    assert!(
                        layout
                            .parted_script(firmware, "ext4")
                            .join(" ")
                            .contains("mkpart boot ext4 3MiB 1027MiB")
                    );
                }
            }
        }
        assert!(
            Layout::for_install(40 * 1024 * MIB, crate::Firmware::Uefi, Some(u64::MAX), true)
                .is_err()
        );
    }
}
