//! What the computer is, from the firmware's DMI (SMBIOS) tables in
//! `/sys/class/dmi/id`.
//!
//! Firmware often fills fields with placeholders ("To be filled by
//! O.E.M.", "System Product Name", "Default string"); those count as
//! missing. Vendors put the model in different fields: most in
//! `product_name`, but Lenovo puts a machine type there ("21V7006FUS") and
//! the marketing name in `product_version` ("ThinkPad X1 Carbon Gen 14").
//! [`HostModel::display_name`] knows this. Only world-readable fields are
//! read (not serial numbers or the UUID). ARM boards without DMI have
//! `/proc/device-tree/model` instead, which [`host_model`] falls back to.

use super::read_attr;
use std::path::Path;

const DMI: &str = "/sys/class/dmi/id";
const DEVICE_TREE_MODEL: &str = "/proc/device-tree/model";

/// The computer's make and model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostModel {
    /// The manufacturer as the firmware gives it (`sys_vendor`), such as
    /// "LENOVO" or "ASUSTeK COMPUTER INC."; see
    /// [`vendor_name`](Self::vendor_name) for display.
    pub vendor: Option<String>,
    /// `product_name`: usually the model; on Lenovo, a machine type.
    pub product_name: Option<String>,
    /// `product_version`: on Lenovo, the model; elsewhere often a revision.
    pub product_version: Option<String>,
    /// `product_family`, such as "ThinkPad X1 Carbon Gen 14" or "XPS".
    pub product_family: Option<String>,
    /// The mainboard's manufacturer (`board_vendor`).
    pub board_vendor: Option<String>,
    /// The mainboard's model (`board_name`), the useful field on
    /// self-built desktops.
    pub board_name: Option<String>,
    /// The SMBIOS chassis type number (`chassis_type`).
    pub chassis_type: Option<u8>,
    /// What kind of computer the chassis type says it is.
    pub chassis: Chassis,
    /// The device-tree model of boards without DMI, such as "Raspberry Pi 4
    /// Model B Rev 1.4".
    pub device_tree_model: Option<String>,
}

/// What kind of computer it is, from the SMBIOS chassis type (grouped as
/// `systemd-hostnamed` does).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Chassis {
    /// Desktop, tower, all-in-one, mini PC.
    Desktop,
    /// Laptop, notebook, portable.
    Laptop,
    /// Convertible or detachable laptop.
    Convertible,
    /// Tablet.
    Tablet,
    /// Hand-held device.
    Handset,
    /// Server, blade.
    Server,
    /// Embedded PC, IoT gateway.
    Embedded,
    /// Not reported, or a type not listed here.
    #[default]
    Unknown,
}

impl Chassis {
    /// The kind for an SMBIOS chassis type number.
    pub fn from_smbios(chassis_type: u8) -> Chassis {
        // The top bit is a "chassis lock present" flag.
        match chassis_type & 0x7f {
            0x03 | 0x04 | 0x06 | 0x07 | 0x0d | 0x23 | 0x24 => Chassis::Desktop,
            0x08 | 0x09 | 0x0a | 0x0e => Chassis::Laptop,
            0x0b => Chassis::Handset,
            0x11 | 0x1c | 0x1d => Chassis::Server,
            0x1e => Chassis::Tablet,
            0x1f | 0x20 => Chassis::Convertible,
            0x21 | 0x22 => Chassis::Embedded,
            _ => Chassis::Unknown,
        }
    }
}

impl HostModel {
    /// The manufacturer's usual short name: "Lenovo" for "LENOVO", "ASUS"
    /// for "ASUSTeK COMPUTER INC.", "Dell" for "Dell Inc."; otherwise the
    /// firmware's name without a corporate suffix. From the board vendor if
    /// the system vendor is missing.
    pub fn vendor_name(&self) -> Option<String> {
        self.vendor.as_deref().or(self.board_vendor.as_deref()).map(tidy_vendor).filter(|v| !v.is_empty())
    }

    /// A name for "About this computer", such as "Lenovo ThinkPad X1 Carbon
    /// Gen 11", "Dell XPS 13 9310" or, on a self-built desktop, "ASUS ROG
    /// STRIX B550-F GAMING" (the mainboard). `None` if the firmware says
    /// nothing useful.
    pub fn display_name(&self) -> Option<String> {
        let vendor = self.vendor_name();
        let is_lenovo = vendor.as_deref() == Some("Lenovo");
        let product = if is_lenovo {
            // product_version can be "Lenovo" alone on some IdeaPads.
            self.product_version
                .clone()
                .filter(|v| !v.eq_ignore_ascii_case("lenovo"))
                .or_else(|| self.product_family.clone())
                .or_else(|| self.product_name.clone())
        } else {
            self.product_name.clone()
        };
        let (vendor, product) = match product {
            Some(product) => (vendor, product),
            None => match &self.board_name {
                Some(board) => (self.board_vendor.as_deref().map(tidy_vendor).or(vendor), board.clone()),
                None => return self.device_tree_model.clone(),
            },
        };
        Some(match vendor {
            Some(vendor) if !starts_with_ignoring_case(&product, &vendor) => {
                format!("{vendor} {product}")
            }
            _ => product,
        })
    }
}

