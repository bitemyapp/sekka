//! What's plugged in, from sysfs alone (no device is opened): the
//! connected connectors with their EDIDs and DDC buses, grouped into the
//! monitors they belong to; the Studio Displays' HID brightness
//! interfaces, and which monitor each belongs to.
//!
//! A tiled monitor (the Studio Display over Thunderbolt is two
//! DisplayPort connectors, one per half) is one monitor: connectors whose
//! EDIDs share a DisplayID container ID or tiled-display group go
//! together. A HID interface belongs to the monitor whose container ID
//! its USB device (or a hub above it, inside the same monitor) carries in
//! its Container ID capability; without one, a lone Studio Display and a
//! lone Studio Display HID interface go together.

use super::edid::{self, Edid};
use super::hid::{self, Field};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// A connected connector.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Connector {
    /// Its sysfs name: `card0-DP-1`.
    pub sysfs: String,
    /// The compositor's name for it: `DP-1`.
    pub name: String,
    pub edid: Option<Edid>,
    /// A hash of the EDID's bytes, to know a probe's display is the same.
    pub edid_hash: u64,
    /// Its DDC channel's I2C bus number (`/dev/i2c-N`).
    pub bus: Option<u32>,
}

impl Connector {
    /// A laptop's own panel: brightness through its backlight instead.
    pub fn builtin(&self) -> bool {
        ["eDP", "LVDS", "DSI"].iter().any(|p| self.name.starts_with(p))
    }
}

/// The connectors of one physical monitor.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Group {
    pub connectors: Vec<Connector>,
}

impl Group {
    pub fn edid(&self) -> Option<&Edid> {
        self.connectors.iter().find_map(|c| c.edid.as_ref())
    }

    pub fn container_id(&self) -> Option<[u8; 16]> {
        self.connectors.iter().find_map(|c| c.edid.as_ref()?.container_id)
    }

    /// An Apple Studio Display (which takes brightness over USB, not DDC):
    /// the 2022 model's EDID names it `StudioDisplay`; the newer models'
    /// names haven't been seen here, so any Apple name with "studio" in it.
    pub fn studio_display(&self) -> bool {
        self.edid().is_some_and(|e| e.make == "APP" && e.model.to_ascii_lowercase().contains("studio"))
    }
}

/// A Studio Display's HID brightness interface.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HidDisplay {
    /// `hidraw7`.
    pub node: String,
    pub product: u16,
    /// The USB interface number, when sysfs says.
    pub interface: Option<u8>,
    pub field: Field,
    /// Container IDs of its USB device and the hubs above it.
    pub containers: Vec<[u8; 16]>,
}

/// What sysfs shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Scan {
    /// External monitors (built-in panels are left out).
    pub groups: Vec<Group>,
    pub hid: Vec<HidDisplay>,
    /// A Studio Display on Thunderbolt that the computer hasn't authorized:
    /// over Thunderbolt 3 its USB devices come only once it is.
    pub unauthorized_display: bool,
}

impl Scan {
    /// Which HID interface each group takes, by index.
    pub fn pairs(&self) -> Vec<Option<usize>> {
        let mut pairs: Vec<Option<usize>> = vec![None; self.groups.len()];
        let mut taken = vec![false; self.hid.len()];
        for (g, group) in self.groups.iter().enumerate() {
            let Some(id) = group.container_id() else { continue };
            if let Some(h) = (0..self.hid.len())
                .find(|h| !taken[*h] && self.hid[*h].containers.iter().any(|c| edid::same_container(&id, c)))
            {
                pairs[g] = Some(h);
                taken[h] = true;
            }
        }
        let lone_groups: Vec<usize> =
            (0..self.groups.len()).filter(|g| pairs[*g].is_none() && self.groups[*g].studio_display()).collect();
        let lone_hid: Vec<usize> = (0..self.hid.len()).filter(|h| !taken[*h]).collect();
        if let ([g], [h]) = (lone_groups.as_slice(), lone_hid.as_slice()) {
            pairs[*g] = Some(*h);
        }
        pairs
    }

    /// HID interfaces no monitor took.
    pub fn unpaired_hid(&self) -> usize {
        let pairs = self.pairs();
        (0..self.hid.len()).filter(|h| !pairs.contains(&Some(*h))).count()
    }
}

