// SPDX-License-Identifier: GPL-3.0-or-later
//! The installation journal: records written so that a power loss at any
//! moment leaves either the journal as it was or the journal with one more
//! complete record, and never anything in between that is believed.
//!
//! An installation's records live in their own directory, one file per
//! record, named by sequence number (`00000001.json`). A record is written
//! to a temporary file, synchronized, then published under its final name
//! with a hard link (which refuses to replace anything), and the directory
//! is synchronized. Its canonical bytes carry the previous record's digest,
//! so a reader checks the whole chain: a missing, torn, reordered or
//! substituted record makes the journal unreadable, which the helper treats
//! as "stop and reconcile by hand", never as permission to start over.
//!
//! Before writing, [`Journal::append`] checks that the new record is a
//! transition [`InstallState::observe`] allows, so the journal can't record
//! an impossible history even if its caller tries.
#![forbid(unsafe_code)]

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use sekka_mac_contract::{
    Event, InstallState, Record, RecordSchema, Timestamp, Uuid, canonical, limits,
};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum JournalError {
    #[error("the installation record couldn't be written or read: {0}")]
    Io(String),
    #[error("the installation record is damaged: {0}")]
    Damaged(String),
    #[error("{0}")]
    Refused(String),
    #[error("an installation with this identity already has a record")]
    Exists,
}

impl From<io::Error> for JournalError {
    fn from(e: io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

/// The file operations the journal needs, so tests can fail any of them.
pub trait Storage {
    fn create_dir(&self, path: &Path) -> io::Result<()>;
    fn list(&self, dir: &Path) -> io::Result<Vec<String>>;
    fn read(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>>;
    /// Create a new file (never an existing one), write all of `bytes`,
    /// and synchronize it.
    fn write_new(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    /// Give `from` the name `to` without replacing anything at `to`.
    fn link(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove(&self, path: &Path) -> io::Result<()>;
    fn sync_dir(&self, dir: &Path) -> io::Result<()>;
}

/// The real file system.
#[derive(Clone, Copy, Debug, Default)]
pub struct FsStorage;

impl Storage for FsStorage {
    fn create_dir(&self, path: &Path) -> io::Result<()> {
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(path)
    }
    fn list(&self, dir: &Path) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in fs::read_dir(dir)? {
            names.push(entry?.file_name().to_string_lossy().into_owned());
        }
        names.sort();
        Ok(names)
    }
    fn read(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        let mut f = open_regular(path)?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut f).take(limit as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "record too large"));
        }
        Ok(bytes)
    }
    fn write_new(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut f = options.open(path)?;
        f.write_all(bytes)?;
        f.sync_all()
    }
    fn link(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::hard_link(from, to)
    }
    fn remove(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }
    fn sync_dir(&self, dir: &Path) -> io::Result<()> {
        File::open(dir)?.sync_all()
    }
}

/// Open a regular file without following a symbolic link at its last
/// component.
fn open_regular(path: &Path) -> io::Result<File> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.file_type().is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a regular file"));
    }
    File::open(path)
}

fn record_name(sequence: u64) -> String {
    format!("{sequence:08}.json")
}

