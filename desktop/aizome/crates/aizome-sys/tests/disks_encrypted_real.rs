//! Read-only API assertions after the disposable VM seeds actual USB/internal LUKS devices.
use aizome_sys::{Wake, disks::Disks};
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires the disposable encrypted-volume VM"]
fn encrypted_volume_visibility() {
    assert_eq!(std::fs::read_to_string("/etc/aizome-encryption-test-vm").unwrap(), "disposable-encryption-test\n");
    let disks = Disks::new(Wake::none());
    let deadline = Instant::now() + Duration::from_secs(10);
    let state = loop {
        let state = disks.snapshot();
        if ["QA_PRIVATE", "QA_SYSTEM", "QA_IGNORED"]
            .iter()
            .all(|label| state.blocks.iter().any(|b| b.id_label == *label))
        {
            break state;
        }
        assert!(Instant::now() < deadline, "real encrypted devices did not appear: {state:?}");
        std::thread::sleep(Duration::from_millis(100));
    };
    println!("REAL_UDISKS_STATE {state:#?}");
    let volumes = state.volumes();
    println!("VISIBLE_VOLUMES {volumes:#?}");
    for label in ["QA_PRIVATE", "QA_SYSTEM", "QA_IGNORED"] {
        let clear = state.blocks.iter().find(|b| b.id_label == label).unwrap();
        assert!(clear.hint_system, "expected real dm-crypt default for{label}");
        assert!(!clear.hint_ignore);
        let backing = state.block(clear.crypto_backing_device.as_deref().unwrap()).unwrap();
        if label == "QA_SYSTEM" {
            assert!(backing.hint_system, "internal encrypted partition policy");
        } else {
            assert!(!backing.hint_system, "USB encrypted partition policy");
            assert!(backing.partition.is_some());
            let disk = state.block(&backing.partition.as_ref().unwrap().table).unwrap();
            assert_eq!(disk.hint_ignore, label == "QA_IGNORED");
        }
        let shown = volumes.iter().find(|v| v.block == clear.path);
        if label == "QA_PRIVATE" {
            let volume = shown.expect("unlocked removable volume must stay visible");
            assert_eq!(volume.name, label);
            assert!(!volume.locked && !volume.system);
            assert_eq!(volume.drive.as_deref(), Some(state.drive_of(clear).unwrap().path.as_str()));
            assert_eq!(volume.mount_point, clear.filesystem.as_ref().unwrap().mount_points.first().cloned());
        } else {
            assert!(shown.is_none(), "system and ignored encrypted volumes must stay hidden: {label}");
        }
    }
    println!("PASS actual removable identity and system/ignored ancestry exclusions");
}
