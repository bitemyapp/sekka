// SPDX-License-Identifier: GPL-3.0-or-later
//! Closed filesystem choices and the shared format/probe/mount protocol.
use crate::{Firmware, process::output};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Filesystem {
    #[default]
    Ext4,
    Btrfs,
    Xfs,
}
impl Filesystem {
    pub const ALL: [Self; 3] = [Self::Ext4, Self::Btrfs, Self::Xfs];
    pub fn name(self) -> &'static str {
        match self {
            Self::Ext4 => "ext4",
            Self::Btrfs => "btrfs",
            Self::Xfs => "xfs",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Ext4 => "ext4 — general purpose (default)",
            Self::Btrfs => "Btrfs — checksums and compression",
            Self::Xfs => "XFS — large files and parallel I/O",
        }
    }
    fn formatter(self) -> &'static str {
        match self {
            Self::Ext4 => "mkfs.ext4",
            Self::Btrfs => "mkfs.btrfs",
            Self::Xfs => "mkfs.xfs",
        }
    }
    pub(crate) fn preflight(self, firmware: Firmware) -> Result<()> {
        preflight_with(self, firmware, output, || {
            Ok(fs::read_to_string("/proc/filesystems")?)
        })
    }
    /// Options for both the installation mount and the installed system.
    /// noatime avoids a metadata write per read; zstd level 1 keeps Btrfs
    /// compression cheap enough for installation and interactive use.
    pub fn mount_options(self) -> &'static [&'static str] {
        match self {
            Self::Btrfs => &["noatime", "compress=zstd:1"],
            Self::Ext4 | Self::Xfs => &["noatime"],
        }
    }
    pub(crate) fn format(self, device: &str, uuid: &str) -> Result<()> {
        format_with(self, device, uuid, output)
            .with_context(|| format!("Formatting {device} as {}", self.name()))
    }
    pub(crate) fn mount(self, device: &str, target: &str) -> Result<()> {
        mount_with(
            self.name(),
            device,
            target,
            &self.mount_options().join(","),
            output,
        )
    }
}

/// Filesystem identities chosen before the first disk write. Formatting
/// requests them explicitly and probing must return exactly these values, so
/// the complete target configuration can be evaluated and built before
/// erasure without ever trusting a stale /dev/disk/by-uuid alias.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identities {
    pub root: String,
    /// FAT volume serial in blkid's display form, e.g. `1A2B-3C4D`.
    pub efi: Option<String>,
    pub swap: Option<String>,
}
impl Identities {
    pub fn generate(firmware: Firmware, swap: bool) -> Result<Self> {
        Ok(Self {
            root: uuid_v4(random()?),
            efi: if firmware == Firmware::Uefi {
                Some(vfat_serial(random()?))
            } else {
                None
            },
            swap: if swap { Some(uuid_v4(random()?)) } else { None },
        })
    }
}

fn random<const N: usize>() -> Result<[u8; N]> {
    use std::io::Read;
    let mut bytes = [0u8; N];
    fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut bytes))
        .context("Could not read system randomness")?;
    Ok(bytes)
}

