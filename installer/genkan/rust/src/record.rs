// SPDX-License-Identifier: GPL-3.0-or-later
//! The installation record: what Genkan meant to do to a disk and what it
//! verified, as evidence that outlives the window and the helper.
//!
//! A record is a chain of entries, one JSON object per line. Each names the
//! installation, its place in the sequence and the SHA-256 of the line
//! before it, so a torn, reordered, edited or substituted entry breaks the
//! chain, and each must be exactly what Genkan writes for its contents. A
//! stage that changes the disk is written as an intent before the change,
//! and as completed (with what was verified) or failed after it.
//!
//! The live copy is `/run/sekka-install/<installation>.jsonl`, root's only.
//! `/run` is memory: the copy outlives the window and the helper, not a
//! restart. Once the new root filesystem is formatted, its identity read
//! back from its superblock and mounted, the record is mirrored to
//! `/var/log/sekka-install/` on it, and every later entry is written to
//! both, the live copy first. The mirror on the installed system is the
//! durable evidence; it can lag the live copy, never lead it.
//!
//! Entries hold identities, sizes and digests: never a password, a hash of
//! one, a Wi-Fi secret or a private key. Reading a record parses every line
//! and checks it again; a record is never deserialized whole:
//! ```compile_fail
//! let record: calamares_nixos::record::Record = serde_json::from_str("{}").unwrap();
//! ```
//! A record is evidence, not permission: what it allows next
//! ([`Record::next`]) is where checking the real disk starts, never a
//! substitute for it.
use crate::{Filesystem, Firmware, disk::Layout};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    fmt, fs,
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const SCHEMA: &str = "sekka.genkan.record/1";
/// The live copies, one per installation. Memory: lost on restart.
pub const LIVE_DIR: &str = "/run/sekka-install";
/// The mirror, relative to the installed system's root.
pub const TARGET_DIR: &str = "var/log/sekka-install";
pub const MAX_ENTRIES: usize = 64;
pub const MAX_LINE: usize = 4096;
pub const MAX_BYTES: usize = 64 * 1024;
/// A failure's message as kept in the record; the window has all of it.
pub const MAX_MESSAGE: usize = 1024;
const MAX_TEXT: usize = 256;
/// Installing the bootloader is tried at most this often per installation.
pub const BOOTLOADER_ATTEMPTS: u32 = 3;

/// SHA-256, written as 64 lowercase hex digits.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Digest([u8; 32]);
impl Digest {
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }
    pub fn parse(text: &str) -> Result<Self> {
        ensure!(
            text.len() == 64
                && text
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "Not a SHA-256 digest: {text:?}"
        );
        let mut bytes = [0u8; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)?;
        }
        Ok(Self(bytes))
    }
}
impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}
impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({self})")
    }
}

/// The installation's stages, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    /// Checks, configuration, evaluation and the build in the live store.
    /// Changes no disk.
    Preparation,
    /// Partition table, filesystems (and encryption), mounted.
    Storage,
    /// Configuration, account and Wi-Fi settings on the new root.
    Files,
    /// The prepared system copied (or built) into the new root's store.
    Copy,
    /// The system profile, activation and the bootloader.
    Bootloader,
    /// The target read back against this record before handing it over.
    Verification,
    /// Writes flushed and the target unmounted.
    Flush,
}
impl Stage {
    pub const ORDER: [Self; 7] = [
        Self::Preparation,
        Self::Storage,
        Self::Files,
        Self::Copy,
        Self::Bootloader,
        Self::Verification,
        Self::Flush,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Preparation => "preparation",
            Self::Storage => "storage",
            Self::Files => "files",
            Self::Copy => "copy",
            Self::Bootloader => "bootloader",
            Self::Verification => "verification",
            Self::Flush => "flush",
        }
    }
    pub fn parse(name: &str) -> Result<Self> {
        Self::ORDER
            .into_iter()
            .find(|stage| stage.name() == name)
            .with_context(|| format!("Unknown installation stage {name:?}"))
    }
    /// Whether it writes to the target disk.
    pub fn mutates(self) -> bool {
        !matches!(self, Self::Preparation | Self::Verification)
    }
    fn next(self) -> Option<Self> {
        let index = Self::ORDER.iter().position(|stage| *stage == self)?;
        Self::ORDER.get(index + 1).copied()
    }
}

/// How far a completed stage's result was checked. Distinct claims: none
/// of them says the installed system starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verification {
    /// Only the tools' success.
    None,
    /// Read back as metadata: the kernel's partition geometry, identities
    /// from the superblocks, what is mounted where, the system profile.
    Inspected,
    /// Files read back from the target and compared with what was written
    /// (through the page cache, before the final flush).
    ReadBack,
    /// The target store registers exactly the prepared closure, with its
    /// NAR hashes. Its contents were not hashed again.
    Registered,
}
impl Verification {
    const ALL: [Self; 4] = [
        Self::None,
        Self::Inspected,
        Self::ReadBack,
        Self::Registered,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Inspected => "inspected",
            Self::ReadBack => "read-back",
            Self::Registered => "registered",
        }
    }
    fn parse(name: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|v| v.name() == name)
            .with_context(|| format!("Unknown verification {name:?}"))
    }
}

/// What a failed stage left on the disk, as far as Genkan can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiskChanged {
    No,
    Yes,
    /// It can't be said without looking.
    Unknown,
}
impl DiskChanged {
    const ALL: [Self; 3] = [Self::No, Self::Yes, Self::Unknown];
    fn name(self) -> &'static str {
        match self {
            Self::No => "no",
            Self::Yes => "yes",
            Self::Unknown => "unknown",
        }
    }
    fn parse(name: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|v| v.name() == name)
            .with_context(|| format!("Unknown disk change {name:?}"))
    }
}

/// The kinds of value a fact may have. Every fact name has exactly one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Digest,
    /// A filesystem or LUKS UUID.
    Uuid,
    /// A FAT volume serial, `1A2B-3C4D`.
    Serial,
    Number,
    /// `yes` or `no`.
    Flag,
    Firmware,
    Filesystem,
    StorePath,
    Device,
    /// `major:minor`.
    DeviceNumber,
    /// A partition's start and end in MiB, `1-1025`.
    Span,
    /// Free text for people, such as a disk's model. Never compared.
    Text,
}

/// Every fact a record may hold. Anything else is refused when reading.
const FACTS: &[(&str, Kind)] = &[
    // The plan the record is for.
    ("disk", Kind::Device),
    ("disk-identity", Kind::Digest),
    ("disk-number", Kind::DeviceNumber),
    ("disk-bytes", Kind::Number),
    ("disk-model", Kind::Text),
    ("disk-serial", Kind::Text),
    ("disk-wwn", Kind::Text),
    ("firmware", Kind::Firmware),
    ("filesystem", Kind::Filesystem),
    ("encrypted", Kind::Flag),
    ("swap", Kind::Flag),
    ("helper", Kind::Text),
    // The prepared system and the confirmed configuration.
    ("system", Kind::StorePath),
    ("derivation", Kind::StorePath),
    ("deferred", Kind::Flag),
    ("closure", Kind::Digest),
    ("closure-paths", Kind::Number),
    ("closure-bytes", Kind::Number),
    ("configuration", Kind::Digest),
    ("flake", Kind::Digest),
    ("lock", Kind::Digest),
    ("hardware", Kind::Digest),
    // Storage.
    ("boot-mib", Kind::Span),
    ("root-mib", Kind::Span),
    ("swap-mib", Kind::Span),
    ("bios-boot-mib", Kind::Span),
    ("partitions", Kind::Number),
    ("root-uuid", Kind::Uuid),
    ("efi-serial", Kind::Serial),
    ("swap-uuid", Kind::Uuid),
    ("bios-boot-uuid", Kind::Uuid),
    ("luks-root-uuid", Kind::Uuid),
    ("luks-swap-uuid", Kind::Uuid),
    // Files.
    ("account", Kind::Flag),
    ("wifi-profiles", Kind::Number),
    // Bootloader and verification.
    ("attempt", Kind::Number),
    ("profile", Kind::StorePath),
    ("bootloader", Kind::Text),
];

fn kind_of(name: &str) -> Option<Kind> {
    FACTS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, kind)| *kind)
}

fn number(text: &str) -> Option<u64> {
    let value: u64 = text.parse().ok()?;
    (value.to_string() == text).then_some(value)
}

/// A store path as Nix writes it: `/nix/store/<32 hash characters>-<name>`.
fn store_path(text: &str) -> bool {
    let Some(name) = text.strip_prefix("/nix/store/") else {
        return false;
    };
    let bytes = name.as_bytes();
    bytes.len() > 33
        && bytes.len() <= 211
        && bytes[32] == b'-'
        && bytes[..32]
            .iter()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
        && bytes[33..]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"+-._?=".contains(b))
}

fn plain(text: &str) -> bool {
    !text.chars().any(char::is_control)
}

