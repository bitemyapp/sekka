// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use sekka_mac_contract::{DiskChanged, Fact, Failure, Stage, Verification, samples};
use std::cell::Cell;

fn opened() -> Event {
    let plan = samples::plan();
    Event::Opened {
        plan_digest: plan.digest().unwrap(),
        plan: Box::new(plan),
        app: "0.1.0".parse().unwrap(),
        helper: "0.1.0".parse().unwrap(),
    }
}

fn at() -> Timestamp {
    Timestamp::from_unix(1_800_000_000)
}

fn id() -> Uuid {
    "0f0f0f0f-0f0f-4f0f-8f0f-0f0f0f0f0f0f".parse().unwrap()
}

fn completed(stage: Stage) -> Event {
    Event::Completed {
        stage,
        facts: vec![Fact::new("checked", "yes").unwrap()],
        verification: Verification::Inspected,
    }
}

#[test]
fn records_survive_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journal::create(FsStorage, dir.path(), id(), at(), opened()).unwrap();
    j.append(at(), Event::Intent { stage: Stage::Import }).unwrap();
    j.append(at(), completed(Stage::Import)).unwrap();
    let again = Journal::open(FsStorage, dir.path(), id()).unwrap();
    assert_eq!(again.records(), j.records());
    assert_eq!(again.state(), &InstallState::AssetsVerified);
    assert_eq!(installations(&FsStorage, dir.path()).unwrap(), vec![id()]);
    assert_eq!(
        Journal::create(FsStorage, dir.path(), id(), at(), opened()).err(),
        Some(JournalError::Exists)
    );
}

#[test]
fn impossible_histories_are_refused_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journal::create(FsStorage, dir.path(), id(), at(), opened()).unwrap();
    assert!(matches!(
        j.append(at(), Event::Intent { stage: Stage::Root }),
        Err(JournalError::Refused(_))
    ));
    assert_eq!(j.records().len(), 1);
    assert_eq!(Journal::open(FsStorage, dir.path(), id()).unwrap().records().len(), 1);
}

#[test]
fn damage_is_detected_not_repaired() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journal::create(FsStorage, dir.path(), id(), at(), opened()).unwrap();
    j.append(at(), Event::Intent { stage: Stage::Import }).unwrap();
    j.append(at(), completed(Stage::Import)).unwrap();
    let records = Journal::<FsStorage>::directory(dir.path(), id());

    // A record edited in place.
    let path = records.join("00000002.json");
    let original = fs::read(&path).unwrap();
    let edited = String::from_utf8(original.clone()).unwrap().replace("import", "shrink");
    fs::write(&path, edited).unwrap();
    assert!(matches!(
        Journal::open(FsStorage, dir.path(), id()),
        Err(JournalError::Damaged(_))
    ));
    fs::write(&path, &original).unwrap();

    // A record truncated.
    fs::write(&path, &original[..original.len() / 2]).unwrap();
    assert!(matches!(
        Journal::open(FsStorage, dir.path(), id()),
        Err(JournalError::Damaged(_))
    ));
    fs::write(&path, &original).unwrap();

    // A record missing from the middle.
    fs::rename(&path, records.join("moved")).unwrap();
    assert!(Journal::open(FsStorage, dir.path(), id()).is_err());
    fs::rename(records.join("moved"), &path).unwrap();

    // A record from another installation's journal substituted.
    let other: Uuid = "1f0f0f0f-0f0f-4f0f-8f0f-0f0f0f0f0f0f".parse().unwrap();
    let mut o = Journal::create(FsStorage, dir.path(), other, at(), opened()).unwrap();
    o.append(at(), Event::Intent { stage: Stage::Import }).unwrap();
    let foreign = fs::read(Journal::<FsStorage>::directory(dir.path(), other).join("00000002.json")).unwrap();
    fs::write(&path, foreign).unwrap();
    assert!(Journal::open(FsStorage, dir.path(), id()).is_err());
    fs::write(&path, &original).unwrap();

    assert!(Journal::open(FsStorage, dir.path(), id()).is_ok());
}

