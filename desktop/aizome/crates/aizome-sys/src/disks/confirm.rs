//! Confirmations for operations that change what is on a disk.
//!
//! Every such [`Disks`](super::Disks) method takes a [`Confirm<Op, B>`],
//! made from the [`Identity`] of the device as the person saw it when they
//! agreed, and a brand `B` that only the code that showed it to them can
//! make: `Confirm::new(state.identity(&block)?, brand)`. An identity holds
//! what stays with a device, not only what another device can take over
//! (an object path, a `/dev` name, a size): the drive it is on (its serial,
//! its WWN, the USB port it is in, and when UDisks detected it and the
//! medium in it) or the image a
//! loop device holds; a partition's place in its table and its UUID; and
//! what is on it (its filesystem's type and UUID, or its partition table's
//! partitions). Only [`DisksState::identity`] makes one, from the whole
//! state: a block device's record alone doesn't know its drive, and a whole
//! partitioned disk has no filesystem UUID, so two sticks of the same size
//! at the same name look alike in it.
//!
//! The operation reads UDisks's state again when it runs, identifies the
//! device at the confirmed path again, and refuses unless it is the same
//! target ([`SameTarget`]) and the confirmation is less than ten minutes
//! old; one that destroys data also refuses while anything on the device
//! is in use. What must be the same depends on the operation
//! ([`Change::HOLDS`]): a format holds the device to exactly what the
//! person saw, while setting a partition's flags holds it only to its
//! place, so it can follow a format of the same partition in one plan. A
//! confirmation is used once: it is moved into the call, and spent by
//! [`Confirm::check`], whose proof ([`Confirmed`]) carries the device as it
//! is now. The operation acts on that device, not on one passed beside it.
//!
//! What stays a run-time check, or the caller's:
//!
//! - The identities are UDisks's values, so comparing them can only happen
//!   when the operation runs; and UDisks has no "only if still" call, so
//!   the device could still change between the check and UDisks acting.
//! - An identity read from any state but the one the person reviewed (the
//!   snapshot when the operation runs, or a `DisksState` put together by
//!   hand) confirms whatever is there. This module can't know when the
//!   person looked; the caller's brand must keep its other code from making
//!   confirmations (Tansu's are branded by its queued operations).
//! - A card swapped for another in the same reader faster than the kernel
//!   polls for media (about two seconds) may keep the reader's
//!   `TimeMediaDetected`; then only the partitions' and filesystems' UUIDs
//!   tell the cards apart, and an operation held only to the disk's place
//!   (making a partition, in free space checked again) goes ahead.

use super::model::{BlockDevice, DisksState};
use crate::error::{Error, Result};
use aizome_typestate::{Confirmed, Reviewed, SameTarget};
use std::marker::PhantomData;
use std::time::{Duration, Instant};

/// How long a confirmation stays good.
pub const CONFIRMATION_LIFETIME: Duration = Duration::from_secs(600);

/// What a confirmation holds its device to, besides being at the same path
/// on the same drive (or image), in the same USB port: every kind holds
/// that, since another port means another stick, or this one moved while
/// the person wasn't looking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holds {
    /// Its place: the same partition of the same table (number, start and
    /// UUID), or the same whole device (and size); whatever is on it now.
    Place,
    /// Its place and what is on it: the same filesystem (type and UUID), or
    /// the same partition table (kind and partitions).
    Contents,
    /// Its place, what is on it, and its size.
    Exactly,
}

/// An operation that changes what is on a disk.
pub trait Change {
    /// What it does, for messages ("format").
    const WHAT: &'static str;
    /// What its confirmation holds the device to.
    const HOLDS: Holds;
    /// Whether it is refused while the device, or anything on it, is
    /// mounted or in use as swap (those that destroy data).
    const REFUSED_IN_USE: bool;
}

