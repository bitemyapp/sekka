//! The Trash, by the freedesktop.org Trash specification (1.0).
//!
//! Each filesystem has its own trash can, so trashing is always a rename
//! (never a copy):
//!
//! - the **home trash**, `$XDG_DATA_HOME/Trash`, for items on the
//!   filesystem it's on;
//! - on other filesystems, at the filesystem's top folder (`$topdir`),
//!   `$topdir/.Trash/$uid` when an administrator has made `$topdir/.Trash`
//!   (a real folder, not a link, with the sticky bit), else
//!   `$topdir/.Trash-$uid`, made by us and private to us.
//!
//! A can holds `files/` (the items, under names unique in the can) and
//! `info/` (a `name.trashinfo` for each: `Path=`, percent-encoded,
//! absolute in the home trash and relative to `$topdir` in the others,
//! and `DeletionDate=` in local time). The info file is made first, with
//! `O_EXCL`, to claim the name; the item follows by rename; a failed
//! rename takes the info file back. `directorysizes` caches trashed
//! folders' sizes.
//!
//! Some things never go to the Trash: the home folder and folders that
//! hold it, `/`, trash cans and what's in them (that is deleted from the
//! Trash instead), and items that aren't there.

use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::names;
use super::sys;

/// Where the trash cans are, for a user.
#[derive(Clone, Debug)]
pub struct Trash {
    /// The home trash: `$XDG_DATA_HOME/Trash`.
    pub home: PathBuf,
    /// The home folder (never trashed).
    pub home_dir: Option<PathBuf>,
    pub uid: u32,
    /// The filesystems' top folders, for tests and tools: `None` finds
    /// them by walking up while the device stays the same.
    pub topdirs: Option<Vec<PathBuf>>,
}

/// A trash can.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Can {
    /// The folder holding `files/` and `info/`.
    pub root: PathBuf,
    /// The filesystem's top folder, for a can other than the home trash
    /// (`Path=` is relative to it there).
    pub topdir: Option<PathBuf>,
}

impl Can {
    pub fn files(&self) -> PathBuf {
        self.root.join("files")
    }

    pub fn info(&self) -> PathBuf {
        self.root.join("info")
    }
}

/// An item just trashed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trashed {
    /// Where it was.
    pub original: PathBuf,
    /// Where it is now, in a can's `files/`.
    pub file: PathBuf,
    /// Its `.trashinfo`.
    pub info: PathBuf,
}

/// An item in a trash can.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Its name in `files/`.
    pub name: OsString,
    /// Its path in `files/`.
    pub file: PathBuf,
    /// Its `.trashinfo`, if it has one.
    pub info: Option<PathBuf>,
    /// Where it came from, if its info says.
    pub original: Option<PathBuf>,
    /// When it was trashed, as the info says (`2026-10-17T21:42:05`).
    pub deleted: Option<String>,
    pub can: Can,
    pub is_dir: bool,
}

impl Entry {
    /// The name to show: the original's, else the one in the can.
    pub fn display_name(&self) -> String {
        self.original
            .as_ref()
            .and_then(|o| o.file_name())
            .map(names::display)
            .unwrap_or_else(|| names::display(&self.name))
    }
}

