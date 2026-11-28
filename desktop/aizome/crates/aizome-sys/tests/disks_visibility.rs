//! Desktop visibility uses the encrypted medium's policy, never just dm-crypt's default hint.
//! A file manager's sidebar adds the volumes of built-in drives the system doesn't run from.
use aizome_sys::disks::{BlockDevice, DisksState, Drive, Encrypted, Filesystem, Partition, Swap};

fn usb() -> DisksState {
    DisksState {
        available: true,
        drives: vec![Drive { path: "usb-drive".into(), removable: true, ejectable: true, ..Default::default() }],
        blocks: vec![
            BlockDevice { path: "usb".into(), drive: Some("usb-drive".into()), ..Default::default() },
            BlockDevice {
                path: "encrypted".into(),
                partition: Some(Partition { table: "usb".into(), ..Default::default() }),
                encrypted: Some(Encrypted { cleartext_device: Some("cleartext".into()), ..Default::default() }),
                ..Default::default()
            },
            BlockDevice {
                path: "cleartext".into(),
                hint_system: true,
                crypto_backing_device: Some("encrypted".into()),
                id_label: "PRIVATE".into(),
                filesystem: Some(Filesystem {
                    mount_points: vec!["/run/media/u/PRIVATE".into()],
                    ..Default::default()
                }),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

#[test]
fn encrypted_removable_volume_stays_visible_across_unlock_mount_and_lock() {
    let mut state = usb();
    let unlocked = state.volumes();
    assert_eq!(unlocked.len(), 1);
    assert_eq!(unlocked[0].block, "cleartext");
    assert_eq!(unlocked[0].name, "PRIVATE");
    assert_eq!(unlocked[0].mount_point.as_deref(), Some("/run/media/u/PRIVATE"));
    assert_eq!(unlocked[0].drive.as_deref(), Some("usb-drive"));
    assert!(unlocked[0].ejectable && !unlocked[0].locked && !unlocked[0].system);
    state.blocks[2].filesystem.as_mut().unwrap().mount_points.clear();
    assert_eq!(state.volumes()[0].mount_point, None);
    let before = state.clone();
    state.blocks.pop();
    state.blocks[1].encrypted.as_mut().unwrap().cleartext_device = None;
    let locked = state.volumes();
    assert_eq!(locked.len(), 1);
    assert_eq!(locked[0].block, "encrypted");
    assert!(locked[0].locked);
    let changes = state.volume_changes(&before);
    assert_eq!(changes.added, locked);
    assert_eq!(changes.removed[0].block, "cleartext");
}

#[test]
fn encrypted_system_and_ignored_ancestors_stay_hidden() {
    for ignored in 0..3 {
        let mut state = usb();
        state.blocks[ignored].hint_ignore = true;
        assert!(state.volumes().is_empty(), "ignored ancestor{ignored}");
    }
    let mut state = usb();
    state.blocks[1].hint_system = true;
    assert!(state.volumes().is_empty());
    state.blocks[2].hint_system = false;
    assert!(state.volumes().is_empty(), "cleartext cannot expose system backing");
    state.blocks[2].filesystem.as_mut().unwrap().mount_points = vec!["/".into()];
    let desktop = state.desktop_volumes("/home/u");
    assert_eq!(desktop.len(), 1);
    assert!(desktop[0].system && !desktop[0].ejectable);
}

#[test]
fn nested_crypto_inherits_policy_but_ordinary_partition_hints_are_preserved() {
    let mut state = usb();
    let mut nested = state.blocks[2].clone();
    nested.path = "nested".into();
    nested.crypto_backing_device = Some("cleartext".into());
    state.blocks[2].filesystem = None;
    state.blocks[2].encrypted = Some(Encrypted { cleartext_device: Some("nested".into()), ..Default::default() });
    state.blocks.push(nested);
    assert_eq!(state.volumes()[0].block, "nested");
    state.blocks[1].hint_system = true;
    assert!(state.volumes().is_empty());
    let mut state = usb();
    state.blocks[2].crypto_backing_device = None;
    state.blocks[2].partition = Some(Partition { table: "usb".into(), ..Default::default() });
    assert!(state.volumes().is_empty(), "explicit system partition on external disk stays hidden");
    state.blocks[2].hint_system = false;
    assert_eq!(state.volumes()[0].block, "cleartext");
}

#[test]
fn missing_and_cyclic_crypto_ancestry_is_hidden() {
    for backing in ["missing", "cleartext"] {
        let mut state = usb();
        state.blocks[2].crypto_backing_device = Some(backing.into());
        assert!(state.volumes().is_empty());
    }
    let mut state = usb();
    state.blocks[1].partition.as_mut().unwrap().table = "cleartext".into();
    assert!(state.volumes().is_empty());
}

const GIB: u64 = 1 << 30;
const LINUX: &str = "0fc63daf-8483-4772-8e79-3d69d8477de4";
const BASIC_DATA: &str = "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7";
const ESP: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";
const SWAP: &str = "0657fd6d-a4ab-43c4-84e5-0933c84b4f4f";
const LUKS: &str = "ca7d7ccb-63ed-4c53-861c-1742536059cc";

/// A built-in drive's whole-disk device: HintSystem, as UDisks marks all of it.
fn disk(path: &str, drive: &str) -> BlockDevice {
    BlockDevice { path: path.into(), drive: Some(drive.into()), hint_system: true, ..Default::default() }
}

/// A partition of a built-in drive, by what is on it: swap (in use), a LUKS
/// container (locked), or a filesystem mounted at `mounts`.
fn part(path: &str, table: &str, type_id: &str, id_type: &str, size: u64, mounts: &[&str]) -> BlockDevice {
    let filesystem =
        Filesystem { mount_points: mounts.iter().map(|m| (*m).to_owned()).collect(), ..Default::default() };
    BlockDevice {
        path: path.into(),
        size,
        id_type: id_type.into(),
        hint_system: true,
        partition: Some(Partition { table: table.into(), type_id: type_id.into(), ..Default::default() }),
        filesystem: (id_type != "swap" && id_type != "crypto_LUKS").then_some(filesystem),
        swap: (id_type == "swap").then_some(Swap { active: true }),
        encrypted: (id_type == "crypto_LUKS").then(Encrypted::default),
        ..Default::default()
    }
}

/// An unlocked container's cleartext device, with a filesystem that isn't mounted.
fn cleartext(path: &str, backing: &str) -> BlockDevice {
    BlockDevice {
        path: path.into(),
        hint_system: true,
        crypto_backing_device: Some(backing.into()),
        filesystem: Some(Filesystem::default()),
        ..Default::default()
    }
}

/// The machine of issue 345, from its `lsblk`: the system's NVMe with
/// `/boot`, `/` (and `/nix/store`) and swap, and a second NVMe with one
/// Btrfs filesystem that isn't mounted.
fn two_nvmes() -> DisksState {
    DisksState {
        available: true,
        drives: vec![
            Drive { path: "nvme0".into(), ..Default::default() },
            Drive { path: "nvme1".into(), ..Default::default() },
        ],
        blocks: vec![
            disk("nvme0n1", "nvme0"),
            part("nvme0n1p1", "nvme0n1", ESP, "vfat", GIB, &["/boot"]),
            part("nvme0n1p2", "nvme0n1", LINUX, "btrfs", 9145 * GIB / 10, &["/nix/store", "/"]),
            part("nvme0n1p3", "nvme0n1", SWAP, "swap", 16 * GIB, &[]),
            disk("nvme1n1", "nvme1"),
            part("nvme1n1p1", "nvme1n1", LINUX, "btrfs", 4769 * GIB / 10, &[]),
        ],
        ..Default::default()
    }
}

fn block<'a>(state: &'a mut DisksState, path: &str) -> &'a mut BlockDevice {
    state.blocks.iter_mut().find(|b| b.path == path).unwrap()
}

fn mounts<'a>(state: &'a mut DisksState, path: &str) -> &'a mut Vec<String> {
    &mut block(state, path).filesystem.as_mut().unwrap().mount_points
}

