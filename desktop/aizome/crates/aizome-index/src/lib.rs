//! What's in folders and drives, kept in one SQLite database that every
//! Aizome app shares (`$XDG_CACHE_HOME/aizome/index/files.sqlite`), so a
//! drive one app has read is known to the others, and a second look reads
//! only what changed. Chanoma finds games with it.
//!
//! - **Refreshing** a folder ([`Index::refresh`]) walks it, but lists a
//!   folder again only when its modified time has changed since it was
//!   listed: adding, removing or renaming something in a folder changes
//!   the folder's time. A change deeper down doesn't change the folders
//!   above it, so every folder is still looked at (one `stat` each), but
//!   files aren't, and nothing unchanged is read. Files are named and told
//!   apart from folders by the listing alone; their size and time are read
//!   only when asked for ([`Refresh::stat`]).
//! - **A change in the same tick** as a listing could hide behind an
//!   unchanged time, so a folder listed within that tick of its time (two
//!   seconds on FAT and exFAT, whose clocks are that coarse) is listed
//!   again next time, as Git does with its index.
//! - **Drives** are known by their filesystem's UUID ([`volume`]), and
//!   paths by where they are on it, so a drive plugged in again elsewhere
//!   is the same drive. Refreshing stays on one filesystem and follows no
//!   links.
//! - **Not read**: folders whose names start with a dot (configuration and
//!   caches), folders holding a `CACHEDIR.TAG`, the Trash, `.git` and
//!   `node_modules`, and the folders other systems keep on drives
//!   (`$RECYCLE.BIN`, `System Volume Information`, `lost+found`).
//! - **Shared**: SQLite in write-ahead mode, so apps read while one
//!   refreshes, which commits as it goes; a lock per drive keeps two from
//!   refreshing the same one at once.
//! - **Facts** ([`Index::fact`]) are what an app works out from a file (a
//!   game's checksum), kept beside it and forgotten when its size or time
//!   changes.
//!
//! Measured on 2026-11-02 on a USB exFAT drive (8,326 folders, 194,056
//! files): a cold first look took about three minutes however it was
//! done, since exFAT must read a folder's parent to say anything about it.
//! With the drive's folders in memory, listing everything took 1.1 s,
//! listing and stat'ing every file 9.8 s, and checking every folder's time
//! (this crate's second look) 0.01 s. `docs/files-index.md` has the rest.

pub mod volume;

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Transaction, params};

pub use volume::Volume;

/// What the index's schema is: a database of another is made again.
const SCHEMA: i64 = 1;
/// How long a refresh writes before it commits what it has, so others
/// see it and may write too.
const BATCH: Duration = Duration::from_millis(250);

/// Folders never read, by name.
const SKIP: &[&str] = &["$RECYCLE.BIN", "System Volume Information", "lost+found", "node_modules", "RECYCLER"];

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Db(rusqlite::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Db(e) => write!(f, "the files index: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Error {
        Error::Db(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// What a thing in a folder is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Folder = 1,
    File = 2,
    Link = 3,
    Other = 4,
}

impl Kind {
    fn of(t: std::fs::FileType) -> Kind {
        if t.is_dir() {
            Kind::Folder
        } else if t.is_file() {
            Kind::File
        } else if t.is_symlink() {
            Kind::Link
        } else {
            Kind::Other
        }
    }

    fn from_db(v: i64) -> Kind {
        match v {
            1 => Kind::Folder,
            2 => Kind::File,
            3 => Kind::Link,
            _ => Kind::Other,
        }
    }
}

/// How to refresh.
#[derive(Default)]
pub struct Refresh<'a> {
    /// Read each file's size and modified time (one `stat` a file, even in
    /// a folder that didn't change, where a file may have).
    pub stat: bool,
    /// Told now and then how far it's got.
    pub progress: Option<&'a dyn Fn(&Report)>,
    /// Stop when set; what was read is kept.
    pub stop: Option<&'a AtomicBool>,
}

/// What a refresh did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Folders looked at, and those listed (the others hadn't changed).
    pub folders: usize,
    pub listed: usize,
    /// Files seen in them, known or new.
    pub files: usize,
    /// Things found that weren't known, and known ones gone or changed.
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
    /// The folder being read.
    pub at: PathBuf,
    pub took: Duration,
    /// It stopped when asked, before the end.
    pub stopped: bool,
}

