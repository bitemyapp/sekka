//! Linux: drives from lsblk, written through the block device.

use super::{Device, run};
use crate::drive::Drive;
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::path::Path;

pub(crate) type Held = ();

/// Mount points the running system needs.
const SYSTEM: [&str; 9] = ["/", "/boot", "/boot/efi", "/efi", "/home", "/nix", "/nix/store", "/usr", "/var"];

fn flag(v: &Value) -> bool {
    v.as_bool().unwrap_or_else(|| matches!(v.as_str(), Some("1") | Some("true")))
}

fn number(v: &Value) -> u64 {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0)
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or("").trim().to_owned()
}

/// Mount points of a device and everything on it.
fn mounts(dev: &Value, into: &mut Vec<String>) {
    for key in ["mountpoints", "mountpoint"] {
        match &dev[key] {
            Value::Array(points) => into.extend(points.iter().filter_map(Value::as_str).map(str::to_owned)),
            Value::String(point) => into.push(point.clone()),
            _ => {}
        }
    }
    for child in dev["children"].as_array().into_iter().flatten() {
        mounts(child, into);
    }
}

pub(crate) fn parse(json: &[u8]) -> Result<Vec<Drive>> {
    let tree: Value = serde_json::from_slice(json)?;
    let mut drives = Vec::new();
    for dev in tree["blockdevices"].as_array().into_iter().flatten() {
        if text(&dev["type"]) != "disk" {
            continue;
        }
        let name = text(&dev["name"]);
        let tran = text(&dev["tran"]);
        let mut points = Vec::new();
        mounts(dev, &mut points);
        points.retain(|p| !p.is_empty());
        let removable = flag(&dev["rm"]);
        let hotplug = flag(&dev["hotplug"]);
        drives.push(Drive {
            bytes: number(&dev["size"]),
            vendor: text(&dev["vendor"]),
            model: text(&dev["model"]),
            serial: text(&dev["serial"]),
            usb_or_removable: tran == "usb" || removable,
            internal: !(tran == "usb" || removable || hotplug),
            virtual_drive: ["loop", "zram", "ram", "nbd", "dm-", "md"].iter().any(|p| name.starts_with(p)),
            read_only: flag(&dev["ro"]),
            system: points.iter().any(|p| SYSTEM.contains(&p.as_str()) || p == "[SWAP]"),
            mounts: points,
            bus: if tran.is_empty() { String::new() } else { tran.to_uppercase() },
            name,
        });
    }
    Ok(drives)
}

pub(crate) fn drives() -> Result<Vec<Drive>> {
    let columns = "NAME,SIZE,TYPE,TRAN,RM,HOTPLUG,RO,VENDOR,MODEL,SERIAL";
    // MOUNTPOINTS lists every mount (util-linux 2.37); MOUNTPOINT, older,
    // only one.
    let out = run("lsblk", &["-J", "-b", "-o", &format!("{columns},MOUNTPOINTS")])
        .or_else(|_| run("lsblk", &["-J", "-b", "-o", &format!("{columns},MOUNTPOINT")]))?;
    parse(&out)
}

fn parent(device: &str) -> Option<String> {
    let out = run("lsblk", &["-no", "PKNAME", device]).ok()?;
    let name = String::from_utf8_lossy(&out).lines().next().unwrap_or("").trim().to_owned();
    if name.is_empty() { Path::new(device).file_name().map(|n| n.to_string_lossy().into_owned()) } else { Some(name) }
}

pub(crate) fn image_drive(image: &Path) -> Option<String> {
    let out = run("df", &["-P", &image.to_string_lossy()]).ok()?;
    let line = String::from_utf8_lossy(&out).lines().nth(1)?.to_owned();
    let device = line.split_whitespace().next()?;
    device.starts_with("/dev/").then(|| parent(device)).flatten()
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_lsblk() {
        let json = br#"{"blockdevices": [
          {"name":"nvme0n1","size":2000398934016,"type":"disk","tran":"nvme","rm":false,"hotplug":false,"ro":false,
           "vendor":null,"model":"Samsung SSD 990 PRO 2TB","serial":"S7","mountpoints":[null],
           "children":[{"name":"nvme0n1p2","type":"part","mountpoints":["/nix/store","/"]}]},
          {"name":"sdb","size":32015679488,"type":"disk","tran":"usb","rm":true,"hotplug":true,"ro":false,
           "vendor":"SanDisk ","model":"Ultra","serial":"4C53","mountpoints":[null],
           "children":[{"name":"sdb1","type":"part","mountpoints":["/run/media/me/STICK"]}]},
          {"name":"loop0","size":1000,"type":"loop","tran":null}
        ]}"#;
        let drives = parse(json).unwrap();
        assert_eq!(drives.len(), 2);
        let (ssd, stick) = (&drives[0], &drives[1]);
        assert!(ssd.internal && ssd.system);
        assert!(stick.usb_or_removable && !stick.internal && !stick.system);
        assert_eq!(stick.vendor, "SanDisk");
        assert_eq!(stick.bus, "USB");
        assert_eq!(stick.mounts, vec!["/run/media/me/STICK"]);
        assert_eq!(stick.excluded(None, false), None);
    }
}
