//! Desktop visibility uses the encrypted medium's policy, never just dm-crypt's default hint.
use aizome_sys::disks::{BlockDevice, DisksState, Drive, Encrypted, Filesystem, Partition};

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