impl Trash {
    /// The user's trash, from the environment.
    pub fn from_env() -> Trash {
        let home_dir = std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
        let data = match std::env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
            Some(p) if p.is_absolute() => p,
            _ => home_dir.clone().unwrap_or_else(|| PathBuf::from("/tmp")).join(".local/share"),
        };
        Trash { home: data.join("Trash"), home_dir, uid: sys::uid(), topdirs: None }
    }

    /// The home can.
    pub fn home_can(&self) -> Can {
        Can { root: self.home.clone(), topdir: None }
    }

    /// Why `path` can't go to the Trash, if it can't.
    pub fn refusal(&self, path: &Path) -> Option<String> {
        if !path.is_absolute() {
            return Some("it isn't an absolute path".into());
        }
        let real = real_path(path);
        if real == Path::new("/") {
            return Some("it's the root folder".into());
        }
        if let Some(home) = &self.home_dir {
            let home = fs::canonicalize(home).unwrap_or_else(|_| home.clone());
            if home.starts_with(&real) {
                return Some("it's the home folder, or holds it".into());
            }
        }
        if self.in_a_can(&real) {
            return Some("it's in the Trash already (delete it from there)".into());
        }
        let home_trash = fs::canonicalize(&self.home).unwrap_or_else(|_| self.home.clone());
        if home_trash.starts_with(&real) {
            return Some("it holds the Trash".into());
        }
        match fs::symlink_metadata(path) {
            Err(_) => return Some("it isn't there".into()),
            Ok(m) if super::transfer::is_mount_point(path, &m) => return Some("it's where a disk is mounted".into()),
            Ok(_) => {}
        }
        None
    }

    /// Whether `real` (a real path) is a can or inside one.
    pub fn in_a_can(&self, real: &Path) -> bool {
        let home_trash = fs::canonicalize(&self.home).unwrap_or_else(|_| self.home.clone());
        if real.starts_with(&home_trash) {
            return true;
        }
        let uid_name = format!(".Trash-{}", self.uid);
        let mut seen_trash = false;
        for c in real.components() {
            let n = c.as_os_str();
            if n == OsStr::new(&uid_name) {
                return true;
            }
            if seen_trash && n == OsStr::new(&self.uid.to_string()) {
                return true;
            }
            seen_trash = n == ".Trash";
        }
        false
    }

    /// The top folder of the filesystem `path` is on.
    pub fn topdir_of(&self, path: &Path) -> io::Result<PathBuf> {
        let real = real_path(path);
        if let Some(tops) = &self.topdirs {
            return tops
                .iter()
                .filter(|t| real.starts_with(t))
                .max_by_key(|t| t.as_os_str().len())
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no filesystem top folder for it"));
        }
        let dev = fs::symlink_metadata(path)?.dev();
        let mut top = real.parent().unwrap_or(Path::new("/")).to_path_buf();
        while let Some(up) = top.parent() {
            match fs::metadata(up) {
                Ok(m) if m.dev() == dev => top = up.to_path_buf(),
                _ => break,
            }
        }
        Ok(top)
    }

    /// The can an item at `path` goes into (made if need be).
    pub fn can_for(&self, path: &Path) -> io::Result<Can> {
        let dev = fs::symlink_metadata(path)?.dev();
        let home_dev = existing_ancestor_dev(&self.home)?;
        if dev == home_dev {
            make_private_dir(&self.home)?;
            let can = self.home_can();
            make_private_dir(&can.files())?;
            make_private_dir(&can.info())?;
            return Ok(can);
        }
        let top = self.topdir_of(path)?;
        let can = self.topdir_can(&top, true).ok_or_else(|| {
            io::Error::new(io::ErrorKind::PermissionDenied, format!("no Trash can be made on {}", top.display()))
        })?;
        if fs::metadata(&can.root)?.dev() != dev {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "its filesystem's Trash is on another one"));
        }
        Ok(can)
    }

    /// The can at `top`, if there is one (or, with `make`, if one can be
    /// made).
    fn topdir_can(&self, top: &Path, make: bool) -> Option<Can> {
        let shared = top.join(".Trash");
        if let Ok(m) = fs::symlink_metadata(&shared)
            && m.is_dir()
            && m.permissions().mode() & 0o1000 != 0
        {
            let root = shared.join(self.uid.to_string());
            if (make && make_private_dir(&root).is_ok()) || is_own_dir(&root, self.uid) {
                let can = Can { root, topdir: Some(top.to_path_buf()) };
                if !make || (make_private_dir(&can.files()).is_ok() && make_private_dir(&can.info()).is_ok()) {
                    return Some(can);
                }
            }
        }
        let root = top.join(format!(".Trash-{}", self.uid));
        if make {
            make_private_dir(&root).ok()?;
        }
        if !is_own_dir(&root, self.uid) {
            return None;
        }
        let can = Can { root, topdir: Some(top.to_path_buf()) };
        if make {
            make_private_dir(&can.files()).ok()?;
            make_private_dir(&can.info()).ok()?;
        }
        Some(can)
    }

    /// Every can there is: the home trash, and those at the top of the
    /// filesystems mounted (or of [`Trash::topdirs`]).
    pub fn cans(&self) -> Vec<Can> {
        let mut out = vec![self.home_can()];
        let tops = match &self.topdirs {
            Some(t) => t.clone(),
            None => mount_points(),
        };
        for top in tops {
            for can in self.topdir_cans(&top) {
                if !out.contains(&can) {
                    out.push(can);
                }
            }
        }
        out
    }

    /// The cans there are at a filesystem's top folder: its shared
    /// `.Trash/$uid` (if `.Trash` is fit for use) and its `.Trash-$uid`.
    fn topdir_cans(&self, top: &Path) -> Vec<Can> {
        let mut out = Vec::new();
        let shared = top.join(".Trash");
        if let Ok(m) = fs::symlink_metadata(&shared)
            && m.is_dir()
            && m.permissions().mode() & 0o1000 != 0
        {
            let root = shared.join(self.uid.to_string());
            if is_own_dir(&root, self.uid) && root.join("files").is_dir() {
                out.push(Can { root, topdir: Some(top.to_path_buf()) });
            }
        }
        let root = top.join(format!(".Trash-{}", self.uid));
        if is_own_dir(&root, self.uid) && root.join("files").is_dir() {
            out.push(Can { root, topdir: Some(top.to_path_buf()) });
        }
        out
    }

    /// Move `path` to the Trash.
    pub fn trash(&self, path: &Path) -> io::Result<Trashed> {
        if let Some(why) = self.refusal(path) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, why));
        }
        self.trash_as(path, path)
    }

    /// Move the item at `path` to the Trash as though it came from
    /// `original` (an item set aside under a temporary name while its
    /// replacement came).
    pub fn trash_as(&self, path: &Path, original: &Path) -> io::Result<Trashed> {
        let meta = fs::symlink_metadata(path)?;
        let can = self.can_for(path)?;
        let original = names::normalize(original);
        let recorded = match &can.topdir {
            Some(top) => original.strip_prefix(top).map(Path::to_path_buf).unwrap_or_else(|_| original.clone()),
            None => original.clone(),
        };
        let body = format!("[Trash Info]\nPath={}\nDeletionDate={}\n", encode(&recorded), local_timestamp());
        let base = original.file_name().unwrap_or(OsStr::new("item")).to_os_string();
        let (files, info) = (can.files(), can.info());
        let mut tried = 0;
        let mut name = base.clone();
        loop {
            tried += 1;
            if tried > 1 {
                name = names::free_name_with(&base, meta.is_dir(), |n| {
                    fs::symlink_metadata(files.join(n)).is_err()
                        && fs::symlink_metadata(info.join(info_name(n))).is_err()
                });
            }
            if fs::symlink_metadata(files.join(&name)).is_ok() {
                continue;
            }
            let info_path = info.join(info_name(&name));
            let mut f = match OpenOptions::new().write(true).create_new(true).mode_private().open(&info_path) {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            };
            if let Err(e) = f.write_all(body.as_bytes()).and_then(|()| f.sync_all()) {
                let _ = fs::remove_file(&info_path);
                return Err(e);
            }
            drop(f);
            let target = files.join(&name);
            match sys::rename_noreplace(path, &target) {
                Ok(()) => {
                    let _ = sys::sync_dir(&files);
                    let _ = sys::sync_dir(&info);
                    if meta.is_dir()
                        && let Some((_, bytes)) = super::delete::measure(&target, &super::Control::new(), 20_000)
                    {
                        let mtime = fs::metadata(&info_path).map(|m| m.mtime()).unwrap_or(0);
                        let _ = edit_sizes(&can, |lines| lines.push(format!("{bytes} {mtime} {}", encode_name(&name))));
                    }
                    return Ok(Trashed { original, file: target, info: info_path });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    let _ = fs::remove_file(&info_path);
                    continue;
                }
                Err(e) => {
                    let _ = fs::remove_file(&info_path);
                    return Err(e);
                }
            }
        }
    }

    /// What's in every can, newest first.
    pub fn list(&self) -> Vec<Entry> {
        let mut out = Vec::new();
        for can in self.cans() {
            out.extend(list_can(&can));
        }
        out.sort_by(|a, b| b.deleted.cmp(&a.deleted).then_with(|| a.name.cmp(&b.name)));
        out
    }

    /// Whether any can holds anything.
    pub fn is_empty(&self) -> bool {
        self.cans().iter().all(|c| fs::read_dir(c.files()).map(|mut rd| rd.next().is_none()).unwrap_or(true))
    }

    /// The entry for an item in a can's `files/`, by its path there.
    pub fn entry_for(&self, file: &Path) -> Option<Entry> {
        let files = file.parent()?;
        let root = files.parent()?;
        let can = self.cans().into_iter().find(|c| c.root == root)?;
        list_can(&can).into_iter().find(|e| e.file == file)
    }

    /// Take an entry's info (and its folder size) out of its can, once
    /// its item has gone (restored or deleted).
    pub fn forget(&self, entry: &Entry) {
        if let Some(info) = &entry.info {
            let _ = fs::remove_file(info);
        }
        let key = encode_name(&entry.name);
        let _ = edit_sizes(&entry.can, |lines| lines.retain(|l| l.rsplit(' ').next() != Some(key.as_str())));
    }
}