/// Make a new filesystem (or wipe, or encrypt): everything on the device is
/// lost.
pub struct Format;
/// Make a new partition table: every partition on the disk is lost.
pub struct NewPartitionTable;
/// Delete a partition and what is on it.
pub struct DeletePartition;
/// Change a partition's or filesystem's size: shrinking can lose what is
/// past the new end. Held to the filesystem rather than the size, which the
/// steps of one resize change.
pub struct Resize;
/// Repair a filesystem: the repair may discard what it can't save.
pub struct Repair;
/// Make a partition in a disk's free space (and perhaps a filesystem in
/// it). Held to the disk, not its partitions, which earlier operations of
/// the same plan may have changed.
pub struct CreatePartition;
/// Change a filesystem's label.
pub struct Relabel;
/// Change a partition's type, name or flags: its entry in the table, not
/// what is in it (which an earlier format may have changed).
pub struct PartitionEntry;
/// Change an encrypted volume's passphrase.
pub struct ChangePassphrase;

impl Change for Format {
    const WHAT: &'static str = "format";
    const HOLDS: Holds = Holds::Exactly;
    const REFUSED_IN_USE: bool = true;
}
impl Change for NewPartitionTable {
    const WHAT: &'static str = "create a partition table on";
    const HOLDS: Holds = Holds::Exactly;
    const REFUSED_IN_USE: bool = true;
}
impl Change for DeletePartition {
    const WHAT: &'static str = "delete";
    const HOLDS: Holds = Holds::Exactly;
    const REFUSED_IN_USE: bool = true;
}
impl Change for Resize {
    const WHAT: &'static str = "resize";
    const HOLDS: Holds = Holds::Contents;
    const REFUSED_IN_USE: bool = true;
}
impl Change for Repair {
    const WHAT: &'static str = "repair";
    const HOLDS: Holds = Holds::Exactly;
    const REFUSED_IN_USE: bool = true;
}
impl Change for CreatePartition {
    const WHAT: &'static str = "make a partition on";
    const HOLDS: Holds = Holds::Place;
    const REFUSED_IN_USE: bool = false;
}
impl Change for Relabel {
    const WHAT: &'static str = "relabel";
    const HOLDS: Holds = Holds::Contents;
    const REFUSED_IN_USE: bool = false;
}
impl Change for PartitionEntry {
    const WHAT: &'static str = "change the type, name or flags of";
    const HOLDS: Holds = Holds::Place;
    const REFUSED_IN_USE: bool = false;
}
impl Change for ChangePassphrase {
    const WHAT: &'static str = "change the passphrase of";
    const HOLDS: Holds = Holds::Exactly;
    const REFUSED_IN_USE: bool = false;
}

/// What identifies a block device: where it is, what it is on, and what is
/// on it. Only [`DisksState::identity`] makes one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    path: String,
    device: String,
    size: u64,
    medium: Medium,
    /// The USB port the drive is in, as udev names it (`pci-0000:00:14.0-usb-0:2:1.0-scsi-0:0:0:0`),
    /// when it is on USB and UDisks lists the link.
    port: Option<String>,
    place: Option<Place>,
    contents: Contents,
}

/// What a device is on.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Medium {
    /// A drive, as UDisks knows it, and the medium in it.
    Drive { path: String, id: String, serial: String, wwn: String, detected: u64, media_detected: u64 },
    /// A loop device's image file.
    Image { file: String },
    /// Neither (a device mapper device of its own).
    Unknown,
}

/// A partition's place in its table.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Place {
    table: String,
    number: u32,
    offset: u64,
    uuid: String,
}

/// What is on a device.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Contents {
    id_type: String,
    id_uuid: String,
    /// A partition table's kind and its partitions (object path and UUID).
    table: Option<(String, Vec<(String, String)>)>,
}

impl Identity {
    /// The block device's object path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Its node (`/dev/sdb1`).
    pub fn device(&self) -> &str {
        &self.device
    }

    /// Whether `fresh` is the same target as this, held to `holds`.
    pub fn same(&self, fresh: &Identity, holds: Holds) -> bool {
        let place = self.path == fresh.path
            && self.device == fresh.device
            && self.medium == fresh.medium
            // Another port is another stick, even one alike in every other
            // way; compared only when both readings have one, as udev can drop
            // and remake the link while it probes the disk again.
            && self.port.as_ref().zip(fresh.port.as_ref()).is_none_or(|(seen, now)| seen == now)
            && self.place == fresh.place
            // A partition's size is what resizing changes; a whole device's
            // is part of what it is.
            && (self.place.is_some() || self.size == fresh.size);
        match holds {
            Holds::Place => place,
            Holds::Contents => place && self.contents == fresh.contents,
            Holds::Exactly => place && self.contents == fresh.contents && self.size == fresh.size,
        }
    }
}

