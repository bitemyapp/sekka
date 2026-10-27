//! Read-only probe of this machine's system services, through Aizome's own
//! clients: what the systray panels and Settei would see.
//!
//! ```text
//! cargo run -p aizome-sys --example probe [-- SECTION…]
//! ```
//!
//! Sections: `network`, `bluetooth`, `power`, `disks`, `system` (logind,
//! hostnamed, timedated, localed, AccountsService) and `drm` (whether the
//! displays take gamma ramps, for Night Light); all of them by default.
//!
//! It only reads: properties, `GetManagedObjects`, NetworkManager's cached
//! access points (`GetAllAccessPoints`, never `RequestScan`), and the DRM
//! device's resources and properties without DRM master. Point
//! `DBUS_SYSTEM_BUS_ADDRESS` at a filtering proxy to have the bus enforce
//! that too. Sound isn't probed here: it would connect to whatever
//! PipeWire `PULSE_SERVER` or `XDG_RUNTIME_DIR` names.

use std::time::{Duration, Instant};

use aizome_sys::Wake;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let want = |s: &str| args.is_empty() || args.iter().any(|a| a == s);
    if want("network") {
        network();
    }
    if want("bluetooth") {
        bluetooth();
    }
    if want("power") {
        power();
    }
    if want("disks") {
        disks();
    }
    if want("system") {
        system();
    }
    if want("drm") {
        drm::probe();
    }
}

