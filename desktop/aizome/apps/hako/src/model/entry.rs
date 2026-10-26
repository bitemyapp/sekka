//! The items in a folder, read and sorted.

use std::cmp::Ordering;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::mime::{self, Mime, MimeDb};
use crate::fsops::names;

/// What an item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Folder,
    File,
    /// A symbolic link: to a folder (browsable), to something else, or
    /// to nothing.
    Link {
        to_folder: bool,
        broken: bool,
    },
    /// A pipe, socket or device.
    Special,
}

/// What the Trash knows of an item in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Trashed {
    pub original: Option<PathBuf>,
    /// `2026-10-17T21:42:05`.
    pub deleted: Option<String>,
}

/// An item in a folder.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: OsString,
    /// The name to show (and sort and search by).
    pub display: String,
    pub path: PathBuf,
    pub kind: Kind,
    /// Bytes: a file's length (a link's target's, if a file).
    pub size: u64,
    /// Seconds since the epoch.
    pub modified: Option<i64>,
    pub created: Option<i64>,
    pub accessed: Option<i64>,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub mime: Mime,
    /// What it is, in words ("PNG image", "Folder").
    pub kind_name: std::sync::Arc<str>,
    pub hidden: bool,
    pub link_target: Option<PathBuf>,
    pub dev: u64,
    pub ino: u64,
    /// In a trash can.
    pub trashed: Option<Trashed>,
}

fn secs(t: io::Result<std::time::SystemTime>) -> Option<i64> {
    let t = t.ok()?;
    Some(match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    })
}

impl Entry {
    /// The item at `path`, as a listing shows it.
    pub fn read(path: &Path) -> io::Result<Entry> {
        let meta = fs::symlink_metadata(path)?;
        Ok(Entry::from_meta(
            path.to_path_buf(),
            path.file_name().unwrap_or(path.as_os_str()).to_os_string(),
            &meta,
            MimeDb::get(),
        ))
    }

    /// An entry from what `lstat` said.
    pub fn from_meta(path: PathBuf, name: OsString, meta: &fs::Metadata, db: &MimeDb) -> Entry {
        let display = names::display(&name);
        let ft = meta.file_type();
        let mut size = meta.len();
        let mut link_target = None;
        let (kind, mime) = if ft.is_dir() {
            (Kind::Folder, db.intern(mime::DIRECTORY))
        } else if ft.is_symlink() {
            link_target = fs::read_link(&path).ok();
            match fs::metadata(&path) {
                Ok(t) if t.is_dir() => (Kind::Link { to_folder: true, broken: false }, db.intern(mime::DIRECTORY)),
                Ok(t) => {
                    size = t.len();
                    (
                        Kind::Link { to_folder: false, broken: false },
                        db.by_name(&display).unwrap_or_else(|| db.intern(mime::UNKNOWN)),
                    )
                }
                Err(_) => (Kind::Link { to_folder: false, broken: true }, db.intern(mime::SYMLINK)),
            }
        } else if ft.is_file() {
            let m = db.by_name(&display).unwrap_or_else(|| {
                if meta.len() == 0 {
                    db.intern(mime::EMPTY)
                } else if meta.mode() & 0o111 != 0 {
                    db.intern("application/x-executable")
                } else {
                    db.intern(mime::UNKNOWN)
                }
            });
            (Kind::File, m)
        } else {
            (Kind::Special, db.intern("inode/special"))
        };
        Entry {
            hidden: names::is_hidden_name(&name),
            name,
            display,
            path,
            kind,
            size,
            modified: secs(meta.modified()),
            created: secs(meta.created()),
            accessed: secs(meta.accessed()),
            mode: meta.mode(),
            uid: meta.uid(),
            gid: meta.gid(),
            kind_name: match kind {
                Kind::Link { broken: true, .. } => db.intern("Broken link"),
                Kind::Special => db.intern("Special file"),
                _ => db.kind_name(&mime),
            },
            mime,
            link_target,
            dev: meta.dev(),
            ino: meta.ino(),
            trashed: None,
        }
    }

    /// A folder, or a link to one: something to browse into.
    pub fn is_folder(&self) -> bool {
        matches!(self.kind, Kind::Folder | Kind::Link { to_folder: true, .. })
    }

    pub fn is_link(&self) -> bool {
        matches!(self.kind, Kind::Link { .. })
    }
}

/// Read the items in `dir` (unsorted). Items that vanish while it's read
/// are left out.
pub fn read_folder(dir: &Path) -> io::Result<Vec<Entry>> {
    let db = MimeDb::get();
    let mut out = Vec::new();
    for e in fs::read_dir(dir)? {
        let Ok(e) = e else { continue };
        let Ok(meta) = e.metadata() else { continue };
        out.push(Entry::from_meta(e.path(), e.file_name(), &meta, db));
    }
    Ok(out)
}