impl DisksState {
    /// The USB port the disk holding `block` is in: its whole disk's
    /// `/dev/disk/by-path` link, when that is a USB one.
    fn usb_port(&self, block: &BlockDevice) -> Option<String> {
        let mut disk = block;
        for _ in 0..8 {
            let up =
                disk.crypto_backing_device.as_deref().or_else(|| disk.partition.as_ref().map(|p| p.table.as_str()));
            match up.and_then(|path| self.block(path)) {
                Some(next) => disk = next,
                None => break,
            }
        }
        disk.symlinks
            .iter()
            .filter_map(|link| link.strip_prefix("/dev/disk/by-path/"))
            .find(|name| name.contains("-usb-"))
            .map(str::to_owned)
    }

    /// The identity of the block device at `path` (an object path), as this
    /// state has it: where the person reviews the device, make the
    /// confirmation from this.
    pub fn identity(&self, path: &str) -> Option<Identity> {
        let b = self.block(path)?;
        let medium = match (self.drive_of(b), self.loop_of(b)) {
            (Some(d), _) => Medium::Drive {
                path: d.path.clone(),
                id: d.id.clone(),
                serial: d.serial.clone(),
                wwn: d.wwn.clone(),
                detected: d.time_detected,
                media_detected: d.time_media_detected,
            },
            (None, Some(l)) => Medium::Image { file: l.backing_file.clone() },
            (None, None) => Medium::Unknown,
        };
        let place = b.partition.as_ref().map(|p| Place {
            table: p.table.clone(),
            number: p.number,
            offset: p.offset,
            uuid: p.uuid.clone(),
        });
        let table = b.partition_table.as_ref().map(|t| {
            let mut parts: Vec<(String, String)> = self
                .partitions(path)
                .iter()
                .map(|p| (p.path.clone(), p.partition.as_ref().map(|p| p.uuid.clone()).unwrap_or_default()))
                .collect();
            parts.sort();
            (t.kind.clone(), parts)
        });
        Some(Identity {
            path: b.path.clone(),
            device: b.device.clone(),
            size: b.size,
            medium,
            port: self.usb_port(b),
            place,
            contents: Contents { id_type: b.id_type.clone(), id_uuid: b.id_uuid.clone(), table },
        })
    }
}

/// A device's identity as the person saw it, compared as `Op` needs it
/// ([`Change::HOLDS`]) with a fresh reading. Private, so a confirmation
/// made for one operation can't be rebuilt as another's.
struct Seen<Op> {
    identity: Identity,
    op: PhantomData<fn() -> Op>,
}

impl<Op: Change> SameTarget<Identity> for Seen<Op> {
    fn same_target(&self, fresh: &Identity) -> bool {
        self.identity.same(fresh, Op::HOLDS)
    }
}

/// The person agreed to `Op` on this device, as they saw it.
///
/// `B` is the brand of the review: a type only the caller's reviewing code
/// can make. The operations take any brand; the caller's own interface
/// names the one it accepts (Tansu's backend takes
/// `Confirm<_, tansu::plan::Queued>`), so the caller's other code can't make
/// a confirmation of a device nobody reviewed.
#[must_use = "a confirmation does nothing until it is passed to the operation"]
pub struct Confirm<Op: Change, B> {
    reviewed: Reviewed<Seen<Op>, B>,
    at: Instant,
}

impl<Op: Change, B> std::fmt::Debug for Confirm<Op, B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Confirm").field("op", &Op::WHAT).field("device", &self.device()).finish()
    }
}

