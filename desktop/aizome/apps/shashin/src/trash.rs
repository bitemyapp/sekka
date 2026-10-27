//! Moving a picture to the Trash, by the freedesktop.org Trash
//! specification as Hako keeps it, and taking it back out (Undo).
//!
//! A picture on the home folder's filesystem goes to `$XDG_DATA_HOME/Trash`;
//! one elsewhere to its filesystem's `$topdir/.Trash/$uid` (when an
//! administrator made `.Trash`, sticky and not a link) or
//! `$topdir/.Trash-$uid`. Each can has `files/` and `info/`; the info file
//! (`Path=`, `DeletionDate=`) is made first, with `O_EXCL`, to claim the
//! name, then the picture is renamed into `files/`. Trashing is always a
//! rename, never a copy.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// A picture in the Trash, to take back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trashed {
    pub original: PathBuf,
    pub file: PathBuf,
    pub info: PathBuf,
}

/// The trash cans of a user.
#[derive(Clone, Debug)]
pub struct Trash {
    /// `$XDG_DATA_HOME/Trash`.
    pub home: PathBuf,
    pub uid: u32,
}

impl Trash {
    pub fn from_env() -> Trash {
        let home_dir =
            std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
        let data = match std::env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
            Some(p) if p.is_absolute() => p,
            _ => home_dir.join(".local/share"),
        };
        // SAFETY: getuid has no preconditions and can't fail.
        let uid = unsafe { libc_getuid() };
        Trash { home: data.join("Trash"), uid }
    }

    /// The can for `path`: the home trash if they share a filesystem, else
    /// one at the top of the picture's filesystem.
    pub fn can_for(&self, path: &Path) -> io::Result<(PathBuf, Option<PathBuf>)> {
        let dev = fs::symlink_metadata(path)?.dev();
        let home_dev = nearest_existing(&self.home).and_then(|p| fs::metadata(p).ok()).map(|m| m.dev());
        if home_dev == Some(dev) {
            return Ok((self.home.clone(), None));
        }
        let top = topdir(path, dev);
        let admin = top.join(".Trash");
        if let Ok(m) = fs::symlink_metadata(&admin)
            && m.is_dir()
            && m.permissions().mode() & 0o1000 != 0
        {
            let can = admin.join(self.uid.to_string());
            if fs::create_dir_all(can.join("files")).is_ok() && fs::create_dir_all(can.join("info")).is_ok() {
                return Ok((can, Some(top)));
            }
        }
        Ok((top.join(format!(".Trash-{}", self.uid)), Some(top)))
    }

    /// Move `path` to the Trash.
    pub fn trash(&self, path: &Path) -> io::Result<Trashed> {
        let path = std::path::absolute(path)?;
        let (can, top) = self.can_for(&path)?;
        let (files, info) = (can.join("files"), can.join("info"));
        for dir in [&can, &files, &info] {
            if !dir.exists() {
                fs::DirBuilder::new().recursive(true).create(dir)?;
                fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
            }
        }
        let recorded = match &top {
            Some(t) => path.strip_prefix(t).map(Path::to_path_buf).unwrap_or_else(|_| path.clone()),
            None => path.clone(),
        };
        let body = format!("[Trash Info]\nPath={}\nDeletionDate={}\n", encode(&recorded), timestamp());
        let base = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "picture".into());
        for n in 0..10_000 {
            let name = if n == 0 { base.clone() } else { numbered(&base, n + 1) };
            let info_path = info.join(format!("{name}.trashinfo"));
            let target = files.join(&name);
            if fs::symlink_metadata(&target).is_ok() {
                continue;
            }
            let mut f = match OpenOptions::new().write(true).create_new(true).mode(0o600).open(&info_path) {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            };
            if let Err(e) = f.write_all(body.as_bytes()) {
                let _ = fs::remove_file(&info_path);
                return Err(e);
            }
            drop(f);
            if let Err(e) = fs::rename(&path, &target) {
                let _ = fs::remove_file(&info_path);
                return Err(e);
            }
            return Ok(Trashed { original: path, file: target, info: info_path });
        }
        Err(io::Error::other("the Trash has too many items of that name"))
    }

    /// Put a trashed picture back where it was (unless something has
    /// taken its place).
    pub fn restore(&self, t: &Trashed) -> io::Result<()> {
        if fs::symlink_metadata(&t.original).is_ok() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "another file has its name now"));
        }
        if let Some(dir) = t.original.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::rename(&t.file, &t.original)?;
        let _ = fs::remove_file(&t.info);
        Ok(())
    }
}

