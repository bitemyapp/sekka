//! Each operating system's write operations on a whole drive: unmount it,
//! open it exclusively, flush it, forget its cache, eject it. Unmounting
//! and opening take [`still_there`](crate::still_there)'s proof. Listing the
//! drives is the core's, which the window shares.

use crate::{Requested, StillThere, still_there};
use aizome_typestate::{Confirmed, Reviewed};
use anyhow::{Result, bail, ensure};
use kakehashi_core::Drive;
use kakehashi_core::image::Cancelled;
use kakehashi_core::write::TAIL;
use std::fs::File;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as imp;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use self::windows as imp;

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod unsupported;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
use unsupported as imp;

/// A whole drive opened for writing, exclusively.
pub(crate) struct Device {
    file: File,
    bytes: u64,
    /// Reads and writes are whole multiples of this.
    sector: u64,
    /// Whatever must stay open while the drive is written, such as locked
    /// volumes on Windows.
    #[allow(dead_code)]
    held: imp::Held,
}

/// A drive opened without changing its contents. The file remains private
/// until the identity, capacity and cancellation checks have succeeded.
///
/// ```compile_fail,E0599
/// use kakehashi_write::Opened;
/// fn skip_checks(opened: Opened, touched: &mut bool) {
///     opened.begin(touched);
/// }
/// ```
///
/// ```compile_fail,E0616
/// use kakehashi_write::Opened;
/// fn bypass(opened: Opened) { let _ = opened.device; }
/// ```
pub struct Opened {
    device: Device,
    reviewed: Reviewed<Drive, Opening>,
}

struct Opening;

/// An opened device checked again and ready for its first destructive
/// action. Neither its proof nor its raw file can be supplied by a caller.
///
/// ```compile_fail,E0061
/// use kakehashi_write::Ready;
/// fn forget_to_record_damage(ready: Ready) { ready.begin(); }
/// ```
///
/// ```
/// use kakehashi_write::{Opened, Requested, Writing};
/// fn checked(opened: Opened, plan: &Requested, len: u64, touched: &mut bool)
///     -> anyhow::Result<Writing>
/// {
///     opened.check(plan, len, &|| false)?.begin(touched)
/// }
/// ```
pub struct Ready {
    device: Device,
    drive: Confirmed<Drive, Opening>,
}

/// A device whose first write may have happened. Only Ready::begin creates
/// one, after setting the caller's damage flag. Its handle stays owned until
/// writing and verification are finished, including the Windows volume locks.
pub struct Writing {
    device: Device,
    drive: Confirmed<Drive, Opening>,
}

impl Opened {
    fn new(device: Device, drive: Drive) -> Self {
        Self { device, reviewed: Reviewed::new(drive, Opening) }
    }

    /// Re-read the requested drive after opening and any authorization wait.
    /// Also bind that reading to the drive this handle was opened for.
    pub fn check(self, plan: &Requested, len: u64, cancelled: &dyn Fn() -> bool) -> Result<Ready> {
        self.check_with(len, cancelled, || Ok(still_there(plan)?.into_target()))
    }

    fn check_with(
        self,
        len: u64,
        cancelled: &dyn Fn() -> bool,
        fresh: impl FnOnce() -> Result<Drive>,
    ) -> Result<Ready> {
        let drive = self.reviewed.target();
        ensure!(
            self.device.bytes == drive.bytes,
            "{} reports {} bytes, not {}",
            drive.name,
            self.device.bytes,
            drive.bytes
        );
        let drive = self
            .reviewed
            .confirm(fresh()?)
            .map_err(|_| anyhow::anyhow!("the opened drive changed; nothing was written"))?;
        let sector = self.device.sector;
        ensure!(sector.is_power_of_two() && (512..=4096).contains(&sector), "unexpected sector size {sector}");
        // Checked arithmetic also refuses an image length that would wrap
        // while rounding to a sector or reserving the cleared tail.
        let needed = len.checked_add(sector - 1).map(|n| n / sector * sector).and_then(|n| n.checked_add(TAIL));
        ensure!(
            len > 0 && needed.is_some_and(|n| n <= self.device.bytes),
            "the image ({}) doesn't fit on {} ({})",
            kakehashi_core::drive::human(len),
            drive.target().label(),
            kakehashi_core::drive::human(self.device.bytes)
        );
        if cancelled() {
            bail!(Cancelled);
        }
        Ok(Ready { device: self.device, drive })
    }
}

impl Ready {
    /// Record possible damage before any platform-specific write, including
    /// Windows' partition-table deletion. An error still leaves touched true.
    /// Windows refusing that deletion is not one: the write goes on.
    pub fn begin(self, touched: &mut bool) -> Result<Writing> {
        self.begin_with(touched, |file| {
            #[cfg(windows)]
            imp::start(file);
            #[cfg(not(windows))]
            let _ = file;
            Ok(())
        })
    }

    fn begin_with(self, touched: &mut bool, start: impl FnOnce(&File) -> Result<()>) -> Result<Writing> {
        *touched = true;
        start(&self.device.file)?;
        Ok(Writing { device: self.device, drive: self.drive })
    }
}

