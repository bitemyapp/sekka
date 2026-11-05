// SPDX-License-Identifier: GPL-3.0-or-later
//! What the system says: mounts, block devices, NetworkManager, the TPM and
//! the system generations the boot menu offers.

use std::fs;
use std::path::{Path, PathBuf};

use crate::cmd::tool;

/// A line of /proc/self/mountinfo.
#[derive(Debug, PartialEq)]
pub struct Mount {
    pub point: PathBuf,
    pub source: String,
}

fn unescape(field: &str) -> String {
    // Spaces and the like appear as \040.
    let mut out = String::new();
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let code: String = chars.by_ref().take(3).collect();
            match u8::from_str_radix(&code, 8) {
                Ok(byte) => out.push(byte as char),
                Err(_) => {
                    out.push('\\');
                    out.push_str(&code);
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn parse_mountinfo(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(" - ")?;
            let point = left.split(' ').nth(4)?;
            let source = right.split(' ').nth(1)?;
            Some(Mount {
                point: PathBuf::from(unescape(point)),
                source: unescape(source),
            })
        })
        .collect()
}

pub fn mounts() -> Vec<Mount> {
    parse_mountinfo(&fs::read_to_string("/proc/self/mountinfo").unwrap_or_default())
}

/// The mount `path` is on: the last mounted at the longest prefix.
pub fn mount_of<'m>(mounts: &'m [Mount], path: &Path) -> Option<&'m Mount> {
    mounts
        .iter()
        .filter(|m| path.starts_with(&m.point))
        .max_by_key(|m| m.point.components().count())
}

/// Whether block device `name` (as in /sys/class/block) is dm-crypt, or
/// sits on one (LVM on LUKS, say).
fn on_crypt(name: &str, depth: u32) -> bool {
    let dev = Path::new("/sys/class/block").join(name);
    if fs::read_to_string(dev.join("dm/uuid")).is_ok_and(|uuid| uuid.starts_with("CRYPT-")) {
        return true;
    }
    depth < 8
        && fs::read_dir(dev.join("slaves"))
            .into_iter()
            .flatten()
            .flatten()
            .any(|slave| on_crypt(&slave.file_name().to_string_lossy(), depth + 1))
}

/// Whether the filesystem holding `path` is encrypted already (Genkan's
/// opt-in LUKS), so a store would add nothing.
pub fn encrypted(path: &Path) -> bool {
    let mounts = mounts();
    let Some(mount) = mount_of(&mounts, path) else {
        return false;
    };
    let Ok(device) = fs::canonicalize(&mount.source) else {
        return false;
    };
    device.starts_with("/dev")
        && device
            .file_name()
            .is_some_and(|name| on_crypt(&name.to_string_lossy(), 0))
}

/// Whether the filesystem holding `path` is on removable media: a USB or
/// FireWire drive, an SD card, or a disk behind a port the firmware calls
/// external (Thunderbolt). A system installed there may start other
/// computers, whose TPMs couldn't open a store sealed to this one's.
pub fn removable(path: &Path) -> bool {
    let mounts = mounts();
    let Some(mount) = mount_of(&mounts, path) else {
        return false;
    };
    let Ok(device) = fs::canonicalize(&mount.source) else {
        return false;
    };
    device.starts_with("/dev")
        && device
            .file_name()
            .is_some_and(|name| on_removable(Path::new("/sys"), &name.to_string_lossy(), 0))
}

