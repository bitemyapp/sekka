//! Graphics cards, from `/sys/class/drm/card*`.
//!
//! Each `cardN` (not the connectors such as `card1-eDP-1`) is one GPU. Its
//! `device` directory is the PCI device, with `vendor` and `device` ids,
//! the `driver` link (`i915`, `xe`, `amdgpu`, `nouveau`, `nvidia`),
//! `boot_vga` (the GPU the firmware used to boot) and, with amdgpu,
//! `mem_info_vram_total`.
//!
//! The GPU's name comes from `pci.ids` (hwdata), searched at
//! [`PCI_IDS_PATHS`] or given to [`gpus_with_pci_ids`]: "Navi 21 [Radeon RX
//! 6800/6800 XT / 6900 XT]" becomes "AMD Radeon RX 6800/6800 XT / 6900 XT
//! (Navi 21)". Without it, a small table names the vendor: "Intel graphics
//! (8086:a7a0)". On NixOS, `pci.ids` is in the system profile only when
//! `pkgs.hwdata` is in `environment.systemPackages`; Sekka's module should
//! add it. `/nix/store` is never scanned.

use super::{parse_hex, read_attr, read_u64};
use std::path::{Path, PathBuf};

const DRM: &str = "/sys/class/drm";

/// Where [`gpus`] looks for `pci.ids`, in order: the NixOS system profile,
/// then the usual places of other distributions.
pub const PCI_IDS_PATHS: &[&str] =
    &["/run/current-system/sw/share/hwdata/pci.ids", "/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids"];

/// One graphics card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gpu {
    /// The DRM card, such as `card0`.
    pub card: String,
    /// A name for the UI, such as "Intel Graphics (Panther Lake)" or "AMD
    /// Radeon RX 6800/6800 XT / 6900 XT (Navi 21)"; see the module
    /// documentation.
    pub name: String,
    /// The PCI vendor id (`0x8086` Intel, `0x1002` AMD, `0x10de` NVIDIA);
    /// `None` for GPUs that aren't PCI devices, as on ARM boards.
    pub vendor_id: Option<u16>,
    /// The PCI device id.
    pub device_id: Option<u16>,
    /// The vendor's name in `pci.ids`, such as "Intel Corporation".
    pub vendor_name: Option<String>,
    /// The device's name in `pci.ids`, such as "Panther Lake [Intel
    /// Graphics]".
    pub device_name: Option<String>,
    /// The kernel driver bound to it, such as `xe` or `amdgpu`.
    pub driver: Option<String>,
    /// Dedicated video memory in bytes, where the driver reports it
    /// (amdgpu).
    pub vram: Option<u64>,
    /// Whether the firmware booted on it: the primary GPU.
    pub boot_vga: bool,
    /// Its bus address, such as `0000:00:02.0`.
    pub bus_address: Option<String>,
}

/// The GPUs, named from `pci.ids` found at [`PCI_IDS_PATHS`].
pub fn gpus() -> Vec<Gpu> {
    gpus_with_pci_ids(None)
}

/// The GPUs, named from `pci_ids` if given and readable, else from the
/// first file found at [`PCI_IDS_PATHS`].
pub fn gpus_with_pci_ids(pci_ids: Option<&Path>) -> Vec<Gpu> {
    let ids = read_pci_ids(pci_ids);
    gpus_in(Path::new(DRM), ids.as_deref())
}

/// The text of `pci.ids`: `explicit` if readable, else the first of
/// [`PCI_IDS_PATHS`].
pub(crate) fn read_pci_ids(explicit: Option<&Path>) -> Option<String> {
    explicit
        .into_iter()
        .map(Path::to_path_buf)
        .chain(PCI_IDS_PATHS.iter().map(PathBuf::from))
        .find_map(|path| std::fs::read_to_string(path).ok())
}