/// Fails the `fail_at`-th storage operation in one of several ways, as a
/// crash or a full disk would at that moment.
struct Faulty {
    count: Cell<usize>,
    fail_at: usize,
    mode: Fault,
}
#[derive(Clone, Copy, Debug)]
enum Fault {
    /// The operation fails and has no effect (ENOSPC, EIO).
    Fail,
    /// A write stops half way, as a power loss would leave it.
    Short,
    /// The operation takes effect, then the process dies before hearing so.
    AfterEffect,
}
impl Faulty {
    fn tick(&self) -> Option<Fault> {
        let n = self.count.get();
        self.count.set(n + 1);
        (n == self.fail_at).then_some(self.mode)
    }
    fn err() -> io::Error {
        io::Error::other("injected fault")
    }
}
impl Storage for Faulty {
    fn create_dir(&self, path: &Path) -> io::Result<()> {
        match self.tick() {
            Some(Fault::AfterEffect) => FsStorage.create_dir(path).and(Err(Self::err())),
            Some(_) => Err(Self::err()),
            None => FsStorage.create_dir(path),
        }
    }
    fn list(&self, dir: &Path) -> io::Result<Vec<String>> {
        FsStorage.list(dir)
    }
    fn read(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        FsStorage.read(path, limit)
    }
    fn write_new(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        match self.tick() {
            Some(Fault::Fail) => Err(io::Error::from_raw_os_error(28)),
            Some(Fault::Short) => {
                fs::write(path, &bytes[..bytes.len() / 2])?;
                Err(Self::err())
            }
            Some(Fault::AfterEffect) => FsStorage.write_new(path, bytes).and(Err(Self::err())),
            None => FsStorage.write_new(path, bytes),
        }
    }
    fn link(&self, from: &Path, to: &Path) -> io::Result<()> {
        match self.tick() {
            Some(Fault::AfterEffect) => FsStorage.link(from, to).and(Err(Self::err())),
            Some(_) => Err(Self::err()),
            None => FsStorage.link(from, to),
        }
    }
    fn remove(&self, path: &Path) -> io::Result<()> {
        match self.tick() {
            Some(Fault::AfterEffect) => FsStorage.remove(path).and(Err(Self::err())),
            Some(_) => Err(Self::err()),
            None => FsStorage.remove(path),
        }
    }
    fn sync_dir(&self, dir: &Path) -> io::Result<()> {
        match self.tick() {
            Some(Fault::AfterEffect) => FsStorage.sync_dir(dir).and(Err(Self::err())),
            Some(_) => Err(Self::err()),
            None => FsStorage.sync_dir(dir),
        }
    }
}

/// At every storage operation of an append, in every failure mode, a
/// journal reopened afterwards is either what it was or what it was plus
/// the new record — and appending again then works.
#[test]
fn every_interruption_leaves_a_believable_journal() {
    for mode in [Fault::Fail, Fault::Short, Fault::AfterEffect] {
        for fail_at in 0..6 {
            let dir = tempfile::tempdir().unwrap();
            let mut j = Journal::create(FsStorage, dir.path(), id(), at(), opened()).unwrap();
            j.append(at(), Event::Intent { stage: Stage::Import }).unwrap();
            let before = j.records().to_vec();

            let mut faulty = Journal::open(
                Faulty {
                    count: Cell::new(0),
                    fail_at,
                    mode,
                },
                dir.path(),
                id(),
            )
            .unwrap();
            let result = faulty.append(at(), completed(Stage::Import)).cloned();
            if result.is_err() {
                assert_eq!(faulty.records(), &before[..], "{mode:?}@{fail_at}: memory unchanged");
            }

            let reopened = Journal::open(FsStorage, dir.path(), id())
                .unwrap_or_else(|e| panic!("{mode:?}@{fail_at}: {e}"));
            let n = reopened.records().len();
            assert!(
                n == before.len() || n == before.len() + 1,
                "{mode:?}@{fail_at}: {n} records"
            );
            assert_eq!(&reopened.records()[..before.len()], &before[..]);
            if let Ok(record) = &result {
                assert_eq!(reopened.records().last(), Some(record), "{mode:?}@{fail_at}: an acknowledged record is durable");
            }

            let mut reopened = reopened;
            if n == before.len() {
                reopened.append(at(), completed(Stage::Import)).unwrap();
            }
            reopened.append(at(), Event::Intent { stage: Stage::AppleInputs }).unwrap();
            assert_eq!(
                Journal::open(FsStorage, dir.path(), id()).unwrap().records().len(),
                before.len() + 2
            );
        }
    }
}

