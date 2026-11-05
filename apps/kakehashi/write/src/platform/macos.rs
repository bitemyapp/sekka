//! macOS: the raw device, unmounted with diskutil, opened through Apple's
//! authopen broker, flushed with a storage ioctl, and ejected.

use super::Device;
use anyhow::{Context, Result, ensure};
use kakehashi_core::Drive;
use kakehashi_core::platform::sys::run;
use std::fs;
use std::path::Path;

mod authopen;

pub(crate) type Held = ();

pub(crate) fn unmount(drive: &Drive) -> Result<()> {
    run("/usr/sbin/diskutil", &["unmountDisk", &format!("/dev/{}", drive.name)])?;
    Ok(())
}

/// The raw device, locked exclusively and uncached, with its size and
/// block size.
pub(crate) fn open(drive: &Drive, cancelled: &dyn Fn() -> bool) -> Result<Device> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    use std::os::unix::io::AsRawFd;
    let path = format!("/dev/r{}", drive.name);
    let before = fs::symlink_metadata(&path)?;
    ensure!(before.file_type().is_char_device(), "{path} is not a raw disk device");
    let file = authopen::open(Path::new(&path), cancelled)?;
    let after = file.metadata()?;
    ensure!(after.file_type().is_char_device() && after.rdev() == before.rdev(), "{path} changed while opening it");
    let fd = file.as_raw_fd();
    let (mut block, mut count) = (0u32, 0u64);
    // DKIOCGETBLOCKSIZE and DKIOCGETBLOCKCOUNT.
    // SAFETY: each ioctl writes one integer of the size passed.
    unsafe {
        ensure!(libc::ioctl(fd, 0x40046418, &mut block) == 0, "could not read the block size");
        ensure!(libc::ioctl(fd, 0x40086419, &mut count) == 0, "could not read the block count");
        libc::fcntl(fd, libc::F_NOCACHE, 1);
    }
    ensure!(block == 512 || block == 4096, "unexpected block size {block}");
    Ok(Device { file, bytes: block as u64 * count, sector: block as u64, held: () })
}

/// Flush the storage device's write cache. F_FULLFSYNC (File::sync_all)
/// operates on filesystems and returns ENOTTY on /dev/rdiskN. A plain fsync
/// is not a substitute for flushing the device's own cache.
pub(crate) fn sync(file: &std::fs::File) -> Result<()> {
    use std::os::fd::AsRawFd;
    // <sys/disk.h>: _IO('d', 22). This ioctl takes no data argument.
    const DKIOCSYNCHRONIZECACHE: libc::c_ulong = 0x20006416;
    loop {
        // SAFETY: file owns a live descriptor; the ioctl takes no pointer.
        if unsafe { libc::ioctl(file.as_raw_fd(), DKIOCSYNCHRONIZECACHE) } == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error).context("could not flush the drive's write cache before verification");
        }
    }
}

/// The raw device is uncached already.
pub(crate) fn drop_cache(_device: &Device) {}

pub(crate) fn eject(drive: &Drive) -> Result<()> {
    run("/usr/sbin/diskutil", &["eject", &format!("/dev/{}", drive.name)])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_sync_preserves_an_unsupported_ioctl_error() {
        // A temporary regular file supports filesystem sync, but not device
        // sync. Do not silently fall back or treat a failed flush as success.
        let file = tempfile::tempfile().unwrap();
        file.sync_all().unwrap();
        let error = sync(&file).unwrap_err();
        assert_eq!(error.downcast_ref::<std::io::Error>().unwrap().raw_os_error(), Some(libc::ENOTTY));
        assert!(error.to_string().contains("write cache before verification"));
    }

    #[test]
    #[ignore = "requires KAKEHASHI_CHECK_USB naming a USB disk and macOS authorization; flushes existing cache only"]
    fn flush_authorized_usb_without_writing() {
        let name = std::env::var("KAKEHASHI_CHECK_USB").expect("set KAKEHASHI_CHECK_USB to the selected disk name");
        let drive = kakehashi_core::platform::drives()
            .unwrap()
            .into_iter()
            .find(|d| d.name == name)
            .expect("selected disk not found");
        assert_eq!(drive.bus, "USB");
        assert!(drive.excluded(None, true).is_none(), "selected disk is excluded");
        let device = open(&drive, &|| false).unwrap();
        assert_eq!(device.bytes, drive.bytes);
        // Reproduce the original failure and exercise the production replacement.
        assert_eq!(device.file.sync_all().unwrap_err().raw_os_error(), Some(libc::ENOTTY));
        crate::platform::sync(&device).unwrap();
        eprintln!("{}: filesystem sync rejected; device cache flush succeeded; no bytes written", drive.name);
    }

    /// Explicit opt-in only: obtain and inspect the real production handle,
    /// then close it. No unmount, writes, seeks, or ejects are performed.
    #[test]
    #[ignore = "requires KAKEHASHI_CHECK_USB naming a USB disk and macOS authorization"]
    fn inspect_authorized_usb_handle_without_writing() {
        let name = std::env::var("KAKEHASHI_CHECK_USB").expect("set KAKEHASHI_CHECK_USB to the selected disk name");
        let drive = kakehashi_core::platform::drives()
            .unwrap()
            .into_iter()
            .find(|d| d.name == name)
            .expect("selected disk not found");
        assert_eq!(drive.bus, "USB");
        assert!(drive.excluded(None, true).is_none(), "selected disk is excluded");
        let device = open(&drive, &|| false).unwrap();
        assert_eq!(device.bytes, drive.bytes);
        assert!(matches!(device.sector, 512 | 4096));
        eprintln!(
            "{}: authorized {} bytes, {}-byte sectors; no bytes written",
            drive.name, device.bytes, device.sector
        );
    }
}
