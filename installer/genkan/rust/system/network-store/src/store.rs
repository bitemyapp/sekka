// SPDX-License-Identifier: GPL-3.0-or-later
//! The store's lifecycle as types. A store is built under a hidden name and
//! proven to open through the TPM before it's committed under its own; only
//! a committed store can be opened, only an opened one checked, only a
//! checked one mounted, and only a mounted one bound where NetworkManager
//! reads. Each step consumes the last.

use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::cmd::tool;
use crate::rotation::{KEPT, Srk};
use crate::seed::Profile;
use crate::{crash_point, files, sys};

/// Where things are.
pub struct Context {
    /// The system's root: / when it's running, the target's mount when Genkan
    /// installs it. The paths below are under it.
    pub root: PathBuf,
    /// The image, its SRK, the secret-less copies, counters: root's only.
    pub state: PathBuf,
    /// NetworkManager's keyfile directory.
    pub keyfiles: PathBuf,
    /// Staging mounts and the status file.
    pub run: PathBuf,
    /// Where the system's generations are listed.
    pub profiles: PathBuf,
    /// systemd's SRK setup, run when the TPM has no SRK stored.
    pub tpm2_setup: PathBuf,
}

impl Context {
    pub fn image(&self) -> PathBuf {
        self.state.join("store.luks")
    }

    fn building(&self) -> PathBuf {
        self.state.join(".store.luks.new")
    }

    pub fn srk(&self) -> PathBuf {
        self.state.join("store.srk")
    }

    pub fn rotated(&self) -> PathBuf {
        self.state.join("rotated")
    }

    pub fn skeleton(&self) -> PathBuf {
        self.state.join("skeleton")
    }

    pub fn unseal_failures(&self) -> PathBuf {
        self.state.join("unseal-failures")
    }

    pub fn create_failures(&self) -> PathBuf {
        self.state.join("create-failures")
    }

    pub fn lock(&self) -> PathBuf {
        self.state.join("lock")
    }

    /// For desktop UIs: what the store is doing (no secrets).
    pub fn status(&self) -> PathBuf {
        self.run.join("network-store.json")
    }

    /// Set when this boot's unseal failed, for `create` to decide on.
    pub fn unseal_failed(&self) -> PathBuf {
        self.run.join("network-store-unseal-failed")
    }

    fn staging(&self, which: &str) -> PathBuf {
        self.run.join(format!("network-store-{which}"))
    }
}

/// The LUKS2 label: the store's format. A store with another is left alone.
pub const LABEL: &str = "sekka-network-store-v1";
const SIZE: &str = "64MiB";
pub const MAPPER: &str = "sekka-network-store";
const MAPPER_NEW: &str = "sekka-network-store-new";
const MAPPER_OLD: &str = "sekka-network-store-old";
const CONNECTIONS: &str = "connections";
const ATTACH_LIMIT: Duration = Duration::from_secs(15);

/// Why a store didn't open.
#[derive(Debug)]
pub enum Failure {
    /// The TPM didn't give its key: the only failure that may set it aside.
    Unseal(String),
    /// Opened, but its filesystem is damaged.
    Damaged(String),
    /// Of a format this program doesn't know.
    Unknown(String),
    Other(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Failure::Unseal(e) => write!(f, "the TPM didn't unlock it: {e}"),
            Failure::Damaged(e) => write!(f, "its filesystem is damaged: {e}"),
            Failure::Unknown(e) => write!(f, "it isn't a store this program knows: {e}"),
            Failure::Other(e) => f.write_str(e),
        }
    }
}

/// An image, the name its device opens under, and where it mounts.
struct Volume {
    image: PathBuf,
    mapper: &'static str,
    staging: PathBuf,
}

impl Volume {
    fn device(&self) -> PathBuf {
        Path::new("/dev/mapper").join(self.mapper)
    }
}

pub fn detach(mapper: &str) {
    if Path::new("/dev/mapper").join(mapper).exists() {
        let _ = tool("systemd-cryptsetup")
            .args(["detach", mapper])
            .limit(ATTACH_LIMIT)
            .run();
    }
}

fn unmount(path: &Path) -> Result<(), String> {
    tool("umount")
        .arg(path)
        .limit(Duration::from_secs(30))
        .ok()
        .map(drop)
}

