//! Partition types and flags by name: GPT type GUIDs and MBR type bytes as
//! Tansu shows them, and the flag bits UDisks reports.

/// A partition type: the identifier UDisks uses and a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartitionType {
    /// A GPT type GUID (lowercase) or an MBR type (`0x83`).
    pub id: &'static str,
    pub name: &'static str,
    /// Offered when creating partitions (rather than only recognised).
    pub common: bool,
}

const fn t(id: &'static str, name: &'static str, common: bool) -> PartitionType {
    PartitionType { id, name, common }
}

/// GPT partition types, the common ones first.
pub const GPT_TYPES: &[PartitionType] = &[
    t("0fc63daf-8483-4772-8e79-3d69d8477de4", "Linux filesystem", true),
    t("c12a7328-f81f-11d2-ba4b-00a0c93ec93b", "EFI System", true),
    t("ebd0a0a2-b9e5-4433-87c0-68b6b72699c7", "Microsoft basic data", true),
    t("0657fd6d-a4ab-43c4-84e5-0933c84b4f4f", "Linux swap", true),
    t("ca7d7ccb-63ed-4c53-861c-1742536059cc", "Linux LUKS", true),
    t("e6d6d379-f507-44c2-a23c-238f2a3df928", "Linux LVM", true),
    t("a19d880f-05fc-4d3b-a006-743f0f84911e", "Linux RAID", true),
    t("933ac7e1-2eb4-4f13-b844-0e14e2aef915", "Linux home", true),
    t("bc13c2ff-59e6-4262-a352-b275fd6f7172", "Linux extended boot", true),
    t("21686148-6449-6e6f-744e-656564454649", "BIOS boot", true),
    t("4f68bce3-e8cd-4db1-96e7-fbcaf984b709", "Linux root (x86-64)", false),
    t("b921b045-1df0-41c3-af44-4c6f280d3fae", "Linux root (ARM64)", false),
    t("44479540-f297-41b2-9af7-d131d5f0458a", "Linux root (x86)", false),
    t("8484680c-9521-48c6-9c11-b0720656f69e", "Linux /usr (x86-64)", false),
    t("b0e01050-ee5f-4390-949a-9101b17104e9", "Linux /usr (ARM64)", false),
    t("3b8f8425-20e0-4f3b-907f-1a25a76f98e8", "Linux server data", false),
    t("4d21b016-b534-45c2-a9fb-5c16e091fd2d", "Linux variable data", false),
    t("7ec6f557-3bc5-4aca-b293-16ef5df639d1", "Linux temporary data", false),
    t("773f91ef-66d4-49b5-bd83-d683bf40ad16", "Linux user's home", false),
    t("e3c9e316-0b5c-4db8-817d-f92df00215ae", "Microsoft reserved", false),
    t("de94bba4-06d1-4d40-a16a-bfd50179d6ac", "Windows recovery", false),
    t("5808c8aa-7e8f-42e0-85d2-e1e90434cfb3", "Windows LDM metadata", false),
    t("af9b60a0-1431-4f62-bc68-3311714a69ad", "Windows LDM data", false),
    t("48465300-0000-11aa-aa11-00306543ecac", "Apple HFS+", false),
    t("7c3457ef-0000-11aa-aa11-00306543ecac", "Apple APFS", false),
    t("426f6f74-0000-11aa-aa11-00306543ecac", "Apple boot", false),
    t("516e7cb4-6ecf-11d6-8ff8-00022d09712b", "FreeBSD data", false),
    t("516e7cba-6ecf-11d6-8ff8-00022d09712b", "FreeBSD ZFS", false),
    t("6a898cc3-1dd2-11b2-99a6-080020736631", "ZFS", false),
    t("fe3a2a5d-4f32-41a7-b725-accc3285a309", "ChromeOS kernel", false),
    t("3cb8e202-3b7e-47dd-8a3c-7ff2a13cfcec", "ChromeOS root", false),
    t("024dee41-33e7-11d3-9d69-0008c781f39f", "MBR partition scheme", false),
];

/// MBR (DOS) partition types, the common ones first.
pub const DOS_TYPES: &[PartitionType] = &[
    t("0x83", "Linux", true),
    t("0x0c", "W95 FAT32 (LBA)", true),
    t("0x07", "HPFS/NTFS/exFAT", true),
    t("0x82", "Linux swap", true),
    t("0xef", "EFI (FAT-12/16/32)", true),
    t("0x8e", "Linux LVM", true),
    t("0xfd", "Linux RAID autodetect", true),
    t("0x05", "Extended", true),
    t("0x0b", "W95 FAT32", false),
    t("0x0e", "W95 FAT16 (LBA)", false),
    t("0x06", "FAT16", false),
    t("0x01", "FAT12", false),
    t("0x0f", "W95 Extended (LBA)", false),
    t("0x85", "Linux extended", false),
    t("0x27", "Hidden NTFS WinRE", false),
    t("0xa5", "FreeBSD", false),
    t("0xa8", "Darwin UFS", false),
    t("0xaf", "HFS / HFS+", false),
    t("0xee", "GPT protective", false),
    t("0x00", "Empty", false),
];

