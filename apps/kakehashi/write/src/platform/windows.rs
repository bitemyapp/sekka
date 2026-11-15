//! Windows: the volumes on the drive locked and dismounted, the drive
//! opened unbuffered and write-through, its layout removed, then ejected.

use super::Device;
use anyhow::{Context, Result, bail, ensure};
use kakehashi_core::Drive;
use kakehashi_core::platform::sys::{disk_geometry, disk_length, open_raw, volumes};
use std::fs::File;
use std::os::windows::io::{AsHandle, AsRawHandle, OwnedHandle};
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{FILE_FLAG_NO_BUFFERING, FILE_FLAG_WRITE_THROUGH, FILE_FLAGS_AND_ATTRIBUTES};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{
    FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, IOCTL_DISK_DELETE_DRIVE_LAYOUT, IOCTL_DISK_UPDATE_PROPERTIES,
    IOCTL_STORAGE_EJECT_MEDIA,
};

// Only the helper can issue these controls. Each takes no buffers; no
// arbitrary number or Rust output value crosses this safe interface.
enum Control {
    LockVolume,
    DismountVolume,
    DeleteDriveLayout,
    UpdateProperties,
    EjectMedia,
}

fn control(handle: &impl AsHandle, operation: Control) -> windows::core::Result<()> {
    let code = match operation {
        Control::LockVolume => FSCTL_LOCK_VOLUME,
        Control::DismountVolume => FSCTL_DISMOUNT_VOLUME,
        Control::DeleteDriveLayout => IOCTL_DISK_DELETE_DRIVE_LAYOUT,
        Control::UpdateProperties => IOCTL_DISK_UPDATE_PROPERTIES,
        Control::EjectMedia => IOCTL_STORAGE_EJECT_MEDIA,
    };
    let mut returned = 0;
    // SAFETY: a live borrowed handle and a synchronous control which takes
    // no input or output buffer. returned outlives the call.
    unsafe {
        DeviceIoControl(HANDLE(handle.as_handle().as_raw_handle()), code, None, 0, None, 0, Some(&mut returned), None)
    }
}

/// The volumes on the drive, locked and dismounted, which must stay open
/// while it is written.
pub(crate) type Held = Vec<OwnedHandle>;

fn number_of(drive: &Drive) -> Result<u32> {
    drive
        .name
        .strip_prefix("PhysicalDrive")
        .and_then(|n| n.parse().ok())
        .with_context(|| format!("{} is not a physical drive", drive.name))
}

/// Volumes are locked and dismounted when the drive is opened.
pub(crate) fn unmount(_drive: &Drive) -> Result<()> {
    Ok(())
}

/// Lock and dismount every volume on the drive, keeping the locks, then
/// open the drive itself unbuffered. Its contents are unchanged: layout
/// deletion belongs to the checked Ready-to-Writing transition.
pub(crate) fn open(drive: &Drive) -> Result<Device> {
    let number = number_of(drive)?;
    let mut held = Vec::new();
    for (volume, _) in volumes().remove(&number).unwrap_or_default() {
        let handle =
            open_raw(volume.trim_end_matches('\\'), GENERIC_READ.0 | GENERIC_WRITE.0, FILE_FLAGS_AND_ATTRIBUTES(0))
                .with_context(|| format!("could not open the volume {volume}"))?;
        let mut locked = false;
        for _ in 0..20 {
            if control(&handle, Control::LockVolume).is_ok() {
                locked = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        ensure!(locked, "could not lock {volume}: close anything using the drive and try again");
        control(&handle, Control::DismountVolume).with_context(|| format!("could not dismount {volume}"))?;
        held.push(handle);
    }
    let handle = open_raw(
        &format!("\\\\.\\PhysicalDrive{number}"),
        GENERIC_READ.0 | GENERIC_WRITE.0,
        FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH,
    )
    .with_context(|| format!("could not open {}", drive.name))?;
    let bytes = disk_length(&handle).context("could not read the size")?;
    let geometry = disk_geometry(&handle).context("could not read the geometry")?;
    let sector = geometry.Geometry.BytesPerSector as u64;
    ensure!(sector.is_power_of_two() && (512..=4096).contains(&sector), "unexpected sector size {sector}");
    Ok(Device { file: File::from(handle), bytes, sector, held })
}

/// The first destructive action, called only by Ready::begin after the
/// final checks and after recording that the disk may have changed: asks
/// Windows to forget the drive's layout, so that it mounts nothing on the
/// drive while it is written.
///
/// It returns nothing, because a refusal doesn't stop the write. The request
/// can be refused where there is no layout to remove. The write doesn't
/// depend on it: the volumes found on the drive are already locked and
/// dismounted, the image replaces the layout either way, and what was
/// written is read back and compared. A drive that can't be written fails at
/// the first write.
pub(super) fn start(file: &File) {
    let _ = control(file, Control::DeleteDriveLayout);
}

/// The drive was written unbuffered: there is nothing cached to forget.
pub(crate) fn drop_cache(_device: &Device) {}

pub(crate) fn eject(drive: &Drive) -> Result<()> {
    let number = number_of(drive)?;
    let handle = open_raw(&format!("\\\\.\\PhysicalDrive{number}"), GENERIC_READ.0, FILE_FLAGS_AND_ATTRIBUTES(0))?;
    // Windows reads the new partition table, then the drive is ejected.
    let _ = control(&handle, Control::UpdateProperties);
    if control(&handle, Control::EjectMedia).is_err() {
        bail!("it could not be ejected, but it is written, so you can unplug it");
    }
    Ok(())
}