#[test]
fn a_journal_that_never_began_is_taken_away() {
    let dir = tempfile::tempdir().unwrap();
    for fail_at in 0..4 {
        let result = Journal::create(
            Faulty {
                count: Cell::new(0),
                fail_at,
                mode: Fault::Fail,
            },
            dir.path(),
            id(),
            at(),
            opened(),
        );
        if result.is_err() {
            assert!(
                installations(&FsStorage, dir.path()).unwrap().is_empty()
                    || Journal::open(FsStorage, dir.path(), id()).is_ok(),
                "fault {fail_at}: either no journal or a whole one"
            );
        }
        let _ = fs::remove_dir_all(Journal::<FsStorage>::directory(dir.path(), id()));
    }
    // A directory left by a crash with only a pending write is removed;
    // one with a record is never touched.
    let left = Journal::<FsStorage>::directory(dir.path(), id());
    fs::create_dir_all(&left).unwrap();
    fs::write(left.join(".pending-00000001.json"), b"{").unwrap();
    assert!(discard_if_unopened(&FsStorage, dir.path(), id()));
    assert!(!left.exists());
    Journal::create(FsStorage, dir.path(), id(), at(), opened()).unwrap();
    assert!(!discard_if_unopened(&FsStorage, dir.path(), id()));
    assert!(Journal::open(FsStorage, dir.path(), id()).is_ok());
}

#[test]
fn another_copy_may_lag_the_helpers_own_but_never_lead_or_differ() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journal::create(FsStorage, dir.path(), id(), at(), opened()).unwrap();
    j.append(at(), Event::Intent { stage: Stage::Import }).unwrap();
    let short = j.records().to_vec();
    j.append(at(), completed(Stage::Import)).unwrap();
    let long = j.records().to_vec();
    assert_eq!(reconcile(&long, &short).unwrap(), long);
    assert_eq!(reconcile(&long, &long).unwrap(), long);
    // A copy that goes further is refused, however well its records chain:
    // nothing authenticates what it adds.
    assert!(matches!(reconcile(&short, &long), Err(JournalError::Refused(_))));

    let dir2 = tempfile::tempdir().unwrap();
    let mut k = Journal::create(FsStorage, dir2.path(), id(), at(), opened()).unwrap();
    k.append(at(), Event::Intent { stage: Stage::Import }).unwrap();
    k.append(
        at(),
        Event::Failed {
            stage: Stage::Import,
            failure: Failure {
                code: "download".parse().unwrap(),
                message: "A download didn't match.".parse().unwrap(),
                apple_diagnostic: None,
                disk_changed: DiskChanged::No,
            },
        },
    )
    .unwrap();
    assert!(reconcile(&long, k.records()).is_err());
}

#[test]
fn one_installation_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lock");
    let first = InstallLock::try_acquire(&path).unwrap();
    assert!(first.is_some());
    assert!(InstallLock::try_acquire(&path).unwrap().is_none());
    drop(first);
    assert!(InstallLock::try_acquire(&path).unwrap().is_some());
}
