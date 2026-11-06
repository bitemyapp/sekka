// SPDX-License-Identifier: GPL-3.0-or-later
//! Installation records: the journal's entries.
//!
//! Each record names the installation, its place in the sequence and the
//! digest of the record before it, so a journal is a chain: a torn,
//! reordered or substituted record breaks it. A stage that changes the disk
//! is written as an [`Event::Intent`] before the change and an
//! [`Event::Completed`] (with what was verified) or [`Event::Failed`] after.
//! Records hold identities, sizes and digests, never secrets and never a
//! person's password.
use serde::{Deserialize, Serialize};

use crate::{Digest, Error, Plan, Slug, Text, Timestamp, Uuid, Version, invalid, schema_tag};

schema_tag!(RecordSchema, "sekka.mac.record/1");

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub schema: RecordSchema,
    pub installation: Uuid,
    /// 1 for the first record, then one more each time.
    pub sequence: u64,
    /// The digest of the previous record's canonical bytes; none for the
    /// first.
    pub previous: Option<Digest>,
    pub at: Timestamp,
    pub event: Event,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Event {
    /// The helper accepted a confirmed plan. Always the first record.
    Opened {
        plan: Box<Plan>,
        plan_digest: Digest,
        app: Version,
        helper: Version,
    },
    /// About to start a stage.
    Intent { stage: Stage },
    Completed {
        stage: Stage,
        facts: Vec<Fact>,
        verification: Verification,
    },
    Failed { stage: Stage, failure: Failure },
    /// The installation was abandoned before anything changed.
    Cancelled,
}

/// Stages in the order they happen. Stages through `StartupDisk` run in
/// macOS under the helper; `Enrollment` and `StartupSelection` run in
/// paired Recovery under the Recovery app.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    /// Copy the verified downloads into the helper's private staging and
    /// verify them again. Changes no disk.
    Import,
    /// Shrink the macOS container (only when the plan does).
    Shrink,
    /// Create the stub's APFS container.
    StubPartition,
    /// Install the stub's macOS from Apple's restore image.
    StubSystem,
    /// Create the ESP and root partitions.
    Partitions,
    /// Copy Apple's firmware for this Mac to the ESP.
    Firmware,
    /// Write the ESP's files and the per-install handoff.
    Esp,
    /// Write the root image, and read it back.
    Root,
    /// Place the Recovery app, its configuration and the boot object.
    Recovery,
    /// Make the new system the startup disk, and check it is selected.
    StartupDisk,
    /// In Recovery: allow the new system's startup software (`bputil`).
    Enrollment,
    /// In Recovery: install the boot object (`kmutil configure-boot`) and
    /// select the new system.
    StartupSelection,
}

impl Stage {
    pub const ORDER: [Stage; 12] = [
        Stage::Import,
        Stage::Shrink,
        Stage::StubPartition,
        Stage::StubSystem,
        Stage::Partitions,
        Stage::Firmware,
        Stage::Esp,
        Stage::Root,
        Stage::Recovery,
        Stage::StartupDisk,
        Stage::Enrollment,
        Stage::StartupSelection,
    ];

    /// Whether the stage changes a disk or the boot policy.
    pub fn mutates(self) -> bool {
        self != Stage::Import
    }

    /// Whether repeating it after a failure could make things worse. Only
    /// stages that write files on partitions this installation created, or
    /// ask Apple's tools for a policy change, may be retried; resizing and
    /// partitioning never are.
    pub fn retry_allowed(self) -> bool {
        matches!(
            self,
            Stage::Import | Stage::StartupDisk | Stage::Enrollment | Stage::StartupSelection
        )
    }

    pub fn in_recovery(self) -> bool {
        matches!(self, Stage::Enrollment | Stage::StartupSelection)
    }

    pub fn title(self) -> &'static str {
        match self {
            Stage::Import => "Checking the downloads",
            Stage::Shrink => "Making room on the disk",
            Stage::StubPartition => "Creating Sekka's startup volume",
            Stage::StubSystem => "Installing the startup volume",
            Stage::Partitions => "Creating Sekka's partitions",
            Stage::Firmware => "Copying this Mac's firmware",
            Stage::Esp => "Writing the boot files",
            Stage::Root => "Writing Sekka",
            Stage::Recovery => "Preparing the Recovery step",
            Stage::StartupDisk => "Setting the startup disk",
            Stage::Enrollment => "Allowing Sekka's startup software",
            Stage::StartupSelection => "Installing the startup software",
        }
    }
}

/// One piece of evidence: `esp-partition = 910de5ca-…`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fact {
    pub name: Slug,
    pub value: Text,
}
impl Fact {
    pub fn new(name: &str, value: impl std::fmt::Display) -> Result<Self, Error> {
        Ok(Self {
            name: name.parse()?,
            value: value.to_string().parse()?,
        })
    }
}

/// How far a completed stage's result was verified. These are distinct
/// claims: hashing the source while writing says nothing about what the
/// disk returns, and neither says the Mac boots it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verification {
    /// Nothing to verify, or nothing verified.
    None,
    /// The tool reported success and the result was read back as metadata
    /// (a partition's UUID and size, a container's new size).
    Inspected,
    /// The bytes written were hashed as they were read from the source.
    SourceHashed,
    /// Written, flushed, and synchronized to the disk.
    Flushed,
    /// Read back from the disk and hashed: the installed bytes match.
    ReadBack,
    /// macOS or Recovery reports the selection or policy took effect.
    PolicyReadBack,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    pub code: Slug,
    pub message: Text,
    /// Apple's own message and code, verbatim, when an Apple tool failed
    /// (`-69606`).
    pub apple_diagnostic: Option<Text>,
    pub disk_changed: DiskChanged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiskChanged {
    /// Reinspected: nothing changed.
    No,
    Yes,
    /// It can't be said without looking.
    Unknown,
}

impl Record {
    pub fn bytes(&self) -> Result<Vec<u8>, Error> {
        crate::canonical::to_vec(self)
    }
    pub fn digest(&self) -> Result<Digest, Error> {
        Ok(Digest::of(&self.bytes()?))
    }

    /// Whether `next` may follow `self` in a journal.
    pub fn check_successor(&self, next: &Record) -> Result<(), Error> {
        if next.installation != self.installation {
            return Err(invalid("a record names a different installation"));
        }
        if next.sequence != self.sequence + 1 {
            return Err(invalid(format!(
                "record {} follows record {}",
                next.sequence, self.sequence
            )));
        }
        if next.previous != Some(self.digest()?) {
            return Err(invalid(format!(
                "record {} doesn't follow the record before it",
                next.sequence
            )));
        }
        if matches!(next.event, Event::Opened { .. }) {
            return Err(invalid("only the first record opens an installation"));
        }
        Ok(())
    }
}
