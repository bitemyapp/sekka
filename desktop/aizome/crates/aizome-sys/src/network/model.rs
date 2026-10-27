//! What NetworkManager reports, as plain data, and how it is read from
//! NetworkManager's objects. Nothing here calls the bus: [`build`] turns the
//! cached objects and connection settings into a [`NetworkState`], so it is
//! tested with hand-made objects.

use crate::bus::{Objects, Props, PropsExt, value_bytes, value_dict, value_dicts, value_str, value_u64};
use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};
use zbus::zvariant::{OwnedValue, Value};

pub(crate) const NM: &str = "org.freedesktop.NetworkManager";
pub(crate) const NM_PATH: &str = "/org/freedesktop/NetworkManager";
pub(crate) const SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
pub(crate) const IFACE_DEVICE: &str = "org.freedesktop.NetworkManager.Device";
pub(crate) const IFACE_WIRED: &str = "org.freedesktop.NetworkManager.Device.Wired";
pub(crate) const IFACE_WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
pub(crate) const IFACE_AP: &str = "org.freedesktop.NetworkManager.AccessPoint";
pub(crate) const IFACE_ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
pub(crate) const IFACE_SETTINGS: &str = "org.freedesktop.NetworkManager.Settings";
pub(crate) const IFACE_CONNECTION: &str = "org.freedesktop.NetworkManager.Settings.Connection";
pub(crate) const IFACE_IP4: &str = "org.freedesktop.NetworkManager.IP4Config";
pub(crate) const IFACE_IP6: &str = "org.freedesktop.NetworkManager.IP6Config";

/// A connection's settings, as `GetSettings` returns them: setting name →
/// property → value.
pub type ConnectionSettings = HashMap<String, HashMap<String, OwnedValue>>;

/// Everything NetworkManager reports.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetworkState {
    /// NetworkManager is running.
    pub available: bool,
    pub version: String,
    pub state: NmState,
    pub connectivity: Connectivity,
    /// Networking as a whole (NetworkManager's "Enable Networking").
    pub networking_enabled: bool,
    /// The Wi-Fi switch.
    pub wifi_enabled: bool,
    /// False when a hardware switch or the firmware keeps Wi-Fi off.
    pub wifi_hardware_enabled: bool,
    /// The mobile broadband switch.
    pub wwan_enabled: bool,
    pub wwan_hardware_enabled: bool,
    /// Every radio is off: all rfkill switches blocked (Wi-Fi, Bluetooth,
    /// mobile), or, without rfkill, Wi-Fi and mobile off.
    pub airplane_mode: bool,
    /// The kernel's radio switches (`/dev/rfkill`).
    pub radios: Vec<Radio>,
    /// Whether the primary connection costs money per byte, as
    /// NetworkManager guesses or was told.
    pub metered: Metered,
    /// The active connection carrying the default route (its D-Bus path).
    pub primary_connection: Option<String>,
    /// The system hostname NetworkManager knows.
    pub hostname: String,
    /// Whether the person may change connections (`Settings.CanModify`).
    pub can_modify: bool,
    pub devices: Vec<Device>,
    /// Every access point in range, one per BSSID and device.
    pub access_points: Vec<AccessPoint>,
    /// Wi-Fi networks in range, one per name: connected first, then by
    /// strength.
    pub networks: Vec<WifiNetwork>,
    /// Saved connections, most recently used first.
    pub connections: Vec<SavedConnection>,
    pub active: Vec<ActiveConnection>,
}

impl NetworkState {
    /// The first managed Wi-Fi device.
    pub fn wifi_device(&self) -> Option<&Device> {
        self.devices.iter().find(|d| d.kind == DeviceKind::Wifi && d.managed)
    }

    /// Managed wired devices.
    pub fn wired_devices(&self) -> impl Iterator<Item = &Device> {
        self.devices.iter().filter(|d| d.kind == DeviceKind::Ethernet && d.managed)
    }

    pub fn device(&self, path: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.path == path)
    }

    pub fn device_by_interface(&self, interface: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.interface == interface)
    }

    pub fn connection(&self, uuid: &str) -> Option<&SavedConnection> {
        self.connections.iter().find(|c| c.uuid == uuid)
    }

    pub fn network(&self, ssid: &str) -> Option<&WifiNetwork> {
        self.networks.iter().find(|n| n.ssid == ssid)
    }

    /// The saved Wi-Fi connection for a network name, the active or most
    /// recently used one first.
    pub fn saved_for(&self, ssid: &str) -> Option<&SavedConnection> {
        self.connections
            .iter()
            .filter(|c| c.kind == ConnectionKind::Wifi && c.ssid.as_deref() == Some(ssid))
            .max_by_key(|c| (c.active, c.timestamp))
    }

    /// The active connection with this UUID.
    pub fn active_connection(&self, uuid: &str) -> Option<&ActiveConnection> {
        self.active.iter().find(|a| a.uuid == uuid)
    }

    /// The Wi-Fi network the Wi-Fi device is connected to.
    pub fn current_wifi(&self) -> Option<&WifiNetwork> {
        self.networks.iter().find(|n| n.connected)
    }

    /// The primary connection.
    pub fn primary(&self) -> Option<&ActiveConnection> {
        let path = self.primary_connection.as_deref()?;
        self.active.iter().find(|a| a.path == path)
    }

    /// The details of a device's connection: addresses, router, DNS,
    /// hardware address and speed.
    pub fn details(&self, device_path: &str) -> Option<Details> {
        let device = self.device(device_path)?;
        let active = device.active_connection.as_deref().and_then(|path| self.active.iter().find(|a| a.path == path));
        Some(Details {
            interface: device.interface.clone(),
            connection: active.map(|a| a.id.clone()),
            hardware_address: device.hw_address.clone(),
            ipv4: device.ip4.clone(),
            ipv6: device.ip6.clone(),
            speed_mbps: match device.kind {
                DeviceKind::Wifi => device.bitrate_kbps.map(|kbps| kbps / 1000),
                _ => device.speed_mbps,
            },
            frequency_mhz: (device.kind == DeviceKind::Wifi)
                .then(|| {
                    let ap = device.active_access_point.as_deref()?;
                    self.access_points.iter().find(|a| a.path == ap).map(|a| a.frequency)
                })
                .flatten(),
            metered: device.metered,
            proxy: active.and_then(|a| self.connection(&a.uuid)).map(|c| c.proxy.clone()).unwrap_or_default(),
        })
    }

    /// The details of the primary connection's first device.
    pub fn primary_details(&self) -> Option<Details> {
        let primary = self.primary()?;
        self.details(primary.devices.first()?)
    }

    /// Saved VPN and WireGuard connections.
    pub fn vpns(&self) -> impl Iterator<Item = &SavedConnection> {
        self.connections.iter().filter(|c| c.kind.is_vpn())
    }

    /// Saved Wi-Fi connections, most recently used first.
    pub fn saved_wifi(&self) -> impl Iterator<Item = &SavedConnection> {
        self.connections.iter().filter(|c| c.kind == ConnectionKind::Wifi)
    }

    /// The hotspot this machine is running, if any.
    pub fn hotspot(&self) -> Option<&ActiveConnection> {
        self.active.iter().find(|a| self.connection(&a.uuid).is_some_and(|c| c.wifi_mode.as_deref() == Some("ap")))
    }
}

/// NetworkManager's overall state (`NMState`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NmState {
    #[default]
    Unknown,
    Asleep,
    Disconnected,
    Disconnecting,
    Connecting,
    ConnectedLocal,
    ConnectedSite,
    ConnectedGlobal,
}

