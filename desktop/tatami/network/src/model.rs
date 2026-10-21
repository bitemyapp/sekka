// SPDX-License-Identifier: MIT OR Apache-2.0
//! What NetworkManager reports, read from nmcli's terse output, and what the
//! window decides from it. Nothing here runs a command (see nm.rs).
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};

/// Fields of a terse nmcli list line: `:` separates them, `\:` and `\\`
/// escape.
pub fn fields(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    fields.last_mut().unwrap().push(next);
                }
            }
            ':' => fields.push(String::new()),
            c => fields.last_mut().unwrap().push(c),
        }
    }
    fields
}

/// `name:value` lines, as `nmcli -t -f` prints settings and device
/// properties: values are not escaped, and a blank line ends each
/// connection's or device's block.
pub fn blocks(text: &str) -> Vec<Vec<(String, String)>> {
    let mut blocks = vec![Vec::new()];
    for line in text.lines() {
        match line.split_once(':') {
            Some((name, value)) => blocks
                .last_mut()
                .unwrap()
                .push((name.to_owned(), value.to_owned())),
            None if line.trim().is_empty() => blocks.push(Vec::new()),
            None => {}
        }
    }
    blocks.retain(|block| !block.is_empty());
    blocks
}

fn get<'a>(block: &'a [(String, String)], name: &str) -> Option<&'a str> {
    block
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// Every value of a numbered property (`IP4.ADDRESS[1]`, `IP4.ADDRESS[2]`…).
fn get_all<'a>(block: &'a [(String, String)], name: &str) -> Vec<&'a str> {
    block
        .iter()
        .filter(|(key, value)| {
            !value.is_empty()
                && (key == name
                    || key
                        .strip_prefix(name)
                        .is_some_and(|rest| rest.starts_with('[')))
        })
        .map(|(_, value)| value.as_str())
        .collect()
}

/// A comma-separated setting, as nmcli prints lists.
fn list(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Wifi,
    Ethernet,
    Vpn,
    WireGuard,
    Other,
}

impl Kind {
    /// A connection's type (`connection.type`).
    pub fn of_connection(kind: &str) -> Kind {
        match kind {
            "802-11-wireless" | "wifi" => Kind::Wifi,
            "802-3-ethernet" | "ethernet" => Kind::Ethernet,
            "vpn" => Kind::Vpn,
            "wireguard" => Kind::WireGuard,
            _ => Kind::Other,
        }
    }

    /// A device's type (`nmcli device`).
    pub fn of_device(kind: &str) -> Kind {
        match kind {
            "wifi" => Kind::Wifi,
            "ethernet" => Kind::Ethernet,
            "wireguard" => Kind::WireGuard,
            _ => Kind::Other,
        }
    }

    pub fn is_vpn(self) -> bool {
        matches!(self, Kind::Vpn | Kind::WireGuard)
    }
}

/// A network device, from `nmcli -t -f DEVICE,TYPE,STATE,CONNECTION,CON-UUID
/// device`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub kind: Kind,
    /// NetworkManager's word for it: connected, connecting (…),
    /// disconnected, unavailable (no cable, or Wi-Fi off), unmanaged…
    pub state: String,
    pub connection: Option<String>,
}

impl Device {
    pub fn connected(&self) -> bool {
        self.state.starts_with("connected")
    }

    pub fn connecting(&self) -> bool {
        self.state.starts_with("connecting")
    }

    /// Left to something other than NetworkManager, so not set up here.
    pub fn unmanaged(&self) -> bool {
        self.state == "unmanaged"
    }
}

pub fn devices(text: &str) -> Vec<Device> {
    text.lines()
        .map(fields)
        .filter(|fields| fields.len() >= 5 && !fields[0].is_empty())
        .map(|fields| Device {
            kind: Kind::of_device(&fields[1]),
            state: fields[2].clone(),
            connection: Some(fields[4].clone()).filter(|uuid| !uuid.is_empty()),
            name: fields[0].clone(),
        })
        .collect()
}

