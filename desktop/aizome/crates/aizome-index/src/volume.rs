//! Which filesystem a path is on, known by something that stays the same
//! when a drive comes back somewhere else: its UUID, and where in it the
//! mount starts (a Btrfs subvolume, a bind mount).

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// A mounted filesystem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    /// What it's known by in the index: `uuid:<uuid><root>`, or for a
    /// filesystem without one (tmpfs), `dev:<major>:<minor>:<source><root>`.
    pub key: String,
    /// Where it's mounted.
    pub mount: PathBuf,
    pub fstype: String,
    pub label: Option<String>,
    /// Its device number, which other filesystems mounted inside it don't
    /// share.
    pub dev: u64,
}

impl Volume {
    /// How close a folder's modified time may come to when it was listed
    /// before a change in the same tick could hide behind it: FAT and
    /// exFAT keep times to two seconds, others to a clock tick.
    pub fn racy_ns(&self) -> i64 {
        match self.fstype.as_str() {
            "vfat" | "msdos" | "exfat" | "fuseblk" | "ntfs" | "ntfs3" | "hfsplus" => 2_000_000_000,
            _ => 100_000_000,
        }
    }
}

/// The filesystem `path` (canonical) is on.
pub fn of(path: &Path) -> std::io::Result<Volume> {
    let dev = std::fs::metadata(path)?.dev();
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo")?;
    let Some(m) = mount_of(&mountinfo, path, dev, |p| std::fs::metadata(p).ok().map(|m| m.dev())) else {
        return Err(std::io::Error::other(format!("{} isn't on a mounted filesystem", path.display())));
    };
    let root = if m.root == "/" { String::new() } else { m.root };
    let by = |dir: &str| -> Option<String> {
        let device = std::fs::canonicalize(&m.source).ok()?;
        std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
            (std::fs::canonicalize(e.path()).ok()? == device).then(|| unescape(&e.file_name().to_string_lossy()))
        })
    };
    let (uuid, label) =
        if m.source.starts_with("/dev/") { (by("/dev/disk/by-uuid"), by("/dev/disk/by-label")) } else { (None, None) };
    let key = match uuid {
        Some(u) => format!("uuid:{u}{root}"),
        None => format!("dev:{}:{}:{}{root}", libc::major(dev), libc::minor(dev), m.source),
    };
    Ok(Volume { key, mount: m.mount, fstype: m.fstype, label, dev })
}

/// A line of mountinfo.
#[derive(Debug, PartialEq, Eq)]
struct Mount {
    mount: PathBuf,
    /// Where in the filesystem the mount starts.
    root: String,
    fstype: String,
    source: String,
}

/// The mount `path` is on, of those in `mountinfo`: the deepest whose
/// device is `dev`. Btrfs gives each subvolume a device of its own, which
/// mountinfo doesn't show (it gives the filesystem's), so failing that,
/// the deepest whose mount point (`dev_of`) is on `dev`. Of two mounts on
/// one place, the later is the one showing.
fn mount_of(mountinfo: &str, path: &Path, dev: u64, dev_of: impl Fn(&Path) -> Option<u64>) -> Option<Mount> {
    let majmin = format!("{}:{}", libc::major(dev), libc::minor(dev));
    let mut by_number: Option<(Mount, usize)> = None;
    let mut by_place: Option<(Mount, usize)> = None;
    for line in mountinfo.lines() {
        let fields: Vec<&str> = line.split(' ').collect();
        let Some(dash) = fields.iter().position(|f| *f == "-") else { continue };
        if fields.len() < dash + 3 || fields.len() < 5 {
            continue;
        }
        let mount = PathBuf::from(unescape(fields[4]));
        if !path.starts_with(&mount) {
            continue;
        }
        let depth = mount.as_os_str().len();
        let m = || Mount {
            mount: mount.clone(),
            root: unescape(fields[3]),
            fstype: fields[dash + 1].to_owned(),
            source: unescape(fields[dash + 2]),
        };
        if fields[2] == majmin {
            if by_number.as_ref().is_none_or(|(_, d)| depth >= *d) {
                by_number = Some((m(), depth));
            }
        } else if by_place.as_ref().is_none_or(|(_, d)| depth >= *d) && dev_of(&mount) == Some(dev) {
            by_place = Some((m(), depth));
        }
    }
    by_number.or(by_place).map(|(m, _)| m)
}

/// A mountinfo field (or a by-label name) with its escapes undone: `\040`
/// is a space, `\x20` too.
fn unescape(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let escape = b[i] == b'\\' && i + 3 < b.len();
        if escape
            && b[i + 1].is_ascii_digit()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 4], 8)
        {
            out.push(v);
            i += 4;
            continue;
        }
        if escape
            && b[i + 1] == b'x'
            && let Ok(v) = u8::from_str_radix(&s[i + 2..i + 4], 16)
        {
            out.push(v);
            i += 4;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_are_undone_and_a_path_has_a_volume() {
        assert_eq!(unescape(r"/run/media/alice/Extreme\040Pro"), "/run/media/alice/Extreme Pro");
        assert_eq!(unescape(r"Extreme\x20Pro"), "Extreme Pro");
        let v = of(&std::env::temp_dir().canonicalize().unwrap()).unwrap();
        assert!(v.key.starts_with("uuid:") || v.key.starts_with("dev:"), "{}", v.key);
        assert!(std::env::temp_dir().canonicalize().unwrap().starts_with(&v.mount));
    }

    #[test]
    fn a_btrfs_subvolume_is_found_by_its_mount_point() {
        // Root and home are subvolumes of one Btrfs filesystem, which
        // mountinfo calls 0:26; stat gives each subvolume its own number.
        let mountinfo = "\
22 1 0:26 /@ / rw,noatime shared:1 - btrfs /dev/nvme0n1p2 rw,subvol=/@
23 22 0:26 /@home /home rw,noatime shared:2 - btrfs /dev/nvme0n1p2 rw,subvol=/@home
24 22 0:5 / /dev rw,nosuid shared:3 - devtmpfs devtmpfs rw
40 23 8:1 / /home/alice/Games ro shared:4 - exfat /dev/sda1 ro
41 40 8:2 / /home/alice/Games ro shared:5 - vfat /dev/sdb1 ro
";
        let (root, home) = (libc::makedev(0, 31), libc::makedev(0, 33));
        let dev_of = |p: &Path| match p.to_str() {
            Some("/") => Some(root),
            Some("/home") => Some(home),
            _ => None,
        };
        let m = mount_of(mountinfo, Path::new("/home/alice/Music"), home, dev_of).unwrap();
        assert_eq!((m.mount.to_str(), m.root.as_str(), m.fstype.as_str()), (Some("/home"), "/@home", "btrfs"));
        let m = mount_of(mountinfo, Path::new("/etc"), root, dev_of).unwrap();
        assert_eq!((m.mount.to_str(), m.root.as_str()), (Some("/"), "/@"));
        // Found by number, the deeper mount wins, and of two on one place
        // the later, which is the one showing.
        let m = mount_of(mountinfo, Path::new("/home/alice/Games/snes"), libc::makedev(8, 2), dev_of).unwrap();
        assert_eq!(m.source, "/dev/sdb1");
        // On nothing mounted.
        assert_eq!(mount_of(mountinfo, Path::new("/srv"), libc::makedev(0, 40), dev_of), None);
    }
}