pub(crate) fn uuid_v4(mut b: [u8; 16]) -> String {
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex = |range: std::ops::Range<usize>| {
        b[range]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    format!(
        "{}-{}-{}-{}-{}",
        hex(0..4),
        hex(4..6),
        hex(6..8),
        hex(8..10),
        hex(10..16)
    )
}

pub(crate) fn vfat_serial(b: [u8; 4]) -> String {
    format!("{:02X}{:02X}-{:02X}{:02X}", b[0], b[1], b[2], b[3])
}

fn preflight_with(
    filesystem: Filesystem,
    firmware: Firmware,
    mut run: impl FnMut(&str, &[&str], u64) -> Result<String>,
    read_filesystems: impl FnOnce() -> Result<String>,
) -> Result<()> {
    run(filesystem.formatter(), &[if filesystem == Filesystem::Btrfs { "--version" } else { "-V" }], 15)
        .context("The installation media is missing a working filesystem formatter; no disk writes were made")?;
    // Cold module loading can include decompression and kernel relocation while
    // the live desktop is starting. This is read-only preparation, not a disk job.
    run("modprobe", &[filesystem.name()], 120).with_context(|| {
        format!(
            "Could not load {} kernel support; no disk writes were made",
            filesystem.name()
        )
    })?;
    if firmware == Firmware::Uefi {
        run("mkfs.fat", &["--help"], 15)
            .context("Could not check the EFI formatter; no disk writes were made")?;
        run("modprobe", &["vfat"], 120)
            .context("Could not load FAT32 kernel support; no disk writes were made")?;
    }
    let supported = read_filesystems()?;
    for name in [
        Some(filesystem.name()),
        (firmware == Firmware::Uefi).then_some("vfat"),
    ]
    .into_iter()
    .flatten()
    {
        ensure!(
            supported
                .lines()
                .any(|line| line.split_whitespace().last() == Some(name)),
            "The live kernel does not support {name}; no disk writes were made"
        );
    }
    Ok(())
}

fn verify_with(
    device: &str,
    expected: &str,
    run: &mut impl FnMut(&str, &[&str], u64) -> Result<String>,
) -> Result<()> {
    // Low-level probing bypasses both blkid's cache and udev's cached ID_FS_TYPE.
    // An ambiguous signature (exit 8) is an error, never a reason to guess.
    let actual = run(
        "blkid",
        &[
            "--probe",
            "--output",
            "value",
            "--match-tag",
            "TYPE",
            device,
        ],
        30,
    )?;
    ensure!(
        actual.trim() == expected,
        "Filesystem verification failed on {device}: expected {expected}, found {:?}",
        actual.trim()
    );
    Ok(())
}

pub(crate) fn check_uuid(uuid: &str, kind: &str) -> Result<()> {
    let (length, separators): (usize, &[usize]) = if kind == "vfat" {
        (9, &[4])
    } else {
        (36, &[8, 13, 18, 23])
    };
    ensure!(
        uuid.len() == length
            && uuid.bytes().enumerate().all(|(index, byte)| {
                if separators.contains(&index) {
                    byte == b'-'
                } else {
                    byte.is_ascii_hexdigit()
                }
            }),
        "Invalid {kind} filesystem UUID: {uuid:?}"
    );
    Ok(())
}

fn uuid_from_probe(probe: &str, expected: &str) -> Result<String> {
    let field = |name: &str| -> Result<&str> {
        let prefix = format!("{name}=");
        let values = probe
            .lines()
            .filter_map(|line| line.strip_prefix(&prefix))
            .collect::<Vec<_>>();
        ensure!(
            values.len() == 1,
            "Missing or ambiguous {name} in filesystem probe"
        );
        Ok(values[0])
    };
    ensure!(
        field("TYPE")? == expected,
        "Filesystem type changed during UUID probing"
    );
    let uuid = field("UUID")?;
    check_uuid(uuid, expected)?;
    Ok(uuid.to_owned())
}

#[cfg(test)]
pub(crate) fn probe_uuid(device: &str, expected: &str) -> Result<String> {
    // Old /dev/disk/by-uuid aliases can outlive a reformat even after udev
    // settles. Read the superblock itself, never select an alias by its rdev.
    let probe = output("blkid", &["--probe", "--output", "export", device], 30)?;
    uuid_from_probe(&probe, expected)
        .with_context(|| format!("Reading the new filesystem UUID on {device}"))
}
fn format_with(
    filesystem: Filesystem,
    device: &str,
    uuid: &str,
    mut run: impl FnMut(&str, &[&str], u64) -> Result<String>,
) -> Result<()> {
    check_uuid(uuid, filesystem.name())?;
    // Wiping the whole disk only removes its partition table, not signatures
    // inside its partitions. Do this even when mkfs would accept -f/-F.
    run("wipefs", &["--all", "--force", device], 60)?;
    let xfs_uuid = format!("uuid={uuid}");
    let args: Vec<&str> = match filesystem {
        Filesystem::Ext4 => vec!["-F", "-U", uuid, device],
        Filesystem::Btrfs => vec!["-f", "-U", uuid, device],
        Filesystem::Xfs => vec!["-f", "-m", &xfs_uuid, device],
    };
    run(filesystem.formatter(), &args, 300)?;
    verify_with(device, filesystem.name(), &mut run)?;
    verify_identity(device, filesystem.name(), uuid, &mut run)
}
pub(crate) fn format_efi(device: &str, serial: &str) -> Result<()> {
    format_efi_with(device, serial, output)
}
fn format_efi_with(
    device: &str,
    serial: &str,
    mut run: impl FnMut(&str, &[&str], u64) -> Result<String>,
) -> Result<()> {
    check_uuid(serial, "vfat")?;
    run("wipefs", &["--all", "--force", device], 60)?;
    run(
        "mkfs.fat",
        &["-F", "32", "-i", &serial.replace('-', ""), device],
        60,
    )?;
    verify_with(device, "vfat", &mut run)?;
    verify_identity(device, "vfat", serial, &mut run)
}
pub(crate) fn format_swap(device: &str, uuid: &str) -> Result<()> {
    format_swap_with(device, uuid, output)
}
fn format_swap_with(
    device: &str,
    uuid: &str,
    mut run: impl FnMut(&str, &[&str], u64) -> Result<String>,
) -> Result<()> {
    check_uuid(uuid, "swap")?;
    run("wipefs", &["--all", "--force", device], 60)?;
    run("mkswap", &["--uuid", uuid, "--label", "swap", device], 60)?;
    verify_with(device, "swap", &mut run)?;
    verify_identity(device, "swap", uuid, &mut run)
}
/// The superblock must carry exactly the identity written into configuration.
fn verify_identity(
    device: &str,
    kind: &str,
    expected: &str,
    run: &mut impl FnMut(&str, &[&str], u64) -> Result<String>,
) -> Result<()> {
    let probe = run("blkid", &["--probe", "--output", "export", device], 30)?;
    let actual = uuid_from_probe(&probe, kind)
        .with_context(|| format!("Reading the new filesystem UUID on {device}"))?;
    ensure!(
        actual.eq_ignore_ascii_case(expected),
        "Filesystem identity mismatch on {device}: requested {expected}, found {actual}"
    );
    Ok(())
}
pub(crate) fn mount_efi(device: &str, target: &str) -> Result<()> {
    mount_with("vfat", device, target, "umask=0077", output)
}
fn mount_with(
    kind: &str,
    device: &str,
    target: &str,
    options: &str,
    mut run: impl FnMut(&str, &[&str], u64) -> Result<String>,
) -> Result<()> {
    verify_with(device, kind, &mut run)?;
    run(
        "mount",
        &["--types", kind, "--options", options, "--", device, target],
        30,
    )
    .with_context(|| format!("Mounting verified {kind} filesystem {device} at {target}"))?;
    Ok(())
}

pub(crate) fn diagnose(error: anyhow::Error, disk: &str) -> anyhow::Error {
    let mut report = format!("Storage preparation failed on {disk}: {error:#}\n");
    let root = crate::disk::partition(disk, 2);
    let boot = crate::disk::partition(disk, 1);
    let swap = crate::disk::partition(disk, 3);
    for (program, args) in [
        (
            "lsblk",
            vec![
                "--bytes",
                "--output",
                "PATH,TYPE,SIZE,FSTYPE,FSVER,UUID,MOUNTPOINTS",
                disk,
            ],
        ),
        ("blkid", vec!["--probe", root.as_str()]),
        ("blkid", vec!["--probe", boot.as_str()]),
        ("blkid", vec!["--probe", swap.as_str()]),
        ("dmesg", vec!["--ctime", "--level=err,warn"]),
    ] {
        let text = match output(program, &args, 15) {
            Ok(text) => text,
            Err(error) => format!("{error:#}"),
        };
        let tail: String = text
            .chars()
            .rev()
            .take(12000)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        report.push_str(&format!("\n{program}:\n{tail}\n"));
    }
    let saved = (|| -> Result<_> {
        // NamedTempFile is mode 0600; diagnostics are local and contain no request secrets.
        let mut file = tempfile::Builder::new()
            .prefix("calamares-storage-")
            .suffix(".log")
            .tempfile_in("/run")?;
        file.write_all(report.as_bytes())?;
        let (_, path) = file.keep()?;
        Ok(path)
    })();
    match saved {
        Ok(path) => error.context(format!(
            "Could not prepare {disk}. Storage diagnostics saved to {} (copy before rebooting).",
            path.display()
        )),
        Err(save_error) => error.context(format!(
            "Could not prepare {disk}; saving storage diagnostics also failed: {save_error}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filesystem_uuid_requires_one_matching_unambiguous_identity() {
        let uuid = "11111111-2222-3333-4444-555555555555";
        assert_eq!(
            uuid_from_probe(&format!("TYPE=ext4\nUUID={uuid}\n"), "ext4").unwrap(),
            uuid
        );
        assert_eq!(
            uuid_from_probe("TYPE=vfat\nUUID=60F4-E5D4\n", "vfat").unwrap(),
            "60F4-E5D4"
        );
        for probe in [
            "TYPE=ext4\n",
            "TYPE=vfat\nUUID=60F4-E5D4\n",
            "TYPE=ext4\nUUID=\n",
            "TYPE=ext4\nUUID=../../device\n",
            "TYPE=ext4\nUUID=11111111-2222-3333-4444-555555555555\nUUID=11111111-2222-3333-4444-555555555555\n",
            "TYPE=ext4\nTYPE=xfs\nUUID=11111111-2222-3333-4444-555555555555\n",
        ] {
            assert!(uuid_from_probe(probe, "ext4").is_err(), "{probe}");
        }
    }
    #[test]
    fn refuses_wrong_or_ambiguous_signatures_before_mounting() {
        for answer in [
            Ok("ntfs\n".into()),
            Ok("ext4\nbtrfs\n".into()),
            Err(anyhow::anyhow!("ambiguous probe")),
        ] {
            let mut answer = Some(answer);
            let mut calls = vec![];
            assert!(
                mount_with(
                    "ext4",
                    "/dev/nvme0n1p2",
                    "/target",
                    "defaults",
                    |program, args, _| {
                        calls.push(program.to_owned());
                        assert_eq!(args[0], "--probe");
                        answer.take().unwrap()
                    }
                )
                .is_err()
            );
            assert_eq!(calls, ["blkid"]);
        }
    }
    const ROOT: &str = "0f1e2d3c-4b5a-4978-8695-a4b3c2d1e0f9";
    fn probe(kind: &str, uuid: &str) -> String {
        format!("DEVNAME=/dev/test\nUUID={uuid}\nTYPE={kind}\n")
    }
    #[test]
    fn formatting_stops_at_each_failed_step() {
        for fs in Filesystem::ALL {
            for failure in 0..4 {
                let mut calls = 0;
                assert!(
                    format_with(fs, "/dev/nvme0n1p2", ROOT, |_, args, _| {
                        let current = calls;
                        calls += 1;
                        if current == failure {
                            anyhow::bail!("injected I/O failure");
                        }
                        Ok(if args.contains(&"export") {
                            probe(fs.name(), ROOT)
                        } else {
                            format!("{}\n", fs.name())
                        })
                    })
                    .is_err()
                );
                assert_eq!(calls, failure + 1);
            }
        }
    }
    #[test]
    fn every_formatter_requests_and_verifies_the_chosen_identity() {
        for fs in Filesystem::ALL {
            let mut calls = vec![];
            let mut run = |program: &str, args: &[&str], _: u64| {
                calls.push((
                    program.to_owned(),
                    args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                ));
                Ok(if args.contains(&"export") {
                    probe(fs.name(), ROOT)
                } else if program == "blkid" {
                    format!("{}\n", fs.name())
                } else {
                    String::new()
                })
            };
            format_with(fs, "/dev/nvme0n1p2", ROOT, &mut run).unwrap();
            mount_with(
                fs.name(),
                "/dev/nvme0n1p2",
                "/target",
                &fs.mount_options().join(","),
                &mut run,
            )
            .unwrap();
            assert_eq!(
                calls.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(),
                ["wipefs", fs.formatter(), "blkid", "blkid", "blkid", "mount"]
            );
            assert_eq!(calls[0].1, ["--all", "--force", "/dev/nvme0n1p2"]);
            let requested = calls[1].1.join(" ");
            assert!(requested.contains(ROOT) && requested.ends_with("/dev/nvme0n1p2"));
            assert!(fs.mount_options().contains(&"noatime"));
            assert_eq!(calls[5].1[3], fs.mount_options().join(","));
        }
    }
    #[test]
    fn a_mismatched_or_malformed_identity_is_rejected() {
        let other = "11111111-2222-4333-8444-555555555555";
        let answer = |kind: &'static str, uuid: &'static str| {
            move |program: &str, args: &[&str], _: u64| -> Result<String> {
                Ok(if args.contains(&"export") {
                    probe(kind, uuid)
                } else if program == "blkid" {
                    format!("{kind}\n")
                } else {
                    String::new()
                })
            }
        };
        assert!(format_with(Filesystem::Ext4, "/dev/vda2", ROOT, answer("ext4", other)).is_err());
        assert!(
            format_with(
                Filesystem::Ext4,
                "/dev/vda2",
                "not-a-uuid",
                answer("ext4", ROOT)
            )
            .is_err()
        );
        assert!(format_swap_with("/dev/vda3", ROOT, answer("swap", ROOT)).is_ok());
        assert!(format_swap_with("/dev/vda3", ROOT, answer("swap", other)).is_err());
        assert!(format_efi_with("/dev/vda1", "1A2B-3C4D", answer("vfat", "1A2B-3C4D")).is_ok());
        assert!(format_efi_with("/dev/vda1", "1A2B-3C4D", answer("vfat", "1A2B-3C4E")).is_err());
    }
    #[test]
    fn generated_identities_are_well_formed_and_distinct() {
        let a = Identities::generate(Firmware::Uefi, true).unwrap();
        let b = Identities::generate(Firmware::Uefi, true).unwrap();
        assert_ne!(a, b);
        check_uuid(&a.root, "ext4").unwrap();
        check_uuid(a.swap.as_deref().unwrap(), "swap").unwrap();
        check_uuid(a.efi.as_deref().unwrap(), "vfat").unwrap();
        assert_eq!(&a.root[14..15], "4");
        assert!(matches!(&a.root[19..20], "8" | "9" | "a" | "b"));
        let bios = Identities::generate(Firmware::Bios, false).unwrap();
        assert!(bios.efi.is_none() && bios.swap.is_none());
        assert_eq!(uuid_v4([0; 16]), "00000000-0000-4000-8000-000000000000");
        assert_eq!(vfat_serial([0x1a, 0x2b, 0x3c, 0x4d]), "1A2B-3C4D");
    }
    #[test]
    fn missing_kernel_support_and_helpers_fail_preflight() {
        assert!(
            preflight_with(
                Filesystem::Xfs,
                Firmware::Uefi,
                |_, _, _| Ok(String::new()),
                || Ok("nodev\ttmpfs\n\text4\n\tvfat\n".into())
            )
            .is_err()
        );
        let mut calls = 0;
        assert!(
            preflight_with(
                Filesystem::Ext4,
                Firmware::Bios,
                |_, _, _| {
                    calls += 1;
                    anyhow::bail!("missing formatter")
                },
                || panic!("must stop before inspecting the kernel")
            )
            .is_err()
        );
        assert_eq!(calls, 1);
    }
    #[test]
    fn module_load_failures_report_that_the_disk_is_untouched() {
        for failed_module in ["xfs", "vfat"] {
            let error = preflight_with(
                Filesystem::Xfs,
                Firmware::Uefi,
                |program, args, _| {
                    if program == "modprobe" && args == [failed_module] {
                        anyhow::bail!("modprobe timed out");
                    }
                    Ok(String::new())
                },
                || panic!("must stop after the failed module load"),
            )
            .unwrap_err();
            let message = format!("{error:#}");
            assert!(message.contains("no disk writes were made"));
            assert!(message.contains("modprobe timed out"));
            assert!(message.contains(if failed_module == "xfs" {
                "xfs"
            } else {
                "FAT32"
            }));
        }
    }
    /// Run only inside a disposable Linux VM/container with loop and mount access.
    /// Every write is confined to a loop device backed by our newly created file.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires Linux root, loop devices and filesystem tools"]
    fn real_reformat_mount_matrix() -> Result<()> {
        ensure!(
            unsafe { libc::geteuid() } == 0,
            "Needs disposable Linux root"
        );
        struct Image {
            device: String,
            dir: std::path::PathBuf,
        }
        impl Drop for Image {
            fn drop(&mut self) {
                let mount = self.dir.join("mount");
                if let Some(mount) = mount.to_str() {
                    let _ = output("umount", &[mount], 30);
                }
                // Never recursively delete a mountpoint, including on failure.
                if output("losetup", &["--detach", &self.device], 30).is_ok() {
                    let _ = fs::remove_file(self.dir.join("disk.raw"));
                }
                let _ = fs::remove_dir(mount);
                let _ = fs::remove_dir(&self.dir);
            }
        }
        for sector_size in ["512", "4096"] {
            let dir = tempfile::Builder::new()
                .prefix("calamares-fs-test-")
                .tempdir()?
                .keep();
            let backing = dir.join("disk.raw");
            fs::File::create(&backing)?.set_len(2 * 1024u64.pow(3))?;
            fs::create_dir(dir.join("mount"))?;
            let device = output(
                "losetup",
                &[
                    "--find",
                    "--show",
                    "--sector-size",
                    sector_size,
                    backing.to_str().unwrap(),
                ],
                30,
            )?
            .trim()
            .to_owned();
            let image = Image { device, dir };
            ensure!(
                image.device.starts_with("/dev/loop"),
                "Unexpected test device"
            );
            ensure!(
                output(
                    "losetup",
                    &["--noheadings", "--output", "BACK-FILE", &image.device],
                    30
                )?
                .trim()
                    == backing.to_str().unwrap(),
                "Loop backing file changed"
            );
            let mount = image.dir.join("mount");
            let mount = mount.to_str().unwrap();
            for previous in Filesystem::ALL {
                for next in Filesystem::ALL {
                    let old_uuid = uuid_v4(random()?);
                    previous.format(&image.device, &old_uuid)?;
                    // Prime the ordinary userspace probe before replacing it.
                    output("blkid", &[&image.device], 30)?;
                    let new_uuid = uuid_v4(random()?);
                    next.format(&image.device, &new_uuid)?;
                    ensure!(
                        probe_uuid(&image.device, next.name())? == new_uuid,
                        "Reformat did not apply the requested UUID"
                    );
                    next.mount(&image.device, mount)?;
                    ensure!(
                        output(
                            "findmnt",
                            &["--noheadings", "--output", "FSTYPE", "--mountpoint", mount],
                            15
                        )?
                        .trim()
                            == next.name(),
                        "Mounted wrong filesystem"
                    );
                    fs::write(
                        std::path::Path::new(mount).join("sentinel"),
                        b"persistent storage",
                    )?;
                    output("sync", &[], 30)?;
                    output("umount", &[mount], 30)?;
                    next.mount(&image.device, mount)?;
                    ensure!(
                        fs::read(std::path::Path::new(mount).join("sentinel"))?
                            == b"persistent storage",
                        "Data lost on remount"
                    );
                    output("umount", &[mount], 30)?;
                    println!(
                        "PASS sector={sector_size} {} -> {}",
                        previous.name(),
                        next.name()
                    );
                }
            }
            format_efi(&image.device, &vfat_serial(random()?))?;
            output("blkid", &[&image.device], 30)?;
            let serial = vfat_serial(random()?);
            format_efi(&image.device, &serial)?;
            ensure!(
                probe_uuid(&image.device, "vfat")? == serial,
                "FAT reformat did not apply the requested serial"
            );
            mount_efi(&image.device, mount)?;
            ensure!(
                output(
                    "findmnt",
                    &["-n", "-o", "FSTYPE", "--mountpoint", mount],
                    15
                )?
                .trim()
                    == "vfat",
                "Wrong EFI filesystem"
            );
            output("umount", &[mount], 30)?;
            // Swap replaces the FAT signature and must carry the chosen UUID.
            let swap = uuid_v4(random()?);
            format_swap(&image.device, &swap)?;
            ensure!(
                probe_uuid(&image.device, "swap")? == swap,
                "Swap format did not apply the requested UUID"
            );
        }
        Ok(())
    }
}