/// Whether block device `name` is on removable media, or sits on one; `sys`
/// is where sysfs is mounted.
fn on_removable(sys: &Path, name: &str, depth: u32) -> bool {
    let read = |path: PathBuf| {
        fs::read_to_string(path)
            .map(|text| text.trim().to_owned())
            .unwrap_or_default()
    };
    let Ok(mut device) = fs::canonicalize(sys.join("class/block").join(name)) else {
        return false;
    };
    // A partition's disk is the directory above it.
    if device.join("partition").exists()
        && let Some(disk) = device.parent()
    {
        device = disk.to_path_buf();
    }
    if read(device.join("removable")) == "1" {
        return true;
    }
    let Ok(devices) = fs::canonicalize(sys.join("devices")) else {
        return false;
    };
    let external = device
        .ancestors()
        .take_while(|dir| dir.starts_with(&devices))
        .any(|dir| {
            let subsystem = fs::read_link(dir.join("subsystem"))
                .ok()
                .and_then(|link| link.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_default();
            read(dir.join("removable")) == "removable"
                || matches!(
                    subsystem.as_str(),
                    "usb" | "firewire" | "pcmcia" | "memstick"
                )
                || (subsystem == "mmc" && read(dir.join("type")) == "SD")
        });
    external
        || (depth < 8
            && fs::read_dir(device.join("slaves"))
                .into_iter()
                .flatten()
                .flatten()
                .any(|slave| on_removable(sys, &slave.file_name().to_string_lossy(), depth + 1)))
}

/// Whether NetworkManager is running (or might be: when systemd can't say,
/// nothing NetworkManager reads is touched).
pub fn network_manager_running() -> bool {
    match tool("systemctl")
        .args([
            "show",
            "--property=MainPID",
            "--value",
            "NetworkManager.service",
        ])
        .ok()
    {
        Ok(pid) => {
            let pid = String::from_utf8_lossy(&pid);
            !matches!(pid.trim(), "" | "0")
        }
        Err(_) => true,
    }
}

/// Whether there may be a TPM: one in use, or one the firmware reports
/// whose driver may still be loading.
pub fn tpm_present() -> bool {
    tpm_loaded() || Path::new("/sys/firmware/acpi/tables/TPM2").exists()
}

/// Whether a TPM is in use, its driver loaded.
pub fn tpm_loaded() -> bool {
    fs::read_dir("/sys/class/tpmrm").is_ok_and(|mut d| d.next().is_some())
}

/// The file in a system generation that says it can open the store.
pub const MARKER: &str = "sekka/network-store-v1";

/// `path` with its symlinks followed as the system installed at `root`
/// sees them (its store paths are under `root`), if it leads somewhere.
fn resolve(root: &Path, path: &Path) -> Option<PathBuf> {
    if root == Path::new("/") {
        return fs::canonicalize(path).ok();
    }
    let mut path = path.to_path_buf();
    for _ in 0..40 {
        match fs::read_link(&path) {
            Ok(to) if to.is_absolute() => path = root.join(to.strip_prefix("/").ok()?),
            Ok(to) => path = path.parent()?.join(to),
            Err(_) => return path.exists().then_some(path),
        }
    }
    None
}

/// The system generations that can be booted or switched to: each profile
/// generation and its specialisations, and, on the running system (`root`
/// is /), the running and booted systems.
pub fn generations(root: &Path, profiles: &Path) -> Vec<PathBuf> {
    let running: &[&str] = if root == Path::new("/") {
        &["/run/current-system", "/run/booted-system"]
    } else {
        &[]
    };
    let mut tops: Vec<PathBuf> = fs::read_dir(profiles)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.starts_with("system-") && name.ends_with("-link")
        })
        .map(|e| e.path())
        .chain(running.iter().map(PathBuf::from))
        .filter_map(|p| resolve(root, &p))
        .collect();
    let specialisations: Vec<PathBuf> = tops
        .iter()
        .flat_map(|top| {
            fs::read_dir(top.join("specialisation"))
                .into_iter()
                .flatten()
                .flatten()
        })
        .filter_map(|e| resolve(root, &e.path()))
        .collect();
    tops.extend(specialisations);
    tops.sort();
    tops.dedup();
    tops
}

/// Generations that couldn't open a store: booting one would lose every
/// saved network.
pub fn unmarked(generations: &[PathBuf]) -> Vec<PathBuf> {
    generations
        .iter()
        .filter(|g| !g.join(MARKER).exists())
        .cloned()
        .collect()
}

/// A small counter kept in a file.
pub fn read_count(path: &Path) -> u32 {
    fs::read_to_string(path)
        .ok()
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(0)
}

