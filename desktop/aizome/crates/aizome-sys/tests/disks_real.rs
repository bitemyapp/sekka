//! Real UDisks/kernel regression, only inside nix/vm-loop-images-test.nix.
//! No host device is ever accepted: all backing files are made by this test.
use aizome_sys::{Wake, disks::Disks};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn fixture(name: &str) -> (Disks, PathBuf) {
    assert_eq!(fs::read_to_string("/etc/aizome-loop-test-vm").unwrap(), "disposable-loop-test\n");
    let dir = Path::new("/var/tmp").join(format!("aizome-loop-{name}"));
    fs::create_dir_all(&dir).unwrap();
    let disks = Disks::new(Wake::none());
    wait("UDisks available", || disks.available());
    (disks, dir)
}

fn wait(description: &str, mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if predicate() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("{description}");
}

fn image(dir: &Path, name: &str, filesystem: bool) -> PathBuf {
    let path = dir.join(name);
    File::create(&path).unwrap().set_len(128 * 1024 * 1024).unwrap();
    if filesystem {
        assert!(
            Command::new("mkfs.ext4")
                .args(["-F", "-b", "4096", "-L", "AIZOME_IMAGE"])
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
    }
    path
}

fn attached(disks: &Disks, image: &Path) -> Vec<String> {
    disks
        .snapshot()
        .blocks
        .iter()
        .filter(|b| b.loop_device.as_ref().is_some_and(|l| Path::new(&l.backing_file) == image))
        .map(|b| b.path.clone())
        .collect()
}

#[test]
#[ignore = "requires the disposable UDisks VM"]
fn manual_loop_lifetime() {
    let (disks, dir) = fixture("manual");
    for read_only in [true, false] {
        let path = image(&dir, if read_only { "readonly.img" } else { "writable.img" }, false);
        let device = disks.loop_setup(&path, read_only).wait().unwrap();
        // Nothing opens the block device while checking persistence.
        std::thread::sleep(Duration::from_secs(2));
        let state = disks.snapshot();
        let block = state.block(&device).expect("manual attached image disappeared");
        assert_eq!(block.read_only, read_only);
        let loop_info = block.loop_device.as_ref().unwrap();
        assert!(!loop_info.autoclear);
        assert_eq!(Path::new(&loop_info.backing_file), path);
        assert!(block.device.starts_with("/dev/loop"));
        if read_only {
            // Linux can allow O_WRONLY on a read-only loop; the actual write
            // must be rejected, without changing the backing file.
            let write =
                OpenOptions::new().write(true).open(&block.device).and_then(|mut file| file.write_all(b"forbidden"));
            assert!(write.is_err(), "read-only loop accepted a write");
            let mut bytes = [1; 9];
            File::open(&path).unwrap().read_exact(&mut bytes).unwrap();
            assert_eq!(bytes, [0; 9]);
        } else {
            let mut file = OpenOptions::new().write(true).open(&block.device).unwrap();
            file.seek(SeekFrom::Start(4096)).unwrap();
            file.write_all(b"manual image write").unwrap();
            file.sync_all().unwrap();
            drop(file);
            let mut file = File::open(&path).unwrap();
            file.seek(SeekFrom::Start(4096)).unwrap();
            let mut bytes = [0; 18];
            file.read_exact(&mut bytes).unwrap();
            assert_eq!(&bytes, b"manual image write");
        }
        disks.loop_delete(&device).wait().unwrap();
        wait("explicit detach removes manual image", || attached(&disks, &path).is_empty());
        println!("PASS manual attach persists, read_only={read_only}, explicit detach: {}", path.display());
    }
}

#[test]
#[ignore = "requires the disposable UDisks VM"]
fn mounted_loop_lifetime() {
    let (disks, dir) = fixture("mounted");
    for partitioned in [false, true] {
        let path = image(&dir, if partitioned { "partitioned.img" } else { "filesystem.img" }, !partitioned);
        if partitioned {
            let mut child = Command::new("sfdisk").arg(&path).stdin(Stdio::piped()).spawn().unwrap();
            child.stdin.take().unwrap().write_all(b"label: gpt\nstart=2048,size=256000,type=L\n").unwrap();
            assert!(child.wait().unwrap().success());
            assert!(
                Command::new("mkfs.ext4")
                    .args(["-F", "-b", "4096", "-E", "offset=1048576", "-L", "AIZOME_PART"])
                    .arg(&path)
                    .arg("32000")
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let mount_point = disks.mount_image(&path).wait().unwrap();
        wait("mounted image appears in snapshot", || !attached(&disks, &path).is_empty());
        std::thread::sleep(Duration::from_secs(2));
        let state = disks.snapshot();
        let block = state
            .blocks
            .iter()
            .find(|b| b.loop_device.as_ref().is_some_and(|l| Path::new(&l.backing_file) == path))
            .unwrap();
        assert!(block.loop_device.as_ref().unwrap().autoclear);
        assert!(block.read_only);
        let filesystem = state
            .blocks
            .iter()
            .find(|b| b.filesystem.as_ref().is_some_and(|f| f.mount_points == vec![mount_point.clone()]))
            .unwrap();
        assert_eq!(filesystem.partition.is_some(), partitioned);
        assert!(Path::new(&mount_point).join("lost+found").is_dir());
        let device = filesystem.path.clone();
        disks.unmount(&device, false).wait().unwrap();
        wait("unmount autoclears the image", || attached(&disks, &path).is_empty());
        assert!(!Path::new(&mount_point).join("lost+found").exists());
    }
    println!("PASS plain and partitioned mounted images survive until unmount, then autoclear");
}

#[test]
#[ignore = "requires the disposable UDisks VM"]
fn failed_mount_cleanup() {
    let (disks, dir) = fixture("failed");
    let keeper = image(&dir, "keeper.img", false);
    let kept_device = disks.loop_setup(&keeper, true).wait().unwrap();
    wait("keeper is attached", || !attached(&disks, &keeper).is_empty());
    let blank = image(&dir, "blank.img", false);
    assert!(disks.mount_image(&blank).wait().unwrap_err().to_string().contains("No filesystem found"));
    wait("no-filesystem error releases owned loop", || attached(&disks, &blank).is_empty());

    let corrupt = image(&dir, "corrupt.img", true);
    let mut file = OpenOptions::new().write(true).open(&corrupt).unwrap();
    file.seek(SeekFrom::Start(4096)).unwrap();
    file.write_all(&[0; 4096]).unwrap();
    drop(file);
    let error = disks.mount_image(&corrupt).wait().unwrap_err();
    assert!(
        !error.to_string().contains("No filesystem found"),
        "must reach an actual filesystem mount failure: {error}"
    );
    wait("mount error releases owned loop", || attached(&disks, &corrupt).is_empty());
    assert!(disks.mount_image(&dir.join("missing.img")).wait().is_err());
    assert!(disks.mount_image(&dir).wait().is_err());
    assert_eq!(attached(&disks, &keeper), vec![kept_device.clone()], "unrelated attachment must survive cleanup");
    disks.loop_delete(&kept_device).wait().unwrap();
    wait("keeper cleanup", || attached(&disks, &keeper).is_empty());
    // A successful retry after errors still mounts and autoclears normally.
    let retry = image(&dir, "retry.img", true);
    disks.mount_image(&retry).wait().unwrap();
    wait("retry attached", || !attached(&disks, &retry).is_empty());
    let device = attached(&disks, &retry).pop().unwrap();
    disks.unmount(&device, false).wait().unwrap();
    wait("retry autocleared", || attached(&disks, &retry).is_empty());
    println!("PASS invalid/no-filesystem/mount errors clean up only their loop and allow retry");
}
