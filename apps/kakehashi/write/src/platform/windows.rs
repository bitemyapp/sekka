//! Windows: the volumes on the drive locked and dismounted, the drive
//! opened unbuffered and write-through, its layout removed, then ejected.

use super::Device;
use anyhow::{Context, Result, bail, ensure};
use kakehashi_core::Drive;
use kakehashi_core::platform::sys::{ioctl, no_output, open_raw, volumes};
use std::fs::File;
use std::os::windows::io::OwnedHandle;
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Storage::FileSystem::{FILE_FLAG_NO_BUFFERING, FILE_FLAG_WRITE_THROUGH, FILE_FLAGS_AND_ATTRIBUTES};
use windows::Win32::System::Ioctl::{
    DISK_GEOMETRY_EX, FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, GET_LENGTH_INFORMATION, IOCTL_DISK_DELETE_DRIVE_LAYOUT,
    IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, IOCTL_DISK_GET_LENGTH_INFO, IOCTL_DISK_UPDATE_PROPERTIES,
    IOCTL_STORAGE_EJECT_MEDIA,
};

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
/// open the drive itself unbuffered and remove its partition table, so
/// Windows does not mount anything on it while it is written.
pub(crate) fn open(drive: &Drive) -> Result<Device> {
    let number = number_of(drive)?;
    let mut held = Vec::new();
    for (volume, _) in volumes().remove(&number).unwrap_or_default() {
        let handle =
            open_raw(volume.trim_end_matches('\\'), GENERIC_READ.0 | GENERIC_WRITE.0, FILE_FLAGS_AND_ATTRIBUTES(0))
                .with_context(|| format!("could not open the volume {volume}"))?;
        let mut locked = false;
        for _ in 0..20 {
            if no_output(&handle, FSCTL_LOCK_VOLUME).is_ok() {
                locked = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        ensure!(locked, "could not lock {volume}: close anything using the drive and try again");
        no_output(&handle, FSCTL_DISMOUNT_VOLUME).with_context(|| format!("could not dismount {volume}"))?;
        held.push(handle);
    }
    let handle = open_raw(
        &format!("\\\\.\\PhysicalDrive{number}"),
        GENERIC_READ.0 | GENERIC_WRITE.0,
        FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH,
    )
    .with_context(|| format!("could not open {}", drive.name))?;
    let _ = no_output(&handle, IOCTL_DISK_DELETE_DRIVE_LAYOUT);
    let mut length = GET_LENGTH_INFORMATION::default();
    ioctl(&handle, IOCTL_DISK_GET_LENGTH_INFO, None::<&()>, &mut length).context("could not read the size")?;
    let mut geometry = DISK_GEOMETRY_EX::default();
    ioctl(&handle, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, None::<&()>, &mut geometry)
        .context("could not read the geometry")?;
    let sector = geometry.Geometry.BytesPerSector as u64;
    ensure!(sector.is_power_of_two() && (512..=4096).contains(&sector), "unexpected sector size {sector}");
    Ok(Device { file: File::from(handle), bytes: length.Length as u64, sector, held })
}

/// The drive was written unbuffered: there is nothing cached to forget.
pub(crate) fn drop_cache(_device: &Device) {}

pub(crate) fn eject(drive: &Drive) -> Result<()> {
    let number = number_of(drive)?;
    let handle = open_raw(&format!("\\\\.\\PhysicalDrive{number}"), GENERIC_READ.0, FILE_FLAGS_AND_ATTRIBUTES(0))?;
    // Windows reads the new partition table, then the drive is ejected.
    let _ = no_output(&handle, IOCTL_DISK_UPDATE_PROPERTIES);
    if no_output(&handle, IOCTL_STORAGE_EJECT_MEDIA).is_err() {
        bail!("it could not be ejected, but it is written, so you can unplug it");
    }
    Ok(())
}