impl NmState {
    pub fn from_u32(n: u32) -> NmState {
        match n {
            10 => NmState::Asleep,
            20 => NmState::Disconnected,
            30 => NmState::Disconnecting,
            40 => NmState::Connecting,
            50 => NmState::ConnectedLocal,
            60 => NmState::ConnectedSite,
            70 => NmState::ConnectedGlobal,
            _ => NmState::Unknown,
        }
    }

    pub fn connected(self) -> bool {
        matches!(self, NmState::ConnectedLocal | NmState::ConnectedSite | NmState::ConnectedGlobal)
    }
}

/// Whether the internet is reachable (`NMConnectivityState`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Connectivity {
    #[default]
    Unknown,
    None,
    /// A captive portal wants a login.
    Portal,
    Limited,
    Full,
}

impl Connectivity {
    pub fn from_u32(n: u32) -> Connectivity {
        match n {
            1 => Connectivity::None,
            2 => Connectivity::Portal,
            3 => Connectivity::Limited,
            4 => Connectivity::Full,
            _ => Connectivity::Unknown,
        }
    }
}

/// Whether a connection is metered (`NMMetered`); the guesses come from
/// NetworkManager (a phone's hotspot, mobile broadband).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Metered {
    #[default]
    Unknown,
    Yes,
    No,
    GuessYes,
    GuessNo,
}

impl Metered {
    pub fn from_u32(n: u32) -> Metered {
        match n {
            1 => Metered::Yes,
            2 => Metered::No,
            3 => Metered::GuessYes,
            4 => Metered::GuessNo,
            _ => Metered::Unknown,
        }
    }

    pub fn is_metered(self) -> bool {
        matches!(self, Metered::Yes | Metered::GuessYes)
    }

    /// The `connection.metered` setting value: 0 automatic, 1 yes, 2 no.
    pub(crate) fn setting(self) -> i32 {
        match self {
            Metered::Yes | Metered::GuessYes => 1,
            Metered::No | Metered::GuessNo => 2,
            Metered::Unknown => 0,
        }
    }
}

/// A network device.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Device {
    /// The D-Bus object path, used to act on it.
    pub path: String,
    pub interface: String,
    pub kind: DeviceKind,
    pub state: DeviceState,
    /// Why the device is in its state (`NMDeviceStateReason`); see
    /// [`describe_reason`].
    pub state_reason: u32,
    /// NetworkManager manages it (unmanaged devices are left to others).
    pub managed: bool,
    pub autoconnect: bool,
    pub driver: String,
    pub hw_address: String,
    pub perm_hw_address: String,
    /// Wired link speed in Mb/s.
    pub speed_mbps: Option<u32>,
    /// Whether a cable is plugged in (wired).
    pub carrier: Option<bool>,
    /// Wi-Fi bit rate in kb/s.
    pub bitrate_kbps: Option<u32>,
    pub active_connection: Option<String>,
    pub active_access_point: Option<String>,
    pub ip4: IpConfig,
    pub ip6: IpConfig,
    pub mtu: u32,
    pub metered: Metered,
    /// When the last Wi-Fi scan finished, in ms of `CLOCK_BOOTTIME`.
    pub last_scan_ms: Option<i64>,
}

impl Device {
    pub fn connected(&self) -> bool {
        self.state == DeviceState::Activated
    }

    pub fn connecting(&self) -> bool {
        self.state.connecting()
    }
}

/// The kind of a device (`NMDeviceType`), the common ones named.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeviceKind {
    Ethernet,
    Wifi,
    Bluetooth,
    Modem,
    Bridge,
    Bond,
    Vlan,
    Tun,
    WireGuard,
    WifiP2p,
    Loopback,
    Veth,
    #[default]
    Unknown,
    Other(u32),
}

impl DeviceKind {
    pub fn from_u32(n: u32) -> DeviceKind {
        match n {
            0 => DeviceKind::Unknown,
            1 => DeviceKind::Ethernet,
            2 => DeviceKind::Wifi,
            5 => DeviceKind::Bluetooth,
            8 => DeviceKind::Modem,
            10 => DeviceKind::Bond,
            11 => DeviceKind::Vlan,
            13 => DeviceKind::Bridge,
            16 => DeviceKind::Tun,
            20 => DeviceKind::Veth,
            29 => DeviceKind::WireGuard,
            30 => DeviceKind::WifiP2p,
            32 => DeviceKind::Loopback,
            n => DeviceKind::Other(n),
        }
    }
}

/// A device's state (`NMDeviceState`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeviceState {
    #[default]
    Unknown,
    Unmanaged,
    /// No cable, or Wi-Fi off.
    Unavailable,
    Disconnected,
    Prepare,
    Config,
    NeedAuth,
    IpConfig,
    IpCheck,
    Secondaries,
    Activated,
    Deactivating,
    Failed,
}

impl DeviceState {
    pub fn from_u32(n: u32) -> DeviceState {
        match n {
            10 => DeviceState::Unmanaged,
            20 => DeviceState::Unavailable,
            30 => DeviceState::Disconnected,
            40 => DeviceState::Prepare,
            50 => DeviceState::Config,
            60 => DeviceState::NeedAuth,
            70 => DeviceState::IpConfig,
            80 => DeviceState::IpCheck,
            90 => DeviceState::Secondaries,
            100 => DeviceState::Activated,
            110 => DeviceState::Deactivating,
            120 => DeviceState::Failed,
            _ => DeviceState::Unknown,
        }
    }

    pub fn connecting(self) -> bool {
        matches!(
            self,
            DeviceState::Prepare
                | DeviceState::Config
                | DeviceState::NeedAuth
                | DeviceState::IpConfig
                | DeviceState::IpCheck
                | DeviceState::Secondaries
        )
    }
}

/// Addresses of a device on one IP version.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IpConfig {
    /// Addresses with their prefix length, as `192.168.1.20/24`.
    pub addresses: Vec<String>,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
    pub domains: Vec<String>,
}

impl IpConfig {
    /// The first address without its prefix length.
    pub fn address(&self) -> Option<&str> {
        self.addresses.first().map(|a| a.split('/').next().unwrap_or(a))
    }
}

/// What the details page of a connection shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Details {
    pub interface: String,
    /// The connection's name.
    pub connection: Option<String>,
    pub hardware_address: String,
    pub ipv4: IpConfig,
    pub ipv6: IpConfig,
    /// Link speed (wired) or bit rate (Wi-Fi), in Mb/s.
    pub speed_mbps: Option<u32>,
    pub frequency_mhz: Option<u32>,
    pub metered: Metered,
    pub proxy: Proxy,
}

/// A Wi-Fi access point.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccessPoint {
    pub path: String,
    /// The device that sees it.
    pub device: String,
    pub ssid: String,
    /// The SSID's bytes, which needn't be UTF-8.
    pub ssid_bytes: Vec<u8>,
    pub bssid: String,
    /// In MHz.
    pub frequency: u32,
    pub band: Band,
    /// 0–100.
    pub strength: u8,
    pub security: Security,
    pub max_bitrate_kbps: u32,
    /// The access point is in infrastructure mode (an ordinary network).
    pub infrastructure: bool,
}

/// A Wi-Fi band.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Band {
    TwoPointFourGhz,
    FiveGhz,
    SixGhz,
    SixtyGhz,
    #[default]
    Other,
}

impl Band {
    pub fn from_mhz(mhz: u32) -> Band {
        match mhz {
            2400..=2500 => Band::TwoPointFourGhz,
            4900..=5900 => Band::FiveGhz,
            5925..=7125 => Band::SixGhz,
            57000..=71000 => Band::SixtyGhz,
            _ => Band::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Band::TwoPointFourGhz => "2.4 GHz",
            Band::FiveGhz => "5 GHz",
            Band::SixGhz => "6 GHz",
            Band::SixtyGhz => "60 GHz",
            Band::Other => "",
        }
    }
}

