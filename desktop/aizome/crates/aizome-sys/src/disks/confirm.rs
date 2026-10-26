//! Confirmations for operations that destroy data.
//!
//! Every destructive [`Disks`](super::Disks) method takes a [`Confirm<Op>`],
//! which only the UI makes, from the device as the person saw it when they
//! agreed: `Confirm::<Format>::new(&block)`. The operation checks it against
//! the device as it is when it runs, and refuses if anything that
//! identifies it changed (another disk now at that path, a partition moved,
//! a new filesystem with a new UUID) or the confirmation is older than ten
//! minutes. A confirmation is used once: it is moved into the call.

use super::model::{BlockDevice, DisksState};
use crate::error::{Error, Result};
use std::marker::PhantomData;
use std::time::{Duration, Instant};

/// How long a confirmation stays good.
pub const CONFIRMATION_LIFETIME: Duration = Duration::from_secs(600);

/// An operation that destroys data.
pub trait Destructive {
    /// What it is, for messages ("format").
    const WHAT: &'static str;
}

/// Make a new filesystem (or wipe, or encrypt): everything on the device is
/// lost.
pub struct Format;
/// Make a new partition table: every partition on the disk is lost.
pub struct NewPartitionTable;
/// Delete a partition and what is on it.
pub struct DeletePartition;
/// Change a partition's or filesystem's size: shrinking can lose what is
/// past the new end.
pub struct Resize;
/// Repair a filesystem: the repair may discard what it can't save.
pub struct Repair;

impl Destructive for Format {
    const WHAT: &'static str = "format";
}
impl Destructive for NewPartitionTable {
    const WHAT: &'static str = "create a partition table on";
}
impl Destructive for DeletePartition {
    const WHAT: &'static str = "delete";
}
impl Destructive for Resize {
    const WHAT: &'static str = "resize";
}
impl Destructive for Repair {
    const WHAT: &'static str = "repair";
}

/// What identifies a block device's contents at the moment of confirming.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Fingerprint {
    path: String,
    device: String,
    size: u64,
    id_type: String,
    id_uuid: String,
    partition: Option<(String, u64, u64)>,
}

impl Fingerprint {
    fn of(block: &BlockDevice) -> Fingerprint {
        Fingerprint {
            path: block.path.clone(),
            device: block.device.clone(),
            size: block.size,
            id_type: block.id_type.clone(),
            id_uuid: block.id_uuid.clone(),
            partition: block.partition.as_ref().map(|p| (p.table.clone(), p.offset, p.size)),
        }
    }
}

/// The person agreed to `Op` on this device, as it was when they agreed.
#[must_use = "a confirmation does nothing until it is passed to the operation"]
pub struct Confirm<Op: Destructive> {
    fingerprint: Fingerprint,
    at: Instant,
    _op: PhantomData<fn() -> Op>,
}

impl<Op: Destructive> std::fmt::Debug for Confirm<Op> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Confirm").field("op", &Op::WHAT).field("device", &self.fingerprint.device).finish()
    }
}

impl<Op: Destructive> Confirm<Op> {
    /// The person confirmed `Op` on `block`, as shown to them.
    pub fn new(block: &BlockDevice) -> Confirm<Op> {
        Confirm { fingerprint: Fingerprint::of(block), at: Instant::now(), _op: PhantomData }
    }

    /// The block device's object path.
    pub fn block(&self) -> &str {
        &self.fingerprint.path
    }

    /// The device as it is now, if it is still the one confirmed.
    pub(crate) fn check<'s>(&self, state: &'s DisksState) -> Result<&'s BlockDevice> {
        if self.at.elapsed() > CONFIRMATION_LIFETIME {
            return Err(Error::InvalidArgument(format!(
                "The confirmation to {} {} is too old: ask again",
                Op::WHAT,
                self.fingerprint.device
            )));
        }
        let block = state
            .block(&self.fingerprint.path)
            .ok_or_else(|| Error::NotFound(format!("{} is gone", self.fingerprint.device)))?;
        if Fingerprint::of(block) != self.fingerprint {
            return Err(Error::InvalidArgument(format!(
                "{} changed since you agreed to {} it: ask again",
                self.fingerprint.device,
                Op::WHAT
            )));
        }
        Ok(block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(uuid: &str) -> BlockDevice {
        BlockDevice {
            path: "/b/sdb1".into(),
            device: "/dev/sdb1".into(),
            size: 100,
            id_uuid: uuid.into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_confirmation_holds_only_for_the_device_seen() {
        let confirm = Confirm::<Format>::new(&block("aaaa"));
        let same = DisksState { blocks: vec![block("aaaa")], ..Default::default() };
        assert_eq!(confirm.check(&same).unwrap().id_uuid, "aaaa");
        let changed = DisksState { blocks: vec![block("bbbb")], ..Default::default() };
        assert!(matches!(confirm.check(&changed), Err(Error::InvalidArgument(_))));
        let gone = DisksState::default();
        assert!(matches!(confirm.check(&gone), Err(Error::NotFound(_))));
    }
}
