//! Windows: drives from the storage IOCTLs of each `\\.\PhysicalDriveN`,
//! written unbuffered after the volumes on them are locked and dismounted.

// Windows' own names for the storage bus types.
#![allow(non_upper_case_globals)]

use super::Device;
use crate::drive::Drive;
use anyhow::{Context, Result, bail, ensure};
use std::collections::HashMap;
use std::ffi::c_void;
use std::fs::File;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::path::Path;
use windows::Win32::Foundation::{ERROR_WRITE_PROTECT, GENERIC_READ, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{
    BusTypeFileBackedVirtual, BusTypeMmc, BusTypeNvme, BusTypeRAID, BusTypeSas, BusTypeSata, BusTypeScsi, BusTypeSd,
    BusTypeSpaces, BusTypeUsb, BusTypeVirtual, CreateFileW, FILE_FLAG_NO_BUFFERING, FILE_FLAG_WRITE_THROUGH,
    FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, FindFirstVolumeW, FindNextVolumeW, FindVolumeClose,
    GetVolumeNameForVolumeMountPointW, GetVolumePathNameW, GetVolumePathNamesForVolumeNameW,
    IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, OPEN_EXISTING, STORAGE_BUS_TYPE,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{
    DISK_GEOMETRY_EX, FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, GET_LENGTH_INFORMATION, IOCTL_DISK_DELETE_DRIVE_LAYOUT,
    IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, IOCTL_DISK_GET_LENGTH_INFO, IOCTL_DISK_IS_WRITABLE, IOCTL_DISK_UPDATE_PROPERTIES,
    IOCTL_STORAGE_EJECT_MEDIA, IOCTL_STORAGE_QUERY_PROPERTY, PropertyStandardQuery, STORAGE_DEVICE_DESCRIPTOR,
    STORAGE_PROPERTY_QUERY, StorageDeviceProperty, VOLUME_DISK_EXTENTS,
};
use windows::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;
use windows::core::PCWSTR;

/// The volumes on the drive, locked and dismounted, which must stay open
/// while it is written.
pub(crate) type Held = Vec<OwnedHandle>;

/// Physical drives are numbered from zero; this many are looked for.
const MOST_DRIVES: u32 = 64;

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// A handle closed when dropped.
fn open_raw(path: &str, access: u32, flags: FILE_FLAGS_AND_ATTRIBUTES) -> windows::core::Result<OwnedHandle> {
    let path = wide(path);
    // SAFETY: `path` is NUL-terminated and outlives the call.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            flags,
            None,
        )?
    };
    // SAFETY: a valid handle that nothing else owns.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

fn raw(handle: &OwnedHandle) -> HANDLE {
    use std::os::windows::io::AsRawHandle;
    HANDLE(handle.as_raw_handle())
}

/// One IOCTL with an optional input, filling `out`.
fn ioctl<I, O>(handle: &OwnedHandle, code: u32, input: Option<&I>, out: &mut O) -> windows::core::Result<u32> {
    ioctl_bytes(
        handle,
        code,
        input.map(|i| (i as *const I as *const c_void, size_of::<I>() as u32)),
        (out as *mut O as *mut c_void, size_of::<O>() as u32),
    )
}

fn ioctl_bytes(
    handle: &OwnedHandle,
    code: u32,
    input: Option<(*const c_void, u32)>,
    out: (*mut c_void, u32),
) -> windows::core::Result<u32> {
    let mut returned = 0u32;
    // SAFETY: the buffers are valid for the sizes given for the call.
    unsafe {
        DeviceIoControl(
            raw(handle),
            code,
            input.map(|(p, _)| p),
            input.map(|(_, n)| n).unwrap_or(0),
            if out.1 == 0 { None } else { Some(out.0) },
            out.1,
            Some(&mut returned),
            None,
        )?;
    }
    Ok(returned)
}

fn no_output(handle: &OwnedHandle, code: u32) -> windows::core::Result<()> {
    ioctl_bytes(handle, code, None, (std::ptr::null_mut(), 0)).map(|_| ())
}

/// An ANSI string at `offset` in a descriptor, or empty.
fn descriptor_text(buf: &[u8], offset: u32) -> String {
    let offset = offset as usize;
    if offset == 0 || offset >= buf.len() {
        return String::new();
    }
    let end = buf[offset..].iter().position(|&b| b == 0).map_or(buf.len(), |n| offset + n);
    String::from_utf8_lossy(&buf[offset..end]).trim().to_owned()
}

fn bus_name(bus: STORAGE_BUS_TYPE) -> &'static str {
    match bus {
        BusTypeUsb => "USB",
        BusTypeSd => "SD",
        BusTypeMmc => "MMC",
        BusTypeSata => "SATA",
        BusTypeNvme => "NVMe",
        BusTypeScsi => "SCSI",
        BusTypeSas => "SAS",
        BusTypeRAID => "RAID",
        BusTypeSpaces => "Storage Spaces",
        BusTypeVirtual | BusTypeFileBackedVirtual => "Virtual",
        _ => "",
    }
}

/// Every volume's GUID path (`\\?\Volume{…}\`) and mount points, by the
/// number of each drive it lives on.
fn volumes() -> HashMap<u32, Vec<(String, Vec<String>)>> {
    let mut by_disk: HashMap<u32, Vec<(String, Vec<String>)>> = HashMap::new();
    let mut name = [0u16; 512];
    // SAFETY: the buffer is valid for its length.
    let Ok(find) = (unsafe { FindFirstVolumeW(&mut name) }) else {
        return by_disk;
    };
    loop {
        let volume = from_wide(&name);
        let points = mount_points(&volume);
        for disk in disks_of(&volume) {
            by_disk.entry(disk).or_default().push((volume.clone(), points.clone()));
        }
        // SAFETY: as above.
        if unsafe { FindNextVolumeW(find, &mut name) }.is_err() {
            break;
        }
    }
    // SAFETY: the handle came from FindFirstVolumeW.
    let _ = unsafe { FindVolumeClose(find) };
    by_disk
}