impl Report {
    /// Whether anything is different.
    pub fn any(&self) -> bool {
        self.added + self.removed + self.changed > 0
    }
}

/// A thing in the index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub kind: Kind,
    /// Its extension, lower case (files).
    pub ext: Option<String>,
    /// Its size and modified time (nanoseconds since 1970), when read.
    pub size: Option<u64>,
    pub mtime: Option<i64>,
}

/// The shared index.
pub struct Index {
    db: Connection,
    dir: PathBuf,
}

fn now_ns() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64)
}

fn mtime_ns(md: &std::fs::Metadata) -> i64 {
    md.mtime() * 1_000_000_000 + md.mtime_nsec()
}

fn ext_of(name: &OsStr) -> Option<String> {
    Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase())
}

/// Whether a folder of this name is never read.
fn skipped(name: &OsStr) -> bool {
    let b = name.as_bytes();
    b.first() == Some(&b'.') || SKIP.iter().any(|s| s.as_bytes() == b)
}

impl Index {
    /// The shared index, made if it isn't there.
    pub fn open() -> Result<Index> {
        let cache = std::env::var_os("XDG_CACHE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .unwrap_or_else(std::env::temp_dir);
        Index::open_at(&cache.join("aizome/index/files.sqlite"))
    }

    /// The index in the database at `path` (tests, a private one).
    pub fn open_at(path: &Path) -> Result<Index> {
        let dir = path.parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        std::fs::create_dir_all(&dir)?;
        let db = Connection::open(path)?;
        // Others may be writing: wait for them rather than fail.
        db.busy_timeout(Duration::from_secs(30))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "NORMAL")?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version != SCHEMA {
            // A cache: one of another shape is made again.
            db.execute_batch("DROP TABLE IF EXISTS fact; DROP TABLE IF EXISTS node; DROP TABLE IF EXISTS volume;")?;
        }
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS volume (
                id INTEGER PRIMARY KEY,
                key TEXT NOT NULL UNIQUE,
                label TEXT,
                fstype TEXT,
                mount TEXT,
                generation INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS node (
                id INTEGER PRIMARY KEY,
                volume INTEGER NOT NULL REFERENCES volume(id) ON DELETE CASCADE,
                parent INTEGER REFERENCES node(id) ON DELETE CASCADE,
                name BLOB NOT NULL,
                kind INTEGER NOT NULL,
                ext TEXT,
                size INTEGER,
                mtime INTEGER,
                listed INTEGER,
                checked INTEGER,
                UNIQUE (volume, parent, name)
            );
            CREATE INDEX IF NOT EXISTS node_parent ON node(parent);
            CREATE TABLE IF NOT EXISTS fact (
                node INTEGER NOT NULL REFERENCES node(id) ON DELETE CASCADE,
                key TEXT NOT NULL,
                size INTEGER,
                mtime INTEGER,
                value BLOB,
                PRIMARY KEY (node, key)
            );",
        )?;
        db.pragma_update(None, "user_version", SCHEMA)?;
        Ok(Index { db, dir })
    }

    /// The row of `vol`, made if it's new, its mount kept current.
    fn volume_id(&self, vol: &Volume) -> Result<i64> {
        self.db.execute(
            "INSERT INTO volume (key, label, fstype, mount) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(key) DO UPDATE SET label = excluded.label, fstype = excluded.fstype, mount = excluded.mount",
            params![vol.key, vol.label, vol.fstype, vol.mount.to_string_lossy()],
        )?;
        Ok(self.db.query_row("SELECT id FROM volume WHERE key = ?1", [&vol.key], |r| r.get(0))?)
    }

