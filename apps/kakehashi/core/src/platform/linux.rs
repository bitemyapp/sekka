//! Linux: drives from lsblk.

use super::sys::run;
use crate::drive::Drive;
use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::path::Path;

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
            port: None,
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
    let mut drives = parse(&out)?;
    for drive in &mut drives {
        drive.port = fs::canonicalize(Path::new("/sys/block").join(&drive.name)).ok().and_then(|p| usb_port(&p));
    }
    Ok(drives)
}

/// The USB port in a block device's sysfs path: the last USB device on the
/// way to it, `2-1.3` in `…/usb2/2-1/2-1.3/2-1.3:1.0/host6/…/block/sdb`
/// (bus 2, port 1, then port 3 of the hub there).
fn usb_port(sysfs: &Path) -> Option<String> {
    let is_port = |name: &&str| {
        name.split_once('-').is_some_and(|(bus, ports)| {
            let number = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
            number(bus) && ports.split('.').all(number)
        })
    };
    sysfs.components().filter_map(|c| c.as_os_str().to_str()).rfind(is_port).map(str::to_owned)
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

    #[test]
    fn finds_the_usb_port_in_sysfs() {
        let stick = Path::new(
            "/sys/devices/pci0000:00/0000:00:14.0/usb2/2-1/2-1.3/2-1.3:1.0/host6/target6:0:0/6:0:0:0/block/sdb",
        );
        assert_eq!(usb_port(stick).as_deref(), Some("2-1.3"));
        let direct =
            Path::new("/sys/devices/pci0000:00/0000:00:14.0/usb3/3-4/3-4:1.0/host7/target7:0:0/7:0:0:0/block/sdc");
        assert_eq!(usb_port(direct).as_deref(), Some("3-4"));
        let ssd = Path::new("/sys/devices/pci0000:00/0000:00:1d.0/0000:3d:00.0/nvme/nvme0/nvme0n1");
        assert_eq!(usb_port(ssd), None);
    }
}