/// `name.trashinfo`.
fn info_name(name: &OsStr) -> OsString {
    let mut n = name.to_os_string();
    n.push(".trashinfo");
    n
}

/// The items in one can: each info with its file, and files without an
/// info (left by a crash or another program), whose origin is unknown.
pub fn list_can(can: &Can) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Ok(rd) = fs::read_dir(can.info()) {
        for e in rd.flatten() {
            let fname = e.file_name();
            let Some(name) = fname.as_bytes().strip_suffix(b".trashinfo") else { continue };
            let name = OsString::from_vec(name.to_vec());
            let file = can.files().join(&name);
            let Ok(meta) = fs::symlink_metadata(&file) else { continue };
            let text = fs::read_to_string(e.path()).unwrap_or_default();
            let (path, deleted) = parse_info(&text);
            let original = path.map(|p| match (&can.topdir, p.is_absolute()) {
                (Some(top), false) => top.join(p),
                _ => p,
            });
            seen.insert(name.clone());
            out.push(Entry {
                name,
                file,
                info: Some(e.path()),
                original,
                deleted,
                can: can.clone(),
                is_dir: meta.is_dir(),
            });
        }
    }
    if let Ok(rd) = fs::read_dir(can.files()) {
        for e in rd.flatten() {
            let name = e.file_name();
            if seen.contains(&name) {
                continue;
            }
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            out.push(Entry {
                file: e.path(),
                name,
                info: None,
                original: None,
                deleted: None,
                can: can.clone(),
                is_dir,
            });
        }
    }
    out
}