/// The blocks of the internal data volumes, in device order.
fn internal(state: &DisksState) -> Vec<String> {
    let mut blocks: Vec<String> = state.internal_volumes().into_iter().map(|v| v.block).collect();
    blocks.sort();
    blocks
}

#[test]
fn second_internal_drive_adds_only_its_data_partition() {
    let mut state = two_nvmes();
    assert!(state.volumes().is_empty(), "the desktop's list is as it was");
    assert_eq!(state.desktop_volumes("/home/u").len(), 1, "the system's disk alone");
    let found = state.internal_volumes();
    assert_eq!(found.len(), 1, "never the ESP, swap or root: {found:?}");
    let data = &found[0];
    assert_eq!((data.block.as_str(), data.name.as_str()), ("nvme1n1p1", "512 GB Volume"));
    assert_eq!((data.mount_point.as_deref(), data.drive.as_deref()), (None, Some("nvme1")));
    assert_eq!(data.icon_name, "drive-harddisk");
    assert!(data.internal && !data.locked && !data.system && data.image_file.is_none());
    // The system's own partitions are no more to be shown when they are
    // idle: an ESP mounted on demand, swap that is off.
    mounts(&mut state, "nvme0n1p1").clear();
    block(&mut state, "nvme0n1p3").swap = Some(Swap { active: false });
    assert_eq!(internal(&state), ["nvme1n1p1"]);
    // Mounted where UDisks mounts for people, or by hand under /mnt, it
    // stays (to be unmounted), and whatever its drive claims it is never
    // something to eject or power off.
    state.drives[1].ejectable = true;
    state.drives[1].can_power_off = true;
    for point in ["/run/media/u/data", "/media/data", "/mnt/data", "/mnt"] {
        *mounts(&mut state, "nvme1n1p1") = vec![point.into()];
        let shown = state.internal_volumes();
        assert_eq!(shown.len(), 1, "{point}");
        assert_eq!((shown[0].block.as_str(), shown[0].mount_point.as_deref()), ("nvme1n1p1", Some(point)));
        assert!(!shown[0].ejectable && !shown[0].can_power_off);
    }
    assert!(state.volumes().is_empty() && state.volume_changes(&two_nvmes()).added.is_empty());
}

