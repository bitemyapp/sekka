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
    fs::read_dir("/sys/class/tpmrm").is_ok_and(|mut d| d.next().is_some())
        || Path::new("/sys/firmware/acpi/tables/TPM2").exists()
}

/// The file in a system generation that says it can open the store.
pub const MARKER: &str = "sekka/network-store-v1";

/// The system generations that can be booted or switched to: each profile
/// generation and its specialisations, and the running and booted systems.
pub fn generations(profiles: &Path) -> Vec<PathBuf> {
    let mut tops: Vec<PathBuf> = fs::read_dir(profiles)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.starts_with("system-") && name.ends_with("-link")
        })
        .map(|e| e.path())
        .chain([
            PathBuf::from("/run/current-system"),
            PathBuf::from("/run/booted-system"),
        ])
        .filter_map(|p| fs::canonicalize(p).ok())
        .collect();
    let specialisations: Vec<PathBuf> = tops
        .iter()
        .flat_map(|top| {
            fs::read_dir(top.join("specialisation"))
                .into_iter()
                .flatten()
                .flatten()
        })
        .filter_map(|e| fs::canonicalize(e.path()).ok())
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
        let gens = generations(&profiles);
        assert!(gens.contains(&fs::canonicalize(&new).unwrap()));
        assert!(unmarked(&[fs::canonicalize(&new).unwrap()]).is_empty());
        std::os::unix::fs::symlink(&old, profiles.join("system-1-link")).unwrap();
        std::os::unix::fs::symlink(&old, new.join("specialisation/plain")).unwrap();
        let gens = generations(&profiles);
        assert_eq!(
            unmarked(&gens)
                .iter()
                .filter(|g| **g == fs::canonicalize(&old).unwrap())
                .count(),
            1
        );
        let _ = fs::remove_dir_all(dir);
    }
}
