//! Linux: the block device, unmounted, opened exclusively, then ejected.

use super::Device;
use anyhow::{Context, Result, bail, ensure};
use kakehashi_core::Drive;
use kakehashi_core::platform::sys::run;
use std::fs::{self, OpenOptions};

pub(crate) type Held = ();

pub(crate) fn unmount(drive: &Drive) -> Result<()> {
    for point in &drive.mounts {
        if point == "[SWAP]" {
            continue;
        }
        run("umount", &[point]).with_context(|| format!("could not unmount {point}"))?;
    }
    Ok(())
}

/// The block device, opened exclusively (which fails while anything on it
/// is mounted), with its size and logical sector size.
pub(crate) fn open(drive: &Drive) -> Result<Device> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
    use std::os::unix::io::AsRawFd;
    let path = format!("/dev/{}", drive.name);
    let before = fs::symlink_metadata(&path)?;
    ensure!(before.file_type().is_block_device(), "{path} is not a block device");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_EXCL | libc::O_NOFOLLOW)
        .open(&path)
        .with_context(|| format!("could not open {path} exclusively (is something on it still in use?)"))?;
    ensure!(file.metadata()?.rdev() == before.rdev(), "{path} changed while opening it");
    let (mut bytes, mut sector) = (0u64, 0 as libc::c_int);
    // BLKGETSIZE64 and BLKSSZGET.
    // SAFETY: each ioctl writes one integer of the type passed.
    unsafe {
        ensure!(libc::ioctl(file.as_raw_fd(), 0x8008_1272_u64 as _, &mut bytes) == 0, "could not read the size");
        ensure!(libc::ioctl(file.as_raw_fd(), 0x1268_u64 as _, &mut sector) == 0, "could not read the sector size");
    }
    ensure!(sector > 0 && (sector as u64).is_power_of_two(), "unexpected sector size {sector}");
    Ok(Device { file, bytes, sector: sector as u64, held: () })
}

pub(crate) fn drop_cache(device: &Device) {
    use std::os::unix::io::AsRawFd;
    // BLKFLSBUF, then advise the kernel the cache is not needed.
    // SAFETY: neither call touches memory.
    unsafe {
        libc::ioctl(device.file.as_raw_fd(), 0x1261_u64 as _, 0);
        libc::posix_fadvise(device.file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
    }
}

pub(crate) fn eject(drive: &Drive) -> Result<()> {
    let device = format!("/dev/{}", drive.name);
    if run("udisksctl", &["power-off", "--no-user-interaction", "-b", &device]).is_ok()
        || run("eject", &[&device]).is_ok()
    {
        Ok(())
    } else {
        bail!("it could not be ejected, but it is written and synced, so you can unplug it")
    }
}
