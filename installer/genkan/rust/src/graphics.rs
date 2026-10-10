// SPDX-License-Identifier: GPL-3.0-or-later
//! NVIDIA GPUs found in sysfs. Installed systems use NVIDIA's own driver for
//! them instead of nouveau. Whether PRIME offload applies depends on where the
//! displays are connected, not on the kind of machine. Under offload, X draws
//! only on the integrated GPU: with the panel wired to the NVIDIA GPU (a MUX
//! switch in discrete mode) the X11 login screen would stay black. Without
//! offload, X draws only on the NVIDIA GPU, and displays on the integrated GPU
//! would stay black instead.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const NVIDIA: &str = "0x10de";
const INTEL: &str = "0x8086";
const AMD: &str = "0x1002";
/// SMBIOS chassis types: portable, laptop, notebook, sub-notebook,
/// convertible, detachable.
const LAPTOP_CHASSIS: [u32; 6] = [8, 9, 10, 14, 31, 32];
const PANELS: [&str; 3] = ["-eDP-", "-LVDS-", "-DSI-"];

/// An Intel or AMD GPU alongside the NVIDIA one, as a NixOS bus ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum Integrated {
    Intel(String),
    Amd(String),
}
impl Integrated {
    pub fn bus_id(&self) -> &str {
        match self {
            Self::Intel(id) | Self::Amd(id) => id,
        }
    }
    /// The NixOS `hardware.nvidia.prime` option for this GPU.
    pub fn option(&self) -> &'static str {
        match self {
            Self::Intel(_) => "intelBusId",
            Self::Amd(_) => "amdgpuBusId",
        }
    }
    fn name(&self) -> &'static str {
        match self {
            Self::Intel(_) => "Intel",
            Self::Amd(_) => "AMD",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Graphics {
    /// The first NVIDIA GPU, as a NixOS bus ID such as `PCI:1:0:0`.
    pub nvidia: Option<String>,
    pub integrated: Option<Integrated>,
    /// The displays are on the integrated GPU, so the NVIDIA GPU is used
    /// through PRIME offload rather than as the primary GPU.
    pub offload: bool,
}

/// NixOS's `PCI:bus:device:function` form, in decimal, of a sysfs slot name
/// such as `0000:01:00.0`; other PCI domains use `PCI:bus@domain:...`.
fn bus_id(slot: &str) -> Option<String> {
    let (rest, function) = slot.rsplit_once('.')?;
    let mut parts = rest.split(':');
    let (domain, bus, device) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let hex = |s: &str| u32::from_str_radix(s, 16).ok();
    let (domain, bus, device, function) = (hex(domain)?, hex(bus)?, hex(device)?, hex(function)?);
    Some(if domain == 0 {
        format!("PCI:{bus}:{device}:{function}")
    } else {
        format!("PCI:{bus}@{domain}:{device}:{function}")
    })
}

fn valid_bus_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("PCI:") else {
        return false;
    };
    let fields: Vec<&str> = rest.split(':').collect();
    let number = |s: &str| !s.is_empty() && s.len() <= 5 && s.bytes().all(|b| b.is_ascii_digit());
    fields.len() == 3
        && fields[1..].iter().all(|f| number(f))
        && match fields[0].split_once('@') {
            Some((bus, domain)) => number(bus) && number(domain),
            None => number(fields[0]),
        }
}

/// Connected connectors in `drm` (`/sys/class/drm`): their names and the PCI
/// slot of their GPU, the path component before `drm`.
fn connected(drm: &Path) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = fs::read_dir(drm)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            fs::read_to_string(entry.path().join("status"))
                .ok()
                .filter(|s| s.trim() == "connected")?;
            let path = fs::canonicalize(entry.path()).ok()?;
            let parts: Vec<String> = path
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            let at = parts.iter().position(|p| p == "drm")?;
            Some((name, parts.get(at.checked_sub(1)?)?.clone()))
        })
        .collect();
    found.sort();
    found
}

/// The GPU driving the displays: the one with the internal panel, otherwise
/// one with any connected display.
fn display_gpu(drm: &Path) -> Option<String> {
    let connected = connected(drm);
    let panel = connected
        .iter()
        .find(|(name, _)| PANELS.iter().any(|p| name.contains(p)));
    panel.or(connected.first()).map(|(_, slot)| slot.clone())
}

impl Graphics {
    pub fn detect() -> Self {
        Self::scan(
            Path::new("/sys/bus/pci/devices"),
            Path::new("/sys/class/dmi/id/chassis_type"),
            Path::new("/sys/class/drm"),
        )
    }

