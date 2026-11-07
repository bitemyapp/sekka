// SPDX-License-Identifier: GPL-3.0-or-later
//! Evidence for the installation record, read from the real target, and the
//! one stage Genkan repeats: the bootloader.
//!
//! The bootloader is installed again only on the target the record
//! describes: the same disk (by its identity, not its name), the kernel's
//! partition geometry, the identities in the superblocks, the same mounts,
//! the same configuration files and the same system in the target's store.
//! The window asks by naming the record's last entry as it was offered; the
//! helper reads the record back, checks it is exactly what it wrote and
//! that it allows a retry, then checks the disk, before writing anything.
//! Partitioning, formatting and copying are never repeated: a failure there
//! ends the installation, and the window says what changed.
use crate::{
    Filesystem, Firmware, InstallPlan,
    disk::{self, Layout},
    filesystem::{self, Identities},
    install::Event,
    process::{output, output_full},
    record::{self, Digest, DiskChanged, Facts, Journal, Record, Stage, Verification},
    session::RetryRequest,
};
use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::Deserialize;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

/// The files the record keeps digests of, under the target's root.
pub const FILES: [(&str, &str); 4] = [
    ("configuration", "etc/nixos/configuration.nix"),
    ("flake", "etc/nixos/flake.nix"),
    ("lock", "etc/nixos/flake.lock"),
    ("hardware", "etc/nixos/hardware-configuration.nix"),
];

/// How the window answers an offer to try the bootloader again.
pub trait Answers {
    /// Forget what the window sent before the offer: it can't answer it.
    /// False once the window has closed the session.
    fn discard_stale(&mut self) -> bool;
    /// The window's next line; `None` when it closed the session, which
    /// declines.
    fn next(&mut self) -> Option<Vec<u8>>;
}
/// No window to ask (`install` and `preflight`): nothing is offered.
pub struct NoAnswers;
impl Answers for NoAnswers {
    fn discard_stale(&mut self) -> bool {
        false
    }
    fn next(&mut self) -> Option<Vec<u8>> {
        None
    }
}

/// A system's closure as a store registers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Closure {
    /// Of each path's NAR hash and size, sorted by path.
    pub digest: Digest,
    pub paths: u64,
    pub bytes: u64,
}

/// `nix path-info --json --recursive`, in either of the forms Nix has used:
/// an array of entries with paths, or an object keyed by path.
pub fn closure(value: &serde_json::Value) -> Result<Closure> {
    use serde_json::Value;
    let entries: Vec<(String, &Value)> = match value {
        Value::Array(items) => items
            .iter()
            .map(|item| {
                Ok((
                    item["path"]
                        .as_str()
                        .context("A closure entry has no path")?
                        .to_owned(),
                    item,
                ))
            })
            .collect::<Result<_>>()?,
        Value::Object(items) => items
            .iter()
            .map(|(path, item)| (path.clone(), item))
            .collect(),
        _ => bail!("Invalid closure information"),
    };
    ensure!(!entries.is_empty(), "Empty closure");
    let mut lines = Vec::with_capacity(entries.len());
    let mut bytes = 0u64;
    for (path, item) in entries {
        let path = if path.starts_with('/') {
            path
        } else {
            format!("/nix/store/{path}")
        };
        ensure!(item.is_object(), "{path} isn't valid in the store");
        let hash = match &item["narHash"] {
            Value::String(hash) => hash.clone(),
            Value::Null => bail!("{path} has no NAR hash"),
            other => other.to_string(),
        };
        let size = item["narSize"]
            .as_u64()
            .with_context(|| format!("{path} has no NAR size"))?;
        bytes = bytes.checked_add(size).context("Closure size overflow")?;
        lines.push(format!("{path} {hash} {size}\n"));
    }
    lines.sort();
    let count = lines.len();
    lines.dedup();
    ensure!(lines.len() == count, "A closure lists a path twice");
    Ok(Closure {
        digest: Digest::of(lines.concat().as_bytes()),
        paths: count as u64,
        bytes,
    })
}

/// The real target, as a check reads it now. Tests substitute their own.
pub trait Probe {
    /// What lsblk reports for `path` now, whether or not it is in use.
    fn disk(&mut self, path: &str) -> Result<disk::Identity>;
    /// The kernel's partitions must be exactly `layout`.
    fn layout(&mut self, disk: &disk::Identity, layout: &Layout) -> Result<()>;
    /// The identity in the superblock on `device`, a `kind` filesystem.
    fn uuid(&mut self, device: &str, kind: &str) -> Result<String>;
    /// What is mounted at `mount`: its source device and filesystem type.
    fn mount(&mut self, mount: &Path) -> Result<(String, String)>;
    /// A system's closure in `store` (the live store when `None`).
    fn closure(&mut self, store: Option<&str>, system: &str) -> Result<Closure>;
    fn read(&mut self, path: &Path) -> Result<Vec<u8>>;
}