impl<Op: Change, B> Confirm<Op, B> {
    /// The person agreed to `Op` on the device they saw: `seen`, read with
    /// [`DisksState::identity`] from the state they reviewed, and the brand
    /// of the code that showed it to them.
    ///
    /// A block device's record alone can't confirm anything: it doesn't
    /// know the drive it is on. (SYA-13: the confirmation for a new
    /// partition table on one stick, made from its record, held for another
    /// stick of the same size that came up at the same name.)
    ///
    /// ```compile_fail,E0308
    /// use aizome_sys::disks::{BlockDevice, Confirm, NewPartitionTable};
    /// struct Sheet;
    /// let stick = BlockDevice {
    ///     path: "/org/freedesktop/UDisks2/block_devices/sdb".into(),
    ///     device: "/dev/sdb".into(),
    ///     size: 32_017_047_552,
    ///     ..Default::default()
    /// };
    /// let _ = Confirm::<NewPartitionTable, Sheet>::new(&stick, Sheet);
    /// ```
    ///
    /// Nor can an identity be put together, such as another device's given
    /// the path and name the person saw:
    ///
    /// ```compile_fail,E0451
    /// use aizome_sys::disks::{Confirm, Identity, NewPartitionTable};
    /// struct Sheet;
    /// fn forge(another: Identity) -> Confirm<NewPartitionTable, Sheet> {
    ///     let seen = Identity {
    ///         path: "/org/freedesktop/UDisks2/block_devices/sdb".into(),
    ///         device: "/dev/sdb".into(),
    ///         ..another
    ///     };
    ///     Confirm::new(seen, Sheet)
    /// }
    /// ```
    ///
    /// and a confirmation made for one operation can't be rebuilt as another's
    /// (one held only to a partition's place, for its flags, as a format's): its
    /// review is private, and its brand can't be moved out. An [`Identity`]
    /// carries no operation, though: any `Confirm<Op, B>` takes one, with any
    /// brand its caller can make. So the caller's brand must be made only
    /// where each operation is reviewed, for that operation (Tansu: only
    /// `plan.rs` makes `Queued`, of each operation's own kind).
    ///
    /// ```compile_fail,E0603
    /// use aizome_sys::disks::{Format, Identity};
    /// fn as_format(flags_seen: Identity) {
    ///     let _ = aizome_sys::disks::confirm::Seen::<Format> { identity: flags_seen, op: Default::default() };
    /// }
    /// ```
    ///
    /// The identity read from the state the person saw holds the drive, so
    /// the confirmation doesn't hold for another stick at the same name:
    ///
    /// ```
    /// use aizome_sys::disks::{BlockDevice, Confirm, DisksState, Drive, NewPartitionTable, PartitionTable};
    /// /// Where the person sees the stick.
    /// struct Sheet;
    /// let disk = "/org/freedesktop/UDisks2/block_devices/sdb";
    /// let stick = |serial: &str, table: &str| DisksState {
    ///     drives: vec![Drive {
    ///         path: format!("/org/freedesktop/UDisks2/drives/SanDisk_Ultra_{serial}"),
    ///         serial: serial.into(),
    ///         size: 32_017_047_552,
    ///         ..Default::default()
    ///     }],
    ///     blocks: vec![BlockDevice {
    ///         path: disk.into(),
    ///         device: "/dev/sdb".into(),
    ///         size: 32_017_047_552,
    ///         drive: Some(format!("/org/freedesktop/UDisks2/drives/SanDisk_Ultra_{serial}")),
    ///         partition_table: Some(PartitionTable { kind: table.into(), partitions: Vec::new() }),
    ///         ..Default::default()
    ///     }],
    ///     ..Default::default()
    /// };
    /// let (a, b) = (stick("4C530001230101112233", "gpt"), stick("4C530001990304445566", "dos"));
    /// let confirm = || Confirm::<NewPartitionTable, Sheet>::new(a.identity(disk).unwrap(), Sheet);
    /// assert!(confirm().check(&b).is_err());
    /// assert_eq!(confirm().check(&a).unwrap().target().device(), "/dev/sdb");
    /// ```
    pub fn new(seen: Identity, brand: B) -> Confirm<Op, B> {
        Confirm { reviewed: Reviewed::new(Seen { identity: seen, op: PhantomData }, brand), at: Instant::now() }
    }

