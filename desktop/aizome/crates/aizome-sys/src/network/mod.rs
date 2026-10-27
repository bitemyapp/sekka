//! Networks, through NetworkManager's D-Bus API.
//!
//! [`Network`] follows NetworkManager's `ObjectManager` (devices, access
//! points, active connections, IP configurations, saved profiles), so the
//! [`NetworkState`] snapshot is always current without polling. Wi-Fi
//! networks come grouped by name with their strength, security, bands and
//! whether they're known or connected.
//!
//! Actions:
//! - Wi-Fi, mobile and networking switches; **airplane mode** blocks every
//!   radio through `/dev/rfkill` (Wi-Fi, Bluetooth, mobile, as the hardware
//!   key does), falling back, when rfkill can't be written, to
//!   NetworkManager's Wi-Fi and mobile switches and BlueZ's adapters
//!   (those on are switched on again after, kept in
//!   [`RadioPaths::fallback`] meanwhile);
//! - scan; join a network (with a password, enterprise credentials, or a
//!   hidden one by name); disconnect; forget; auto-connect; metered;
//! - VPN and WireGuard: activate, deactivate, import a `.conf` / `.ovpn`
//!   file (through `nmcli connection import`, which runs the VPN plugin's
//!   importer);
//! - a hotspot sharing this machine's connection.
//!
//! This client does not register a secret agent: the independent
//! `aizome-network-agent` session service handles NetworkManager's later
//! credential requests, including keyring and VPN authentication.
//! Passwords supplied here go into the profile when joining (owned by
//! the system, readable by root only), and a new
//! profile that fails to join is removed again, so a mistyped password
//! leaves nothing behind. A saved network joined with a new password gets
//! it in memory first and keeps it only once joining works.
//!
//! Needs NetworkManager on the system bus (`networking.networkmanager.enable`).
//! Changing system connections asks polkit; NixOS's NetworkManager allows
//! the active session's user (in the `networkmanager` group, or through
//! polkit's `org.freedesktop.NetworkManager.*` actions).

mod model;
mod profile;
pub(crate) mod rfkill;

pub use profile::{IPV6_METHODS, Profile, read_profile};

pub use model::{
    AccessPoint, ActiveConnection, ActiveState, Band, ConnectionKind, ConnectionSettings, Connectivity, Details,
    Device, DeviceKind, DeviceState, Enterprise, IpConfig, Ipv4Method, Metered, NetworkState, NmState, Proxy, Radio,
    RadioKind, SavedConnection, Security, WifiNetwork, bars, describe_reason, signal_icon, ssid_name,
};

use crate::bus::{self, Bus, Event, Objects, Opts, PropsExt, signals_from};
use crate::error::{Error, Result};
use crate::service::{Model, Stop, Task, Wake, lock, service_handle};
use model::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

/// How long joining a network may take before giving up.
const JOIN_TIMEOUT: Duration = Duration::from_secs(45);

/// Where the radio switches are; tests point these elsewhere.
#[derive(Clone, Debug)]
pub struct RadioPaths {
    /// `/dev/rfkill`.
    pub device: PathBuf,
    /// `/sys/class/rfkill`.
    pub sysfs: PathBuf,
    /// Airplane mode turned on without rfkill (NetworkManager's radios and
    /// BlueZ's adapters switched off instead): the adapters it switched off,
    /// one per line, to switch on again. Gone while it's off.
    pub fallback: PathBuf,
}

impl Default for RadioPaths {
    fn default() -> RadioPaths {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        RadioPaths {
            device: rfkill::DEVICE.into(),
            sysfs: rfkill::SYSFS.into(),
            fallback: runtime.join("aizome-airplane-mode"),
        }
    }
}

/// BlueZ, for airplane mode without rfkill.
const BLUEZ: &str = "org.bluez";
const IFACE_ADAPTER: &str = "org.bluez.Adapter1";

/// BlueZ's adapters that are on.
fn bluetooth_on(bus: &Bus) -> Vec<String> {
    match Objects::load(bus, BLUEZ, "/") {
        Ok(objects) => {
            objects.with(IFACE_ADAPTER).filter(|(_, p)| p.bool("Powered")).map(|(path, _)| path.to_owned()).collect()
        }
        Err(_) => Vec::new(),
    }
}