unsafe extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

/// `name 2.jpg`, `name 3.jpg`: the extension kept.
fn numbered(name: &str, n: usize) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem} {n}.{ext}"),
        _ => format!("{name} {n}"),
    }
}

fn nearest_existing(p: &Path) -> Option<PathBuf> {
    let mut p = p.to_path_buf();
    loop {
        if p.exists() {
            return Some(p);
        }
        if !p.pop() {
            return None;
        }
    }
}

/// The top folder of the filesystem `path` (on device `dev`) is on.
fn topdir(path: &Path, dev: u64) -> PathBuf {
    let mut top = path.parent().unwrap_or(Path::new("/")).to_path_buf();
    while let Some(up) = top.parent() {
        match fs::metadata(up) {
            Ok(m) if m.dev() == dev => top = up.to_path_buf(),
            _ => break,
        }
    }
    top
}

/// Percent-encoding for `Path=`: bytes other than unreserved ones and `/`.
fn encode(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut s = String::new();
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

/// The local time as the spec writes it: `2026-10-17T21:42:05`.
fn timestamp() -> String {
    let secs =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let offset = local_offset(secs);
    let t = secs + offset;
    let (days, rem) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    let (y, m, d) = civil(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Seconds east of UTC here, from the C library.
pub fn local_offset(secs: i64) -> i64 {
    #[repr(C)]
    struct Tm {
        sec: i32,
        min: i32,
        hour: i32,
        mday: i32,
        mon: i32,
        year: i32,
        wday: i32,
        yday: i32,
        isdst: i32,
        gmtoff: std::ffi::c_long,
        zone: *const std::ffi::c_char,
    }
    unsafe extern "C" {
        fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
    }
    let mut tm = std::mem::MaybeUninit::<Tm>::zeroed();
    // SAFETY: both pointers are valid for the call; localtime_r fills `tm`.
    let ok = unsafe { !localtime_r(&secs, tm.as_mut_ptr()).is_null() };
    if ok {
        // SAFETY: localtime_r succeeded, so `tm` is filled.
        unsafe { tm.assume_init() }.gmtoff
    } else {
        0
    }
}

/// Days since 1970-01-01 as a date (Howard Hinnant's algorithm).
pub fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trashes_and_restores() {
        let dir = std::env::temp_dir().join(format!("shashin-trash-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let trash = Trash { home: dir.join("Trash"), uid: 1000 };
        let pic = dir.join("a b.png");
        fs::write(&pic, b"x").unwrap();
        let t = trash.trash(&pic).unwrap();
        assert!(!pic.exists());
        assert_eq!(t.file, dir.join("Trash/files/a b.png"));
        let info = fs::read_to_string(&t.info).unwrap();
        assert!(info.contains("Path=") && info.contains("a%20b.png"), "{info}");
        // A second of the same name is numbered.
        fs::write(&pic, b"y").unwrap();
        let t2 = trash.trash(&pic).unwrap();
        assert_eq!(t2.file, dir.join("Trash/files/a b 2.png"));
        trash.restore(&t2).unwrap();
        assert_eq!(fs::read(&pic).unwrap(), b"y");
        assert!(!t2.info.exists());
        assert!(trash.restore(&t).is_err(), "its name is taken");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_743), (2026, 10, 17));
    }
}
