// SPDX-License-Identifier: GPL-3.0-or-later
use crate::process::output;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::Path,
};

#[allow(clippy::unnecessary_cast)] // dev_t is u64 on Linux, i32 on Darwin.
fn device_number(meta: &fs::Metadata) -> libc::dev_t {
    meta.rdev() as libc::dev_t
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub path: String,
    pub major_minor: String,
    pub bytes: u64,
    pub serial: String,
    pub wwn: String,
    pub model: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Disk {
    pub identity: Identity,
    pub blocked: Option<String>,
}

fn busy(node: &Value) -> bool {
    node["mountpoints"]
        .as_array()
        .is_some_and(|a| a.iter().any(|m| m.as_str().is_some_and(|s| !s.is_empty())))
        || node["children"]
            .as_array()
            .is_some_and(|a| a.iter().any(busy))
}

pub fn parse(text: &str) -> Result<Vec<Disk>> {
    let data: Value = serde_json::from_str(text)?;
    let nodes = data["blockdevices"]
        .as_array()
        .context("lsblk did not return block devices")?;
    ensure!(
        !nodes.iter().any(|n| n["type"] == "part"),
        "lsblk returned a flat list, not the required device tree"
    );
    let mut disks = Vec::new();
    for node in nodes {
        if node["type"] != "disk" {
            continue;
        }
        let s = |key: &str| node[key].as_str().unwrap_or("").trim().to_string();
        // The live session's compressed RAM swap is never a target; listing
        // it as an unavailable disk only confuses.
        if s("path").starts_with("/dev/zram") {
            continue;
        }
        let bytes = node["size"].as_u64().context("Missing disk size")?;
        let blocked = if node["ro"] != false {
            Some("Read-only device")
        } else if busy(node) {
            Some("Contains a mounted filesystem or active swap (including live media)")
        } else if bytes < 24 * 1024u64.pow(3) {
            Some("Needs at least 24 GiB")
        } else if !s("path").starts_with("/dev/") {
            Some("Invalid device path")
        } else {
            None
        };
        disks.push(Disk {
            identity: Identity {
                path: s("path"),
                major_minor: s("maj:min"),
                bytes,
                serial: s("serial"),
                wwn: s("wwn"),
                model: s("model"),
            },
            blocked: blocked.map(str::to_owned),
        });
    }
    Ok(disks)
}

pub fn discover() -> Result<Vec<Disk>> {
    let text = output(
        "lsblk",
        &[
            "--json",
            "--tree",
            "--bytes",
            "--paths",
            "--output",
            "PATH,TYPE,SIZE,RO,MODEL,SERIAL,WWN,MAJ:MIN,MOUNTPOINTS",
        ],
        30,
    )?;
    let mut disks = parse(&text)?;
    for disk in &mut disks {
        let p = Path::new(&disk.identity.path);
        let meta = fs::metadata(p)?;
        ensure!(
            meta.file_type().is_block_device() && p.canonicalize()? == p,
            "Noncanonical block device"
        );
        let major = libc::major(device_number(&meta));
        let minor = libc::minor(device_number(&meta));
        ensure!(
            disk.identity.major_minor == format!("{major}:{minor}"),
            "Disk changed during discovery"
        );
        let sys = Path::new("/sys/dev/block")
            .join(&disk.identity.major_minor)
            .canonicalize()?;
        if sys.starts_with("/sys/devices/virtual/block") {
            disk.blocked =
                Some("Virtual memory or logical block device is not an installation target".into());
        }
        // Holders (LVM, dm-crypt, RAID) must block the whole drive even when
        // no filesystem on the logical device is currently mounted.
        for node in std::iter::once(sys.clone()).chain(
            fs::read_dir(&sys)?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.join("partition").exists()),
        ) {
            if fs::read_dir(node.join("holders"))?.next().is_some() {
                disk.blocked =
                    Some("Device or partition is used by LVM, encryption or RAID".into());
            }
        }
    }
    Ok(disks)
}

pub fn revalidate(identity: &Identity) -> Result<()> {
    let disks = discover()?;
    let current = disks
        .iter()
        .find(|d| d.identity.path == identity.path)
        .context("Selected disk disappeared")?;
    ensure!(
        &current.identity == identity,
        "Disk identity changed; rescan and review again"
    );
    ensure!(
        current.blocked.is_none(),
        "Disk is unavailable: {}",
        current.blocked.as_deref().unwrap_or("")
    );
    Ok(())
}

pub fn partition(path: &str, index: u8) -> String {
    format!(
        "{path}{}{index}",
        if path.ends_with(|c: char| c.is_ascii_digit()) {
            "p"
        } else {
            ""
        }
    )
}