/// The disks and tools themselves.
pub struct Real;
impl Probe for Real {
    fn disk(&mut self, path: &str) -> Result<disk::Identity> {
        disk::discover()?
            .into_iter()
            .find(|d| d.identity.path == path)
            .map(|d| d.identity)
            .with_context(|| format!("{path} disappeared"))
    }
    fn layout(&mut self, disk: &disk::Identity, layout: &Layout) -> Result<()> {
        disk::verify_layout(disk, layout)
    }
    fn uuid(&mut self, device: &str, kind: &str) -> Result<String> {
        filesystem::probe_uuid(device, kind)
    }
    fn mount(&mut self, mount: &Path) -> Result<(String, String)> {
        #[derive(Deserialize)]
        struct Mount {
            source: String,
            fstype: String,
        }
        #[derive(Deserialize)]
        struct Mounts {
            filesystems: Vec<Mount>,
        }
        let mounts: Mounts = serde_json::from_str(&output(
            "findmnt",
            &[
                "--json",
                "--output",
                "SOURCE,FSTYPE",
                "--mountpoint",
                mount.to_str().context("Target path encoding")?,
            ],
            15,
        )?)?;
        let [mount] = <[Mount; 1]>::try_from(mounts.filesystems)
            .map_err(|_| anyhow!("Not exactly one filesystem at {}", mount.display()))?;
        // Btrfs names the mounted subvolume after the device: `/dev/vda2[/]`.
        let source = mount.source.split('[').next().unwrap_or_default();
        // A mapping may be named /dev/mapper/… or /dev/dm-N.
        let source = fs::canonicalize(source)
            .with_context(|| format!("Mounted device {source}"))?
            .to_string_lossy()
            .into_owned();
        Ok((source, mount.fstype))
    }
    fn closure(&mut self, store: Option<&str>, system: &str) -> Result<Closure> {
        let mut args = vec!["path-info", "--json", "--recursive"];
        if let Some(store) = store {
            args.extend(["--store", store]);
        }
        args.push(system);
        closure(&serde_json::from_str(&output_full("nix", &args, 600)?)?)
    }
    fn read(&mut self, path: &Path) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        fs::File::open(path)
            .with_context(|| format!("Reading {}", path.display()))?
            .take(16 << 20)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

/// The target as the confirmed plan and its preparation describe it.
pub struct Expected<'a> {
    pub disk: &'a disk::Identity,
    pub firmware: Firmware,
    pub filesystem: Filesystem,
    pub layout: &'a Layout,
    pub ids: &'a Identities,
    /// The target's root, as mounted.
    pub mount: &'a Path,
    pub system: &'a str,
    /// Digests of the files in `FILES`, in that order.
    pub files: [Digest; 4],
}
impl Expected<'_> {
    fn store(&self) -> Result<String> {
        Ok(format!(
            "local?root={}",
            self.mount.to_str().context("Target path encoding")?
        ))
    }
}

/// The identity a record binds a disk by, serial number and WWN included.
pub fn identity_digest(identity: &disk::Identity) -> Digest {
    Digest::of(&serde_json::to_vec(identity).expect("a disk identity serializes"))
}

/// The record's first entry: the plan, without anything secret.
pub fn opening(plan: &InstallPlan) -> Result<Facts> {
    let disk = plan.disk();
    let mut facts = Facts::default();
    facts.add("disk", &disk.path)?;
    facts.add("disk-identity", identity_digest(disk))?;
    facts.add("disk-number", &disk.major_minor)?;
    facts.add("disk-bytes", disk.bytes)?;
    facts.add("disk-model", &disk.model)?;
    facts.add("disk-serial", &disk.serial)?;
    facts.add("disk-wwn", &disk.wwn)?;
    facts.add("firmware", record::firmware_name(plan.firmware()))?;
    facts.add("filesystem", plan.filesystem().name())?;
    facts.flag("encrypted", plan.encrypt())?;
    facts.flag("swap", plan.swap())?;
    facts.add("helper", env!("CARGO_PKG_VERSION"))?;
    Ok(facts)
}

/// Names that identify the person or the computer, kept out of support
/// reports.
pub fn personal(plan: &InstallPlan) -> Vec<String> {
    let mut names = vec![
        plan.hostname().as_str().to_string(),
        plan.username().as_str().to_string(),
        plan.full_name().to_string(),
    ];
    let onboarding = plan.onboarding();
    if let Some(user) = onboarding.github() {
        names.push(user.as_str().into());
    }
    if let Some(git) = onboarding.git() {
        names.push(git.name().into());
        names.push(git.email().into());
    }
    names
}

fn mapping(uuid: &str) -> String {
    format!("/dev/mapper/genkan-{uuid}")
}

/// The identities storage chose and the geometry it will write: its intent.
pub fn planned(ids: &Identities, layout: &Layout) -> Result<Facts> {
    let mut facts = Facts::default();
    record::add_layout(&mut facts, layout)?;
    add_identities(&mut facts, ids)?;
    Ok(facts)
}
fn add_identities(facts: &mut Facts, ids: &Identities) -> Result<()> {
    facts.add("root-uuid", &ids.root)?;
    if let Some(serial) = &ids.efi {
        facts.add("efi-serial", serial)?;
    }
    if let Some(uuid) = &ids.swap {
        facts.add("swap-uuid", uuid)?;
    }
    if let Some(uuid) = &ids.bios_boot {
        facts.add("bios-boot-uuid", uuid)?;
    }
    if let Some(encryption) = &ids.encryption {
        facts.add("luks-root-uuid", &encryption.root)?;
        if let Some(uuid) = &encryption.swap {
            facts.add("luks-swap-uuid", uuid)?;
        }
    }
    Ok(())
}