fn check_value(kind: Kind, value: &str) -> Result<()> {
    let valid = match kind {
        Kind::Digest => Digest::parse(value).is_ok(),
        Kind::Uuid => crate::filesystem::check_uuid(value, "ext4").is_ok(),
        Kind::Serial => crate::filesystem::check_uuid(value, "vfat").is_ok(),
        Kind::Number => number(value).is_some(),
        Kind::Flag => matches!(value, "yes" | "no"),
        Kind::Firmware => matches!(value, "uefi" | "bios"),
        Kind::Filesystem => Filesystem::ALL.iter().any(|fs| fs.name() == value),
        Kind::StorePath => store_path(value),
        Kind::Device => {
            value.len() <= 128
                && value.len() > 5
                && value.starts_with("/dev/")
                && !value.contains("..")
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/_.:+-".contains(&b))
        }
        Kind::DeviceNumber => value
            .split_once(':')
            .is_some_and(|(major, minor)| number(major).is_some() && number(minor).is_some()),
        Kind::Span => span(value).is_some(),
        Kind::Text => value.len() <= MAX_TEXT && plain(value),
    };
    ensure!(valid, "Invalid value for a {kind:?} fact: {value:?}");
    Ok(())
}

fn span(text: &str) -> Option<(u64, u64)> {
    let (start, end) = text.split_once('-')?;
    let (start, end) = (number(start)?, number(end)?);
    (start < end).then_some((start, end))
}

