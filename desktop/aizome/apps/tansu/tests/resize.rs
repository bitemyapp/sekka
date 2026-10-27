//! Ordering and partial-failure contracts for confirmed resize plans.
//! The backend is in memory; real filesystem coverage lives in the VM.
use tansu::backend::Backend;
use tansu::mock::Mock;
use tansu::plan::Plan;
use tansu::run::{Outcome, Run, Status};

const GIB: u64 = 1 << 30;
const MIB: u64 = 1 << 20;
const DISK: &str = "/org/freedesktop/UDisks2/block_devices/sda";
const PART: &str = "/org/freedesktop/UDisks2/block_devices/sda3";

fn fixture(fs: &str, to: u64) -> (Mock, Run) {
    let mut state = (*Mock::default().snapshot()).clone();
    let block = state.blocks.iter_mut().find(|b| b.path == PART).unwrap();
    block.id_type = fs.into();
    if fs.is_empty() {
        block.filesystem = None;
        block.id_usage.clear();
    } else {
        let filesystem = block.filesystem.as_mut().unwrap();
        filesystem.mount_points.clear();
        filesystem.usage = None;
    }
    let mock = Mock::new(state);
    let mut plan = Plan::default();
    let layout = plan.preview(&mock.snapshot(), DISK).unwrap();
    let index = layout.segs.iter().position(|s| s.short_name() == "sda3").unwrap();
    plan.resize(&layout, index, to).unwrap();
    let run = Run::agree(&plan, &mock.snapshot());
    (mock, run)
}

fn finish(mock: &Mock, run: &mut Run) -> Outcome {
    for _ in 0..20 {
        run.advance(mock);
        if let Some(outcome) = &run.outcome {
            return outcome.clone();
        }
    }
    panic!("in-memory resize did not finish");
}

#[test]
fn ext_resizes_check_before_changing_partition_or_filesystem() {
    for fs in ["ext2", "ext3", "ext4"] {
        for to in [120 * GIB, 160 * GIB] {
            let (mock, mut run) = fixture(fs, to);
            assert!(run.items[0].words.detail.contains("checked and repaired"));
            assert_eq!(finish(&mock, &mut run), Outcome::Done);
            let mut expected = vec!["Repair /dev/sda3".to_owned()];
            if to < 140 * GIB {
                expected.push(format!("ResizeFilesystem({}) /dev/sda3", to - MIB));
            }
            expected.push(format!("ResizePartition({to}) /dev/sda3"));
            expected.push("ResizeFilesystem(0) /dev/sda3".into());
            assert_eq!(mock.calls(), expected, "{fs}");
            let state = mock.snapshot();
            let block = state.block(PART).unwrap();
            assert_eq!(block.size, to);
            assert_eq!(block.filesystem.as_ref().unwrap().size, Some(to));
        }
    }
}

#[test]
fn other_filesystems_and_empty_partitions_do_not_get_ext_repair() {
    for fs in ["vfat", "ntfs", ""] {
        let (mock, mut run) = fixture(fs, 160 * GIB);
        assert_eq!(finish(&mock, &mut run), Outcome::Done);
        let mut expected = vec![format!("ResizePartition({}) /dev/sda3", 160 * GIB)];
        if !fs.is_empty() {
            expected.push("ResizeFilesystem(0) /dev/sda3".into());
        }
        assert_eq!(mock.calls(), expected, "{fs}");
    }
}

#[test]
fn a_failed_precheck_leaves_partition_and_filesystem_sizes_unchanged() {
    let (mock, mut run) = fixture("ext4", 160 * GIB);
    mock.fail_next("Repair", "e2fsck: filesystem cannot be repaired");
    assert!(matches!(finish(&mock, &mut run), Outcome::Failed { error, .. } if error.contains("cannot be repaired")));
    assert_eq!(mock.calls(), ["Repair /dev/sda3"]);
    let state = mock.snapshot();
    let block = state.block(PART).unwrap();
    assert_eq!(block.size, 140 * GIB);
    assert_eq!(block.filesystem.as_ref().unwrap().size, Some(140 * GIB));
    assert!(run.done_ids().is_empty());
}

#[test]
fn a_later_grow_failure_is_reported_with_partial_changes_retained() {
    let (mock, mut run) = fixture("ext4", 160 * GIB);
    mock.fail_next("ResizeFilesystem", "resize2fs: simulated write error");
    assert!(
        matches!(finish(&mock, &mut run), Outcome::Failed { error, .. } if error.contains("simulated write error"))
    );
    assert_eq!(
        mock.calls(),
        [
            "Repair /dev/sda3".to_owned(),
            format!("ResizePartition({}) /dev/sda3", 160 * GIB),
            "ResizeFilesystem(0) /dev/sda3".into()
        ]
    );
    let state = mock.snapshot();
    let block = state.block(PART).unwrap();
    assert_eq!(block.size, 160 * GIB, "partition enlargement is not rolled back");
    assert_eq!(block.filesystem.as_ref().unwrap().size, Some(140 * GIB));
    assert!(matches!(run.items[0].status, Status::Failed(_)));
    assert!(run.done_ids().is_empty(), "never report the partially completed operation as done");
}