/// Read the target's storage back: the same disk, the kernel's partition
/// geometry, each superblock's identity, and what is mounted at the root
/// and at /boot. The evidence that storage completed, and checked again
/// before the bootloader is retried and before the target is handed over.
pub fn storage(expected: &Expected, probe: &mut dyn Probe) -> Result<Facts> {
    let path = &expected.disk.path;
    let current = probe.disk(path)?;
    ensure!(
        current == *expected.disk,
        "The disk at {path} is no longer the one selected; Genkan won't write to it"
    );
    probe
        .layout(&current, expected.layout)
        .context("The disk's partitions are not the ones Genkan created")?;
    let ids = expected.ids;
    let mut check = |device: &str, kind: &str, uuid: &str| -> Result<()> {
        let found = probe.uuid(device, kind)?;
        ensure!(
            found.eq_ignore_ascii_case(uuid),
            "{device} holds {kind} {found}, not the {uuid} Genkan created"
        );
        Ok(())
    };
    let root_partition = disk::partition(path, 2);
    let root = match &ids.encryption {
        Some(encryption) => {
            check(&root_partition, "crypto_LUKS", &encryption.root)?;
            mapping(&encryption.root)
        }
        None => root_partition,
    };
    check(&root, expected.filesystem.name(), &ids.root)?;
    let boot = disk::partition(path, 1);
    if let Some(serial) = &ids.efi {
        check(&boot, "vfat", serial)?;
    }
    if let Some(uuid) = &ids.swap {
        let partition = disk::partition(path, 3);
        let device = match ids.encryption.as_ref().and_then(|e| e.swap.as_deref()) {
            Some(luks) => {
                check(&partition, "crypto_LUKS", luks)?;
                mapping(luks)
            }
            None => partition,
        };
        check(&device, "swap", uuid)?;
    }
    let bios_boot = expected
        .layout
        .bios_boot_partition()
        .map(|index| disk::partition(path, index));
    if let (Some(device), Some(uuid)) = (&bios_boot, &ids.bios_boot) {
        check(device, "ext4", uuid)?;
    }
    let mut mounted = |at: PathBuf, device: &str, kind: &str| -> Result<()> {
        let (source, fstype) = probe.mount(&at)?;
        let device = fs::canonicalize(device).unwrap_or_else(|_| device.into());
        ensure!(
            Path::new(&source) == device && fstype == kind,
            "{} has {source} ({fstype}) mounted, not {} ({kind})",
            at.display(),
            device.display()
        );
        Ok(())
    };
    mounted(expected.mount.into(), &root, expected.filesystem.name())?;
    if ids.efi.is_some() {
        mounted(expected.mount.join("boot"), &boot, "vfat")?;
    } else if let (Some(device), Some(_)) = (&bios_boot, &ids.bios_boot) {
        mounted(expected.mount.join("boot"), device, "ext4")?;
    }
    let mut facts = Facts::default();
    record::add_layout(&mut facts, expected.layout)?;
    facts.add(
        "partitions",
        2 + u64::from(expected.layout.swap.is_some()) + u64::from(bios_boot.is_some()),
    )?;
    add_identities(&mut facts, ids)?;
    Ok(facts)
}

/// Read the configuration back from the target: the evidence that writing
/// files completed.
pub fn files(expected: &Expected, probe: &mut dyn Probe, wifi_profiles: usize) -> Result<Facts> {
    let mut facts = Facts::default();
    for ((name, path), digest) in FILES.iter().zip(expected.files) {
        let found = Digest::of(&probe.read(&expected.mount.join(path))?);
        ensure!(
            found == digest,
            "/{path} on the new disk differs from what Genkan wrote"
        );
        facts.add(name, found)?;
    }
    facts.flag("account", true)?;
    facts.add("wifi-profiles", wifi_profiles)?;
    Ok(facts)
}

/// The system's closure in the target's store: the evidence that copying
/// completed. With `live`, it must be exactly the prepared closure.
pub fn copied(expected: &Expected, live: Option<Closure>, probe: &mut dyn Probe) -> Result<Facts> {
    let found = probe.closure(Some(&expected.store()?), expected.system)?;
    if let Some(live) = live {
        ensure!(
            found == live,
            "The new disk's store doesn't hold exactly the prepared system: {} paths, {} expected",
            found.paths,
            live.paths
        );
    }
    let mut facts = Facts::default();
    facts.add("system", expected.system)?;
    facts.add("closure", found.digest)?;
    facts.add("closure-paths", found.paths)?;
    facts.add("closure-bytes", found.bytes)?;
    Ok(facts)
}