#[test]
fn encrypted_volume_on_a_second_internal_drive() {
    let mut state = two_nvmes();
    *block(&mut state, "nvme1n1p1") = part("nvme1n1p1", "nvme1n1", LUKS, "crypto_LUKS", 4769 * GIB / 10, &[]);
    let locked = state.internal_volumes();
    assert_eq!(locked.len(), 1);
    assert_eq!((locked[0].block.as_str(), locked[0].name.as_str()), ("nvme1n1p1", "512 GB Encrypted"));
    assert!(locked[0].locked && locked[0].internal && !locked[0].ejectable);
    // Unlocked, it shows through its cleartext device, like a removable one.
    block(&mut state, "nvme1n1p1").encrypted.as_mut().unwrap().cleartext_device = Some("dm-1".into());
    state.blocks.push(BlockDevice { id_label: "VAULT".into(), ..cleartext("dm-1", "nvme1n1p1") });
    let unlocked = state.internal_volumes();
    assert_eq!(unlocked.len(), 1);
    assert_eq!((unlocked[0].block.as_str(), unlocked[0].name.as_str()), ("dm-1", "VAULT"));
    assert_eq!((unlocked[0].locked, unlocked[0].drive.as_deref()), (false, Some("nvme1")));
    assert!(state.volumes().is_empty());
    // The root filesystem's own container and its cleartext device never
    // show, nor a locked container beside them on the system's drive.
    let mut state = two_nvmes();
    let mut root = part("nvme0n1p2", "nvme0n1", LUKS, "crypto_LUKS", 9145 * GIB / 10, &[]);
    root.encrypted = Some(Encrypted { cleartext_device: Some("dm-0".into()), ..Default::default() });
    *block(&mut state, "nvme0n1p2") = root;
    state.blocks.push(cleartext("dm-0", "nvme0n1p2"));
    *mounts(&mut state, "dm-0") = vec!["/nix/store".into(), "/".into()];
    state.blocks.push(part("nvme0n1p4", "nvme0n1", LUKS, "crypto_LUKS", 100 * GIB, &[]));
    assert_eq!(internal(&state), ["nvme1n1p1"]);
    // An unlocked root that can't be traced to its drive: nothing is shown.
    block(&mut state, "dm-0").crypto_backing_device = None;
    assert!(state.internal_volumes().is_empty());
}

#[test]
fn ignored_second_drive_stays_hidden() {
    for ignored in ["nvme1n1", "nvme1n1p1"] {
        let mut state = two_nvmes();
        block(&mut state, ignored).hint_ignore = true;
        assert!(state.internal_volumes().is_empty(), "{ignored}");
    }
    // Encrypted: the disk's, the container's and the cleartext device's.
    for ignored in ["nvme1n1", "nvme1n1p1", "dm-1"] {
        let mut state = two_nvmes();
        let mut container = part("nvme1n1p1", "nvme1n1", LUKS, "crypto_LUKS", GIB, &[]);
        container.encrypted = Some(Encrypted { cleartext_device: Some("dm-1".into()), ..Default::default() });
        *block(&mut state, "nvme1n1p1") = container;
        state.blocks.push(cleartext("dm-1", "nvme1n1p1"));
        assert_eq!(internal(&state), ["dm-1"]);
        block(&mut state, ignored).hint_ignore = true;
        assert!(state.internal_volumes().is_empty(), "{ignored}");
    }
}