/// How a Wi-Fi network is secured, from what its access points advertise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Security {
    #[default]
    Open,
    /// Opportunistic Wireless Encryption: no password, but encrypted.
    EnhancedOpen,
    /// WEP: broken, joined only on request.
    Wep,
    /// WPA (version 1) with a password.
    WpaPersonal,
    /// WPA2 with a password (also WPA/WPA2 mixed).
    Wpa2Personal,
    /// WPA2/WPA3 transition mode: either works.
    Wpa2Wpa3Personal,
    /// WPA3 (SAE) only.
    Wpa3Personal,
    /// 802.1X: a username and password from an organization.
    Enterprise,
}

impl Security {
    // NM80211ApSecurityFlags.
    const KEY_MGMT_PSK: u32 = 0x100;
    const KEY_MGMT_802_1X: u32 = 0x200;
    const KEY_MGMT_SAE: u32 = 0x400;
    const KEY_MGMT_OWE: u32 = 0x800;
    const KEY_MGMT_OWE_TM: u32 = 0x1000;
    const KEY_MGMT_EAP_SUITE_B_192: u32 = 0x2000;
    // NM80211ApFlags.
    const PRIVACY: u32 = 0x1;

    /// From an access point's `Flags`, `WpaFlags` and `RsnFlags`.
    pub fn from_flags(flags: u32, wpa: u32, rsn: u32) -> Security {
        let both = wpa | rsn;
        if both & (Self::KEY_MGMT_802_1X | Self::KEY_MGMT_EAP_SUITE_B_192) != 0 {
            Security::Enterprise
        } else if rsn & Self::KEY_MGMT_SAE != 0 {
            if both & Self::KEY_MGMT_PSK != 0 { Security::Wpa2Wpa3Personal } else { Security::Wpa3Personal }
        } else if rsn & Self::KEY_MGMT_PSK != 0 {
            Security::Wpa2Personal
        } else if wpa & Self::KEY_MGMT_PSK != 0 {
            Security::WpaPersonal
        } else if rsn & (Self::KEY_MGMT_OWE | Self::KEY_MGMT_OWE_TM) != 0 {
            Security::EnhancedOpen
        } else if flags & Self::PRIVACY != 0 {
            Security::Wep
        } else {
            Security::Open
        }
    }

    /// From a saved connection's `802-11-wireless-security.key-mgmt`.
    pub fn from_key_mgmt(key_mgmt: Option<&str>) -> Security {
        match key_mgmt {
            None | Some("") => Security::Open,
            Some("owe") => Security::EnhancedOpen,
            Some("none") | Some("ieee8021x") => Security::Wep,
            Some("wpa-psk") => Security::Wpa2Personal,
            Some("sae") => Security::Wpa3Personal,
            Some(_) => Security::Enterprise,
        }
    }

    /// Whether joining asks for a password (or a username and password).
    pub fn needs_password(self) -> bool {
        !matches!(self, Security::Open | Security::EnhancedOpen)
    }

    pub fn is_enterprise(self) -> bool {
        self == Security::Enterprise
    }

    /// NetworkManager's `key-mgmt` for a new connection; `None` for open
    /// networks, which have no security setting.
    pub fn key_mgmt(self) -> Option<&'static str> {
        match self {
            Security::Open => None,
            Security::EnhancedOpen => Some("owe"),
            Security::Wep => Some("none"),
            Security::WpaPersonal | Security::Wpa2Personal | Security::Wpa2Wpa3Personal => Some("wpa-psk"),
            Security::Wpa3Personal => Some("sae"),
            Security::Enterprise => Some("wpa-eap"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Security::Open => "Open",
            Security::EnhancedOpen => "Enhanced Open",
            Security::Wep => "WEP",
            Security::WpaPersonal => "WPA",
            Security::Wpa2Personal => "WPA2",
            Security::Wpa2Wpa3Personal => "WPA2/WPA3",
            Security::Wpa3Personal => "WPA3",
            Security::Enterprise => "Enterprise",
        }
    }
}

/// A Wi-Fi network in range: its access points grouped by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WifiNetwork {
    pub ssid: String,
    pub ssid_bytes: Vec<u8>,
    /// The strongest access point's strength, 0–100.
    pub strength: u8,
    pub security: Security,
    /// Bands its access points use, lowest first.
    pub bands: Vec<Band>,
    /// The strongest access point (its path), to connect through.
    pub best_access_point: String,
    pub access_points: Vec<String>,
    /// The device that sees it best.
    pub device: String,
    /// A saved connection exists for it.
    pub known: bool,
    /// The saved connection's UUID (the active or most recently used one).
    pub connection: Option<String>,
    /// It is the network in use.
    pub connected: bool,
    /// Joining it is in progress.
    pub connecting: bool,
}

impl WifiNetwork {
    /// Signal strength in four steps, as icons show it.
    pub fn bars(&self) -> u8 {
        bars(self.strength)
    }
}

/// Signal strength in four steps (1–4).
pub fn bars(strength: u8) -> u8 {
    match strength {
        75.. => 4,
        50..=74 => 3,
        25..=49 => 2,
        _ => 1,
    }
}

/// The symbolic icon name for a signal strength.
pub fn signal_icon(strength: u8) -> &'static str {
    match bars(strength) {
        4 => "network-wireless-signal-excellent-symbolic",
        3 => "network-wireless-signal-good-symbolic",
        2 => "network-wireless-signal-ok-symbolic",
        _ => "network-wireless-signal-weak-symbolic",
    }
}

/// A saved connection profile.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavedConnection {
    pub path: String,
    pub uuid: String,
    /// Its name.
    pub id: String,
    pub kind: ConnectionKind,
    pub autoconnect: bool,
    /// When it was last connected (seconds since 1970); 0 never.
    pub timestamp: u64,
    /// The network name (Wi-Fi).
    pub ssid: Option<String>,
    /// `802-11-wireless-security.key-mgmt` (Wi-Fi).
    pub key_mgmt: Option<String>,
    pub hidden: bool,
    /// `802-11-wireless.mode`: `infrastructure`, `ap` (a hotspot), `adhoc`.
    pub wifi_mode: Option<String>,
    /// The `connection.metered` setting.
    pub metered: Metered,
    /// The interface it is bound to, if any.
    pub interface_name: Option<String>,
    /// A VPN plugin's service type (`org.freedesktop.NetworkManager.openvpn`).
    pub vpn_service: Option<String>,
    pub proxy: Proxy,
    /// It is active (or activating) now.
    pub active: bool,
    /// Changes are in memory only.
    pub unsaved: bool,
    /// The file it is stored in.
    pub filename: String,
}

/// The kind of a saved connection (`connection.type`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ConnectionKind {
    Wifi,
    Ethernet,
    /// A VPN through a NetworkManager plugin (OpenVPN, OpenConnect…).
    Vpn,
    WireGuard,
    Bluetooth,
    Mobile,
    Bridge,
    Bond,
    Vlan,
    Loopback,
    #[default]
    Unknown,
    Other(String),
}

impl ConnectionKind {
    pub fn from_type(kind: &str) -> ConnectionKind {
        match kind {
            "802-11-wireless" | "wifi" => ConnectionKind::Wifi,
            "802-3-ethernet" | "ethernet" => ConnectionKind::Ethernet,
            "vpn" => ConnectionKind::Vpn,
            "wireguard" => ConnectionKind::WireGuard,
            "bluetooth" => ConnectionKind::Bluetooth,
            "gsm" | "cdma" => ConnectionKind::Mobile,
            "bridge" => ConnectionKind::Bridge,
            "bond" => ConnectionKind::Bond,
            "vlan" => ConnectionKind::Vlan,
            "loopback" => ConnectionKind::Loopback,
            "" => ConnectionKind::Unknown,
            other => ConnectionKind::Other(other.to_owned()),
        }
    }