/// A saved connection, from `nmcli -t -f NAME,UUID,TYPE,AUTOCONNECT,ACTIVE,
/// DEVICE,STATE,TIMESTAMP connection show`, with the Wi-Fi settings added
/// from its own block (wifi_settings).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub name: String,
    pub uuid: String,
    pub kind: Kind,
    pub autoconnect: bool,
    pub active: bool,
    pub activating: bool,
    pub device: Option<String>,
    /// When it was last connected (seconds since 1970), 0 never.
    pub timestamp: u64,
    pub ssid: Option<String>,
    pub security: Option<String>,
}

pub fn saved(text: &str) -> Vec<Saved> {
    text.lines()
        .map(fields)
        .filter(|fields| fields.len() >= 8 && !fields[1].is_empty())
        .map(|fields| Saved {
            name: fields[0].clone(),
            uuid: fields[1].clone(),
            kind: Kind::of_connection(&fields[2]),
            autoconnect: fields[3] == "yes",
            active: fields[4] == "yes",
            activating: fields[6] == "activating",
            device: Some(fields[5].clone()).filter(|device| !device.is_empty()),
            timestamp: fields[7].parse().unwrap_or(0),
            ssid: None,
            security: None,
        })
        .collect()
}

/// Add each Wi-Fi connection's network name and key management, from
/// `nmcli -t -f connection.uuid,802-11-wireless.ssid,
/// 802-11-wireless-security.key-mgmt connection show UUID…`.
pub fn wifi_settings(saved: &mut [Saved], text: &str) {
    for block in blocks(text) {
        let Some(uuid) = get(&block, "connection.uuid") else {
            continue;
        };
        if let Some(connection) = saved.iter_mut().find(|saved| saved.uuid == uuid) {
            connection.ssid = get(&block, "802-11-wireless.ssid").map(str::to_owned);
            connection.security = get(&block, "802-11-wireless-security.key-mgmt")
                .filter(|key| !key.is_empty())
                .map(str::to_owned);
        }
    }
}

/// How a network is joined, from what it advertises.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Security {
    /// No key: open and enhanced-open (OWE) networks.
    Open,
    /// A personal password, with NetworkManager's key management for it.
    Personal(&'static str),
    /// 802.1X: a username and password from an organization.
    Enterprise,
    /// WEP, which is not secure, and not offered.
    Wep,
}

impl Security {
    pub fn from_advertised(security: &str) -> Security {
        if security.is_empty() || security == "--" || security == "OWE" {
            Security::Open
        } else if security.contains("802.1X") {
            Security::Enterprise
        } else if security.contains("WEP") {
            Security::Wep
        } else if security.contains("WPA3")
            && !security.contains("WPA2")
            && !security.contains("WPA1")
        {
            Security::Personal("sae")
        } else {
            Security::Personal("wpa-psk")
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Security::Open => "Open",
            Security::Personal(_) => "Secured",
            Security::Enterprise => "Enterprise",
            Security::Wep => "WEP, not secure",
        }
    }
}

/// A Wi-Fi network in range, one per name, from `nmcli -t -f
/// IN-USE,SSID,SECURITY,SIGNAL device wifi list`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Network {
    pub ssid: String,
    pub security: String,
    pub signal: u8,
    pub in_use: bool,
}

impl Network {
    pub fn security(&self) -> Security {
        Security::from_advertised(&self.security)
    }
}

/// Networks in range, one per name, strongest first. Hidden networks (no
/// name) are joined by name instead.
pub fn networks(list: &str) -> Vec<Network> {
    let mut found: Vec<Network> = Vec::new();
    for line in list.lines() {
        let [in_use, ssid, security, signal] = fields(line).try_into().unwrap_or_default();
        if ssid.is_empty() {
            continue;
        }
        let network = Network {
            in_use: in_use == "*",
            signal: signal.parse().unwrap_or(0),
            ssid,
            security,
        };
        match found.iter_mut().find(|known| known.ssid == network.ssid) {
            Some(known) => {
                known.in_use |= network.in_use;
                if network.signal > known.signal {
                    known.signal = network.signal;
                    known.security = network.security;
                }
            }
            None => found.push(network),
        }
    }
    found.sort_by(|a, b| b.signal.cmp(&a.signal).then_with(|| a.ssid.cmp(&b.ssid)));
    found
}