/// Open `volume`'s image through the TPM: never asking for a password, and
/// giving up if the TPM doesn't answer.
fn attach(volume: &Volume) -> Result<(), Failure> {
    // A device a killed run left behind, and its mount.
    if sys::mounts().iter().any(|m| m.point == volume.staging) {
        let _ = unmount(&volume.staging);
    }
    detach(volume.mapper);
    let image = volume.image.to_string_lossy();
    tool("systemd-cryptsetup")
        .args([
            "attach",
            volume.mapper,
            &image,
            "-",
            "tpm2-device=auto,headless=1,token-timeout=10s,tries=1",
        ])
        .limit(ATTACH_LIMIT)
        .ok()
        .map(drop)
        .map_err(Failure::Unseal)
}

/// e2fsck's exit code: repaired or clean, worth `-y`, or beyond it.
#[derive(Debug, PartialEq)]
pub enum Fsck {
    Clean,
    NeedsYes,
    Failed,
}

pub fn fsck_outcome(code: Option<i32>) -> Fsck {
    match code {
        Some(code) if code & !3 == 0 => Fsck::Clean,
        Some(code) if code & !7 == 0 => Fsck::NeedsYes,
        _ => Fsck::Failed,
    }
}

fn check(device: &Path) -> Result<(), Failure> {
    let limit = Duration::from_secs(20);
    let preen = tool("e2fsck").arg("-p").arg(device).limit(limit).run();
    match fsck_outcome(preen.code) {
        Fsck::Clean => Ok(()),
        Fsck::NeedsYes => {
            let yes = tool("e2fsck").arg("-y").arg(device).limit(limit).run();
            match fsck_outcome(yes.code) {
                Fsck::Clean => Ok(()),
                _ => Err(Failure::Damaged(yes.stderr)),
            }
        }
        Fsck::Failed => Err(Failure::Damaged(preen.stderr)),
    }
}

fn random_key() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// What the TPM says its SRK is.
pub fn read_srk() -> Srk {
    let out = tool("systemd-analyze")
        .arg("srk")
        .limit(Duration::from_secs(20))
        .run();
    match (out.success(), out.stdout.is_empty()) {
        (true, false) => Srk::Known(out.stdout),
        (true, true) => Srk::Missing,
        (false, _) if out.stderr.contains("No SRK") => Srk::Missing,
        (false, _) => Srk::Unreadable,
    }
}

/// No store: nothing under the image's name.
pub struct Absent<'c>(&'c Context);

/// A complete store under its hidden name: its only key sealed to the TPM,
/// proven by opening it that way, with an empty `connections/` inside.
pub struct Built<'c>(&'c Context);

/// A store in place, of this program's format, closed.
pub struct Sealed<'c> {
    cx: &'c Context,
    volume: Volume,
}

/// Its device open through the TPM, the filesystem checked.
pub struct Checked<'c> {
    cx: &'c Context,
    volume: Volume,
}

/// Its filesystem mounted at the staging directory.
pub struct Mounted<'c> {
    cx: &'c Context,
    volume: Volume,
}

/// Its `connections/` bound over NetworkManager's keyfile directory.
pub struct Bound<'c>(&'c Context);

impl<'c> Absent<'c> {
    pub fn find(cx: &'c Context) -> Option<Absent<'c>> {
        (!cx.image().exists()).then_some(Absent(cx))
    }

    fn volume(&self) -> Volume {
        Volume {
            image: self.0.building(),
            mapper: MAPPER_NEW,
            staging: self.0.staging("new"),
        }
    }

    /// What a killed build left: its device, mount and image.
    fn clean(&self) {
        let volume = self.volume();
        if sys::mounts().iter().any(|m| m.point == volume.staging) {
            let _ = unmount(&volume.staging);
        }
        detach(volume.mapper);
        let _ = fs::remove_file(&volume.image);
    }

    /// Build it with `seed` (Genkan's profiles at installation) already in
    /// `connections/`: written only inside it, never anywhere in plaintext.
    pub fn build(self, seed: &[Profile]) -> Result<Built<'c>, String> {
        self.clean();
        let result = self.make(seed);
        if result.is_err() {
            self.clean();
        }
        result.map(|()| Built(self.0))
    }