    pub fn is_vpn(&self) -> bool {
        matches!(self, ConnectionKind::Vpn | ConnectionKind::WireGuard)
    }
}

/// A connection's proxy setting (read only).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Proxy {
    /// Use a PAC file (`auto`) rather than none.
    pub automatic: bool,
    pub pac_url: Option<String>,
    /// Only browsers are meant to use it.
    pub browser_only: bool,
}

/// An active (or activating) connection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActiveConnection {
    pub path: String,
    /// The saved connection's path.
    pub connection: String,
    pub uuid: String,
    pub id: String,
    /// `connection.type`.
    pub kind: String,
    pub state: ActiveState,
    pub devices: Vec<String>,
    /// It carries the IPv4 default route.
    pub default4: bool,
    pub default6: bool,
    pub vpn: bool,
    /// The access point (Wi-Fi) it was activated on.
    pub specific_object: Option<String>,
    pub ip4: IpConfig,
    pub ip6: IpConfig,
}

/// An active connection's state (`NMActiveConnectionState`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ActiveState {
    #[default]
    Unknown,
    Activating,
    Activated,
    Deactivating,
    Deactivated,
}

impl ActiveState {
    pub fn from_u32(n: u32) -> ActiveState {
        match n {
            1 => ActiveState::Activating,
            2 => ActiveState::Activated,
            3 => ActiveState::Deactivating,
            4 => ActiveState::Deactivated,
            _ => ActiveState::Unknown,
        }
    }
}

/// A kernel radio switch, from `/dev/rfkill`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Radio {
    pub index: u32,
    pub kind: RadioKind,
    /// Switched off in software (airplane mode, `rfkill block`).
    pub soft_blocked: bool,
    /// Switched off by a hardware switch or the firmware.
    pub hard_blocked: bool,
}

/// What an rfkill switch controls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RadioKind {
    Wlan,
    Bluetooth,
    Uwb,
    Wimax,
    Wwan,
    Gps,
    Fm,
    Nfc,
    #[default]
    Other,
}

impl RadioKind {
    pub(crate) fn from_u8(n: u8) -> RadioKind {
        match n {
            1 => RadioKind::Wlan,
            2 => RadioKind::Bluetooth,
            3 => RadioKind::Uwb,
            4 => RadioKind::Wimax,
            5 => RadioKind::Wwan,
            6 => RadioKind::Gps,
            7 => RadioKind::Fm,
            8 => RadioKind::Nfc,
            _ => RadioKind::Other,
        }
    }
}

/// What a device-state reason means, for the person (`NMDeviceStateReason`).
pub fn describe_reason(reason: u32) -> &'static str {
    match reason {
        7..=11 => "The password was not accepted",
        5 => "Could not get an address",
        16 | 17 => "The network did not answer",
        40 => "The cable is unplugged",
        53 => "The network is out of range",
        _ => "Could not connect",
    }
}

/// Whether a device-state reason means the password was wrong or missing.
pub fn reason_is_password(reason: u32) -> bool {
    // NO_SECRETS, SUPPLICANT_DISCONNECT, SUPPLICANT_CONFIG_FAILED,
    // SUPPLICANT_FAILED, SUPPLICANT_TIMEOUT.
    matches!(reason, 7..=11)
}

/// Build the state from NetworkManager's objects and the saved connections'
/// settings (by object path).
pub(crate) fn build(
    objects: &Objects,
    settings: &HashMap<String, ConnectionSettings>,
    radios: &[Radio],
) -> NetworkState {
    let empty = Props::new();
    let manager = objects.get(NM_PATH, NM).unwrap_or(&empty);
    let settings_root = objects.get(SETTINGS_PATH, IFACE_SETTINGS).unwrap_or(&empty);

    let mut devices: Vec<Device> = objects
        .with(IFACE_DEVICE)
        .filter(|(_, props)| props.opt_bool("Real").unwrap_or(true))
        .map(|(path, props)| device(objects, path, props))
        .collect();
    devices.sort_by(|a, b| a.interface.cmp(&b.interface));

    let active: Vec<ActiveConnection> = objects
        .with(IFACE_ACTIVE)
        .map(|(path, props)| ActiveConnection {
            path: path.to_owned(),
            connection: props.path("Connection").unwrap_or_default(),
            uuid: props.str("Uuid"),
            id: props.str("Id"),
            kind: props.str("Type"),
            state: ActiveState::from_u32(props.u32("State")),
            devices: props.strings("Devices"),
            default4: props.bool("Default"),
            default6: props.bool("Default6"),
            vpn: props.bool("Vpn"),
            specific_object: props.path("SpecificObject"),
            ip4: ip_config(objects, props.path("Ip4Config"), false),
            ip6: ip_config(objects, props.path("Ip6Config"), true),
        })
        .collect();

    let mut connections: Vec<SavedConnection> = settings
        .iter()
        .map(|(path, settings)| {
            let props = objects.get(path, IFACE_CONNECTION).unwrap_or(&empty);
            saved_connection(path, settings, props, &active)
        })
        .collect();
    connections.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| a.id.cmp(&b.id)));

    let access_points = access_points(objects, &devices);
    let networks = networks(&access_points, &devices, &connections, &active);

    let wifi_enabled = manager.bool("WirelessEnabled");
    let wwan_enabled = manager.bool("WwanEnabled");
    let airplane_mode = if radios.is_empty() {
        !wifi_enabled && !wwan_enabled
    } else {
        radios.iter().all(|r| r.soft_blocked || r.hard_blocked)
    };

    NetworkState {
        available: true,
        version: manager.str("Version"),
        state: NmState::from_u32(manager.u32("State")),
        connectivity: Connectivity::from_u32(manager.u32("Connectivity")),
        networking_enabled: manager.bool("NetworkingEnabled"),
        wifi_enabled,
        wifi_hardware_enabled: manager.bool("WirelessHardwareEnabled"),
        wwan_enabled,
        wwan_hardware_enabled: manager.bool("WwanHardwareEnabled"),
        airplane_mode,
        radios: radios.to_vec(),
        metered: Metered::from_u32(manager.u32("Metered")),
        primary_connection: manager.path("PrimaryConnection"),
        hostname: settings_root.str("Hostname"),
        can_modify: settings_root.opt_bool("CanModify").unwrap_or(true),
        devices,
        access_points,
        networks,
        connections,
        active,
    }
}