/// Airplane mode where rfkill can't be written: NetworkManager's Wi-Fi and
/// mobile radios, and BlueZ's adapters (those on, kept in `fallback` to
/// switch on again when it ends).
fn airplane_without_rfkill(bus: &Bus, fallback: &Path, on: bool) -> Result<()> {
    bus.set(NM, NM_PATH, NM, "WirelessEnabled", !on)?;
    // Mobile broadband, where NetworkManager has it.
    if let Err(e) = bus.set(NM, NM_PATH, NM, "WwanEnabled", !on) {
        log::debug!("aizome-sys: airplane mode: mobile broadband: {e}");
    }
    if on {
        let adapters = bluetooth_on(bus);
        for a in &adapters {
            if let Err(e) = bus.set(BLUEZ, a, IFACE_ADAPTER, "Powered", false) {
                log::warn!("aizome-sys: airplane mode: switching {a} off: {e}");
            }
        }
        std::fs::write(fallback, adapters.join("\n"))?;
    } else {
        let adapters = std::fs::read_to_string(fallback).unwrap_or_default();
        for a in adapters.lines().filter(|l| !l.trim().is_empty()) {
            if let Err(e) = bus.set(BLUEZ, a.trim(), IFACE_ADAPTER, "Powered", true) {
                log::warn!("aizome-sys: airplane mode: switching {a} on: {e}");
            }
        }
        let _ = std::fs::remove_file(fallback);
    }
    Ok(())
}

/// Whether airplane mode is on: every radio blocked, or (without rfkill)
/// NetworkManager's Wi-Fi off since airplane mode turned it off.
fn airplane(radios: &[Radio], fallback: &Path, wifi_enabled: bool) -> bool {
    (!radios.is_empty() && radios.iter().all(|r| r.soft_blocked || r.hard_blocked))
        || (!wifi_enabled && fallback.exists())
}

/// NetworkManager: devices, Wi-Fi, VPNs.
pub struct Network {
    bus: Option<Bus>,
    model: Arc<Model<NetworkState>>,
    radios: RadioPaths,
    stop: Stop,
}

service_handle!(Network, NetworkState);

impl Network {
    /// Follow NetworkManager on the system bus.
    pub fn new(wake: Wake) -> Network {
        match Bus::system() {
            Ok(bus) => Network::with_bus(bus, wake, RadioPaths::default()),
            Err(error) => {
                log::info!("aizome-sys: no system bus for NetworkManager: {error}");
                Network {
                    bus: None,
                    model: Arc::new(Model::new(NetworkState::default(), wake)),
                    radios: RadioPaths::default(),
                    stop: Stop::default(),
                }
            }
        }
    }

    /// Follow NetworkManager on `bus`, with the radio switches at `radios`.
    pub fn with_bus(bus: Bus, wake: Wake, radios: RadioPaths) -> Network {
        let model = Arc::new(Model::new(NetworkState::default(), wake));
        let stop = Stop::default();
        let shared_radios = Arc::new(Mutex::new(rfkill::read_sysfs(&radios.sysfs)));

        // Radio switches change without NetworkManager saying so (the
        // Bluetooth one, for instance): follow /dev/rfkill too.
        {
            let shared = shared_radios.clone();
            let model = model.clone();
            let fallback = radios.fallback.clone();
            rfkill::watch(radios.device.clone(), stop.clone(), move |now| {
                *lock(&shared) = now.clone();
                if model.get().available {
                    model.update(|state| {
                        state.airplane_mode = airplane(&now, &fallback, state.wifi_enabled);
                        state.radios = now;
                    });
                }
            });
        }

        let mut follower = Follower {
            objects: Objects::default(),
            settings: HashMap::new(),
            radios: shared_radios,
            fallback: radios.fallback.clone(),
            model: model.clone(),
            dirty: false,
            present: false,
        };
        bus::follow(bus.clone(), NM, vec![signals_from(NM)], false, stop.clone(), move |bus, event| {
            follower.handle(bus, event)
        });
        Network { bus: Some(bus), model, radios, stop }
    }

    /// Whether NetworkManager is running.
    pub fn available(&self) -> bool {
        self.model.get().available
    }

