//! SYA-13: a confirmation to make a new partition table, given for USB
//! stick A, must not hold after A is swapped for stick B of the same size
//! that comes up at the same kernel name. A whole partitioned disk has
//! empty `IdType`/`IdUUID` and no `Partition`, so path, device, size and
//! the empty fields all match: when the confirmation kept only those, B
//! would have been wiped. It now holds the stick's identity, with its
//! drive and its partition table.
//!
//! Fake disk records only: the matching logic, `Confirm::check`, run
//! against two `DisksState`s.

use super::*;
use crate::disks::model::{BlockDevice, Drive, PartitionTable};

const BLOCK: &str = "/org/freedesktop/UDisks2/block_devices/sdb";
const SIZE: u64 = 32_017_047_552;

fn stick(serial: &str, table: &str) -> (Drive, BlockDevice) {
    let drive = Drive {
        path: format!("/org/freedesktop/UDisks2/drives/SanDisk_Ultra_{serial}"),
        vendor: "SanDisk".into(),
        model: "Ultra".into(),
        serial: serial.into(),
        size: SIZE,
        removable: true,
        connection_bus: "usb".into(),
        ..Default::default()
    };
    let block = BlockDevice {
        path: BLOCK.into(),
        device: "/dev/sdb".into(),
        preferred_device: "/dev/sdb".into(),
        size: SIZE,
        drive: Some(drive.path.clone()),
        // A whole disk with partitions on it: no filesystem of its own.
        id_usage: String::new(),
        id_type: String::new(),
        id_uuid: String::new(),
        partition_table: Some(PartitionTable { kind: table.into(), partitions: vec![format!("{BLOCK}1")] }),
        partition: None,
        ..Default::default()
    };
    (drive, block)
}

fn state(drive: Drive, block: BlockDevice) -> DisksState {
    DisksState { drives: vec![drive], blocks: vec![block], ..Default::default() }
}

#[test]
fn a_confirmation_for_one_stick_does_not_hold_for_another_at_the_same_name() {
    let (drive_a, a) = stick("4C530001230101112233", "gpt");
    let (drive_b, b) = stick("4C530001990304445566", "dos");
    // The person agreed to a new partition table on A, as shown (a
    // confirmation is spent by its check: one to check against A, one to
    // check against B).
    let seen = state(drive_a, a.clone());
    let confirm = || confirm::<NewPartitionTable>(seen.identity(BLOCK).expect("A"));
    assert!(confirm().check(&seen).is_ok(), "A as agreed to");
    // A is pulled out and B, another stick of the same size, comes up as
    // sdb: same object path, same device node.
    let now = state(drive_b, b.clone());
    let checked = confirm().check(&now);
    eprintln!(
        "A: drive {:?}, table {:?}; B: drive {:?}, table {:?}; check against B: {:?}",
        a.drive,
        a.partition_table.as_ref().map(|t| &t.kind),
        b.drive,
        b.partition_table.as_ref().map(|t| &t.kind),
        checked.as_ref().map(|c| c.target().device())
    );
    assert!(checked.is_err(), "the confirmation given for stick A must not let stick B be wiped");
}