    fn make(&self, seed: &[Profile]) -> Result<(), String> {
        let cx = self.0;
        let volume = self.volume();
        let image = volume.image.as_path();
        files::ensure_dir(&cx.state, 0o700).map_err(|e| e.to_string())?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(image)
            .map_err(|e| e.to_string())?;
        // Copy-on-write would fragment it on btrfs; harmless to ask elsewhere.
        let _ = tool("chattr").arg("+C").arg(image).run();
        // Preallocated: a full root filesystem then fails a save, not the store.
        tool("fallocate").args(["-l", SIZE]).arg(image).ok()?;
        crash_point("allocated");
        // A random key, only ever in memory and pipes, used to format and
        // to enroll the TPM, then wiped from the header. High-entropy, so
        // the minimal key derivation is enough.
        let key = random_key().map_err(|e| e.to_string())?;
        tool("cryptsetup")
            .args([
                "luksFormat",
                "--type",
                "luks2",
                "--batch-mode",
                "--label",
                LABEL,
            ])
            .args([
                "--pbkdf",
                "pbkdf2",
                "--pbkdf-force-iterations",
                "1000",
                "--key-file",
                "-",
            ])
            .arg(image)
            .input(key.as_bytes())
            .ok()?;
        crash_point("formatted");
        // Sealed to the TPM alone: no PCRs (firmware, Secure Boot and kernel
        // updates can't lock it), and no pcrlock or signed policy picked up
        // from the system.
        tool("systemd-cryptenroll")
            .args([
                "--tpm2-device=auto",
                "--tpm2-pcrs=",
                "--tpm2-pcrlock=",
                "--tpm2-public-key=",
            ])
            .args(["--wipe-slot=password"])
            .arg(image)
            .env("PASSWORD", &key)
            .limit(Duration::from_secs(120))
            .ok()?;
        crash_point("enrolled");
        attach(&volume).map_err(|e| e.to_string())?;
        let device = volume.device();
        tool("mkfs.ext4")
            .args(["-q", "-L", "sekka-netstore"])
            .arg(&device)
            .ok()?;
        files::ensure_dir(&volume.staging, 0o700).map_err(|e| e.to_string())?;
        tool("mount")
            .args(["-t", "ext4", "-o", "nodev,nosuid,noexec"])
            .arg(&device)
            .arg(&volume.staging)
            .ok()?;
        let connections = volume.staging.join(CONNECTIONS);
        let made = files::ensure_dir(&connections, 0o700)
            .and_then(|()| {
                seed.iter().try_for_each(|profile| {
                    files::write_atomic(&connections.join(&profile.name), &profile.bytes, 0o600)
                })
            })
            .map_err(|e| e.to_string());
        unmount(&volume.staging)?;
        made?;
        detach(volume.mapper);
        crash_point("made");
        fs::File::open(image)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    }
}

impl<'c> Built<'c> {
    /// Put it in place under its own name, with the TPM's SRK beside it.
    pub fn commit(self, srk: Option<&[u8]>) -> Result<Sealed<'c>, String> {
        let cx = self.0;
        match srk {
            Some(srk) => files::write_atomic(&cx.srk(), srk, 0o600),
            None => fs::remove_file(cx.srk()).or_else(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(e)
                }
            }),
        }
        .map_err(|e| e.to_string())?;
        crash_point("before-commit");
        fs::rename(cx.building(), cx.image()).map_err(|e| e.to_string())?;
        files::sync_dir(&cx.state).map_err(|e| e.to_string())?;
        Ok(Sealed::main(cx))
    }
}

/// The LUKS2 label of `image`.
fn label(image: &Path) -> Result<String, String> {
    let dump = tool("cryptsetup").arg("luksDump").arg(image).ok()?;
    Ok(String::from_utf8_lossy(&dump)
        .lines()
        .find_map(|l| l.strip_prefix("Label:").map(|v| v.trim().to_owned()))
        .unwrap_or_default())
}

impl<'c> Sealed<'c> {
    fn main(cx: &'c Context) -> Sealed<'c> {
        Sealed {
            cx,
            volume: Volume {
                image: cx.image(),
                mapper: MAPPER,
                staging: cx.staging("open"),
            },
        }
    }

    /// The store, if there is one.
    pub fn find(cx: &'c Context) -> Result<Option<Sealed<'c>>, Failure> {
        if !cx.image().exists() {
            return Ok(None);
        }
        match label(&cx.image()) {
            Ok(l) if l == LABEL => Ok(Some(Sealed::main(cx))),
            Ok(l) => Err(Failure::Unknown(format!("label {l:?}"))),
            Err(e) => Err(Failure::Other(e)),
        }
    }

    /// Stores set aside for a new one, newest first.
    pub fn rotated(cx: &'c Context) -> Vec<Sealed<'c>> {
        let mut images: Vec<PathBuf> = fs::read_dir(cx.rotated())
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "luks"))
            .collect();
        images.sort();
        images.reverse();
        images
            .into_iter()
            .map(|image| Sealed {
                cx,
                volume: Volume {
                    image,
                    mapper: MAPPER_OLD,
                    staging: cx.staging("old"),
                },
            })
            .collect()
    }

    pub fn image(&self) -> &Path {
        &self.volume.image
    }

    pub fn attach(self) -> Result<Checked<'c>, Failure> {
        attach(&self.volume)?;
        if let Err(e) = check(&self.volume.device()) {
            detach(self.volume.mapper);
            return Err(e);
        }
        Ok(Checked {
            cx: self.cx,
            volume: self.volume,
        })
    }