#[test]
fn data_partition_on_the_system_drive_stays_hidden() {
    // The rule goes by drive, to be safe: nothing on a drive the system
    // runs from is offered, not even a partition the system doesn't use.
    let mut state = two_nvmes();
    state.blocks.push(part("nvme0n1p4", "nvme0n1", LINUX, "ext4", 100 * GIB, &[]));
    assert_eq!(internal(&state), ["nvme1n1p1"]);
    *mounts(&mut state, "nvme0n1p4") = vec!["/run/media/u/scratch".into()];
    assert_eq!(internal(&state), ["nvme1n1p1"]);
    // Each of `/boot` and `/nix/store` alone makes its drive the system's,
    // as `/` does.
    for (partition, point) in [("nvme0n1p1", "/boot"), ("nvme0n1p2", "/nix/store")] {
        let mut state = two_nvmes();
        mounts(&mut state, partition).retain(|m| m != point);
        state.blocks.push(part("nvme1n1p2", "nvme1n1", LINUX, "ext4", GIB, &[point]));
        assert!(state.internal_volumes().is_empty(), "{point} on the second drive");
    }
}

#[test]
fn a_drive_the_system_uses_is_left_out_whole() {
    // Any filesystem mounted outside the media folders.
    for point in ["/home", "/nix", "/var/lib/postgresql", "/data", "/home/u/games", "/mnt-backup", "/run/mediax"] {
        let mut state = two_nvmes();
        state.blocks.push(part("nvme1n1p2", "nvme1n1", LINUX, "ext4", GIB, &[point]));
        assert!(state.internal_volumes().is_empty(), "{point} beside the data");
        let mut state = two_nvmes();
        *mounts(&mut state, "nvme1n1p1") = vec!["/run/media/u/data".into(), point.into()];
        assert!(state.internal_volumes().is_empty(), "the data itself at {point}");
        // Even with drive links that disagree (the partition's and its
        // disk's), what the system has mounted is never listed.
        block(&mut state, "nvme1n1p1").drive = Some("nvme0".into());
        assert!(state.internal_volumes().is_empty(), "the data itself at {point}, whatever its drive");
    }
    // Swap in use; swap that is off hides nothing and isn't listed itself.
    let mut state = two_nvmes();
    state.blocks.push(part("nvme1n1p2", "nvme1n1", LINUX, "swap", GIB, &[]));
    assert!(state.internal_volumes().is_empty());
    block(&mut state, "nvme1n1p2").swap = Some(Swap { active: false });
    assert_eq!(internal(&state), ["nvme1n1p1"]);
    // Another device of the root filesystem (Btrfs across both drives): it
    // has the filesystem's UUID and no mount point of its own.
    let mut state = two_nvmes();
    for member in ["nvme0n1p2", "nvme1n1p1"] {
        block(&mut state, member).id_uuid = "727a7c93-6c86-45fa-949a-9e0c766e0eed".into();
    }
    state.blocks.push(part("nvme1n1p2", "nvme1n1", LINUX, "ext4", GIB, &[]));
    assert!(state.internal_volumes().is_empty(), "the member, and the rest of its drive");
    block(&mut state, "nvme1n1p1").id_uuid = "e6cd9e43-db4e-4f71-9784-298207c4d982".into();
    assert_eq!(internal(&state), ["nvme1n1p1", "nvme1n1p2"]);
}