/// Free text with its control characters (but `keep`) replaced by spaces,
/// cut to at most `limit` bytes with an ellipsis.
fn sanitize(text: &str, limit: usize, keep: &[char]) -> String {
    let clean: String = text
        .chars()
        .map(|c| {
            if c.is_control() && !keep.contains(&c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    if clean.len() <= limit {
        return clean;
    }
    let mut end = limit - '…'.len_utf8();
    while !clean.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &clean[..end])
}

/// One piece of evidence: `root-uuid = 0f1e2d3c-…`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fact {
    name: &'static str,
    value: String,
}
impl Fact {
    pub fn name(&self) -> &str {
        self.name
    }
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// A stage's evidence, each fact named once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts(Vec<Fact>);
impl Facts {
    /// Add a fact. Free text is cut and cleaned; anything else must already
    /// be what its kind allows.
    pub fn add(&mut self, name: &str, value: impl fmt::Display) -> Result<&mut Self> {
        let (name, kind) = FACTS
            .iter()
            .find(|(n, _)| *n == name)
            .copied()
            .with_context(|| format!("Unknown installation fact {name:?}"))?;
        let mut value = value.to_string();
        if kind == Kind::Text {
            value = sanitize(&value, MAX_TEXT, &[]);
        }
        check_value(kind, &value).with_context(|| format!("Fact {name}"))?;
        ensure!(self.get(name).is_none(), "Fact {name} given twice");
        self.0.push(Fact { name, value });
        Ok(self)
    }
    pub fn flag(&mut self, name: &str, value: bool) -> Result<&mut Self> {
        self.add(name, if value { "yes" } else { "no" })
    }
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|fact| fact.name == name)
            .map(|fact| fact.value.as_str())
    }
    /// A fact the stage must have recorded.
    pub fn require(&self, name: &str) -> Result<&str> {
        self.get(name)
            .with_context(|| format!("The installation record has no {name}"))
    }
    pub fn digest(&self, name: &str) -> Result<Digest> {
        Digest::parse(self.require(name)?)
    }
    pub fn number(&self, name: &str) -> Result<u64> {
        number(self.require(name)?).with_context(|| format!("Invalid {name}"))
    }
    pub fn is(&self, name: &str) -> Result<bool> {
        Ok(self.require(name)? == "yes")
    }
    pub fn span(&self, name: &str) -> Result<Option<(u64, u64)>> {
        self.get(name)
            .map(|text| span(text).with_context(|| format!("Invalid {name}")))
            .transpose()
    }
    pub fn iter(&self) -> impl Iterator<Item = &Fact> {
        self.0.iter()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    fn parse(pairs: Vec<[String; 2]>) -> Result<Self> {
        let mut facts = Self::default();
        for [name, value] in pairs {
            let kind = kind_of(&name).with_context(|| format!("Unknown fact {name:?}"))?;
            check_value(kind, &value).with_context(|| format!("Fact {name}"))?;
            facts.add(&name, value)?;
        }
        Ok(facts)
    }
    fn pairs(&self) -> Vec<[String; 2]> {
        self.0
            .iter()
            .map(|fact| [fact.name.to_string(), fact.value.clone()])
            .collect()
    }
}

/// The partition geometry a record names, as a layout to check the kernel's
/// against.
pub fn layout(facts: &Facts) -> Result<Layout> {
    Ok(Layout {
        boot: facts
            .span("boot-mib")?
            .context("The record has no boot partition")?,
        root: facts
            .span("root-mib")?
            .context("The record has no root partition")?,
        swap: facts.span("swap-mib")?,
        bios_boot: facts.span("bios-boot-mib")?,
        encrypted: facts.is("encrypted")?,
    })
}
pub fn add_layout(facts: &mut Facts, layout: &Layout) -> Result<()> {
    let span = |(start, end): (u64, u64)| format!("{start}-{end}");
    facts.add("boot-mib", span(layout.boot))?;
    facts.add("root-mib", span(layout.root))?;
    if let Some(swap) = layout.swap {
        facts.add("swap-mib", span(swap))?;
    }
    if let Some(boot) = layout.bios_boot {
        facts.add("bios-boot-mib", span(boot))?;
    }
    facts.flag("encrypted", layout.encrypted)?;
    Ok(())
}
pub fn firmware(facts: &Facts) -> Result<Firmware> {
    match facts.require("firmware")? {
        "uefi" => Ok(Firmware::Uefi),
        "bios" => Ok(Firmware::Bios),
        other => bail!("Invalid firmware {other:?}"),
    }
}
pub fn firmware_name(firmware: Firmware) -> &'static str {
    match firmware {
        Firmware::Uefi => "uefi",
        Firmware::Bios => "bios",
    }
}
pub fn filesystem(facts: &Facts) -> Result<Filesystem> {
    let name = facts.require("filesystem")?;
    Filesystem::ALL
        .into_iter()
        .find(|fs| fs.name() == name)
        .with_context(|| format!("Invalid filesystem {name:?}"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The record's first entry: the plan it is for.
    Opened { facts: Facts },
    /// About to start a stage; for one that changes the disk, before it does.
    Intent { stage: Stage, facts: Facts },
    /// The stage finished and its result was checked.
    Completed {
        stage: Stage,
        facts: Facts,
        verification: Verification,
    },
    Failed {
        stage: Stage,
        message: String,
        disk_changed: DiskChanged,
    },
    /// The typed confirmation matched, for this configuration.
    Confirmed { facts: Facts },
    /// The window withdrew the request before anything was written.
    Cancelled,
    /// Genkan stopped instead of going on: a check before a stage (or
    /// before repeating one) didn't pass. Nothing follows it.
    Refused { message: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub installation: String,
    pub sequence: u64,
    pub previous: Option<Digest>,
    /// Seconds since 1970, by the live system's clock.
    pub at: u64,
    pub event: Event,
}

/// An entry as it is written: untrusted until `Entry::parse` checks it.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    schema: String,
    installation: String,
    sequence: u64,
    previous: Option<String>,
    at: u64,
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stage: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    facts: Vec<[String; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    verification: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    disk_changed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

fn check_message(message: &str) -> Result<()> {
    ensure!(
        !message.is_empty()
            && message.len() <= MAX_MESSAGE
            && !message.chars().any(|c| c.is_control() && c != '\n'),
        "Invalid message in the installation record"
    );
    Ok(())
}
fn message(text: &str) -> String {
    let text = sanitize(text.trim(), MAX_MESSAGE, &['\n']);
    if text.is_empty() {
        "No message".into()
    } else {
        text
    }
}

impl Entry {
    fn wire(&self) -> Wire {
        let mut wire = Wire {
            schema: SCHEMA.into(),
            installation: self.installation.clone(),
            sequence: self.sequence,
            previous: self.previous.map(|d| d.to_string()),
            at: self.at,
            kind: String::new(),
            stage: None,
            facts: Vec::new(),
            verification: None,
            disk_changed: None,
            message: None,
        };
        let (kind, stage) = match &self.event {
            Event::Opened { facts } => {
                wire.facts = facts.pairs();
                ("opened", None)
            }
            Event::Intent { stage, facts } => {
                wire.facts = facts.pairs();
                ("intent", Some(*stage))
            }
            Event::Completed {
                stage,
                facts,
                verification,
            } => {
                wire.facts = facts.pairs();
                wire.verification = Some(verification.name().into());
                ("completed", Some(*stage))
            }
            Event::Failed {
                stage,
                message,
                disk_changed,
            } => {
                wire.message = Some(message.clone());
                wire.disk_changed = Some(disk_changed.name().into());
                ("failed", Some(*stage))
            }
            Event::Confirmed { facts } => {
                wire.facts = facts.pairs();
                ("confirmed", None)
            }
            Event::Cancelled => ("cancelled", None),
            Event::Refused { message } => {
                wire.message = Some(message.clone());
                ("refused", None)
            }
        };
        wire.kind = kind.into();
        wire.stage = stage.map(|s| s.name().into());
        wire
    }

    /// The entry's line, without its newline. Its digest is the chain's.
    pub fn line(&self) -> Vec<u8> {
        serde_json::to_vec(&self.wire()).expect("an entry serializes")
    }

    /// Parse one line and check every field. The line must be exactly what
    /// Genkan writes for the entry it describes.
    pub fn parse(line: &[u8]) -> Result<Self> {
        ensure!(line.len() <= MAX_LINE, "Entry is longer than Genkan writes");
        let wire: Wire =
            serde_json::from_slice(line).map_err(|_| anyhow::anyhow!("Unreadable entry"))?;
        ensure!(
            wire.schema == SCHEMA,
            "Unsupported record version {:?}",
            wire.schema
        );
        crate::filesystem::check_uuid(&wire.installation, "ext4")
            .context("Invalid installation identity")?;
        ensure!(wire.sequence >= 1, "Entries are numbered from 1");
        let previous = wire.previous.as_deref().map(Digest::parse).transpose()?;
        let stage = wire.stage.as_deref().map(Stage::parse).transpose()?;
        let verification = wire
            .verification
            .as_deref()
            .map(Verification::parse)
            .transpose()?;
        let disk_changed = wire
            .disk_changed
            .as_deref()
            .map(DiskChanged::parse)
            .transpose()?;
        if let Some(message) = &wire.message {
            check_message(message)?;
        }
        let facts = Facts::parse(wire.facts.clone())?;
        // Which fields each kind has: anything more or less is refused.
        let shape = (
            stage.is_some(),
            !facts.is_empty(),
            verification.is_some(),
            disk_changed.is_some(),
            wire.message.is_some(),
        );
        let event = match wire.kind.as_str() {
            "opened" if shape == (false, true, false, false, false) => Event::Opened { facts },
            "confirmed" if shape == (false, true, false, false, false) => {
                Event::Confirmed { facts }
            }
            "intent" if matches!(shape, (true, _, false, false, false)) => Event::Intent {
                stage: stage.unwrap(),
                facts,
            },
            "completed" if matches!(shape, (true, _, true, false, false)) => Event::Completed {
                stage: stage.unwrap(),
                facts,
                verification: verification.unwrap(),
            },
            "failed" if shape == (true, false, false, true, true) => Event::Failed {
                stage: stage.unwrap(),
                message: wire.message.clone().unwrap(),
                disk_changed: disk_changed.unwrap(),
            },
            "cancelled" if shape == (false, false, false, false, false) => Event::Cancelled,
            "refused" if shape == (false, false, false, false, true) => Event::Refused {
                message: wire.message.clone().unwrap(),
            },
            kind => bail!("Unexpected {kind:?} entry"),
        };
        let entry = Self {
            installation: wire.installation,
            sequence: wire.sequence,
            previous,
            at: wire.at,
            event,
        };
        ensure!(
            entry.line() == line,
            "Entry is not in the form Genkan writes"
        );
        Ok(entry)
    }
}

/// Where an installation stands, read from its record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Opened; preparation hasn't started.
    Opened,
    /// Preparation started and its result isn't recorded.
    Preparing,
    Prepared,
    /// The confirmation matched; nothing is written yet.
    Confirmed,
    /// A stage started and its result isn't recorded. For one that writes
    /// to the disk, how far it got is unknown.
    Started(Stage),
    /// The stage finished and was checked; the next hasn't started.
    Done(Stage),
    Failed {
        stage: Stage,
        disk_changed: DiskChanged,
    },
    Cancelled,
    Refused,
}

/// What may safely be offered next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// Nothing was written: review again, or close.
    Review,
    /// Install the bootloader again on the same, rechecked target.
    RetryBootloader,
    /// The disk changed and Genkan won't continue on it by itself: install
    /// from the start, which erases it again after a new review.
    StartOver,
    /// The system was handed over: shut down and start the installed disk.
    Restart,
}

/// The state, and what the entries say beyond it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Progress {
    state: State,
    /// How often the bootloader was started.
    attempts: u32,
    /// Whether a stage that writes to the disk was started.
    mutated: bool,
}

/// Read a chain's meaning, refusing entries that contradict each other or
/// the order Genkan works in. The chain itself is checked by the caller.
fn observe(entries: &[Entry]) -> Result<Progress> {
    let first = entries
        .first()
        .context("A record has at least its first entry")?;
    ensure!(
        matches!(first.event, Event::Opened { .. }),
        "A record must start by naming its plan"
    );
    let mut progress = Progress {
        state: State::Opened,
        attempts: 0,
        mutated: false,
    };
    for entry in &entries[1..] {
        let at = entry.sequence;
        let state = &progress.state;
        let next = match &entry.event {
            Event::Opened { .. } => bail!("Entry {at}: a record names its plan once"),
            Event::Confirmed { .. } => {
                ensure!(
                    *state == State::Prepared,
                    "Entry {at}: confirmed before preparation finished"
                );
                State::Confirmed
            }
            Event::Cancelled => {
                ensure!(
                    matches!(state, State::Opened | State::Preparing | State::Prepared),
                    "Entry {at}: only an installation that changed nothing can be cancelled"
                );
                State::Cancelled
            }
            Event::Refused { .. } => {
                ensure!(
                    !matches!(
                        state,
                        State::Preparing
                            | State::Started(_)
                            | State::Cancelled
                            | State::Refused
                            | State::Done(Stage::Flush)
                    ) && !matches!(state, State::Failed { stage, .. } if *stage != Stage::Bootloader),
                    "Entry {at}: nothing can be refused here"
                );
                State::Refused
            }
            Event::Intent { stage, .. } => {
                let allowed = match state {
                    State::Opened => *stage == Stage::Preparation,
                    State::Confirmed => *stage == Stage::Storage,
                    State::Done(done) => done.next() == Some(*stage),
                    State::Failed {
                        stage: Stage::Bootloader,
                        ..
                    } => *stage == Stage::Bootloader && progress.attempts < BOOTLOADER_ATTEMPTS,
                    _ => false,
                };
                ensure!(allowed, "Entry {at}: {} can't start here", stage.name());
                if *stage == Stage::Bootloader {
                    progress.attempts += 1;
                }
                progress.mutated |= stage.mutates();
                if *stage == Stage::Preparation {
                    State::Preparing
                } else {
                    State::Started(*stage)
                }
            }
            Event::Completed {
                stage,
                verification,
                ..
            } => {
                let open = match state {
                    State::Preparing => Some(Stage::Preparation),
                    State::Started(open) => Some(*open),
                    _ => None,
                };
                ensure!(
                    open == Some(*stage),
                    "Entry {at}: {} finished without starting",
                    stage.name()
                );
                // Completion is recorded only after its result was checked.
                ensure!(
                    *verification != Verification::None,
                    "Entry {at}: {} finished unchecked",
                    stage.name()
                );
                if *stage == Stage::Preparation {
                    State::Prepared
                } else {
                    State::Done(*stage)
                }
            }
            Event::Failed {
                stage,
                disk_changed,
                ..
            } => {
                let open = match state {
                    State::Preparing => Some(Stage::Preparation),
                    State::Started(open) => Some(*open),
                    _ => None,
                };
                ensure!(
                    open == Some(*stage),
                    "Entry {at}: {} failed without starting",
                    stage.name()
                );
                ensure!(
                    *stage != Stage::Preparation || *disk_changed == DiskChanged::No,
                    "Entry {at}: preparation can't change a disk"
                );
                State::Failed {
                    stage: *stage,
                    disk_changed: *disk_changed,
                }
            }
        };
        progress.state = next;
    }
    Ok(progress)
}

/// A record read and checked: its chain, every entry, and their order.
/// Built only by parsing (`Record::parse`) or by a `Journal` writing it.
#[derive(Clone, Debug)]
pub struct Record {
    entries: Vec<Entry>,
    lines: Vec<Vec<u8>>,
    progress: Progress,
}
impl Record {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(!bytes.is_empty(), "The installation record is empty");
        ensure!(
            bytes.len() <= MAX_BYTES,
            "The installation record is larger than Genkan writes"
        );
        ensure!(
            bytes.ends_with(b"\n"),
            "The installation record ends in the middle of an entry: its last write was cut short"
        );
        let mut entries: Vec<Entry> = Vec::new();
        let mut lines: Vec<Vec<u8>> = Vec::new();
        for line in bytes[..bytes.len() - 1].split(|b| *b == b'\n') {
            let number = lines.len() + 1;
            ensure!(
                number <= MAX_ENTRIES,
                "The installation record has more entries than Genkan writes"
            );
            let entry = Entry::parse(line)
                .with_context(|| format!("Entry {number} of the installation record"))?;
            ensure!(
                entry.sequence == number as u64,
                "Entry {number} of the installation record is numbered {}",
                entry.sequence
            );
            match (lines.last(), entries.first()) {
                (Some(previous), Some(first)) => {
                    ensure!(
                        entry.installation == first.installation,
                        "Entry {number} belongs to another installation"
                    );
                    ensure!(
                        entry.previous == Some(Digest::of(previous)),
                        "Entry {number} doesn't follow the entry before it"
                    );
                }
                _ => ensure!(
                    entry.previous.is_none(),
                    "The first entry names an entry before it"
                ),
            }
            lines.push(line.to_vec());
            entries.push(entry);
        }
        let progress = observe(&entries)?;
        Ok(Self {
            entries,
            lines,
            progress,
        })
    }

    /// Read a copy Genkan wrote: root's own regular file, at most
    /// `MAX_BYTES`. Any other owner, a link, or a file others may write is
    /// refused.
    pub fn read(path: &Path) -> Result<Self> {
        let bytes = read_private(path)?;
        Self::parse(&bytes).with_context(|| format!("Reading {}", path.display()))
    }

    pub fn installation(&self) -> &str {
        &self.entries[0].installation
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    pub fn state(&self) -> &State {
        &self.progress.state
    }
    /// How often installing the bootloader was started.
    pub fn attempts(&self) -> u32 {
        self.progress.attempts
    }
    /// Whether anything that writes to the disk was started.
    pub fn changed_disk(&self) -> bool {
        self.progress.mutated
    }
    /// The digest of the last entry: what a retry request must name.
    pub fn head(&self) -> Digest {
        Digest::of(self.lines.last().expect("a record has an entry"))
    }
    pub fn bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for line in &self.lines {
            bytes.extend_from_slice(line);
            bytes.push(b'\n');
        }
        bytes
    }
    /// The plan the record is for.
    pub fn opened(&self) -> &Facts {
        match &self.entries[0].event {
            Event::Opened { facts } => facts,
            _ => unreachable!("checked when parsed"),
        }
    }
    pub fn confirmed(&self) -> Option<&Facts> {
        self.entries.iter().find_map(|entry| match &entry.event {
            Event::Confirmed { facts } => Some(facts),
            _ => None,
        })
    }
    /// The latest completion of `stage`, with how it was checked.
    pub fn completed(&self, stage: Stage) -> Option<(&Facts, Verification)> {
        self.entries
            .iter()
            .rev()
            .find_map(|entry| match &entry.event {
                Event::Completed {
                    stage: s,
                    facts,
                    verification,
                } if *s == stage => Some((facts, *verification)),
                _ => None,
            })
    }

    /// What may safely happen next. Each is a starting point for looking at
    /// the real disk.
    pub fn next(&self) -> Vec<Next> {
        match &self.progress.state {
            State::Opened
            | State::Preparing
            | State::Prepared
            | State::Confirmed
            | State::Cancelled => vec![Next::Review],
            State::Refused if !self.progress.mutated => vec![Next::Review],
            State::Failed {
                disk_changed: DiskChanged::No,
                ..
            } if !self.progress.mutated => vec![Next::Review],
            State::Failed {
                stage: Stage::Bootloader,
                ..
            } if self.retry_eligible() => vec![Next::RetryBootloader, Next::StartOver],
            State::Started(Stage::Flush) | State::Done(Stage::Verification | Stage::Flush) => {
                vec![Next::Restart]
            }
            _ => vec![Next::StartOver],
        }
    }

    /// The bootloader may be tried again only after the stages before it
    /// completed with their checks, and only a few times.
    fn retry_eligible(&self) -> bool {
        self.progress.attempts < BOOTLOADER_ATTEMPTS
            && self.confirmed().is_some()
            && [
                (Stage::Storage, Verification::Inspected),
                (Stage::Files, Verification::ReadBack),
                (Stage::Copy, Verification::Registered),
            ]
            .into_iter()
            .all(|(stage, needed)| {
                self.completed(stage)
                    .is_some_and(|(_, verification)| verification == needed)
            })
    }

    /// The offer to make when the bootloader may be tried again.
    pub fn retry_offer(&self) -> Option<RetryOffer> {
        self.next()
            .contains(&Next::RetryBootloader)
            .then(|| RetryOffer {
                stage: Stage::Bootloader.name().into(),
                record: self.head().to_string(),
                attempt: self.progress.attempts + 1,
                attempts: BOOTLOADER_ATTEMPTS,
            })
    }

    fn disk_name(&self) -> String {
        let facts = self.opened();
        match (facts.get("disk-model"), facts.get("disk")) {
            (Some(model), Some(path)) if !model.is_empty() => format!("{model} ({path})"),
            (_, Some(path)) => path.to_string(),
            _ => "the selected disk".into(),
        }
    }

    /// What the record shows was done to the disk, in a sentence or two.
    pub fn changed(&self) -> String {
        let disk = self.disk_name();
        if !self.progress.mutated {
            return format!("Nothing was written to {disk}.");
        }
        let done = |stage| self.completed(stage).is_some();
        if done(Stage::Flush) {
            format!("The system is installed on {disk}, its writes flushed.")
        } else if done(Stage::Verification) {
            format!(
                "Everything was written to {disk} and checked; flushing the writes and unmounting it didn't finish."
            )
        } else if done(Stage::Bootloader) {
            format!(
                "Everything was written to {disk}, but the final check didn't pass, so it may not start."
            )
        } else if done(Stage::Copy) {
            format!(
                "{disk} was erased, and the system was copied to it and checked. Installing the bootloader didn't finish, so the disk can't start yet."
            )
        } else if done(Stage::Files) {
            format!(
                "{disk} was erased and the system's configuration written to it. Copying the system didn't finish, so it can't start."
            )
        } else if done(Stage::Storage) {
            format!(
                "{disk} was erased and its new filesystems were created and checked. Writing the system's configuration didn't finish."
            )
        } else {
            format!(
                "Genkan began erasing {disk}. Its partition table and filesystems may be partly written: treat everything that was on it as lost."
            )
        }
    }

    /// The next safe action, in a sentence or two. `offered` says whether
    /// the bootloader may be tried again from this window.
    pub fn next_step(&self, offered: bool) -> String {
        let next = self.next();
        if offered && next.contains(&Next::RetryBootloader) {
            "Try the bootloader again. Genkan first checks that this is the same disk, with the same filesystems and the same copied system, and changes nothing else on it.".into()
        } else if next.contains(&Next::Restart) {
            "Shut down, remove the installation media, then start the installed disk.".into()
        } else if next.contains(&Next::Review) {
            "The disk is as it was. Close the installer and open it again to review your choices and install.".into()
        } else {
            "Genkan won't continue on this disk by itself. Copy the details, then close the installer and open it again to install from the start, which erases the disk again.".into()
        }
    }

    /// The record redacted for support: the disk's serial number and WWN
    /// shortened to digests, and `personal` names (the account's, the
    /// computer's) in messages replaced. Its lines no longer match their
    /// digests; the full record stays with root.
    pub fn support(&self, personal: &[String]) -> String {
        let id = self.installation();
        let mut text = format!(
            "Genkan installation record {id}, redacted for support.\nFull record: {LIVE_DIR}/{id}.jsonl in the live session, /{TARGET_DIR}/{id}.jsonl on the installed disk.\n"
        );
        for entry in &self.entries {
            let mut wire = entry.wire();
            for [name, value] in &mut wire.facts {
                if matches!(name.as_str(), "disk-serial" | "disk-wwn") && !value.is_empty() {
                    *value = format!("sha256:{}", &Digest::of(value.as_bytes()).to_string()[..12]);
                }
            }
            if let Some(message) = &mut wire.message {
                *message = redact(message, personal);
            }
            text.push_str(&serde_json::to_string(&wire).expect("an entry serializes"));
            text.push('\n');
        }
        text
    }

    /// For the window: what changed, what to do next, and the offer when
    /// the bootloader may be tried again.
    pub fn report(&self, message: &str, offer: bool, personal: &[String]) -> Report {
        let retry = if offer { self.retry_offer() } else { None };
        Report {
            message: message.into(),
            changed: self.changed(),
            next: self.next_step(retry.is_some()),
            retry,
            support: self.support(personal),
        }
    }

    /// Choose between the live copy and the installed system's mirror. The
    /// mirror is written after the live copy, so it may lag it: both must
    /// hold the same entries as far as the shorter goes, and the mirror
    /// may not be longer. Otherwise they conflict, and neither is trusted.
    pub fn reconcile(live: Option<Self>, mirror: Option<Self>) -> Result<Self> {
        match (live, mirror) {
            (Some(live), Some(mirror)) => {
                ensure!(
                    mirror.lines.len() <= live.lines.len()
                        && live.lines[..mirror.lines.len()] == mirror.lines[..],
                    "The installed system's copy of the installation record disagrees with the live copy"
                );
                Ok(live)
            }
            (Some(record), None) | (None, Some(record)) => Ok(record),
            (None, None) => bail!("No installation record was found"),
        }
    }
}

