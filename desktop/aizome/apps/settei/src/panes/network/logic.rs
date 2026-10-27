//! What the Network pane decides from NetworkManager's state, with no
//! window and no bus: the words rows say, DNS presets, addresses typed by
//! hand, private-address choices, how long ago a network was joined, why
//! joining failed. Ported from Tatami's Network (`model.rs`, `window.rs`),
//! with nmcli's text replaced by `aizome-sys`'s snapshot.

use std::net::{IpAddr, Ipv4Addr};

use aizome_sys::Error;
use aizome_sys::network::{
    ConnectionKind, Device, DeviceKind, DeviceState, NetworkState, SavedConnection, Security, WifiNetwork,
};

/// DNS providers, as in Tatami's network panel and Omarchy's Setup › DNS.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dns {
    Automatic,
    Cloudflare,
    Google,
    Quad9,
    Custom,
}

pub const CLOUDFLARE: [&str; 4] = ["1.1.1.1", "1.0.0.1", "2606:4700:4700::1111", "2606:4700:4700::1001"];
pub const GOOGLE: [&str; 4] = ["8.8.8.8", "8.8.4.4", "2001:4860:4860::8888", "2001:4860:4860::8844"];
pub const QUAD9: [&str; 4] = ["9.9.9.9", "149.112.112.112", "2620:fe::fe", "2620:fe::9"];

impl Dns {
    pub const ALL: [Dns; 5] = [Dns::Automatic, Dns::Cloudflare, Dns::Google, Dns::Quad9, Dns::Custom];

    pub fn name(self) -> &'static str {
        match self {
            Dns::Automatic => "Automatic",
            Dns::Cloudflare => "Cloudflare",
            Dns::Google => "Google",
            Dns::Quad9 => "Quad9",
            Dns::Custom => "Custom",
        }
    }

    pub fn servers(self) -> &'static [&'static str] {
        match self {
            Dns::Cloudflare => &CLOUDFLARE,
            Dns::Google => &GOOGLE,
            Dns::Quad9 => &QUAD9,
            Dns::Automatic | Dns::Custom => &[],
        }
    }

    /// Which provider a connection's own servers are.
    pub fn of(ignore_auto: bool, servers: &[String]) -> Dns {
        if servers.is_empty() && !ignore_auto {
            Dns::Automatic
        } else if servers.iter().any(|s| s == "1.1.1.1") {
            Dns::Cloudflare
        } else if servers.iter().any(|s| s == "8.8.8.8") {
            Dns::Google
        } else if servers.iter().any(|s| s == "9.9.9.9") {
            Dns::Quad9
        } else {
            Dns::Custom
        }
    }

    /// The servers as addresses.
    pub fn addresses(self) -> Vec<IpAddr> {
        self.servers().iter().filter_map(|s| s.parse().ok()).collect()
    }
}

/// Servers typed for Custom: addresses separated by spaces or commas;
/// nothing when any of them isn't an address.
pub fn custom_servers(text: &str) -> Option<Vec<IpAddr>> {
    let mut out = Vec::new();
    for word in text.split(|c: char| c == ',' || c.is_whitespace()) {
        match word.parse::<IpAddr>() {
            Ok(ip) => out.push(ip),
            Err(_) if word.is_empty() => {}
            Err(_) => return None,
        }
    }
    (!out.is_empty()).then_some(out)
}

/// An IPv4 address with its prefix length, as `192.168.1.20/24`.
pub fn address_with_prefix(text: &str) -> Option<(Ipv4Addr, u8)> {
    let (address, prefix) = text.trim().split_once('/')?;
    let address: Ipv4Addr = address.trim().parse().ok()?;
    let prefix: u8 = prefix.trim().parse().ok()?;
    (1..=32).contains(&prefix).then_some((address, prefix))
}

pub fn ipv4(text: &str) -> Option<Ipv4Addr> {
    text.trim().parse::<Ipv4Addr>().ok()
}

/// A connection's own hardware address on Wi-Fi
/// (`802-11-wireless.assigned-mac-address`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Mac {
    /// The system's choice (NetworkManager's default for all connections).
    Default,
    /// The device's own address.
    Permanent,
    /// A private address that stays the same on this network.
    Stable,
    /// A new private address each time it connects.
    Random,
    /// Some other value, set elsewhere and left as it is.
    Other(String),
}

impl Mac {
    pub const CHOICES: [Mac; 4] = [Mac::Default, Mac::Permanent, Mac::Stable, Mac::Random];