/// Wait up to five seconds for `ready`, then a moment more for the rest of
/// a daemon's objects.
fn settle(ready: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while !ready() {
        if start.elapsed() > Duration::from_secs(5) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    std::thread::sleep(Duration::from_millis(300));
    true
}

fn network() {
    use aizome_sys::network::{DeviceKind, Network};
    println!("== NetworkManager");
    let nm = Network::new(Wake::none());
    if !settle(|| nm.available()) {
        println!("  not running");
        return;
    }
    let s = nm.snapshot();
    println!(
        "  version {} state {:?} connectivity {:?} networking={} wifi={} (hardware {}) airplane={}",
        s.version,
        s.state,
        s.connectivity,
        s.networking_enabled,
        s.wifi_enabled,
        s.wifi_hardware_enabled,
        s.airplane_mode
    );
    for d in &s.devices {
        println!(
            "  device {} {:?} {:?} managed={} active={:?} ap={:?} last scan {:?} ms",
            d.interface, d.kind, d.state, d.managed, d.active_connection, d.active_access_point, d.last_scan_ms
        );
    }
    for a in &s.active {
        println!(
            "  active {:?} {} {:?} default4={} vpn={} devices={:?}",
            a.id, a.kind, a.state, a.default4, a.vpn, a.devices
        );
    }
    println!("  primary: {:?}", s.primary().map(|a| &a.id));
    println!("  access points in the model: {}", s.access_points.len());
    for ap in &s.access_points {
        println!(
            "    ap {:?} {} MHz {:?} {}% {:?} infrastructure={}",
            ap.ssid, ap.frequency, ap.band, ap.strength, ap.security, ap.infrastructure
        );
    }
    println!("  networks as the panel lists them: {}", s.networks.len());
    for n in &s.networks {
        println!(
            "    {:?} {}% ({} bars) {:?} {:?} aps={} known={} connected={} connecting={}",
            n.ssid,
            n.strength,
            n.bars(),
            n.security,
            n.bands,
            n.access_points.len(),
            n.known,
            n.connected,
            n.connecting
        );
    }
    println!("  saved connections: {}", s.connections.len());
    for c in &s.connections {
        println!("    {:?} {:?} active={} autoconnect={}", c.id, c.kind, c.active, c.autoconnect);
    }

    // NetworkManager's own cache, asked directly, to compare with the
    // device's `AccessPoints` property that the model follows.
    match zbus::blocking::Connection::system() {
        Ok(conn) => {
            for d in s.devices.iter().filter(|d| d.kind == DeviceKind::Wifi) {
                let aps: zbus::Result<Vec<zbus::zvariant::OwnedObjectPath>> = zbus::blocking::Proxy::new(
                    &conn,
                    "org.freedesktop.NetworkManager",
                    d.path.as_str(),
                    "org.freedesktop.NetworkManager.Device.Wireless",
                )
                .and_then(|p| p.call("GetAllAccessPoints", &()));
                match aps {
                    Ok(aps) => {
                        let modelled = s.access_points.iter().filter(|a| a.device == d.path).count();
                        println!(
                            "  {}: GetAllAccessPoints {} ({} in the model){}",
                            d.interface,
                            aps.len(),
                            modelled,
                            if aps.len() == modelled { "" } else { "  <-- differs" }
                        );
                        for path in aps {
                            if !s.access_points.iter().any(|a| a.path == path.as_str()) {
                                println!("    missing from the model: {path}");
                            }
                        }
                    }
                    Err(e) => println!("  {}: GetAllAccessPoints: {e}", d.interface),
                }
            }
        }
        Err(e) => println!("  no system bus for the direct check: {e}"),
    }
}

fn bluetooth() {
    use aizome_sys::bluetooth::Bluetooth;
    println!("== BlueZ");
    let bt = Bluetooth::new(Wake::none());
    if !settle(|| bt.available()) {
        println!("  not running");
        return;
    }
    let s = bt.snapshot();
    for a in &s.adapters {
        println!(
            "  adapter {} {:?} powered={} ({}) discoverable={} pairable={} discovering={}",
            a.path, a.alias, a.powered, a.power_state, a.discoverable, a.pairable, a.discovering
        );
    }
    println!(
        "  devices known: {} (paired {}, nearby {}); default adapter {:?}",
        s.devices.len(),
        s.paired().count(),
        s.nearby().count(),
        s.default_adapter().map(|a| &a.path)
    );
    for d in &s.devices {
        println!(
            "    {} {:?} {:?} paired={} bonded={} trusted={} connected={} battery={:?}",
            d.address, d.name, d.kind, d.paired, d.bonded, d.trusted, d.connected, d.battery
        );
    }
}

fn power() {
    use aizome_sys::power::{PowerProfiles, Upower};
    println!("== UPower");
    let up = Upower::new(Wake::none());
    if settle(|| up.available()) {
        let s = up.snapshot();
        println!(
            "  on battery {} plugged in {} lid present {} closed {}",
            s.on_battery,
            s.plugged_in(),
            s.lid_present,
            s.lid_closed
        );
        if let Some(d) = &s.display {
            println!(
                "  display device: present={} {:.0}% {:?} to empty {:?} to full {:?} icon {}",
                d.is_present, d.percentage, d.state, d.time_to_empty, d.time_to_full, d.icon_name
            );
        }
        for d in &s.devices {
            println!(
                "  {:?} {} supply={} present={} online={} {:.0}% {:?} {:.2} W {:.2}/{:.2}/{:.2} Wh health {:.0}% cycles {:?} thresholds {:?}-{:?} (supported {}, on {})",
                d.kind,
                d.native_path,
                d.power_supply,
                d.is_present,
                d.online,
                d.percentage,
                d.state,
                d.energy_rate,
                d.energy,
                d.energy_full,
                d.energy_full_design,
                d.capacity,
                d.charge_cycles,
                d.charge_start_threshold,
                d.charge_end_threshold,
                d.charge_threshold_supported,
                d.charge_threshold_enabled
            );
        }
        println!("  keyboard lights: {:?}", s.keyboard_backlights);
    } else {
        println!("  not running");
    }
    println!("== power-profiles-daemon");
    let pp = PowerProfiles::new(Wake::none());
    if settle(|| pp.available()) {
        let p = pp.snapshot();
        println!(
            "  active {} of {:?}; degraded {:?}; holds {:?}",
            p.active,
            p.profiles.iter().map(|p| (&p.name, &p.driver)).collect::<Vec<_>>(),
            p.performance_degraded,
            p.holds
        );
    } else {
        println!("  not running");
    }
}

fn disks() {
    use aizome_sys::disks::{Disks, format_size};
    println!("== UDisks2");
    let disks = Disks::new(Wake::none());
    if !settle(|| disks.available() && !disks.snapshot().drives.is_empty()) {
        println!("  not running, or no drives");
        return;
    }
    let s = disks.snapshot();
    println!("  version {}", s.version);
    for drive in &s.drives {
        println!(
            "  drive {} {} serial {} {:?}",
            drive.display_name(),
            format_size(drive.size),
            drive.serial,
            drive.rotation
        );
        let Some(disk) = s.drive_block(&drive.path) else { continue };
        println!("    block {}", disk.device);
        for part in s.partitions(&disk.path) {
            let Some(p) = part.partition.as_ref() else { continue };
            println!(
                "    {} #{} {:?} {} {} mounted at {:?}",
                part.device,
                p.number,
                p.type_name,
                format_size(p.size),
                part.content(),
                part.filesystem.as_ref().map(|f| &f.mount_points)
            );
        }
    }
    println!("  volumes for the desktop: {:?}", s.volumes().iter().map(|v| &v.name).collect::<Vec<_>>());
}

fn system() {
    use aizome_sys::datetime::{Hostname, Locale, TimeDate};
    use aizome_sys::power::Logind;
    println!("== logind");
    let logind = Logind::new(Wake::none());
    if settle(|| logind.available()) {
        let l = logind.snapshot();
        println!(
            "  session {} active={} locked={} idle={} lid={} external power={} docked={}",
            l.session_id,
            l.session_active,
            l.session_locked_hint,
            l.idle_hint,
            l.lid_closed,
            l.on_external_power,
            l.docked
        );
        println!(
            "  can power off {:?} reboot {:?} suspend {:?} hibernate {:?} hybrid {:?} suspend-then-hibernate {:?}",
            l.can_power_off,
            l.can_reboot,
            l.can_suspend,
            l.can_hibernate,
            l.can_hybrid_sleep,
            l.can_suspend_then_hibernate
        );
        println!("  inhibited: block {:?} delay {:?}", l.block_inhibited, l.delay_inhibited);
    } else {
        println!("  not running");
    }
    println!("== hostnamed");
    let host = Hostname::new(Wake::none());
    if settle(|| host.available()) {
        let h = host.snapshot();
        println!(
            "  {} (static {:?}, pretty {:?}) chassis {} icon {} vendor {:?} os {:?}",
            h.hostname,
            h.static_hostname,
            h.pretty_hostname,
            h.chassis,
            h.icon_name,
            h.hardware_vendor,
            h.os_pretty_name
        );
    }
    println!("== timedated");
    let time = TimeDate::new(Wake::none());
    if settle(|| time.available()) {
        let t = time.snapshot();
        println!(
            "  zone {} (managed {}) ntp={} synchronized={} can ntp={} local rtc={}",
            t.timezone, t.timezone_managed, t.ntp, t.ntp_synchronized, t.can_ntp, t.local_rtc
        );
    }
    println!("== localed");
    let locale = Locale::new(Wake::none());
    if settle(|| locale.available()) {
        let l = locale.snapshot();
        println!(
            "  {:?} (managed {}) x11 {:?}/{:?}/{:?} console {:?}",
            l.locale, l.locale_managed, l.x11_layout, l.x11_variant, l.x11_options, l.vconsole_keymap
        );
    }
    println!("== AccountsService");
    let users = aizome_sys::users::Users::new(Wake::none());
    if settle(|| users.available() && !users.snapshot().users.is_empty()) {
        let s = users.snapshot();
        for u in &s.users {
            println!(
                "  {} {:?} {:?} {:?} picture {:?}",
                u.uid,
                u.name,
                u.real_name,
                u.account_type,
                u.icon_file.as_ref().map(|_| "yes")
            );
        }
        println!("  current {:?}", s.current().map(|u| &u.name));
    } else {
        println!("  not running, or no users");
    }
}

/// The DRM side of Night Light: the compositor warms an output through its
/// CRTC's gamma ramp when the CRTC's `gamma_size` (from `GETCRTC`) is more
/// than 1 (`aizome-comp`'s `Udev::set_temperature`). Read here with the
/// mode-setting getters, which need no DRM master; connectors' modes are
/// read with a non-zero mode count so the kernel doesn't probe them.
mod drm {
    use std::os::fd::AsRawFd;

    const fn iowr(nr: u64, size: usize) -> u64 {
        (3 << 30) | ((size as u64) << 16) | ((b'd' as u64) << 8) | nr
    }

    #[repr(C)]
    #[derive(Default)]
    struct CardRes {
        fb_id_ptr: u64,
        crtc_id_ptr: u64,
        connector_id_ptr: u64,
        encoder_id_ptr: u64,
        count_fbs: u32,
        count_crtcs: u32,
        count_connectors: u32,
        count_encoders: u32,
        min_width: u32,
        max_width: u32,
        min_height: u32,
        max_height: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct ModeInfo {
        clock: u32,
        h: [u16; 5],
        v: [u16; 5],
        vrefresh: u32,
        flags: u32,
        kind: u32,
        name: [u8; 32],
    }

    #[repr(C)]
    #[derive(Default)]
    struct Crtc {
        set_connectors_ptr: u64,
        count_connectors: u32,
        crtc_id: u32,
        fb_id: u32,
        x: u32,
        y: u32,
        gamma_size: u32,
        mode_valid: u32,
        mode: ModeInfo,
    }

    #[repr(C)]
    #[derive(Default)]
    struct Connector {
        encoders_ptr: u64,
        modes_ptr: u64,
        props_ptr: u64,
        prop_values_ptr: u64,
        count_modes: u32,
        count_props: u32,
        count_encoders: u32,
        encoder_id: u32,
        connector_id: u32,
        connector_type: u32,
        connector_type_id: u32,
        connection: u32,
        mm_width: u32,
        mm_height: u32,
        subpixel: u32,
        pad: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct Encoder {
        encoder_id: u32,
        encoder_type: u32,
        crtc_id: u32,
        possible_crtcs: u32,
        possible_clones: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct ObjProps {
        props_ptr: u64,
        prop_values_ptr: u64,
        count_props: u32,
        obj_id: u32,
        obj_type: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct Property {
        values_ptr: u64,
        enum_blob_ptr: u64,
        prop_id: u32,
        flags: u32,
        name: [u8; 32],
        count_values: u32,
        count_enum_blobs: u32,
    }

    const GETRESOURCES: u64 = iowr(0xA0, std::mem::size_of::<CardRes>());
    const GETCRTC: u64 = iowr(0xA1, std::mem::size_of::<Crtc>());
    const GETENCODER: u64 = iowr(0xA6, std::mem::size_of::<Encoder>());
    const GETCONNECTOR: u64 = iowr(0xA7, std::mem::size_of::<Connector>());
    const GETPROPERTY: u64 = iowr(0xAA, std::mem::size_of::<Property>());
    const OBJ_GETPROPERTIES: u64 = iowr(0xB9, std::mem::size_of::<ObjProps>());
    const OBJECT_CRTC: u32 = 0xcccc_cccc;
    const OBJECT_CONNECTOR: u32 = 0xc0c0_c0c0;

    fn ioctl<T>(fd: i32, request: u64, arg: &mut T) -> std::io::Result<()> {
        loop {
            // Safety: `arg` is the #[repr(C)] struct the request names, and
            // every pointer in it points at a buffer as long as its count.
            let r = unsafe { libc::ioctl(fd, request as _, arg as *mut T) };
            if r == 0 {
                return Ok(());
            }
            let e = std::io::Error::last_os_error();
            if !matches!(e.raw_os_error(), Some(libc::EINTR) | Some(libc::EAGAIN)) {
                return Err(e);
            }
        }
    }

    fn name(bytes: &[u8]) -> String {
        String::from_utf8_lossy(&bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]).into_owned()
    }

    /// An object's properties: name, value.
    fn properties(fd: i32, id: u32, kind: u32) -> std::io::Result<Vec<(String, u64)>> {
        let mut q = ObjProps { obj_id: id, obj_type: kind, ..ObjProps::default() };
        ioctl(fd, OBJ_GETPROPERTIES, &mut q)?;
        let mut ids = vec![0u32; q.count_props as usize];
        let mut values = vec![0u64; q.count_props as usize];
        q.props_ptr = ids.as_mut_ptr() as u64;
        q.prop_values_ptr = values.as_mut_ptr() as u64;
        ioctl(fd, OBJ_GETPROPERTIES, &mut q)?;
        let n = (q.count_props as usize).min(ids.len());
        Ok(ids[..n]
            .iter()
            .zip(&values[..n])
            .map(|(&prop_id, &value)| {
                let mut p = Property { prop_id, ..Property::default() };
                let label = ioctl(fd, GETPROPERTY, &mut p).map(|()| name(&p.name)).unwrap_or_default();
                (label, value)
            })
            .collect())
    }

    fn connector_name(kind: u32, n: u32) -> String {
        let kind = match kind {
            1 => "VGA",
            2 => "DVI-I",
            3 => "DVI-D",
            10 => "DP",
            11 => "HDMI-A",
            12 => "HDMI-B",
            14 => "eDP",
            15 => "Virtual",
            16 => "DSI",
            19 => "Writeback",
            20 => "SPI",
            21 => "USB",
            _ => "Unknown",
        };
        format!("{kind}-{n}")
    }

    pub fn probe() {
        println!("== DRM (Night Light)");
        let Ok(dir) = std::fs::read_dir("/dev/dri") else {
            println!("  no /dev/dri");
            return;
        };
        let mut cards: Vec<_> = dir
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("card")))
            .collect();
        cards.sort();
        for path in cards {
            // Read only, and never SET_MASTER: whoever drives the displays
            // keeps them.
            let file = match std::fs::File::open(&path) {
                Ok(f) => f,
                Err(e) => {
                    println!("  {}: {e}", path.display());
                    continue;
                }
            };
            let fd = file.as_raw_fd();
            if let Err(e) = card(fd, &path.display().to_string()) {
                println!("  {}: {e}", path.display());
            }
        }
    }

    fn card(fd: i32, label: &str) -> std::io::Result<()> {
        let mut res = CardRes::default();
        ioctl(fd, GETRESOURCES, &mut res)?;
        let mut crtcs = vec![0u32; res.count_crtcs as usize];
        let mut connectors = vec![0u32; res.count_connectors as usize];
        let mut encoders = vec![0u32; res.count_encoders as usize];
        res.count_fbs = 0;
        res.crtc_id_ptr = crtcs.as_mut_ptr() as u64;
        res.connector_id_ptr = connectors.as_mut_ptr() as u64;
        res.encoder_id_ptr = encoders.as_mut_ptr() as u64;
        ioctl(fd, GETRESOURCES, &mut res)?;
        crtcs.truncate(res.count_crtcs as usize);
        connectors.truncate(res.count_connectors as usize);
        println!("  {label}: {} CRTCs, {} connectors", crtcs.len(), connectors.len());

        for &id in &connectors {
            // One mode slot, so the kernel answers from what it knows
            // instead of probing the connector.
            let mut mode = [ModeInfo::default()];
            let mut c = Connector {
                connector_id: id,
                count_modes: 1,
                modes_ptr: mode.as_mut_ptr() as u64,
                ..Connector::default()
            };
            ioctl(fd, GETCONNECTOR, &mut c)?;
            let crtc = if c.encoder_id != 0 {
                let mut e = Encoder { encoder_id: c.encoder_id, ..Encoder::default() };
                ioctl(fd, GETENCODER, &mut e).map(|()| e.crtc_id).unwrap_or(0)
            } else {
                0
            };
            let gamma: Vec<String> = properties(fd, id, OBJECT_CONNECTOR)?
                .into_iter()
                .filter(|(n, _)| n.contains("GAMMA"))
                .map(|(n, v)| format!("{n}={v}"))
                .collect();
            println!(
                "    connector {} {} crtc {} gamma properties {:?}",
                connector_name(c.connector_type, c.connector_type_id),
                match c.connection {
                    1 => "connected",
                    2 => "disconnected",
                    _ => "unknown",
                },
                crtc,
                gamma
            );
        }
        for &id in &crtcs {
            let mut c = Crtc { crtc_id: id, ..Crtc::default() };
            ioctl(fd, GETCRTC, &mut c)?;
            let props = properties(fd, id, OBJECT_CRTC)?;
            let colour: Vec<String> = props
                .iter()
                .filter(|(n, _)| n.contains("GAMMA") || n.contains("CTM"))
                .map(|(n, v)| format!("{n}={v}"))
                .collect();
            println!(
                "    crtc {id} {} {}x{} gamma_size {} -> Night Light {}; {:?}",
                if c.mode_valid != 0 { "lit" } else { "off" },
                c.mode.h[0],
                c.mode.v[0],
                c.gamma_size,
                if c.gamma_size > 1 { "possible" } else { "not possible" },
                colour
            );
        }
        Ok(())
    }
}