impl Writing {
    pub(crate) fn file(&mut self) -> &mut File {
        &mut self.device.file
    }
    pub(crate) fn bytes(&self) -> u64 {
        self.device.bytes
    }
    pub(crate) fn sector(&self) -> u64 {
        self.device.sector
    }
    pub(crate) fn drive(&self) -> &Drive {
        self.drive.target()
    }
    pub(crate) fn sync(&self) -> Result<()> {
        sync(&self.device)
    }
    pub(crate) fn drop_cache(&self) {
        imp::drop_cache(&self.device);
    }
}

/// Unmounts the drive the request names, as [`still_there`](crate::still_there)
/// found it.
pub(crate) fn unmount(drive: &StillThere) -> Result<()> {
    imp::unmount(drive.target())
}

/// Opens the drive the request names, as [`still_there`](crate::still_there)
/// found it, for writing.
pub(crate) fn open(drive: &StillThere, _cancelled: &dyn Fn() -> bool) -> Result<Opened> {
    #[cfg(target_os = "macos")]
    let device = imp::open(drive.target(), _cancelled)?;
    #[cfg(not(target_os = "macos"))]
    let device = imp::open(drive.target())?;
    Ok(Opened::new(device, drive.target().clone()))
}

/// Finish device writes before read-back verification. On macOS a raw disk
/// needs a storage ioctl, not the filesystem fcntl used by File::sync_all.
pub(crate) fn sync(device: &Device) -> Result<()> {
    #[cfg(target_os = "macos")]
    return imp::sync(&device.file);
    #[cfg(not(target_os = "macos"))]
    {
        use anyhow::Context;
        device.file.sync_all().context("could not flush the drive before verification")
    }
}

pub(crate) fn eject(drive: &Drive) -> Result<()> {
    imp::eject(drive)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek, SeekFrom, Write};

    fn drive() -> Drive {
        Drive {
            name: "test-device".into(),
            bytes: TAIL * 4,
            vendor: "test".into(),
            model: "disposable file".into(),
            serial: "original".into(),
            bus: "USB".into(),
            port: None,
            usb_or_removable: true,
            internal: false,
            virtual_drive: false,
            read_only: false,
            system: false,
            mounts: vec![],
        }
    }

    fn opened(drive: &Drive, bytes: u64, sector: u64) -> (Opened, File) {
        // Ordinary temporary files only, on every platform. The callback at
        // Ready::begin stands for the first destructive platform operation.
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"original partition table").unwrap();
        let observer = file.try_clone().unwrap();
        (Opened::new(Device { file, bytes, sector, held: Default::default() }, drive.clone()), observer)
    }

    fn contents(mut file: File) -> Vec<u8> {
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn rejected_preparation_never_reaches_the_first_write() {
        let original = drive();
        for case in ["size", "identity", "missing", "fit", "overflow", "empty", "sector", "stop"] {
            let (bytes, sector, len) = match case {
                "size" => (original.bytes + 512, 512, 512),
                "fit" => (original.bytes, 512, original.bytes),
                "overflow" => (original.bytes, 512, u64::MAX),
                "empty" => (original.bytes, 512, 0),
                "sector" => (original.bytes, 0, 512),
                _ => (original.bytes, 512, 512),
            };
            let (opened, observer) = opened(&original, bytes, sector);
            let mut fresh = original.clone();
            if case == "identity" {
                fresh.serial = "replacement".into();
            }
            let mut touched = false;
            let result = opened
                .check_with(len, &|| case == "stop", || {
                    if case == "missing" {
                        bail!("drive disconnected");
                    }
                    Ok(fresh)
                })
                .and_then(|ready| ready.begin_with(&mut touched, |_| panic!("{case} reached deletion")));
            assert!(result.is_err(), "{case}");
            assert!(!touched, "{case} reported damage before any write");
            assert_eq!(contents(observer), b"original partition table", "{case}");
        }
    }

    #[test]
    fn stopping_while_opening_keeps_the_partition_table() {
        let drive = drive();
        let (opened, observer) = opened(&drive, drive.bytes, 512);
        // The open/volume-lock wait finishes after Stop was pressed. The
        // opened handle alone cannot start deletion; check observes Stop.
        let error = opened.check_with(512, &|| true, || Ok(drive)).err().unwrap();
        assert!(error.is::<Cancelled>());
        assert_eq!(contents(observer), b"original partition table");
    }

    #[test]
    fn a_failed_first_write_is_already_reported_as_touched() {
        let drive = drive();
        let (opened, observer) = opened(&drive, drive.bytes, 512);
        let ready = opened.check_with(512, &|| false, || Ok(drive)).unwrap();
        let mut touched = false;
        let result = ready.begin_with(&mut touched, |file| {
            // A driver may change the disk even if its IOCTL then fails.
            file.set_len(0)?;
            bail!("layout deletion failed after changing the disk")
        });
        assert!(result.is_err());
        assert!(touched);
        assert!(contents(observer).is_empty());
    }

    #[test]
    fn a_checked_device_enters_writing_after_marking_it_touched() {
        let drive = drive();
        let (opened, observer) = opened(&drive, drive.bytes, 512);
        let ready = opened.check_with(512, &|| false, || Ok(drive.clone())).unwrap();
        let mut touched = false;
        let mut writing = ready
            .begin_with(&mut touched, |file| {
                file.set_len(0)?;
                Ok(())
            })
            .unwrap();
        assert!(touched);
        assert_eq!(writing.drive().serial, drive.serial);
        writing.file().seek(SeekFrom::Start(0)).unwrap();
        writing.file().write_all(b"new image").unwrap();
        assert_eq!(contents(observer), b"new image");
    }
}
