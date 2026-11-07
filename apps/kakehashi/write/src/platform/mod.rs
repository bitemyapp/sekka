//! Each operating system's write operations on a whole drive: unmount it,
//! open it exclusively, flush it, forget its cache, eject it. Unmounting
//! and opening take [`still_there`](crate::still_there)'s proof. Listing the
//! drives is the core's, which the window shares.

use crate::StillThere;
use anyhow::Result;
use kakehashi_core::Drive;
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
    pub(crate) file: File,
    pub(crate) bytes: u64,
    /// Reads and writes are whole multiples of this.
    pub(crate) sector: u64,
    /// Whatever must stay open while the drive is written, such as locked
    /// volumes on Windows.
    #[allow(dead_code)]
    pub(crate) held: imp::Held,
}

/// Unmounts the drive the request names, as [`still_there`](crate::still_there)
/// found it.
pub(crate) fn unmount(drive: &StillThere) -> Result<()> {
    imp::unmount(drive.target())
}

/// Opens the drive the request names, as [`still_there`](crate::still_there)
/// found it, for writing.
pub(crate) fn open(drive: &StillThere, _cancelled: &dyn Fn() -> bool) -> Result<Device> {
    #[cfg(target_os = "macos")]
    return imp::open(drive.target(), _cancelled);
    #[cfg(not(target_os = "macos"))]
    return imp::open(drive.target());
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

/// Forget cached blocks, so reading back reads the drive.
pub(crate) fn drop_cache(device: &Device) {
    imp::drop_cache(device)
}

pub(crate) fn eject(drive: &Drive) -> Result<()> {
    imp::eject(drive)
}