/// Test diagnostics are only available with our marked, disposable QEMU disk.
/// Both transports must satisfy the same serial and size checks.
pub fn vm_test_disk() -> Result<Identity> {
    let mut matches = discover()?.into_iter().filter(|d| {
        ["/dev/vda", "/dev/nvme0n1"].contains(&d.identity.path.as_str())
            && d.identity.serial == "RESPIN_TEST_ONLY"
            && [40 * 1024u64.pow(3), 80 * 1024u64.pow(3)].contains(&d.identity.bytes)
    });
    let disk = matches
        .next()
        .context("Missing marked 40 or 80 GiB VM disk")?;
    ensure!(matches.next().is_none(), "Multiple VM test disks");
    Ok(disk.identity)
}

const MIB: u64 = 1024 * 1024;
/// The installed root filesystem must keep at least this much space.
pub const MIN_ROOT_BYTES: u64 = 20 * 1024 * MIB;

/// Exact GPT geometry in MiB: boot (ESP or BIOS boot), root, optional swap at
/// the end. Explicit aligned boundaries let the kernel's view be checked
/// exactly instead of trusting that partition names exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub boot: (u64, u64),
    pub root: (u64, u64),
    pub swap: Option<(u64, u64)>,
}
impl Layout {
    pub fn new(
        disk_bytes: u64,
        firmware: crate::Firmware,
        swap_bytes: Option<u64>,
    ) -> Result<Self> {
        let boot = if firmware == crate::Firmware::Uefi {
            (1, 1025)
        } else {
            (1, 3)
        };
        // Leave the final MiB for the backup GPT (33 sectors at 512 bytes,
        // 5 at 4096) and partition alignment.
        let end = (disk_bytes / MIB)
            .checked_sub(1)
            .context("Disk is too small")?;
        let swap_mib = swap_bytes.map(|bytes| bytes.div_ceil(MIB));
        let root_end = end
            .checked_sub(swap_mib.unwrap_or(0))
            .filter(|root_end| *root_end > boot.1)
            .context("Disk is too small for the swap partition")?;
        let layout = Self {
            boot,
            root: (boot.1, root_end),
            swap: swap_mib.map(|_| (root_end, end)),
        };
        ensure!(
            layout.root_bytes() >= MIN_ROOT_BYTES,
            "The root filesystem would have only {:.1} GiB; at least {} GiB is required{}",
            layout.root_bytes() as f64 / 1024f64.powi(3),
            MIN_ROOT_BYTES / 1024u64.pow(3),
            if swap_bytes.is_some() {
                ". Disable the RAM-sized swap partition or choose a larger disk"
            } else {
                ""
            }
        );
        Ok(layout)
    }
    pub fn root_bytes(&self) -> u64 {
        (self.root.1 - self.root.0) * MIB
    }
    pub fn swap_bytes(&self) -> Option<u64> {
        self.swap.map(|(start, end)| (end - start) * MIB)
    }
    /// One parted invocation: fewer partition-table rereads and udev events.
    pub(crate) fn parted_script(&self, firmware: crate::Firmware, filesystem: &str) -> Vec<String> {
        // Binary units are exact in parted; decimal units are rounded.
        let mut script = vec!["mklabel".into(), "gpt".into()];
        let mut part = |name: &str, kind: &str, (start, end): (u64, u64)| {
            script.extend([
                "mkpart".into(),
                name.into(),
                kind.into(),
                format!("{start}MiB"),
                format!("{end}MiB"),
            ]);
        };
        if firmware == crate::Firmware::Uefi {
            part("ESP", "fat32", self.boot);
        } else {
            part("BIOS", "", self.boot);
        }
        part("root", filesystem, self.root);
        if let Some(swap) = self.swap {
            part("swap", "linux-swap", swap);
        }
        script.retain(|word| !word.is_empty());
        script.extend(
            if firmware == crate::Firmware::Uefi {
                ["set", "1", "esp", "on"]
            } else {
                ["set", "1", "bios_grub", "on"]
            }
            .map(String::from),
        );
        script
    }
    fn partitions(&self) -> Vec<(u8, (u64, u64))> {
        let mut partitions = vec![(1, self.boot), (2, self.root)];
        partitions.extend(self.swap.map(|swap| (3, swap)));
        partitions
    }
}