    pub fn of(value: &str) -> Mac {
        match value {
            "" | "preserve" => Mac::Default,
            "permanent" => Mac::Permanent,
            "stable" => Mac::Stable,
            "random" => Mac::Random,
            other => Mac::Other(other.to_owned()),
        }
    }

    pub fn value(&self) -> &str {
        match self {
            Mac::Default => "",
            Mac::Permanent => "permanent",
            Mac::Stable => "stable",
            Mac::Random => "random",
            Mac::Other(v) => v,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Mac::Default => "System default".into(),
            Mac::Permanent => "Off, the device's own".into(),
            Mac::Stable => "Private, fixed for this network".into(),
            Mac::Random => "Private, new each time".into(),
            Mac::Other(v) => format!("Set elsewhere ({v})"),
        }
    }
}

/// The shortest password a network's key management takes: WPA's
/// passphrases are 8 to 63 characters; WPA3 (SAE) and 802.1X take any.
pub fn minimum_password(security: Security) -> usize {
    match security {
        Security::WpaPersonal | Security::Wpa2Personal | Security::Wpa2Wpa3Personal => 8,
        Security::Wep => 5,
        _ => 1,
    }
}

/// Whether a password can be tried: long enough, and short enough for
/// WPA-PSK, and on one line.
pub fn password_ok(security: Security, password: &str) -> bool {
    let n = password.chars().count();
    let max = match security {
        Security::WpaPersonal | Security::Wpa2Personal | Security::Wpa2Wpa3Personal => 63,
        _ => usize::MAX,
    };
    n >= minimum_password(security) && n <= max && !password.contains('\n')
}

/// What to tell someone when joining or changing failed.
pub fn reason(error: &Error) -> String {
    match error {
        Error::Rejected(_) => "Check the password and try again".into(),
        Error::Timeout => "The network didn't answer in time".into(),
        Error::NotAuthorized(_) => "You aren't allowed to change network connections".into(),
        Error::Unavailable(_) => "NetworkManager isn't running".into(),
        Error::Cancelled => "Cancelled".into(),
        Error::Failed(m) | Error::InvalidArgument(m) | Error::NotFound(m) => m.clone(),
        Error::Command { message, .. } => message.clone(),
        other => other.to_string(),
    }
}

