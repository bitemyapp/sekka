//! The sidebar's places: the person's folders (XDG user directories),
//! the filesystems mounted, and their space.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::fsops::trash;

/// What a place is, for its icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlaceKind {
    Home,
    Desktop,
    Documents,
    Downloads,
    Pictures,
    Music,
    Videos,
    Folder,
    /// The current Desk's folder.
    Desk,
    /// The system's disk.
    Disk,
    /// A removable or external volume.
    Volume,
    /// A mounted image (`.iso`).
    Image,
    Trash,
    Recent,
}

/// A place in the sidebar.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    pub path: PathBuf,
    pub kind: PlaceKind,
}

/// The XDG user directories, from `$XDG_CONFIG_HOME/user-dirs.dirs`
/// (`XDG_DOWNLOAD_DIR="$HOME/Downloads"`), with the usual names where it
/// says nothing.
pub fn user_dirs(home: &Path) -> HashMap<&'static str, PathBuf> {
    let mut out: HashMap<&'static str, PathBuf> = HashMap::new();
    let defaults = [
        ("DESKTOP", "Desktop"),
        ("DOCUMENTS", "Documents"),
        ("DOWNLOAD", "Downloads"),
        ("MUSIC", "Music"),
        ("PICTURES", "Pictures"),
        ("VIDEOS", "Videos"),
        ("TEMPLATES", "Templates"),
        ("PUBLICSHARE", "Public"),
    ];
    let file = crate::xdg_dir("XDG_CONFIG_HOME", ".config").join("user-dirs.dirs");
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let parsed = parse_user_dirs(&text, home);
    for (key, name) in defaults {
        let p = parsed.get(key).cloned().unwrap_or_else(|| home.join(name));
        out.insert(key, p);
    }
    out
}

/// `XDG_<KEY>_DIR="…"` lines, with `$HOME` expanded. A folder set to the
/// home folder itself means "none" and is left out.
pub fn parse_user_dirs(text: &str, home: &Path) -> HashMap<String, PathBuf> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let Some(key) = k.trim().strip_prefix("XDG_").and_then(|k| k.strip_suffix("_DIR")) else { continue };
        let v = v.trim().trim_matches('"');
        let p = if let Some(rest) = v.strip_prefix("$HOME") {
            home.join(rest.trim_start_matches('/'))
        } else if v.starts_with('/') {
            PathBuf::from(v)
        } else {
            continue;
        };
        if p != home {
            out.insert(key.to_owned(), p);
        }
    }
    out
}

/// The person's places that are there: Home, then Desktop, Documents,
/// Downloads, Pictures, Music and Videos.
pub fn standard_places(home: &Path) -> Vec<Place> {
    let dirs = user_dirs(home);
    let mut out = vec![Place { name: "Home".into(), path: home.to_path_buf(), kind: PlaceKind::Home }];
    for (key, name, kind) in [
        ("DESKTOP", "Desktop", PlaceKind::Desktop),
        ("DOCUMENTS", "Documents", PlaceKind::Documents),
        ("DOWNLOAD", "Downloads", PlaceKind::Downloads),
        ("PICTURES", "Pictures", PlaceKind::Pictures),
        ("MUSIC", "Music", PlaceKind::Music),
        ("VIDEOS", "Videos", PlaceKind::Videos),
    ] {
        if let Some(p) = dirs.get(key)
            && p.is_dir()
        {
            let shown = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| name.into());
            out.push(Place { name: shown, path: p.clone(), kind });
        }
    }
    out
}

/// The kind a folder is, if it's one of the person's places.
pub fn kind_of(path: &Path, places: &[Place]) -> Option<PlaceKind> {
    places.iter().find(|p| p.path == path).map(|p| p.kind)
}

/// The disk the system runs from, named as the desktop names it
/// ("nvme0", "sda"): the one at `/`, or, where `/` is no disk (a
/// container's tmpfs), the one holding `home`.
pub fn system_disk(home: &Path) -> Option<Place> {
    let text = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
    let mounts = trash::parse_mountinfo(&text);
    let root = mounts.iter().find(|m| m.mount_point == Path::new("/"));
    let chosen = match root {
        Some(r) if r.source.starts_with("/dev/") => r,
        _ => mounts
            .iter()
            .filter(|m| home.starts_with(&m.mount_point) && m.source.starts_with("/dev/"))
            .max_by_key(|m| m.mount_point.as_os_str().len())?,
    };
    Some(Place {
        name: disk_name(&chosen.source).unwrap_or_else(|| "System".into()),
        path: chosen.mount_point.clone(),
        kind: PlaceKind::Disk,
    })
}

/// A device's short name: `/dev/nvme0n1p2` → `nvme0`, `/dev/sda2` →
/// `sda`, `/dev/mapper/root` → `root`.
pub fn disk_name(source: &str) -> Option<String> {
    let dev = source.strip_prefix("/dev/")?;
    let dev = dev.rsplit('/').next().unwrap_or(dev);
    if let Some(rest) = dev.strip_prefix("nvme") {
        let n: String = rest.chars().take_while(char::is_ascii_digit).collect();
        return Some(format!("nvme{n}"));
    }
    if let Some(rest) = dev.strip_prefix("mmcblk") {
        let n: String = rest.chars().take_while(char::is_ascii_digit).collect();
        return Some(format!("mmcblk{n}"));
    }
    if dev.starts_with("sd") || dev.starts_with("vd") || dev.starts_with("hd") || dev.starts_with("xvd") {
        return Some(dev.trim_end_matches(|c: char| c.is_ascii_digit()).to_owned());
    }
    Some(dev.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_user_dirs() {
        let home = Path::new("/home/u");
        let text = "# comment\nXDG_DOWNLOAD_DIR=\"$HOME/Fetched\"\nXDG_MUSIC_DIR=\"/srv/music\"\nXDG_PUBLICSHARE_DIR=\"$HOME/\"\nXDG_BAD=\"x\"\n";
        let d = parse_user_dirs(text, home);
        assert_eq!(d.get("DOWNLOAD"), Some(&PathBuf::from("/home/u/Fetched")));
        assert_eq!(d.get("MUSIC"), Some(&PathBuf::from("/srv/music")));
        assert_eq!(d.get("PUBLICSHARE"), None, "the home folder means none");
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn names_disks() {
        assert_eq!(disk_name("/dev/nvme0n1p2").as_deref(), Some("nvme0"));
        assert_eq!(disk_name("/dev/sda2").as_deref(), Some("sda"));
        assert_eq!(disk_name("/dev/mmcblk0p1").as_deref(), Some("mmcblk0"));
        assert_eq!(disk_name("/dev/mapper/cryptroot").as_deref(), Some("cryptroot"));
        assert_eq!(disk_name("tmpfs"), None);
    }
}