fn mount_points(volume: &str) -> Vec<String> {
    let name = wide(volume);
    let mut buf = vec![0u16; 1024];
    let mut len = 0u32;
    // SAFETY: the buffers are valid for their lengths.
    if unsafe { GetVolumePathNamesForVolumeNameW(PCWSTR(name.as_ptr()), Some(&mut buf), &mut len) }.is_err() {
        return Vec::new();
    }
    buf.split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect()
}

/// The drive numbers a volume spans.
fn disks_of(volume: &str) -> Vec<u32> {
    // The volume device is its GUID path without the trailing backslash.
    let Ok(handle) = open_raw(volume.trim_end_matches('\\'), 0, FILE_FLAGS_AND_ATTRIBUTES(0)) else {
        return Vec::new();
    };
    // Room for a few extents beyond the one the struct declares.
    let mut buf = [0u64; 64];
    let Ok(_) = ioctl_bytes(
        &handle,
        IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,
        None,
        (buf.as_mut_ptr() as *mut c_void, size_of_val(&buf) as u32),
    ) else {
        return Vec::new();
    };
    // SAFETY: the buffer is aligned for and large enough to hold the header
    // and the extents it says it holds, up to the buffer's size.
    let extents = unsafe { &*(buf.as_ptr() as *const VOLUME_DISK_EXTENTS) };
    let count = (extents.NumberOfDiskExtents as usize).min(20);
    let first = extents.Extents.as_ptr();
    // SAFETY: `count` extents fit in `buf`, as clamped above.
    (0..count).map(|i| unsafe { (*first.add(i)).DiskNumber }).collect()
}

/// The root of the drive Windows runs from, such as `C:\`.
fn system_root() -> String {
    let mut buf = [0u16; 260];
    // SAFETY: the buffer is valid for its length.
    let n = unsafe { GetSystemWindowsDirectoryW(Some(&mut buf)) } as usize;
    let dir = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
    dir.get(..3).unwrap_or("C:\\").to_ascii_uppercase()
}

fn describe(number: u32, volumes: &HashMap<u32, Vec<(String, Vec<String>)>>, root: &str) -> Option<Drive> {
    let handle = open_raw(&format!("\\\\.\\PhysicalDrive{number}"), 0, FILE_FLAGS_AND_ATTRIBUTES(0)).ok()?;
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceProperty,
        QueryType: PropertyStandardQuery,
        AdditionalParameters: [0],
    };
    let mut buf = [0u64; 128];
    ioctl_bytes(
        &handle,
        IOCTL_STORAGE_QUERY_PROPERTY,
        Some((&query as *const _ as *const c_void, size_of_val(&query) as u32)),
        (buf.as_mut_ptr() as *mut c_void, size_of_val(&buf) as u32),
    )
    .ok()?;
    // SAFETY: the buffer is aligned for and at least as large as the header.
    let descriptor = unsafe { &*(buf.as_ptr() as *const STORAGE_DEVICE_DESCRIPTOR) };
    // SAFETY: plain bytes of the same buffer.
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, size_of_val(&buf)) };
    let mut geometry = DISK_GEOMETRY_EX::default();
    let size = ioctl(&handle, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, None::<&()>, &mut geometry)
        .map(|_| geometry.DiskSize as u64)
        .unwrap_or(0);
    let read_only =
        matches!(no_output(&handle, IOCTL_DISK_IS_WRITABLE), Err(e) if e.code() == ERROR_WRITE_PROTECT.to_hresult());
    let bus = descriptor.BusType;
    let usb_or_removable = matches!(bus, BusTypeUsb | BusTypeSd | BusTypeMmc) || descriptor.RemovableMedia;
    let mounts: Vec<String> = volumes.get(&number).into_iter().flatten().flat_map(|(_, p)| p.clone()).collect();
    Some(Drive {
        name: format!("PhysicalDrive{number}"),
        bytes: size,
        vendor: descriptor_text(bytes, descriptor.VendorIdOffset),
        model: descriptor_text(bytes, descriptor.ProductIdOffset),
        serial: descriptor_text(bytes, descriptor.SerialNumberOffset),
        bus: bus_name(bus).to_owned(),
        usb_or_removable,
        internal: !usb_or_removable,
        virtual_drive: matches!(bus, BusTypeVirtual | BusTypeFileBackedVirtual | BusTypeSpaces),
        read_only,
        system: mounts.iter().any(|p| p.eq_ignore_ascii_case(root)),
        mounts,
    })
}

pub(crate) fn drives() -> Result<Vec<Drive>> {
    let volumes = volumes();
    let root = system_root();
    Ok((0..MOST_DRIVES).filter_map(|n| describe(n, &volumes, &root)).collect())
}

fn number_of(drive: &Drive) -> Result<u32> {
    drive
        .name
        .strip_prefix("PhysicalDrive")
        .and_then(|n| n.parse().ok())
        .with_context(|| format!("{} is not a physical drive", drive.name))
}

pub(crate) fn image_drive(image: &Path) -> Option<String> {
    let path: Vec<u16> = image.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut root = [0u16; 512];
    // SAFETY: the buffers are valid and NUL-terminated as required.
    unsafe { GetVolumePathNameW(PCWSTR(path.as_ptr()), &mut root) }.ok()?;
    let mut volume = [0u16; 512];
    // SAFETY: as above.
    unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(root.as_ptr()), &mut volume) }.ok()?;
    disks_of(&from_wide(&volume)).first().map(|n| format!("PhysicalDrive{n}"))
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