/// Signal strength in four steps, as the icons show it; the window orders
/// and redraws by these, so small changes don't move rows about.
pub fn bars(signal: u8) -> u8 {
    match signal {
        75.. => 4,
        50..=74 => 3,
        25..=49 => 2,
        _ => 1,
    }
}

pub fn signal_icon(signal: u8) -> &'static str {
    match bars(signal) {
        4 => "network-wireless-signal-excellent-symbolic",
        3 => "network-wireless-signal-good-symbolic",
        2 => "network-wireless-signal-ok-symbolic",
        _ => "network-wireless-signal-weak-symbolic",
    }
}

/// A device's addresses, from `nmcli -t -f GENERAL.DEVICE,GENERAL.HWADDR,
/// IP4.ADDRESS,IP4.GATEWAY,IP4.DNS,IP6.ADDRESS,IP6.DNS device show`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Addresses {
    pub hardware: String,
    pub ipv4: Vec<String>,
    pub router: String,
    pub dns: Vec<String>,
    pub ipv6: Vec<String>,
}

impl Addresses {
    /// The first IPv4 address without its prefix length, as rows show it.
    pub fn address(&self) -> Option<&str> {
        self.ipv4
            .first()
            .map(|address| address.split('/').next().unwrap_or(address))
    }
}

pub fn addresses(text: &str) -> HashMap<String, Addresses> {
    blocks(text)
        .into_iter()
        .filter_map(|block| {
            let device = get(&block, "GENERAL.DEVICE")?.to_owned();
            let all = |name| get_all(&block, name).into_iter().map(str::to_owned);
            Some((
                device,
                Addresses {
                    hardware: get(&block, "GENERAL.HWADDR").unwrap_or("").to_owned(),
                    ipv4: all("IP4.ADDRESS").collect(),
                    router: get(&block, "IP4.GATEWAY").unwrap_or("").to_owned(),
                    dns: all("IP4.DNS").chain(all("IP6.DNS")).collect(),
                    ipv6: all("IP6.ADDRESS").collect(),
                },
            ))
        })
        .collect()
}

/// Everything the window shows, read at once (nm::snapshot).
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub wifi_enabled: bool,
    /// False when a hardware switch or the firmware keeps Wi-Fi off.
    pub wifi_hardware: bool,
    pub devices: Vec<Device>,
    pub saved: Vec<Saved>,
    pub networks: Vec<Network>,
    pub addresses: HashMap<String, Addresses>,
}

impl Snapshot {
    pub fn wifi_device(&self) -> Option<&Device> {
        self.devices
            .iter()
            .find(|device| device.kind == Kind::Wifi && !device.unmanaged())
    }

    pub fn ethernet(&self) -> impl Iterator<Item = &Device> {
        self.devices
            .iter()
            .filter(|device| device.kind == Kind::Ethernet && !device.unmanaged())
    }

    pub fn connection(&self, uuid: &str) -> Option<&Saved> {
        self.saved.iter().find(|saved| saved.uuid == uuid)
    }

    /// The saved connection for a network, the most recently used first.
    pub fn saved_for(&self, ssid: &str) -> Option<&Saved> {
        self.saved
            .iter()
            .filter(|saved| saved.kind == Kind::Wifi && saved.ssid.as_deref() == Some(ssid))
            .max_by_key(|saved| (saved.active, saved.timestamp))
    }

    /// The Wi-Fi network in use and its connection.
    pub fn current_wifi(&self) -> Option<(&Network, &Saved)> {
        let device = self.wifi_device()?;
        let uuid = device.connection.as_deref()?;
        let saved = self.connection(uuid)?;
        let ssid = saved.ssid.as_deref()?;
        let network = self.networks.iter().find(|network| network.ssid == ssid)?;
        Some((network, saved))
    }

    pub fn vpns(&self) -> impl Iterator<Item = &Saved> {
        self.saved.iter().filter(|saved| saved.kind.is_vpn())
    }

    /// Saved Wi-Fi networks, the most recently used first.
    pub fn saved_wifi(&self) -> Vec<&Saved> {
        let mut wifi: Vec<&Saved> = self
            .saved
            .iter()
            .filter(|saved| saved.kind == Kind::Wifi)
            .collect();
        wifi.sort_by(|a, b| {
            b.timestamp
                .cmp(&a.timestamp)
                .then_with(|| a.name.cmp(&b.name))
        });
        wifi
    }
}

