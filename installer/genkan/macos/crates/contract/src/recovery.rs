// SPDX-License-Identifier: GPL-3.0-or-later
//! The Recovery app's configuration: which installation it finishes, and
//! the exact bytes it may enroll.
//!
//! The engine writes it beside the Recovery app on the new system's stub
//! (`InstallData/recovery.json`), with the boot object and the firmware
//! baseline. The app bundle itself is signed and identical on every Mac;
//! everything particular to one installation is here, outside it, bound by
//! identity, size and digest. The Recovery app checks all of it against the
//! volume it was started from before asking for a password.
use serde::{Deserialize, Serialize};

use crate::{Digest, Error, Slug, Text, Uuid, Version, invalid, schema_tag};

schema_tag!(RecoverySchema, "sekka.mac.recovery/1");

/// Where the configuration lives on the stub's System volume.
pub const RECOVERY_DATA_DIR: &str = "Sekka/InstallData";
pub const RECOVERY_CONFIG_FILE: &str = "recovery.json";
/// The Recovery app's own record, beside its data.
pub const RECOVERY_STATE_DIR: &str = "Sekka/State";
pub const RECOVERY_RECEIPT_FILE: &str = "receipt.json";

schema_tag!(RecoveryReceiptSchema, "sekka.mac.recovery-receipt/1");

/// What the Recovery app has done for its installation
/// (`Sekka/State/receipt.json` on the stub's System volume). The Recovery
/// app writes it, each step as started before it runs and as completed or
/// failed after, as diagnostics. Nothing trusts it as evidence, since
/// anyone who can write the stub can write it: the helper never reads it,
/// and learns how the Recovery step went from its own inspection as root
/// (`Request::Reconcile`); the Recovery app itself reads the Mac's policy
/// before deciding what to repeat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryReceipt {
    pub schema: RecoveryReceiptSchema,
    pub installation: Uuid,
    /// `bputil`: allowing the startup software.
    pub enrollment: RecoveryStep,
    /// `kmutil configure-boot`, then making Sekka the startup disk.
    pub selection: RecoveryStep,
    pub attempts: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RecoveryStep {
    NotStarted,
    Started { at: crate::Timestamp },
    Completed { at: crate::Timestamp },
    Failed { at: crate::Timestamp, message: Text },
}

impl RecoveryReceipt {
    pub fn new(installation: Uuid) -> Self {
        Self {
            schema: RecoveryReceiptSchema,
            installation,
            enrollment: RecoveryStep::NotStarted,
            selection: RecoveryStep::NotStarted,
            attempts: 0,
        }
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        crate::canonical::from_slice(bytes, 16 * 1024, RecoveryReceiptSchema::VALUE)
    }

