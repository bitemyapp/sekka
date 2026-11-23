//! A whole drive as the operating system describes it, and whether it may
//! be offered for writing.

use aizome_typestate::SameTarget;
use serde::{Deserialize, Serialize};

/// Larger than this is probably an external disk, not a flash drive.
pub const FLASH_DRIVE_MAX: u64 = 2_000_000_000_000;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drive {
    /// `sdb` on Linux, `disk4` on macOS, `PhysicalDrive2` on Windows.
    pub name: String,
    pub bytes: u64,
    pub vendor: String,
    pub model: String,
    pub serial: String,
    /// `USB`, `Secure Digital`, `SATA`, ...
    pub bus: String,
    pub usb_or_removable: bool,
    pub internal: bool,
    pub virtual_drive: bool,
    pub read_only: bool,
    /// Holds a filesystem the running system needs.
    pub system: bool,
    pub mounts: Vec<String>,
    /// The USB port it is plugged into, as the system names it: `2-1.3` on
    /// Linux (bus 2, port 1, then port 3 of the hub there), the location ID
    /// `0x14200000` on macOS. `None` when it is not on USB or the system
    /// doesn't say (Windows). Plans without it still read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
}

/// Why a drive is not offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Excluded {
    Internal,
    Virtual,
    ReadOnly,
    System,
    HoldsImage,
    NoMedia,
    NotRemovable,
    TooLarge,
}

impl Excluded {
    pub fn reason(self) -> &'static str {
        match self {
            Excluded::Internal => "internal",
            Excluded::Virtual => "virtual (a disk image or synthesized volume)",
            Excluded::ReadOnly => "read-only",
            Excluded::System => "holds the running system",
            Excluded::HoldsImage => "holds the image",
            Excluded::NoMedia => "no media",
            Excluded::NotRemovable => "not USB or removable",
            Excluded::TooLarge => "larger than 2 TB, probably not a flash drive",
        }
    }

    /// Whether "show all external drives" lifts it.
    pub fn liftable(self) -> bool {
        matches!(self, Excluded::NotRemovable | Excluded::TooLarge)
    }
}

impl Drive {
    pub fn label(&self) -> String {
        let name = format!("{} {}", self.vendor.trim(), self.model.trim());
        let name = name.trim();
        if name.is_empty() { "Unnamed drive".to_owned() } else { name.to_owned() }
    }

    pub fn serial_hint(&self) -> String {
        let serial = self.serial.trim();
        match serial.chars().count() {
            0 => "no serial".to_owned(),
            1..=12 => format!("serial {serial}"),
            n => format!("serial …{}", serial.chars().skip(n - 8).collect::<String>()),
        }
    }

    /// The port in words, if the system named one.
    pub fn port_hint(&self) -> Option<String> {
        self.port.as_ref().map(|port| format!("USB port {port}"))
    }

    /// What tells it from another drive of the same make: its serial, and
    /// its port if known.
    pub fn identity_hint(&self) -> String {
        match self.port_hint() {
            Some(port) => format!("{}, {port}", self.serial_hint()),
            None => self.serial_hint(),
        }
    }

    /// Why this drive is not offered, if it isn't. `all` offers external
    /// drives that are not USB or removable, or are large.
    pub fn excluded(&self, image_drive: Option<&str>, all: bool) -> Option<Excluded> {
        if self.internal {
            Some(Excluded::Internal)
        } else if self.virtual_drive {
            Some(Excluded::Virtual)
        } else if self.read_only {
            Some(Excluded::ReadOnly)
        } else if self.system {
            Some(Excluded::System)
        } else if image_drive == Some(self.name.as_str()) {
            Some(Excluded::HoldsImage)
        } else if self.bytes == 0 {
            Some(Excluded::NoMedia)
        } else if !all && !self.usb_or_removable {
            Some(Excluded::NotRemovable)
        } else if !all && self.bytes > FLASH_DRIVE_MAX {
            Some(Excluded::TooLarge)
        } else {
            None
        }
    }

    /// The same physical drive: compared before writing, so a drive that
    /// was unplugged and replaced in the meantime is not erased. The port
    /// counts when both readings have one, so a stick moved to another port,
    /// or a stick just like it without a serial in another port, isn't the
    /// same.
    pub fn same(&self, other: &Drive) -> bool {
        (&self.name, self.bytes, &self.vendor, &self.model, &self.serial, &self.bus)
            == (&other.name, other.bytes, &other.vendor, &other.model, &other.serial, &other.bus)
            && self.port.as_ref().zip(other.port.as_ref()).is_none_or(|(a, b)| a == b)
    }
}

