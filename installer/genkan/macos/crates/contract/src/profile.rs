// SPDX-License-Identifier: GPL-3.0-or-later
//! Hardware profiles: the exact Mac, firmware and boot stack a release is
//! claimed to work on, and what that claim rests on.
//!
//! A profile is matched on identifiers macOS reports (product type, device
//! class, chip), never on a marketing name, and Sekka's own builds also
//! carry a compiled-in allowlist of boards ([`KNOWN_BOARDS`]): a signed
//! catalog can narrow what the app will install on, but cannot add a Mac the
//! app's source has never described.
use serde::{Deserialize, Serialize};

use crate::{Error, Slug, Text, Timestamp, Version, invalid, schema_tag};

schema_tag!(ProfileSchema, "sekka.mac.profile/1");

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardwareProfile {
    pub schema: ProfileSchema,
    /// `j514s`.
    pub id: Slug,
    /// What a person calls this Mac: "MacBook Pro (14-inch, M3 Pro, Nov 2023)".
    pub name: Text,
    pub machine: MachineMatch,
    pub firmware: FirmwarePolicy,
    pub macos: MacosPolicy,
    /// The platform that boots: kernel, m1n1, U-Boot, Mesa, Sekka itself.
    pub boot_stack: Vec<Component>,
    pub support: Support,
    /// What the claim rests on, observation by observation.
    pub evidence: Vec<Evidence>,
}

