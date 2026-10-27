//! The drives in the sidebar: each drive UDisks knows, and each attached
//! image, with what kind it is and how healthy.

use aizome_sys::disks::{DisksState, Drive, Health, Rotation, format_size};

/// What a drive is, for its glyph and its words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Nvme,
    Ssd,
    Hdd,
    Usb,
    Sd,
    Optical,
    /// A disk image attached as a loop device.
    Image,
}

impl Kind {
    pub fn words(self) -> &'static str {
        match self {
            Kind::Nvme => "NVMe",
            Kind::Ssd => "SSD",
            Kind::Hdd => "Hard disk",
            Kind::Usb => "USB",
            Kind::Sd => "SD card",
            Kind::Optical => "Optical",
            Kind::Image => "Disk image",
        }
    }
}

/// A row of the sidebar.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The drive's object path, or the loop device's for an image.
    pub key: String,
    /// The whole-disk block device.
    pub disk: Option<String>,
    pub name: String,
    pub size: u64,
    pub kind: Kind,
    pub health: Health,
    /// `nvme0n1`.
    pub device: String,
    /// Removable, external or an image: changes to it don't need the
    /// disk's name typed.
    pub removable: bool,
    pub can_power_off: bool,
    pub read_only: bool,
}

impl Entry {
    pub fn subtitle(&self) -> String {
        format!("{} · {}", format_size(self.size), self.kind.words())
    }

    pub fn is_image(&self) -> bool {
        self.kind == Kind::Image
    }
}

/// What kind a drive is.
pub fn kind_of(drive: &Drive, device: &str) -> Kind {
    let dev = device.trim_start_matches("/dev/");
    if drive.optical {
        Kind::Optical
    } else if drive.connection_bus == "sdio" || dev.starts_with("mmcblk") {
        Kind::Sd
    } else if drive.connection_bus == "usb" {
        // Card readers on USB show their cards as SD.
        if drive.media_removable && drive.model.to_ascii_lowercase().contains("sd") { Kind::Sd } else { Kind::Usb }
    } else if dev.starts_with("nvme") {
        Kind::Nvme
    } else {
        match drive.rotation {
            Rotation::SolidState => Kind::Ssd,
            _ => Kind::Hdd,
        }
    }
}

/// The sidebar's rows: drives in UDisks's order (internal first), then
/// attached images.
/// The entry holding `what`: a drive's object path, a whole disk's, or
/// a block device's (object path or `/dev` node) on one, through its
/// partition table and encryption.
pub fn entry_for(state: &DisksState, entries: &[Entry], what: &str) -> Option<String> {
    let by_key = |p: &str| entries.iter().find(|e| e.key == p || e.disk.as_deref() == Some(p)).map(|e| e.key.clone());
    if let Some(k) = by_key(what) {
        return Some(k);
    }
    let mut block = state.block(what).or_else(|| state.block_by_device(what))?;
    if let Some(k) = state.drive_of(block).and_then(|d| by_key(&d.path)) {
        return Some(k);
    }
    for _ in 0..8 {
        if let Some(k) = by_key(&block.path) {
            return Some(k);
        }
        let next =
            block.crypto_backing_device.as_deref().or_else(|| block.partition.as_ref().map(|p| p.table.as_str()))?;
        block = state.block(next)?;
    }
    None
}

pub fn entries(state: &DisksState) -> Vec<Entry> {
    let mut out = Vec::new();
    for drive in &state.drives {
        let block = state.drive_block(&drive.path);
        let device = block.map(|b| b.device.clone()).unwrap_or_default();
        let kind = kind_of(drive, &device);
        out.push(Entry {
            key: drive.path.clone(),
            disk: block.map(|b| b.path.clone()),
            name: drive.display_name(),
            size: if drive.size > 0 { drive.size } else { block.map_or(0, |b| b.size) },
            kind,
            health: drive.smart.as_ref().map(|s| s.health()).unwrap_or_default(),
            device: device.trim_start_matches("/dev/").to_owned(),
            removable: drive.external() || drive.media_removable,
            can_power_off: drive.can_power_off,
            read_only: block.is_some_and(|b| b.read_only),
        });
    }
    for b in &state.blocks {
        let Some(l) = &b.loop_device else { continue };
        if l.backing_file.is_empty() || b.size == 0 || b.partition.is_some() {
            continue;
        }
        let file = l.backing_file.rsplit('/').next().unwrap_or(&l.backing_file).to_owned();
        out.push(Entry {
            key: b.path.clone(),
            disk: Some(b.path.clone()),
            name: file,
            size: b.size,
            kind: Kind::Image,
            health: Health::Unknown,
            device: b.device.trim_start_matches("/dev/").to_owned(),
            removable: true,
            can_power_off: false,
            read_only: b.read_only,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_from_bus_device_and_rotation() {
        let d = |bus: &str, rotation: Rotation| Drive { connection_bus: bus.into(), rotation, ..Default::default() };
        assert_eq!(kind_of(&d("", Rotation::SolidState), "/dev/nvme0n1"), Kind::Nvme);
        assert_eq!(kind_of(&d("", Rotation::SolidState), "/dev/sda"), Kind::Ssd);
        assert_eq!(kind_of(&d("", Rotation::Rpm(7200)), "/dev/sdb"), Kind::Hdd);
        assert_eq!(kind_of(&d("usb", Rotation::Unknown), "/dev/sdc"), Kind::Usb);
        assert_eq!(kind_of(&d("sdio", Rotation::Unknown), "/dev/mmcblk0"), Kind::Sd);
    }
}
