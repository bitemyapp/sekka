//! Drives are mounted as they come, once each: never one someone
//! unmounted, the system's own, a locked one or an attached image.
use aizome_sys::disks::{Automount, BlockDevice, DisksState, Drive, Encrypted, Filesystem, LoopDevice};

fn stick(n: u32, mounted: bool) -> (Drive, BlockDevice) {
    let drive = Drive { path: format!("stick{n}"), removable: true, ejectable: true, ..Default::default() };
    let block = BlockDevice {
        path: format!("sd{n}"),
        drive: Some(drive.path.clone()),
        id_label: format!("STICK{n}"),
        id_uuid: format!("{n:04}-ABCD"),
        hint_auto: true,
        filesystem: Some(Filesystem {
            mount_points: if mounted { vec![format!("/run/media/u/STICK{n}")] } else { vec![] },
            ..Default::default()
        }),
        ..Default::default()
    };
    (drive, block)
}

fn state(sticks: &[&(Drive, BlockDevice)]) -> DisksState {
    DisksState {
        available: true,
        drives: sticks.iter().map(|s| s.0.clone()).collect(),
        blocks: sticks.iter().map(|s| s.1.clone()).collect(),
        ..Default::default()
    }
}

fn planned(a: &mut Automount, s: &DisksState) -> Vec<String> {
    a.plan(s).into_iter().map(|v| v.block).collect()
}

#[test]
fn a_drive_plugged_in_is_mounted_once() {
    let mut a = Automount::new();
    assert!(planned(&mut a, &DisksState::default()).is_empty(), "UDisks not read yet");
    let one = stick(1, false);
    assert_eq!(planned(&mut a, &state(&[&one])), ["sd1"]);
    // Still being mounted, or failed: not again.
    assert!(planned(&mut a, &state(&[&one])).is_empty());
    // Mounted, then unmounted by someone: left alone.
    assert!(planned(&mut a, &state(&[&stick(1, true)])).is_empty());
    assert!(planned(&mut a, &state(&[&one])).is_empty());
    // Unplugged and plugged in again: new.
    assert!(planned(&mut a, &state(&[])).is_empty());
    assert_eq!(planned(&mut a, &state(&[&one])), ["sd1"]);
    // Asked to try again.
    a.try_again("sd1");
    assert_eq!(planned(&mut a, &state(&[&one])), ["sd1"]);
}

#[test]
fn a_filesystem_that_drops_out_of_view_for_a_moment_is_not_mounted_again() {
    // UDisks can drop a block's filesystem for a moment, probing a drive
    // just plugged in or as it's mounted: the drive is still there, and
    // mounting it again would be refused as already mounted.
    let mut a = Automount::new();
    let one = stick(1, false);
    assert_eq!(planned(&mut a, &state(&[&one])), ["sd1"]);
    let mut probing = stick(1, false);
    probing.1.filesystem = None;
    assert!(planned(&mut a, &state(&[&probing])).is_empty());
    assert!(planned(&mut a, &state(&[&one])).is_empty(), "the same drive");
}

#[test]
fn a_drive_mounted_when_first_seen_stays_as_it_is() {
    let mut a = Automount::new();
    let two = stick(2, false);
    assert_eq!(planned(&mut a, &state(&[&stick(1, true), &two])), ["sd2"]);
    // The first unmounted later (by Hako, say): not mounted again.
    assert!(planned(&mut a, &state(&[&stick(1, false), &two])).is_empty());
}

#[test]
fn only_what_udisks_says_to_mount_by_itself() {
    let mut a = Automount::new();
    let mut manual = stick(1, false);
    manual.1.hint_auto = false;
    let mut system = stick(2, false);
    system.1.hint_system = true;
    let mut ignored = stick(3, false);
    ignored.1.hint_ignore = true;
    let mut locked = stick(4, false);
    locked.1.filesystem = None;
    locked.1.encrypted = Some(Encrypted::default());
    let mut image = stick(5, false);
    image.1.drive = None;
    image.1.loop_device = Some(LoopDevice { backing_file: "/home/u/disc.iso".into(), ..Default::default() });
    let s = state(&[&manual, &system, &ignored, &locked, &image]);
    assert_eq!(s.volumes().len(), 3, "the manual stick, the locked one and the image show");
    assert!(planned(&mut a, &s).is_empty());
}