    /// Move the store and its SRK to `rotated/`, keeping the newest few.
    pub fn set_aside(self) -> Result<(), String> {
        let cx = self.cx;
        let dir = cx.rotated();
        files::ensure_dir(&dir, 0o700).map_err(|e| e.to_string())?;
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut stem = format!("{secs:020}");
        let mut n = 1;
        while dir.join(format!("{stem}.luks")).exists() {
            stem = format!("{secs:020}.{n}");
            n += 1;
        }
        if cx.srk().exists() {
            fs::rename(cx.srk(), dir.join(format!("{stem}.srk"))).map_err(|e| e.to_string())?;
        }
        fs::rename(&self.volume.image, dir.join(format!("{stem}.luks")))
            .map_err(|e| e.to_string())?;
        files::sync_dir(&dir)
            .and_then(|()| files::sync_dir(&cx.state))
            .map_err(|e| e.to_string())?;
        for old in Sealed::rotated(cx).into_iter().skip(KEPT) {
            old.forget();
        }
        Ok(())
    }

    /// Remove a set-aside store and its SRK (merged back, or too old).
    pub fn forget(self) {
        let _ = fs::remove_file(self.volume.image.with_extension("srk"));
        let _ = fs::remove_file(&self.volume.image);
    }
}

impl<'c> Checked<'c> {
    pub fn mount(self, read_only: bool) -> Result<Mounted<'c>, Failure> {
        let volume = &self.volume;
        let result = (|| {
            files::ensure_dir(&volume.staging, 0o700).map_err(|e| e.to_string())?;
            let ro = tool("blockdev").arg("--getro").arg(volume.device()).ok()?;
            if !read_only && String::from_utf8_lossy(&ro).trim() != "0" {
                return Err("its device opened read-only".to_owned());
            }
            let options = if read_only {
                "ro,nodev,nosuid,noexec"
            } else {
                "rw,nodev,nosuid,noexec,errors=remount-ro"
            };
            tool("mount")
                .args(["-t", "ext4", "-o", options])
                .arg(volume.device())
                .arg(&volume.staging)
                .ok()?;
            Ok(())
        })();
        match result {
            Ok(()) => Ok(Mounted {
                cx: self.cx,
                volume: self.volume,
            }),
            Err(e) => {
                detach(self.volume.mapper);
                Err(Failure::Other(e))
            }
        }
    }
}

impl<'c> Mounted<'c> {
    pub fn connections(&self) -> PathBuf {
        self.volume.staging.join(CONNECTIONS)
    }

    /// Bind `connections/` over NetworkManager's keyfile directory and let
    /// go of the staging mount; the bind holds the filesystem.
    pub fn bind(self) -> Result<Bound<'c>, String> {
        let cx = self.cx;
        if let Err(e) = tool("mount")
            .arg("--bind")
            .arg(self.connections())
            .arg(&cx.keyfiles)
            .ok()
        {
            let _ = self.close();
            return Err(e);
        }
        let _ = unmount(&self.volume.staging);
        Ok(Bound(cx))
    }

    pub fn close(self) -> Result<(), String> {
        unmount(&self.volume.staging)?;
        detach(self.volume.mapper);
        Ok(())
    }
}

impl<'c> Bound<'c> {
    /// The store, if it's what NetworkManager reads now.
    pub fn current(cx: &'c Context) -> Option<Bound<'c>> {
        let source = Path::new("/dev/mapper").join(MAPPER);
        sys::mounts()
            .iter()
            .any(|m| m.point == cx.keyfiles && Path::new(&m.source) == source)
            .then_some(Bound(cx))
    }

    pub fn close(self) -> Result<(), String> {
        unmount(&self.0.keyfiles)?;
        detach(MAPPER);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fsck_codes() {
        assert_eq!(fsck_outcome(Some(0)), Fsck::Clean);
        assert_eq!(fsck_outcome(Some(1)), Fsck::Clean);
        assert_eq!(fsck_outcome(Some(3)), Fsck::Clean);
        assert_eq!(fsck_outcome(Some(4)), Fsck::NeedsYes);
        assert_eq!(fsck_outcome(Some(5)), Fsck::NeedsYes);
        assert_eq!(fsck_outcome(Some(8)), Fsck::Failed);
        assert_eq!(fsck_outcome(Some(12)), Fsck::Failed);
        assert_eq!(fsck_outcome(None), Fsck::Failed);
    }
}