/// A drive is the same target when [`Drive::same`] says so: its serial,
/// size, vendor, model, bus and port, as well as its name, which alone another
/// drive can take over.
impl SameTarget for Drive {
    fn same_target(&self, fresh: &Drive) -> bool {
        self.same(fresh)
    }
}

/// Sizes as drive makers count them (powers of 1000).
pub fn human(bytes: u64) -> String {
    let units = ["B", "kB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < units.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", units[unit]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stick() -> Drive {
        Drive {
            name: "sdb".into(),
            bytes: 32_000_000_000,
            vendor: "SanDisk".into(),
            model: "Ultra".into(),
            serial: "4C530001230529115034".into(),
            bus: "USB".into(),
            usb_or_removable: true,
            ..Drive::default()
        }
    }

    #[test]
    fn offers_a_usb_stick_and_nothing_risky() {
        assert_eq!(stick().excluded(None, false), None);
        type Change = fn(&mut Drive);
        let cases: [(Change, Excluded); 6] = [
            (|d| d.internal = true, Excluded::Internal),
            (|d| d.virtual_drive = true, Excluded::Virtual),
            (|d| d.read_only = true, Excluded::ReadOnly),
            (|d| d.system = true, Excluded::System),
            (|d| d.bytes = 0, Excluded::NoMedia),
            (|d| d.usb_or_removable = false, Excluded::NotRemovable),
        ];
        for (change, why) in cases {
            let mut d = stick();
            change(&mut d);
            assert_eq!(d.excluded(None, false), Some(why));
        }
        assert_eq!(stick().excluded(Some("sdb"), true), Some(Excluded::HoldsImage));
    }

    #[test]
    fn all_lifts_only_the_removable_and_size_rules() {
        let mut d = stick();
        d.usb_or_removable = false;
        d.bytes = 4_000_000_000_000;
        assert_eq!(d.excluded(None, false), Some(Excluded::NotRemovable));
        assert_eq!(d.excluded(None, true), None);
        d.system = true;
        assert_eq!(d.excluded(None, true), Some(Excluded::System));
        assert!(!Excluded::System.liftable() && Excluded::TooLarge.liftable());
    }

    #[test]
    fn a_replaced_drive_is_not_the_same() {
        let mut other = stick();
        assert!(stick().same(&other));
        other.serial = "different".into();
        assert!(!stick().same(&other));
    }

    #[test]
    fn a_serial_less_stick_in_another_port_is_not_the_same() {
        let in_port = |port: Option<&str>| Drive { serial: String::new(), port: port.map(str::to_owned), ..stick() };
        // Two of the same make, neither with a serial, the second plugged in
        // after the first was pulled, so it got the same name.
        assert!(!in_port(Some("2-1")).same(&in_port(Some("2-2"))));
        assert!(in_port(Some("2-1")).same(&in_port(Some("2-1"))));
        // The same stick, moved to another port.
        assert!(!Drive { port: Some("2-1".into()), ..stick() }.same(&Drive { port: Some("3-4".into()), ..stick() }));
        // A reading without a port (Windows, an older plan) leaves it out.
        assert!(in_port(Some("2-1")).same(&in_port(None)));
        assert_eq!(in_port(Some("2-1")).identity_hint(), "no serial, USB port 2-1");
    }

    #[test]
    fn a_plan_without_a_port_still_reads() {
        let mut json = serde_json::to_value(stick()).unwrap();
        assert!(json.get("port").is_none(), "no port, no key: {json}");
        json.as_object_mut().unwrap().remove("port");
        let back: Drive = serde_json::from_value(json).unwrap();
        assert_eq!(back, stick());
        let with: Drive =
            serde_json::from_value(serde_json::to_value(Drive { port: Some("2-1".into()), ..stick() }).unwrap())
                .unwrap();
        assert_eq!(with.port.as_deref(), Some("2-1"));
    }

    #[test]
    fn labels_and_sizes() {
        assert_eq!(stick().label(), "SanDisk Ultra");
        assert_eq!(stick().serial_hint(), "serial …29115034");
        assert_eq!(Drive::default().label(), "Unnamed drive");
        assert_eq!(human(999), "999 B");
        assert_eq!(human(32_000_000_000), "32.0 GB");
        assert_eq!(human(12_157_655_040), "12.2 GB");
    }
}
