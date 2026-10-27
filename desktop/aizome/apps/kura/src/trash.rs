//! Moving to the Trash, by the freedesktop.org Trash specification (1.0):
//! the home trash is `$XDG_DATA_HOME/Trash` (`~/.local/share/Trash`), each
//! item goes into `files/` under a name of its own, and an
//! `info/<name>.trashinfo` beside it says where it came from and when, so
//! any file manager can put it back.
//!
//! Kura trashes only what lives on the home trash's filesystem (a rename,
//! never a copy), never anything in `/nix/store`, and never the home folder,
//! the Trash, or a folder that holds either.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// The home trash: `$XDG_DATA_HOME/Trash`, else `~/.local/share/Trash`.
pub fn home_trash() -> Option<PathBuf> {
    crate::xdg_dir("XDG_DATA_HOME", ".local/share").map(|d| d.join("Trash"))
}

/// What trashing a set of paths did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    /// Each path trashed, and where it went.
    pub trashed: Vec<(PathBuf, PathBuf)>,
    /// Each path left where it is, and why.
    pub refused: Vec<(PathBuf, String)>,
}

/// Why a path can't go to the Trash, if it can't.
pub fn refusal(path: &Path, trash: &Path) -> Option<String> {
    if !path.is_absolute() {
        return Some("not an absolute path".into());
    }
    if crate::nix::in_store(path) {
        return Some("it is in the Nix store, which Yukimi's Storage cleans up".into());
    }
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => return Some(e.to_string()),
    };
    let real =
        path.parent().and_then(|p| fs::canonicalize(p).ok()).map(|p| p.join(path.file_name().unwrap_or_default()));
    let real = real.unwrap_or_else(|| path.to_path_buf());
    if crate::nix::in_store(&real) {
        return Some("it is in the Nix store, which Yukimi's Storage cleans up".into());
    }
    let trash_real = fs::canonicalize(trash).unwrap_or_else(|_| trash.to_path_buf());
    if real.starts_with(&trash_real) || trash_real.starts_with(&real) {
        return Some("it is the Trash or holds it".into());
    }
    if let Some(home) = crate::home_dir() {
        let home = fs::canonicalize(&home).unwrap_or(home);
        if home.starts_with(&real) {
            return Some("it is the home folder or holds it".into());
        }
    }
    if real == Path::new("/") {
        return Some("it is the root folder".into());
    }
    let trash_dev = trash_device(trash);
    match trash_dev {
        Some(dev) if dev == meta.dev() => None,
        Some(_) => Some("it is on another filesystem than the home folder's Trash".into()),
        None => Some("the Trash can't be made".into()),
    }
}

/// The device the trash is on (made if it isn't there yet).
fn trash_device(trash: &Path) -> Option<u64> {
    if fs::create_dir_all(trash.join("files")).is_err() || fs::create_dir_all(trash.join("info")).is_err() {
        return None;
    }
    fs::metadata(trash).ok().map(|m| m.dev())
}

/// Move `paths` to the trash at `trash`. `sizes` gives the bytes of
/// folders (for the trash's `directorysizes` cache), when known.
pub fn trash(paths: &[PathBuf], trash: &Path, sizes: &dyn Fn(&Path) -> Option<u64>) -> Outcome {
    let mut out = Outcome::default();
    for path in paths {
        if let Some(why) = refusal(path, trash) {
            out.refused.push((path.clone(), why));
            continue;
        }
        match trash_one(path, trash, sizes) {
            Ok(to) => out.trashed.push((path.clone(), to)),
            Err(e) => out.refused.push((path.clone(), e.to_string())),
        }
    }
    out
}