fn read_string(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_owned())
}

fn hash(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// `card0-DP-1` as (`card0`, `DP-1`).
fn connector_name(entry: &str) -> Option<(&str, &str)> {
    let (card, name) = entry.split_once('-')?;
    (card.starts_with("card") && card[4..].chars().all(|c| c.is_ascii_digit()) && card.len() > 4)
        .then_some((card, name))
}

/// The bus number of `i2c-13` (the file name of a `ddc` link or a
/// connector's own adapter directory).
fn bus_number(name: &str) -> Option<u32> {
    name.strip_prefix("i2c-")?.parse().ok()
}

/// The connected connectors under `sys/class/drm`, sorted.
pub(crate) fn connectors(sys: &Path) -> Vec<Connector> {
    let Ok(entries) = std::fs::read_dir(sys.join("class/drm")) else { return Vec::new() };
    let mut out: Vec<Connector> = entries
        .flatten()
        .filter_map(|e| {
            let sysfs = e.file_name().to_string_lossy().into_owned();
            let (_, name) = connector_name(&sysfs)?;
            let dir = e.path();
            if read_string(&dir.join("status")).as_deref() != Some("connected") {
                return None;
            }
            let bytes = std::fs::read(dir.join("edid")).unwrap_or_default();
            let bus = std::fs::read_link(dir.join("ddc"))
                .ok()
                .and_then(|l| l.file_name().and_then(|n| bus_number(&n.to_string_lossy())))
                .or_else(|| {
                    std::fs::read_dir(&dir).ok()?.flatten().find_map(|c| bus_number(&c.file_name().to_string_lossy()))
                });
            Some(Connector { name: name.to_owned(), edid: edid::parse(&bytes), edid_hash: hash(&bytes), bus, sysfs })
        })
        .collect();
    out.sort_by(|a, b| a.sysfs.cmp(&b.sysfs));
    out
}

/// The external monitors: connectors grouped by container ID or tile
/// group, else one each.
pub(crate) fn groups(connectors: Vec<Connector>) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for c in connectors.into_iter().filter(|c| !c.builtin()) {
        let same = |g: &Group| {
            g.connectors.iter().any(|o| {
                let (Some(a), Some(b)) = (&o.edid, &c.edid) else { return false };
                (a.container_id.is_some() && a.container_id == b.container_id)
                    || (a.tile_group.is_some() && a.tile_group == b.tile_group)
            })
        };
        match groups.iter_mut().find(|g| same(g)) {
            Some(g) => g.connectors.push(c),
            None => groups.push(Group { connectors: vec![c] }),
        }
    }
    groups
}

/// `HID_ID=0003:000005AC:00001114` as (bus, vendor, product).
fn hid_id(uevent: &str) -> Option<(u16, u16, u16)> {
    let id = uevent.lines().find_map(|l| l.strip_prefix("HID_ID="))?;
    let mut parts = id.split(':').map(|p| u32::from_str_radix(p, 16).ok());
    Some((parts.next()?? as u16, parts.next()?? as u16, parts.next()?? as u16))
}

/// The Studio Displays' HID brightness interfaces under
/// `sys/class/hidraw`.
pub(crate) fn hid_displays(sys: &Path) -> Vec<HidDisplay> {
    let Ok(entries) = std::fs::read_dir(sys.join("class/hidraw")) else { return Vec::new() };
    let mut out: Vec<HidDisplay> = entries
        .flatten()
        .filter_map(|e| {
            let node = e.file_name().to_string_lossy().into_owned();
            let device = std::fs::canonicalize(e.path().join("device")).ok()?;
            let (bus, vendor, product) = hid_id(&std::fs::read_to_string(device.join("uevent")).ok()?)?;
            if bus != 3 || vendor != hid::APPLE || !hid::STUDIO_DISPLAYS.iter().any(|(p, _)| *p == product) {
                return None;
            }
            let field = hid::brightness_field(&std::fs::read(device.join("report_descriptor")).ok()?)?;
            // The HID device's parent is the USB interface (3-1.3:1.7),
            // whose parent is the USB device (3-1.3), under its hubs.
            let interface = device.parent().and_then(|i| read_string(&i.join("bInterfaceNumber")));
            let interface = interface.and_then(|n| u8::from_str_radix(&n, 16).ok());
            let mut containers = Vec::new();
            let mut usb: Option<PathBuf> = device.parent().and_then(Path::parent).map(Path::to_path_buf);
            while let Some(dir) = usb.filter(|d| d.join("idVendor").exists()) {
                if let Some(id) = std::fs::read(dir.join("bos_descriptors")).ok().and_then(|b| hid::container_id(&b)) {
                    containers.push(id);
                }
                usb = dir.parent().map(Path::to_path_buf);
            }
            Some(HidDisplay { node, product, interface, field, containers })
        })
        .collect();
    out.sort_by(|a, b| a.node.cmp(&b.node));
    out
}