pub fn write_count(path: &Path, n: u32) {
    if n == 0 {
        let _ = fs::remove_file(path);
    } else if let Err(error) = crate::files::write_atomic(path, format!("{n}\n").as_bytes(), 0o600)
    {
        eprintln!("sekka-network-store: {}: {error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: &str = "\
22 1 259:2 / / rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw
25 22 0:22 / /run rw,nosuid shared:5 - tmpfs tmpfs rw,mode=755
90 22 254:0 /connections /etc/NetworkManager/system-connections rw,nodev shared:40 - ext4 /dev/mapper/sekka-network-store rw
91 22 0:50 / /mnt/with\\040space rw - tmpfs none rw
";

    #[test]
    fn mountinfo_gives_mount_points_and_sources() {
        let mounts = parse_mountinfo(INFO);
        assert_eq!(mounts.len(), 4);
        assert_eq!(
            mounts[2],
            Mount {
                point: "/etc/NetworkManager/system-connections".into(),
                source: "/dev/mapper/sekka-network-store".into()
            }
        );
        assert_eq!(mounts[3].point, Path::new("/mnt/with space"));
    }

    #[test]
    fn a_path_is_on_its_longest_mount() {
        let mounts = parse_mountinfo(INFO);
        let on = |p: &str| mount_of(&mounts, Path::new(p)).unwrap().source.clone();
        assert_eq!(
            on("/etc/NetworkManager/system-connections/Home.nmconnection"),
            "/dev/mapper/sekka-network-store"
        );
        assert_eq!(on("/etc/NetworkManager"), "/dev/nvme0n1p2");
        assert_eq!(on("/run/sekka"), "tmpfs");
    }

    #[test]
    fn a_generation_without_the_marker_is_found() {
        let dir =
            std::env::temp_dir().join(format!("sekka-network-store-gens-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let (old, new) = (dir.join("old"), dir.join("new"));
        fs::create_dir_all(new.join("sekka")).unwrap();
        fs::write(new.join(MARKER), "").unwrap();
        fs::create_dir_all(new.join("specialisation")).unwrap();
        fs::create_dir_all(&old).unwrap();
        let profiles = dir.join("profiles");
        fs::create_dir_all(&profiles).unwrap();
        std::os::unix::fs::symlink(&new, profiles.join("system-2-link")).unwrap();
        let gens = generations(Path::new("/"), &profiles);
        assert!(gens.contains(&fs::canonicalize(&new).unwrap()));
        assert!(unmarked(&[fs::canonicalize(&new).unwrap()]).is_empty());
        std::os::unix::fs::symlink(&old, profiles.join("system-1-link")).unwrap();
        std::os::unix::fs::symlink(&old, new.join("specialisation/plain")).unwrap();
        let gens = generations(Path::new("/"), &profiles);
        assert_eq!(
            unmarked(&gens)
                .iter()
                .filter(|g| **g == fs::canonicalize(&old).unwrap())
                .count(),
            1
        );
        let _ = fs::remove_dir_all(dir);
    }

    fn scratch(what: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sekka-network-store-{what}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_installed_systems_generations_are_found_in_its_own_store() {
        let root = scratch("root");
        let system = root.join("nix/store/aaaa-nixos-system");
        let plain = root.join("nix/store/bbbb-nixos-system");
        fs::create_dir_all(system.join("sekka")).unwrap();
        fs::write(system.join(MARKER), "").unwrap();
        fs::create_dir_all(system.join("specialisation")).unwrap();
        fs::create_dir_all(&plain).unwrap();
        let profiles = root.join("nix/var/nix/profiles");
        fs::create_dir_all(&profiles).unwrap();
        // The links name store paths as the installed system sees them.
        std::os::unix::fs::symlink(
            "/nix/store/aaaa-nixos-system",
            profiles.join("system-1-link"),
        )
        .unwrap();
        // Not copied to the target: not a generation it can boot.
        std::os::unix::fs::symlink(
            "/nix/store/cccc-nixos-system",
            profiles.join("system-2-link"),
        )
        .unwrap();
        assert_eq!(generations(&root, &profiles), std::slice::from_ref(&system));
        assert!(unmarked(&generations(&root, &profiles)).is_empty());
        std::os::unix::fs::symlink(
            "/nix/store/bbbb-nixos-system",
            system.join("specialisation/plain"),
        )
        .unwrap();
        assert_eq!(unmarked(&generations(&root, &profiles)), [plain]);
        let _ = fs::remove_dir_all(root);
    }

    /// A device's path under `devices/`, its subsystem and its attributes.
    type Device<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);

    /// A sysfs tree: `devices/<path>` for each of `devices`, with its
    /// subsystem and attributes, and `class/block/<name>` linking to each
    /// block device.
    fn sysfs(what: &str, devices: &[Device]) -> PathBuf {
        let sys = scratch(what);
        fs::create_dir_all(sys.join("class/block")).unwrap();
        for (path, subsystem, attributes) in devices {
            let dir = sys.join("devices").join(path);
            fs::create_dir_all(&dir).unwrap();
            if !subsystem.is_empty() {
                fs::create_dir_all(sys.join("bus").join(subsystem)).unwrap();
                std::os::unix::fs::symlink(sys.join("bus").join(subsystem), dir.join("subsystem"))
                    .unwrap();
            }
            for (name, value) in *attributes {
                fs::write(dir.join(name), format!("{value}\n")).unwrap();
            }
            if *subsystem == "block" {
                let name = Path::new(path).file_name().unwrap();
                std::os::unix::fs::symlink(&dir, sys.join("class/block").join(name)).unwrap();
            }
        }
        sys
    }

    #[test]
    fn removable_media_is_told_from_fixed_disks() {
        let pci = "pci0000:00/0000:00:14.0";
        let usb = format!("{pci}/usb2/2-1/2-1:1.0/host6/target6:0:0/6:0:0:0/block/sdb");
        let sata = format!("{pci}/ata1/host0/target0:0:0/0:0:0:0/block/sda");
        let nvme = "pci0000:00/0000:00:1d.0/nvme/nvme0/nvme0n1";
        let thunderbolt = "pci0000:00/0000:00:07.0/0000:05:00.0/nvme/nvme1/nvme1n1";
        let sd = "pci0000:00/0000:00:1a.0/mmc_host/mmc0/mmc0:aaaa/block/mmcblk0";
        let emmc = "pci0000:00/0000:00:1b.0/mmc_host/mmc1/mmc1:0001/block/mmcblk1";
        let sys = sysfs(
            "sysfs",
            &[
                (&format!("{pci}/usb2/2-1"), "usb", &[("removable", "fixed")]),
                (&usb, "block", &[("removable", "0")]),
                (&format!("{usb}/sdb2"), "block", &[("partition", "2")]),
                (&sata, "block", &[("removable", "0")]),
                (&format!("{sata}/sda2"), "block", &[("partition", "2")]),
                ("pci0000:00/0000:00:1d.0", "pci", &[("removable", "fixed")]),
                (nvme, "block", &[("removable", "0")]),
                (
                    "pci0000:00/0000:00:07.0/0000:05:00.0",
                    "pci",
                    &[("removable", "removable")],
                ),
                (thunderbolt, "block", &[("removable", "0")]),
                (
                    "pci0000:00/0000:00:1a.0/mmc_host/mmc0/mmc0:aaaa",
                    "mmc",
                    &[("type", "SD")],
                ),
                (sd, "block", &[("removable", "0")]),
                (
                    "pci0000:00/0000:00:1b.0/mmc_host/mmc1/mmc1:0001",
                    "mmc",
                    &[("type", "MMC")],
                ),
                (emmc, "block", &[("removable", "0")]),
                ("virtual/block/dm-0", "block", &[]),
                ("virtual/block/dm-1", "block", &[]),
            ],
        );
        // dm-0 on the USB drive's partition, dm-1 on the SATA disk's.
        let slaves = |dm: &str, on: &str| {
            let dir = sys.join("devices/virtual/block").join(dm).join("slaves");
            fs::create_dir_all(&dir).unwrap();
            std::os::unix::fs::symlink(sys.join("class/block").join(on), dir.join(on)).unwrap();
        };
        slaves("dm-0", "sdb2");
        slaves("dm-1", "sda2");
        let removable = |name: &str| on_removable(&sys, name, 0);
        assert!(removable("sdb2"), "a USB drive");
        assert!(removable("dm-0"), "LUKS on a USB drive");
        assert!(removable("nvme1n1"), "behind an external Thunderbolt port");
        assert!(removable("mmcblk0"), "an SD card");
        assert!(!removable("sda2"), "an internal SATA disk");
        assert!(!removable("dm-1"), "LUKS on an internal disk");
        assert!(!removable("nvme0n1"), "an internal NVMe disk");
        assert!(!removable("mmcblk1"), "soldered eMMC");
        assert!(!removable("sdz"), "no such device");
        let _ = fs::remove_dir_all(sys);
    }
}