/// Personal names (three characters or more) replaced in free text.
fn redact(text: &str, personal: &[String]) -> String {
    let mut text = text.to_string();
    let mut names: Vec<&String> = personal.iter().filter(|n| n.chars().count() >= 3).collect();
    // Longer first: a full name before the username inside it.
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    for name in names {
        text = text.replace(name.as_str(), "[redacted]");
    }
    text
}

/// What the window shows after a failure once the disk was confirmed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// The failure, in full.
    pub message: String,
    /// What the record shows changed on the disk.
    pub changed: String,
    /// The next safe action.
    pub next: String,
    /// Present when the bootloader may be tried again.
    pub retry: Option<RetryOffer>,
    /// The record, redacted for support.
    pub support: String,
}

/// The helper waits for the window's answer to this. A retry names the
/// record's last entry as offered, so an answer can't apply to any other.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryOffer {
    pub stage: String,
    pub record: String,
    /// This would be attempt `attempt` of at most `attempts`.
    pub attempt: u32,
    pub attempts: u32,
}

/// The file operations an atomic write needs. Tests make any of them fail.
pub trait Io {
    /// Create `path` (mode 0600), failing if it exists.
    fn create(&mut self, path: &Path) -> io::Result<Box<dyn Output>>;
    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()>;
    fn sync_dir(&mut self, dir: &Path) -> io::Result<()>;
    fn remove(&mut self, path: &Path) -> io::Result<()>;
    /// Read a file this journal wrote, refusing one it can't have.
    fn read(&mut self, path: &Path) -> io::Result<Vec<u8>>;
    /// Make `dir`, or check the one there: root's private directory (0700)
    /// when `private`, otherwise an ordinary one (0755).
    fn dir(&mut self, dir: &Path, private: bool) -> io::Result<()>;
}
pub trait Output: Write {
    fn sync(&mut self) -> io::Result<()>;
}

