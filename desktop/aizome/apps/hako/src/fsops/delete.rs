//! Removing trees: what emptying the Trash, deleting from it, a move's
//! sources once they've arrived, and abandoned partial copies come to.
//!
//! The walk never follows links (a link is removed, not what it points
//! to), stays on the filesystem it starts on, and makes folders of its
//! own writable when they're read-only (as a build's caches often are)
//! so their contents can go.

use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use super::{Cancelled, Control};

/// What a removal came to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Removed {
    pub items: u64,
    pub bytes: u64,
    /// What couldn't be removed, and why.
    pub failed: Vec<(std::path::PathBuf, String)>,
}

/// Remove `path` and everything under it. `each` hears every item
/// removed (its size). Stops at a cancel, leaving what's left.
pub fn remove_tree(path: &Path, control: &Control, each: &mut dyn FnMut(u64)) -> Result<Removed, Cancelled> {
    let mut out = Removed::default();
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
        Err(e) => {
            out.failed.push((path.to_path_buf(), super::describe(&e)));
            return Ok(out);
        }
    };
    remove_any(path, &meta, meta.dev(), control, each, &mut out)?;
    Ok(out)
}

/// [`remove_tree`] that can't be cancelled and hears nothing (cleaning
/// up a partial copy).
pub fn remove_quietly(path: &Path) -> Removed {
    remove_tree(path, &Control::new(), &mut |_| {}).unwrap_or_default()
}

fn remove_any(
    path: &Path,
    meta: &fs::Metadata,
    dev: u64,
    control: &Control,
    each: &mut dyn FnMut(u64),
    out: &mut Removed,
) -> Result<(), Cancelled> {
    control.checkpoint()?;
    if meta.is_dir() {
        if meta.dev() != dev {
            out.failed.push((path.to_path_buf(), "it's another filesystem mounted here".into()));
            return Ok(());
        }
        let entries = match fs::read_dir(path) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied && make_writable(path, meta) => {
                match fs::read_dir(path) {
                    Ok(rd) => rd,
                    Err(e) => {
                        out.failed.push((path.to_path_buf(), super::describe(&e)));
                        return Ok(());
                    }
                }
            }
            Err(e) => {
                out.failed.push((path.to_path_buf(), super::describe(&e)));
                return Ok(());
            }
        };
        let children: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        let mut made_writable = false;
        for child in children {
            let Ok(m) = fs::symlink_metadata(&child) else { continue };
            // A read-only folder keeps its entries: open it up first.
            if !made_writable && meta.permissions().mode() & 0o200 == 0 {
                made_writable = make_writable(path, meta);
            }
            remove_any(&child, &m, dev, control, each, out)?;
        }
        match fs::remove_dir(path) {
            Ok(()) => {
                out.items += 1;
                each(0);
            }
            Err(e) => out.failed.push((path.to_path_buf(), super::describe(&e))),
        }
    } else {
        let size = if meta.is_file() { meta.len() } else { 0 };
        let mut r = fs::remove_file(path);
        if let Err(e) = &r
            && e.kind() == io::ErrorKind::PermissionDenied
            && let Some(parent) = path.parent()
            && let Ok(pm) = fs::symlink_metadata(parent)
            && make_writable(parent, &pm)
        {
            r = fs::remove_file(path);
        }
        match r {
            Ok(()) => {
                out.items += 1;
                out.bytes += size;
                each(size);
            }
            Err(e) => out.failed.push((path.to_path_buf(), super::describe(&e))),
        }
    }
    Ok(())
}

/// Give a folder of ours its owner's write permission: whether it was
/// changed (it's not ours, or already writable: no).
fn make_writable(dir: &Path, meta: &fs::Metadata) -> bool {
    let mode = meta.permissions().mode();
    if meta.uid() != super::sys::uid() || mode & 0o700 == 0o700 {
        return false;
    }
    fs::set_permissions(dir, fs::Permissions::from_mode((mode | 0o700) & 0o7777)).is_ok()
}

/// How many items and bytes are under `path` (it counted), not following
/// links; `None` if cancelled. `limit` stops counting after that many
/// items (the count is then a lower bound).
pub fn measure(path: &Path, control: &Control, limit: u64) -> Option<(u64, u64)> {
    fn walk(p: &Path, control: &Control, limit: u64, n: &mut (u64, u64)) -> Result<(), Cancelled> {
        let Ok(m) = fs::symlink_metadata(p) else { return Ok(()) };
        n.0 += 1;
        if m.is_file() {
            n.1 += m.len();
        }
        if n.0.is_multiple_of(512) {
            control.checkpoint()?;
        }
        if m.is_dir()
            && n.0 < limit
            && let Ok(rd) = fs::read_dir(p)
        {
            for e in rd.flatten() {
                walk(&e.path(), control, limit, n)?;
                if n.0 >= limit {
                    break;
                }
            }
        }
        Ok(())
    }
    let mut n = (0, 0);
    walk(path, control, limit, &mut n).ok()?;
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsops::testing::{TempDir, is_root};

    #[test]
    fn removes_trees_without_following_links() {
        let t = TempDir::new("remove");
        let keep = t.file("keep/precious.txt", "precious");
        t.file("doomed/a/b.txt", "b");
        t.file("doomed/c.txt", "cc");
        std::os::unix::fs::symlink(t.join("keep"), t.join("doomed/link-to-keep")).unwrap();
        std::os::unix::fs::symlink(&keep, t.join("doomed/a/link-to-file")).unwrap();
        let mut heard = 0;
        let r = remove_tree(&t.join("doomed"), &Control::new(), &mut |_| heard += 1).unwrap();
        assert!(r.failed.is_empty(), "{:?}", r.failed);
        assert!(!t.join("doomed").exists());
        assert_eq!(std::fs::read_to_string(&keep).unwrap(), "precious", "a link's target stays");
        assert_eq!(r.bytes, 3);
        assert_eq!(r.items, 6);
        assert_eq!(heard, 6);
        // A link to a folder, removed by itself, takes nothing with it.
        std::os::unix::fs::symlink(t.join("keep"), t.join("lnk")).unwrap();
        remove_quietly(&t.join("lnk"));
        assert!(keep.exists());
        assert!(std::fs::symlink_metadata(t.join("lnk")).is_err());
    }

    #[test]
    fn opens_read_only_folders_of_ours() {
        if is_root() {
            return;
        }
        let t = TempDir::new("remove-ro");
        t.file("cache/sub/file", "x");
        std::fs::set_permissions(t.join("cache/sub"), fs::Permissions::from_mode(0o555)).unwrap();
        std::fs::set_permissions(t.join("cache"), fs::Permissions::from_mode(0o555)).unwrap();
        let r = remove_quietly(&t.join("cache"));
        assert!(r.failed.is_empty(), "{:?}", r.failed);
        assert!(!t.join("cache").exists());
    }

    #[test]
    fn stops_when_cancelled_and_measures() {
        let t = TempDir::new("remove-cancel");
        for i in 0..20 {
            t.file(&format!("d/f{i}"), "1234");
        }
        assert_eq!(measure(&t.join("d"), &Control::new(), u64::MAX), Some((21, 80)));
        let c = Control::new();
        c.cancel();
        assert!(remove_tree(&t.join("d"), &c, &mut |_| {}).is_err());
        assert!(t.join("d/f0").exists());
        assert!(measure(&t.join("d"), &Control::new(), 5).unwrap().0 >= 5);
    }
}
