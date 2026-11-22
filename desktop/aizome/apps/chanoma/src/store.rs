//! Where Chanoma keeps things:
//!
//! - `$XDG_CONFIG_HOME/aizome/chanoma.json`: the grown-ups' settings and
//!   rules (the PIN's hash, what kids may open, time limits, folders);
//! - `$XDG_STATE_HOME/aizome/chanoma/`: today's minutes (`usage.json`),
//!   wrong PINs (`lockout.json`), what was played lately (`recent.json`)
//!   and controllers' button layouts set up here (`controllers.json`);
//! - `$XDG_DATA_HOME/aizome/chanoma/`: games' saves (`saves/`), where they
//!   were left (`states/`), each service's browser profile (`browser/`)
//!   and BIOS files cores ask for (`system/`; `system-free/` when only
//!   the free ones are offered).
//! - `$XDG_CACHE_HOME/aizome/chanoma/unzipped/`: the last few zipped games
//!   played, unpacked for their cores.
//! - `$XDG_CACHE_HOME/aizome/chanoma/art/`: games' box art fetched from
//!   libretro-thumbnails, by system (`art/snes/Game (USA).png`), and a
//!   `.none` beside where one wasn't found.
//!
//! Every file is written whole and renamed over the old one.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirs {
    /// Holds `chanoma.json`.
    pub config: PathBuf,
    pub state: PathBuf,
    pub data: PathBuf,
    /// The files index Aizome's apps share (`aizome_index`).
    pub index: PathBuf,
    /// What can be made again: zipped games unpacked (`unzipped/`), box
    /// art fetched (`art/`).
    pub cache: PathBuf,
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

impl Dirs {
    /// The shared files index, or one of its own (a temporary one) if it
    /// can't be opened.
    pub fn open_index(&self) -> aizome_index::Index {
        aizome_index::Index::open_at(&self.index).unwrap_or_else(|e| {
            log::warn!("chanoma: the files index: {e}; keeping one of its own");
            let own = std::env::temp_dir().join(format!("chanoma-index-{}.sqlite", std::process::id()));
            aizome_index::Index::open_at(&own).expect("a temporary files index")
        })
    }

    /// The XDG places.
    pub fn standard() -> Dirs {
        Dirs {
            config: xdg("XDG_CONFIG_HOME", ".config").join("aizome"),
            state: xdg("XDG_STATE_HOME", ".local/state").join("aizome/chanoma"),
            data: xdg("XDG_DATA_HOME", ".local/share").join("aizome/chanoma"),
            index: xdg("XDG_CACHE_HOME", ".cache").join("aizome/index/files.sqlite"),
            cache: xdg("XDG_CACHE_HOME", ".cache").join("aizome/chanoma"),
        }
    }

    /// Everything under `root` (tests).
    pub fn under(root: &Path) -> Dirs {
        Dirs {
            config: root.join("config"),
            state: root.join("state"),
            data: root.join("data"),
            index: root.join("cache/index.sqlite"),
            cache: root.join("cache/chanoma"),
        }
    }

    pub fn settings(&self) -> PathBuf {
        self.config.join("chanoma.json")
    }

    pub fn saves(&self) -> PathBuf {
        self.data.join("saves")
    }

    pub fn states(&self) -> PathBuf {
        self.data.join("states")
    }

    pub fn system(&self) -> PathBuf {
        self.data.join("system")
    }

    /// The cores' system folder when they're offered only free BIOS
    /// images (`Config::free_bios`).
    pub fn free_system(&self) -> PathBuf {
        self.data.join("system-free")
    }

    pub fn browser(&self) -> PathBuf {
        self.data.join("browser")
    }

    pub fn unzipped(&self) -> PathBuf {
        self.cache.join("unzipped")
    }

    /// Box art fetched, by system.
    pub fn art(&self) -> PathBuf {
        self.cache.join("art")
    }
}

/// `path` as JSON, or the default where it's missing or unreadable.
pub fn read<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// `path` as JSON, if it's there and reads.
pub fn read_opt<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

/// What a file the rules stand on held: one that's there and can't be read
/// isn't taken for one that was never written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kept<T> {
    /// No such file: nothing was kept yet.
    Missing,
    Read(T),
    /// It's there, and it can't be read or isn't what was written: why.
    Unreadable(String),
}

/// `path` as JSON, telling a file that isn't there from one that can't be
/// read.
pub fn read_kept<T: for<'de> Deserialize<'de>>(path: &Path) -> Kept<T> {
    match std::fs::read(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Kept::Missing,
        Err(e) => Kept::Unreadable(e.to_string()),
        Ok(b) => match serde_json::from_slice(&b) {
            Ok(v) => Kept::Read(v),
            Err(e) => Kept::Unreadable(e.to_string()),
        },
    }
}

/// Write `value` to `path` whole: a sibling file renamed over it.
pub fn write<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    write_whole(path, &bytes, false)
}

/// Write `value` to `path` whole, on the disk before it takes the old
/// file's place, and its new name after: for what the rules stand on (the
/// settings, wrong PINs, today's minutes). Renamed without that, a file
/// can come back from a power cut empty on some filesystems.
pub fn write_kept<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    write_whole(path, &bytes, true)
}

/// Write `bytes` to `path` whole.
pub fn write_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_whole(path, bytes, false)
}

fn write_whole(path: &Path, bytes: &[u8], synced: bool) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.tmp"));
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        if synced {
            f.sync_all()?;
        }
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written?;
    // The folder too, so the rename is kept. Where a folder can't be
    // synced the file is still whole, old or new.
    if synced && let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_cant_be_read_isnt_one_that_was_never_written() {
        let dir = std::env::temp_dir().join(format!("chanoma-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("kept.json");
        assert_eq!(read_kept::<Vec<u32>>(&path), Kept::Missing);
        write_kept(&path, &vec![1u32, 2, 3]).unwrap();
        assert_eq!(read_kept::<Vec<u32>>(&path), Kept::Read(vec![1, 2, 3]));
        assert!(!dir.join(".kept.json.tmp").exists(), "the sibling file is gone once renamed");
        // Cut short, emptied, or of another shape: there, and unreadable.
        for damaged in ["[1, 2", "", "{\"a\": 1}"] {
            std::fs::write(&path, damaged).unwrap();
            assert!(matches!(read_kept::<Vec<u32>>(&path), Kept::Unreadable(_)), "{damaged:?}");
        }
        // The lenient read still takes it for nothing kept.
        assert_eq!(read::<Vec<u32>>(&path), Vec::<u32>::new());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
