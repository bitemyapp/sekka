//! Mounted filesystems, from `/proc/self/mountinfo`, with their space from
//! `statvfs`: the disk picker's list, and what the scanner must not enter.

use std::collections::HashSet;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// One mounted filesystem.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Mount {
    pub mount_point: PathBuf,
    /// What's mounted: `/dev/nvme0n1p2`, `tmpfs`, a server's share.
    pub source: String,
    pub fs_type: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// Free to someone who isn't root.
    pub available_bytes: u64,
    pub read_only: bool,
}

impl Mount {
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.free_bytes)
    }

    /// Used share of the whole, 0–1.
    pub fn used_fraction(&self) -> f64 {
        if self.total_bytes == 0 { 0.0 } else { self.used_bytes() as f64 / self.total_bytes as f64 }
    }

    /// A name for the picker: "Home", the last part of the mount point, or
    /// "System" for `/`.
    pub fn title(&self) -> String {
        if self.mount_point == Path::new("/") {
            return "System".into();
        }
        if let Some(home) = crate::home_dir()
            && self.mount_point == home
        {
            return "Home".into();
        }
        self.mount_point
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.mount_point.display().to_string())
    }
}

/// Filesystems that hold no files on any disk: never scanned, never listed.
pub const PSEUDO: &[&str] = &[
    "proc",
    "sysfs",
    "devtmpfs",
    "devpts",
    "cgroup",
    "cgroup2",
    "securityfs",
    "debugfs",
    "tracefs",
    "fusectl",
    "configfs",
    "bpf",
    "mqueue",
    "hugetlbfs",
    "pstore",
    "efivarfs",
    "autofs",
    "binfmt_misc",
    "rpc_pipefs",
    "nsfs",
    "selinuxfs",
    "ramfs",
    "fuse.portal",
    "fuse.gvfsd-fuse",
    "overlay",
    "squashfs",
    "nfsd",
];

/// Whether a filesystem type is a pseudo-filesystem.
pub fn is_pseudo(fs_type: &str) -> bool {
    PSEUDO.contains(&fs_type)
}

/// Every mount, as the kernel lists them (pseudo ones too), without space.
pub fn mountinfo() -> Vec<(PathBuf, String, String, bool)> {
    let Ok(text) = std::fs::read_to_string("/proc/self/mountinfo") else {
        return Vec::new();
    };
    parse_mountinfo(&text)
}

/// `(mount point, source, type, read-only)` from mountinfo's text.
pub fn parse_mountinfo(text: &str) -> Vec<(PathBuf, String, String, bool)> {
    let mut out = Vec::new();
    for line in text.lines() {
        // id parent major:minor root mount-point options [optional…] - type source super-options
        let Some((left, right)) = line.split_once(" - ") else { continue };
        let l: Vec<&str> = left.split(' ').collect();
        let r: Vec<&str> = right.split(' ').collect();
        if l.len() < 6 || r.len() < 2 {
            continue;
        }
        let point = PathBuf::from(unescape(l[4]));
        let ro = l[5].split(',').any(|o| o == "ro");
        out.push((point, unescape(r[1]), r[0].to_owned(), ro));
    }
    out
}

/// Mountinfo escapes space, tab, newline and backslash as octal.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c)) {
            let v = (b[i + 1] - b'0') * 64 + (b[i + 2] - b'0') * 8 + (b[i + 3] - b'0');
            out.push(v);
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Where pseudo-filesystems are mounted.
pub fn pseudo_mount_points() -> HashSet<PathBuf> {
    mountinfo().into_iter().filter(|(_, _, t, _)| is_pseudo(t)).map(|(p, ..)| p).collect()
}

/// Space on the filesystem holding `path`: (total, free, available).
pub fn space(path: &Path) -> Option<(u64, u64, u64)> {
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `statvfs` writes only into `s`, and `c` is a valid C string.
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let f = s.f_frsize as u64;
    Some((s.f_blocks as u64 * f, s.f_bfree as u64 * f, s.f_bavail as u64 * f))
}

/// The filesystems worth showing in the picker: real ones with space,
/// each device once (at its shortest mount point), `/` and the home's
/// first, then by mount point. Bind mounts of one device show once.
pub fn list() -> Vec<Mount> {
    let mut seen_sources: HashSet<String> = HashSet::new();
    let mut all = mountinfo();
    all.sort_by(|a, b| a.0.as_os_str().len().cmp(&b.0.as_os_str().len()).then(a.0.cmp(&b.0)));
    let mut out = Vec::new();
    for (point, source, fs_type, ro) in all {
        if is_pseudo(&fs_type) || fs_type == "tmpfs" || fs_type == "devtmpfs" {
            continue;
        }
        if point.starts_with("/proc") || point.starts_with("/sys") || point.starts_with("/run/user") {
            continue;
        }
        if point.starts_with("/nix/store") {
            continue;
        }
        let key = if source.starts_with('/') { source.clone() } else { format!("{source}@{}", point.display()) };
        if !seen_sources.insert(key) {
            continue;
        }
        let Some((total, free, avail)) = space(&point) else { continue };
        if total == 0 {
            continue;
        }
        out.push(Mount {
            mount_point: point,
            source,
            fs_type,
            total_bytes: total,
            free_bytes: free,
            available_bytes: avail,
            read_only: ro,
        });
    }
    out
}

/// The mount `path` is on (the longest mount point above it).
pub fn mount_of(path: &Path) -> Option<Mount> {
    let all = mountinfo();
    let (point, source, fs_type, ro) =
        all.into_iter().filter(|(p, ..)| path.starts_with(p)).max_by_key(|(p, ..)| p.as_os_str().len())?;
    let (total, free, avail) = space(path).unwrap_or((0, 0, 0));
    Some(Mount {
        mount_point: point,
        source,
        fs_type,
        total_bytes: total,
        free_bytes: free,
        available_bytes: avail,
        read_only: ro,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mountinfo() {
        let text = "22 1 0:21 / /proc rw,nosuid shared:5 - proc proc rw\n\
                    27 1 259:2 / / rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw\n\
                    40 27 259:2 /home /home\\040dir ro,relatime - ext4 /dev/nvme0n1p2 rw\n";
        let m = parse_mountinfo(text);
        assert_eq!(m.len(), 3);
        assert_eq!(m[0], (PathBuf::from("/proc"), "proc".into(), "proc".into(), false));
        assert_eq!(m[2].0, PathBuf::from("/home dir"));
        assert!(m[2].3);
        assert!(is_pseudo(&m[0].2));
    }

    #[test]
    fn the_root_has_space() {
        let (total, free, _) = space(Path::new("/")).unwrap();
        assert!(total > 0 && free <= total);
    }
}