/// The GPUs under `drm` (normally `/sys/class/drm`), named with `pci_ids`
/// (the file's text) when given.
pub(crate) fn gpus_in(drm: &Path, pci_ids: Option<&str>) -> Vec<Gpu> {
    let Ok(entries) = std::fs::read_dir(drm) else {
        return Vec::new();
    };
    let mut cards: Vec<(u32, String)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let number = name.strip_prefix("card")?.parse().ok()?;
            Some((number, name))
        })
        .collect();
    cards.sort();

    let mut gpus: Vec<Gpu> = Vec::new();
    let mut seen = Vec::new();
    for (_, card) in cards {
        let device = drm.join(&card).join("device");
        // Two cards on one device (rare) are one GPU.
        let Ok(real) = std::fs::canonicalize(&device) else {
            continue;
        };
        if seen.contains(&real) {
            continue;
        }
        let gpu = read_gpu(&card, &device, &real, pci_ids);
        seen.push(real);
        gpus.push(gpu);
    }
    gpus
}

fn read_gpu(card: &str, device: &Path, real: &Path, pci_ids: Option<&str>) -> Gpu {
    let id =
        |what: &str| read_attr(device.join(what)).and_then(|text| parse_hex(&text)).and_then(|n| u16::try_from(n).ok());
    let vendor_id = id("vendor");
    let device_id = id("device");
    let driver = std::fs::read_link(device.join("driver"))
        .ok()
        .and_then(|target| Some(target.file_name()?.to_string_lossy().into_owned()));
    let (vendor_name, device_name) = match (pci_ids, vendor_id, device_id) {
        (Some(text), Some(vendor), Some(device)) => lookup_pci_ids(text, vendor, device),
        _ => (None, None),
    };
    let name = gpu_name(vendor_id, device_id, vendor_name.as_deref(), device_name.as_deref(), driver.as_deref());
    let is_pci = vendor_id.is_some();
    Gpu {
        card: card.to_owned(),
        name,
        vendor_id,
        device_id,
        vendor_name,
        device_name,
        driver,
        vram: read_u64(device.join("mem_info_vram_total")).filter(|&v| v > 0),
        boot_vga: read_attr(device.join("boot_vga")).as_deref() == Some("1"),
        bus_address: is_pci.then(|| Some(real.file_name()?.to_string_lossy().into_owned())).flatten(),
    }
}

/// Look up a vendor and device in the text of `pci.ids`: the vendor's name,
/// and the device's if listed.
///
/// The format: a vendor line is four hex digits, two spaces and the name;
/// its devices follow on lines starting with one tab (two tabs are
/// subsystems); `#` starts a comment.
pub(crate) fn lookup_pci_ids(text: &str, vendor: u16, device: u16) -> (Option<String>, Option<String>) {
    let vendor_hex = format!("{vendor:04x}");
    let device_hex = format!("{device:04x}");
    let mut vendor_name: Option<String> = None;
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if vendor_name.is_some() {
            let Some(entry) = line.strip_prefix('\t') else {
                // The next vendor (or the class list): no such device.
                break;
            };
            if let Some(name) = entry.strip_prefix(device_hex.as_str()).and_then(|rest| rest.strip_prefix("  ")) {
                return (vendor_name, Some(name.trim().to_owned()));
            }
        } else if let Some(name) = line.strip_prefix(vendor_hex.as_str()).and_then(|rest| rest.strip_prefix("  ")) {
            vendor_name = Some(name.trim().to_owned());
        }
    }
    (vendor_name, None)
}

/// The short name of a GPU vendor, for the vendors worth knowing without
/// `pci.ids`.
fn known_vendor(vendor: u16) -> Option<&'static str> {
    Some(match vendor {
        0x8086 => "Intel",
        0x1002 | 0x1022 => "AMD",
        0x10de => "NVIDIA",
        0x1af4 | 0x1b36 => "Red Hat",
        0x1234 => "QEMU",
        0x15ad => "VMware",
        0x80ee => "VirtualBox",
        0x1414 => "Microsoft",
        0x1a03 => "ASPEED",
        0x102b => "Matrox",
        0x5143 => "Qualcomm",
        0x106b => "Apple",
        _ => return None,
    })
}