/// Whether a Thunderbolt Studio Display waits for authorization.
pub(crate) fn unauthorized_display(sys: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(sys.join("bus/thunderbolt/devices")) else { return false };
    entries.flatten().any(|e| {
        let d = e.path();
        read_string(&d.join("authorized")).as_deref() == Some("0")
            && read_string(&d.join("vendor_name")).is_some_and(|v| v.starts_with("Apple"))
            && read_string(&d.join("device_name")).is_some_and(|n| n.contains("Studio Display"))
    })
}

pub(crate) fn scan(sys: &Path) -> Scan {
    Scan { groups: groups(connectors(sys)), hid: hid_displays(sys), unauthorized_display: unauthorized_display(sys) }
}

/// Whether this process may read and write `path`.
pub(crate) fn accessible(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return false };
    // SAFETY: a valid NUL-terminated path; access() only reads it.
    unsafe { libc::access(c.as_ptr(), libc::R_OK | libc::W_OK) == 0 }
}

/// A cheap summary of everything a scan depends on, without opening a
/// device: the connectors' states and EDIDs, the HID devices, whether
/// their device nodes can be opened (udev grants access when someone
/// logs in at the computer), Thunderbolt authorization. When it changes,
/// it's time to look again.
pub(crate) fn fingerprint(sys: &Path, dev: &Path) -> u64 {
    let mut h = DefaultHasher::new();
    if let Ok(entries) = std::fs::read_dir(sys.join("class/drm")) {
        let mut names: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        names.sort();
        for dir in names {
            dir.hash(&mut h);
            read_string(&dir.join("status")).hash(&mut h);
            std::fs::read(dir.join("edid")).unwrap_or_default().hash(&mut h);
        }
    }
    for c in connectors(sys) {
        if let Some(bus) = c.bus {
            accessible(&dev.join(format!("i2c-{bus}"))).hash(&mut h);
        }
    }
    if let Ok(entries) = std::fs::read_dir(sys.join("class/hidraw")) {
        let mut nodes: Vec<String> = entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        nodes.sort();
        for n in nodes {
            n.hash(&mut h);
            read_string(&sys.join("class/hidraw").join(&n).join("device/uevent")).hash(&mut h);
            accessible(&dev.join(&n)).hash(&mut h);
        }
    }
    unauthorized_display(sys).hash(&mut h);
    h.finish()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::edid::tests::{CONTAINER, base, studio_display};
    use super::super::hid::tests::STUDIO_DISPLAY;
    use super::*;

    /// A sysfs and /dev of their own for a test.
    pub(crate) struct Fake {
        pub(crate) root: PathBuf,
    }

    impl Fake {
        pub(crate) fn new(name: &str) -> Fake {
            let root = std::env::temp_dir().join(format!("aizome-sys-monitors-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("sys/class/drm")).unwrap();
            std::fs::create_dir_all(root.join("sys/class/hidraw")).unwrap();
            std::fs::create_dir_all(root.join("dev")).unwrap();
            Fake { root }
        }

        pub(crate) fn sys(&self) -> PathBuf {
            self.root.join("sys")
        }

        pub(crate) fn dev(&self) -> PathBuf {
            self.root.join("dev")
        }

        /// A connector, connected with `edid` (or not), its DDC on `bus`.
        pub(crate) fn connector(&self, sysfs: &str, edid: Option<&[u8]>, bus: Option<u32>) {
            let dir = self.sys().join("class/drm").join(sysfs);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("status"), if edid.is_some() { "connected\n" } else { "disconnected\n" }).unwrap();
            std::fs::write(dir.join("edid"), edid.unwrap_or_default()).unwrap();
            if let Some(bus) = bus {
                std::os::unix::fs::symlink(format!("../../../i2c-{bus}"), dir.join("ddc")).unwrap();
                std::fs::write(self.dev().join(format!("i2c-{bus}")), "").unwrap();
            }
        }

        /// A Studio Display's USB device at `port` with HID interfaces
        /// `interfaces` (hidraw numbers from `first`), the brightness one
        /// at 7; its hub carries `container`.
        pub(crate) fn studio_hid(&self, port: &str, first: u32, product: u16, container: Option<[u8; 16]>) {
            let usb = self.sys().join("devices/usb3").join(port);
            std::fs::create_dir_all(&usb).unwrap();
            std::fs::write(usb.join("idVendor"), "05ac\n").unwrap();
            let hub = usb.parent().unwrap();
            std::fs::write(hub.join("idVendor"), "1d6b\n").unwrap();
            if let Some(id) = container {
                let mut bos = vec![0x05, 0x0f, 25, 0, 1, 0x14, 0x10, 0x04, 0x00];
                bos.extend(id);
                std::fs::write(usb.join("bos_descriptors"), bos).unwrap();
            }
            for (k, interface) in [5u8, 7].into_iter().enumerate() {
                let intf = usb.join(format!("{port}:1.{interface}"));
                let hid = intf.join(format!("0003:05AC:{product:04X}.00{k}{first}"));
                std::fs::create_dir_all(&hid).unwrap();
                std::fs::write(intf.join("bInterfaceNumber"), format!("{interface:02x}\n")).unwrap();
                std::fs::write(
                    hid.join("uevent"),
                    format!("HID_ID=0003:000005AC:0000{product:04X}\nHID_UNIQ=SERIAL\n"),
                )
                .unwrap();
                // Interface 5 is something else of the display's.
                let desc: &[u8] = if interface == 7 { &STUDIO_DISPLAY } else { &[0x06, 0x00, 0xff, 0x09, 0x01] };
                std::fs::write(hid.join("report_descriptor"), desc).unwrap();
                let node = format!("hidraw{}", first + k as u32);
                let link = self.sys().join("class/hidraw").join(&node);
                std::fs::create_dir_all(&link).unwrap();
                std::os::unix::fs::symlink(&hid, link.join("device")).unwrap();
                std::fs::write(self.dev().join(&node), "").unwrap();
            }
        }

        pub(crate) fn thunderbolt(&self, authorized: bool) {
            let d = self.sys().join("bus/thunderbolt/devices/0-1");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("authorized"), if authorized { "1\n" } else { "0\n" }).unwrap();
            std::fs::write(d.join("vendor_name"), "Apple Inc.\n").unwrap();
            std::fs::write(d.join("device_name"), "Studio Display\n").unwrap();
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// ganymede's displays: the built-in panel, a Studio Display as two
    /// tiles (DP-1, DP-2), a BenQ on HDMI, and an empty DP-3.
    pub(crate) fn ganymede(f: &Fake) {
        f.connector("card0-eDP-1", Some(&base(*b"SDC", 0x419d, 0, "ATNA40HQ10-0", None)), Some(12));
        f.connector("card0-DP-1", Some(&studio_display(0)), Some(13));
        f.connector("card0-DP-2", Some(&studio_display(1)), Some(14));
        f.connector("card0-DP-3", None, Some(15));
        f.connector(
            "card0-HDMI-A-1",
            Some(&base(*b"BNQ", 32859, 0x0101_0101, "BenQ RD280U", Some("G5R0013401Q"))),
            Some(9),
        );
        std::fs::write(f.sys().join("class/drm/version"), "drm 1.1.0\n").unwrap();
        std::fs::create_dir_all(f.sys().join("class/drm/renderD128")).unwrap();
    }

    #[test]
    fn the_tiles_are_one_monitor_and_the_panel_isnt_one() {
        let f = Fake::new("tiles");
        ganymede(&f);
        let s = scan(&f.sys());
        let names: Vec<Vec<&str>> =
            s.groups.iter().map(|g| g.connectors.iter().map(|c| c.name.as_str()).collect()).collect();
        assert_eq!(names, vec![vec!["DP-1", "DP-2"], vec!["HDMI-A-1"]]);
        assert_eq!(s.groups[0].connectors[0].bus, Some(13));
        assert!(s.groups[0].studio_display() && !s.groups[1].studio_display());
        assert_eq!(s.groups[1].edid().unwrap().model, "BenQ RD280U");
        assert!(s.hid.is_empty() && !s.unauthorized_display);
    }

    #[test]
    fn a_studio_displays_hid_goes_with_its_container() {
        let f = Fake::new("container");
        ganymede(&f);
        f.studio_hid("3-1.3", 6, 0x1114, Some(CONTAINER));
        let s = scan(&f.sys());
        assert_eq!(s.hid.len(), 1, "interface 7's descriptor has brightness; 5's hasn't");
        assert_eq!((s.hid[0].node.as_str(), s.hid[0].interface), ("hidraw7", Some(7)));
        assert_eq!(s.hid[0].containers, vec![CONTAINER]);
        assert_eq!(s.pairs(), vec![Some(0), None]);
    }

    #[test]
    fn a_lone_studio_display_takes_the_lone_interface_and_two_need_containers() {
        let f = Fake::new("lone");
        ganymede(&f);
        // The 2026 model, its USB device without a Container ID.
        f.studio_hid("3-1.3", 6, 0x1118, None);
        let s = scan(&f.sys());
        assert_eq!(s.pairs(), vec![Some(0), None]);
        // A second Studio Display, neither with a Container ID: which is
        // which can't be told.
        let mut second = studio_display(0);
        second[12] ^= 0xff;
        second[128 + 5 + 3] ^= 0xff;
        let tile = 128 + 5 + 3 + 16 + 3 + 18;
        second[tile] ^= 0xff;
        f.connector("card0-DP-4", Some(&second), Some(16));
        f.studio_hid("3-2.3", 8, 0x1114, None);
        let s = scan(&f.sys());
        assert_eq!(s.groups.len(), 3);
        assert_eq!(s.pairs(), vec![None, None, None]);
        assert_eq!(s.unpaired_hid(), 2);
    }

    #[test]
    fn other_hid_devices_and_unauthorized_thunderbolt() {
        let f = Fake::new("other");
        ganymede(&f);
        // A camera's HID (Chicony), not a display.
        let cam = f.sys().join("devices/usb3/3-4/3-4:1.4/0003:04F2:B87A.0001");
        std::fs::create_dir_all(&cam).unwrap();
        std::fs::write(cam.join("uevent"), "HID_ID=0003:000004F2:0000B87A\n").unwrap();
        std::fs::write(cam.join("report_descriptor"), STUDIO_DISPLAY).unwrap();
        std::fs::create_dir_all(f.sys().join("class/hidraw/hidraw0")).unwrap();
        std::os::unix::fs::symlink(&cam, f.sys().join("class/hidraw/hidraw0/device")).unwrap();
        f.thunderbolt(false);
        let s = scan(&f.sys());
        assert!(s.hid.is_empty(), "only Apple's Studio Displays");
        assert!(s.unauthorized_display);
        let before = fingerprint(&f.sys(), &f.dev());
        f.thunderbolt(true);
        assert_ne!(fingerprint(&f.sys(), &f.dev()), before, "authorizing it is a change");
        assert_eq!(hid_id("HID_ID=0003:000005AC:00001116\n"), Some((3, 0x05ac, 0x1116)));
        assert_eq!(connector_name("card0-HDMI-A-1"), Some(("card0", "HDMI-A-1")));
        assert_eq!(connector_name("renderD128"), None);
        assert_eq!(connector_name("cardX-DP-1"), None);
    }

    #[test]
    fn the_fingerprint_follows_plugging_in() {
        let f = Fake::new("fingerprint");
        ganymede(&f);
        let a = fingerprint(&f.sys(), &f.dev());
        assert_eq!(fingerprint(&f.sys(), &f.dev()), a, "nothing changed");
        f.connector("card0-DP-3", Some(&base(*b"DEL", 0x4321, 7, "DELL U2723QE", None)), None);
        let b = fingerprint(&f.sys(), &f.dev());
        assert_ne!(a, b);
        f.studio_hid("3-1.3", 6, 0x1114, Some(CONTAINER));
        assert_ne!(fingerprint(&f.sys(), &f.dev()), b);
    }
}