    /// The folder rows from the volume's root to `rel` (relative to the
    /// mount), made as needed when `make`.
    fn folder_id(&self, vid: i64, rel: &Path, make: bool) -> Result<Option<i64>> {
        let root: Option<i64> = self
            .db
            .query_row("SELECT id FROM node WHERE volume = ?1 AND parent IS NULL", [vid], |r| r.get(0))
            .optional()?;
        let mut id = match root {
            Some(id) => id,
            None if make => {
                self.db.execute(
                    "INSERT INTO node (volume, parent, name, kind) VALUES (?1, NULL, x'', ?2)",
                    params![vid, Kind::Folder as i64],
                )?;
                self.db.last_insert_rowid()
            }
            None => return Ok(None),
        };
        for part in rel.components() {
            let name = part.as_os_str().as_bytes();
            let found: Option<i64> = self
                .db
                .query_row(
                    "SELECT id FROM node WHERE volume = ?1 AND parent = ?2 AND name = ?3",
                    params![vid, id, name],
                    |r| r.get(0),
                )
                .optional()?;
            id = match found {
                Some(c) => c,
                None if make => {
                    self.db.execute(
                        "INSERT INTO node (volume, parent, name, kind) VALUES (?1, ?2, ?3, ?4)",
                        params![vid, id, name, Kind::Folder as i64],
                    )?;
                    self.db.last_insert_rowid()
                }
                None => return Ok(None),
            };
        }
        Ok(Some(id))
    }

    /// Where `path` is: its volume, its row there (when known), and its
    /// volume's row.
    fn locate(&self, path: &Path, make: bool) -> Result<Option<(Volume, i64, i64)>> {
        let path = path.canonicalize()?;
        let vol = volume::of(&path)?;
        let vid = if make {
            self.volume_id(&vol)?
        } else {
            match self.db.query_row("SELECT id FROM volume WHERE key = ?1", [&vol.key], |r| r.get(0)).optional()? {
                Some(v) => v,
                None => return Ok(None),
            }
        };
        let rel = path.strip_prefix(&vol.mount).unwrap_or(Path::new(""));
        Ok(self.folder_id(vid, rel, make)?.map(|id| (vol, vid, id)))
    }

