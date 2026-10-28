// SPDX-License-Identifier: GPL-3.0-or-later
//! The live media: whether "Copy to RAM" was asked for at boot but the
//! session still runs from the USB drive or DVD (nix/iso-copytoram.nix keeps
//! the media mounted when a complete copy wouldn't fit). Genkan says so
//! when it opens, so nobody unplugs the media the session still reads.
use std::fs;

/// The boot asked to copy the media to memory and `/iso` isn't that copy.
pub fn copy_to_ram_failed() -> bool {
    let (Ok(cmdline), Ok(mounts)) = (
        fs::read_to_string("/proc/cmdline"),
        fs::read_to_string("/proc/self/mountinfo"),
    ) else {
        return false;
    };
    copy_to_ram_requested(&cmdline) && iso_filesystem(&mounts).is_some_and(|fs| fs != "tmpfs")
}

fn copy_to_ram_requested(cmdline: &str) -> bool {
    cmdline
        .split_whitespace()
        .any(|word| word == "copytoram" || word.starts_with("copytoram="))
}

/// The filesystem type mounted at `/iso`, from mountinfo's lines:
/// `id parent major:minor root mountpoint options [optional…] - type source super`.
fn iso_filesystem(mountinfo: &str) -> Option<&str> {
    mountinfo.lines().rev().find_map(|line| {
        let mut fields = line.split_whitespace();
        let mountpoint = fields.nth(4)?;
        let (_, after) = line.split_once(" - ")?;
        (mountpoint == "/iso")
            .then(|| after.split_whitespace().next())
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copy_to_ram_is_read_from_the_kernel_command_line() {
        assert!(copy_to_ram_requested("init=/x copytoram quiet"));
        assert!(copy_to_ram_requested("copytoram=1"));
        assert!(!copy_to_ram_requested("nocopytoram quiet"));
        assert!(!copy_to_ram_requested(""));
    }
    #[test]
    fn the_iso_mount_is_found_by_its_mountpoint() {
        let media = "30 1 0:27 / / rw - tmpfs none rw\n\
                     41 30 7:0 / /iso ro,relatime - iso9660 /dev/sr0 ro\n";
        assert_eq!(iso_filesystem(media), Some("iso9660"));
        let copied = "41 30 0:44 / /iso rw shared:9 - tmpfs tmpfs rw,size=8G\n";
        assert_eq!(iso_filesystem(copied), Some("tmpfs"));
        assert_eq!(iso_filesystem("30 1 0:27 / / rw - tmpfs none rw\n"), None);
    }
}