fn parse_record_name(name: &str) -> Option<u64> {
    let digits = name.strip_suffix(".json")?;
    (digits.len() == 8 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

pub struct Journal<S: Storage = FsStorage> {
    storage: S,
    dir: PathBuf,
    installation: Uuid,
    records: Vec<Record>,
    state: InstallState,
}

impl<S: Storage> Journal<S> {
    /// The directory an installation's records live in.
    pub fn directory(root: &Path, installation: Uuid) -> PathBuf {
        root.join(installation.to_string())
    }

    /// Start a journal with its opening record.
    pub fn create(
        storage: S,
        root: &Path,
        installation: Uuid,
        at: Timestamp,
        opened: Event,
    ) -> Result<Self, JournalError> {
        if !matches!(opened, Event::Opened { .. }) {
            return Err(JournalError::Refused("a journal starts with its opening record".into()));
        }
        let dir = Self::directory(root, installation);
        match storage.create_dir(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Err(JournalError::Exists),
            Err(e) => return Err(e.into()),
        }
        storage.sync_dir(root)?;
        let mut journal = Self {
            storage,
            dir,
            installation,
            records: Vec::new(),
            state: InstallState::Planned,
        };
        journal.append(at, opened)?;
        Ok(journal)
    }

    /// Read an installation's journal and check all of it.
    pub fn open(storage: S, root: &Path, installation: Uuid) -> Result<Self, JournalError> {
        let dir = Self::directory(root, installation);
        let mut sequences = Vec::new();
        for name in storage.list(&dir)? {
            if let Some(seq) = parse_record_name(&name) {
                sequences.push(seq);
            } else if name.starts_with(".pending-") {
                // An unpublished write from before a crash: never part of
                // the journal. Leave it for diagnostics; it is replaced if
                // the same sequence is written again.
            } else {
                return Err(JournalError::Damaged(format!("unexpected file {name}")));
            }
        }
        sequences.sort_unstable();
        if sequences.first() != Some(&1) || sequences.windows(2).any(|w| w[1] != w[0] + 1) {
            return Err(JournalError::Damaged(
                "records are missing from the sequence".into(),
            ));
        }
        let mut records: Vec<Record> = Vec::with_capacity(sequences.len());
        for seq in sequences {
            let bytes = storage.read(&dir.join(record_name(seq)), limits::RECORD)?;
            let record: Record = canonical::from_slice(&bytes, limits::RECORD, RecordSchema::VALUE)
                .map_err(|e| JournalError::Damaged(format!("record {seq}: {e}")))?;
            if record.sequence != seq || record.installation != installation {
                return Err(JournalError::Damaged(format!(
                    "record {seq} names another place or installation"
                )));
            }
            match records.last() {
                Some(previous) => previous
                    .check_successor(&record)
                    .map_err(|e| JournalError::Damaged(e.to_string()))?,
                None if record.previous.is_some() => {
                    return Err(JournalError::Damaged("the first record has a predecessor".into()));
                }
                None => {}
            }
            records.push(record);
        }
        let state =
            InstallState::observe(&records).map_err(|e| JournalError::Damaged(e.to_string()))?;
        Ok(Self {
            storage,
            dir,
            installation,
            records,
            state,
        })
    }

    pub fn installation(&self) -> Uuid {
        self.installation
    }
    pub fn records(&self) -> &[Record] {
        &self.records
    }
    pub fn state(&self) -> &InstallState {
        &self.state
    }

    /// Durably add a record. When this returns `Ok`, the record survives a
    /// power loss; when it returns an error, the journal in memory is
    /// unchanged and the record may or may not be on disk — reopen the
    /// journal to find out before doing anything else.
    pub fn append(&mut self, at: Timestamp, event: Event) -> Result<&Record, JournalError> {
        let previous = match self.records.last() {
            Some(r) => Some(r.digest().map_err(|e| JournalError::Damaged(e.to_string()))?),
            None => None,
        };
        let record = Record {
            schema: RecordSchema,
            installation: self.installation,
            sequence: self.records.len() as u64 + 1,
            previous,
            at,
            event,
        };
        let mut next = self.records.clone();
        next.push(record.clone());
        let state = InstallState::observe(&next).map_err(|e| JournalError::Refused(e.to_string()))?;
        let bytes = record
            .bytes()
            .map_err(|e| JournalError::Refused(e.to_string()))?;
        if bytes.len() > limits::RECORD {
            return Err(JournalError::Refused("the record is too large".into()));
        }

        let name = record_name(record.sequence);
        let pending = self.dir.join(format!(".pending-{name}"));
        // A pending file left by an earlier crash at this sequence was never
        // published; it is safe to replace.
        match self.storage.remove(&pending) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        self.storage.write_new(&pending, &bytes)?;
        self.storage.link(&pending, &self.dir.join(&name))?;
        self.storage.sync_dir(&self.dir)?;
        let _ = self.storage.remove(&pending);

        self.records = next;
        self.state = state;
        Ok(self.records.last().expect("just pushed"))
    }
}

/// Every installation with a journal under `root`.
pub fn installations(storage: &impl Storage, root: &Path) -> Result<Vec<Uuid>, JournalError> {
    let mut found = Vec::new();
    let names = match storage.list(root) {
        Ok(names) => names,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(found),
        Err(e) => return Err(e.into()),
    };
    for name in names {
        if let Ok(id) = name.parse::<Uuid>() {
            found.push(id);
        }
    }
    Ok(found)
}

/// Two copies of one installation's records (the helper's, and the copy
/// Recovery extends on the stub) agree only if one is a prefix of the
/// other, record for record. The longer one is then the history; any
/// divergence means one of them can't be trusted and nothing is
/// reconciled automatically. Taking whichever copy claims more progress
/// would be unsafe, so this never does.
pub fn reconcile(a: &[Record], b: &[Record]) -> Result<Vec<Record>, JournalError> {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    if short.iter().zip(long).any(|(x, y)| x != y) {
        return Err(JournalError::Damaged(
            "the two copies of the installation record disagree".into(),
        ));
    }
    Ok(long.to_vec())
}

/// The lock that makes the helper run one installation at a time. Held for
/// as long as the value lives; the operating system releases it if the
/// process dies.
pub struct InstallLock {
    _file: File,
}

impl InstallLock {
    pub fn try_acquire(path: &Path) -> Result<Option<Self>, JournalError> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(fs::TryLockError::WouldBlock) => Ok(None),
            Err(fs::TryLockError::Error(e)) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests;