/// A `.trashinfo`'s `Path` (decoded) and `DeletionDate`.
pub fn parse_info(text: &str) -> (Option<PathBuf>, Option<String>) {
    let mut in_group = false;
    let (mut path, mut date) = (None, None);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_group = line == "[Trash Info]";
            continue;
        }
        if !in_group {
            continue;
        }
        if let Some(v) = line.strip_prefix("Path=") {
            path = Some(PathBuf::from(OsString::from_vec(decode(v))));
        } else if let Some(v) = line.strip_prefix("DeletionDate=") {
            date = Some(v.to_owned());
        }
    }
    (path, date)
}

/// A path percent-encoded as the spec asks (RFC 2396), slashes kept.
pub fn encode(path: &Path) -> String {
    encode_bytes(path.as_os_str().as_bytes(), true)
}

fn encode_name(name: &OsStr) -> String {
    encode_bytes(name.as_bytes(), false)
}

fn encode_bytes(b: &[u8], keep_slash: bool) -> String {
    let mut s = String::with_capacity(b.len());
    for &c in b {
        if c.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&c) || (keep_slash && c == b'/') {
            s.push(c as char);
        } else {
            s.push_str(&format!("%{c:02X}"));
        }
    }
    s
}

/// Percent-decoding.
pub fn decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("x"), 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// Edit a can's `directorysizes`, replacing it atomically.
fn edit_sizes(can: &Can, f: impl FnOnce(&mut Vec<String>)) -> io::Result<()> {
    let path = can.root.join("directorysizes");
    let text = fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().filter(|l| !l.trim().is_empty()).map(str::to_owned).collect();
    let before = lines.clone();
    f(&mut lines);
    if lines == before {
        return Ok(());
    }
    let tmp = can.root.join(format!("directorysizes.{}", sys::random_word()));
    let mut body = lines.join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    {
        let mut file = OpenOptions::new().write(true).create_new(true).mode_private().open(&tmp)?;
        file.write_all(body.as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(&tmp, &path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// The cached sizes of a can's folders, by their name in `files/`.
pub fn directory_sizes(can: &Can) -> std::collections::HashMap<OsString, u64> {
    let text = fs::read_to_string(can.root.join("directorysizes")).unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let mut parts = l.splitn(3, ' ');
            let size = parts.next()?.parse().ok()?;
            let _mtime = parts.next()?;
            Some((OsString::from_vec(decode(parts.next()?)), size))
        })
        .collect()
}

/// The path with its parent's links resolved (the item itself, maybe a
/// link, kept).
fn real_path(path: &Path) -> PathBuf {
    let path = names::normalize(path);
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf()).join(name),
        _ => path,
    }
}

