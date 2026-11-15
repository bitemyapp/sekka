//! Windows: drives from the storage IOCTLs of each `\\.\PhysicalDriveN`, and
//! the handle and fixed read-only queries the helper's write step uses.

// Windows' own names for the storage bus types.
#![allow(non_upper_case_globals)]

use crate::drive::Drive;
use anyhow::Result;
use std::collections::HashMap;
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use windows::Win32::Foundation::{ERROR_WRITE_PROTECT, HANDLE};
use windows::Win32::Storage::FileSystem::{
    BusTypeFileBackedVirtual, BusTypeMmc, BusTypeNvme, BusTypeRAID, BusTypeSas, BusTypeSata, BusTypeScsi, BusTypeSd,
    BusTypeSpaces, BusTypeUsb, BusTypeVirtual, CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FindFirstVolumeW, FindNextVolumeW, FindVolumeClose, GetVolumeNameForVolumeMountPointW,
    GetVolumePathNameW, GetVolumePathNamesForVolumeNameW, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, OPEN_EXISTING,
    STORAGE_BUS_TYPE,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{
    DISK_GEOMETRY_EX, GET_LENGTH_INFORMATION, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, IOCTL_DISK_GET_LENGTH_INFO,
    IOCTL_DISK_IS_WRITABLE, IOCTL_STORAGE_QUERY_PROPERTY, PropertyStandardQuery, STORAGE_DEVICE_DESCRIPTOR,
    STORAGE_PROPERTY_QUERY, StorageDeviceProperty, VOLUME_DISK_EXTENTS,
};
use windows::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;
use windows::core::PCWSTR;

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
pub fn open_raw(path: &str, access: u32, flags: FILE_FLAGS_AND_ATTRIBUTES) -> windows::core::Result<OwnedHandle> {
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

fn raw(handle: &impl AsHandle) -> HANDLE {
    HANDLE(handle.as_handle().as_raw_handle())
}

/// Read the fixed disk geometry result; callers cannot choose its buffer type.
///
/// ```compile_fail,E0432
/// use kakehashi_core::platform::sys::ioctl;
/// ```
///
/// ```compile_fail,E0432
/// use kakehashi_core::platform::sys::control;
/// ```
///
/// ```compile_fail,E0061
/// use kakehashi_core::platform::sys::disk_geometry;
/// fn corrupt(handle: &std::fs::File, invalid: &mut [bool; 64]) {
///     disk_geometry(handle, invalid);
/// }
/// ```
///
/// ```
/// use kakehashi_core::platform::sys::disk_geometry;
/// fn geometry(handle: &std::fs::File) -> windows::core::Result<u64> {
///     Ok(disk_geometry(handle)?.DiskSize as u64)
/// }
/// ```
pub fn disk_geometry(handle: &impl AsHandle) -> windows::core::Result<DISK_GEOMETRY_EX> {
    let mut geometry = DISK_GEOMETRY_EX::default();
    // SAFETY: this IOCTL writes DISK_GEOMETRY_EX, whose integer/newtype
    // fields accept every bit pattern. The owned output lives through the call.
    unsafe {
        ioctl_bytes(
            handle,
            IOCTL_DISK_GET_DRIVE_GEOMETRY_EX,
            None,
            (&mut geometry as *mut _ as *mut c_void, size_of_val(&geometry) as u32),
        )?;
    }
    Ok(geometry)
}

/// Read the disk length with the API's fixed output representation.
pub fn disk_length(handle: &impl AsHandle) -> windows::core::Result<u64> {
    let mut length = GET_LENGTH_INFORMATION::default();
    // SAFETY: GET_LENGTH_INFORMATION contains only the integer this IOCTL writes.
    unsafe {
        ioctl_bytes(
            handle,
            IOCTL_DISK_GET_LENGTH_INFO,
            None,
            (&mut length as *mut _ as *mut c_void, size_of_val(&length) as u32),
        )?;
    }
    Ok(length.Length as u64)
}

fn is_writable(handle: &impl AsHandle) -> windows::core::Result<()> {
    // SAFETY: this fixed read-only query takes no buffers.
    unsafe { ioctl_bytes(handle, IOCTL_DISK_IS_WRITABLE, None, (std::ptr::null_mut(), 0)) }.map(|_| ())
}

/// # Safety
/// Each pointer must outlive the synchronous call, with the size, alignment
/// and representation required by this specific control. The output must
/// accept every bit pattern the control can write.
unsafe fn ioctl_bytes(
    handle: &impl AsHandle,
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
pub fn volumes() -> HashMap<u32, Vec<(String, Vec<String>)>> {
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
    // SAFETY: the aligned integer buffer can hold the variable-length extents output.
    let Ok(_) = (unsafe {
        ioctl_bytes(
            &handle,
            IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,
            None,
            (buf.as_mut_ptr() as *mut c_void, size_of_val(&buf) as u32),
        )
    }) else {
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
    // SAFETY: query is the required input; the aligned integer buffer accepts
    // all bytes returned by the storage descriptor query. Both outlive the call.
    unsafe {
        ioctl_bytes(
            &handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some((&query as *const _ as *const c_void, size_of_val(&query) as u32)),
            (buf.as_mut_ptr() as *mut c_void, size_of_val(&buf) as u32),
        )
    }
    .ok()?;
    // SAFETY: the buffer is aligned for and at least as large as the header.
    let descriptor = unsafe { &*(buf.as_ptr() as *const STORAGE_DEVICE_DESCRIPTOR) };
    // SAFETY: plain bytes of the same buffer.
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, size_of_val(&buf)) };
    let size = disk_geometry(&handle).map(|geometry| geometry.DiskSize as u64).unwrap_or(0);
    let read_only = matches!(is_writable(&handle), Err(e) if e.code() == ERROR_WRITE_PROTECT.to_hresult());
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
        // Its location path would take SetupAPI; the serial and the rest identify it.
        port: None,
    })
}

pub(crate) fn drives() -> Result<Vec<Drive>> {
    let volumes = volumes();
    let root = system_root();
    Ok((0..MOST_DRIVES).filter_map(|n| describe(n, &volumes, &root)).collect())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_queries_reject_an_ordinary_file_without_changing_it() {
        use std::io::{Read, Seek, Write};
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"an ordinary file").unwrap();
        assert!(disk_geometry(&file).is_err());
        assert!(disk_length(&file).is_err());
        file.rewind().unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"an ordinary file");
    }
}