/// DNS providers, as in the network panel and Omarchy's Setup › DNS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dns {
    Automatic,
    Cloudflare,
    Google,
    Custom,
}

pub const CLOUDFLARE: [&str; 4] = [
    "1.1.1.1",
    "1.0.0.1",
    "2606:4700:4700::1111",
    "2606:4700:4700::1001",
];
pub const GOOGLE: [&str; 4] = [
    "8.8.8.8",
    "8.8.4.4",
    "2001:4860:4860::8888",
    "2001:4860:4860::8844",
];

impl Dns {
    pub const ALL: [Dns; 4] = [Dns::Automatic, Dns::Cloudflare, Dns::Google, Dns::Custom];

    pub fn name(self) -> &'static str {
        match self {
            Dns::Automatic => "Automatic",
            Dns::Cloudflare => "Cloudflare",
            Dns::Google => "Google",
            Dns::Custom => "Custom",
        }
    }

    pub fn servers(self) -> &'static [&'static str] {
        match self {
            Dns::Cloudflare => &CLOUDFLARE,
            Dns::Google => &GOOGLE,
            Dns::Automatic | Dns::Custom => &[],
        }
    }

    fn of(ignore_auto: bool, servers: &[String]) -> Dns {
        if !ignore_auto && servers.is_empty() {
            Dns::Automatic
        } else if servers.iter().any(|server| server == "1.1.1.1") {
            Dns::Cloudflare
        } else if servers.iter().any(|server| server == "8.8.8.8") {
            Dns::Google
        } else {
            Dns::Custom
        }
    }
}

/// Servers typed for Custom: addresses separated by spaces or commas, split
/// into IPv4 and IPv6; nothing when any of them is not an address.
pub fn custom_servers(text: &str) -> Option<(Vec<String>, Vec<String>)> {
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    for word in text.split(|c: char| c == ',' || c.is_whitespace()) {
        match word.parse::<IpAddr>() {
            Ok(address) => {
                let servers = if address.is_ipv4() { &mut v4 } else { &mut v6 };
                let address = address.to_string();
                if !servers.contains(&address) {
                    servers.push(address);
                }
            }
            Err(_) if word.is_empty() => {}
            Err(_) => return None,
        }
    }
    (!v4.is_empty() || !v6.is_empty()).then_some((v4, v6))
}

/// An IPv4 address with its prefix length, as `192.168.1.20/24`.
pub fn address_with_prefix(text: &str) -> Option<String> {
    let (address, prefix) = text.trim().split_once('/')?;
    let address: Ipv4Addr = address.parse().ok()?;
    let prefix: u8 = prefix.parse().ok()?;
    (1..=32)
        .contains(&prefix)
        .then(|| format!("{address}/{prefix}"))
}

pub fn ipv4(text: &str) -> Option<String> {
    text.trim()
        .parse::<Ipv4Addr>()
        .ok()
        .map(|address| address.to_string())
}

/// The connection's own hardware address on Wi-Fi
/// (`802-11-wireless.cloned-mac-address`).
#[derive(Clone, Debug, PartialEq, Eq)]
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
            "" => Mac::Default,
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
            Mac::Other(value) => value,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Mac::Default => "System default".into(),
            Mac::Permanent => "Off, the device's own".into(),
            Mac::Stable => "Private, fixed for this network".into(),
            Mac::Random => "Private, new each time".into(),
            Mac::Other(value) => format!("Set elsewhere ({value})"),
        }
    }
}

/// The settings the details page shows and changes, from `nmcli -t -f
/// FIELDS connection show uuid UUID`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub version: u64,
    pub name: String,
    pub uuid: String,
    pub kind: Option<Kind>,
    pub autoconnect: bool,
    /// `yes`, `no`, or NetworkManager's guess (`unknown`).
    pub metered: String,
    pub ipv4_method: String,
    pub ipv4_addresses: Vec<String>,
    pub ipv4_gateway: String,
    pub dns: Option<Dns>,
    pub dns_servers: Vec<String>,
    pub ipv4_dns: Vec<String>,
    pub ipv4_ignore_auto_dns: bool,
    pub ipv6_dns: Vec<String>,
    pub ipv6_ignore_auto_dns: bool,
    pub suspended_dns: Option<String>,
    pub ipv6_method: String,
    pub mac: String,
    pub hidden: bool,
    pub key_mgmt: String,
}