    /// Both steps finished: the Mac allows and starts Sekka.
    pub fn finished(&self) -> bool {
        matches!(self.enrollment, RecoveryStep::Completed { .. })
            && matches!(self.selection, RecoveryStep::Completed { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryConfig {
    pub schema: RecoverySchema,
    pub installation: Uuid,
    pub release: Slug,
    /// "Sekka", as the startup options show it.
    pub volume_name: Text,
    pub stub: StubTarget,
    /// The ESP's and the root's GPT partition UUIDs.
    pub esp: Uuid,
    pub root: Uuid,
    /// The m1n1 first stage `kmutil configure-boot` installs, relative to the
    /// data directory.
    pub boot_object: InventoryFile,
    /// Copies of what the ESP needs from this Mac (Apple's firmware for it),
    /// kept to repair the ESP from Recovery.
    pub firmware: Vec<InventoryFile>,
    /// The macOS account that authorized the installation, whose password
    /// Recovery asks for.
    pub owner: Owner,
    pub os_firmware: Version,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StubTarget {
    pub container: Uuid,
    pub volume_group: Uuid,
    pub system_volume: Uuid,
    pub data_volume: Uuid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryFile {
    /// A plain relative path: no `..`, no leading `/`.
    pub path: Text,
    pub size: u64,
    pub digest: Digest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub short_name: Text,
    pub full_name: Text,
    /// The volume group of the macOS that account belongs to.
    pub macos_volume_group: Uuid,
}

impl InventoryFile {
    pub fn check(&self, limit: u64) -> Result<(), Error> {
        let p = self.path.as_str();
        let plain = !p.is_empty()
            && !p.starts_with('/')
            && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
            && p.len() <= 255;
        if !plain {
            return Err(invalid(format!("not a plain relative path: {p}")));
        }
        if self.size == 0 || self.size > limit {
            return Err(invalid(format!("{p} is {} bytes", self.size)));
        }
        Ok(())
    }
}

impl RecoveryConfig {
    pub fn check(&self) -> Result<(), Error> {
        self.boot_object.check(64 * 1024 * 1024)?;
        if self.firmware.is_empty() {
            return Err(invalid("the firmware baseline is empty"));
        }
        for f in &self.firmware {
            f.check(256 * 1024 * 1024)?;
        }
        // A volume group made with `diskutil apfs addVolume … -groupWith`
        // (as Asahi's installer makes the stub's) takes its Data volume's
        // UUID: Omarchy's recorded M1 Pro stub has group and Data volume
        // 0BF26A61-…. So the group may share the Data volume's identity;
        // every other pair must differ.
        let ids = [
            self.stub.container,
            self.stub.system_volume,
            self.stub.data_volume,
            self.esp,
            self.root,
        ];
        for (i, a) in ids.iter().enumerate() {
            if ids[i + 1..].contains(a) {
                return Err(invalid("two of the target's identities are the same"));
            }
        }
        if [self.stub.container, self.stub.system_volume, self.esp, self.root]
            .contains(&self.stub.volume_group)
        {
            return Err(invalid("two of the target's identities are the same"));
        }
        if self.stub.volume_group == self.owner.macos_volume_group {
            return Err(invalid("the stub can't be the owner's own macOS"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RecoveryConfig {
        let u = |n: u8| Uuid::from_bytes([n; 16]);
        let f = |p: &str| InventoryFile {
            path: p.parse().unwrap(),
            size: 10,
            digest: Digest::of(p.as_bytes()),
        };
        RecoveryConfig {
            schema: RecoverySchema,
            installation: u(1),
            release: "sekka-j514s-4882dc33".parse().unwrap(),
            volume_name: "Sekka".parse().unwrap(),
            stub: StubTarget {
                container: u(2),
                volume_group: u(3),
                system_volume: u(4),
                data_volume: u(5),
            },
            esp: u(6),
            root: u(7),
            boot_object: f("boot.bin"),
            firmware: vec![f("firmware/firmware.cpio"), f("firmware/manifest.txt")],
            owner: Owner {
                short_name: "callen".parse().unwrap(),
                full_name: "Chris Allen".parse().unwrap(),
                macos_volume_group: u(8),
            },
            os_firmware: "14.8.3".parse().unwrap(),
        }
    }

    #[test]
    fn a_consistent_configuration() {
        sample().check().unwrap();
    }

    #[test]
    fn paths_and_identities_are_checked() {
        for bad in ["../boot.bin", "/boot.bin", "a//b", "a/./b", ""] {
            let mut c = sample();
            c.boot_object.path = bad.parse().unwrap();
            assert!(c.check().is_err(), "{bad}");
        }
        let mut c = sample();
        c.root = c.esp;
        assert!(c.check().is_err());
        let mut c = sample();
        c.owner.macos_volume_group = c.stub.volume_group;
        assert!(c.check().is_err());
        let mut c = sample();
        c.stub.volume_group = c.stub.system_volume;
        assert!(c.check().is_err());
    }

    #[test]
    fn a_group_may_carry_its_data_volumes_identity() {
        let mut c = sample();
        c.stub.volume_group = c.stub.data_volume;
        c.check().unwrap();
    }
}
