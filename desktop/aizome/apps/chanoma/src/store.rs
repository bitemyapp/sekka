//! Where Chanoma keeps things:
//!
//! - `$XDG_CONFIG_HOME/aizome/chanoma.json`: the grown-ups' settings and
//!   rules (the PIN's hash, what kids may open, time limits, folders);
//! - `$XDG_STATE_HOME/aizome/chanoma/`: today's minutes (`usage.json`),
//!   wrong PINs (`lockout.json`), what was played lately (`recent.json`)
//!   and controllers' button layouts set up here (`controllers.json`);
//! - `$XDG_DATA_HOME/aizome/chanoma/`: games' saves (`saves/`), where they
//!   were left (`states/`), each service's browser profile (`browser/`)
//!   and BIOS files cores ask for (`system/`).
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
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

impl Dirs {
    /// The XDG places.
    pub fn standard() -> Dirs {
        Dirs {
            config: xdg("XDG_CONFIG_HOME", ".config").join("aizome"),
            state: xdg("XDG_STATE_HOME", ".local/state").join("aizome/chanoma"),
            data: xdg("XDG_DATA_HOME", ".local/share").join("aizome/chanoma"),
        }
    }

    /// Everything under `root` (tests).
    pub fn under(root: &Path) -> Dirs {
        Dirs { config: root.join("config"), state: root.join("state"), data: root.join("data") }
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

    pub fn browser(&self) -> PathBuf {
        self.data.join("browser")
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

/// Write `value` to `path` whole: a sibling file renamed over it.
pub fn write<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    write_bytes(path, &bytes)
}

/// Write `bytes` to `path` whole.
pub fn write_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.tmp"));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}