    /// The block device's object path.
    pub fn block(&self) -> &str {
        self.reviewed.target().identity.path()
    }

    /// Its node (`/dev/sdb`), as the person saw it.
    pub fn device(&self) -> &str {
        self.reviewed.target().identity.device()
    }

    /// How long ago the confirmation was made.
    pub fn age(&self) -> Duration {
        self.at.elapsed()
    }

    /// Spends the confirmation: the device as `state` has it now, if it is
    /// still the one confirmed and the confirmation isn't too old. The
    /// operation (or a stand-in for UDisks) acts on the proof's device.
    pub fn check(self, state: &DisksState) -> Result<Confirmed<Identity, B>> {
        if self.at.elapsed() > CONFIRMATION_LIFETIME {
            return Err(Error::InvalidArgument(format!(
                "The confirmation to {} {} is too old: ask again",
                Op::WHAT,
                self.device()
            )));
        }
        let fresh =
            state.identity(self.block()).ok_or_else(|| Error::NotFound(format!("{} is gone", self.device())))?;
        self.reviewed.confirm(fresh).map_err(|changed| {
            Error::InvalidArgument(format!(
                "{} changed since you agreed to {} it: ask again",
                changed.reviewed.identity.device(),
                Op::WHAT
            ))
        })
    }
}