    /// Display controllers (PCI class 0x03) under `devices`, in slot order.
    pub fn scan(devices: &Path, chassis: &Path, drm: &Path) -> Self {
        let mut found: Vec<(String, String)> = fs::read_dir(devices)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let read = |name: &str| fs::read_to_string(entry.path().join(name)).ok();
                let display = read("class")?.trim().starts_with("0x03");
                let vendor = read("vendor")?.trim().to_owned();
                let slot = entry.file_name().to_string_lossy().into_owned();
                display.then_some((slot, vendor))
            })
            .collect();
        found.sort();
        let slot_of = |vendor: &str| {
            found
                .iter()
                .find(|(_, v)| v == vendor)
                .map(|(slot, _)| slot.clone())
        };
        let Some(nvidia) = slot_of(NVIDIA).and_then(|s| bus_id(&s)) else {
            return Self::default();
        };
        let integrated = slot_of(INTEL)
            .and_then(|s| Some((bus_id(&s).map(Integrated::Intel)?, s)))
            .or_else(|| slot_of(AMD).and_then(|s| Some((bus_id(&s).map(Integrated::Amd)?, s))));
        let laptop = fs::read_to_string(chassis)
            .ok()
            .and_then(|t| t.trim().parse().ok())
            .is_some_and(|t: u32| LAPTOP_CHASSIS.contains(&t));
        let offload = match (&integrated, display_gpu(drm)) {
            (Some((_, slot)), Some(display)) => display == *slot,
            // No connected display is visible: assume the usual wiring.
            (Some(_), None) => laptop,
            (None, _) => false,
        };
        Self {
            nvidia: Some(nvidia),
            integrated: integrated.map(|(gpu, _)| gpu),
            offload,
        }
    }

    /// Requests carry the GUI's detection; the helper accepts only bus IDs.
    pub fn check(&self) -> Result<()> {
        let ids = [
            self.nvidia.as_deref(),
            self.integrated.as_ref().map(Integrated::bus_id),
        ];
        ensure!(
            ids.iter().flatten().all(|id| valid_bus_id(id)),
            "Invalid GPU bus ID"
        );
        ensure!(
            self.nvidia.is_some() || (self.integrated.is_none() && !self.offload),
            "Integrated GPU details need an NVIDIA GPU"
        );
        ensure!(
            !self.offload || self.integrated.is_some(),
            "PRIME offload needs an integrated GPU"
        );
        Ok(())
    }

    /// For the review page.
    pub fn describe(&self, allow_unfree: bool) -> String {
        match (&self.nvidia, &self.integrated, allow_unfree) {
            (None, _, _) => "No NVIDIA GPU; the open-source drivers are used".into(),
            (Some(_), _, false) => {
                "NVIDIA GPU found, but its driver needs unfree packages: nouveau is used".into()
            }
            (Some(_), None, true) => "NVIDIA driver (latest release)".into(),
            (Some(_), Some(gpu), true) if self.offload => format!(
                "NVIDIA driver (latest release); {} graphics drives the displays",
                gpu.name()
            ),
            (Some(_), Some(_), true) => {
                "NVIDIA driver (latest release); the NVIDIA GPU drives the displays".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(root: &Path, slot: &str, vendor: &str, class: &str) {
        let dir = root.join("devices").join(slot);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("vendor"), format!("{vendor}\n")).unwrap();
        fs::write(dir.join("class"), format!("{class}\n")).unwrap();
    }

    /// A connector as sysfs lays it out: /sys/class/drm/cardN-eDP-1 links to
    /// .../<PCI slot>/drm/cardN/cardN-eDP-1.
    fn connector(root: &Path, slot: &str, card: &str, name: &str, status: &str) {
        let real = root
            .join("sys/devices/pci0000:00")
            .join(slot)
            .join("drm")
            .join(card)
            .join(format!("{card}-{name}"));
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("status"), format!("{status}\n")).unwrap();
        fs::create_dir_all(root.join("drm")).unwrap();
        std::os::unix::fs::symlink(&real, root.join("drm").join(format!("{card}-{name}"))).unwrap();
    }

    fn scan(root: &Path, chassis: u32) -> Graphics {
        fs::write(root.join("chassis_type"), format!("{chassis}\n")).unwrap();
        Graphics::scan(
            &root.join("devices"),
            &root.join("chassis_type"),
            &root.join("drm"),
        )
    }

    /// An Intel and NVIDIA machine, as on an Acer Predator Helios Neo 14.
    fn hybrid() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        device(dir.path(), "0000:00:02.0", INTEL, "0x030000");
        device(dir.path(), "0000:01:00.0", NVIDIA, "0x030000");
        device(dir.path(), "0000:01:00.1", NVIDIA, "0x040300"); // HDMI audio
        device(dir.path(), "0000:02:00.0", "0x144d", "0x010802"); // NVMe
        dir
    }

    #[test]
    fn panel_on_the_integrated_gpu_uses_offload() {
        let dir = hybrid();
        connector(dir.path(), "0000:00:02.0", "card1", "eDP-1", "connected");
        // An external monitor on the NVIDIA GPU does not change the choice.
        connector(dir.path(), "0000:01:00.0", "card2", "HDMI-A-1", "connected");
        let found = scan(dir.path(), 10);
        assert_eq!(found.nvidia.as_deref(), Some("PCI:1:0:0"));
        assert_eq!(
            found.integrated,
            Some(Integrated::Intel("PCI:0:2:0".into()))
        );
        assert!(found.offload);
        found.check().unwrap();
    }

    #[test]
    fn panel_on_the_nvidia_gpu_makes_it_primary() {
        // MUX in discrete mode, as the Helios Neo 14 reported: the Intel GPU
        // shows a connected DP connector, but the panel is the NVIDIA eDP.
        let dir = hybrid();
        connector(dir.path(), "0000:00:02.0", "card1", "DP-1", "connected");
        connector(dir.path(), "0000:00:02.0", "card1", "DP-2", "disconnected");
        connector(dir.path(), "0000:01:00.0", "card2", "eDP-1", "connected");
        connector(
            dir.path(),
            "0000:01:00.0",
            "card2",
            "HDMI-A-1",
            "disconnected",
        );
        let found = scan(dir.path(), 10);
        assert_eq!(
            found.integrated,
            Some(Integrated::Intel("PCI:0:2:0".into()))
        );
        assert!(!found.offload);
        assert_eq!(
            display_gpu(&dir.path().join("drm")).as_deref(),
            Some("0000:01:00.0")
        );
    }

    #[test]
    fn desktops_follow_the_connected_monitor() {
        let dir = hybrid();
        connector(dir.path(), "0000:01:00.0", "card2", "DP-3", "connected");
        assert!(!scan(dir.path(), 3).offload);
        let dir = hybrid();
        connector(dir.path(), "0000:00:02.0", "card1", "HDMI-A-1", "connected");
        assert!(scan(dir.path(), 3).offload);
        // Nothing visible: laptops assume hybrid wiring, desktops the card.
        let dir = hybrid();
        assert!(scan(dir.path(), 10).offload);
        assert!(!scan(dir.path(), 3).offload);
    }

    #[test]
    fn amd_laptops_and_machines_without_nvidia() {
        let dir = tempfile::tempdir().unwrap();
        device(dir.path(), "0000:c1:00.0", AMD, "0x038000");
        assert_eq!(scan(dir.path(), 31), Graphics::default());
        device(dir.path(), "0000:64:00.0", NVIDIA, "0x030200");
        connector(dir.path(), "0000:c1:00.0", "card0", "eDP-1", "connected");
        let found = scan(dir.path(), 31);
        assert_eq!(found.nvidia.as_deref(), Some("PCI:100:0:0"));
        assert_eq!(
            found.integrated,
            Some(Integrated::Amd("PCI:193:0:0".into()))
        );
        assert!(found.offload);
    }

    #[test]
    fn bus_ids_are_decimal_and_validated() {
        assert_eq!(bus_id("0000:0a:1f.7").as_deref(), Some("PCI:10:31:7"));
        assert_eq!(bus_id("0001:01:00.0").as_deref(), Some("PCI:1@1:0:0"));
        assert_eq!(bus_id("garbage"), None);
        for good in ["PCI:1:0:0", "PCI:1@1:0:0", "PCI:193:0:0"] {
            assert!(valid_bus_id(good), "{good}");
        }
        for bad in [
            "",
            "PCI:1:0",
            "PCI:1:0:0:0",
            "PCI:x:0:0",
            "pci:1:0:0",
            "PCI:1:0:0\"; x",
        ] {
            assert!(!valid_bus_id(bad), "{bad}");
        }
        let without_nvidia = Graphics {
            nvidia: None,
            integrated: Some(Integrated::Intel("PCI:0:2:0".into())),
            offload: true,
        };
        assert!(without_nvidia.check().is_err());
        let offload_alone = Graphics {
            nvidia: Some("PCI:1:0:0".into()),
            integrated: None,
            offload: true,
        };
        assert!(offload_alone.check().is_err());
    }
}