/// The device of `path`, or of its nearest ancestor that exists.
fn existing_ancestor_dev(path: &Path) -> io::Result<u64> {
    let mut p = path;
    loop {
        if let Ok(m) = fs::metadata(p) {
            return Ok(m.dev());
        }
        p = p.parent().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no folder above the Trash"))?;
    }
}

/// Make a folder only its owner may use (and its parents), unless it's
/// there.
fn make_private_dir(dir: &Path) -> io::Result<()> {
    match fs::symlink_metadata(dir) {
        Ok(m) if m.is_dir() => Ok(()),
        Ok(_) => Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} isn't a folder", dir.display()))),
        Err(_) => fs::DirBuilder::new().recursive(true).mode(0o700).create(dir),
    }
}

/// A real folder (not a link) owned by `uid`.
fn is_own_dir(dir: &Path, uid: u32) -> bool {
    fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir() && m.uid() == uid)
}

/// Open options for files only their owner reads.
trait PrivateMode {
    fn mode_private(&mut self) -> &mut Self;
}

impl PrivateMode for OpenOptions {
    fn mode_private(&mut self) -> &mut Self {
        use std::os::unix::fs::OpenOptionsExt;
        self.mode(0o600)
    }
}

/// The filesystems mounted, as `/proc/self/mountinfo` lists them, real
/// ones only (not proc, sysfs, cgroups and the like).
pub fn mount_points() -> Vec<PathBuf> {
    let text = fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    parse_mountinfo(&text).into_iter().map(|m| m.mount_point).collect()
}

/// A mounted filesystem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mount {
    pub mount_point: PathBuf,
    pub fs_type: String,
    pub source: String,
}

/// The real filesystems in a `mountinfo`'s text.
pub fn parse_mountinfo(text: &str) -> Vec<Mount> {
    const VIRTUAL: &[&str] = &[
        "proc",
        "sysfs",
        "devtmpfs",
        "devpts",
        "cgroup",
        "cgroup2",
        "securityfs",
        "pstore",
        "bpf",
        "tracefs",
        "debugfs",
        "mqueue",
        "hugetlbfs",
        "configfs",
        "fusectl",
        "efivarfs",
        "autofs",
        "binfmt_misc",
        "ramfs",
        "nsfs",
        "rpc_pipefs",
        "overlay",
        "squashfs",
    ];
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((left, right)) = line.split_once(" - ") else { continue };
        let fields: Vec<&str> = left.split(' ').collect();
        let rfields: Vec<&str> = right.split(' ').collect();
        if fields.len() < 5 || rfields.len() < 2 {
            continue;
        }
        let mount_point = PathBuf::from(OsString::from_vec(unescape_octal(fields[4])));
        let fs_type = rfields[0].to_owned();
        if VIRTUAL.contains(&fs_type.as_str()) {
            continue;
        }
        if mount_point.starts_with("/nix/store") || mount_point.starts_with("/proc") || mount_point.starts_with("/sys")
        {
            continue;
        }
        out.push(Mount { mount_point, fs_type, source: rfields[1].to_owned() });
    }
    out
}

