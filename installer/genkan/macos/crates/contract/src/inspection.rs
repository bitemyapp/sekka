// SPDX-License-Identifier: GPL-3.0-or-later
//! What the engine found on this Mac, without changing anything: the
//! machine, macOS, power, FileVault, the internal disk's layout and where
//! Sekka could go.
//!
//! The engine (Asahi's installer with Sekka's adapter) writes it; the app
//! and the helper read it. The helper runs its own inspection as root before
//! anything changes and compares the layout digest with the plan's.
use serde::{Deserialize, Serialize};

use crate::{Digest, Error, MachineFacts, Text, Uuid, Version, invalid, schema_tag};

schema_tag!(InspectionSchema, "sekka.mac.inspection/1");

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inspection {
    pub schema: InspectionSchema,
    pub machine: Machine,
    pub macos: Macos,
    pub power: Power,
    pub filevault: FileVault,
    pub disk: Disk,
    /// Over the disk's identity and every partition and container, as
    /// [`Disk::layout_digest`] computes it.
    pub layout_digest: Digest,
    /// Where Sekka could go, largest first.
    pub candidates: Vec<Candidate>,
    /// Other systems and partial installations on this disk, as recognized.
    pub installations: Vec<Installation>,
    pub issues: Vec<Issue>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Machine {
    pub product_type: Text,
    pub device_class: Text,
    pub chip_id: u32,
    pub board_id: u32,
    /// "MacBook Pro (14-inch, Nov 2023)", from the device tree.
    pub product_name: Text,
    pub soc_name: Text,
    pub system_firmware: Text,
    /// The digest of the hardware UUID under Sekka's domain string, so plans
    /// and records name this Mac without carrying its UUID.
    pub binding: Digest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Macos {
    pub version: Version,
    pub build: Text,
    /// The system firmware's own macOS version (the SFR).
    pub system_firmware_version: Version,
    /// The volume group macOS is running from.
    pub volume_group: Uuid,
    /// Booted from the internal disk in normal macOS (not Recovery, not an
    /// external disk).
    pub internal_boot: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Power {
    pub has_battery: bool,
    /// On a charger. Desktops always are.
    pub on_ac: bool,
    pub battery_percent: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileVault {
    On,
    Off,
    /// Turning on or off right now; resizing waits for it to finish.
    Converting,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Disk {
    /// `disk0`, for display and diagnostics only: never an identity.
    pub device: Text,
    /// The GPT disk UUID.
    pub uuid: Uuid,
    pub size: u64,
    pub block_size: u32,
    pub partitions: Vec<Partition>,
    pub containers: Vec<Container>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Partition {
    /// The GPT partition entry's UUID.
    pub uuid: Uuid,
    pub kind: PartitionKind,
    /// The GPT name, or the FAT volume name for an ESP.
    pub name: Text,
    pub offset: u64,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PartitionKind {
    /// iBoot's system container (`Apple_APFS_ISC`).
    AppleIsc,
    /// An APFS container (`Apple_APFS`).
    Apfs,
    /// The system Recovery container (`Apple_APFS_Recovery`).
    AppleRecovery,
    Efi,
    Linux,
    LinuxSwap,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Container {
    pub uuid: Uuid,
    /// The partition holding it.
    pub partition: Uuid,
    pub size: u64,
    pub free: u64,
    pub volume_groups: Vec<VolumeGroup>,
    /// From `diskutil apfs resizeContainer … limits`, when it could answer.
    pub resize: Option<ResizeLimits>,
    /// Local APFS snapshots, which can stop a resize.
    pub snapshots: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResizeLimits {
    /// The smallest the container can become.
    pub minimum: u64,
    /// The smallest Apple recommends.
    pub recommended: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolumeGroup {
    pub uuid: Uuid,
    /// The volume's name as Startup Disk shows it.
    pub name: Text,
    pub system_volume: Option<Uuid>,
    pub data_volume: Option<Uuid>,
    pub os: GroupOs,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum GroupOs {
    Macos { version: Version },
    /// An Asahi-style stub for another system: a minimal macOS with its own
    /// Recovery, whose name and records say what it is for.
    Stub { for_system: Text },
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extent {
    pub offset: u64,
    pub size: u64,
}
impl Extent {
    pub fn end(&self) -> u64 {
        self.offset.saturating_add(self.size)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Candidate {
    /// Unallocated space already on the disk.
    FreeSpace { extent: Extent },
    /// Space made by shrinking an APFS container, after which it is
    /// unallocated directly behind that container.
    ShrinkContainer {
        container: Uuid,
        partition: Uuid,
        current_size: u64,
        /// The most the container can give: its size less its minimum.
        available: u64,
        /// What it can give and still be at Apple's recommended size.
        available_recommended: u64,
        /// Free space directly behind the container that joins the new
        /// space.
        adjacent_free: u64,
    },
}
impl Candidate {
    /// The most Sekka could have from this candidate.
    pub fn capacity(&self) -> u64 {
        match self {
            Candidate::FreeSpace { extent } => extent.size,
            Candidate::ShrinkContainer {
                available,
                adjacent_free,
                ..
            } => available + adjacent_free,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Installation {
    pub kind: InstallationKind,
    pub name: Text,
    pub state: InstallationState,
    pub stub_group: Option<Uuid>,
    pub esp: Option<Uuid>,
    pub root: Option<Uuid>,
    /// Read from its handoff or Recovery configuration, if it is Sekka's.
    pub installation_id: Option<Uuid>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallationKind {
    Sekka,
    /// Another system installed by Asahi's installer (Asahi, Omarchy, …).
    OtherLinux,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallationState {
    /// Every part is present and Recovery finished.
    Complete,
    /// Waiting for its Recovery step.
    WaitingForRecovery,
    /// Some parts missing: an interrupted install or a partial removal.
    Partial,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Issue {
    pub code: IssueCode,
    /// Whether it stops installation.
    pub blocking: bool,
    pub detail: Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IssueCode {
    UnsupportedMachine,
    UntestedFirmware,
    MacosTooOld,
    KnownMacosIssue,
    /// Started from Recovery, an external disk or a non-default system.
    NotNormalBoot,
    OnBattery,
    BatteryLow,
    FileVaultConverting,
    /// Snapshots exist; Apple's resize may refuse to move past them.
    LocalSnapshots,
    /// Neither free space nor a shrinkable container gives the minimum.
    NotEnoughSpace,
    /// The resize limits could not be read; shrinking isn't offered.
    ResizeLimitsUnavailable,
    /// The disk holds something the engine can't describe with confidence.
    UnrecognizedLayout,
    ExistingSekka,
    PartialInstallation,
    /// More than one internal disk or APFS store.
    AmbiguousDisk,
}

/// Facts about this Mac in the form profiles match.
impl Inspection {
    pub fn machine_facts(&self) -> MachineFacts {
        MachineFacts {
            product_type: self.machine.product_type.to_string(),
            device_class: self.machine.device_class.to_string(),
            chip_id: self.machine.chip_id,
            system_firmware: self.machine.system_firmware.to_string(),
            macos: self.macos.version.clone(),
        }
    }

    pub fn blocking(&self) -> impl Iterator<Item = &Issue> {
        self.issues.iter().filter(|i| i.blocking)
    }

    /// Internal consistency: what the engine says agrees with itself.
    pub fn check(&self) -> Result<(), Error> {
        self.disk.check()?;
        if self.layout_digest != self.disk.layout_digest()? {
            return Err(invalid("the inspection's layout digest doesn't match its disk"));
        }
        for c in &self.candidates {
            match c {
                Candidate::FreeSpace { extent } => {
                    if extent.size == 0 || extent.end() > self.disk.size {
                        return Err(invalid("a free extent lies outside the disk"));
                    }
                    if self
                        .disk
                        .partitions
                        .iter()
                        .any(|p| p.offset < extent.end() && extent.offset < p.offset + p.size)
                    {
                        return Err(invalid("a free extent overlaps a partition"));
                    }
                }
                Candidate::ShrinkContainer {
                    container,
                    partition,
                    current_size,
                    available,
                    available_recommended,
                    ..
                } => {
                    let found = self
                        .disk
                        .containers
                        .iter()
                        .find(|k| &k.uuid == container && &k.partition == partition)
                        .ok_or_else(|| invalid("a shrink candidate names no container"))?;
                    if found.size != *current_size
                        || available_recommended > available
                        || available >= current_size
                    {
                        return Err(invalid("a shrink candidate's sizes are inconsistent"));
                    }
                }
            }
        }
        Ok(())
    }
}

impl Disk {
    pub fn check(&self) -> Result<(), Error> {
        if self.block_size == 0 || !self.block_size.is_power_of_two() {
            return Err(invalid("the disk's block size isn't a power of two"));
        }
        let mut sorted: Vec<&Partition> = self.partitions.iter().collect();
        sorted.sort_by_key(|p| p.offset);
        for pair in sorted.windows(2) {
            if pair[0].offset + pair[0].size > pair[1].offset {
                return Err(invalid("partitions overlap"));
            }
        }
        if sorted
            .last()
            .is_some_and(|p| p.offset + p.size > self.size)
        {
            return Err(invalid("a partition extends past the disk"));
        }
        for container in &self.containers {
            if !self
                .partitions
                .iter()
                .any(|p| p.uuid == container.partition && p.kind == PartitionKind::Apfs)
            {
                return Err(invalid("a container names no APFS partition"));
            }
        }
        Ok(())
    }

    /// The digest that binds a plan to this exact layout: the disk, then
    /// every partition in offset order, then every container's size and
    /// volume groups. Free space, snapshots and resize limits are not
    /// included: they change as macOS runs, and the helper checks the
    /// limits again before shrinking.
    pub fn layout_digest(&self) -> Result<Digest, Error> {
        #[derive(Serialize)]
        struct Layout<'a> {
            domain: &'static str,
            uuid: &'a Uuid,
            size: u64,
            block_size: u32,
            partitions: Vec<(&'a Uuid, PartitionKind, u64, u64)>,
            containers: Vec<(&'a Uuid, &'a Uuid, u64, Vec<&'a Uuid>)>,
        }
        let mut partitions: Vec<_> = self
            .partitions
            .iter()
            .map(|p| (&p.uuid, p.kind, p.offset, p.size))
            .collect();
        partitions.sort_by_key(|p| p.2);
        let mut containers: Vec<_> = self
            .containers
            .iter()
            .map(|c| {
                let mut groups: Vec<&Uuid> = c.volume_groups.iter().map(|g| &g.uuid).collect();
                groups.sort();
                (&c.uuid, &c.partition, c.size, groups)
            })
            .collect();
        containers.sort();
        crate::canonical::digest(&Layout {
            domain: "sekka.mac.layout/1",
            uuid: &self.uuid,
            size: self.size,
            block_size: self.block_size,
            partitions,
            containers,
        })
    }
}

/// The domain string the engine hashes the hardware UUID under for
/// [`Machine::binding`].
pub const MACHINE_BINDING_DOMAIN: &str = "sekka.mac.machine/1\n";

/// Compute a machine binding from the hardware UUID as `ioreg` prints it.
pub fn machine_binding(platform_uuid: &str) -> Result<Digest, Error> {
    let uuid = Uuid::parse_any(platform_uuid)?;
    let mut bytes = MACHINE_BINDING_DOMAIN.as_bytes().to_vec();
    bytes.extend(uuid.to_string().as_bytes());
    Ok(Digest::of(&bytes))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn sample() -> Inspection {
        let u = |s: &str| Uuid::parse_any(s).unwrap();
        let gib = crate::GIB;
        let disk = Disk {
            device: "disk0".parse().unwrap(),
            uuid: u("0d4e3e3b-0b3a-4b0a-9a0a-000000000000"),
            size: 500 * gib,
            block_size: 4096,
            partitions: vec![
                Partition {
                    uuid: u("d5819e8f-c8c3-4e98-b399-89b992e8dab9"),
                    kind: PartitionKind::AppleIsc,
                    name: "iBootSystemContainer".parse().unwrap(),
                    offset: 24576,
                    size: 524288000,
                },
                Partition {
                    uuid: u("b2e3bc98-c2cf-409e-bfc0-f5ce86a8c5d9"),
                    kind: PartitionKind::Apfs,
                    name: "Container".parse().unwrap(),
                    offset: 524312576,
                    size: 494 * gib,
                },
                Partition {
                    uuid: u("1711f7a0-113b-4b89-bc86-e6fa3c675fca"),
                    kind: PartitionKind::AppleRecovery,
                    name: "RecoveryOSContainer".parse().unwrap(),
                    offset: 524312576 + 494 * gib,
                    size: 5 * gib,
                },
            ],
            containers: vec![Container {
                uuid: u("c54f285b-fe6e-4d0c-9bc6-567c62bd7c20"),
                partition: u("b2e3bc98-c2cf-409e-bfc0-f5ce86a8c5d9"),
                size: 494 * gib,
                free: 300 * gib,
                volume_groups: vec![VolumeGroup {
                    uuid: u("8e945eba-968d-4906-966b-e07ec593e169"),
                    name: "Macintosh HD".parse().unwrap(),
                    system_volume: Some(u("11111111-2222-4333-8444-555555555555")),
                    data_volume: Some(u("11111111-2222-4333-8444-666666666666")),
                    os: GroupOs::Macos {
                        version: "27.0.1".parse().unwrap(),
                    },
                }],
                resize: Some(ResizeLimits {
                    minimum: 176 * gib,
                    recommended: 187 * gib,
                }),
                snapshots: 0,
            }],
        };
        Inspection {
            schema: InspectionSchema,
            machine: Machine {
                product_type: "Mac15,6".parse().unwrap(),
                device_class: "j514sap".parse().unwrap(),
                chip_id: 0x6030,
                board_id: 0x44,
                product_name: "MacBook Pro (14-inch, Nov 2023)".parse().unwrap(),
                soc_name: "Apple M3 Pro".parse().unwrap(),
                system_firmware: "mBoot-20457.1.29".parse().unwrap(),
                binding: machine_binding("00000000-0000-4000-8000-000000000000").unwrap(),
            },
            macos: Macos {
                version: "27.0.1".parse().unwrap(),
                build: "26A434".parse().unwrap(),
                system_firmware_version: "27.0".parse().unwrap(),
                volume_group: u("8e945eba-968d-4906-966b-e07ec593e169"),
                internal_boot: true,
            },
            power: Power {
                has_battery: true,
                on_ac: true,
                battery_percent: Some(80),
            },
            filevault: FileVault::On,
            layout_digest: disk.layout_digest().unwrap(),
            candidates: vec![Candidate::ShrinkContainer {
                container: u("c54f285b-fe6e-4d0c-9bc6-567c62bd7c20"),
                partition: u("b2e3bc98-c2cf-409e-bfc0-f5ce86a8c5d9"),
                current_size: 494 * gib,
                available: 318 * gib,
                available_recommended: 307 * gib,
                adjacent_free: 0,
            }],
            disk,
            installations: vec![],
            issues: vec![],
        }
    }

    #[test]
    fn the_sample_is_consistent() {
        sample().check().unwrap();
    }

    #[test]
    fn the_layout_digest_changes_with_the_layout_only() {
        let a = sample();
        let mut b = sample();
        b.disk.containers[0].free -= crate::GIB;
        b.disk.containers[0].snapshots = 2;
        assert_eq!(a.disk.layout_digest().unwrap(), b.disk.layout_digest().unwrap());
        b.disk.partitions[1].size -= crate::GIB;
        assert_ne!(a.disk.layout_digest().unwrap(), b.disk.layout_digest().unwrap());
        assert!(b.check().is_err(), "the stored digest no longer matches");
    }

    #[test]
    fn overlapping_extents_are_refused() {
        let mut s = sample();
        s.candidates.push(Candidate::FreeSpace {
            extent: Extent {
                offset: 524312576,
                size: crate::GIB,
            },
        });
        assert!(s.check().is_err());
        let mut s = sample();
        s.disk.partitions[2].offset -= crate::GIB;
        s.layout_digest = s.disk.layout_digest().unwrap();
        assert!(s.check().is_err());
    }

    #[test]
    fn machine_bindings_ignore_case_and_hide_the_uuid() {
        let a = machine_binding("ABCDEF01-2345-4678-89AB-CDEF01234567").unwrap();
        let b = machine_binding("abcdef01-2345-4678-89ab-cdef01234567").unwrap();
        assert_eq!(a, b);
        assert!(!a.to_string().contains("abcdef01"));
    }
}