/// Normalise an MBR type (`0xC`, `0x0c`, `12`) to `0x0c`.
fn dos_id(id: &str) -> Option<String> {
    let n = match id.strip_prefix("0x").or_else(|| id.strip_prefix("0X")) {
        Some(hex) => u8::from_str_radix(hex, 16).ok()?,
        None => id.parse::<u8>().ok()?,
    };
    Some(format!("0x{n:02x}"))
}

/// The name of a partition type, by GUID or MBR type.
pub fn partition_type_name(id: &str) -> Option<&'static str> {
    let lower = id.to_ascii_lowercase();
    GPT_TYPES
        .iter()
        .find(|p| p.id == lower)
        .or_else(|| {
            let id = dos_id(&lower)?;
            DOS_TYPES.iter().find(|p| p.id == id)
        })
        .map(|p| p.name)
}

/// The partition type to use for a filesystem on a table of `table_kind`
/// (`gpt` or `dos`).
pub fn type_for_filesystem(table_kind: &str, fs_type: &str) -> &'static str {
    let gpt = table_kind == "gpt";
    match fs_type {
        "vfat" | "ntfs" | "exfat" if gpt => "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7",
        "vfat" => "0x0c",
        "ntfs" | "exfat" => "0x07",
        "swap" if gpt => "0657fd6d-a4ab-43c4-84e5-0933c84b4f4f",
        "swap" => "0x82",
        "crypto_LUKS" | "luks" if gpt => "ca7d7ccb-63ed-4c53-861c-1742536059cc",
        _ if gpt => "0fc63daf-8483-4772-8e79-3d69d8477de4",
        _ => "0x83",
    }
}

/// GPT attribute bits and the MBR boot flag, as UDisks's `Flags`.
pub mod flags {
    /// GPT: required by the platform (firmware).
    pub const GPT_SYSTEM: u64 = 1 << 0;
    /// GPT: legacy BIOS bootable.
    pub const GPT_LEGACY_BOOT: u64 = 1 << 2;
    /// GPT (Microsoft basic data): read-only.
    pub const GPT_READ_ONLY: u64 = 1 << 60;
    /// GPT (Microsoft basic data): hidden.
    pub const GPT_HIDDEN: u64 = 1 << 62;
    /// GPT (Microsoft basic data): don't mount automatically.
    pub const GPT_NO_AUTOMOUNT: u64 = 1 << 63;
    /// MBR: the boot (active) flag.
    pub const DOS_BOOTABLE: u64 = 0x80;
}

/// Names for the flags set in `bits` on a table of `table_kind`.
pub fn flag_names(table_kind: &str, bits: u64) -> Vec<&'static str> {
    let named: &[(u64, &str)] = if table_kind == "dos" {
        &[(flags::DOS_BOOTABLE, "boot")]
    } else {
        &[
            (flags::GPT_SYSTEM, "system"),
            (flags::GPT_LEGACY_BOOT, "legacy BIOS boot"),
            (flags::GPT_READ_ONLY, "read-only"),
            (flags::GPT_HIDDEN, "hidden"),
            (flags::GPT_NO_AUTOMOUNT, "no automount"),
        ]
    };
    named.iter().filter(|(bit, _)| bits & bit != 0).map(|(_, name)| *name).collect()
}

/// A filesystem type's name for people.
pub fn filesystem_name(fs_type: &str) -> &str {
    match fs_type {
        "ext2" => "Ext2",
        "ext3" => "Ext3",
        "ext4" => "Ext4",
        "xfs" => "XFS",
        "btrfs" => "Btrfs",
        "f2fs" => "F2FS",
        "vfat" => "FAT",
        "exfat" => "exFAT",
        "ntfs" => "NTFS",
        "udf" => "UDF",
        "iso9660" => "ISO 9660",
        "nilfs2" => "NILFS2",
        "swap" => "Swap",
        "crypto_LUKS" => "LUKS encryption",
        "LVM2_member" => "LVM physical volume",
        "linux_raid_member" => "RAID member",
        "zfs_member" => "ZFS member",
        "bcachefs" => "bcachefs",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_by_guid_and_byte() {
        assert_eq!(partition_type_name("C12A7328-F81F-11D2-BA4B-00A0C93EC93B"), Some("EFI System"));
        assert_eq!(partition_type_name("0x83"), Some("Linux"));
        assert_eq!(partition_type_name("0xC"), Some("W95 FAT32 (LBA)"));
        assert_eq!(partition_type_name("131"), Some("Linux"));
        assert_eq!(partition_type_name("nonsense"), None);
        assert_eq!(type_for_filesystem("gpt", "ext4"), "0fc63daf-8483-4772-8e79-3d69d8477de4");
        assert_eq!(type_for_filesystem("dos", "vfat"), "0x0c");
    }

    #[test]
    fn flags_by_name() {
        assert_eq!(flag_names("gpt", flags::GPT_SYSTEM | flags::GPT_NO_AUTOMOUNT), ["system", "no automount"]);
        assert_eq!(flag_names("dos", 0x80), ["boot"]);
        assert!(flag_names("gpt", 0).is_empty());
    }
}
