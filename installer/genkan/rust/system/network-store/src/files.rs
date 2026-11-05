// SPDX-License-Identifier: GPL-3.0-or-later
//! Writing files so a power loss at any point leaves the old file or the
//! new one: a hidden temporary written and synced, renamed into place, and
//! the directory synced.

use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// The hidden temporary `path` is written under: NetworkManager ignores
/// names starting with a dot.
pub fn temporary(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".sekka-new-{name}"))
}

pub fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

/// `dir`, made if missing, with `mode`.
pub fn ensure_dir(dir: &Path, mode: u32) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, Permissions::from_mode(mode))
}

/// Replace `path` with `bytes`, root's, with `mode`.
pub fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    let tmp = temporary(path);
    let _ = fs::remove_file(&tmp);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&tmp)?;
    file.write_all(bytes)?;
    file.set_permissions(Permissions::from_mode(mode))?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)?;
    if let Some(dir) = path.parent() {
        sync_dir(dir)?;
    }
    Ok(())
}

/// Remove the hidden temporaries a killed run left in `dir`.
pub fn remove_temporaries(dir: &Path) -> io::Result<()> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(".sekka-new-")
        {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}