/// A vendor name from `pci.ids` without its corporate suffix and bracketed
/// aliases: "Silicon Motion, Inc." becomes "Silicon Motion".
fn tidy_vendor(name: &str) -> String {
    let name = name.split(" [").next().unwrap_or(name).trim();
    let mut name = name;
    for suffix in [" Co., Ltd.", ", Inc.", " Inc.", " Inc", " Corporation", " Corp.", " Ltd.", " GmbH", " Limited", ","]
    {
        name = name.strip_suffix(suffix).unwrap_or(name).trim_end();
    }
    name.to_owned()
}

/// The UI name of a GPU; see the module documentation.
fn gpu_name(
    vendor_id: Option<u16>,
    device_id: Option<u16>,
    vendor_name: Option<&str>,
    device_name: Option<&str>,
    driver: Option<&str>,
) -> String {
    let vendor = vendor_id
        .and_then(known_vendor)
        .map(str::to_owned)
        .or_else(|| vendor_name.map(tidy_vendor))
        .filter(|v| !v.is_empty());
    let with_vendor = |text: &str| match &vendor {
        Some(vendor) if !text.to_lowercase().starts_with(&vendor.to_lowercase()) => {
            format!("{vendor} {text}")
        }
        _ => text.to_owned(),
    };
    if let Some(device) = device_name {
        // "Codename [Marketing name]".
        if let Some((codename, rest)) = device.split_once('[')
            && let Some(marketing) = rest.strip_suffix(']')
            && !marketing.trim().is_empty()
        {
            let codename = codename.trim();
            let marketing = with_vendor(marketing.trim());
            return if codename.is_empty() { marketing } else { format!("{marketing} ({codename})") };
        }
        return with_vendor(device);
    }
    match (vendor_id, device_id) {
        (Some(v), Some(d)) => match &vendor {
            Some(vendor) => format!("{vendor} graphics ({v:04x}:{d:04x})"),
            None => format!("Graphics ({v:04x}:{d:04x})"),
        },
        _ => match driver {
            Some(driver) => format!("{driver} graphics"),
            None => "Graphics".to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysinfo::TestTree;

    // Excerpts of hwdata 0.411's pci.ids, with their real layout.
    const PCI_IDS: &str = "\
#
#\tList of PCI IDS
#
# Syntax:
# vendor  vendor_name
#\tdevice  device_name\t\t\t\t<-- single tab
#\t\tsubvendor subdevice  subsystem_name\t<-- two tabs

0010  Allied Telesis, Inc (Wrong ID)
# This is a relabelled RTL-8139
\t8139  AT-2500TX V3 Ethernet
1002  Advanced Micro Devices, Inc. [AMD/ATI]
\t73bf  Navi 21 [Radeon RX 6800/6800 XT / 6900 XT]
\t\t1002 0e3a  Radeon RX 6900 XT
\t15bf  Phoenix1
10de  NVIDIA Corporation
\t2484  GA104 [GeForce RTX 3070]
126f  Silicon Motion, Inc.
\t0712  SM712 LynxEM+
8086  Intel Corporation
\tb090  Panther Lake [Intel Graphics]
\tb0a0  Panther Lake [Intel Graphics]
\t46a6  Alder Lake-P GT2 [Iris Xe Graphics]
8088  Beijing Wangxun Technology Co., Ltd.
\t1001  Ethernet Controller RP1000 for 10GbE SFP+

# List of known device classes, subclasses and programming interfaces
C 00  Unclassified device
\t00  Non-VGA unclassified device
";

    #[test]
    fn pci_ids_lookups_find_vendor_and_device() {
        assert_eq!(
            lookup_pci_ids(PCI_IDS, 0x8086, 0xb090),
            (Some("Intel Corporation".into()), Some("Panther Lake [Intel Graphics]".into()))
        );
        assert_eq!(
            lookup_pci_ids(PCI_IDS, 0x1002, 0x15bf),
            (Some("Advanced Micro Devices, Inc. [AMD/ATI]".into()), Some("Phoenix1".into()))
        );
        // A device of the next vendor isn't mistaken for one of this one.
        assert_eq!(lookup_pci_ids(PCI_IDS, 0x8086, 0x1001), (Some("Intel Corporation".into()), None));
        // Subsystem lines aren't devices.
        assert_eq!(lookup_pci_ids(PCI_IDS, 0x1002, 0x0e3a).1, None);
        assert_eq!(lookup_pci_ids(PCI_IDS, 0xabcd, 0x0001), (None, None));
        assert_eq!(lookup_pci_ids("", 0x8086, 0xb090), (None, None));
    }

    #[test]
    fn gpu_names_put_the_marketing_name_first() {
        let name = |v: u16, d: u16| {
            let (vendor, device) = lookup_pci_ids(PCI_IDS, v, d);
            gpu_name(Some(v), Some(d), vendor.as_deref(), device.as_deref(), None)
        };
        assert_eq!(name(0x8086, 0xb090), "Intel Graphics (Panther Lake)");
        assert_eq!(name(0x8086, 0x46a6), "Intel Iris Xe Graphics (Alder Lake-P GT2)");
        assert_eq!(name(0x1002, 0x73bf), "AMD Radeon RX 6800/6800 XT / 6900 XT (Navi 21)");
        assert_eq!(name(0x1002, 0x15bf), "AMD Phoenix1");
        assert_eq!(name(0x10de, 0x2484), "NVIDIA GeForce RTX 3070 (GA104)");
        assert_eq!(name(0x126f, 0x0712), "Silicon Motion SM712 LynxEM+");
        // Not in pci.ids, or no pci.ids at all.
        assert_eq!(name(0x8086, 0xa7a0), "Intel graphics (8086:a7a0)");
        assert_eq!(gpu_name(Some(0x8086), Some(0xa7a0), None, None, None), "Intel graphics (8086:a7a0)");
        assert_eq!(gpu_name(Some(0xabcd), Some(0x1), None, None, None), "Graphics (abcd:0001)");
        assert_eq!(gpu_name(None, None, None, None, Some("vc4")), "vc4 graphics");
        assert_eq!(gpu_name(None, None, None, None, None), "Graphics");
        assert_eq!(gpu_name(Some(1), Some(2), None, Some("Odd [unclosed"), None), "Odd [unclosed");
        assert_eq!(tidy_vendor("Beijing Wangxun Technology Co., Ltd."), "Beijing Wangxun Technology");
    }

    /// This machine's DRM tree, plus a discrete AMD card and an ARM board's
    /// platform GPU.
    fn fake_drm() -> TestTree {
        let tree = TestTree::new("drm");
        tree.file("devices/pci0000:00/0000:00:02.0/vendor", "0x8086\n")
            .file("devices/pci0000:00/0000:00:02.0/device", "0xb090\n")
            .file("devices/pci0000:00/0000:00:02.0/boot_vga", "1\n")
            .dir("drivers/xe")
            .link("devices/pci0000:00/0000:00:02.0/driver", "../../../drivers/xe")
            .link("drm/card0", "../devices/pci0000:00/0000:00:02.0/drm/card0")
            .link("devices/pci0000:00/0000:00:02.0/drm/card0/device", "../../../0000:00:02.0")
            .link("drm/card0-eDP-1", "../devices/pci0000:00/0000:00:02.0/drm/card0/card0-eDP-1")
            .dir("devices/pci0000:00/0000:00:02.0/drm/card0/card0-eDP-1")
            .file("drm/version", "drm 1.1.0 20060810\n")
            .file("devices/pci0000:00/0000:03:00.0/vendor", "0x1002\n")
            .file("devices/pci0000:00/0000:03:00.0/device", "0x73bf\n")
            .file("devices/pci0000:00/0000:03:00.0/boot_vga", "0\n")
            .file("devices/pci0000:00/0000:03:00.0/mem_info_vram_total", "17163091968\n")
            .dir("drivers/amdgpu")
            .link("devices/pci0000:00/0000:03:00.0/driver", "../../../drivers/amdgpu")
            .link("drm/card1", "../devices/pci0000:00/0000:03:00.0/drm/card1")
            .link("devices/pci0000:00/0000:03:00.0/drm/card1/device", "../../../0000:03:00.0")
            .dir("devices/platform/gpu")
            .dir("drivers/vc4-drm")
            .link("devices/platform/gpu/driver", "../../../drivers/vc4-drm")
            .link("drm/card2", "../devices/platform/gpu/drm/card2")
            .link("devices/platform/gpu/drm/card2/device", "../../../gpu")
            .dir("drm/card3");
        tree
    }

    #[test]
    fn drm_cards_are_read_and_connectors_skipped() {
        let tree = fake_drm();
        let gpus = gpus_in(&tree.path("drm"), Some(PCI_IDS));
        let cards: Vec<&str> = gpus.iter().map(|g| g.card.as_str()).collect();
        assert_eq!(cards, ["card0", "card1", "card2"]);

        let intel = &gpus[0];
        assert_eq!(intel.name, "Intel Graphics (Panther Lake)");
        assert_eq!((intel.vendor_id, intel.device_id), (Some(0x8086), Some(0xb090)));
        assert_eq!(intel.driver.as_deref(), Some("xe"));
        assert!(intel.boot_vga);
        assert_eq!(intel.vram, None);
        assert_eq!(intel.bus_address.as_deref(), Some("0000:00:02.0"));

        let amd = &gpus[1];
        assert_eq!(amd.name, "AMD Radeon RX 6800/6800 XT / 6900 XT (Navi 21)");
        assert_eq!(amd.vram, Some(17163091968));
        assert!(!amd.boot_vga);

        let arm = &gpus[2];
        assert_eq!(arm.name, "vc4-drm graphics");
        assert_eq!(arm.vendor_id, None);
        assert_eq!(arm.bus_address, None);

        let unnamed = gpus_in(&tree.path("drm"), None);
        assert_eq!(unnamed[0].name, "Intel graphics (8086:b090)");
        assert!(gpus_in(Path::new("/nonexistent/drm"), None).is_empty());
    }

    #[test]
    fn an_explicit_pci_ids_path_comes_first() {
        let tree = TestTree::new("pci-ids");
        tree.file("pci.ids", PCI_IDS);
        let text = read_pci_ids(Some(&tree.path("pci.ids"))).unwrap();
        assert!(text.contains("Panther Lake"));
    }

    #[test]
    #[ignore = "reads this machine"]
    fn live_gpus_are_found() {
        let found = gpus();
        println!("standard paths: {found:#?}");
        assert!(!found.is_empty());
        assert!(found.iter().all(|g| !g.name.is_empty()));
        // This NixOS host has hwdata only in the store; find a copy (this
        // test may scan, the library never does).
        let store_copy = std::fs::read_dir("/nix/store").ok().and_then(|entries| {
            entries.flatten().map(|e| e.path()).find_map(|p| {
                let name = p.file_name()?.to_str()?.to_owned();
                let candidate = p.join("share/hwdata/pci.ids");
                (name.contains("-hwdata-") && candidate.exists()).then_some(candidate)
            })
        });
        if let Some(path) = store_copy {
            let started = std::time::Instant::now();
            let named = gpus_with_pci_ids(Some(&path));
            let took = started.elapsed();
            println!("with {} ({took:?}): {named:#?}", path.display());
            assert!(named.iter().any(|g| g.device_name.is_some()));
        }
    }
}