fn device(objects: &Objects, path: &str, props: &Props) -> Device {
    let wired = objects.get(path, IFACE_WIRED);
    let wireless = objects.get(path, IFACE_WIRELESS);
    let kind = DeviceKind::from_u32(props.u32("DeviceType"));
    let state_reason = props
        .value("StateReason")
        .and_then(|value| match value {
            Value::Structure(s) => s.fields().get(1).and_then(value_u64),
            _ => None,
        })
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0);
    let hw_address = props
        .opt_str("HwAddress")
        .filter(|a| !a.is_empty())
        .or_else(|| wired.map(|w| w.str("HwAddress")))
        .or_else(|| wireless.map(|w| w.str("HwAddress")))
        .unwrap_or_default();
    let perm_hw_address = wired.or(wireless).map(|p| p.str("PermHwAddress")).unwrap_or_default();
    Device {
        path: path.to_owned(),
        interface: props.str("Interface"),
        kind,
        state: DeviceState::from_u32(props.u32("State")),
        state_reason,
        managed: props.bool("Managed"),
        autoconnect: props.bool("Autoconnect"),
        driver: props.str("Driver"),
        hw_address,
        perm_hw_address,
        speed_mbps: wired.map(|w| w.u32("Speed")).filter(|&s| s > 0),
        carrier: wired.and_then(|w| w.opt_bool("Carrier")),
        bitrate_kbps: wireless.map(|w| w.u32("Bitrate")).filter(|&b| b > 0),
        active_connection: props.path("ActiveConnection"),
        active_access_point: wireless.and_then(|w| w.path("ActiveAccessPoint")),
        ip4: ip_config(objects, props.path("Ip4Config"), false),
        ip6: ip_config(objects, props.path("Ip6Config"), true),
        mtu: props.u32("Mtu"),
        metered: Metered::from_u32(props.u32("Metered")),
        last_scan_ms: wireless.and_then(|w| w.opt_i64("LastScan")).filter(|&t| t >= 0),
    }
}

/// An IP4Config or IP6Config object, read with the current properties
/// (`AddressData`, `NameserverData`) and the older ones as a fallback.
fn ip_config(objects: &Objects, path: Option<String>, v6: bool) -> IpConfig {
    let Some(path) = path else { return IpConfig::default() };
    let Some(props) = objects.get(&path, if v6 { IFACE_IP6 } else { IFACE_IP4 }) else {
        return IpConfig::default();
    };
    let addresses = props
        .value("AddressData")
        .map(value_dicts)
        .unwrap_or_default()
        .iter()
        .filter_map(|data| {
            let address = data.opt_str("address")?;
            Some(format!("{address}/{}", data.u32("prefix")))
        })
        .collect();
    let mut dns: Vec<String> = props
        .value("NameserverData")
        .map(value_dicts)
        .unwrap_or_default()
        .iter()
        .filter_map(|data| data.opt_str("address"))
        .collect();
    if dns.is_empty() {
        dns = match props.value("Nameservers") {
            Some(Value::Array(array)) if v6 => array
                .iter()
                .filter_map(|bytes| {
                    let bytes: [u8; 16] = value_bytes(bytes).try_into().ok()?;
                    Some(Ipv6Addr::from(bytes).to_string())
                })
                .collect(),
            Some(Value::Array(array)) => array
                .iter()
                .filter_map(|n| {
                    let n = u32::try_from(value_u64(n)?).ok()?;
                    // Network byte order in a host integer.
                    Some(Ipv4Addr::from(n.to_ne_bytes()).to_string())
                })
                .collect(),
            _ => Vec::new(),
        };
    }
    IpConfig {
        addresses,
        gateway: props.opt_str("Gateway").filter(|g| !g.is_empty()),
        dns,
        domains: props.strings("Domains"),
    }
}

fn setting<'a>(settings: &'a ConnectionSettings, name: &str) -> Option<&'a Props> {
    settings.get(name)
}

fn saved_connection(
    path: &str,
    settings: &ConnectionSettings,
    props: &Props,
    active: &[ActiveConnection],
) -> SavedConnection {
    let empty = Props::new();
    let connection = setting(settings, "connection").unwrap_or(&empty);
    let wireless = setting(settings, "802-11-wireless");
    let security = setting(settings, "802-11-wireless-security");
    let proxy = setting(settings, "proxy");
    let uuid = connection.str("uuid");
    let is_active =
        active.iter().any(|a| a.uuid == uuid && matches!(a.state, ActiveState::Activating | ActiveState::Activated));
    SavedConnection {
        path: path.to_owned(),
        id: connection.str("id"),
        kind: ConnectionKind::from_type(&connection.str("type")),
        autoconnect: connection.opt_bool("autoconnect").unwrap_or(true),
        timestamp: connection.u64("timestamp"),
        ssid: wireless.map(|w| w.bytestring("ssid")).filter(|s| !s.is_empty()),
        key_mgmt: security.and_then(|s| s.opt_str("key-mgmt")),
        hidden: wireless.is_some_and(|w| w.bool("hidden")),
        wifi_mode: wireless.map(|w| w.opt_str("mode").unwrap_or_else(|| "infrastructure".into())),
        metered: match connection.i64("metered") {
            1 => Metered::Yes,
            2 => Metered::No,
            _ => Metered::Unknown,
        },
        interface_name: connection.opt_str("interface-name").filter(|i| !i.is_empty()),
        vpn_service: setting(settings, "vpn").and_then(|v| v.opt_str("service-type")),
        proxy: Proxy {
            automatic: proxy.is_some_and(|p| p.i64("method") == 1),
            pac_url: proxy.and_then(|p| p.opt_str("pac-url")).filter(|u| !u.is_empty()),
            browser_only: proxy.is_some_and(|p| p.bool("browser-only")),
        },
        active: is_active,
        unsaved: props.bool("Unsaved"),
        filename: props.str("Filename"),
        uuid,
    }
}

fn access_points(objects: &Objects, devices: &[Device]) -> Vec<AccessPoint> {
    let mut found = Vec::new();
    for device in devices.iter().filter(|d| d.kind == DeviceKind::Wifi) {
        let Some(wireless) = objects.get(&device.path, IFACE_WIRELESS) else { continue };
        for path in wireless.strings("AccessPoints") {
            let Some(props) = objects.get(&path, IFACE_AP) else { continue };
            let ssid_bytes = props.bytes("Ssid");
            let frequency = props.u32("Frequency");
            found.push(AccessPoint {
                ssid: ssid_name(&ssid_bytes),
                ssid_bytes,
                bssid: props.str("HwAddress"),
                frequency,
                band: Band::from_mhz(frequency),
                strength: u8::try_from(props.u64("Strength").min(100)).unwrap_or(0),
                security: Security::from_flags(props.u32("Flags"), props.u32("WpaFlags"), props.u32("RsnFlags")),
                max_bitrate_kbps: props.u32("MaxBitrate"),
                infrastructure: props.u32("Mode") == 2,
                device: device.path.clone(),
                path,
            });
        }
    }
    found
}

/// An SSID for display: UTF-8 when it is, otherwise escaped bytes.
pub fn ssid_name(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.trim_end_matches('\0').to_owned(),
        Err(_) => bytes
            .iter()
            .map(|&b| if b.is_ascii_graphic() || b == b' ' { (b as char).to_string() } else { format!("\\x{b:02x}") })
            .collect(),
    }
}