fn starts_with_ignoring_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len()).is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

/// The firmware's make and model.
pub fn host_model() -> HostModel {
    let mut model = host_model_in(Path::new(DMI));
    if model.vendor.is_none() && model.product_name.is_none() {
        model.device_tree_model = read_attr(DEVICE_TREE_MODEL).filter(|m| !is_placeholder(m));
    }
    model
}

/// Read the DMI attributes under `root` (normally `/sys/class/dmi/id`).
pub(crate) fn host_model_in(root: &Path) -> HostModel {
    let field = |name: &str| read_attr(root.join(name)).filter(|v| !is_placeholder(v));
    let chassis_type = read_attr(root.join("chassis_type")).and_then(|t| t.parse::<u8>().ok());
    HostModel {
        vendor: field("sys_vendor"),
        product_name: field("product_name"),
        product_version: field("product_version"),
        product_family: field("product_family"),
        board_vendor: field("board_vendor"),
        board_name: field("board_name"),
        chassis_type,
        chassis: chassis_type.map_or(Chassis::Unknown, Chassis::from_smbios),
        device_tree_model: None,
    }
}

/// Whether a DMI string is a firmware placeholder rather than a value.
pub(crate) fn is_placeholder(value: &str) -> bool {
    const PLACEHOLDERS: &[&str] = &[
        "to be filled by o.e.m.",
        "to be filled by oem",
        "system product name",
        "system manufacturer",
        "system version",
        "system name",
        "default string",
        "not applicable",
        "not specified",
        "not available",
        "none",
        "n/a",
        "na",
        "unknown",
        "undefined",
        "invalid",
        "oem",
        "o.e.m.",
        "0123456789",
        "123456789",
        "1234567890",
        "type1productconfigid",
        "base board product name",
        "base board manufacturer",
        "chassis manufacturer",
        "all series",
        "x.x",
        "xxxxx",
        "empty",
    ];
    let value = value.trim();
    value.is_empty()
        || value.chars().all(|c| c == '0' || c == ' ' || c == '.')
        || PLACEHOLDERS.iter().any(|p| value.eq_ignore_ascii_case(p))
}