fn trash_one(path: &Path, trash: &Path, sizes: &dyn Fn(&Path) -> Option<u64>) -> std::io::Result<PathBuf> {
    let files = trash.join("files");
    let info = trash.join("info");
    let is_dir = fs::symlink_metadata(path)?.is_dir();
    let name = path.file_name().ok_or_else(|| std::io::Error::other("no file name"))?.to_os_string();
    let (stem, ext) = split_name(name.as_bytes());
    let deleted = local_timestamp();
    let body = format!("[Trash Info]\nPath={}\nDeletionDate={deleted}\n", encode(path));
    for n in 1u32.. {
        let candidate: Vec<u8> = if n == 1 {
            name.as_bytes().to_vec()
        } else {
            let mut c = stem.to_vec();
            c.extend_from_slice(format!(".{n}").as_bytes());
            c.extend_from_slice(ext);
            c
        };
        let candidate = std::ffi::OsStr::from_bytes(&candidate);
        let target = files.join(candidate);
        let mut info_name = candidate.to_os_string();
        info_name.push(".trashinfo");
        let info_path = info.join(&info_name);
        if fs::symlink_metadata(&target).is_ok() {
            continue;
        }
        // The info file, made exclusively, claims the name.
        let mut f = match OpenOptions::new().write(true).create_new(true).open(&info_path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        if let Err(e) = f.write_all(body.as_bytes()).and_then(|()| f.sync_all()) {
            let _ = fs::remove_file(&info_path);
            return Err(e);
        }
        drop(f);
        if let Err(e) = fs::rename(path, &target) {
            let _ = fs::remove_file(&info_path);
            return Err(e);
        }
        if is_dir && let Some(size) = sizes(path) {
            let mtime = fs::metadata(&info_path).map(|m| m.mtime()).unwrap_or(0);
            let _ = add_directory_size(trash, candidate.as_bytes(), size, mtime);
        }
        return Ok(target);
    }
    unreachable!("some name is free")
}

/// `name.tar.gz` → (`name.tar`, `.gz`); `.hidden` and `plain` keep it all.
fn split_name(name: &[u8]) -> (&[u8], &[u8]) {
    match name.iter().rposition(|&b| b == b'.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, &[]),
    }
}

/// The trash's `directorysizes` cache gets a line for a folder trashed:
/// `size mtime percent-encoded-name`, the file replaced atomically.
fn add_directory_size(trash: &Path, name: &[u8], size: u64, mtime: i64) -> std::io::Result<()> {
    let file = trash.join("directorysizes");
    let mut text = fs::read_to_string(&file).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("{size} {mtime} {}\n", encode_bytes(name, false)));
    crate::write_atomic(&file, text.as_bytes())
}

/// A path percent-encoded as the spec asks (RFC 2396), slashes kept.
pub fn encode(path: &Path) -> String {
    encode_bytes(path.as_os_str().as_bytes(), true)
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

/// Now, in local time, as the spec's `DeletionDate` wants: `2026-10-17T21:42:05`.
fn local_timestamp() -> String {
    // SAFETY: `localtime_r` writes only into `tm`.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return crate::time::rfc3339(now).trim_end_matches('Z').to_owned();
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

/// Empty the trash at `trash` for good: everything in `files/`, `info/`
/// and `expunged/`, and the `directorysizes` cache. Refuses a folder that
/// isn't a trash (named `Trash` or `.Trash-…`, holding `files/`). The
/// number of items deleted.
pub fn empty(trash: &Path) -> std::io::Result<usize> {
    let name = trash.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !(name == "Trash" || name.starts_with(".Trash")) || !trash.join("files").is_dir() {
        return Err(std::io::Error::other(format!("{} isn't a trash folder", trash.display())));
    }
    let mut count = 0;
    for part in ["files", "info", "expunged"] {
        let Ok(rd) = fs::read_dir(trash.join(part)) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            // Never through a link: a link in the trash goes, not its target.
            let is_dir = fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir());
            if is_dir {
                fs::remove_dir_all(&p)?
            } else {
                fs::remove_file(&p)?
            }
            if part == "files" {
                count += 1;
            }
        }
    }
    match fs::remove_file(trash.join("directorysizes")) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    Ok(count)
}