/// A confirmation of `identity`, as a test reviews it (any brand will do).
#[cfg(test)]
fn confirm<Op: Change>(identity: Identity) -> Confirm<Op, ()> {
    Confirm::new(identity, ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disks::model::{BlockDevice, Drive, Partition};

    const DRIVE: &str = "/d/stick";

    fn block(uuid: &str) -> BlockDevice {
        BlockDevice {
            path: "/b/sdb1".into(),
            device: "/dev/sdb1".into(),
            size: 100,
            drive: Some(DRIVE.into()),
            id_uuid: uuid.into(),
            ..Default::default()
        }
    }

    fn on(drive: Drive, blocks: Vec<BlockDevice>) -> DisksState {
        DisksState { drives: vec![drive], blocks, ..Default::default() }
    }

    fn stick() -> Drive {
        Drive { path: DRIVE.into(), serial: "A".into(), time_detected: 1, ..Default::default() }
    }

    #[test]
    fn a_confirmation_holds_only_for_the_device_seen() {
        let seen = on(stick(), vec![block("aaaa")]);
        let confirm = || confirm::<Format>(seen.identity("/b/sdb1").unwrap());
        assert_eq!(confirm().check(&seen).unwrap().target(), &seen.identity("/b/sdb1").unwrap());
        let changed = on(stick(), vec![block("bbbb")]);
        assert!(matches!(confirm().check(&changed), Err(Error::InvalidArgument(_))));
        let gone = DisksState::default();
        assert!(matches!(confirm().check(&gone), Err(Error::NotFound(_))));
    }

    #[test]
    fn another_stick_at_the_same_name_is_another_target_even_as_a_copy() {
        // A copy of the stick (`dd`), with the same filesystem UUID, on a
        // stick of a model without serial numbers: only UDisks's detection
        // time tells them apart.
        let seen = on(stick(), vec![block("aaaa")]);
        let copy = on(Drive { serial: "A".into(), time_detected: 2, ..stick() }, vec![block("aaaa")]);
        assert!(confirm::<Format>(seen.identity("/b/sdb1").unwrap()).check(&copy).is_err());
        let another_card = on(Drive { time_media_detected: 9, ..stick() }, vec![block("aaaa")]);
        assert!(confirm::<Format>(seen.identity("/b/sdb1").unwrap()).check(&another_card).is_err());
    }

    #[test]
    fn a_copy_of_a_whole_disk_on_another_stick_gets_no_new_table() {
        // Two sticks of a model without serial numbers, the second a `dd`
        // copy of the first: same names, size, table and partitions. Only
        // UDisks's detection time differs.
        let disk = |detected: u64| {
            let drive = Drive { path: DRIVE.into(), time_detected: detected, ..Default::default() };
            let whole = BlockDevice {
                path: "/b/sdb".into(),
                device: "/dev/sdb".into(),
                size: 32_017_047_552,
                drive: Some(DRIVE.into()),
                partition_table: Some(crate::disks::model::PartitionTable {
                    kind: "dos".into(),
                    partitions: vec!["/b/sdb1".into()],
                }),
                ..Default::default()
            };
            let part = BlockDevice {
                path: "/b/sdb1".into(),
                device: "/dev/sdb1".into(),
                partition: Some(Partition {
                    table: "/b/sdb".into(),
                    number: 1,
                    offset: 1 << 20,
                    uuid: "4a3b2c1d-01".into(),
                    ..Default::default()
                }),
                ..block("aaaa")
            };
            on(drive, vec![whole, part])
        };
        let (first, copy) = (disk(1_792_000_000_000_000), disk(1_792_000_042_000_000));
        let confirm = || confirm::<NewPartitionTable>(first.identity("/b/sdb").unwrap());
        assert!(confirm().check(&first).is_ok());
        assert!(confirm().check(&copy).is_err(), "the copy is another stick");
    }

    #[test]
    fn another_usb_port_is_another_stick() {
        let port = |n: u8| format!("/dev/disk/by-path/pci-0000:00:14.0-usb-0:{n}:1.0-scsi-0:0:0:0");
        let stick = |links: Vec<String>| {
            let whole = BlockDevice {
                path: "/b/sdb".into(),
                device: "/dev/sdb".into(),
                size: 1 << 30,
                drive: Some(DRIVE.into()),
                symlinks: links,
                ..Default::default()
            };
            let part = BlockDevice {
                partition: Some(Partition { table: "/b/sdb".into(), number: 1, ..Default::default() }),
                drive: None,
                ..block("aaaa")
            };
            on(stick(), vec![whole, part])
        };
        let seen = stick(vec![port(2), "/dev/disk/by-id/usb-Stick-0:0".into()]);
        let flags = || confirm::<PartitionEntry>(seen.identity("/b/sdb1").unwrap());
        // Every kind holds a device to its port, flags (held only to place)
        // included.
        assert!(flags().check(&stick(vec![port(3)])).is_err(), "another port");
        assert!(flags().check(&stick(vec![port(2)])).is_ok(), "the same port");
        // While udev probes the disk again it may list no link: no port to
        // compare, so nothing is refused for it.
        assert!(flags().check(&stick(Vec::new())).is_ok());
        assert_eq!(seen.identity("/b/sdb1").unwrap().port.as_deref(), Some(&port(2)["/dev/disk/by-path/".len()..]));
    }

    #[test]
    fn what_a_confirmation_holds_depends_on_the_operation() {
        let part = |uuid: &str, size: u64| BlockDevice {
            size,
            partition: Some(Partition {
                table: "/b/sdb".into(),
                number: 1,
                offset: 1 << 20,
                size,
                ..Default::default()
            }),
            ..block(uuid)
        };
        let seen = on(stick(), vec![part("aaaa", 100)]);
        let identity = || seen.identity("/b/sdb1").unwrap();
        // Formatted (by the step before, in one plan): the flags still go on
        // the same partition, a format confirmed before it doesn't.
        let formatted = on(stick(), vec![part("cccc", 100)]);
        assert!(confirm::<PartitionEntry>(identity()).check(&formatted).is_ok());
        assert!(confirm::<Relabel>(identity()).check(&formatted).is_err());
        assert!(confirm::<Format>(identity()).check(&formatted).is_err());
        // Shrunk (by a resize's earlier step): the resize goes on, a delete
        // confirmed before it doesn't.
        let shrunk = on(stick(), vec![part("aaaa", 50)]);
        assert!(confirm::<Resize>(identity()).check(&shrunk).is_ok());
        assert!(confirm::<DeletePartition>(identity()).check(&shrunk).is_err());
        // Moved: nothing holds.
        let mut moved = part("aaaa", 100);
        moved.partition.as_mut().unwrap().offset = 2 << 20;
        let moved = on(stick(), vec![moved]);
        assert!(confirm::<PartitionEntry>(identity()).check(&moved).is_err());
    }
}
#[cfg(test)]
#[path = "repro_sya_13.rs"]
mod repro_sya_13;