/// A manufacturer's usual short name.
fn tidy_vendor(vendor: &str) -> String {
    const KNOWN: &[(&str, &str)] = &[
        ("lenovo", "Lenovo"),
        ("dell", "Dell"),
        ("hewlett-packard", "HP"),
        ("hp", "HP"),
        ("asustek", "ASUS"),
        ("asus", "ASUS"),
        ("acer", "Acer"),
        ("micro-star", "MSI"),
        ("msi", "MSI"),
        ("gigabyte", "Gigabyte"),
        ("apple", "Apple"),
        ("microsoft", "Microsoft"),
        ("samsung", "Samsung"),
        ("toshiba", "Toshiba"),
        ("dynabook", "Dynabook"),
        ("fujitsu", "Fujitsu"),
        ("sony", "Sony"),
        ("lg electronics", "LG"),
        ("huawei", "Huawei"),
        ("xiaomi", "Xiaomi"),
        ("framework", "Framework"),
        ("system76", "System76"),
        ("tuxedo", "TUXEDO"),
        ("purism", "Purism"),
        ("razer", "Razer"),
        ("google", "Google"),
        ("intel", "Intel"),
        ("supermicro", "Supermicro"),
        ("asrock", "ASRock"),
        ("vmware", "VMware"),
        ("innotek", "Oracle"),
        ("qemu", "QEMU"),
    ];
    let trimmed = vendor.trim();
    let lower = trimmed.to_lowercase();
    for (prefix, name) in KNOWN {
        if let Some(rest) = lower.strip_prefix(prefix)
            && rest.chars().next().is_none_or(|c| !c.is_alphanumeric())
        {
            return (*name).to_owned();
        }
    }
    let mut name = trimmed;
    for suffix in [
        " Co., Ltd.",
        " Co.,Ltd.",
        " CO., LTD.",
        ", Inc.",
        " Inc.",
        " INC.",
        " Corporation",
        " Corp.",
        " Ltd.",
        " LTD.",
        " GmbH",
        " Limited",
        " LLC",
        ",",
    ] {
        name = name.strip_suffix(suffix).unwrap_or(name).trim_end();
    }
    name.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysinfo::TestTree;

    /// This machine's DMI attributes.
    fn thinkpad() -> TestTree {
        let tree = TestTree::new("dmi-thinkpad");
        tree.file("sys_vendor", "LENOVO\n")
            .file("product_name", "21V7006FUS\n")
            .file("product_version", "ThinkPad X1 Carbon Gen 14\n")
            .file("product_family", "ThinkPad X1 Carbon Gen 14\n")
            .file("board_vendor", "LENOVO\n")
            .file("board_name", "21V7006FUS\n")
            .file("chassis_type", "10\n")
            .file("chassis_version", "None\n");
        tree
    }

    #[test]
    fn a_thinkpad_is_named_from_product_version() {
        let tree = thinkpad();
        let model = host_model_in(&tree.0);
        assert_eq!(model.vendor.as_deref(), Some("LENOVO"));
        assert_eq!(model.vendor_name().as_deref(), Some("Lenovo"));
        assert_eq!(model.chassis_type, Some(10));
        assert_eq!(model.chassis, Chassis::Laptop);
        assert_eq!(model.display_name().as_deref(), Some("Lenovo ThinkPad X1 Carbon Gen 14"));
    }

    fn model(vendor: &str, product: &str) -> HostModel {
        HostModel {
            vendor: Some(vendor.into()).filter(|v: &String| !is_placeholder(v)),
            product_name: Some(product.into()).filter(|v: &String| !is_placeholder(v)),
            ..HostModel::default()
        }
    }

    #[test]
    fn display_names_combine_vendor_and_model() {
        assert_eq!(model("Dell Inc.", "XPS 13 9310").display_name().as_deref(), Some("Dell XPS 13 9310"));
        assert_eq!(
            model("HP", "HP EliteBook 840 G8 Notebook PC").display_name().as_deref(),
            Some("HP EliteBook 840 G8 Notebook PC")
        );
        assert_eq!(
            model("Framework", "Laptop 13 (AMD Ryzen 7040Series)").display_name().as_deref(),
            Some("Framework Laptop 13 (AMD Ryzen 7040Series)")
        );
        assert_eq!(
            model("ASUSTeK COMPUTER INC.", "ROG Zephyrus G14 GA401QM").display_name().as_deref(),
            Some("ASUS ROG Zephyrus G14 GA401QM")
        );
        assert_eq!(
            model("Micro-Star International Co., Ltd.", "Prestige 14").display_name().as_deref(),
            Some("MSI Prestige 14")
        );
        assert_eq!(
            model("Star Labs Systems Ltd.", "StarBook").display_name().as_deref(),
            Some("Star Labs Systems StarBook")
        );
        // An IdeaPad whose product_version is just "Lenovo".
        let ideapad = HostModel {
            vendor: Some("LENOVO".into()),
            product_name: Some("81YM".into()),
            product_version: Some("Lenovo".into()),
            product_family: Some("IdeaPad 5 14ARE05".into()),
            ..HostModel::default()
        };
        assert_eq!(ideapad.display_name().as_deref(), Some("Lenovo IdeaPad 5 14ARE05"));
    }

    #[test]
    fn self_built_desktops_are_named_after_the_board() {
        let tree = TestTree::new("dmi-desktop");
        tree.file("sys_vendor", "System manufacturer\n")
            .file("product_name", "System Product Name\n")
            .file("product_version", "System Version\n")
            .file("board_vendor", "ASUSTeK COMPUTER INC.\n")
            .file("board_name", "ROG STRIX B550-F GAMING\n")
            .file("chassis_type", "3\n");
        let model = host_model_in(&tree.0);
        assert_eq!(model.vendor, None);
        assert_eq!(model.product_name, None);
        assert_eq!(model.chassis, Chassis::Desktop);
        assert_eq!(model.display_name().as_deref(), Some("ASUS ROG STRIX B550-F GAMING"));
    }

    #[test]
    fn placeholders_and_missing_fields_give_nothing() {
        for junk in [
            "To be filled by O.E.M.",
            "To Be Filled By O.E.M.",
            "Default string",
            "Not Applicable",
            "None",
            "0123456789",
            "  ",
            "0",
            "00000000",
        ] {
            assert!(is_placeholder(junk), "{junk:?}");
        }
        assert!(!is_placeholder("ThinkPad X1 Carbon Gen 14"));
        let empty = TestTree::new("dmi-empty");
        let model = host_model_in(&empty.0);
        assert_eq!(model, HostModel::default());
        assert_eq!(model.display_name(), None);
        let board =
            HostModel { device_tree_model: Some("Raspberry Pi 4 Model B Rev 1.4".into()), ..HostModel::default() };
        assert_eq!(board.display_name().as_deref(), Some("Raspberry Pi 4 Model B Rev 1.4"));
    }

    #[test]
    fn chassis_types_group_like_hostnamed() {
        assert_eq!(Chassis::from_smbios(3), Chassis::Desktop);
        assert_eq!(Chassis::from_smbios(9), Chassis::Laptop);
        assert_eq!(Chassis::from_smbios(0x80 | 10), Chassis::Laptop);
        assert_eq!(Chassis::from_smbios(31), Chassis::Convertible);
        assert_eq!(Chassis::from_smbios(30), Chassis::Tablet);
        assert_eq!(Chassis::from_smbios(23), Chassis::Unknown);
        assert_eq!(Chassis::from_smbios(17), Chassis::Server);
        assert_eq!(Chassis::from_smbios(1), Chassis::Unknown);
    }

    #[test]
    #[ignore = "reads this machine"]
    fn live_host_model_is_read() {
        let model = host_model();
        println!("{model:#?}\ndisplay name: {:?}", model.display_name());
    }
}