/// Require the kernel's actual partition geometry, not merely existing names.
/// sysfs start/size are always in 512-byte sectors, including on 4Kn NVMe.
pub(crate) fn verify_layout(disk: &Identity, layout: &Layout) -> Result<()> {
    let parent = Path::new("/sys/dev/block")
        .join(&disk.major_minor)
        .canonicalize()?;
    let expected = layout.partitions();
    // A leftover partition 4+ would mean the table was not fully replaced.
    let count = fs::read_dir(&parent)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().join("partition").exists())
        .count();
    ensure!(
        count == expected.len(),
        "Expected {} partitions, the kernel reports {count}",
        expected.len()
    );
    for (index, (start_mib, end_mib)) in expected {
        let device = partition(&disk.path, index);
        let meta = fs::metadata(&device)?;
        ensure!(
            meta.file_type().is_block_device()
                && Path::new(&device).canonicalize()? == Path::new(&device),
            "Invalid partition node {device}"
        );
        let node = Path::new("/sys/dev/block")
            .join(format!(
                "{}:{}",
                libc::major(device_number(&meta)),
                libc::minor(device_number(&meta))
            ))
            .canonicalize()?;
        ensure!(
            node.parent() == Some(parent.as_path()),
            "Partition {device} belongs to another disk"
        );
        let number = |name: &str| -> Result<u64> {
            Ok(fs::read_to_string(node.join(name))?.trim().parse()?)
        };
        ensure!(
            number("partition")? == u64::from(index),
            "Wrong partition number for {device}"
        );
        let start = number("start")?
            .checked_mul(512)
            .context("Partition start overflow")?;
        let size = number("size")?
            .checked_mul(512)
            .context("Partition size overflow")?;
        ensure!(
            start == start_mib * MIB && size == (end_mib - start_mib) * MIB,
            "Unexpected partition geometry for {device}: start {start}, size {size}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_reserves_ram_sized_swap_at_the_end() {
        use crate::Firmware;
        let gib = 1024u64.pow(3);
        let layout = Layout::new(512 * gib, Firmware::Uefi, Some(32 * gib)).unwrap();
        assert_eq!(layout.boot, (1, 1025));
        assert_eq!(layout.root.0, 1025);
        assert_eq!(layout.swap_bytes(), Some(32 * gib));
        assert_eq!(layout.swap.unwrap().1, 512 * 1024 - 1);
        assert_eq!(layout.root.1, layout.swap.unwrap().0);
        let bios = Layout::new(40 * gib, Firmware::Bios, None).unwrap();
        assert_eq!(bios.boot, (1, 3));
        assert_eq!(bios.root, (3, 40 * 1024 - 1));
        assert!(bios.swap.is_none());
        // Swap may not squeeze root below its minimum.
        assert!(Layout::new(40 * gib, Firmware::Uefi, Some(32 * gib)).is_err());
        assert!(Layout::new(gib / 2, Firmware::Uefi, None).is_err());
        assert_eq!(
            layout.parted_script(Firmware::Uefi, "ext4").join(" "),
            format!(
                "mklabel gpt mkpart ESP fat32 1MiB 1025MiB mkpart root ext4 1025MiB {}MiB mkpart swap linux-swap {}MiB {}MiB set 1 esp on",
                layout.root.1,
                layout.root.1,
                layout.swap.unwrap().1
            )
        );
        assert_eq!(
            bios.parted_script(Firmware::Bios, "xfs").join(" "),
            format!(
                "mklabel gpt mkpart BIOS 1MiB 3MiB mkpart root xfs 3MiB {}MiB set 1 bios_grub on",
                bios.root.1
            )
        );
    }
    #[test]
    fn live_zram_is_not_listed() {
        let disks = parse(r#"{"blockdevices":[{"type":"disk","path":"/dev/zram0","size":16000000000,"ro":false,"mountpoints":["[SWAP]"]},{"type":"disk","path":"/dev/vda","size":42949672960,"ro":false}]}"#).unwrap();
        assert_eq!(disks.len(), 1);
        assert_eq!(disks[0].identity.path, "/dev/vda");
    }
    #[test]
    fn flat_device_list_is_rejected() {
        assert!(parse(r#"{"blockdevices":[{"type":"disk","size":42949672960,"ro":false,"path":"/dev/vda"},{"type":"part","mountpoints":["/"]}]}"#).is_err());
    }
    #[test]
    fn partition_names() {
        assert_eq!(partition("/dev/vda", 2), "/dev/vda2");
        assert_eq!(partition("/dev/nvme0n1", 2), "/dev/nvme0n1p2");
        assert_eq!(partition("/dev/mmcblk0", 2), "/dev/mmcblk0p2");
    }
    #[test]
    fn refuses_live_swap_small_and_readonly() {
        for extra in [
            r#""ro":true"#,
            r#""ro":false,"mountpoints":["[SWAP]"]"#,
            r#""ro":false,"children":[{"mountpoints":["/iso"]}]"#,
        ] {
            let json = format!(
                r#"{{"blockdevices":[{{"type":"disk","path":"/dev/vda","size":42949672960,{extra}}}]}}"#
            );
            assert!(parse(&json).unwrap()[0].blocked.is_some());
        }
        assert!(
            parse(r#"{"blockdevices":[{"type":"disk","path":"/dev/vda","size":1000,"ro":false}]}"#)
                .unwrap()[0]
                .blocked
                .is_some()
        );
    }
}