pub const SETTINGS_FIELDS: &str = "connection.id,connection.uuid,connection.type,\
connection.autoconnect,connection.metered,ipv4.method,ipv4.addresses,ipv4.gateway,\
ipv4.dns,ipv4.ignore-auto-dns,ipv6.method,ipv6.dns,ipv6.ignore-auto-dns,802-11-wireless.cloned-mac-address,\
802-11-wireless.hidden,802-11-wireless-security.key-mgmt";

pub fn settings(text: &str) -> Settings {
    let block = blocks(text).into_iter().next().unwrap_or_default();
    let value = |name| get(&block, name).unwrap_or("").to_owned();
    let ignore_auto = value("ipv4.ignore-auto-dns") == "yes";
    let mut settings = Settings {
        version: 0,
        name: value("connection.id"),
        uuid: value("connection.uuid"),
        kind: get(&block, "connection.type").map(Kind::of_connection),
        autoconnect: value("connection.autoconnect") == "yes",
        metered: value("connection.metered"),
        ipv4_method: value("ipv4.method"),
        ipv4_addresses: list(get(&block, "ipv4.addresses")),
        ipv4_gateway: value("ipv4.gateway"),
        dns: None,
        dns_servers: Vec::new(),
        ipv4_dns: list(get(&block, "ipv4.dns")),
        ipv4_ignore_auto_dns: ignore_auto,
        ipv6_dns: list(get(&block, "ipv6.dns")),
        ipv6_ignore_auto_dns: value("ipv6.ignore-auto-dns") == "yes",
        suspended_dns: None,
        ipv6_method: value("ipv6.method"),
        mac: value("802-11-wireless.cloned-mac-address"),
        hidden: value("802-11-wireless.hidden") == "yes",
        key_mgmt: value("802-11-wireless-security.key-mgmt"),
    };
    settings.refresh_dns();
    settings
}

impl Settings {
    pub fn refresh_dns(&mut self) {
        let v6 = if self.ipv6_off() {
            self.suspended_dns().ok().flatten().unwrap_or_else(|| self.ipv6_dns.clone())
        } else {
            self.ipv6_dns.clone()
        };
        self.dns_servers = self.ipv4_dns.iter().cloned().chain(v6).collect();
        self.dns = Some(Dns::of(self.ipv4_ignore_auto_dns, &self.dns_servers));
    }
}

/// Whether joining failed because the network refused the password (or
/// gave up waiting for one).
pub fn refused(error: &str) -> bool {
    error.contains("Secrets were required")
        || error.contains("802-1X supplicant")
        || error.contains("4-way handshake")
        || error.contains("Timeout")
}

