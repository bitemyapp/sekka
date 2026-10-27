//! What Tansu knows about each filesystem: its name, whether it can be
//! made, labelled, checked, repaired and resized through UDisks2 (and
//! how), and its colour on the partition map.

use aizome_kit::Color;
use aizome_kit::theme::Theme;
use aizome_sys::disks::types::filesystem_name;
use aizome_theme::{DYE_RAMP, Rgba};

/// The filesystems Tansu offers when formatting, in its order.
pub const FORMATS: [&str; 7] = ["ext4", "btrfs", "xfs", "vfat", "exfat", "ntfs", "swap"];

/// A filesystem's name for people ("Ext4", "FAT"); "Unformatted" for none.
pub fn name(fs: &str) -> String {
    if fs.is_empty() { "Unformatted".into() } else { filesystem_name(fs).to_owned() }
}

/// What the format sheet says under each choice.
pub fn describe(fs: &str) -> &'static str {
    match fs {
        "ext4" => "Linux's usual filesystem: sturdy, and readable by every Linux.",
        "btrfs" => "Snapshots, checksums and compression. Resizes only while mounted.",
        "xfs" => "Fast with large files. Grows, but can't shrink.",
        "vfat" => "FAT32: every computer, camera and console reads it. Files up to 4 GB.",
        "exfat" => "Large files on drives shared with Windows and macOS.",
        "ntfs" => "Windows's filesystem.",
        "swap" => "Room for memory pages, not files.",
        _ => "",
    }
}

/// The longest label `fs` takes, in bytes (`None`: no label at all).
pub fn label_limit(fs: &str) -> Option<usize> {
    match fs {
        "swap" => Some(15),
        other => aizome_sys::disks::label_limit(other).or(if other.is_empty() { None } else { Some(64) }),
    }
}

/// Whether a filesystem's label can be changed through UDisks (swap's
/// can't: it has no Filesystem interface).
pub fn can_label(fs: &str) -> bool {
    matches!(fs, "ext2" | "ext3" | "ext4" | "xfs" | "btrfs" | "vfat" | "exfat" | "ntfs" | "f2fs" | "nilfs2")
}

/// Whether the label can change while the filesystem is mounted.
pub fn labels_while_mounted(fs: &str) -> bool {
    matches!(fs, "ext2" | "ext3" | "ext4" | "btrfs")
}

/// Whether UDisks can check and repair it.
pub fn can_repair(fs: &str) -> bool {
    matches!(fs, "ext2" | "ext3" | "ext4" | "xfs" | "btrfs" | "vfat" | "exfat" | "ntfs" | "f2fs")
}

/// How a filesystem resizes through UDisks, which resizes only unmounted
/// filesystems here (Tansu's operations refuse mounted devices).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resizable {
    pub grow: bool,
    pub shrink: bool,
    /// Why not, when it can't.
    pub why_not: Option<&'static str>,
}

pub fn resizable(fs: &str) -> Resizable {
    let both = Resizable { grow: true, shrink: true, why_not: None };
    match fs {
        // A partition with nothing on it resizes freely.
        "" | "ext2" | "ext3" | "ext4" | "vfat" | "ntfs" => both,
        "f2fs" => Resizable { grow: true, shrink: false, why_not: Some("F2FS can grow but not shrink.") },
        "btrfs" => Resizable {
            grow: false,
            shrink: false,
            why_not: Some(
                "Btrfs resizes only while mounted (`btrfs filesystem resize`), and Tansu changes only unmounted filesystems.",
            ),
        },
        "xfs" => Resizable {
            grow: false,
            shrink: false,
            why_not: Some("XFS grows only while mounted (`xfs_growfs`) and never shrinks."),
        },
        "exfat" => Resizable {
            grow: false,
            shrink: false,
            why_not: Some("exFAT can't be resized: copy its files off, then format."),
        },
        "swap" => Resizable {
            grow: false,
            shrink: false,
            why_not: Some("Swap can't be resized: delete it and make a new one."),
        },
        "crypto_LUKS" => Resizable {
            grow: false,
            shrink: false,
            why_not: Some("Encrypted volumes resize from inside (cryptsetup resize), which UDisks doesn't offer."),
        },
        _ => {
            Resizable { grow: false, shrink: false, why_not: Some("Tansu doesn't know how to resize this filesystem.") }
        }
    }
}

/// Whether resizing `fs` needs a forced check first (resize2fs refuses
/// an offline resize when it wasn't checked since it was last mounted).
pub fn check_before_resize(fs: &str) -> bool {
    matches!(fs, "ext2" | "ext3" | "ext4")
}

/// The colour of a filesystem on the partition map: the dye ramp for
/// Linux's own, the accents for the rest.
pub fn color(fs: &str, theme: &Theme) -> Color {
    let p = &theme.palette;
    let dye = |i: usize| DYE_RAMP[i].2;
    let c: Rgba = match fs {
        "ext2" | "ext3" | "ext4" => p.blue,
        "btrfs" => p.violet,
        "xfs" | "f2fs" | "nilfs2" => p.cyan,
        "vfat" => p.amber,
        "exfat" => p.amber.mix(p.red, 0.35),
        "ntfs" => p.green,
        "swap" => dye(2),
        "crypto_LUKS" => dye(0).mix(dye(2), 0.55),
        "LVM2_member" | "linux_raid_member" | "zfs_member" => p.violet.mix(p.text3, 0.4),
        "iso9660" | "udf" => p.cyan.mix(p.text3, 0.3),
        _ => p.text3,
    };
    aizome_kit::theme::color(c)
}

/// Whether text over `fill` should be dark.
pub fn dark_text_on(fill: Color) -> bool {
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    let l = Rgba { r: byte(fill.r), g: byte(fill.g), b: byte(fill.b), a: 255 }.luminance();
    l > 0.3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_format_choice_is_named_and_described() {
        for fs in FORMATS {
            assert!(!describe(fs).is_empty());
            assert_ne!(name(fs), fs, "{fs} has a name for people");
        }
        assert_eq!(name(""), "Unformatted");
    }

    #[test]
    fn resizing_follows_what_udisks_can_do_offline() {
        assert!(resizable("ext4").shrink && resizable("ext4").grow);
        assert!(!resizable("xfs").grow, "xfs_growfs needs it mounted");
        assert!(resizable("btrfs").why_not.is_some());
        assert!(resizable("").shrink, "an empty partition resizes freely");
        assert!(check_before_resize("ext4") && !check_before_resize("vfat"));
    }

    #[test]
    fn labels_have_limits() {
        assert_eq!(label_limit("vfat"), Some(11));
        assert_eq!(label_limit("ext4"), Some(16));
        assert!(!can_label("swap"));
    }
}