    fn task<T: Send + 'static>(&self, work: impl FnOnce(&Bus) -> Result<T> + Send + 'static) -> Task<T> {
        bus::spawn(&self.bus, "NetworkManager", self.model.wake(), work)
    }

    /// Turn Wi-Fi on or off.
    pub fn set_wifi_enabled(&self, on: bool) -> Task<()> {
        self.task(move |bus| bus.set(NM, NM_PATH, NM, "WirelessEnabled", on))
    }

    /// Turn mobile broadband on or off.
    pub fn set_wwan_enabled(&self, on: bool) -> Task<()> {
        self.task(move |bus| bus.set(NM, NM_PATH, NM, "WwanEnabled", on))
    }

    /// Turn networking as a whole on or off.
    pub fn set_networking_enabled(&self, on: bool) -> Task<()> {
        self.task(move |bus| bus.call_with(Opts::ACTION, NM, NM_PATH, NM, "Enable", &(on,)))
    }

    /// Airplane mode: block (or unblock) every radio through rfkill; where
    /// rfkill can't be written, switch NetworkManager's Wi-Fi and mobile
    /// radios and BlueZ's adapters off instead (and those back on after).
    pub fn set_airplane_mode(&self, on: bool) -> Task<()> {
        let device = self.radios.device.clone();
        let fallback = self.radios.fallback.clone();
        let wake = self.model.wake();
        let bus = self.bus.clone();
        let model = self.model.clone();
        Task::spawn("airplane mode", &wake, move || match rfkill::block_all(&device, on) {
            Ok(()) => {
                // Turned off through rfkill after the fallback: what the
                // fallback switched off comes back too.
                if !on
                    && fallback.exists()
                    && let Some(bus) = &bus
                {
                    airplane_without_rfkill(bus, &fallback, false)?;
                }
                Ok(())
            }
            Err(error) if rfkill::is_permission(&error) => {
                let bus = bus.ok_or_else(|| Error::Unavailable("NetworkManager".into()))?;
                airplane_without_rfkill(&bus, &fallback, on)?;
                // Shown at once (NetworkManager says Wi-Fi is off a moment
                // later), so the switch doesn't spring back.
                model.update(|s| s.airplane_mode = on || airplane(&s.radios, &fallback, s.wifi_enabled));
                Ok(())
            }
            Err(error) => Err(error),
        })
    }

    /// Ask every Wi-Fi device to scan; results arrive in later snapshots.
    /// NetworkManager refuses scans in quick succession, which is not an
    /// error here.
    pub fn scan(&self) -> Task<()> {
        let devices: Vec<String> = self
            .snapshot()
            .devices
            .iter()
            .filter(|d| d.kind == DeviceKind::Wifi && d.managed)
            .map(|d| d.path.clone())
            .collect();
        self.task(move |bus| {
            for device in devices {
                let options: HashMap<&str, Value> = HashMap::new();
                match bus.call_with::<_, ()>(Opts::ACTION, NM, &device, IFACE_WIRELESS, "RequestScan", &(options,)) {
                    Ok(()) => {}
                    Err(Error::DBus { name, .. }) if name.ends_with("NotAllowed") => {}
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        })
    }

    /// Join a network in range: through its saved connection when it has
    /// one (with `password` replacing the saved one if given), otherwise
    /// with a new connection secured as the network advertises. Waits
    /// until the connection is up; a refused password ends in
    /// [`Error::Rejected`].
    pub fn connect(&self, network: &WifiNetwork, password: Option<&str>) -> Task<()> {
        let network = network.clone();
        let password = password.map(str::to_owned);
        let saved = network.connection.as_ref().and_then(|uuid| self.snapshot().connection(uuid).cloned());
        self.task(move |bus| match saved {
            Some(saved) => match password {
                None => activate_and_wait(bus, &saved.path, &network.device, &network.best_access_point),
                Some(password) => {
                    let (group, key) = secret_key(Security::from_key_mgmt(saved.key_mgmt.as_deref()));
                    rejoin(bus, &saved, group, key, &password, &network.device, &network.best_access_point)
                }
            },
            None => {
                if network.security.is_enterprise() {
                    return Err(Error::InvalidArgument("This network needs a username: use connect_enterprise".into()));
                }
                if network.security.needs_password() && password.as_deref().is_none_or(str::is_empty) {
                    return Err(Error::InvalidArgument("This network needs a password".into()));
                }
                let settings = wifi_settings(&network.ssid_bytes, network.security, password.as_deref(), None, false);
                add_and_wait(bus, settings, &network.device, &network.best_access_point)
            }
        })
    }

    /// Join an enterprise (802.1X) network with a username and password.
    /// A saved profile takes the new account in memory first and keeps it
    /// only once joining works (as a new password does, [`Self::connect`]):
    /// a refused account leaves the saved profile as it was.
    pub fn connect_enterprise(&self, network: &WifiNetwork, credentials: Enterprise) -> Task<()> {
        let network = network.clone();
        let saved = network.connection.as_ref().and_then(|uuid| self.snapshot().connection(uuid).cloned());
        self.task(move |bus| {
            let settings = wifi_settings(&network.ssid_bytes, Security::Enterprise, None, Some(&credentials), false);
            match &saved {
                Some(saved) => rejoin_with(
                    bus,
                    saved,
                    |current| enterprise_account(current, &settings),
                    &network.device,
                    &network.best_access_point,
                ),
                None => add_and_wait(bus, settings, &network.device, &network.best_access_point),
            }
        })
    }

    /// Join a hidden network by name.
    pub fn connect_hidden(&self, ssid: &str, security: Security, password: Option<&str>) -> Task<()> {
        let ssid = ssid.to_owned();
        let password = password.map(str::to_owned);
        let device = self.snapshot().wifi_device().map(|d| d.path.clone());
        self.task(move |bus| {
            if ssid.is_empty() || ssid.len() > 32 {
                return Err(Error::InvalidArgument("A network name has 1 to 32 bytes".into()));
            }
            if security.is_enterprise() {
                return Err(Error::InvalidArgument("Use connect_hidden_enterprise".into()));
            }
            let device = device.ok_or_else(|| Error::NotFound("No Wi-Fi device".into()))?;
            let settings = wifi_settings(ssid.as_bytes(), security, password.as_deref(), None, true);
            add_and_wait(bus, settings, &device, "/")
        })
    }

    /// Join a hidden enterprise network by name.
    pub fn connect_hidden_enterprise(&self, ssid: &str, credentials: Enterprise) -> Task<()> {
        let ssid = ssid.to_owned();
        let device = self.snapshot().wifi_device().map(|d| d.path.clone());
        self.task(move |bus| {
            let device = device.ok_or_else(|| Error::NotFound("No Wi-Fi device".into()))?;
            let settings = wifi_settings(ssid.as_bytes(), Security::Enterprise, None, Some(&credentials), true);
            add_and_wait(bus, settings, &device, "/")
        })
    }

    /// Bring up a saved connection (Wi-Fi, wired, VPN, WireGuard) and wait
    /// until it is up.
    pub fn activate(&self, uuid: &str) -> Task<()> {
        let uuid = uuid.to_owned();
        self.task(move |bus| {
            let path = connection_path(bus, &uuid)?;
            activate_and_wait(bus, &path, "/", "/")
        })
    }

    /// Take down an active connection.
    pub fn deactivate(&self, uuid: &str) -> Task<()> {
        let active = self.snapshot().active_connection(uuid).map(|a| a.path.clone());
        let uuid = uuid.to_owned();
        self.task(move |bus| {
            let active = active.ok_or_else(|| Error::NotFound(format!("{uuid} is not active")))?;
            let active = ObjectPath::try_from(active.as_str())?;
            bus.call_with(Opts::ACTION, NM, NM_PATH, NM, "DeactivateConnection", &(active,))
        })
    }

    /// Disconnect a device; it stays down until asked to connect again
    /// (no auto-connect), as a "Disconnect" button means.
    pub fn disconnect_device(&self, device_path: &str) -> Task<()> {
        let device = device_path.to_owned();
        self.task(move |bus| bus.call_with(Opts::ACTION, NM, &device, IFACE_DEVICE, "Disconnect", &()))
    }

    /// Delete a saved connection.
    pub fn forget(&self, uuid: &str) -> Task<()> {
        let uuid = uuid.to_owned();
        self.task(move |bus| {
            let path = connection_path(bus, &uuid)?;
            forget_paths(bus, &[path])
        })
    }

    /// Delete every saved connection for a Wi-Fi network.
    pub fn forget_network(&self, ssid: &str) -> Task<()> {
        let paths: Vec<String> =
            self.snapshot().saved_wifi().filter(|c| c.ssid.as_deref() == Some(ssid)).map(|c| c.path.clone()).collect();
        self.task(move |bus| forget_paths(bus, &paths))
    }

    /// Whether a saved connection joins by itself.
    pub fn set_autoconnect(&self, uuid: &str, on: bool) -> Task<()> {
        let uuid = uuid.to_owned();
        self.task(move |bus| update_setting(bus, &uuid, "connection", "autoconnect", Value::from(on)))
    }

    /// Mark a saved connection metered (or not, or automatic).
    pub fn set_metered(&self, uuid: &str, metered: Metered) -> Task<()> {
        let uuid = uuid.to_owned();
        self.task(move |bus| update_setting(bus, &uuid, "connection", "metered", Value::from(metered.setting())))
    }

    /// How a saved connection gets its IPv4 address: automatically (DHCP)
    /// or fixed ones; reconnect it for the change to take effect.
    pub fn set_ipv4(&self, uuid: &str, method: Ipv4Method) -> Task<()> {
        if let Ipv4Method::Manual { addresses, .. } = &method
            && (addresses.is_empty() || addresses.iter().any(|(_, prefix)| !(1..=32).contains(prefix)))
        {
            return Task::ready(Err(Error::InvalidArgument(
                "Give at least one address with a prefix of 1 to 32".into(),
            )));
        }
        let uuid = uuid.to_owned();
        self.task(move |bus| update_settings(bus, &uuid, |settings| apply_ipv4(settings, &method)))
    }

    /// A saved connection's DNS servers: `servers` instead of the
    /// network's (`automatic` false), or as well (`automatic` true; with no
    /// servers, the network's alone).
    pub fn set_dns(&self, uuid: &str, servers: &[std::net::IpAddr], automatic: bool) -> Task<()> {
        let uuid = uuid.to_owned();
        let servers = servers.to_vec();
        self.task(move |bus| update_settings(bus, &uuid, |settings| apply_dns(settings, &servers, automatic)))
    }

    /// Import a VPN configuration file: `.conf` (WireGuard) or `.ovpn`
    /// (OpenVPN; needs NetworkManager's OpenVPN plugin), or another type
    /// given by its plugin name (`vpnc`, `openconnect`…). Returns the new
    /// connection's UUID.
    pub fn import_vpn(&self, file: &Path, kind: Option<&str>) -> Task<String> {
        let file = file.to_owned();
        let kind = kind.map(str::to_owned);
        Task::spawn("VPN import", &self.model.wake(), move || import_vpn(&file, kind.as_deref()))
    }

    /// Share this machine's connection over Wi-Fi as a WPA2 hotspot.
    pub fn start_hotspot(&self, ssid: &str, password: &str, band: Option<Band>) -> Task<()> {
        let state = self.snapshot();
        let device = state.wifi_device().map(|d| d.path.clone());
        let old: Vec<String> =
            state.saved_wifi().filter(|c| c.wifi_mode.as_deref() == Some("ap")).map(|c| c.path.clone()).collect();
        let ssid = ssid.to_owned();
        let password = password.to_owned();
        self.task(move |bus| {
            if ssid.is_empty() || ssid.len() > 32 {
                return Err(Error::InvalidArgument("A network name has 1 to 32 bytes".into()));
            }
            if !(8..=63).contains(&password.len()) {
                return Err(Error::InvalidArgument("A hotspot password has 8 to 63 characters".into()));
            }
            let device = device.ok_or_else(|| Error::NotFound("No Wi-Fi device".into()))?;
            forget_paths(bus, &old)?;
            add_and_wait(bus, hotspot_settings(&ssid, &password, band), &device, "/")
        })
    }

    /// Stop the hotspot.
    pub fn stop_hotspot(&self) -> Task<()> {
        let active = self.snapshot().hotspot().map(|a| a.path.clone());
        self.task(move |bus| {
            let Some(active) = active else { return Ok(()) };
            let active = ObjectPath::try_from(active.as_str())?;
            bus.call_with(Opts::ACTION, NM, NM_PATH, NM, "DeactivateConnection", &(active,))
        })
    }

    /// A saved connection's full settings (without secrets), for a details
    /// page that shows more than [`SavedConnection`] does.
    pub fn connection_settings(&self, uuid: &str) -> Task<ConnectionSettings> {
        let uuid = uuid.to_owned();
        self.task(move |bus| {
            let path = connection_path(bus, &uuid)?;
            bus.call(NM, &path, IFACE_CONNECTION, "GetSettings", &())
        })
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

/// Keeps the model current from NetworkManager's signals.
struct Follower {
    objects: Objects,
    settings: HashMap<String, ConnectionSettings>,
    radios: Arc<Mutex<Vec<Radio>>>,
    /// Airplane mode turned on without rfkill ([`RadioPaths::fallback`]).
    fallback: PathBuf,
    model: Arc<Model<NetworkState>>,
    dirty: bool,
    present: bool,
}

impl Follower {
    fn handle(&mut self, bus: &Bus, event: Event) {
        match event {
            Event::Appeared => {
                self.present = true;
                self.objects = Objects::load(bus, NM, "/org/freedesktop").unwrap_or_else(|error| {
                    log::warn!("aizome-sys: reading NetworkManager's objects: {error}");
                    Objects::default()
                });
                let paths: Vec<String> = self.objects.with(IFACE_CONNECTION).map(|(p, _)| p.to_owned()).collect();
                self.settings =
                    paths.into_iter().filter_map(|path| Some((path.clone(), get_settings(bus, &path)?))).collect();
                self.dirty = true;
            }
            Event::Vanished => {
                self.present = false;
                self.objects = Objects::default();
                self.settings.clear();
                let radios = lock(&self.radios).clone();
                self.model.set_if_changed(NetworkState { radios, ..NetworkState::default() });
            }
            Event::Signal(msg) => {
                let (iface, member) = bus::member(&msg);
                let path = bus::path(&msg);
                let applied = self.objects.apply(&msg);
                if !applied.invalidated.is_empty() {
                    self.objects.refetch(bus, NM, applied.invalidated);
                }
                self.dirty |= applied.changed;
                match (iface.as_str(), member.as_str()) {
                    ("org.freedesktop.DBus.ObjectManager", "InterfacesAdded") => {
                        let added = msg
                            .body()
                            .deserialize::<(OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>)>()
                            .ok();
                        if let Some((path, ifaces)) = added
                            && ifaces.contains_key(IFACE_CONNECTION)
                            && let Some(settings) = get_settings(bus, path.as_str())
                        {
                            self.settings.insert(path.to_string(), settings);
                        }
                    }
                    ("org.freedesktop.DBus.ObjectManager", "InterfacesRemoved") => {
                        if let Ok((path, _)) = msg.body().deserialize::<(OwnedObjectPath, Vec<String>)>()
                            && self.objects.get(path.as_str(), IFACE_CONNECTION).is_none()
                        {
                            self.settings.remove(path.as_str());
                        }
                    }
                    (IFACE_CONNECTION, "Updated") => {
                        if let Some(settings) = get_settings(bus, &path) {
                            self.settings.insert(path, settings);
                            self.dirty = true;
                        }
                    }
                    (IFACE_CONNECTION, "Removed") => {
                        self.settings.remove(&path);
                        self.dirty = true;
                    }
                    _ => {}
                }
            }
            Event::Idle => {
                if self.present && self.dirty {
                    let radios = lock(&self.radios).clone();
                    let mut state = build(&self.objects, &self.settings, &radios);
                    state.airplane_mode |= !state.wifi_enabled && self.fallback.exists();
                    self.model.set_if_changed(state);
                    self.dirty = false;
                }
            }
        }
    }
}

fn get_settings(bus: &Bus, path: &str) -> Option<ConnectionSettings> {
    bus.call(NM, path, IFACE_CONNECTION, "GetSettings", &())
        .inspect_err(|error| log::debug!("aizome-sys: settings of {path}: {error}"))
        .ok()
}

fn connection_path(bus: &Bus, uuid: &str) -> Result<String> {
    let path: OwnedObjectPath = bus.call(NM, SETTINGS_PATH, IFACE_SETTINGS, "GetConnectionByUuid", &(uuid,))?;
    Ok(path.to_string())
}

fn object_path(path: &str) -> Result<ObjectPath<'_>> {
    let path = if path.is_empty() { "/" } else { path };
    ObjectPath::try_from(path).map_err(|e| Error::InvalidArgument(e.to_string()))
}

fn forget_paths(bus: &Bus, paths: &[String]) -> Result<()> {
    for path in paths {
        match bus.call_with::<_, ()>(Opts::ACTION, NM, path, IFACE_CONNECTION, "Delete", &()) {
            Ok(()) | Err(Error::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// The setting and key holding a network's password.
fn secret_key(security: Security) -> (&'static str, &'static str) {
    match security {
        Security::Enterprise => ("802-1x", "password"),
        Security::Wep => ("802-11-wireless-security", "wep-key0"),
        _ => ("802-11-wireless-security", "psk"),
    }
}

// Update2 flags.
const UPDATE_TO_DISK: u32 = 0x1;
const UPDATE_IN_MEMORY: u32 = 0x2;

fn update_setting(bus: &Bus, uuid: &str, group: &str, key: &str, value: Value<'static>) -> Result<()> {
    update_settings(bus, uuid, |settings| {
        settings.entry(group.to_owned()).or_default().insert(key.to_owned(), value);
    })
}

/// Change a saved connection's settings with `edit`, save them to disk,
/// and have NetworkManager apply them where the connection is up.
fn update_settings(bus: &Bus, uuid: &str, edit: impl FnOnce(&mut EditableSettings)) -> Result<()> {
    let path = connection_path(bus, uuid)?;
    let settings: ConnectionSettings = bus.call(NM, &path, IFACE_CONNECTION, "GetSettings", &())?;
    let mut updated = editable(&settings);
    edit(&mut updated);
    let args: HashMap<&str, Value> = HashMap::new();
    bus.call_with::<_, HashMap<String, OwnedValue>>(
        Opts::ACTION,
        NM,
        &path,
        IFACE_CONNECTION,
        "Update2",
        &(updated, UPDATE_TO_DISK, args),
    )
    .map(drop)
}

/// Join a saved network with a new password: in memory first, saved to
/// disk only once joining works, reloaded from disk otherwise.
fn rejoin(
    bus: &Bus,
    saved: &SavedConnection,
    group: &str,
    key: &str,
    password: &str,
    device: &str,
    access_point: &str,
) -> Result<()> {
    rejoin_with(
        bus,
        saved,
        |settings| {
            let props = settings.entry(group.to_owned()).or_default();
            props.insert(key.to_owned(), Value::from(password.to_owned()));
            props.insert(format!("{key}-flags"), Value::from(0u32));
        },
        device,
        access_point,
    )
}

/// A saved profile's settings with a new enterprise account: the new
/// `802-1x` setting in place of the old one, and WPA-EAP.
fn enterprise_account(settings: &mut EditableSettings, new: &NewSettings<'_>) {
    let owned = |props: &HashMap<&'static str, Value<'_>>| -> HashMap<String, Value<'static>> {
        props.iter().filter_map(|(k, v)| Some(((*k).to_owned(), Value::from(v.try_to_owned().ok()?)))).collect()
    };
    if let Some(eap) = new.get("802-1x") {
        settings.insert("802-1x".to_owned(), owned(eap));
    }
    settings
        .entry("802-11-wireless-security".to_owned())
        .or_default()
        .insert("key-mgmt".to_owned(), Value::from("wpa-eap"));
}

/// Join a saved network with its settings changed by `edit` (a new
/// password, a new account): in memory first, saved to disk only once
/// joining works, reloaded from disk otherwise, so a refused change never
/// costs the profile that was saved.
fn rejoin_with(
    bus: &Bus,
    saved: &SavedConnection,
    edit: impl FnOnce(&mut EditableSettings),
    device: &str,
    access_point: &str,
) -> Result<()> {
    let settings: ConnectionSettings = bus.call(NM, &saved.path, IFACE_CONNECTION, "GetSettings", &())?;
    let mut updated = editable(&settings);
    edit(&mut updated);
    let args: HashMap<&str, Value> = HashMap::new();
    bus.call_with::<_, HashMap<String, OwnedValue>>(
        Opts::ACTION,
        NM,
        &saved.path,
        IFACE_CONNECTION,
        "Update2",
        &(updated, UPDATE_IN_MEMORY, args),
    )?;
    match activate_and_wait(bus, &saved.path, device, access_point) {
        Ok(()) => bus.call_with(Opts::ACTION, NM, &saved.path, IFACE_CONNECTION, "Save", &()),
        Err(error) => {
            if !saved.filename.is_empty() {
                let _ = bus.call_with::<_, (bool, Vec<String>)>(
                    Opts::ACTION,
                    NM,
                    SETTINGS_PATH,
                    IFACE_SETTINGS,
                    "LoadConnections",
                    &(vec![saved.filename.as_str()],),
                );
            }
            Err(error)
        }
    }
}

fn activate_and_wait(bus: &Bus, connection: &str, device: &str, specific: &str) -> Result<()> {
    let active: OwnedObjectPath = bus.call_with(
        Opts::ACTION,
        NM,
        NM_PATH,
        NM,
        "ActivateConnection",
        &(object_path(connection)?, object_path(device)?, object_path(specific)?),
    )?;
    wait_active(bus, active.as_str(), device, JOIN_TIMEOUT)
}

/// Add a connection and bring it up; remove it again if it doesn't come up.
fn add_and_wait(bus: &Bus, settings: NewSettings<'_>, device: &str, specific: &str) -> Result<()> {
    let options: HashMap<&str, Value> = HashMap::from([("persist", Value::from("disk"))]);
    let added: Result<(OwnedObjectPath, OwnedObjectPath, HashMap<String, OwnedValue>)> = bus.call_with(
        Opts::ACTION,
        NM,
        NM_PATH,
        NM,
        "AddAndActivateConnection2",
        &(&settings, object_path(device)?, object_path(specific)?, options),
    );
    let (connection, active) = match added {
        Ok((connection, active, _)) => (connection, active),
        // NetworkManager before 1.16.
        Err(Error::NotFound(_)) => bus.call_with::<_, (OwnedObjectPath, OwnedObjectPath)>(
            Opts::ACTION,
            NM,
            NM_PATH,
            NM,
            "AddAndActivateConnection",
            &(&settings, object_path(device)?, object_path(specific)?),
        )?,
        Err(error) => return Err(error),
    };
    wait_active(bus, active.as_str(), device, JOIN_TIMEOUT).inspect_err(|_| {
        let _ = forget_paths(bus, &[connection.to_string()]);
    })
}

/// Wait for an active connection to come up; when it goes away instead,
/// say why, from its device's state reason.
fn wait_active(bus: &Bus, active: &str, device: &str, timeout: Duration) -> Result<()> {
    let mut devices: Vec<OwnedObjectPath> = bus.get(NM, active, IFACE_ACTIVE, "Devices").unwrap_or_default();
    if devices.is_empty()
        && let Ok(device) = OwnedObjectPath::try_from(device.to_owned())
        && device.as_str() != "/"
    {
        devices.push(device);
    }
    let start = Instant::now();
    let failed = |bus: &Bus| -> Error {
        let reason = devices
            .first()
            .and_then(|device| bus.get::<(u32, u32)>(NM, device.as_str(), IFACE_DEVICE, "StateReason").ok())
            .map(|(_, reason)| reason)
            .unwrap_or(0);
        if reason_is_password(reason) {
            Error::Rejected(describe_reason(reason).into())
        } else {
            Error::Failed(describe_reason(reason).into())
        }
    };
    loop {
        match bus.get::<u32>(NM, active, IFACE_ACTIVE, "State").map(ActiveState::from_u32) {
            Ok(ActiveState::Activated) => return Ok(()),
            Ok(ActiveState::Deactivated) | Err(Error::NotFound(_)) => return Err(failed(bus)),
            Ok(_) => {}
            Err(error) => return Err(error),
        }
        if start.elapsed() > timeout {
            return Err(Error::Timeout);
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn import_vpn(file: &Path, kind: Option<&str>) -> Result<String> {
    let kind = match kind {
        Some(kind) => kind.to_owned(),
        None => match file.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
            Some("conf") => "wireguard".into(),
            Some("ovpn") => "openvpn".into(),
            _ => return Err(Error::InvalidArgument("Unknown VPN file type: choose .conf or .ovpn".into())),
        },
    };
    if !file.is_file() {
        return Err(Error::NotFound(file.display().to_string()));
    }
    let out = std::process::Command::new("nmcli")
        .args(["connection", "import", "type", &kind, "file"])
        .arg(file)
        .env("LC_ALL", "C")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| Error::Unavailable(format!("nmcli: {e}")))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(Error::Command {
            program: "nmcli".into(),
            message: stderr.trim().trim_start_matches("Error: ").to_owned(),
        });
    }
    imported_uuid(&stdout).ok_or_else(|| Error::Failed(format!("nmcli said: {}", stdout.trim())))
}

/// The UUID in nmcli's "Connection 'wg0' (UUID) successfully added."
fn imported_uuid(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let start = line.rfind('(')?;
        let end = line[start..].find(')')? + start;
        let uuid = &line[start + 1..end];
        (uuid.len() == 36 && uuid.chars().all(|c| c.is_ascii_hexdigit() || c == '-')).then(|| uuid.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nmcli_import_reports_the_uuid() {
        assert_eq!(
            imported_uuid("Connection 'wg0' (2a5f0c4e-6c1e-4d3a-9d43-0c1f2b3a4d5e) successfully added.\n").as_deref(),
            Some("2a5f0c4e-6c1e-4d3a-9d43-0c1f2b3a4d5e")
        );
        assert_eq!(imported_uuid("Connection 'x (y)' added"), None);
    }
}