    /// Bring what's under the folder `root` up to date.
    pub fn refresh(&mut self, root: &Path, how: &Refresh) -> Result<Report> {
        let began = Instant::now();
        let root = root.canonicalize()?;
        let Some((vol, vid, top)) = self.locate(&root, true)? else {
            return Err(Error::Io(std::io::Error::other(format!("{} can't be indexed", root.display()))));
        };
        // One refresher a drive at a time: another waits, then has little
        // left to do.
        let lock = std::fs::File::create(self.dir.join(format!("refresh-{vid}.lock")))?;
        lock.lock()?;
        let racy = vol.racy_ns();
        let mut report = Report::default();
        let mut tx = self.db.transaction()?;
        let mut since = Instant::now();
        let mut stack = vec![(root.clone(), top)];
        while let Some((path, id)) = stack.pop() {
            if how.stop.is_some_and(|s| s.load(Ordering::Acquire)) {
                report.stopped = true;
                break;
            }
            let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
            // Another filesystem mounted here is its own volume.
            if !md.is_dir() || md.dev() != vol.dev {
                continue;
            }
            report.folders += 1;
            report.at.clone_from(&path);
            let mtime = mtime_ns(&md);
            let (listed, checked): (Option<i64>, Option<i64>) =
                tx.query_row("SELECT listed, checked FROM node WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let unchanged = listed == Some(mtime) && checked.is_some_and(|c| c - mtime > racy);
            if unchanged {
                walk_known(&tx, &path, id, how.stat, &mut stack, &mut report)?;
            } else if list(&tx, vid, &path, id, how.stat, &mut stack, &mut report)? {
                tx.execute("UPDATE node SET listed = ?1, checked = ?2 WHERE id = ?3", params![mtime, now_ns(), id])?;
            }
            if since.elapsed() >= BATCH {
                if report.any() {
                    tx.execute("UPDATE volume SET generation = generation + 1 WHERE id = ?1", [vid])?;
                }
                tx.commit()?;
                if let Some(p) = how.progress {
                    p(&report);
                }
                tx = self.db.transaction()?;
                since = Instant::now();
            }
        }
        if report.any() {
            tx.execute("UPDATE volume SET generation = generation + 1 WHERE id = ?1", [vid])?;
        }
        tx.commit()?;
        drop(lock);
        report.took = began.elapsed();
        if let Some(p) = how.progress {
            p(&report);
        }
        Ok(report)
    }

    /// A number that changes whenever what's indexed on `path`'s drive
    /// does: an app that kept what it found compares it to know whether to
    /// look again.
    pub fn generation(&self, path: &Path) -> Result<u64> {
        let path = path.canonicalize()?;
        let vol = volume::of(&path)?;
        let g: Option<i64> =
            self.db.query_row("SELECT generation FROM volume WHERE key = ?1", [&vol.key], |r| r.get(0)).optional()?;
        Ok(g.unwrap_or(0) as u64)
    }

    /// Everything indexed under the folder `root` (but `root` itself),
    /// as it was last refreshed: nothing if it never was.
    pub fn entries(&self, root: &Path) -> Result<Vec<Entry>> {
        let root = root.canonicalize()?;
        let Some((_, _, top)) = self.locate(&root, false)? else { return Ok(Vec::new()) };
        let mut q = self.db.prepare(
            "WITH RECURSIVE sub(id) AS (
                SELECT ?1 UNION ALL SELECT n.id FROM node n JOIN sub ON n.parent = sub.id WHERE n.kind = 1
             )
             SELECT n.id, n.parent, n.name, n.kind, n.ext, n.size, n.mtime FROM node n
             WHERE n.parent IN (SELECT id FROM sub)",
        )?;
        let rows = q.query_map([top], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Vec<u8>>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<i64>>(5)?,
                r.get::<_, Option<i64>>(6)?,
            ))
        })?;
        let rows: Vec<_> = rows.collect::<std::result::Result<_, _>>()?;
        // Folders' paths, from the root down (a parent's row comes before
        // its children's only by chance, so by asking).
        let mut folders: HashMap<i64, (i64, OsString)> = HashMap::new();
        for (id, parent, name, kind, ..) in &rows {
            if *kind == Kind::Folder as i64 {
                folders.insert(*id, (*parent, OsString::from_vec(name.clone())));
            }
        }
        let mut paths: HashMap<i64, PathBuf> = HashMap::from([(top, root.clone())]);
        fn path_of(id: i64, folders: &HashMap<i64, (i64, OsString)>, paths: &mut HashMap<i64, PathBuf>) -> PathBuf {
            if let Some(p) = paths.get(&id) {
                return p.clone();
            }
            let (parent, name) = &folders[&id];
            let p = path_of(*parent, folders, paths).join(name);
            paths.insert(id, p.clone());
            p
        }
        Ok(rows
            .into_iter()
            .map(|(_, parent, name, kind, ext, size, mtime)| Entry {
                path: path_of(parent, &folders, &mut paths).join(OsStr::from_bytes(&name)),
                kind: Kind::from_db(kind),
                ext,
                size: size.map(|s| s as u64),
                mtime,
            })
            .collect())
    }

    /// What an app worked out from the file at `path` under `key`, if the
    /// file is indexed and hasn't changed (size or time) since.
    pub fn fact(&self, path: &Path, key: &str) -> Result<Option<Vec<u8>>> {
        let Some(id) = self.file_id(path, false)? else { return Ok(None) };
        let md = std::fs::metadata(path)?;
        let row: Option<(Option<i64>, Option<i64>, Vec<u8>)> = self
            .db
            .query_row("SELECT size, mtime, value FROM fact WHERE node = ?1 AND key = ?2", params![id, key], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?;
        Ok(row
            .and_then(|(size, mtime, v)| (size == Some(md.len() as i64) && mtime == Some(mtime_ns(&md))).then_some(v)))
    }

    /// What's known under `key` of the files in `folder`, by name, as
    /// [`Index::fact`] says it of one but in one query for the folder,
    /// where that looks each file up, its folders and volume and all:
    /// thousands of games at a start. With `check`, a fact holds while its
    /// file is as it was (a `stat` each); without, as kept, for a look at
    /// what the index knows that a refresh will follow.
    pub fn facts_in(&self, folder: &Path, key: &str, check: bool) -> Result<HashMap<OsString, Vec<u8>>> {
        let Some((_, _, id)) = self.locate(folder, false)? else { return Ok(HashMap::new()) };
        let mut q = self.db.prepare_cached(
            "SELECT n.name, f.size, f.mtime, f.value FROM node n JOIN fact f ON f.node = n.id
             WHERE n.parent = ?1 AND f.key = ?2",
        )?;
        let rows = q.query_map(params![id, key], |r| {
            let name = OsString::from_vec(r.get::<_, Vec<u8>>(0)?);
            Ok((name, r.get::<_, Option<i64>>(1)?, r.get::<_, Option<i64>>(2)?, r.get::<_, Vec<u8>>(3)?))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (name, size, mtime, value) = row?;
            let holds = !check
                || std::fs::metadata(folder.join(&name))
                    .is_ok_and(|md| size == Some(md.len() as i64) && mtime == Some(mtime_ns(&md)));
            if holds {
                out.insert(name, value);
            }
        }
        Ok(out)
    }

    /// Keep `value` as what's known of the file at `path` under `key`,
    /// while it stays as it is now. The file is indexed if it wasn't.
    pub fn set_fact(&self, path: &Path, key: &str, value: &[u8]) -> Result<()> {
        let md = std::fs::metadata(path)?;
        let Some(id) = self.file_id(path, true)? else { return Ok(()) };
        self.db.execute(
            "INSERT INTO fact (node, key, size, mtime, value) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(node, key) DO UPDATE SET size = excluded.size, mtime = excluded.mtime, value = excluded.value",
            params![id, key, md.len() as i64, mtime_ns(&md), value],
        )?;
        Ok(())
    }

    /// The row of the file at `path`, made (with its folders) when `make`.
    fn file_id(&self, path: &Path, make: bool) -> Result<Option<i64>> {
        let path = path.canonicalize()?;
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else { return Ok(None) };
        let Some((_, vid, folder)) = self.locate(parent, make)? else { return Ok(None) };
        let found: Option<i64> = self
            .db
            .query_row("SELECT id FROM node WHERE parent = ?1 AND name = ?2", params![folder, name.as_bytes()], |r| {
                r.get(0)
            })
            .optional()?;
        match found {
            Some(id) => Ok(Some(id)),
            None if make => {
                self.db.execute(
                    "INSERT INTO node (volume, parent, name, kind, ext) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![vid, folder, name.as_bytes(), Kind::File as i64, ext_of(name)],
                )?;
                Ok(Some(self.db.last_insert_rowid()))
            }
            None => Ok(None),
        }
    }
}

/// A row of a folder's: its id, kind, size and modified time.
type Known = (i64, i64, Option<i64>, Option<i64>);

/// A folder unchanged since it was listed: its folders to look at as
/// they were, and (when stat'ing) its files' sizes and times.
fn walk_known(
    tx: &Transaction,
    path: &Path,
    id: i64,
    stat: bool,
    stack: &mut Vec<(PathBuf, i64)>,
    report: &mut Report,
) -> Result<()> {
    let mut q = tx.prepare_cached("SELECT id, name, kind, size, mtime FROM node WHERE parent = ?1")?;
    let children: Vec<(i64, Vec<u8>, Known)> = q
        .query_map([id], |r| Ok((r.get(0)?, r.get(1)?, (r.get(0)?, r.get(2)?, r.get(3)?, r.get(4)?))))?
        .collect::<std::result::Result<_, _>>()?;
    for (cid, name, (_, kind, size, mtime)) in children {
        let child = path.join(OsStr::from_bytes(&name));
        if kind == Kind::File as i64 {
            report.files += 1;
        }
        if kind == Kind::Folder as i64 {
            stack.push((child, cid));
        } else if stat
            && kind == Kind::File as i64
            && let Ok(md) = std::fs::symlink_metadata(&child)
        {
            let (s, m) = (md.len() as i64, mtime_ns(&md));
            if size != Some(s) || mtime != Some(m) {
                tx.execute("UPDATE node SET size = ?1, mtime = ?2 WHERE id = ?3", params![s, m, cid])?;
                // Read for the first time isn't changed.
                if size.is_some() {
                    report.changed += 1;
                }
            }
        }
    }
    Ok(())
}

/// A folder that changed (or was never listed): listed, and the index
/// brought in line with what's there. False if it couldn't be read (it's
/// kept as it was).
fn list(
    tx: &Transaction,
    vid: i64,
    path: &Path,
    id: i64,
    stat: bool,
    stack: &mut Vec<(PathBuf, i64)>,
    report: &mut Report,
) -> Result<bool> {
    let mut known: HashMap<Vec<u8>, Known> = {
        let mut q = tx.prepare_cached("SELECT id, name, kind, size, mtime FROM node WHERE parent = ?1")?;
        q.query_map([id], |r| Ok((r.get::<_, Vec<u8>>(1)?, (r.get(0)?, r.get(2)?, r.get(3)?, r.get(4)?))))?
            .collect::<std::result::Result<_, _>>()?
    };
    let entries: Vec<std::fs::DirEntry> = match std::fs::read_dir(path) {
        Ok(rd) => rd.flatten().collect(),
        Err(_) => return Ok(false),
    };
    report.listed += 1;
    // A cache's folder (with CACHEDIR.TAG) is kept empty.
    let cache = entries.iter().any(|e| e.file_name() == "CACHEDIR.TAG");
    let mut insert = tx.prepare_cached(
        "INSERT INTO node (volume, parent, name, kind, ext, size, mtime) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    for e in entries {
        let name = e.file_name();
        let Ok(t) = e.file_type() else { continue };
        let kind = Kind::of(t);
        if cache || (kind == Kind::Folder && skipped(&name)) {
            continue;
        }
        if kind == Kind::File {
            report.files += 1;
        }
        let child = path.join(&name);
        let (size, mtime) = match (stat, kind) {
            (true, Kind::File) => e.metadata().map_or((None, None), |m| (Some(m.len() as i64), Some(mtime_ns(&m)))),
            _ => (None, None),
        };
        match known.remove(name.as_bytes()) {
            Some((cid, k, s, m)) if k == kind as i64 => {
                if stat && kind == Kind::File && (s != size || m != mtime) {
                    tx.execute("UPDATE node SET size = ?1, mtime = ?2 WHERE id = ?3", params![size, mtime, cid])?;
                    if s.is_some() {
                        report.changed += 1;
                    }
                }
                if kind == Kind::Folder {
                    stack.push((child, cid));
                }
            }
            other => {
                if let Some((cid, ..)) = other {
                    // Of another kind now: the old one goes, with what was in it.
                    tx.execute("DELETE FROM node WHERE id = ?1", [cid])?;
                    report.changed += 1;
                } else {
                    report.added += 1;
                }
                let ext = if kind == Kind::File { ext_of(&name) } else { None };
                insert.execute(params![vid, id, name.as_bytes(), kind as i64, ext, size, mtime])?;
                if kind == Kind::Folder {
                    stack.push((child, tx.last_insert_rowid()));
                }
            }
        }
    }
    for (cid, ..) in known.into_values() {
        tx.execute("DELETE FROM node WHERE id = ?1", [cid])?;
        report.removed += 1;
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