/// The identifiers a profile matches, as Asahi's installer reads them from
/// the device tree on macOS.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineMatch {
    /// `Mac15,6`.
    pub product_type: Text,
    /// `j514sap`, the device tree's first compatible string, lowercased.
    pub device_class: Slug,
    /// `0x6030`, as a number.
    pub chip_id: u32,
    /// `t6030`.
    pub soc: Slug,
    /// `apple,j514s`, as Linux sees the same board.
    pub linux_compatible: Text,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FirmwarePolicy {
    /// The OS firmware Asahi's installer gives the new system's stub, and
    /// the drivers were tested with (its `supported_fw`).
    pub os_firmware: Vec<Version>,
    /// System firmware (iBoot) versions this boot stack has run on, as the
    /// number after `iBoot-` or `mBoot-`: `20457.1.29`.
    pub system_firmware: Vec<Text>,
    /// What to do on system firmware not in that list.
    pub untested_system_firmware: Untested,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Untested {
    /// Stop with an explanation. The default for anything that changes how
    /// the Mac boots.
    Block,
    /// Explain, and require the owner to acknowledge it on the review.
    Acknowledge,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacosPolicy {
    /// The oldest macOS the installer runs on.
    pub minimum: Version,
    /// macOS builds with a known problem worth stopping for, or warning of.
    pub known_issues: Vec<KnownIssue>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownIssue {
    /// A macOS version prefix: `27.0` covers 27.0 and 27.0.1.
    pub macos: Text,
    pub blocking: bool,
    pub summary: Text,
    pub reference: Text,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub name: Slug,
    pub version: Text,
    /// Where it comes from: a repository and commit, or a release.
    pub source: Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Support {
    /// Built and checked by components; not qualified on hardware. Offered
    /// only by development catalogs.
    Development,
    /// A frozen candidate under qualification.
    Candidate,
    /// Every required qualification gate passed on this exact profile.
    Qualified,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub claim: Text,
    pub status: EvidenceStatus,
    pub source: Text,
    pub recorded: Timestamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceStatus {
    /// Seen on the reference J514S by Sekka's own developers.
    Observed,
    /// Reported by someone else, on hardware we believe matches.
    Reported,
    /// Expected, not yet seen working.
    Untested,
}

/// Boards the app's source knows how to describe, whatever a catalog says.
/// Adding one is a source change, reviewed with its platform work.
pub const KNOWN_BOARDS: &[(&str, &str, u32)] = &[("Mac15,6", "j514sap", 0x6030)];

/// What macOS reports about this Mac, as far as profiles care.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineFacts {
    pub product_type: String,
    pub device_class: String,
    pub chip_id: u32,
    /// As the device tree has it: `iBoot-…` or `mBoot-…`.
    pub system_firmware: String,
    pub macos: Version,
}

/// Why a profile does not admit this Mac, each with what a person can do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The model isn't one Sekka has been brought up on.
    UnknownMachine { product_type: String, device_class: String },
    /// Same model name, different chip: a different Mac as far as booting
    /// goes.
    ChipMismatch { expected: u32, found: u32 },
    UntestedSystemFirmware { found: String, tested: Vec<String> },
    MacosTooOld { minimum: Version, found: Version },
    KnownIssue { summary: String, reference: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Admission {
    pub refusals: Vec<Refusal>,
    /// Admitted with something the owner must acknowledge first.
    pub acknowledgements: Vec<Refusal>,
}
impl Admission {
    pub fn admitted(&self) -> bool {
        self.refusals.is_empty()
    }
}

/// `20457.1.29` from `mBoot-20457.1.29` or `iBoot-20457.1.29`.
pub fn firmware_number(version: &str) -> &str {
    version
        .strip_prefix("iBoot-")
        .or_else(|| version.strip_prefix("mBoot-"))
        .unwrap_or(version)
}

impl HardwareProfile {
    pub fn check(&self) -> Result<(), Error> {
        if !KNOWN_BOARDS.iter().any(|(p, d, c)| {
            *p == self.machine.product_type.as_str()
                && *d == self.machine.device_class.as_str()
                && *c == self.machine.chip_id
        }) {
            return Err(invalid(format!(
                "profile {} names a board this app doesn't know",
                self.id
            )));
        }
        if self.firmware.os_firmware.is_empty() {
            return Err(invalid("a profile names the OS firmware its drivers need"));
        }
        if self.boot_stack.is_empty() || self.evidence.is_empty() {
            return Err(invalid("a profile names its boot stack and evidence"));
        }
        if self.support == Support::Qualified
            && self
                .evidence
                .iter()
                .any(|e| e.status == EvidenceStatus::Untested)
        {
            return Err(invalid(
                "a qualified profile can't rest on untested claims",
            ));
        }
        Ok(())
    }

    /// Whether this profile admits a Mac, and why not.
    pub fn admit(&self, facts: &MachineFacts) -> Admission {
        let mut refusals = Vec::new();
        let mut acknowledgements = Vec::new();
        let m = &self.machine;
        if facts.product_type != m.product_type.as_str()
            || facts.device_class != m.device_class.as_str()
        {
            refusals.push(Refusal::UnknownMachine {
                product_type: facts.product_type.clone(),
                device_class: facts.device_class.clone(),
            });
        } else if facts.chip_id != m.chip_id {
            refusals.push(Refusal::ChipMismatch {
                expected: m.chip_id,
                found: facts.chip_id,
            });
        }
        let found = firmware_number(&facts.system_firmware);
        if !self
            .firmware
            .system_firmware
            .iter()
            .any(|t| t.as_str() == found)
        {
            let refusal = Refusal::UntestedSystemFirmware {
                found: facts.system_firmware.clone(),
                tested: self
                    .firmware
                    .system_firmware
                    .iter()
                    .map(|t| t.to_string())
                    .collect(),
            };
            match self.firmware.untested_system_firmware {
                Untested::Block => refusals.push(refusal),
                Untested::Acknowledge => acknowledgements.push(refusal),
            }
        }
        if facts.macos < self.macos.minimum {
            refusals.push(Refusal::MacosTooOld {
                minimum: self.macos.minimum.clone(),
                found: facts.macos.clone(),
            });
        }
        let running = facts.macos.to_string();
        for issue in &self.macos.known_issues {
            let prefix = issue.macos.as_str();
            let matches = running == prefix || running.starts_with(&format!("{prefix}."));
            if matches {
                let refusal = Refusal::KnownIssue {
                    summary: issue.summary.to_string(),
                    reference: issue.reference.to_string(),
                };
                if issue.blocking {
                    refusals.push(refusal);
                } else {
                    acknowledgements.push(refusal);
                }
            }
        }
        Admission {
            refusals,
            acknowledgements,
        }
    }
}

/// The J514S profile as this source tree describes it
/// (`profiles/j514s.json`).
pub fn j514s() -> HardwareProfile {
    let profile: HardwareProfile = crate::canonical::from_slice_relaxed(
        include_bytes!("../profiles/j514s.json"),
        crate::limits::PROFILE,
        ProfileSchema::VALUE,
    )
    .expect("profiles/j514s.json is a valid profile");
    profile.check().expect("profiles/j514s.json is consistent");
    profile
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> MachineFacts {
        MachineFacts {
            product_type: "Mac15,6".into(),
            device_class: "j514sap".into(),
            chip_id: 0x6030,
            system_firmware: "mBoot-20457.1.29".into(),
            macos: "27.0.1".parse().unwrap(),
        }
    }

    #[test]
    fn the_reference_mac_is_admitted_with_its_known_issue() {
        let admission = j514s().admit(&facts());
        assert!(admission.admitted(), "{admission:?}");
        assert!(matches!(
            admission.acknowledgements.as_slice(),
            [Refusal::KnownIssue { .. }]
        ));
    }

    #[test]
    fn other_macs_and_firmware_fail_closed() {
        let p = j514s();
        let mut f = facts();
        f.product_type = "Mac15,7".into();
        f.device_class = "j516sap".into();
        assert!(matches!(
            p.admit(&f).refusals.as_slice(),
            [Refusal::UnknownMachine { .. }]
        ));
        let mut f = facts();
        f.chip_id = 0x6031;
        assert!(matches!(
            p.admit(&f).refusals.as_slice(),
            [Refusal::ChipMismatch { .. }]
        ));
        let mut f = facts();
        f.system_firmware = "iBoot-20458.1.1".into();
        assert!(matches!(
            p.admit(&f).refusals.as_slice(),
            [Refusal::UntestedSystemFirmware { .. }]
        ));
        let mut f = facts();
        f.macos = "13.0".parse().unwrap();
        assert!(!p.admit(&f).admitted());
    }

    #[test]
    fn iboot_and_mboot_spell_the_same_firmware() {
        assert_eq!(firmware_number("iBoot-20457.1.29"), "20457.1.29");
        assert_eq!(firmware_number("mBoot-20457.1.29"), "20457.1.29");
    }

    #[test]
    fn profiles_for_unknown_boards_are_refused() {
        let mut p = j514s();
        p.machine.device_class = "j614sap".parse().unwrap();
        assert!(p.check().is_err());
    }

    #[test]
    fn the_profile_is_canonical_after_a_round_trip() {
        let p = j514s();
        let bytes = crate::canonical::to_vec(&p).unwrap();
        let back: HardwareProfile =
            crate::canonical::from_slice(&bytes, crate::limits::PROFILE, ProfileSchema::VALUE).unwrap();
        assert_eq!(back, p);
    }
}