/// Check a record and the real target against each other and the plan:
/// the record must be this plan's, with storage, files and copy completed
/// and checked, and the disk must still be what it says Genkan made.
pub fn recheck(record: &Record, expected: &Expected, probe: &mut dyn Probe) -> Result<()> {
    let opened = record.opened();
    ensure!(
        opened.digest("disk-identity")? == identity_digest(expected.disk)
            && opened.require("disk")? == expected.disk.path,
        "The installation record is for another disk"
    );
    ensure!(
        record::firmware(opened)? == expected.firmware
            && record::filesystem(opened)? == expected.filesystem
            && opened.is("encrypted")? == expected.layout.encrypted,
        "The installation record is for another plan"
    );
    let confirmed = record
        .confirmed()
        .context("The installation record has no confirmed configuration")?;
    for ((name, _), digest) in FILES.iter().zip(expected.files) {
        ensure!(
            confirmed.digest(name)? == digest,
            "The installation record was confirmed for another configuration"
        );
    }
    let stage = |stage: Stage, needed: Verification| -> Result<&Facts> {
        match record.completed(stage) {
            Some((facts, verification)) if verification == needed => Ok(facts),
            _ => bail!(
                "The installation record has no checked {} stage",
                stage.name()
            ),
        }
    };
    let stored = stage(Stage::Storage, Verification::Inspected)?;
    ensure!(
        record::layout(stored)? == *expected.layout,
        "The installation record is for other partitions"
    );
    let written = stage(Stage::Files, Verification::ReadBack)?;
    let copy = stage(Stage::Copy, Verification::Registered)?;
    ensure!(
        copy.require("system")? == expected.system,
        "The installation record is for another system"
    );
    // The disk as it is now, against what was recorded when it was made.
    ensure!(
        storage(expected, probe)? == *stored,
        "The disk's storage differs from the installation record"
    );
    let files = files(expected, probe, written.number("wifi-profiles")? as usize)?;
    ensure!(
        files == *written,
        "The new disk's configuration differs from the installation record"
    );
    let closure = probe.closure(Some(&expected.store()?), expected.system)?;
    ensure!(
        copy.digest("closure")? == closure.digest,
        "The new disk's store no longer holds the system as it was checked"
    );
    Ok(())
}