#[test]
fn firmware_boot_recovery_and_swap_partitions_stay_hidden_on_any_drive() {
    let reserved = [
        ESP,
        "C12A7328-F81F-11D2-BA4B-00A0C93EC93B",
        "21686148-6449-6e6f-744e-656564454649",
        "bc13c2ff-59e6-4262-a352-b275fd6f7172",
        SWAP,
        "e3c9e316-0b5c-4db8-817d-f92df00215ae",
        "de94bba4-06d1-4d40-a16a-bfd50179d6ac",
        "0xef",
        "0x27",
    ];
    for type_id in reserved {
        let mut state = two_nvmes();
        state.blocks.push(part("nvme1n1p2", "nvme1n1", type_id, "vfat", GIB, &[]));
        assert_eq!(internal(&state), ["nvme1n1p1"], "{type_id}");
        // Nor what an encrypted one holds, locked or unlocked.
        *block(&mut state, "nvme1n1p2") = part("nvme1n1p2", "nvme1n1", type_id, "crypto_LUKS", GIB, &[]);
        assert_eq!(internal(&state), ["nvme1n1p1"], "{type_id}, locked");
        block(&mut state, "nvme1n1p2").encrypted.as_mut().unwrap().cleartext_device = Some("dm-1".into());
        state.blocks.push(cleartext("dm-1", "nvme1n1p2"));
        assert_eq!(internal(&state), ["nvme1n1p1"], "{type_id}, unlocked");
    }
    // By GPT attribute: required by the platform, or hidden.
    for flags in [1, 1 << 62] {
        let mut state = two_nvmes();
        state.blocks.push(part("nvme1n1p2", "nvme1n1", BASIC_DATA, "ntfs", GIB, &[]));
        assert_eq!(internal(&state), ["nvme1n1p1", "nvme1n1p2"], "another system's data is data");
        block(&mut state, "nvme1n1p2").partition.as_mut().unwrap().flags = flags;
        assert_eq!(internal(&state), ["nvme1n1p1"], "flags {flags:#x}");
    }
}

#[test]
fn nothing_is_listed_unless_root_and_the_volume_are_traced_to_drives() {
    // `/` is no disk here (a container, a live image).
    let mut state = two_nvmes();
    mounts(&mut state, "nvme0n1p2").clear();
    assert!(state.internal_volumes().is_empty());
    // `/` on a device with no drive of its own (LVM).
    state.blocks.push(BlockDevice { crypto_backing_device: None, ..cleartext("dm-0", "") });
    *mounts(&mut state, "dm-0") = vec!["/".into()];
    assert!(state.internal_volumes().is_empty());
    // The system's drive is one UDisks doesn't describe.
    let mut state = two_nvmes();
    state.drives.remove(0);
    assert!(state.internal_volumes().is_empty());
    // The volume's own ancestry is missing, cyclic, or ends at no drive.
    for table in ["missing", "nvme1n1p1"] {
        let mut state = two_nvmes();
        block(&mut state, "nvme1n1p1").partition.as_mut().unwrap().table = table.into();
        assert!(state.internal_volumes().is_empty(), "table {table}");
    }
    let mut state = two_nvmes();
    block(&mut state, "nvme1n1").drive = None;
    assert!(state.internal_volumes().is_empty());
    let mut state = two_nvmes();
    state.drives.pop();
    assert!(state.internal_volumes().is_empty());
    // A volume with no drive at all (LVM, RAID, a loop device).
    let mut state = two_nvmes();
    state.blocks.push(BlockDevice { crypto_backing_device: None, ..cleartext("dm-3", "") });
    assert_eq!(internal(&state), ["nvme1n1p1"]);
}

#[test]
fn removable_volumes_stay_as_they_were_and_nothing_is_listed_twice() {
    let mut state = two_nvmes();
    let stick = usb();
    state.drives.extend(stick.drives);
    state.blocks.extend(stick.blocks);
    let removable = state.volumes();
    assert_eq!(removable, usb().volumes(), "the same list, with or without the second drive");
    assert_eq!(removable.len(), 1);
    assert!(removable[0].ejectable && !removable[0].internal);
    assert_eq!(internal(&state), ["nvme1n1p1"]);
    // A partition marked as the system's on an external disk stays hidden
    // from both lists: external and removable drives are not this category.
    block(&mut state, "encrypted").hint_system = true;
    assert!(state.volumes().is_empty());
    assert_eq!(internal(&state), ["nvme1n1p1"]);
    for (removable, bus, optical) in [(true, "", false), (false, "usb", false), (false, "", true)] {
        let mut state = two_nvmes();
        state.drives[1].media_removable = removable;
        state.drives[1].connection_bus = bus.into();
        state.drives[1].optical = optical;
        assert!(state.internal_volumes().is_empty());
    }
    // A built-in partition whose HintSystem the administrator cleared (a
    // udev rule) is an ordinary volume already: once, in the first list.
    let mut state = two_nvmes();
    block(&mut state, "nvme1n1p1").hint_system = false;
    assert_eq!(state.volumes().len(), 1);
    assert!(state.internal_volumes().is_empty());
}