/// What a listing is sorted by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SortKey {
    #[default]
    Name,
    Size,
    Kind,
    Modified,
}

/// How a listing is sorted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Sort {
    pub key: SortKey,
    pub ascending: bool,
    pub folders_first: bool,
}

impl Default for Sort {
    fn default() -> Sort {
        Sort { key: SortKey::Name, ascending: true, folders_first: true }
    }
}

/// Sort entries as `sort` says; ties go by name.
pub fn sort_entries(entries: &mut [Entry], sort: Sort) {
    let mut order: Vec<usize> = (0..entries.len()).collect();
    let keys: Vec<Vec<Chunk>> = entries.iter().map(|e| natural_key(&e.display)).collect();
    order.sort_by(|&a, &b| {
        let (ea, eb) = (&entries[a], &entries[b]);
        if sort.folders_first && ea.is_folder() != eb.is_folder() {
            return if ea.is_folder() { Ordering::Less } else { Ordering::Greater };
        }
        let by_name = || keys[a].cmp(&keys[b]).then_with(|| ea.name.cmp(&eb.name));
        let primary = match sort.key {
            SortKey::Name => Ordering::Equal,
            SortKey::Size => ea.size.cmp(&eb.size),
            SortKey::Kind => ea.kind_name.cmp(&eb.kind_name),
            SortKey::Modified => ea.modified.cmp(&eb.modified),
        };
        let o = primary.then_with(by_name);
        if sort.ascending { o } else { o.reverse() }
    });
    let sorted: Vec<Entry> = order.iter().map(|&i| entries[i].clone()).collect();
    entries.clone_from_slice(&sorted);
}

/// A piece of a name for natural sorting: runs of digits compare as
/// numbers ("file2" before "file10"), letters without case.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Chunk {
    Number(u128, usize),
    Text(String),
}

pub fn natural_key(s: &str) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut digits = String::new();
    let mut text = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !text.is_empty() {
                out.push(Chunk::Text(std::mem::take(&mut text)));
            }
            digits.push(c);
        } else {
            if !digits.is_empty() {
                let d = std::mem::take(&mut digits);
                out.push(Chunk::Number(d.parse().unwrap_or(u128::MAX), d.len()));
            }
            text.extend(c.to_lowercase());
        }
    }
    if !digits.is_empty() {
        out.push(Chunk::Number(digits.parse().unwrap_or(u128::MAX), digits.len()));
    }
    if !text.is_empty() {
        out.push(Chunk::Text(text));
    }
    out
}

/// Compare names naturally.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    natural_key(a).cmp(&natural_key(b)).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsops::testing::TempDir;

    #[test]
    fn reads_folders_with_links() {
        let t = TempDir::new("entries");
        t.file("photo.png", "png");
        t.file(".hidden", "h");
        t.dir("Folder");
        std::os::unix::fs::symlink("Folder", t.join("to-folder")).unwrap();
        std::os::unix::fs::symlink("photo.png", t.join("to-photo")).unwrap();
        std::os::unix::fs::symlink("nowhere", t.join("broken")).unwrap();
        let mut es = read_folder(t.path()).unwrap();
        sort_entries(&mut es, Sort::default());
        let names: Vec<&str> = es.iter().map(|e| e.display.as_str()).collect();
        assert_eq!(names, ["Folder", "to-folder", ".hidden", "broken", "photo.png", "to-photo"]);
        let get = |n: &str| es.iter().find(|e| e.display == n).unwrap();
        assert!(get("Folder").is_folder());
        assert!(get("to-folder").is_folder());
        assert!(get(".hidden").hidden);
        assert_eq!(get("to-photo").kind, Kind::Link { to_folder: false, broken: false });
        assert_eq!(get("broken").kind, Kind::Link { to_folder: false, broken: true });
        assert_eq!(get("to-folder").link_target.as_deref(), Some(Path::new("Folder")));
    }

    #[test]
    fn sorts_naturally_and_by_keys() {
        let mut v = vec!["file10", "File2", "file1", "a", "B", "file02"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["a", "B", "file1", "File2", "file02", "file10"]);
        let t = TempDir::new("sort");
        t.file("small", "1");
        t.file("big", "1234567");
        t.file("mid", "123");
        let mut es = read_folder(t.path()).unwrap();
        sort_entries(&mut es, Sort { key: SortKey::Size, ascending: false, folders_first: true });
        let names: Vec<&str> = es.iter().map(|e| e.display.as_str()).collect();
        assert_eq!(names, ["big", "mid", "small"]);
    }
}
