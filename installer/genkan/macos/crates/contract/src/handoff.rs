// SPDX-License-Identifier: GPL-3.0-or-later
//! The first-boot handoff: how a freshly installed Sekka knows which disk
//! is its own, and what macOS suggested for its setup.
//!
//! The image is the same for every installation, so its root filesystem and
//! ESP carry the same labels and filesystem UUIDs everywhere: two Sekka
//! installations on one Mac (or an external disk with one) are
//! indistinguishable by them. Only the GPT partition UUIDs are unique, and
//! only the engine knows them, after it partitions. So the engine writes:
//!
//! - `sekka/install.json` on the ESP: this document;
//! - `sekka.root=PARTUUID=…` and `sekka.esp=PARTUUID=…` into the first
//!   generation's boot entry on the same ESP, replacing the image's
//!   `@SEKKA_ROOT_PARTUUID@` and `@SEKKA_ESP_PARTUUID@` placeholders.
//!
//! The initrd mounts the partitions those arguments name, and nothing else;
//! first-boot setup writes the same identities into the installed
//! configuration, so every later generation names them directly.
//! Everything here is public: no secret, password or account is handed off.
use serde::{Deserialize, Serialize};

use crate::{Error, GitRevision, Slug, Text, Timestamp, Uuid, canonical, invalid, limits, schema_tag};

schema_tag!(HandoffSchema, "sekka.mac.handoff/1");

/// The handoff's path on the ESP.
pub const HANDOFF_PATH: &str = "sekka/install.json";
/// Kernel arguments naming the installation's partitions.
pub const ROOT_ARGUMENT: &str = "sekka.root";
pub const ESP_ARGUMENT: &str = "sekka.esp";
/// Placeholders in the image's boot entry, replaced by the engine.
pub const ROOT_PLACEHOLDER: &str = "@SEKKA_ROOT_PARTUUID@";
pub const ESP_PLACEHOLDER: &str = "@SEKKA_ESP_PARTUUID@";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Handoff {
    pub schema: HandoffSchema,
    pub installation: Uuid,
    pub release: Slug,
    pub source_revision: GitRevision,
    /// GPT partition UUIDs.
    pub root: Uuid,
    pub esp: Uuid,
    pub created: Timestamp,
    pub hints: SetupHints,
}

/// What macOS was set to, offered as setup's starting choices. Setup checks
/// each against what it supports and ignores the rest; none is applied
/// without the person seeing it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetupHints {
    /// A glibc locale: `en_US.UTF-8`.
    pub locale: Option<Text>,
    /// An XKB layout: `us`, `de`.
    pub keyboard: Option<Text>,
    /// An IANA zone: `America/Chicago`.
    pub timezone: Option<Text>,
    /// macOS's computer name, for suggesting a host name.
    pub computer_name: Option<Text>,
    /// The macOS account's full name, for suggesting the Linux account's.
    pub full_name: Option<Text>,
}

impl Handoff {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let handoff: Self = canonical::from_slice(bytes, limits::HANDOFF, HandoffSchema::VALUE)?;
        if handoff.root == handoff.esp {
            return Err(invalid("the root and the ESP are the same partition"));
        }
        Ok(handoff)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        canonical::to_vec(self)
    }

    /// The kernel arguments the boot entry carries.
    pub fn kernel_arguments(&self) -> String {
        format!(
            "{ROOT_ARGUMENT}=PARTUUID={} {ESP_ARGUMENT}=PARTUUID={}",
            self.root, self.esp
        )
    }

    /// Replace the boot entry's placeholders, refusing an entry without
    /// exactly one of each.
    pub fn fill_entry(&self, entry: &str) -> Result<String, Error> {
        for p in [ROOT_PLACEHOLDER, ESP_PLACEHOLDER] {
            if entry.matches(p).count() != 1 {
                return Err(invalid(format!("the boot entry must contain {p} once")));
            }
        }
        Ok(entry
            .replace(ROOT_PLACEHOLDER, &self.root.to_string())
            .replace(ESP_PLACEHOLDER, &self.esp.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Handoff {
        Handoff {
            schema: HandoffSchema,
            installation: Uuid::from_bytes([1; 16]),
            release: "sekka-j514s-4882dc33".parse().unwrap(),
            source_revision: "4882dc33652612e7475f467f6373d8b607e5407c".parse().unwrap(),
            root: "63d89be1-72f7-4aff-9679-09cafa7e0c2d".parse().unwrap(),
            esp: "910de5ca-7390-4716-9a54-8028916e4d78".parse().unwrap(),
            created: "2026-11-06T21:00:00Z".parse().unwrap(),
            hints: SetupHints {
                locale: Some("en_US.UTF-8".parse().unwrap()),
                keyboard: Some("us".parse().unwrap()),
                timezone: Some("America/Chicago".parse().unwrap()),
                computer_name: Some("Chris's MacBook Pro".parse().unwrap()),
                full_name: None,
            },
        }
    }

    #[test]
    fn round_trips() {
        let h = sample();
        assert_eq!(Handoff::parse(&h.to_bytes().unwrap()).unwrap(), h);
    }

    #[test]
    fn fills_the_boot_entry() {
        let entry = "title Sekka\noptions init=/nix/store/x/init sekka.root=PARTUUID=@SEKKA_ROOT_PARTUUID@ sekka.esp=PARTUUID=@SEKKA_ESP_PARTUUID@ quiet\n";
        let filled = sample().fill_entry(entry).unwrap();
        assert!(filled.contains(&sample().kernel_arguments()));
        assert!(sample().fill_entry("title Sekka\n").is_err());
        assert!(sample().fill_entry(&format!("{entry}{entry}")).is_err());
    }
}