/// The real filesystem.
pub struct Files;
impl Output for fs::File {
    fn sync(&mut self) -> io::Result<()> {
        self.sync_all()
    }
}
impl Io for Files {
    fn create(&mut self, path: &Path) -> io::Result<Box<dyn Output>> {
        Ok(Box::new(
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(path)?,
        ))
    }
    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }
    fn sync_dir(&mut self, dir: &Path) -> io::Result<()> {
        fs::File::open(dir)?.sync_all()
    }
    fn remove(&mut self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }
    fn read(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        read_private(path).map_err(|error| io::Error::other(format!("{error:#}")))
    }
    fn dir(&mut self, dir: &Path, private: bool) -> io::Result<()> {
        let mode = if private { 0o700 } else { 0o755 };
        match fs::DirBuilder::new().mode(mode).create(dir) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        let meta = fs::symlink_metadata(dir)?;
        let owned = meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0;
        if !meta.is_dir() || (private && !owned) {
            return Err(io::Error::other(format!(
                "{} is not Genkan's {}directory",
                dir.display(),
                if private { "private " } else { "" }
            )));
        }
        Ok(())
    }
}

fn read_private(path: &Path) -> Result<Vec<u8>> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .with_context(|| format!("Opening {}", path.display()))?;
    let meta = file.metadata()?;
    ensure!(
        meta.is_file() && meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0,
        "{} is not a record Genkan wrote",
        path.display()
    );
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_BYTES,
        "{} is larger than Genkan writes",
        path.display()
    );
    Ok(bytes)
}

/// Replace `path` with `bytes` so that, after a crash at any point, it holds
/// either its previous contents or all of the new ones: write a temporary
/// file beside it, flush it to the disk, rename it over `path` and flush the
/// directory. A failure before the rename leaves `path` as it was.
pub fn write_atomic(io: &mut dyn Io, path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().context("A record has a directory")?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .context("A record has a name")?;
    let temporary = dir.join(format!(".{name}.tmp"));
    let mut file = match io.create(&temporary) {
        // Left by an earlier write that stopped before its rename.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            io.remove(&temporary)?;
            io.create(&temporary)?
        }
        result => result?,
    };
    let written = file.write_all(bytes).and_then(|()| file.sync());
    drop(file);
    if let Err(error) = written.and_then(|()| io.rename(&temporary, path)) {
        let _ = io.remove(&temporary);
        return Err(error).with_context(|| format!("Writing {}", path.display()));
    }
    io.sync_dir(dir)
        .with_context(|| format!("Flushing {}", dir.display()))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The record being written by this installation. Every entry is on the
/// disk (the live copy, then the mirror) before `Journal`'s methods return,
/// so an intent precedes the change it announces. After a failed write,
/// nothing more is written: the copies' state is no longer known.
pub struct Journal {
    record: Record,
    live: PathBuf,
    mirror: Option<PathBuf>,
    io: Box<dyn Io + Send>,
    broken: Option<String>,
    personal: Vec<String>,
}
impl Journal {
    /// Open a record in `dir` (root's private directory) for the plan
    /// `facts` describe.
    pub fn open(dir: &Path, facts: Facts, personal: Vec<String>) -> Result<Self> {
        let id = crate::filesystem::uuid_v4(crate::filesystem::random()?);
        Self::open_with(Box::new(Files), dir, &id, facts, personal)
    }
    pub fn open_with(
        mut io: Box<dyn Io + Send>,
        dir: &Path,
        id: &str,
        facts: Facts,
        personal: Vec<String>,
    ) -> Result<Self> {
        crate::filesystem::check_uuid(id, "ext4")?;
        io.dir(dir, true)
            .with_context(|| format!("Creating {}", dir.display()))?;
        let entry = Entry {
            installation: id.into(),
            sequence: 1,
            previous: None,
            at: now(),
            event: Event::Opened { facts },
        };
        let line = entry.line();
        ensure!(
            line.len() <= MAX_LINE,
            "The plan's record entry is too long"
        );
        let live = dir.join(format!("{id}.jsonl"));
        let mut bytes = line.clone();
        bytes.push(b'\n');
        write_atomic(io.as_mut(), &live, &bytes)?;
        Ok(Self {
            record: Record {
                progress: observe(std::slice::from_ref(&entry))?,
                entries: vec![entry],
                lines: vec![line],
            },
            live,
            mirror: None,
            io,
            broken: None,
            personal,
        })
    }

    pub fn record(&self) -> &Record {
        &self.record
    }
    pub fn live_path(&self) -> &Path {
        &self.live
    }
    pub fn mirror_path(&self) -> Option<&Path> {
        self.mirror.as_deref()
    }
    /// Names to keep out of support reports.
    pub fn personal(&self) -> &[String] {
        &self.personal
    }

    fn append(&mut self, event: Event) -> Result<()> {
        if let Some(reason) = &self.broken {
            bail!(
                "The installation record can't be written ({reason}); Genkan stops rather than go on without it"
            );
        }
        let entry = Entry {
            installation: self.record.installation().into(),
            sequence: self.record.entries.len() as u64 + 1,
            previous: Some(self.record.head()),
            at: now(),
            event,
        };
        let line = entry.line();
        ensure!(
            line.len() <= MAX_LINE,
            "An installation record entry is too long"
        );
        let mut next = self.record.clone();
        next.entries.push(entry);
        next.lines.push(line);
        ensure!(
            next.entries.len() <= MAX_ENTRIES && next.bytes().len() <= MAX_BYTES,
            "The installation record is full"
        );
        // Never write what couldn't be read back.
        next.progress = observe(&next.entries)?;
        let bytes = next.bytes();
        let mut targets = vec![self.live.clone()];
        targets.extend(self.mirror.clone());
        for path in targets {
            if let Err(error) = write_atomic(self.io.as_mut(), &path, &bytes) {
                self.broken = Some(format!("{error:#}"));
                return Err(error.context("Writing the installation record"));
            }
        }
        self.record = next;
        Ok(())
    }

    pub fn intent(&mut self, stage: Stage, facts: Facts) -> Result<()> {
        self.append(Event::Intent { stage, facts })
    }
    pub fn completed(
        &mut self,
        stage: Stage,
        facts: Facts,
        verification: Verification,
    ) -> Result<()> {
        self.append(Event::Completed {
            stage,
            facts,
            verification,
        })
    }
    pub fn failed(
        &mut self,
        stage: Stage,
        error: &anyhow::Error,
        disk_changed: DiskChanged,
    ) -> Result<()> {
        self.append(Event::Failed {
            stage,
            message: message(&format!("{error:#}")),
            disk_changed,
        })
    }
    pub fn confirmed(&mut self, facts: Facts) -> Result<()> {
        self.append(Event::Confirmed { facts })
    }
    pub fn cancelled(&mut self) -> Result<()> {
        self.append(Event::Cancelled)
    }
    pub fn refused(&mut self, reason: &str) -> Result<()> {
        self.append(Event::Refused {
            message: message(reason),
        })
    }

    /// Start writing the record to the installed system's root too: only
    /// once that filesystem was created with the chosen identity, checked
    /// and mounted at `root`.
    pub fn mirror_to(&mut self, root: &Path) -> Result<()> {
        if let Some(reason) = &self.broken {
            bail!("The installation record can't be written ({reason})");
        }
        let mut dir = root.to_path_buf();
        let parts: Vec<&str> = TARGET_DIR.split('/').collect();
        for (index, part) in parts.iter().enumerate() {
            dir.push(part);
            // var and var/log as NixOS has them; the record's own is root's.
            self.io
                .dir(&dir, index + 1 == parts.len())
                .with_context(|| format!("Creating {}", dir.display()))?;
        }
        let path = dir.join(format!("{}.jsonl", self.record.installation()));
        if let Err(error) = write_atomic(self.io.as_mut(), &path, &self.record.bytes()) {
            self.broken = Some(format!("{error:#}"));
            return Err(error.context("Copying the installation record to the new disk"));
        }
        self.mirror = Some(path);
        Ok(())
    }
    /// The target is about to be unmounted: later entries go to the live
    /// copy alone.
    pub fn detach_mirror(&mut self) {
        self.mirror = None;
    }