/// How long ago, in words, for when a saved network was last used.
pub fn since(now: u64, then: u64) -> String {
    if then == 0 {
        return "Never joined".into();
    }
    let days = now.saturating_sub(then) / 86_400;
    match days {
        0 => "Joined today".into(),
        1 => "Joined yesterday".into(),
        2..=13 => format!("Joined {days} days ago"),
        14..=59 => format!("Joined {} weeks ago", days / 7),
        60..=729 => format!("Joined {} months ago", days / 30),
        _ => format!("Joined {} years ago", days / 365),
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// What the Wi-Fi row says under its name.
pub fn wifi_subtitle(state: &NetworkState) -> String {
    if state.airplane_mode {
        "Off, in airplane mode".into()
    } else if !state.wifi_hardware_enabled {
        "Off, turned off by the hardware switch".into()
    } else if !state.wifi_enabled {
        "Off".into()
    } else if let Some(n) = state.current_wifi() {
        format!("Connected to {}", n.ssid)
    } else if state.wifi_device().is_some_and(Device::connecting) || state.networks.iter().any(|n| n.connecting) {
        "Connecting…".into()
    } else {
        "Not connected".into()
    }
}

/// "Secured with WPA2", "Open, not secured".
pub fn security_words(security: Security) -> String {
    match security {
        Security::Open => "Open, not secured".into(),
        Security::EnhancedOpen => "Open, encrypted (Enhanced Open)".into(),
        Security::Wep => "WEP, which anyone nearby can break".into(),
        Security::Enterprise => "Enterprise: sign in with an account".into(),
        s => format!("Secured with {}", s.label()),
    }
}

/// Networks in range other than the one in use, split into the known and
/// the others, each by strength; signal changes within a bar don't move
/// rows about.
pub fn in_range(state: &NetworkState) -> (Vec<&WifiNetwork>, Vec<&WifiNetwork>) {
    let mut known = Vec::new();
    let mut others = Vec::new();
    for n in state.networks.iter().filter(|n| !n.connected) {
        if n.known { known.push(n) } else { others.push(n) }
    }
    let order = |a: &&WifiNetwork, b: &&WifiNetwork| {
        b.bars().cmp(&a.bars()).then_with(|| a.ssid.to_lowercase().cmp(&b.ssid.to_lowercase()))
    };
    known.sort_by(order);
    others.sort_by(order);
    (known, others)
}

/// A wired device as its row shows it: title, state words, and the
/// connection it uses (or would use).
#[derive(Clone, Debug, PartialEq)]
pub struct Wired {
    pub device: String,
    pub title: String,
    pub status: String,
    pub connected: bool,
    pub uuid: Option<String>,
}

pub fn wired(state: &NetworkState) -> Vec<Wired> {
    let devices: Vec<&Device> = state.wired_devices().collect();
    let many = devices.len() > 1;
    devices
        .into_iter()
        .map(|d| {
            let details = state.details(&d.path);
            let address = details.as_ref().and_then(|x| x.ipv4.address().map(str::to_owned));
            let status = if d.connected() {
                let speed = d.speed_mbps.filter(|s| *s > 0).map(speed_words);
                match (address, speed) {
                    (Some(a), Some(s)) => format!("Connected · {a} · {s}"),
                    (Some(a), None) => format!("Connected · {a}"),
                    (None, Some(s)) => format!("Connected · {s}"),
                    (None, None) => "Connected".into(),
                }
            } else if d.connecting() {
                "Connecting…".into()
            } else if d.state == DeviceState::Unavailable || d.carrier == Some(false) {
                "Cable unplugged".into()
            } else {
                "Not connected".into()
            };
            let uuid = d
                .active_connection
                .as_deref()
                .and_then(|p| state.active.iter().find(|a| a.path == p))
                .map(|a| a.uuid.clone())
                .or_else(|| {
                    state
                        .connections
                        .iter()
                        .filter(|c| c.kind == ConnectionKind::Ethernet)
                        .filter(|c| c.interface_name.as_ref().is_none_or(|i| *i == d.interface))
                        .max_by_key(|c| c.timestamp)
                        .map(|c| c.uuid.clone())
                });
            Wired {
                device: d.path.clone(),
                title: if many { format!("Ethernet · {}", d.interface) } else { "Ethernet".into() },
                status,
                connected: d.connected(),
                uuid,
            }
        })
        .collect()
}

/// "1 Gb/s", "100 Mb/s", "866 Mb/s".
pub fn speed_words(mbps: u32) -> String {
    if mbps >= 1000 && mbps.is_multiple_of(1000) { format!("{} Gb/s", mbps / 1000) } else { format!("{mbps} Mb/s") }
}

/// "WireGuard", "OpenVPN", "VPN".
pub fn vpn_kind(c: &SavedConnection) -> String {
    if c.kind == ConnectionKind::WireGuard {
        return "WireGuard".into();
    }
    let service = c.vpn_service.as_deref().unwrap_or("");
    let plugin = service.rsplit('.').next().unwrap_or("");
    match plugin {
        "openvpn" => "OpenVPN".into(),
        "openconnect" => "OpenConnect".into(),
        "vpnc" => "Cisco (vpnc)".into(),
        "l2tp" => "L2TP".into(),
        "pptp" => "PPTP".into(),
        "strongswan" | "libreswan" => "IPsec".into(),
        "fortisslvpn" => "Fortinet".into(),
        _ => "VPN".into(),
    }
}

/// The state of a VPN, in words.
pub fn vpn_state(state: &NetworkState, c: &SavedConnection) -> &'static str {
    match state.active_connection(&c.uuid).map(|a| a.state) {
        Some(aizome_sys::network::ActiveState::Activating) => "Connecting…",
        Some(aizome_sys::network::ActiveState::Activated) => "Connected",
        Some(aizome_sys::network::ActiveState::Deactivating) => "Disconnecting…",
        _ => "Not connected",
    }
}

/// Whether the machine has a Wi-Fi device NetworkManager manages.
pub fn has_wifi(state: &NetworkState) -> bool {
    state.wifi_device().is_some()
}

/// Whether the hardware can share a connection as a hotspot: a Wi-Fi
/// device and a connection to share that isn't that Wi-Fi.
pub fn hotspot_possible(state: &NetworkState) -> bool {
    has_wifi(state) && state.devices.iter().any(|d| d.kind == DeviceKind::Wifi)
}

/// The name others see the hotspot by: its network's (the SSID), not its
/// profile's ("Hotspot"); none while it's off.
pub fn hotspot_name(state: &NetworkState) -> Option<String> {
    let a = state.hotspot()?;
    Some(state.connection(&a.uuid).and_then(|c| c.ssid.clone()).unwrap_or_else(|| a.id.clone()))
}

/// A saved network's row words: when it was joined, and whether it joins
/// by itself.
pub fn saved_words(c: &SavedConnection, now: u64) -> String {
    let mut s = if c.active { "Connected".to_owned() } else { since(now, c.timestamp) };
    if !c.autoconnect {
        s.push_str(" · automatic joining off");
    }
    s
}

/// "2.4 GHz", "5 GHz" from MHz.
pub fn band_words(mhz: u32) -> String {
    aizome_sys::network::Band::from_mhz(mhz).label().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hotspot_is_named_by_its_network() {
        use aizome_sys::network::{ActiveConnection, ConnectionKind, SavedConnection};
        let mut state = NetworkState::default();
        assert_eq!(hotspot_name(&state), None);
        state.connections.push(SavedConnection {
            uuid: "h".into(),
            id: "Hotspot".into(),
            kind: ConnectionKind::Wifi,
            ssid: Some("ganymede Hotspot".into()),
            wifi_mode: Some("ap".into()),
            active: true,
            ..SavedConnection::default()
        });
        state.active.push(ActiveConnection { uuid: "h".into(), id: "Hotspot".into(), ..ActiveConnection::default() });
        assert_eq!(hotspot_name(&state).as_deref(), Some("ganymede Hotspot"));
    }

    #[test]
    fn dns_presets_and_custom_servers() {
        assert_eq!(Dns::of(false, &[]), Dns::Automatic);
        assert_eq!(Dns::of(true, &["1.1.1.1".into(), "1.0.0.1".into()]), Dns::Cloudflare);
        assert_eq!(Dns::of(true, &["9.9.9.9".into()]), Dns::Quad9);
        assert_eq!(Dns::of(false, &["10.0.0.53".into()]), Dns::Custom);
        assert_eq!(Dns::Cloudflare.addresses().len(), 4);
        assert_eq!(custom_servers("9.9.9.9, 2620:fe::fe").map(|v| v.len()), Some(2));
        assert_eq!(custom_servers("dns.quad9.net"), None);
        assert_eq!(custom_servers("  "), None);
    }

    #[test]
    fn typed_addresses_are_checked() {
        assert_eq!(address_with_prefix(" 192.168.1.20/24 "), Some((Ipv4Addr::new(192, 168, 1, 20), 24)));
        assert_eq!(address_with_prefix("192.168.1.20"), None);
        assert_eq!(address_with_prefix("192.168.1.300/24"), None);
        assert_eq!(address_with_prefix("192.168.1.20/33"), None);
        assert_eq!(ipv4("10.0.0.1"), Some(Ipv4Addr::new(10, 0, 0, 1)));
        assert_eq!(ipv4("router"), None);
    }

    #[test]
    fn passwords_fit_the_network() {
        assert!(!password_ok(Security::Wpa2Personal, "short"));
        assert!(password_ok(Security::Wpa2Personal, "long enough"));
        assert!(!password_ok(Security::Wpa2Personal, &"x".repeat(64)));
        assert!(password_ok(Security::Wpa3Personal, "x"));
        assert!(!password_ok(Security::Wpa3Personal, "two\nlines"));
    }

    #[test]
    fn private_addresses_and_failures_read_plainly() {
        assert_eq!(Mac::of("stable"), Mac::Stable);
        assert_eq!(Mac::of(""), Mac::Default);
        assert_eq!(Mac::of("02:00:00:00:00:01"), Mac::Other("02:00:00:00:00:01".into()));
        assert_eq!(reason(&Error::Rejected("x".into())), "Check the password and try again");
        assert_eq!(reason(&Error::Failed("The network is out of range".into())), "The network is out of range");
        assert_eq!(speed_words(1000), "1 Gb/s");
        assert_eq!(speed_words(866), "866 Mb/s");
    }

    #[test]
    fn last_joined_in_words() {
        let day = 86_400;
        assert_eq!(since(10 * day, 0), "Never joined");
        assert_eq!(since(10 * day, 10 * day - 5), "Joined today");
        assert_eq!(since(10 * day, 9 * day), "Joined yesterday");
        assert_eq!(since(100 * day, 70 * day), "Joined 4 weeks ago");
    }
}