/// mountinfo's `\040` escapes.
fn unescape_octal(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c)) {
            out.push((b[i + 1] - b'0') * 64 + (b[i + 2] - b'0') * 8 + (b[i + 3] - b'0'));
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// Now in local time, as `DeletionDate` wants: `2026-10-17T21:42:05`.
pub fn local_timestamp() -> String {
    // SAFETY: `localtime_r` writes only into `tm`.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return "1970-01-01T00:00:00".into();
        }
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        )
    }
}

/// A `DeletionDate` as seconds since the epoch (taken as UTC: enough to
/// sort and to say "today").
pub fn parse_date(s: &str) -> Option<i64> {
    let (d, t) = s.split_once('T')?;
    let mut dp = d.split('-').map(|v| v.parse::<i64>().ok());
    let (y, mo, da) = (dp.next()??, dp.next()??, dp.next()??);
    let mut tp = t.split(':').map(|v| v.get(..2).unwrap_or(v).parse::<i64>().ok());
    let (h, mi, se) = (tp.next()??, tp.next()??, tp.next().flatten().unwrap_or(0));
    // Days from the civil date (Howard Hinnant's algorithm).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (mo + if mo > 2 { -3 } else { 9 }) + 2) / 5 + da - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + se)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsops::testing::{TempDir, read};

    fn trash_in(t: &TempDir) -> Trash {
        Trash { home: t.join("data/Trash"), home_dir: Some(t.join("home")), uid: sys::uid(), topdirs: Some(Vec::new()) }
    }

    #[test]
    fn trashes_into_the_home_trash_by_the_spec() {
        let t = TempDir::new("trash-home");
        let tr = trash_in(&t);
        let a = t.file("home/docs/a file%.txt", "hello");
        let folder = t.dir("home/docs/folder/inner");
        t.file("home/docs/folder/inner/x", "12345");
        let got = tr.trash(&a).unwrap();
        assert!(!a.exists());
        assert_eq!(got.file, t.join("data/Trash/files/a file%.txt"));
        assert_eq!(read(&got.file), "hello");
        let info = read(&got.info);
        assert!(info.starts_with("[Trash Info]\nPath="), "{info}");
        assert!(info.contains("/home/docs/a%20file%25.txt\n"), "{info}");
        assert!(info.contains("DeletionDate=20"));
        let mode = fs::metadata(t.join("data/Trash")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "the Trash is private");

        let got = tr.trash(folder.parent().unwrap()).unwrap();
        assert!(got.file.join("inner/x").exists());
        let sizes = directory_sizes(&tr.home_can());
        assert_eq!(sizes.get(OsStr::new("folder")), Some(&5));

        // A second item of a name gets a name of its own in the can.
        let b = t.file("home/other/a file%.txt", "second");
        let got = tr.trash(&b).unwrap();
        assert_eq!(got.file.file_name().unwrap(), "a file% 2.txt");

        let list = tr.list();
        assert_eq!(list.len(), 3);
        let first = list.iter().find(|e| e.name == "a file%.txt").unwrap();
        assert_eq!(first.original.as_deref(), Some(a.as_path()));
        assert_eq!(first.display_name(), "a file%.txt");
        let second = list.iter().find(|e| e.name == "a file% 2.txt").unwrap();
        assert_eq!(second.display_name(), "a file%.txt", "shown by its original name");
        assert!(!tr.is_empty());
    }

    #[test]
    fn refuses_what_it_must() {
        let t = TempDir::new("trash-refuse");
        let tr = trash_in(&t);
        t.dir("home");
        t.file("data/Trash/files/old", "x");
        for (p, why) in [
            (PathBuf::from("relative"), "absolute"),
            (PathBuf::from("/"), "root"),
            (t.join("home"), "home folder"),
            (t.path().to_path_buf(), "home folder"),
            (t.join("data/Trash/files/old"), "Trash already"),
            (t.join("data"), "holds the Trash"),
            (t.join("home/missing"), "isn't there"),
        ] {
            let e = tr.trash(&p).unwrap_err().to_string();
            assert!(e.contains(why), "{}: {e}", p.display());
        }
    }

    #[test]
    fn uses_a_volume_s_own_trash() {
        let t = TempDir::new("trash-topdir-home");
        let Some(vol) = TempDir::elsewhere("trash-topdir") else {
            eprintln!("no second filesystem here: skipped");
            return;
        };
        let tr = Trash { topdirs: Some(vec![vol.path().to_path_buf()]), ..trash_in(&t) };
        let item = vol.file("photos/cat.jpg", "meow");
        let got = tr.trash(&item).unwrap();
        let can = vol.join(format!(".Trash-{}", tr.uid));
        assert_eq!(got.file, can.join("files/cat.jpg"));
        let info = read(&got.info);
        assert!(info.contains("Path=photos/cat.jpg\n"), "relative to the top folder: {info}");
        assert_eq!(fs::metadata(&can).unwrap().permissions().mode() & 0o777, 0o700);
        let entry = tr.list().into_iter().find(|e| e.name == "cat.jpg").unwrap();
        assert_eq!(entry.original.as_deref(), Some(item.as_path()));
        assert_eq!(entry.can.topdir.as_deref(), Some(vol.path()));
        // Nothing went to the home trash.
        assert!(!t.join("data/Trash/files/cat.jpg").exists());
    }

    #[test]
    fn prefers_a_shared_trash_with_the_sticky_bit() {
        let t = TempDir::new("trash-shared-home");
        let Some(vol) = TempDir::elsewhere("trash-shared") else { return };
        let tr = Trash { topdirs: Some(vec![vol.path().to_path_buf()]), ..trash_in(&t) };
        let shared = vol.dir(".Trash");
        // Without the sticky bit it mustn't be used.
        let a = vol.file("a.txt", "a");
        let got = tr.trash(&a).unwrap();
        assert!(got.file.starts_with(vol.join(format!(".Trash-{}", tr.uid))));
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o1777)).unwrap();
        let b = vol.file("b.txt", "b");
        let got = tr.trash(&b).unwrap();
        assert_eq!(got.file, shared.join(format!("{}/files/b.txt", tr.uid)));
        assert_eq!(tr.list().len(), 2, "both cans are listed");
    }

    #[test]
    fn lists_orphans_and_forgets() {
        let t = TempDir::new("trash-orphans");
        let tr = trash_in(&t);
        t.file("data/Trash/files/stray", "s");
        t.file("data/Trash/info/ghost.trashinfo", "[Trash Info]\nPath=/x/ghost\nDeletionDate=2020-01-01T00:00:00\n");
        let list = tr.list();
        assert_eq!(list.len(), 1, "an info without its file isn't listed");
        assert_eq!(list[0].original, None);
        assert_eq!(list[0].display_name(), "stray");
        let a = t.file("home/a", "a");
        tr.trash(&a).unwrap();
        let e = tr.entry_for(&t.join("data/Trash/files/a")).unwrap();
        fs::remove_file(&e.file).unwrap();
        tr.forget(&e);
        assert!(!t.join("data/Trash/info/a.trashinfo").exists());
    }

    #[test]
    fn parses_and_encodes() {
        let (p, d) = parse_info("[Trash Info]\nPath=/home/u/a%20b%C3%A9\nDeletionDate=2026-10-17T21:42:05\n");
        assert_eq!(p, Some(PathBuf::from("/home/u/a bé")));
        assert_eq!(d.as_deref(), Some("2026-10-17T21:42:05"));
        assert_eq!(parse_info("[Other]\nPath=/x\n").0, None);
        assert_eq!(encode(Path::new("/a b/c%d")), "/a%20b/c%25d");
        assert_eq!(decode("%zz%41"), b"%zzA");
        assert_eq!(parse_date("1970-01-02T00:00:01"), Some(86401));
        assert_eq!(parse_date("2026-10-17T00:00:00"), Some(1_792_195_200));
    }

    #[test]
    fn reads_mountinfo() {
        let text = "22 1 259:2 / / rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw\n\
                    23 22 0:5 / /proc rw - proc proc rw\n\
                    40 22 8:17 / /run/media/u/My\\040Stick rw - vfat /dev/sdb1 rw\n";
        let m = parse_mountinfo(text);
        assert_eq!(m.len(), 2);
        assert_eq!(m[1].mount_point, PathBuf::from("/run/media/u/My Stick"));
        assert_eq!(m[1].fs_type, "vfat");
    }
}