    /// Read the copies back and check they are exactly what this journal
    /// wrote. A copy changed by anything else is evidence: it is never
    /// written over, and nothing more is written.
    pub fn reread(&mut self) -> Result<Record> {
        let expected = self.record.bytes();
        let mut paths = vec![self.live.clone()];
        paths.extend(self.mirror.clone());
        for path in paths {
            let bytes = self.io.read(&path);
            let matches = bytes.as_ref().is_ok_and(|bytes| *bytes == expected);
            if !matches {
                let reason = match bytes {
                    Ok(_) => format!("{} changed outside Genkan", path.display()),
                    Err(error) => format!("{} can't be read back: {error}", path.display()),
                };
                self.broken = Some(reason.clone());
                bail!("The installation record no longer matches what Genkan wrote: {reason}");
            }
        }
        Record::parse(&expected)
    }

    /// For the window, from what is on the disk so far.
    pub fn report(&self, message: &str, offer: bool) -> Report {
        self.record
            .report(message, offer && self.broken.is_none(), &self.personal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    const ID: &str = "0f1e2d3c-4b5a-4978-8695-a4b3c2d1e0f9";
    const OTHER: &str = "11111111-2222-4333-8444-555555555555";
    const SYSTEM: &str = "/nix/store/0123456789abcdfghijklmnpqrsvwxyz-nixos-system-test-26.11";

    /// A filesystem in memory whose operations can be made to fail.
    #[derive(Clone, Default)]
    struct Memory(Arc<Mutex<Disk>>);
    #[derive(Default)]
    struct Disk {
        files: HashMap<PathBuf, Vec<u8>>,
        dirs: Vec<PathBuf>,
        /// Fail the nth write call (counting from 0), then write nothing.
        fail_write: Option<(usize, io::ErrorKind)>,
        /// Accept only this many bytes per write call: a short write.
        short: Option<usize>,
        fail_sync: bool,
        fail_rename: bool,
        fail_dir_sync: bool,
        writes: usize,
    }
    struct Open(Arc<Mutex<Disk>>, PathBuf);
    impl Write for Open {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut disk = self.0.lock().unwrap();
            let call = disk.writes;
            disk.writes += 1;
            if let Some((n, kind)) = disk.fail_write
                && n == call
            {
                return Err(io::Error::from(kind));
            }
            let n = disk.short.map_or(buf.len(), |limit| limit.min(buf.len()));
            disk.files
                .get_mut(&self.1)
                .unwrap()
                .extend_from_slice(&buf[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Output for Open {
        fn sync(&mut self) -> io::Result<()> {
            if self.0.lock().unwrap().fail_sync {
                return Err(io::Error::other("injected fsync failure"));
            }
            Ok(())
        }
    }
    impl Io for Memory {
        fn create(&mut self, path: &Path) -> io::Result<Box<dyn Output>> {
            let mut disk = self.0.lock().unwrap();
            if disk.files.contains_key(path) {
                return Err(io::ErrorKind::AlreadyExists.into());
            }
            disk.files.insert(path.into(), Vec::new());
            Ok(Box::new(Open(self.0.clone(), path.into())))
        }
        fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
            let mut disk = self.0.lock().unwrap();
            if disk.fail_rename {
                return Err(io::Error::other("injected rename failure"));
            }
            let bytes = disk.files.remove(from).ok_or(io::ErrorKind::NotFound)?;
            disk.files.insert(to.into(), bytes);
            Ok(())
        }
        fn sync_dir(&mut self, _: &Path) -> io::Result<()> {
            if self.0.lock().unwrap().fail_dir_sync {
                return Err(io::Error::other("injected directory fsync failure"));
            }
            Ok(())
        }
        fn remove(&mut self, path: &Path) -> io::Result<()> {
            self.0.lock().unwrap().files.remove(path);
            Ok(())
        }
        fn read(&mut self, path: &Path) -> io::Result<Vec<u8>> {
            self.0
                .lock()
                .unwrap()
                .files
                .get(path)
                .cloned()
                .ok_or(io::ErrorKind::NotFound.into())
        }
        fn dir(&mut self, dir: &Path, _: bool) -> io::Result<()> {
            self.0.lock().unwrap().dirs.push(dir.into());
            Ok(())
        }
    }
    impl Memory {
        fn file(&self, path: &Path) -> Option<Vec<u8>> {
            self.0.lock().unwrap().files.get(path).cloned()
        }
        fn set(&self, path: &Path, bytes: Vec<u8>) {
            self.0.lock().unwrap().files.insert(path.into(), bytes);
        }
        fn with(&self, change: impl FnOnce(&mut Disk)) {
            change(&mut self.0.lock().unwrap());
        }
    }

    fn opening() -> Facts {
        let mut facts = Facts::default();
        facts.add("disk", "/dev/vda").unwrap();
        facts.add("disk-identity", Digest::of(b"vda")).unwrap();
        facts.add("disk-number", "252:0").unwrap();
        facts.add("disk-bytes", 42949672960u64).unwrap();
        facts.add("disk-model", "QEMU HARDDISK").unwrap();
        facts.add("disk-serial", "RESPIN_TEST_ONLY").unwrap();
        facts.add("disk-wwn", "").unwrap();
        facts.add("firmware", "uefi").unwrap();
        facts.add("filesystem", "ext4").unwrap();
        facts.flag("encrypted", false).unwrap();
        facts.flag("swap", true).unwrap();
        facts
    }
    fn one(name: &str, value: impl fmt::Display) -> Facts {
        let mut facts = Facts::default();
        facts.add(name, value).unwrap();
        facts
    }
    fn journal_for(id: &str) -> (Memory, Journal) {
        let memory = Memory::default();
        let journal = Journal::open_with(
            Box::new(memory.clone()),
            Path::new("/run/sekka-install"),
            id,
            opening(),
            vec!["ada-laptop".into(), "Ada Lovelace".into(), "ada".into()],
        )
        .unwrap();
        (memory, journal)
    }
    fn journal() -> (Memory, Journal) {
        journal_for(ID)
    }
    fn live() -> PathBuf {
        PathBuf::from(format!("/run/sekka-install/{ID}.jsonl"))
    }
    /// The record's first `n` entries, as a copy that lagged would hold.
    fn first(record: &Record, n: usize) -> Record {
        let mut bytes = Vec::new();
        for line in &record.lines[..n] {
            bytes.extend_from_slice(line);
            bytes.push(b'\n');
        }
        Record::parse(&bytes).unwrap()
    }

    /// A journal through `last`, every stage completed with the checks the
    /// retry needs.
    fn through(last: Stage) -> (Memory, Journal) {
        through_for(ID, last)
    }
    fn through_for(id: &str, last: Stage) -> (Memory, Journal) {
        let (memory, mut journal) = journal_for(id);
        journal
            .intent(Stage::Preparation, Facts::default())
            .unwrap();
        journal
            .completed(
                Stage::Preparation,
                one("system", SYSTEM),
                Verification::Inspected,
            )
            .unwrap();
        journal
            .confirmed(one("configuration", Digest::of(b"configuration.nix")))
            .unwrap();
        for (stage, verification) in [
            (Stage::Storage, Verification::Inspected),
            (Stage::Files, Verification::ReadBack),
            (Stage::Copy, Verification::Registered),
            (Stage::Bootloader, Verification::Inspected),
            (Stage::Verification, Verification::ReadBack),
            (Stage::Flush, Verification::Inspected),
        ] {
            if stage > last {
                break;
            }
            journal.intent(stage, Facts::default()).unwrap();
            journal
                .completed(stage, one("partitions", 3), verification)
                .unwrap();
        }
        (memory, journal)
    }

    fn reparse(journal: &Journal) -> Record {
        Record::parse(&journal.record().bytes()).unwrap()
    }

    #[test]
    fn a_complete_installation_reads_back_and_hands_over() {
        let (memory, journal) = through(Stage::Flush);
        let record = Record::parse(&memory.file(&live()).unwrap()).unwrap();
        assert_eq!(record.state(), &State::Done(Stage::Flush));
        assert_eq!(record.next(), [Next::Restart]);
        assert_eq!(record.head(), journal.record().head());
        assert_eq!(record.entries().len(), 16);
        assert_eq!(record.installation(), ID);
    }

    #[test]
    fn every_entry_is_on_disk_before_the_journal_returns() {
        let (memory, mut journal) = through(Stage::Files);
        journal.intent(Stage::Copy, one("system", SYSTEM)).unwrap();
        let record = Record::parse(&memory.file(&live()).unwrap()).unwrap();
        assert_eq!(record.state(), &State::Started(Stage::Copy));
        // An interrupted copy is never resumed: only starting over.
        assert_eq!(record.next(), [Next::StartOver]);
        assert!(record.retry_offer().is_none());
    }

    #[test]
    fn a_torn_or_trailing_write_is_refused() {
        let (_, journal) = through(Stage::Copy);
        let bytes = journal.record().bytes();
        for cut in [1, 10, bytes.len() / 2, bytes.len() - 1] {
            let error = Record::parse(&bytes[..cut]).unwrap_err();
            assert!(format!("{error:#}").len() > 10, "{cut}");
        }
        assert!(
            format!(
                "{:#}",
                Record::parse(&bytes[..bytes.len() - 1]).unwrap_err()
            )
            .contains("cut short")
        );
        let mut doubled = bytes.clone();
        doubled.push(b'\n');
        assert!(Record::parse(&doubled).is_err());
        assert!(Record::parse(b"").is_err());
        assert!(Record::parse(&vec![b'x'; MAX_BYTES + 1]).is_err());
    }

    #[test]
    fn an_edited_reordered_or_foreign_entry_breaks_the_chain() {
        let (_, journal) = through(Stage::Copy);
        let text = String::from_utf8(journal.record().bytes()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let join = |lines: &[&str]| format!("{}\n", lines.join("\n"));
        // A changed value, even one that is still valid.
        let edited = text.replacen("\"partitions\",\"3\"", "\"partitions\",\"4\"", 1);
        assert_ne!(edited, text);
        assert!(Record::parse(edited.as_bytes()).is_err());
        // Two entries swapped, an entry dropped.
        let mut swapped = lines.clone();
        swapped.swap(4, 5);
        assert!(Record::parse(join(&swapped).as_bytes()).is_err());
        let mut dropped = lines.clone();
        dropped.remove(6);
        assert!(Record::parse(join(&dropped).as_bytes()).is_err());
        // Another installation's entry, from its own good chain.
        let foreign =
            String::from_utf8(through_for(OTHER, Stage::Copy).1.record().bytes()).unwrap();
        assert!(Record::parse(foreign.as_bytes()).is_ok());
        let mut mixed = lines.clone();
        let foreign_lines: Vec<&str> = foreign.lines().collect();
        mixed[7] = foreign_lines[7];
        assert!(Record::parse(join(&mixed).as_bytes()).is_err());
        // The installation renamed throughout: every digest breaks.
        assert!(Record::parse(text.replace(ID, OTHER).as_bytes()).is_err());
        // The same contents in another form than Genkan's.
        let spaced = text.replacen("{\"schema\":", "{ \"schema\":", 1);
        assert!(
            format!("{:#}", Record::parse(spaced.as_bytes()).unwrap_err())
                .contains("form Genkan writes")
        );
        // Unknown fields, facts and versions.
        for (from, to) in [
            ("\"kind\":\"opened\"", "\"kind\":\"opened\",\"extra\":1"),
            ("\"disk-model\"", "\"disk-colour\""),
            (SCHEMA, "sekka.genkan.record/2"),
        ] {
            let changed = text.replacen(from, to, 1);
            assert_ne!(changed, text, "{from}");
            assert!(Record::parse(changed.as_bytes()).is_err(), "{to}");
        }
    }

    #[test]
    fn a_forged_but_well_chained_record_still_obeys_the_stage_order() {
        // Rebuild a chain by hand, as a forger could, skipping the copy.
        let mut entries = through(Stage::Files).1.record().entries().to_vec();
        let mut push = |event| {
            let previous = Digest::of(&entries.last().unwrap().line());
            entries.push(Entry {
                installation: ID.into(),
                sequence: entries.len() as u64 + 1,
                previous: Some(previous),
                at: 0,
                event,
            });
        };
        push(Event::Intent {
            stage: Stage::Bootloader,
            facts: Facts::default(),
        });
        let mut bytes = Vec::new();
        for entry in &entries {
            bytes.extend(entry.line());
            bytes.push(b'\n');
        }
        let error = Record::parse(&bytes).unwrap_err();
        assert!(format!("{error:#}").contains("bootloader can't start here"));

        // Completion without its check, and cancelling a changed disk.
        for event in [
            Event::Completed {
                stage: Stage::Storage,
                facts: Facts::default(),
                verification: Verification::None,
            },
            Event::Cancelled,
        ] {
            let mut entries = through(Stage::Preparation).1.record().entries().to_vec();
            assert!(observe(&entries).is_ok());
            for event in [
                Event::Intent {
                    stage: Stage::Storage,
                    facts: Facts::default(),
                },
                event,
            ] {
                let previous = Digest::of(&entries.last().unwrap().line());
                entries.push(Entry {
                    installation: ID.into(),
                    sequence: entries.len() as u64 + 1,
                    previous: Some(previous),
                    at: 0,
                    event,
                });
            }
            assert!(observe(&entries).is_err());
        }
    }

    #[test]
    fn only_a_bootloader_failure_after_verified_stages_may_be_retried() {
        let (_, mut journal) = through(Stage::Copy);
        journal
            .intent(Stage::Bootloader, one("attempt", 1))
            .unwrap();
        journal
            .failed(
                Stage::Bootloader,
                &anyhow::anyhow!("efibootmgr failed"),
                DiskChanged::Yes,
            )
            .unwrap();
        let record = reparse(&journal);
        assert_eq!(record.next(), [Next::RetryBootloader, Next::StartOver]);
        let offer = record.retry_offer().unwrap();
        assert_eq!(offer.record, record.head().to_string());
        assert_eq!((offer.attempt, offer.attempts), (2, BOOTLOADER_ATTEMPTS));
        assert!(
            record
                .changed()
                .contains("Installing the bootloader didn't finish")
        );
        assert!(
            record
                .next_step(true)
                .starts_with("Try the bootloader again")
        );
        // Retried until the attempts run out, then never again.
        for attempt in 2..=BOOTLOADER_ATTEMPTS {
            journal
                .intent(Stage::Bootloader, one("attempt", attempt))
                .unwrap();
            journal
                .failed(
                    Stage::Bootloader,
                    &anyhow::anyhow!("efibootmgr failed"),
                    DiskChanged::Yes,
                )
                .unwrap();
        }
        let record = reparse(&journal);
        assert_eq!(record.next(), [Next::StartOver]);
        assert!(record.retry_offer().is_none());
        assert!(journal.intent(Stage::Bootloader, Facts::default()).is_err());

        // Any other stage's failure ends the installation.
        for stage in [Stage::Storage, Stage::Files, Stage::Copy] {
            let previous = Stage::ORDER[Stage::ORDER.iter().position(|s| *s == stage).unwrap() - 1];
            let (_, mut journal) = through(previous);
            journal.intent(stage, Facts::default()).unwrap();
            journal
                .failed(stage, &anyhow::anyhow!("failed"), DiskChanged::Unknown)
                .unwrap();
            let record = reparse(&journal);
            assert_eq!(record.next(), [Next::StartOver], "{stage:?}");
            assert!(
                journal.intent(stage, Facts::default()).is_err(),
                "{stage:?}"
            );
        }
    }

    #[test]
    fn a_retry_needs_each_earlier_stage_checked_as_far_as_it_can_be() {
        // The copy completed, but only inspected: not proof of the closure.
        let (_, mut journal) = through(Stage::Files);
        journal.intent(Stage::Copy, Facts::default()).unwrap();
        journal
            .completed(Stage::Copy, Facts::default(), Verification::Inspected)
            .unwrap();
        journal.intent(Stage::Bootloader, Facts::default()).unwrap();
        journal
            .failed(Stage::Bootloader, &anyhow::anyhow!("x"), DiskChanged::Yes)
            .unwrap();
        assert_eq!(reparse(&journal).next(), [Next::StartOver]);
    }

    #[test]
    fn nothing_written_means_review_again() {
        let (_, mut journal) = journal();
        journal
            .intent(Stage::Preparation, Facts::default())
            .unwrap();
        journal.cancelled().unwrap();
        let record = reparse(&journal);
        assert_eq!(record.next(), [Next::Review]);
        assert!(
            record
                .changed()
                .starts_with("Nothing was written to QEMU HARDDISK (/dev/vda)")
        );
        let (_, mut journal) = through(Stage::Preparation);
        journal
            .refused("The confirmed plan differs from the prepared system")
            .unwrap();
        assert_eq!(reparse(&journal).next(), [Next::Review]);
        assert!(journal.intent(Stage::Storage, Facts::default()).is_err());
    }

    #[test]
    fn failed_writes_leave_the_previous_record_and_stop_the_journal() {
        type Fault = fn(&mut Disk);
        let faults: [(&str, Fault); 5] = [
            ("no space", |d| {
                d.fail_write = Some((d.writes, io::ErrorKind::StorageFull))
            }),
            ("short write", |d| {
                d.short = Some(0);
            }),
            ("fsync", |d| d.fail_sync = true),
            ("rename", |d| d.fail_rename = true),
            ("directory fsync", |d| d.fail_dir_sync = true),
        ];
        for (name, fault) in faults {
            let (memory, mut journal) = through(Stage::Files);
            let before = memory.file(&live()).unwrap();
            memory.with(fault);
            assert!(
                journal.intent(Stage::Copy, Facts::default()).is_err(),
                "{name}"
            );
            let after = memory.file(&live()).unwrap();
            if name == "directory fsync" {
                // Renamed, but its durability is unknown: the journal stops.
                assert_ne!(after, before);
            } else {
                assert_eq!(after, before, "{name}: the previous record is intact");
                let temporary = PathBuf::from(format!("/run/sekka-install/.{ID}.jsonl.tmp"));
                assert!(
                    memory.file(&temporary).is_none(),
                    "{name}: no temporary left"
                );
            }
            Record::parse(&after).unwrap();
            assert_eq!(journal.record().state(), &State::Done(Stage::Files));
            // Once a write failed, nothing more is written: the copies'
            // state isn't known.
            memory.with(|d| {
                *d = Disk {
                    files: d.files.clone(),
                    ..Disk::default()
                }
            });
            assert!(
                journal.intent(Stage::Copy, Facts::default()).is_err(),
                "{name}"
            );
            assert!(journal.report("x", true).retry.is_none());
        }
    }

    #[test]
    fn a_short_write_that_completes_is_fine() {
        let (memory, mut journal) = through(Stage::Files);
        memory.with(|d| d.short = Some(7));
        journal.intent(Stage::Copy, Facts::default()).unwrap();
        Record::parse(&memory.file(&live()).unwrap()).unwrap();
    }

    #[test]
    fn a_stale_temporary_file_is_replaced() {
        let (memory, mut journal) = through(Stage::Files);
        let temporary = PathBuf::from(format!("/run/sekka-install/.{ID}.jsonl.tmp"));
        memory.set(&temporary, b"left by a crash".to_vec());
        journal.intent(Stage::Copy, Facts::default()).unwrap();
        assert!(memory.file(&temporary).is_none());
        Record::parse(&memory.file(&live()).unwrap()).unwrap();
    }

    #[test]
    fn the_mirror_starts_after_storage_and_lags_but_never_leads() {
        let (memory, mut journal) = through(Stage::Storage);
        journal.mirror_to(Path::new("/target")).unwrap();
        let mirror = journal.mirror_path().unwrap().to_path_buf();
        assert_eq!(
            mirror,
            PathBuf::from(format!("/target/var/log/sekka-install/{ID}.jsonl"))
        );
        assert_eq!(memory.file(&mirror), memory.file(&live()));
        journal.intent(Stage::Files, Facts::default()).unwrap();
        assert_eq!(memory.file(&mirror), memory.file(&live()));
        let full = Record::parse(&memory.file(&live()).unwrap()).unwrap();
        // The mirror one entry behind: a crash between the two writes.
        let behind = first(&full, full.entries().len() - 1);
        assert_eq!(
            Record::reconcile(Some(full.clone()), Some(behind.clone()))
                .unwrap()
                .head(),
            full.head()
        );
        // A mirror ahead of the live copy, or a different one, conflicts,
        // even when it says more progress.
        assert!(Record::reconcile(Some(behind.clone()), Some(full.clone())).is_err());
        let (_, mut other) = through(Stage::Storage);
        other.intent(Stage::Files, one("wifi-profiles", 2)).unwrap();
        other
            .completed(Stage::Files, Facts::default(), Verification::ReadBack)
            .unwrap();
        assert!(Record::reconcile(Some(full.clone()), Some(reparse(&other))).is_err());
        // After a restart only the mirror is left.
        assert_eq!(
            Record::reconcile(None, Some(full.clone())).unwrap().head(),
            full.head()
        );
        assert!(Record::reconcile(None, None).is_err());
    }

    #[test]
    fn rereading_refuses_a_copy_changed_outside_genkan() {
        let (memory, mut journal) = through(Stage::Copy);
        assert_eq!(journal.reread().unwrap().head(), journal.record().head());
        // Even a valid record (another installation's) put in its place.
        let (_, other) = through_for(OTHER, Stage::Copy);
        memory.set(&live(), other.record().bytes());
        let error = journal.reread().unwrap_err();
        assert!(format!("{error:#}").contains("changed outside Genkan"));
        // And it is never written over.
        assert!(journal.refused("x").is_err());
        assert!(journal.report("x", true).retry.is_none());
    }

    #[test]
    fn records_and_reports_hold_no_secrets() {
        let (_, mut journal) = through(Stage::Copy);
        journal.intent(Stage::Bootloader, Facts::default()).unwrap();
        journal
            .failed(
                Stage::Bootloader,
                &anyhow::anyhow!(
                    "error: nixosConfigurations.ada-laptop: grub-install failed for Ada Lovelace (ada)"
                ),
                DiskChanged::Yes,
            )
            .unwrap();
        let report = journal.report("grub-install failed", true);
        assert!(report.retry.is_some());
        // Serial numbers become digests; personal names are replaced.
        assert!(!report.support.contains("RESPIN_TEST_ONLY"));
        assert!(report.support.contains("sha256:"));
        assert!(!report.support.contains("ada-laptop"));
        assert!(!report.support.contains("Ada Lovelace"));
        assert!(report.support.contains("[redacted]"));
        // The private record keeps them, for the person the disk belongs to.
        let bytes = String::from_utf8(journal.record().bytes()).unwrap();
        assert!(bytes.contains("RESPIN_TEST_ONLY"));
        // A record has nowhere to put a secret: every fact is named, and
        // free text is limited to the disk's description and messages.
        assert!(Facts::default().add("password", "x").is_err());
        assert!(Facts::default().add("password-hash", "x").is_err());
        assert!(Facts::default().add("psk", "x").is_err());
        assert!(FACTS.iter().all(|(name, _)| {
            !["pass", "secret", "psk", "key", "hash"]
                .iter()
                .any(|word| name.contains(word))
        }));
    }

    #[test]
    fn facts_are_checked_when_written_and_read() {
        let mut facts = Facts::default();
        assert!(facts.add("root-uuid", "../../etc").is_err());
        assert!(facts.add("system", "/nix/store/../etc/shadow").is_err());
        assert!(facts.add("disk", "/dev/../etc").is_err());
        assert!(facts.add("partitions", "03").is_err());
        assert!(facts.add("boot-mib", "1025-1").is_err());
        facts
            .add("disk-model", "Model\u{7}\nwith controls")
            .unwrap();
        assert_eq!(facts.get("disk-model"), Some("Model  with controls"));
        assert!(facts.add("disk-model", "again").is_err());
        let long = "x".repeat(MAX_TEXT * 2);
        facts.add("disk-serial", &long).unwrap();
        assert!(facts.get("disk-serial").unwrap().len() <= MAX_TEXT);
        // A long failure is kept short, at a character boundary.
        let long = message(&"é".repeat(MAX_MESSAGE));
        assert!(long.len() <= MAX_MESSAGE && long.ends_with('…'));
        check_message(&long).unwrap();
    }

    #[test]
    fn layouts_round_trip_through_facts() {
        let layout = Layout::for_install(
            40 * 1024u64.pow(3),
            Firmware::Bios,
            Some(4 * 1024u64.pow(3)),
            true,
        )
        .unwrap();
        let mut facts = Facts::default();
        add_layout(&mut facts, &layout).unwrap();
        assert_eq!(super::layout(&facts).unwrap(), layout);
    }

    #[test]
    fn digests_are_lowercase_hex() {
        let digest = Digest::of(b"abc");
        assert_eq!(
            digest.to_string(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(Digest::parse(&digest.to_string()).unwrap(), digest);
        assert!(Digest::parse(&digest.to_string().to_uppercase()).is_err());
        assert!(Digest::parse("ba78").is_err());
    }

    #[test]
    fn real_files_are_written_privately_and_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let records = dir.path().join("sekka-install");
        let mut journal = Journal::open(&records, opening(), vec![]).unwrap();
        journal
            .intent(Stage::Preparation, Facts::default())
            .unwrap();
        let path = journal.live_path().to_path_buf();
        let meta = fs::metadata(&path).unwrap();
        assert_eq!(meta.mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(&records).unwrap().mode() & 0o777, 0o700);
        let record = Record::read(&path).unwrap();
        assert_eq!(record.state(), &State::Preparing);
        assert_eq!(journal.reread().unwrap().head(), record.head());
        // A directory others can write, or a link, isn't Genkan's.
        use std::os::unix::fs::PermissionsExt;
        let open = dir.path().join("open");
        fs::create_dir(&open).unwrap();
        fs::set_permissions(&open, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(Journal::open(&open, opening(), vec![]).is_err());
        let link = dir.path().join("link.jsonl");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(Record::read(&link).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Record::read(&path).is_err());
        // The mirror's directories on a target root.
        let root = dir.path().join("target");
        fs::create_dir(&root).unwrap();
        journal.mirror_to(&root).unwrap();
        let mirror = root.join(format!("{TARGET_DIR}/{}.jsonl", record.installation()));
        assert_eq!(fs::read(&mirror).unwrap(), fs::read(&path).unwrap());
        assert_eq!(
            fs::metadata(root.join(TARGET_DIR)).unwrap().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(root.join("var/log")).unwrap().mode() & 0o777,
            0o755
        );
    }
}