fn networks(
    access_points: &[AccessPoint],
    devices: &[Device],
    connections: &[SavedConnection],
    active: &[ActiveConnection],
) -> Vec<WifiNetwork> {
    let mut networks: Vec<WifiNetwork> = Vec::new();
    for ap in access_points.iter().filter(|ap| !ap.ssid_bytes.is_empty() && ap.infrastructure) {
        match networks.iter_mut().find(|n| n.ssid_bytes == ap.ssid_bytes) {
            Some(network) => {
                network.access_points.push(ap.path.clone());
                if !network.bands.contains(&ap.band) {
                    network.bands.push(ap.band);
                }
                if ap.strength > network.strength {
                    network.strength = ap.strength;
                    network.best_access_point = ap.path.clone();
                    network.device = ap.device.clone();
                    network.security = ap.security;
                }
            }
            None => networks.push(WifiNetwork {
                ssid: ap.ssid.clone(),
                ssid_bytes: ap.ssid_bytes.clone(),
                strength: ap.strength,
                security: ap.security,
                bands: vec![ap.band],
                best_access_point: ap.path.clone(),
                access_points: vec![ap.path.clone()],
                device: ap.device.clone(),
                ..WifiNetwork::default()
            }),
        }
    }
    for network in &mut networks {
        network.bands.sort();
        let saved = connections
            .iter()
            .filter(|c| {
                c.kind == ConnectionKind::Wifi
                    && c.ssid.as_deref() == Some(network.ssid.as_str())
                    && c.wifi_mode.as_deref() != Some("ap")
            })
            .max_by_key(|c| (c.active, c.timestamp));
        network.known = saved.is_some();
        network.connection = saved.map(|c| c.uuid.clone());
        // In use: a Wi-Fi device's active access point belongs to it, or
        // an active connection for it is up.
        let on_device = devices.iter().any(|d| {
            d.kind == DeviceKind::Wifi
                && d.connected()
                && d.active_access_point.as_ref().is_some_and(|ap| network.access_points.contains(ap))
        });
        let mine: Vec<&ActiveConnection> = active
            .iter()
            .filter(|a| {
                connections.iter().any(|c| {
                    c.uuid == a.uuid
                        && c.kind == ConnectionKind::Wifi
                        && c.ssid.as_deref() == Some(network.ssid.as_str())
                        && c.wifi_mode.as_deref() != Some("ap")
                })
            })
            .collect();
        network.connected = on_device || mine.iter().any(|a| a.state == ActiveState::Activated);
        network.connecting = !network.connected && mine.iter().any(|a| a.state == ActiveState::Activating);
    }
    networks.sort_by(|a, b| {
        b.connected.cmp(&a.connected).then(b.strength.cmp(&a.strength)).then_with(|| a.ssid.cmp(&b.ssid))
    });
    networks
}

/// Settings for a new connection, to send to NetworkManager.
pub(crate) type NewSettings<'a> = HashMap<&'static str, HashMap<&'static str, Value<'a>>>;

/// Credentials for an enterprise (802.1X) network.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Enterprise {
    /// The outer method: `peap` or `ttls` (`tls` needs certificates and is
    /// left to the configuration).
    pub method: String,
    /// The inner authentication: `mschapv2`, `pap`, `gtc`…
    pub phase2: String,
    pub identity: String,
    pub anonymous_identity: String,
    pub password: String,
    /// The server's domain, checked against its certificate.
    pub domain: String,
    /// Check the server against the system's certificate authorities.
    pub validate_server: bool,
}

/// Settings for joining a Wi-Fi network. Secrets go into the profile with
/// no agent flags, so NetworkManager keeps them (root-only) and needs no
/// secret agent.
pub(crate) fn wifi_settings<'a>(
    ssid: &[u8],
    security: Security,
    password: Option<&'a str>,
    enterprise: Option<&'a Enterprise>,
    hidden: bool,
) -> NewSettings<'a> {
    let mut settings = NewSettings::new();
    let name = ssid_name(ssid);
    settings.insert("connection", HashMap::from([("id", Value::from(name)), ("type", Value::from("802-11-wireless"))]));
    let mut wireless = HashMap::from([("ssid", Value::from(ssid.to_vec())), ("mode", Value::from("infrastructure"))]);
    if hidden {
        wireless.insert("hidden", Value::from(true));
    }
    settings.insert("802-11-wireless", wireless);
    if let Some(key_mgmt) = security.key_mgmt() {
        let mut sec = HashMap::from([("key-mgmt", Value::from(key_mgmt))]);
        match security {
            Security::Wep => {
                sec.insert("wep-key0", Value::from(password.unwrap_or_default()));
                // 1: a key (hex or ASCII), 2: a passphrase.
                sec.insert("wep-key-type", Value::from(1u32));
                sec.insert("wep-key-flags", Value::from(0u32));
            }
            Security::Enterprise | Security::EnhancedOpen | Security::Open => {}
            _ => {
                sec.insert("psk", Value::from(password.unwrap_or_default()));
                sec.insert("psk-flags", Value::from(0u32));
            }
        }
        settings.insert("802-11-wireless-security", sec);
    }
    if security == Security::Enterprise
        && let Some(e) = enterprise
    {
        let mut eap = HashMap::from([
            ("eap", Value::from(vec![e.method.as_str()])),
            ("identity", Value::from(e.identity.as_str())),
            ("password", Value::from(e.password.as_str())),
            ("password-flags", Value::from(0u32)),
        ]);
        if !e.phase2.is_empty() {
            eap.insert("phase2-auth", Value::from(e.phase2.as_str()));
        }
        if !e.anonymous_identity.is_empty() {
            eap.insert("anonymous-identity", Value::from(e.anonymous_identity.as_str()));
        }
        if e.validate_server {
            eap.insert("system-ca-certs", Value::from(true));
            if !e.domain.is_empty() {
                eap.insert("domain-suffix-match", Value::from(e.domain.as_str()));
            }
        }
        settings.insert("802-1x", eap);
    }
    settings
}

/// Settings for a hotspot: an access point sharing this machine's
/// connection, secured with WPA2.
pub(crate) fn hotspot_settings<'a>(ssid: &str, password: &'a str, band: Option<Band>) -> NewSettings<'a> {
    let mut wireless = HashMap::from([("ssid", Value::from(ssid.as_bytes().to_vec())), ("mode", Value::from("ap"))]);
    match band {
        Some(Band::TwoPointFourGhz) => {
            wireless.insert("band", Value::from("bg"));
        }
        Some(Band::FiveGhz) => {
            wireless.insert("band", Value::from("a"));
        }
        _ => {}
    }
    HashMap::from([
        (
            "connection",
            HashMap::from([
                ("id", Value::from("Hotspot")),
                ("type", Value::from("802-11-wireless")),
                ("autoconnect", Value::from(false)),
            ]),
        ),
        ("802-11-wireless", wireless),
        (
            "802-11-wireless-security",
            HashMap::from([
                ("key-mgmt", Value::from("wpa-psk")),
                ("proto", Value::from(vec!["rsn"])),
                ("pairwise", Value::from(vec!["ccmp"])),
                ("group", Value::from(vec!["ccmp"])),
                ("psk", Value::from(password)),
                ("psk-flags", Value::from(0u32)),
            ]),
        ),
        ("ipv4", HashMap::from([("method", Value::from("shared"))])),
        ("ipv6", HashMap::from([("method", Value::from("ignore"))])),
    ])
}

/// A connection's settings in the form `Update2` takes.
pub(crate) type EditableSettings = HashMap<String, HashMap<String, Value<'static>>>;

/// How a connection gets its IPv4 address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ipv4Method {
    /// DHCP (NetworkManager's `auto`).
    Automatic,
    /// Fixed addresses (with prefix lengths) and a router.
    Manual {
        addresses: Vec<(Ipv4Addr, u8)>,
        gateway: Option<Ipv4Addr>,
    },
    /// A 169.254.x.x address only.
    LinkLocal,
    /// Share this machine's connection with the network (a hotspot or a
    /// cable to another computer).
    Shared,
    Disabled,
}

/// Set a connection's IPv4 method (and addresses) in `settings`.
pub(crate) fn apply_ipv4(settings: &mut EditableSettings, method: &Ipv4Method) {
    let ipv4 = settings.entry("ipv4".to_owned()).or_default();
    // The old array forms override the new ones when present: drop them.
    ipv4.remove("addresses");
    ipv4.remove("address-data");
    ipv4.remove("gateway");
    let name = match method {
        Ipv4Method::Automatic => "auto",
        Ipv4Method::Manual { .. } => "manual",
        Ipv4Method::LinkLocal => "link-local",
        Ipv4Method::Shared => "shared",
        Ipv4Method::Disabled => "disabled",
    };
    ipv4.insert("method".to_owned(), Value::from(name));
    if let Ipv4Method::Manual { addresses, gateway } = method {
        let data: Vec<HashMap<String, Value<'static>>> = addresses
            .iter()
            .map(|(address, prefix)| {
                HashMap::from([
                    ("address".to_owned(), Value::from(address.to_string())),
                    ("prefix".to_owned(), Value::from(u32::from(*prefix))),
                ])
            })
            .collect();
        ipv4.insert("address-data".to_owned(), Value::from(data));
        if let Some(gateway) = gateway {
            ipv4.insert("gateway".to_owned(), Value::from(gateway.to_string()));
        }
    }
}

