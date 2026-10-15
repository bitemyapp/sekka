//! macOS: drives from diskutil and the I/O Registry, written through the
//! raw device.

use super::{Device, run};
use crate::drive::Drive;
use anyhow::{Context, Result, ensure};
use plist::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

mod authopen;

pub(crate) type Held = ();

fn plist(args: &[&str]) -> Result<Value> {
    let out = run(args[0], &args[1..])?;
    Ok(Value::from_reader(std::io::Cursor::new(out))?)
}

fn text(dict: &plist::Dictionary, key: &str) -> String {
    dict.get(key).and_then(Value::as_string).unwrap_or("").trim().to_owned()
}

fn flag(dict: &plist::Dictionary, key: &str) -> bool {
    dict.get(key).and_then(Value::as_boolean).unwrap_or(false)
}

/// USB vendor, product and serial for each whole disk, from the I/O
/// Registry: the nearest USB device above each disk's media.
fn usb_details() -> HashMap<String, (String, String, String)> {
    let mut found = HashMap::new();
    let Ok(tree) = plist(&["/usr/sbin/ioreg", "-p", "IOService", "-l", "-a"]) else {
        return found;
    };
    fn walk(
        node: &Value,
        usb: Option<(String, String, String)>,
        found: &mut HashMap<String, (String, String, String)>,
    ) {
        let Some(dict) = node.as_dictionary() else {
            if let Some(list) = node.as_array() {
                for child in list {
                    walk(child, usb.clone(), found);
                }
            }
            return;
        };
        let pick = |keys: &[&str]| keys.iter().map(|k| text(dict, k)).find(|v| !v.is_empty()).unwrap_or_default();
        let serial = pick(&["USB Serial Number", "kUSBSerialNumberString"]);
        let usb = if serial.is_empty() && !dict.contains_key("idVendor") {
            usb
        } else {
            Some((
                pick(&["USB Vendor Name", "kUSBVendorString"]),
                pick(&["USB Product Name", "kUSBProductString"]),
                serial,
            ))
        };
        if flag(dict, "Whole")
            && let (Some(name), Some(usb)) = (dict.get("BSD Name").and_then(Value::as_string), &usb)
        {
            found.insert(name.to_owned(), usb.clone());
        }
        if let Some(children) = dict.get("IORegistryEntryChildren").and_then(Value::as_array) {
            for child in children {
                walk(child, usb.clone(), found);
            }
        }
    }
    walk(&tree, None, &mut found);
    found
}

pub(crate) fn drives() -> Result<Vec<Drive>> {
    let list = plist(&["/usr/sbin/diskutil", "list", "-plist"])?;
    let list = list.as_dictionary().context("diskutil list: not a dictionary")?;
    let wholes: Vec<String> = list
        .get("WholeDisks")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_string).map(str::to_owned).collect())
        .unwrap_or_default();
    // Mount points of each whole disk's partitions and volumes.
    let mut mounts: HashMap<String, Vec<String>> = HashMap::new();
    for entry in list.get("AllDisksAndPartitions").and_then(Value::as_array).into_iter().flatten() {
        let Some(entry) = entry.as_dictionary() else { continue };
        let name = text(entry, "DeviceIdentifier");
        let mut points: Vec<String> = Vec::new();
        let mut add = |d: &plist::Dictionary| {
            let point = text(d, "MountPoint");
            if !point.is_empty() {
                points.push(point);
            }
        };
        add(entry);
        for key in ["Partitions", "APFSVolumes"] {
            for part in entry.get(key).and_then(Value::as_array).into_iter().flatten() {
                if let Some(part) = part.as_dictionary() {
                    add(part);
                }
            }
        }
        mounts.insert(name, points);
    }
    let usb = usb_details();
    let mut drives = Vec::new();
    for name in wholes {
        let info = plist(&["/usr/sbin/diskutil", "info", "-plist", &name])?;
        let info = info.as_dictionary().context("diskutil info: not a dictionary")?;
        let bytes =
            ["Size", "TotalSize"].iter().find_map(|k| info.get(k).and_then(Value::as_unsigned_integer)).unwrap_or(0);
        let bus = text(info, "BusProtocol");
        let points = mounts.remove(&name).unwrap_or_default();
        let (vendor, product, serial) = usb.get(&name).cloned().unwrap_or_default();
        drives.push(Drive {
            bytes,
            vendor,
            model: if product.is_empty() { text(info, "MediaName") } else { product },
            serial,
            usb_or_removable: bus == "USB"
                || bus == "Secure Digital"
                || flag(info, "Removable")
                || flag(info, "RemovableMedia"),
            internal: flag(info, "Internal"),
            virtual_drive: text(info, "VirtualOrPhysical") == "Virtual" || bus == "Disk Image",
            read_only: !flag(info, "WritableMedia"),
            system: points.iter().any(|p| p == "/" || p.starts_with("/System/Volumes")),
            mounts: points,
            bus,
            name,
        });
    }
    Ok(drives)
}

fn parent(device: &str) -> Option<String> {
    let info = plist(&["/usr/sbin/diskutil", "info", "-plist", device]).ok()?;
    Some(text(info.as_dictionary()?, "ParentWholeDisk")).filter(|s| !s.is_empty())
}

pub(crate) fn image_drive(image: &Path) -> Option<String> {
    let out = run("/bin/df", &["-P", &image.to_string_lossy()]).ok()?;
    let line = String::from_utf8_lossy(&out).lines().nth(1)?.to_owned();
    let device = line.split_whitespace().next()?;
    device.starts_with("/dev/").then(|| parent(device)).flatten()
}

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
        let drive = drives().unwrap().into_iter().find(|d| d.name == name).expect("selected disk not found");
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
        let drive = drives().unwrap().into_iter().find(|d| d.name == name).expect("selected disk not found");
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
