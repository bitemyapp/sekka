// SPDX-License-Identifier: GPL-3.0-or-later
//! The Mac qualification ledger: one row per required case of gates M0 to
//! M9 in docs/apple-silicon-installer-plan.md, for one frozen candidate.
//!
//! An untested case is never a pass. Every row starts `not-run`; `fail`,
//! `blocked` and `not-applicable` need a concrete reason, and `pass` needs
//! what was observed and where the evidence is. A candidate is complete
//! only when every row is `pass` or `not-applicable`. The rows are exactly
//! this revision's cases: none dropped, repeated, added or reworded.
use serde::{Deserialize, Serialize};

use sekka_mac_contract::{Digest, GitRevision, Text, Timestamp, schema_tag};

schema_tag!(LedgerSchema, "sekka.mac.ledger/1");

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ledger {
    pub schema: LedgerSchema,
    pub candidate: Candidate,
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub source_revision: GitRevision,
    pub lock: Digest,
    /// Every artifact the owner would receive, by name, size and digest.
    pub artifacts: Vec<CandidateArtifact>,
    pub created: Timestamp,
    /// The toolchains and hosts the candidate was built and checked with.
    pub toolchain: Text,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateArtifact {
    pub name: Text,
    pub size: u64,
    pub digest: Digest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    NotRun,
    Running,
    Pass,
    Fail,
    Blocked,
    NotApplicable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub id: String,
    pub gate: String,
    pub case: String,
    /// What counts as evidence, and what it can't show.
    pub limit: String,
    pub status: Status,
    pub observed: String,
    pub reason: String,
    pub evidence: String,
    pub issue: String,
}

/// (id, case, evidence and limit).
type Case = (&'static str, &'static str, &'static str);

const COMPONENT: &str = "Component tests and recorded fixtures; no claim about macOS APIs or physical disk safety.";
const IMAGE: &str = "Build-time inspection and two boots of the exact packaged image in a disposable VM; fixture firmware is labeled.";
const LINUX: &str = "Normal UI input with inspected screenshots and logs, not commands that bypass the greeter.";
const NATIVE: &str = "Recorded macOS toolchain and real app captures; Linux checks don't cover this gate.";
const RECOVERY: &str = "Owner-operated, exact-candidate J514S evidence for real APFS and Apple policy; fixtures only where representative.";
const HARDWARE: &str = "Exact model and firmware; record success, error or limitation honestly, including a missing device or unsupported port.";
const UPDATES: &str = "Before/after revisions, source and lock, boot selections, installed files and persisted settings; no production branch manipulation.";
const PC: &str = "Follow Q0 to Q8 of the ISO skill; component checks can't certify a changed ISO.";
const PROMOTION: &str = "A report for the complete candidate set; no support claim from a build or signed catalog alone.";

pub const GATES: &[(&str, &str, &[Case])] = &[
    ("M0", COMPONENT, &[
        ("M0-01", "Documents of an unknown schema or version are refused by the app, helper, engine and Recovery.", ""),
        ("M0-02", "Catalogs with a bad signature, an unknown key, the wrong channel, an expired date or an older serial are refused; a clock that is behind is reported as such.", ""),
        ("M0-03", "Another board, another chip and untested system firmware are refused before any password is asked for.", ""),
        ("M0-04", "Swapped, truncated and substituted downloads are refused by the app's stager and again by the helper's import.", ""),
        ("M0-05", "A layout that changed after review makes the helper and the engine refuse the confirmed plan.", ""),
        ("M0-06", "Torn, reordered, edited, foreign and forged journal records are refused, never repaired.", ""),
        ("M0-07", "A second installation request while one runs, or while an earlier one needs reconciling, is refused.", ""),
        ("M0-08", "Existing free space is planned without a resize; too little space, a Mac that isn't ready (on battery, FileVault converting) and an existing or partial Sekka stop before any plan.", ""),
        ("M0-09", "The app asks again in place for a refused password, reconnects to an unfinished installation instead of starting another, never retries an interrupted disk stage, and retries only the startup disk.", ""),
    ]),
    ("M1", IMAGE, &[
        ("M1-01", "The image's store is registered exactly once and holds exactly the system's closure.", ""),
        ("M1-02", "/etc/nixos evaluates offline with its shipped lock to the image's own system.", ""),
        ("M1-03", "The required closure is present in the image's actual bytes.", ""),
        ("M1-04", "The ESP and boot files match the manifest's inventory.", ""),
        ("M1-05", "The root filesystem grows into its partition.", ""),
        ("M1-06", "With two byte-identical images on two disks, each boot mounts its own root and ESP.", ""),
        ("M1-07", "A second boot changes nothing first boot already did.", ""),
        ("M1-08", "A no-change rebuild, a controlled update and a rollback all work.", ""),
    ]),
    ("M2", LINUX, &[
        ("M2-01", "Setup on the greeter creates the owner's account with their choices, offline.", ""),
        ("M2-02", "A wrong password is refused at login; the right one opens Aizome.", ""),
        ("M2-03", "Lock and unlock, and log out.", ""),
        ("M2-04", "Tatami can be chosen at the greeter and starts.", ""),
        ("M2-05", "Locale, keyboard, time zone and host name persist after a reboot.", ""),
        ("M2-06", "No usable default password exists; no credential appears in the store, the journals or an export.", ""),
        ("M2-07", "Every advertised app and setting opens and works (inventory attached).", ""),
        ("M2-08", "Network settings persist across a reboot.", ""),
    ]),
    ("M3", NATIVE, &[
        ("M3-01", "The workspace builds and its tests pass on macOS in debug and release, with clippy clean.", ""),
        ("M3-02", "The helper refuses an app with another signature, and the app refuses a helper with another.", ""),
        ("M3-03", "SMAppService registration, approval in Login Items, and an app update that keeps unfinished records.", ""),
        ("M3-04", "Every simulated scenario's screens, light and dark, captured with --screenshots and reviewed.", ""),
        ("M3-05", "VoiceOver, keyboard-only use, larger text and the smallest window.", ""),
        ("M3-06", "The catalog and the downloads through NSURLSession offline and through a proxy; downloads with a full disk, cancelled and resumed.", ""),
    ]),
    ("M4", RECOVERY, &[
        ("M4-01", "The J514S inspects as supported; another Mac and untested firmware are refused.", ""),
        ("M4-02", "APFS resize limits are shown; a resize failure keeps Apple's message and reports the disk unchanged.", ""),
        ("M4-03", "Payload and firmware are written and read back.", ""),
        ("M4-04", "The handoff and Recovery configuration bind the exact stub, ESP and root.", ""),
        ("M4-05", "Paired Recovery opens Finish Installing Sekka by itself.", ""),
        ("M4-06", "Enrollment with the owner's password, a wrong password retried in place, and startup selection read back.", ""),
        ("M4-07", "A Recovery step interrupted by a restart resumes without repeating a completed step.", ""),
        ("M4-08", "Before any disk change the run's controls are in place and recorded: a verified backup and written restore route, macOS starting normally and its Recovery reachable from startup options, and disposable credentials for every account the run creates.", ""),
        ("M4-09", "An installation with FileVault on and one with it off, each with fdesetup status recorded before and after; macOS stays encrypted when it was.", ""),
        ("M4-10", "An installation into existing free space: no resize is offered or run, macOS's container is unchanged, and the new partitions use only that space.", ""),
        ("M4-11", "With too little space the app says why, offers no plan, asks for no password and changes nothing.", ""),
        ("M4-12", "A Mac that isn't ready (on battery, FileVault converting) is told why and what to do, and nothing is offered until Check Again finds it ready.", ""),
        ("M4-13", "A wrong administrator password at Install is refused by the helper before anything changes and asked for again in place; the right one starts the installation.", ""),
        ("M4-14", "A Mac that already has Sekka, complete or partial, is recognized: the app says what it found and offers no new installation.", ""),
        ("M4-15", "Quitting the app while the helper installs, and restarting the Mac after macOS's part, both reopen that installation at its next step, never a new one.", "A restart in the middle of a disk stage is a separately reviewed interruption test, not this row."),
        ("M4-16", "A disk stage that fails after the disk changed stops with the stage named and what changed, offers only what the journal allows, and never repeats the stage.", "If it can't be induced safely, record it as blocked or not-applicable with the reason; the row is never dropped."),
        ("M4-17", "After macOS's startup-disk stage fails, Set Startup Disk Again checks the target again and runs only the startup-disk stage; the stub then starts into Recovery and opens Finish Installing Sekka.", "If it can't be induced safely, record it as blocked or not-applicable with the reason; the row is never dropped."),
    ]),
    ("M5", HARDWARE, &[
        ("M5-01", "m1n1, U-Boot and systemd-boot start the installed system.", ""),
        ("M5-02", "The built-in panel runs at its native resolution and refresh rate; brightness works.", ""),
        ("M5-03", "The GPU renders the desktop; software rendering works when the GPU is turned off.", ""),
        ("M5-04", "Keyboard and trackpad.", ""),
        ("M5-05", "Wi-Fi and Bluetooth with the firmware this installation extracted.", ""),
        ("M5-06", "Speakers through speakersafetyd, and headphones.", ""),
        ("M5-07", "Camera and video playback.", ""),
        ("M5-08", "Each USB-C port, and an external display.", ""),
        ("M5-09", "Lid close, sleep and wake.", ""),
        ("M5-10", "Cold boot and restart.", ""),
    ]),
    ("M6", UPDATES, &[
        ("M6-01", "After setup, the system works offline with its defaults.", ""),
        ("M6-02", "A no-change rebuild on the Mac.", ""),
        ("M6-03", "An update through Yukimi's window and its command line, including a network failure and retry.", ""),
        ("M6-04", "Every aarch64 path the update needs is in the binary cache.", ""),
        ("M6-05", "The new generation boots; the previous one can be chosen and boots.", ""),
        ("M6-06", "Starting macOS and returning to Sekka; reaching Sekka's paired Recovery again.", ""),
    ]),
    ("M7", "Required before encryption is offered; an explicitly deferred phase is not a passing encrypted installation.", &[
        ("M7-01", "Encryption: provisioning, interruption, passphrases at the real initrd, cold boot, update, rollback and recovery.", ""),
    ]),
    ("M8", PC, &[
        ("M8-01", "Genkan's backend, window and component checks.", ""),
        ("M8-02", "The storage and encryption matrix in disposable VMs, UEFI and BIOS.", ""),
        ("M8-03", "An installation through the ISO's window, disk-only boot, the desktops and an update, on the exact ISO.", ""),
    ]),
    ("M9", PROMOTION, &[
        ("M9-01", "Every artifact's bytes match the candidate's record and the catalog's bindings.", ""),
        ("M9-02", "An independent agent audits all the evidence.", ""),
        ("M9-03", "Signatures and notarization of every Mach-O, archives included.", ""),
        ("M9-04", "A dry run of the uploader against the exact candidate.", ""),
        ("M9-05", "After authorized publication only: the public bytes, manifests and download page verified independently.", ""),
    ]),
];

pub fn rows() -> Vec<Row> {
    let mut rows = Vec::new();
    for (gate, limit, cases) in GATES {
        for (id, case, extra) in *cases {
            rows.push(Row {
                id: id.to_string(),
                gate: gate.to_string(),
                case: case.to_string(),
                limit: if extra.is_empty() { limit.to_string() } else { format!("{limit} {extra}") },
                status: Status::NotRun,
                observed: String::new(),
                reason: String::new(),
                evidence: String::new(),
                issue: String::new(),
            });
        }
    }
    rows
}

/// Problems with a ledger, and whether it is complete.
pub fn check(ledger: &Ledger) -> (Vec<String>, bool) {
    let mut problems = Vec::new();
    let expected = rows();
    let found: Vec<&str> = ledger.rows.iter().map(|r| r.id.as_str()).collect();
    for e in &expected {
        if !found.contains(&e.id.as_str()) {
            problems.push(format!("{} is missing: no required case may be dropped", e.id));
        }
    }
    for (i, r) in ledger.rows.iter().enumerate() {
        if found[..i].contains(&r.id.as_str()) {
            problems.push(format!("{} appears twice", r.id));
        }
        match expected.iter().find(|e| e.id == r.id) {
            None => problems.push(format!("{} isn't a required case of this revision's ledger", r.id)),
            Some(e) if (&e.gate, &e.case, &e.limit) != (&r.gate, &r.case, &r.limit) => {
                problems.push(format!("{} doesn't say what the required case says: start the ledger from this revision's tool", r.id));
            }
            Some(_) => {}
        }
        match r.status {
            Status::Pass if r.observed.trim().is_empty() || r.evidence.trim().is_empty() => {
                problems.push(format!("{} passes without what was observed and where the evidence is", r.id));
            }
            Status::Fail | Status::Blocked | Status::NotApplicable if r.reason.trim().is_empty() => {
                problems.push(format!("{} is {:?} without a reason", r.id, r.status));
            }
            Status::Fail if r.issue.trim().is_empty() => {
                problems.push(format!("{} failed without an issue or pull request", r.id));
            }
            _ => {}
        }
    }
    let complete = problems.is_empty()
        && ledger
            .rows
            .iter()
            .all(|r| matches!(r.status, Status::Pass | Status::NotApplicable));
    (problems, complete)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger() -> Ledger {
        Ledger {
            schema: LedgerSchema,
            candidate: Candidate {
                source_revision: "4882dc33652612e7475f467f6373d8b607e5407c".parse().unwrap(),
                lock: Digest::of(b"lock"),
                artifacts: vec![],
                created: Timestamp::from_unix(0),
                toolchain: "test".parse().unwrap(),
            },
            rows: rows(),
        }
    }

    #[test]
    fn a_new_ledger_is_valid_and_incomplete() {
        let (problems, complete) = check(&ledger());
        assert!(problems.is_empty(), "{problems:?}");
        assert!(!complete);
        assert!(ledger().rows.len() > 50);
    }

    #[test]
    fn passes_need_evidence_and_failures_need_reasons() {
        let mut l = ledger();
        l.rows[0].status = Status::Pass;
        l.rows[1].status = Status::Blocked;
        l.rows[2].status = Status::Fail;
        l.rows[2].reason = "refused the wrong board too late".into();
        let (problems, _) = check(&l);
        assert_eq!(problems.len(), 3, "{problems:?}");
        l.rows.remove(5);
        assert!(check(&l).0.iter().any(|p| p.contains("missing")));
    }

    #[test]
    fn cases_are_numbered_in_order_within_their_gate() {
        for (gate, _, cases) in GATES {
            for (n, (id, case, _)) in cases.iter().enumerate() {
                assert_eq!(*id, format!("{gate}-{:02}", n + 1));
                assert!(!case.is_empty());
            }
        }
    }

    /// The plan's known controls and the paths the app offers each have a
    /// row on the J514S, not only a simulated screen.
    #[test]
    fn controls_and_offered_paths_have_hardware_rows() {
        let m4: Vec<String> = rows().into_iter().filter(|r| r.gate == "M4").map(|r| r.case).collect();
        for words in [
            "verified backup",
            "FileVault on and one with it off",
            "existing free space",
            "too little space",
            "isn't ready",
            "wrong administrator password",
            "already has Sekka, complete or partial",
            "reopen that installation",
            "disk stage that fails",
            "Set Startup Disk Again",
        ] {
            assert!(m4.iter().any(|c| c.contains(words)), "no M4 row for {words:?}");
        }
    }

    #[test]
    fn rows_that_are_not_the_required_cases_are_refused() {
        let mut l = ledger();
        l.rows[3].case = "Something easier.".into();
        let mut extra = l.rows[0].clone();
        extra.id = "M0-99".into();
        l.rows.push(extra);
        let (problems, complete) = check(&l);
        assert!(!complete);
        assert!(problems.iter().any(|p| p.starts_with("M0-04 doesn't say")), "{problems:?}");
        assert!(problems.iter().any(|p| p.starts_with("M0-99 isn't a required case")), "{problems:?}");
    }

    #[test]
    fn complete_means_every_case_passed_or_does_not_apply() {
        let mut l = ledger();
        for r in &mut l.rows {
            r.status = Status::Pass;
            r.observed = "as expected".into();
            r.evidence = "runs/1".into();
        }
        let m7 = l.rows.iter_mut().find(|r| r.gate == "M7").unwrap();
        m7.status = Status::NotApplicable;
        m7.reason = "Encryption is deferred: docs/apple-silicon/encryption.md".into();
        assert_eq!(check(&l), (vec![], true));
    }
}