/// What to tell someone when connecting failed, from nmcli's message.
pub fn reason(error: &str) -> String {
    if refused(error) {
        "Check the password and try again".into()
    } else if error.contains("No network with SSID") {
        "The network is out of range".into()
    } else {
        error.trim().trim_start_matches("Error: ").to_owned()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terse_fields_unescape_colons_and_backslashes() {
        assert_eq!(
            fields(r" :Cafe\: Open \\ Wi-Fi::100"),
            [" ", r"Cafe: Open \ Wi-Fi", "", "100"]
        );
    }

    #[test]
    fn blocks_keep_colons_in_values() {
        let text = "GENERAL.DEVICE:wlan0\nGENERAL.HWADDR:AA:BB:CC:DD:EE:FF\n\nGENERAL.DEVICE:lo\n";
        let blocks = blocks(text);
        assert_eq!(blocks.len(), 2);
        assert_eq!(get(&blocks[0], "GENERAL.HWADDR"), Some("AA:BB:CC:DD:EE:FF"));
    }

    #[test]
    fn devices_know_their_connection() {
        let text = "wlan0:wifi:connected:Home:c225:\nlo:loopback:connected (externally):lo:98a8\n\
                    p2p-dev-wlan0:wifi-p2p:disconnected::\nenp0s31f6:ethernet:unavailable::\n";
        let devices = devices(text);
        assert_eq!(devices[0].name, "wlan0");
        // The fifth field is the UUID; a stray sixth does not shift it.
        assert_eq!(devices[0].connection.as_deref(), Some("c225"));
        assert!(devices[1].connected());
        assert_eq!(devices[3].kind, Kind::Ethernet);
        assert_eq!(devices[3].connection, None);
    }

    #[test]
    fn devices_left_to_others_are_not_shown() {
        let snapshot = Snapshot {
            devices: devices(
                "eth0:ethernet:unmanaged::\nwlan0:wifi:unmanaged::\nenp1s0:ethernet:unavailable::\nwlan1:wifi:disconnected::\n",
            ),
            ..Snapshot::default()
        };
        let ethernet: Vec<&str> = snapshot.ethernet().map(|d| d.name.as_str()).collect();
        assert_eq!(ethernet, ["enp1s0"]);
        assert_eq!(
            snapshot.wifi_device().map(|d| d.name.as_str()),
            Some("wlan1")
        );
    }

    #[test]
    fn saved_connections_and_their_networks() {
        let list = "Home:aaaa:802-11-wireless:yes:yes:wlan0:activated:1791707418\n\
                    Office:bbbb:802-11-wireless:no:no:::0\n\
                    Work VPN:cccc:wireguard:no:no:::1700000000\n";
        let mut saved = saved(list);
        wifi_settings(
            &mut saved,
            "connection.uuid:aaaa\n802-11-wireless.ssid:Home: 5G\n802-11-wireless-security.key-mgmt:sae\n\n\
             connection.uuid:bbbb\n802-11-wireless.ssid:Office\n",
        );
        assert!(saved[0].active && saved[0].autoconnect);
        assert_eq!(saved[0].ssid.as_deref(), Some("Home: 5G"));
        assert_eq!(saved[0].security.as_deref(), Some("sae"));
        assert_eq!(saved[1].security, None);
        assert!(saved[2].kind.is_vpn());
        assert_eq!(saved[2].device, None);
    }

    #[test]
    fn networks_are_unique_and_strongest_first() {
        let list = " :Cafe::40\n :Home:WPA2:57\n*:Office:WPA2 WPA3:30\n :Home:WPA2:80\n :::90\n";
        let networks = networks(list);
        let names: Vec<&str> = networks.iter().map(|n| n.ssid.as_str()).collect();
        assert_eq!(names, ["Home", "Cafe", "Office"]);
        assert_eq!(networks[0].signal, 80);
        assert!(networks[2].in_use);
    }

    #[test]
    fn security_decides_how_to_join() {
        let of = Security::from_advertised;
        assert_eq!(of(""), Security::Open);
        assert_eq!(of("OWE"), Security::Open);
        assert_eq!(of("WPA1 WPA2"), Security::Personal("wpa-psk"));
        assert_eq!(of("WPA2 WPA3"), Security::Personal("wpa-psk"));
        assert_eq!(of("WPA3"), Security::Personal("sae"));
        assert_eq!(of("WPA2 802.1X"), Security::Enterprise);
        assert_eq!(of("WEP"), Security::Wep);
    }

    #[test]
    fn signal_comes_in_four_steps() {
        assert_eq!(
            [bars(100), bars(75), bars(74), bars(30), bars(5)],
            [4, 4, 3, 2, 1]
        );
    }

    #[test]
    fn device_addresses_and_servers() {
        let text = "GENERAL.DEVICE:wlan0\nGENERAL.HWADDR:AA:BB:CC:DD:EE:D0\n\
                    IP4.ADDRESS[1]:192.168.1.164/24\nIP4.GATEWAY:192.168.1.1\nIP4.DNS[1]:1.1.1.3\n\
                    IP6.ADDRESS[1]:fe80::1/64\nIP6.DNS[1]:2606:4700::1113\n\n\
                    GENERAL.DEVICE:lo\nIP4.ADDRESS[1]:127.0.0.1/8\nIP4.GATEWAY:\n";
        let all = addresses(text);
        let wifi = &all["wlan0"];
        assert_eq!(wifi.address(), Some("192.168.1.164"));
        assert_eq!(wifi.router, "192.168.1.1");
        assert_eq!(wifi.dns, ["1.1.1.3", "2606:4700::1113"]);
        assert_eq!(wifi.hardware, "AA:BB:CC:DD:EE:D0");
        assert_eq!(all["lo"].router, "");
    }

    #[test]
    fn the_network_in_use_comes_from_its_device() {
        let snapshot = Snapshot {
            devices: devices("wlan0:wifi:connected:Home:aaaa\n"),
            saved: {
                let mut saved = saved("Home:aaaa:802-11-wireless:yes:yes:wlan0:activated:5\n");
                wifi_settings(
                    &mut saved,
                    "connection.uuid:aaaa\n802-11-wireless.ssid:Home\n",
                );
                saved
            },
            networks: networks("*:Home:WPA2:70\n :Cafe::90\n"),
            ..Snapshot::default()
        };
        let (network, saved) = snapshot.current_wifi().unwrap();
        assert_eq!(
            (network.ssid.as_str(), saved.uuid.as_str()),
            ("Home", "aaaa")
        );
        assert_eq!(
            snapshot.saved_for("Home").map(|s| s.uuid.as_str()),
            Some("aaaa")
        );
        assert!(snapshot.saved_for("Cafe").is_none());
    }

    #[test]
    fn settings_of_a_connection() {
        let text = "connection.id:Home\nconnection.uuid:aaaa\nconnection.type:802-11-wireless\n\
                    connection.autoconnect:yes\nconnection.metered:unknown\nipv4.method:manual\n\
                    ipv4.addresses:192.168.1.20/24\nipv4.gateway:192.168.1.1\nipv4.dns:1.1.1.1,1.0.0.1\n\
                    ipv4.ignore-auto-dns:yes\nipv6.method:auto\nipv6.dns:\n\
                    802-11-wireless.cloned-mac-address:stable\n802-11-wireless.hidden:no\n";
        let settings = settings(text);
        assert_eq!(settings.kind, Some(Kind::Wifi));
        assert_eq!(settings.ipv4_addresses, ["192.168.1.20/24"]);
        assert_eq!(settings.dns, Some(Dns::Cloudflare));
        assert_eq!(Mac::of(&settings.mac), Mac::Stable);
        let automatic = super::settings("ipv4.dns:\nipv4.ignore-auto-dns:no\n");
        assert_eq!(automatic.dns, Some(Dns::Automatic));
        let custom = super::settings("ipv4.dns:9.9.9.9\nipv4.ignore-auto-dns:no\n");
        assert_eq!(custom.dns, Some(Dns::Custom));
    }

    #[test]
    fn typed_addresses_are_checked() {
        assert_eq!(
            address_with_prefix(" 192.168.1.20/24 ").as_deref(),
            Some("192.168.1.20/24")
        );
        assert_eq!(address_with_prefix("192.168.1.20"), None);
        assert_eq!(address_with_prefix("192.168.1.300/24"), None);
        assert_eq!(address_with_prefix("192.168.1.20/33"), None);
        assert_eq!(ipv4("10.0.0.1").as_deref(), Some("10.0.0.1"));
        assert_eq!(ipv4("router"), None);
        assert_eq!(
            custom_servers("9.9.9.9, 2620:fe::fe"),
            Some((vec!["9.9.9.9".into()], vec!["2620:fe::fe".into()]))
        );
        assert_eq!(custom_servers("dns.quad9.net"), None);
        assert_eq!(
            custom_servers("9.9.9.9 9.9.9.9 2620:00fe::fe 2620:fe::fe"),
            Some((vec!["9.9.9.9".into()], vec!["2620:fe::fe".into()]))
        );
    }

    #[test]
    fn failures_read_plainly() {
        assert_eq!(
            reason("Error: Connection activation failed: Secrets were required, but not provided"),
            "Check the password and try again"
        );
        assert_eq!(
            reason("Error: No network with SSID 'X' found."),
            "The network is out of range"
        );
        assert_eq!(reason("Error: something else\n"), "something else");
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
