// SPDX-License-Identifier: GPL-3.0-or-later
//! The installation this app finishes, read from beside it and checked.
use std::ffi::CString;
use std::fs;
use std::io::Read;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

use sekka_mac_contract::{
    Digest, Hasher, InventoryFile, RECOVERY_CONFIG_FILE, RECOVERY_DATA_DIR, RecoveryConfig,
    RecoverySchema, canonical, limits,
};

pub struct Install {
    pub config: RecoveryConfig,
    /// The stub's System volume, where the app and its data are.
    pub volume: PathBuf,
    pub data: PathBuf,
}

/// The volume an app bundle sits at the top of:
/// `/Volumes/Sekka/Sekka Recovery.app` → `/Volumes/Sekka`.
pub fn volume_of(bundle: &Path) -> Option<PathBuf> {
    bundle.parent().map(Path::to_path_buf)
}

/// Whether the file system holding `path` is mounted read-only, as
/// recoveryOS mounts the stub's System volume.
pub fn read_only(path: &Path) -> Result<bool, String> {
    let c = CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let mut stat = MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: a NUL-terminated path, and room for what statvfs fills in.
    if unsafe { libc::statvfs(c.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return Err(format!("{}: {}", path.display(), std::io::Error::last_os_error()));
    }
    // SAFETY: filled in by the call, which succeeded.
    let stat = unsafe { stat.assume_init() };
    Ok(stat.f_flag & libc::ST_RDONLY != 0)
}

/// Hash a file, refusing links and anything over `limit` bytes.
pub fn digest(path: &Path, limit: u64) -> Result<(u64, Digest), String> {
    let mut f = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let meta = f.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > limit {
        return Err(format!("{} isn't a plain file of the expected size", path.display()));
    }
    let mut hasher = Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buf[..n]);
    }
    Ok((size, hasher.finish()))
}

impl Install {
    /// Read and check everything beside the app.
    pub fn load(volume: &Path) -> Result<Self, String> {
        let data = volume.join(RECOVERY_DATA_DIR);
        let bytes = fs::read(data.join(RECOVERY_CONFIG_FILE))
            .map_err(|e| format!("This installation's details are missing ({e}). Start the installation again from macOS."))?;
        let config: RecoveryConfig = canonical::from_slice(&bytes, limits::RECOVERY, RecoverySchema::VALUE)
            .map_err(|e| format!("This installation's details can't be read: {e}"))?;
        config.check().map_err(|e| format!("This installation's details are inconsistent: {e}"))?;
        let install = Self {
            config,
            volume: volume.into(),
            data,
        };
        install.verify_files()?;
        Ok(install)
    }

    pub fn path(&self, file: &InventoryFile) -> PathBuf {
        let mut p = self.data.clone();
        for c in Path::new(file.path.as_str()).components() {
            if let Component::Normal(c) = c {
                p.push(c);
            }
        }
        p
    }

    /// Every file the configuration names is there, with its size and
    /// digest. Called at launch and again right before it is used.
    pub fn verify_files(&self) -> Result<(), String> {
        let check = |f: &InventoryFile, limit: u64| -> Result<(), String> {
            let (size, digest) = digest(&self.path(f), limit)?;
            if size != f.size || digest != f.digest {
                return Err(format!(
                    "{} isn't what the installation wrote. Start the installation again from macOS.",
                    f.path
                ));
            }
            Ok(())
        };
        check(&self.config.boot_object, 64 * 1024 * 1024)?;
        for f in &self.config.firmware {
            check(f, 256 * 1024 * 1024)?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sekka_mac_contract::{Owner, StubTarget, Uuid};

    pub fn fixture(dir: &Path) -> Install {
        let data = dir.join(RECOVERY_DATA_DIR);
        fs::create_dir_all(data.join("firmware")).unwrap();
        let file = |rel: &str, bytes: &[u8]| {
            fs::write(data.join(rel), bytes).unwrap();
            InventoryFile {
                path: rel.parse().unwrap(),
                size: bytes.len() as u64,
                digest: Digest::of(bytes),
            }
        };
        let u = |n: u8| Uuid::from_bytes([n; 16]);
        let config = RecoveryConfig {
            schema: RecoverySchema,
            installation: u(1),
            release: "sekka-j514s-4882dc33".parse().unwrap(),
            volume_name: "Sekka".parse().unwrap(),
            stub: StubTarget {
                container: u(2),
                volume_group: u(3),
                system_volume: u(4),
                data_volume: u(5),
            },
            esp: u(6),
            root: u(7),
            boot_object: file("boot.bin", b"m1n1 stage 1"),
            firmware: vec![file("firmware/firmware.cpio", b"cpio"), file("firmware/manifest.txt", b"manifest")],
            owner: Owner {
                short_name: "chris".parse().unwrap(),
                full_name: "Chris Allen".parse().unwrap(),
                macos_volume_group: u(8),
            },
            os_firmware: "14.8.3".parse().unwrap(),
        };
        fs::write(data.join(RECOVERY_CONFIG_FILE), canonical::to_vec(&config).unwrap()).unwrap();
        Install::load(dir).unwrap()
    }

    #[test]
    fn everything_beside_the_app_is_checked() {
        let dir = tempfile::tempdir().unwrap();
        let install = fixture(dir.path());
        assert_eq!(install.config.owner.short_name.as_str(), "chris");
        fs::write(install.data.join("boot.bin"), b"another boot object").unwrap();
        assert!(Install::load(dir.path()).is_err());
        fs::remove_file(install.data.join("boot.bin")).unwrap();
        std::os::unix::fs::symlink(install.data.join("firmware/manifest.txt"), install.data.join("boot.bin")).unwrap();
        assert!(Install::load(dir.path()).is_err());
    }

    #[test]
    fn the_volume_is_the_bundles_parent() {
        assert_eq!(volume_of(Path::new("/Volumes/Sekka/Sekka Recovery.app")).unwrap(), Path::new("/Volumes/Sekka"));
    }

    #[test]
    fn a_writable_volume_reads_as_writable() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!read_only(dir.path()).unwrap());
        assert!(read_only(&dir.path().join("not there")).is_err());
    }
}