/// Install the bootloader, recording the attempt. After a failure, when
/// the record allows it and the window is there, offer to try again; try
/// only when the window's answer names the offer and every check passes.
/// Anything else ends the installation with the attempt's error.
pub fn bootloader(
    journal: &mut Journal,
    answers: &mut dyn Answers,
    emit: &mut dyn FnMut(Event),
    mut attempt: impl FnMut() -> Result<Facts>,
    mut recheck: impl FnMut(&Record) -> Result<()>,
) -> Result<()> {
    loop {
        let mut intent = Facts::default();
        intent.add("attempt", journal.record().attempts() + 1)?;
        journal.intent(Stage::Bootloader, intent)?;
        let error = match attempt() {
            Ok(facts) => {
                return journal.completed(Stage::Bootloader, facts, Verification::Inspected);
            }
            Err(error) => error,
        };
        let message = format!("{error:#}");
        if let Err(record) = journal.failed(Stage::Bootloader, &error, DiskChanged::Yes) {
            bail!("{message}\nThe installation record could not be updated either: {record:#}");
        }
        // What the window sent before this offer doesn't answer it.
        let listening = answers.discard_stale();
        let report = journal.report(&message, listening);
        let Some(offer) = report.retry.clone() else {
            return Err(error);
        };
        emit(Event::Recovery { report });
        let Some(line) = answers.next() else {
            return Err(error);
        };
        let checked = (|| -> Result<()> {
            let request: RetryRequest = serde_json::from_slice(&line)
                .map_err(|_| anyhow!("The window's answer is not a retry request"))?;
            ensure!(
                request.retry == offer.stage && request.record == offer.record,
                "The retry request names another installation record"
            );
            let record = journal.reread()?;
            ensure!(
                record.retry_offer().as_ref() == Some(&offer),
                "The installation record no longer allows trying the bootloader again"
            );
            recheck(&record)
        })();
        if let Err(reason) = checked {
            let reason = reason.context("Genkan didn't try the bootloader again");
            let _ = journal.refused(&format!("{reason:#}"));
            return Err(reason);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{BOOTLOADER_ATTEMPTS, Next, State};

    const ROOT: &str = "11111111-2222-4333-8444-555555555555";
    const SWAP: &str = "66666666-7777-4888-9999-aaaaaaaaaaaa";
    const EFI: &str = "A1B2-C3D4";
    const SYSTEM: &str = "/nix/store/0123456789abcdfghijklmnpqrsvwxyz-nixos-system-test-26.11";

    fn identity() -> disk::Identity {
        disk::Identity {
            path: "/dev/vda".into(),
            major_minor: "252:0".into(),
            bytes: 40 * 1024u64.pow(3),
            serial: "RESPIN_TEST_ONLY".into(),
            wwn: String::new(),
            model: "QEMU HARDDISK".into(),
        }
    }
    fn ids() -> Identities {
        Identities {
            root: ROOT.into(),
            efi: Some(EFI.into()),
            swap: Some(SWAP.into()),
            encryption: None,
            bios_boot: None,
        }
    }
    fn layout() -> Layout {
        Layout::for_install(40 * 1024u64.pow(3), Firmware::Uefi, Some(1 << 30), false).unwrap()
    }
    const CONTENTS: [&[u8]; 4] = [
        b"configuration.nix",
        b"flake.nix",
        b"flake.lock",
        b"hardware",
    ];
    fn live_closure() -> Closure {
        Closure {
            digest: Digest::of(b"closure"),
            paths: 1200,
            bytes: 6 << 30,
        }
    }

    /// A target as a probe sees it; each field can be changed to model a
    /// disk that changed behind Genkan's back.
    #[derive(Clone)]
    struct Fake {
        disk: disk::Identity,
        layout_ok: bool,
        uuids: Vec<(String, String)>,
        mounts: Vec<(PathBuf, (String, String))>,
        closure: Closure,
        files: Vec<(PathBuf, Vec<u8>)>,
    }
    impl Fake {
        fn new() -> Self {
            let mount = Path::new("/run/calamares-target-test");
            Self {
                disk: identity(),
                layout_ok: true,
                uuids: vec![
                    ("/dev/vda2".into(), ROOT.into()),
                    ("/dev/vda1".into(), EFI.into()),
                    ("/dev/vda3".into(), SWAP.into()),
                ],
                mounts: vec![
                    (mount.into(), ("/dev/vda2".into(), "ext4".into())),
                    (mount.join("boot"), ("/dev/vda1".into(), "vfat".into())),
                ],
                closure: live_closure(),
                files: FILES
                    .iter()
                    .zip(CONTENTS)
                    .map(|((_, path), bytes)| (mount.join(path), bytes.to_vec()))
                    .collect(),
            }
        }
    }
    impl Probe for Fake {
        fn disk(&mut self, _: &str) -> Result<disk::Identity> {
            Ok(self.disk.clone())
        }
        fn layout(&mut self, _: &disk::Identity, _: &Layout) -> Result<()> {
            ensure!(
                self.layout_ok,
                "Unexpected partition geometry for /dev/vda2"
            );
            Ok(())
        }
        fn uuid(&mut self, device: &str, _: &str) -> Result<String> {
            self.uuids
                .iter()
                .find(|(d, _)| d == device)
                .map(|(_, uuid)| uuid.clone())
                .context("No filesystem")
        }
        fn mount(&mut self, mount: &Path) -> Result<(String, String)> {
            self.mounts
                .iter()
                .find(|(m, _)| m == mount)
                .map(|(_, found)| found.clone())
                .context("Nothing mounted")
        }
        fn closure(&mut self, _: Option<&str>, _: &str) -> Result<Closure> {
            Ok(self.closure)
        }
        fn read(&mut self, path: &Path) -> Result<Vec<u8>> {
            self.files
                .iter()
                .find(|(p, _)| p == path)
                .map(|(_, bytes)| bytes.clone())
                .context("Missing file")
        }
    }

    struct Plan {
        disk: disk::Identity,
        ids: Identities,
        layout: Layout,
    }
    impl Plan {
        fn new() -> Self {
            Self {
                disk: identity(),
                ids: ids(),
                layout: layout(),
            }
        }
        fn expected(&self) -> Expected<'_> {
            Expected {
                disk: &self.disk,
                firmware: Firmware::Uefi,
                filesystem: Filesystem::Ext4,
                layout: &self.layout,
                ids: &self.ids,
                mount: Path::new("/run/calamares-target-test"),
                system: SYSTEM,
                files: CONTENTS.map(Digest::of),
            }
        }
    }

    fn opening_facts(plan: &Plan) -> Facts {
        let mut facts = Facts::default();
        facts.add("disk", "/dev/vda").unwrap();
        facts
            .add("disk-identity", identity_digest(&plan.disk))
            .unwrap();
        facts.add("firmware", "uefi").unwrap();
        facts.add("filesystem", "ext4").unwrap();
        facts.flag("encrypted", false).unwrap();
        facts
    }

    /// A journal through a completed copy, made from what the fake target
    /// shows, as `install::execute` makes it from the real one.
    fn journal(plan: &Plan, dir: &Path) -> Journal {
        let expected = plan.expected();
        let mut fake = Fake::new();
        let mut journal = Journal::open(&dir.join("records"), opening_facts(plan), vec![]).unwrap();
        journal
            .intent(Stage::Preparation, Facts::default())
            .unwrap();
        let mut prepared = Facts::default();
        prepared.add("system", SYSTEM).unwrap();
        journal
            .completed(Stage::Preparation, prepared, Verification::Inspected)
            .unwrap();
        let mut confirmed = Facts::default();
        for ((name, _), digest) in FILES.iter().zip(expected.files) {
            confirmed.add(name, digest).unwrap();
        }
        journal.confirmed(confirmed).unwrap();
        journal
            .intent(Stage::Storage, planned(&plan.ids, &plan.layout).unwrap())
            .unwrap();
        let stored = storage(&expected, &mut fake).unwrap();
        journal
            .completed(Stage::Storage, stored, Verification::Inspected)
            .unwrap();
        journal.intent(Stage::Files, Facts::default()).unwrap();
        let written = files(&expected, &mut fake, 2).unwrap();
        journal
            .completed(Stage::Files, written, Verification::ReadBack)
            .unwrap();
        journal.intent(Stage::Copy, Facts::default()).unwrap();
        let copy = copied(&expected, Some(live_closure()), &mut fake).unwrap();
        journal
            .completed(Stage::Copy, copy, Verification::Registered)
            .unwrap();
        journal
    }

    fn answer(offer: &record::RetryOffer) -> Vec<u8> {
        serde_json::to_vec(&RetryRequest {
            retry: offer.stage.clone(),
            record: offer.record.clone(),
        })
        .unwrap()
    }

    /// Run the bootloader stage: `failures` attempts fail, then it works.
    /// The window answers each offer with `respond(offer)`.
    fn run(
        journal: &mut Journal,
        failures: u32,
        fake: Fake,
        mut respond: impl FnMut(&record::RetryOffer) -> Option<Vec<u8>>,
    ) -> (Result<()>, Vec<Event>, u32) {
        let plan = Plan::new();
        let expected = plan.expected();
        let mut events = Vec::new();
        let mut attempts = 0;
        let mut fake = fake;
        // The window answers the offer it was shown: emit sees it first.
        let offers = std::cell::RefCell::new(Vec::new());
        let result = {
            let mut window = WindowFn {
                offers: &offers,
                respond: &mut respond,
            };
            bootloader(
                journal,
                &mut window,
                &mut |event| {
                    if let Event::Recovery { report } = &event
                        && let Some(offer) = &report.retry
                    {
                        offers.borrow_mut().push(offer.clone());
                    }
                    events.push(event);
                },
                || {
                    attempts += 1;
                    ensure!(
                        attempts > failures,
                        "efibootmgr: EFI variables are not writable"
                    );
                    let mut facts = Facts::default();
                    facts.add("profile", SYSTEM)?;
                    Ok(facts)
                },
                |record| recheck(record, &expected, &mut fake),
            )
        };
        (result, events, attempts)
    }
    struct WindowFn<'a> {
        offers: &'a std::cell::RefCell<Vec<record::RetryOffer>>,
        respond: &'a mut dyn FnMut(&record::RetryOffer) -> Option<Vec<u8>>,
    }
    impl Answers for WindowFn<'_> {
        fn discard_stale(&mut self) -> bool {
            true
        }
        fn next(&mut self) -> Option<Vec<u8>> {
            let offer = self.offers.borrow().last().cloned()?;
            (self.respond)(&offer)
        }
    }

    #[test]
    fn a_bootloader_failure_is_retried_on_the_same_checked_target() {
        let dir = tempfile::tempdir().unwrap();
        let plan = Plan::new();
        let mut journal = journal(&plan, dir.path());
        let (result, events, attempts) =
            run(&mut journal, 1, Fake::new(), |offer| Some(answer(offer)));
        result.unwrap();
        assert_eq!(attempts, 2);
        let [Event::Recovery { report }] = &events[..] else {
            panic!("{events:?}")
        };
        let offer = report.retry.as_ref().unwrap();
        assert_eq!((offer.stage.as_str(), offer.attempt), ("bootloader", 2));
        assert!(report.next.starts_with("Try the bootloader again"));
        assert!(report.message.contains("EFI variables are not writable"));
        let record = Record::read(journal.live_path()).unwrap();
        assert_eq!(record.state(), &State::Done(Stage::Bootloader));
        assert_eq!(record.attempts(), 2);
    }

    #[test]
    fn retries_stop_after_the_last_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let plan = Plan::new();
        let mut journal = journal(&plan, dir.path());
        let (result, events, attempts) =
            run(&mut journal, 99, Fake::new(), |offer| Some(answer(offer)));
        assert!(result.is_err());
        assert_eq!(attempts, BOOTLOADER_ATTEMPTS);
        assert_eq!(events.len() as u32, BOOTLOADER_ATTEMPTS - 1);
        let record = journal.record();
        assert_eq!(record.next(), [Next::StartOver]);
    }

    #[test]
    fn closing_the_window_declines_the_retry() {
        let dir = tempfile::tempdir().unwrap();
        let plan = Plan::new();
        let mut journal = journal(&plan, dir.path());
        let (result, events, attempts) = run(&mut journal, 1, Fake::new(), |_| None);
        assert!(format!("{:#}", result.unwrap_err()).contains("EFI variables"));
        assert_eq!((events.len(), attempts), (1, 1));
        // Nothing was refused: the offer stands in the record until the
        // session ends.
        assert!(matches!(
            journal.record().state(),
            State::Failed {
                stage: Stage::Bootloader,
                ..
            }
        ));
    }

    #[test]
    fn without_a_window_nothing_is_offered() {
        let dir = tempfile::tempdir().unwrap();
        let plan = Plan::new();
        let mut journal = journal(&plan, dir.path());
        let mut events = Vec::new();
        let mut tries = 0;
        let result = bootloader(
            &mut journal,
            &mut NoAnswers,
            &mut |event| events.push(event),
            || {
                tries += 1;
                bail!("grub-install failed")
            },
            |_| panic!("nothing to recheck"),
        );
        assert!(result.is_err());
        assert!(events.is_empty());
        assert_eq!(tries, 1);
    }

    /// The refusal reported for a retry answered with `line`, on `fake`.
    fn refused(fake: Fake, line: impl Fn(&record::RetryOffer) -> Vec<u8>) -> String {
        let dir = tempfile::tempdir().unwrap();
        let plan = Plan::new();
        let mut journal = journal(&plan, dir.path());
        let (result, _, attempts) = run(&mut journal, 1, fake, |offer| Some(line(offer)));
        assert_eq!(attempts, 1, "nothing is tried again");
        let record = Record::read(journal.live_path()).unwrap();
        assert_eq!(record.state(), &State::Refused);
        assert_eq!(record.next(), [Next::StartOver]);
        format!("{:#}", result.unwrap_err())
    }

    #[test]
    fn a_changed_target_is_refused() {
        let mut other_disk = Fake::new();
        other_disk.disk.serial = "ANOTHER_DISK".into();
        assert!(refused(other_disk, answer).contains("no longer the one selected"));
        let mut repartitioned = Fake::new();
        repartitioned.layout_ok = false;
        assert!(refused(repartitioned, answer).contains("partitions are not the ones"));
        let mut reformatted = Fake::new();
        reformatted.uuids[0].1 = "99999999-2222-4333-8444-555555555555".into();
        assert!(refused(reformatted, answer).contains("not the 11111111"));
        let mut remounted = Fake::new();
        remounted.mounts[1].1.0 = "/dev/sdb1".into();
        assert!(refused(remounted, answer).contains("mounted"));
        let mut store = Fake::new();
        store.closure.digest = Digest::of(b"another closure");
        assert!(refused(store, answer).contains("no longer holds the system"));
        let mut edited = Fake::new();
        edited.files[0].1 = b"configuration.nix, edited".to_vec();
        assert!(refused(edited, answer).contains("differs from what Genkan wrote"));
    }

    #[test]
    fn a_request_for_another_record_or_a_malformed_one_is_refused() {
        let message = refused(Fake::new(), |offer| {
            serde_json::to_vec(&RetryRequest {
                retry: offer.stage.clone(),
                record: Digest::of(b"another record").to_string(),
            })
            .unwrap()
        });
        assert!(message.contains("names another installation record"));
        let message = refused(Fake::new(), |offer| {
            serde_json::to_vec(&RetryRequest {
                retry: "storage".into(),
                record: offer.record.clone(),
            })
            .unwrap()
        });
        assert!(message.contains("names another installation record"));
        for line in [
            &b"not json"[..],
            &br#"{"retry":"bootloader"}"#[..],
            &br#"{"confirmation":"ERASE /dev/vda"}"#[..],
        ] {
            assert!(refused(Fake::new(), |_| line.to_vec()).contains("not a retry request"));
        }
    }

    #[test]
    fn a_record_changed_on_disk_is_refused_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let plan = Plan::new();
        let mut journal = journal(&plan, dir.path());
        let live = journal.live_path().to_path_buf();
        let mut changed = None;
        let (result, _, attempts) = run(&mut journal, 1, Fake::new(), |offer| {
            // Between the offer and the answer, the record is replaced by
            // one that still chains: the same entries, one dropped.
            let bytes = fs::read(&live).unwrap();
            let text = String::from_utf8(bytes).unwrap();
            let shorter: String = text.lines().take(9).map(|l| format!("{l}\n")).collect();
            Record::parse(shorter.as_bytes()).unwrap();
            fs::write(&live, &shorter).unwrap();
            changed = Some(shorter);
            Some(answer(offer))
        });
        assert_eq!(attempts, 1);
        assert!(format!("{:#}", result.unwrap_err()).contains("changed outside Genkan"));
        // The changed copy is evidence: never written over.
        assert_eq!(fs::read_to_string(&live).unwrap(), changed.unwrap());
    }

    #[test]
    fn a_record_for_another_plan_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let plan = Plan::new();
        let mut journal = journal(&plan, dir.path());
        let record = journal.reread().unwrap();
        let mut other = Plan::new();
        other.disk.serial = "ANOTHER_DISK".into();
        let error = recheck(&record, &other.expected(), &mut Fake::new()).unwrap_err();
        assert!(format!("{error:#}").contains("for another disk"));
        let mut other = Plan::new();
        other.layout =
            Layout::for_install(40 * 1024u64.pow(3), Firmware::Uefi, None, false).unwrap();
        let error = recheck(&record, &other.expected(), &mut Fake::new()).unwrap_err();
        assert!(format!("{error:#}").contains("for other partitions"));
        let plan = Plan::new();
        let mut expected = plan.expected();
        expected.files[0] = Digest::of(b"another configuration");
        let error = recheck(&record, &expected, &mut Fake::new()).unwrap_err();
        assert!(format!("{error:#}").contains("another configuration"));
        let mut expected = plan.expected();
        expected.system = "/nix/store/zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz-nixos-system-other";
        let error = recheck(&record, &expected, &mut Fake::new()).unwrap_err();
        assert!(format!("{error:#}").contains("another system"));
        recheck(&record, &plan.expected(), &mut Fake::new()).unwrap();
    }

    #[test]
    fn an_incomplete_record_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let plan = Plan::new();
        let mut journal =
            Journal::open(&dir.path().join("records"), opening_facts(&plan), vec![]).unwrap();
        journal
            .intent(Stage::Preparation, Facts::default())
            .unwrap();
        journal
            .completed(
                Stage::Preparation,
                Facts::default(),
                Verification::Inspected,
            )
            .unwrap();
        let record = journal.reread().unwrap();
        let error = recheck(&record, &plan.expected(), &mut Fake::new()).unwrap_err();
        assert!(format!("{error:#}").contains("no confirmed configuration"));
    }

    #[test]
    fn storage_evidence_follows_encryption_and_firmware() {
        let mut ids = ids();
        ids.encryption = Some(crate::encryption::Identities {
            root: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into(),
            swap: Some("bbbbbbbb-cccc-4ddd-8eee-ffffffffffff".into()),
        });
        ids.efi = None;
        ids.bios_boot = Some("12345678-1234-4567-8901-123456789012".into());
        let disk = identity();
        let layout =
            Layout::for_install(40 * 1024u64.pow(3), Firmware::Bios, Some(1 << 30), true).unwrap();
        let mount = Path::new("/run/calamares-target-test");
        let expected = Expected {
            disk: &disk,
            firmware: Firmware::Bios,
            filesystem: Filesystem::Btrfs,
            layout: &layout,
            ids: &ids,
            mount,
            system: SYSTEM,
            files: CONTENTS.map(Digest::of),
        };
        let mut fake = Fake::new();
        let root = mapping("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee");
        fake.uuids = vec![
            (
                "/dev/vda2".into(),
                "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into(),
            ),
            (root.clone(), ROOT.into()),
            (
                "/dev/vda3".into(),
                "bbbbbbbb-cccc-4ddd-8eee-ffffffffffff".into(),
            ),
            (mapping("bbbbbbbb-cccc-4ddd-8eee-ffffffffffff"), SWAP.into()),
            (
                "/dev/vda4".into(),
                "12345678-1234-4567-8901-123456789012".into(),
            ),
        ];
        fake.mounts = vec![
            (mount.into(), (root.clone(), "btrfs".into())),
            (mount.join("boot"), ("/dev/vda4".into(), "ext4".into())),
        ];
        let facts = storage(&expected, &mut fake).unwrap();
        assert_eq!(facts.get("partitions"), Some("4"));
        assert_eq!(
            facts.get("luks-root-uuid"),
            Some("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee")
        );
        assert_eq!(facts.get("efi-serial"), None);
        assert_eq!(record::layout(&facts).unwrap(), layout);
        // The mapping's filesystem must be the one mounted.
        fake.mounts[0].1.0 = "/dev/vda2".into();
        assert!(storage(&expected, &mut fake).is_err());
    }

    #[test]
    fn closures_are_digested_in_either_json_form() {
        let object = serde_json::json!({
            "/nix/store/b-two": { "narHash": "sha256-two", "narSize": 2 },
            "/nix/store/a-one": { "narHash": "sha256-one", "narSize": 1, "registrationTime": 5 },
        });
        let array = serde_json::json!([
            { "path": "/nix/store/a-one", "narHash": "sha256-one", "narSize": 1 },
            { "path": "/nix/store/b-two", "narHash": "sha256-two", "narSize": 2, "signatures": [] },
        ]);
        let names = serde_json::json!({
            "a-one": { "narHash": "sha256-one", "narSize": 1 },
            "b-two": { "narHash": "sha256-two", "narSize": 2 },
        });
        let digest = closure(&object).unwrap();
        assert_eq!(digest, closure(&array).unwrap());
        assert_eq!(digest, closure(&names).unwrap());
        assert_eq!((digest.paths, digest.bytes), (2, 3));
        let changed = serde_json::json!({
            "/nix/store/a-one": { "narHash": "sha256-changed", "narSize": 1 },
            "/nix/store/b-two": { "narHash": "sha256-two", "narSize": 2 },
        });
        assert_ne!(closure(&changed).unwrap(), digest);
        for invalid in [
            serde_json::json!({}),
            serde_json::json!([]),
            serde_json::json!({ "/nix/store/a-one": null }),
            serde_json::json!({ "/nix/store/a-one": { "narSize": 1 } }),
            serde_json::json!([{ "narHash": "sha256-one", "narSize": 1 }]),
            serde_json::json!("text"),
        ] {
            assert!(closure(&invalid).is_err(), "{invalid}");
        }
    }

    /// Read-only: a store path's closure as this machine's Nix reports it,
    /// parsed the way the copy is checked.
    #[test]
    #[ignore = "needs Nix and a store path: GENKAN_TEST_CLOSURE=/nix/store/…"]
    fn real_closures_parse() {
        let path = std::env::var("GENKAN_TEST_CLOSURE").expect("GENKAN_TEST_CLOSURE");
        let found = Real.closure(None, &path).unwrap();
        assert!(found.paths > 0 && found.bytes > 0);
        assert_eq!(Real.closure(None, &path).unwrap(), found);
        println!(
            "{path}: {} paths, {} bytes, {}",
            found.paths, found.bytes, found.digest
        );
    }

    #[test]
    fn plans_open_records_without_secrets() {
        let (_dir, settings, mut raw) = crate::plan_tests::fixture();
        raw.encrypt = true;
        raw.encryption_password = "a disk passphrase".into();
        raw.git_name = "Ada Lovelace".into();
        raw.git_email = "ada@example.com".into();
        let plan = raw.parse(&settings).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::open(
            &dir.path().join("records"),
            opening(&plan).unwrap(),
            personal(&plan),
        )
        .unwrap();
        let text = fs::read_to_string(journal.live_path()).unwrap();
        for secret in ["long-enough-test-password", "a disk passphrase"] {
            assert!(!text.contains(secret));
        }
        assert!(text.contains("\"encrypted\",\"yes\""));
        assert!(personal(&plan).contains(&"ada@example.com".to_string()));
    }
}