/// What's in the trash at `trash`: the items in `files/`.
pub fn items(trash: &Path) -> Vec<PathBuf> {
    fs::read_dir(trash.join("files")).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trashes_by_the_spec() {
        let dir = crate::testdir("trash");
        let trash_dir = dir.join("Trash");
        let a = dir.join("a file.txt");
        fs::write(&a, b"hello").unwrap();
        let folder = dir.join("folder");
        fs::create_dir_all(folder.join("inner")).unwrap();
        let second = dir.join("again");
        fs::create_dir_all(&second).unwrap();
        fs::write(second.join("a file.txt"), b"two").unwrap();

        let out = trash(&[a.clone(), folder.clone()], &trash_dir, &|_| Some(4096));
        assert!(out.refused.is_empty(), "{:?}", out.refused);
        assert!(!a.exists() && !folder.exists());
        assert!(trash_dir.join("files/a file.txt").exists());
        let info = fs::read_to_string(trash_dir.join("info/a file.txt.trashinfo")).unwrap();
        assert!(info.starts_with("[Trash Info]\nPath="));
        assert!(info.contains("a%20file.txt\n"));
        assert!(info.contains("DeletionDate="));
        let sizes = fs::read_to_string(trash_dir.join("directorysizes")).unwrap();
        assert!(sizes.starts_with("4096 ") && sizes.trim_end().ends_with(" folder"));

        // A second item of the same name gets a name of its own.
        let out = trash(&[second.join("a file.txt")], &trash_dir, &|_| None);
        assert_eq!(out.trashed[0].1, trash_dir.join("files/a file.2.txt"));
        assert!(trash_dir.join("info/a file.2.txt.trashinfo").exists());
    }

    #[test]
    fn refuses_what_it_must() {
        let dir = crate::testdir("trash-refuse");
        let trash_dir = dir.join("Trash");
        fs::create_dir_all(&trash_dir).unwrap();
        let out = trash(
            &[
                PathBuf::from("/nix/store/abc-x"),
                trash_dir.clone(),
                dir.clone(),
                PathBuf::from("relative"),
                dir.join("missing"),
            ],
            &trash_dir,
            &|_| None,
        );
        assert!(out.trashed.is_empty());
        assert_eq!(out.refused.len(), 5);
        assert!(out.refused[0].1.contains("Nix store"));
        assert!(out.refused[1].1.contains("Trash"));
        assert!(out.refused[2].1.contains("Trash"));
        if let Some(home) = crate::home_dir() {
            assert!(refusal(&home, &trash_dir).is_some());
        }
    }

    #[test]
    fn empties_only_a_trash() {
        let dir = crate::testdir("trash-empty");
        let trash_dir = dir.join("Trash");
        let outside = dir.join("outside");
        fs::write(&outside, b"stays").unwrap();
        let a = dir.join("a");
        fs::create_dir_all(a.join("inner")).unwrap();
        fs::write(a.join("inner/f"), b"x").unwrap();
        // A link in the trash to something outside: the link goes, not what it names.
        let link = dir.join("link");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let out = trash(&[a, link], &trash_dir, &|_| Some(1));
        assert_eq!(out.trashed.len(), 2);
        assert_eq!(empty(&trash_dir).unwrap(), 2);
        assert!(items(&trash_dir).is_empty());
        assert_eq!(fs::read_dir(trash_dir.join("info")).unwrap().count(), 0);
        assert!(!trash_dir.join("directorysizes").exists());
        assert!(outside.exists());
        // Not a trash: refused.
        assert!(empty(&dir).is_err());
    }

    #[test]
    fn encodes_paths() {
        assert_eq!(encode(Path::new("/home/a b/c%d")), "/home/a%20b/c%25d");
        assert_eq!(split_name(b"x.tar.gz"), (&b"x.tar"[..], &b".gz"[..]));
        assert_eq!(split_name(b".bashrc"), (&b".bashrc"[..], &b""[..]));
    }
}