/// Set a connection's DNS servers in `settings`: `servers` (IPv4 and IPv6)
/// instead of the network's when `automatic` is false, besides them when
/// it is true.
pub(crate) fn apply_dns(settings: &mut EditableSettings, servers: &[std::net::IpAddr], automatic: bool) {
    let v4: Vec<u32> = servers
        .iter()
        .filter_map(|ip| match ip {
            std::net::IpAddr::V4(ip) => Some(u32::from_ne_bytes(ip.octets())),
            std::net::IpAddr::V6(_) => None,
        })
        .collect();
    let v6: Vec<Vec<u8>> = servers
        .iter()
        .filter_map(|ip| match ip {
            std::net::IpAddr::V6(ip) => Some(ip.octets().to_vec()),
            std::net::IpAddr::V4(_) => None,
        })
        .collect();
    for (group, list) in [("ipv4", Value::from(v4)), ("ipv6", Value::from(v6))] {
        let props = settings.entry(group.to_owned()).or_default();
        props.remove("dns-data");
        props.insert("dns".to_owned(), list);
        props.insert("ignore-auto-dns".to_owned(), Value::from(!automatic && !servers.is_empty()));
    }
}

/// Settings as `Update2` takes them, from what `GetSettings` returned.
pub(crate) fn editable(settings: &ConnectionSettings) -> EditableSettings {
    settings
        .iter()
        .map(|(group, props)| {
            (
                group.clone(),
                props.iter().filter_map(|(k, v)| Some((k.clone(), v.try_clone().ok().map(Value::from)?))).collect(),
            )
        })
        .collect()
}

/// Whether a value is a string equal to `expected` (for tests and checks).
#[allow(dead_code)]
pub(crate) fn is_str(value: &Value<'_>, expected: &str) -> bool {
    value_str(value).as_deref() == Some(expected)
}

/// The keys of an `a{sv}` value (for checks).
#[allow(dead_code)]
pub(crate) fn dict_keys(value: &Value<'static>) -> Vec<String> {
    let mut keys: Vec<String> = value_dict(value).into_keys().collect();
    keys.sort();
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::owned;

    fn props(entries: Vec<(&str, OwnedValue)>) -> Props {
        entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
    }

    fn object(objects: &mut Objects, path: &str, iface: &str, p: Props) {
        objects.map.entry(path.to_owned()).or_default().insert(iface.to_owned(), p);
    }

    fn address_data(address: &str, prefix: u32) -> OwnedValue {
        let entry: HashMap<String, Value<'static>> = HashMap::from([
            ("address".to_owned(), Value::from(address.to_owned())),
            ("prefix".to_owned(), Value::from(prefix)),
        ]);
        owned(vec![entry])
    }

    fn fixture() -> (Objects, HashMap<String, ConnectionSettings>) {
        let mut o = Objects::default();
        object(
            &mut o,
            NM_PATH,
            NM,
            props(vec![
                ("Version", owned("1.58.1")),
                ("State", owned(70u32)),
                ("Connectivity", owned(4u32)),
                ("NetworkingEnabled", owned(true)),
                ("WirelessEnabled", owned(true)),
                ("WirelessHardwareEnabled", owned(true)),
                ("WwanEnabled", owned(true)),
                ("Metered", owned(4u32)),
                ("PrimaryConnection", owned(zbus::zvariant::ObjectPath::try_from("/ac/1").unwrap())),
            ]),
        );
        object(&mut o, SETTINGS_PATH, IFACE_SETTINGS, props(vec![("Hostname", owned("ganymede"))]));
        object(
            &mut o,
            "/dev/wlan0",
            IFACE_DEVICE,
            props(vec![
                ("Interface", owned("wlan0")),
                ("DeviceType", owned(2u32)),
                ("State", owned(100u32)),
                ("Managed", owned(true)),
                ("Real", owned(true)),
                ("HwAddress", owned("AA:BB:CC:DD:EE:FF")),
                ("ActiveConnection", owned(zbus::zvariant::ObjectPath::try_from("/ac/1").unwrap())),
                ("Ip4Config", owned(zbus::zvariant::ObjectPath::try_from("/ip4/1").unwrap())),
                ("StateReason", owned(zbus::zvariant::Structure::from((100u32, 0u32)))),
            ]),
        );
        let ap = |path: &str| zbus::zvariant::ObjectPath::try_from(path.to_owned()).unwrap();
        object(
            &mut o,
            "/dev/wlan0",
            IFACE_WIRELESS,
            props(vec![
                ("AccessPoints", owned(vec![ap("/ap/1"), ap("/ap/2"), ap("/ap/3"), ap("/ap/4")])),
                ("ActiveAccessPoint", owned(ap("/ap/2"))),
                ("Bitrate", owned(866_700u32)),
            ]),
        );
        let access_point = |ssid: &str, freq: u32, strength: u8, rsn: u32| {
            props(vec![
                ("Ssid", owned(ssid.as_bytes().to_vec())),
                ("Frequency", owned(freq)),
                ("Strength", owned(strength)),
                ("Flags", owned(if rsn != 0 { 1u32 } else { 0u32 })),
                ("WpaFlags", owned(0u32)),
                ("RsnFlags", owned(rsn)),
                ("Mode", owned(2u32)),
                ("HwAddress", owned("00:11:22:33:44:55")),
            ])
        };
        object(&mut o, "/ap/1", IFACE_AP, access_point("Home", 2412, 40, 0x188));
        object(&mut o, "/ap/2", IFACE_AP, access_point("Home", 5180, 80, 0x588));
        object(&mut o, "/ap/3", IFACE_AP, access_point("Cafe", 2437, 90, 0));
        object(&mut o, "/ap/4", IFACE_AP, access_point("", 2462, 99, 0x188));
        object(
            &mut o,
            "/ip4/1",
            IFACE_IP4,
            props(vec![
                ("AddressData", address_data("192.168.1.20", 24)),
                ("Gateway", owned("192.168.1.1")),
                ("Nameservers", owned(vec![u32::from_ne_bytes([1, 1, 1, 1])])),
            ]),
        );
        object(
            &mut o,
            "/ac/1",
            IFACE_ACTIVE,
            props(vec![
                ("Uuid", owned("aaaa")),
                ("Id", owned("Home")),
                ("Type", owned("802-11-wireless")),
                ("State", owned(2u32)),
                ("Devices", owned(vec![ap("/dev/wlan0")])),
                ("Default", owned(true)),
            ]),
        );
        object(&mut o, "/s/1", IFACE_CONNECTION, props(vec![("Filename", owned("/etc/x"))]));
        let connection = |id: &str, uuid: &str, kind: &str, timestamp: u64| -> Props {
            props(vec![
                ("id", owned(id)),
                ("uuid", owned(uuid)),
                ("type", owned(kind)),
                ("timestamp", owned(timestamp)),
            ])
        };
        let mut settings = HashMap::new();
        settings.insert(
            "/s/1".to_owned(),
            HashMap::from([
                ("connection".to_owned(), connection("Home", "aaaa", "802-11-wireless", 50)),
                ("802-11-wireless".to_owned(), props(vec![("ssid", owned(b"Home".to_vec()))])),
                ("802-11-wireless-security".to_owned(), props(vec![("key-mgmt", owned("sae"))])),
            ]),
        );
        settings.insert(
            "/s/2".to_owned(),
            HashMap::from([
                ("connection".to_owned(), connection("Work", "bbbb", "wireguard", 70)),
                ("proxy".to_owned(), props(vec![("method", owned(1i32)), ("pac-url", owned("http://wpad/wpad.dat"))])),
            ]),
        );
        (o, settings)
    }

    #[test]
    fn access_points_group_into_networks() {
        let (objects, settings) = fixture();
        let state = build(&objects, &settings, &[]);
        let names: Vec<&str> = state.networks.iter().map(|n| n.ssid.as_str()).collect();
        // Connected first, then by strength; the nameless one is left out.
        assert_eq!(names, ["Home", "Cafe"]);
        let home = &state.networks[0];
        assert!(home.connected && home.known);
        assert_eq!(home.connection.as_deref(), Some("aaaa"));
        assert_eq!(home.strength, 80);
        assert_eq!(home.best_access_point, "/ap/2");
        assert_eq!(home.bands, [Band::TwoPointFourGhz, Band::FiveGhz]);
        assert_eq!(home.security, Security::Wpa2Wpa3Personal);
        assert_eq!(state.networks[1].security, Security::Open);
        assert!(!state.networks[1].known);
    }

    #[test]
    fn devices_and_details() {
        let (objects, settings) = fixture();
        let state = build(&objects, &settings, &[]);
        assert!(state.available);
        assert_eq!(state.state, NmState::ConnectedGlobal);
        assert_eq!(state.metered, Metered::GuessNo);
        let wifi = state.wifi_device().unwrap();
        assert_eq!(wifi.interface, "wlan0");
        assert_eq!(wifi.state_reason, 0);
        let details = state.primary_details().unwrap();
        assert_eq!(details.ipv4.address(), Some("192.168.1.20"));
        assert_eq!(details.ipv4.gateway.as_deref(), Some("192.168.1.1"));
        assert_eq!(details.ipv4.dns, ["1.1.1.1"]);
        assert_eq!(details.hardware_address, "AA:BB:CC:DD:EE:FF");
        assert_eq!(details.speed_mbps, Some(866));
        assert_eq!(details.frequency_mhz, Some(5180));
        assert_eq!(state.hostname, "ganymede");
    }

    #[test]
    fn saved_connections_and_vpns() {
        let (objects, settings) = fixture();
        let state = build(&objects, &settings, &[]);
        let ids: Vec<&str> = state.connections.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["Work", "Home"], "most recently used first");
        let home = state.connection("aaaa").unwrap();
        assert!(home.active && home.autoconnect);
        assert_eq!(home.ssid.as_deref(), Some("Home"));
        assert_eq!(Security::from_key_mgmt(home.key_mgmt.as_deref()), Security::Wpa3Personal);
        assert_eq!(home.filename, "/etc/x");
        let vpns: Vec<&str> = state.vpns().map(|c| c.id.as_str()).collect();
        assert_eq!(vpns, ["Work"]);
        let work = state.connection("bbbb").unwrap();
        assert!(work.proxy.automatic);
        assert_eq!(work.proxy.pac_url.as_deref(), Some("http://wpad/wpad.dat"));
    }

    #[test]
    fn security_from_flags() {
        let of = Security::from_flags;
        assert_eq!(of(0, 0, 0), Security::Open);
        assert_eq!(of(1, 0, 0), Security::Wep);
        assert_eq!(of(1, 0x188, 0), Security::WpaPersonal);
        assert_eq!(of(1, 0x188, 0x188), Security::Wpa2Personal);
        assert_eq!(of(1, 0, 0x588), Security::Wpa2Wpa3Personal);
        assert_eq!(of(1, 0, 0x488), Security::Wpa3Personal);
        assert_eq!(of(1, 0, 0x288), Security::Enterprise);
        assert_eq!(of(0, 0, 0x888), Security::EnhancedOpen);
        assert_eq!(Security::Wpa3Personal.key_mgmt(), Some("sae"));
        assert_eq!(Security::Wpa2Wpa3Personal.key_mgmt(), Some("wpa-psk"));
        assert!(!Security::EnhancedOpen.needs_password());
    }

    #[test]
    fn airplane_mode_follows_rfkill() {
        let (objects, settings) = fixture();
        let blocked = |soft| Radio { index: 0, kind: RadioKind::Wlan, soft_blocked: soft, hard_blocked: false };
        assert!(!build(&objects, &settings, &[blocked(false)]).airplane_mode);
        assert!(build(&objects, &settings, &[blocked(true)]).airplane_mode);
    }

    #[test]
    fn odd_ssids_are_escaped() {
        assert_eq!(ssid_name(b"Caf\xc3\xa9"), "Café");
        assert_eq!(ssid_name(b"\xff\x01ab"), "\\xff\\x01ab");
        assert_eq!(Band::from_mhz(5955), Band::SixGhz);
    }

    #[test]
    fn addresses_and_dns_edit_the_right_keys() {
        let mut settings: EditableSettings = HashMap::from([(
            "ipv4".to_owned(),
            HashMap::from([
                ("method".to_owned(), Value::from("auto")),
                ("addresses".to_owned(), Value::from(vec![vec![1u32, 24, 0]])),
                ("dns-data".to_owned(), Value::from(vec!["9.9.9.9"])),
            ]),
        )]);
        apply_ipv4(
            &mut settings,
            &Ipv4Method::Manual {
                addresses: vec![(Ipv4Addr::new(192, 168, 1, 20), 24)],
                gateway: Some(Ipv4Addr::new(192, 168, 1, 1)),
            },
        );
        let ipv4 = &settings["ipv4"];
        assert!(is_str(&ipv4["method"], "manual"));
        assert!(!ipv4.contains_key("addresses"), "the old form would override the new");
        assert_eq!(dict_keys(&Value::from(HashMap::from([("k", 1u32)]))), ["k"]);
        assert!(is_str(&ipv4["gateway"], "192.168.1.1"));
        apply_dns(&mut settings, &["1.1.1.1".parse().unwrap(), "2606:4700:4700::1111".parse().unwrap()], false);
        let ipv4 = &settings["ipv4"];
        assert!(!ipv4.contains_key("dns-data"));
        assert_eq!(ipv4["dns"], Value::from(vec![u32::from_ne_bytes([1, 1, 1, 1])]));
        assert_eq!(ipv4["ignore-auto-dns"], Value::from(true));
        assert_eq!(settings["ipv6"]["ignore-auto-dns"], Value::from(true));
        apply_dns(&mut settings, &[], true);
        assert_eq!(settings["ipv4"]["ignore-auto-dns"], Value::from(false));
    }

    #[test]
    fn new_wifi_settings_carry_their_secret() {
        let settings = wifi_settings(b"Home", Security::Wpa3Personal, Some("secret"), None, true);
        let sec = &settings["802-11-wireless-security"];
        assert!(is_str(&sec["key-mgmt"], "sae"));
        assert!(is_str(&sec["psk"], "secret"));
        assert_eq!(settings["802-11-wireless"]["hidden"], Value::from(true));
        let open = wifi_settings(b"Cafe", Security::Open, None, None, false);
        assert!(!open.contains_key("802-11-wireless-security"));
        let hotspot = hotspot_settings("Mine", "password1", Some(Band::FiveGhz));
        assert!(is_str(&hotspot["ipv4"]["method"], "shared"));
        assert!(is_str(&hotspot["802-11-wireless"]["band"], "a"));
    }
}
