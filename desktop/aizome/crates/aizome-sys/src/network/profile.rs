//! A saved connection read whole, for a details page ([`Profile`]), and
//! the changes Settings makes to one besides IPv4 and DNS: IPv6, the
//! private hardware address, and applying changes to the connection in
//! use without reconnecting (`Device.Reapply`).

use super::model::{ConnectionKind, ConnectionSettings, EditableSettings, Metered, Proxy};
use super::{Network, connection_path, update_settings};
use crate::bus::{Opts, Props, PropsExt, unwrap_variant, value_bytes, value_dicts, value_str, value_u64};
use crate::error::Error;
use crate::service::Task;
use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};
use zbus::zvariant::Value;

const NM: &str = "org.freedesktop.NetworkManager";
const IFACE_DEVICE: &str = "org.freedesktop.NetworkManager.Device";

/// Everything a details page shows of a saved connection (no secrets).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    pub id: String,
    pub uuid: String,
    pub kind: ConnectionKind,
    pub autoconnect: bool,
    /// `connection.metered`: yes, no, or NetworkManager's guess.
    pub metered: Metered,
    pub interface_name: Option<String>,
    /// `auto`, `manual`, `link-local`, `shared`, `disabled`.
    pub ipv4_method: String,
    /// Fixed addresses with their prefix length, as `192.168.1.20/24`.
    pub ipv4_addresses: Vec<String>,
    pub ipv4_gateway: Option<String>,
    /// The connection's own DNS servers (not the network's).
    pub ipv4_dns: Vec<String>,
    /// The network's DNS servers are ignored for the connection's own.
    pub ipv4_ignore_auto_dns: bool,
    /// `auto`, `dhcp`, `manual`, `link-local`, `ignore`, `disabled`.
    pub ipv6_method: String,
    pub ipv6_addresses: Vec<String>,
    pub ipv6_dns: Vec<String>,
    pub ipv6_ignore_auto_dns: bool,
    /// Wi-Fi: the network's name.
    pub ssid: Option<String>,
    pub hidden: bool,
    /// Wi-Fi: `wpa-psk`, `sae`, `wpa-eap`, `owe`, `none` (WEP).
    pub key_mgmt: Option<String>,
    /// The hardware address it uses: empty (the system's default),
    /// `permanent`, `stable`, `random`, `preserve`, or an address.
    pub assigned_mac: String,
    /// 802.1X: the account name, and the outer method (`peap`, `ttls`).
    pub eap_identity: Option<String>,
    pub eap_method: Option<String>,
    pub vpn_service: Option<String>,
    pub proxy: Proxy,
}

impl Profile {
    /// The DNS servers it sets, IPv4 then IPv6.
    pub fn dns(&self) -> Vec<String> {
        self.ipv4_dns.iter().chain(&self.ipv6_dns).cloned().collect()
    }

    /// Whether it carries IP settings (Wi-Fi and wired, not VPNs' own).
    pub fn has_ip(&self) -> bool {
        matches!(self.kind, ConnectionKind::Wifi | ConnectionKind::Ethernet)
    }
}

/// Read a profile from `GetSettings`' answer.
pub fn read_profile(settings: &ConnectionSettings) -> Profile {
    let empty = Props::new();
    let group = |name: &str| settings.get(name).unwrap_or(&empty);
    let connection = group("connection");
    let ipv4 = group("ipv4");
    let ipv6 = group("ipv6");
    let wireless = settings.get("802-11-wireless");
    let wired = settings.get("802-3-ethernet");
    let security = settings.get("802-11-wireless-security");
    let eap = settings.get("802-1x");
    let proxy = settings.get("proxy");
    let mac = wireless.or(wired).and_then(|g| g.opt_str("assigned-mac-address")).unwrap_or_default();
    Profile {
        id: connection.str("id"),
        uuid: connection.str("uuid"),
        kind: ConnectionKind::from_type(&connection.str("type")),
        autoconnect: connection.opt_bool("autoconnect").unwrap_or(true),
        metered: match connection.i64("metered") {
            1 => Metered::Yes,
            2 => Metered::No,
            _ => Metered::Unknown,
        },
        interface_name: connection.opt_str("interface-name").filter(|i| !i.is_empty()),
        ipv4_method: ipv4.opt_str("method").unwrap_or_else(|| "auto".into()),
        ipv4_addresses: addresses(ipv4, false),
        ipv4_gateway: ipv4.opt_str("gateway").filter(|g| !g.is_empty()),
        ipv4_dns: dns(ipv4, false),
        ipv4_ignore_auto_dns: ipv4.bool("ignore-auto-dns"),
        ipv6_method: ipv6.opt_str("method").unwrap_or_else(|| "auto".into()),
        ipv6_addresses: addresses(ipv6, true),
        ipv6_dns: dns(ipv6, true),
        ipv6_ignore_auto_dns: ipv6.bool("ignore-auto-dns"),
        ssid: wireless.map(|w| w.bytestring("ssid")).filter(|s| !s.is_empty()),
        hidden: wireless.is_some_and(|w| w.bool("hidden")),
        key_mgmt: security.and_then(|s| s.opt_str("key-mgmt")),
        assigned_mac: mac,
        eap_identity: eap.and_then(|e| e.opt_str("identity")).filter(|i| !i.is_empty()),
        eap_method: eap.and_then(|e| e.strings("eap").into_iter().next()),
        vpn_service: settings.get("vpn").and_then(|v| v.opt_str("service-type")),
        proxy: Proxy {
            automatic: proxy.is_some_and(|p| p.i64("method") == 1),
            pac_url: proxy.and_then(|p| p.opt_str("pac-url")).filter(|u| !u.is_empty()),
            browser_only: proxy.is_some_and(|p| p.bool("browser-only")),
        },
    }
}

/// `address-data` as `a.b.c.d/p`, else the old array form.
fn addresses(props: &Props, v6: bool) -> Vec<String> {
    let data: Vec<String> = props
        .value("address-data")
        .map(value_dicts)
        .unwrap_or_default()
        .iter()
        .filter_map(|d| Some(format!("{}/{}", d.opt_str("address")?, d.u32("prefix"))))
        .collect();
    if !data.is_empty() || v6 {
        return data;
    }
    // ipv4.addresses: aau of [address, prefix, gateway], network order.
    match props.value("addresses") {
        Some(Value::Array(rows)) => rows
            .iter()
            .filter_map(|row| match unwrap_variant(row) {
                Value::Array(cols) => {
                    let n: Vec<u64> = cols.iter().filter_map(value_u64).collect();
                    let address = u32::try_from(*n.first()?).ok()?;
                    Some(format!("{}/{}", Ipv4Addr::from(address.to_ne_bytes()), n.get(1).copied().unwrap_or(0)))
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `dns-data` where NetworkManager sends it, else `dns` (`au` in network
/// order for IPv4, `aay` for IPv6).
fn dns(props: &Props, v6: bool) -> Vec<String> {
    let data = props.strings("dns-data");
    if !data.is_empty() {
        return data;
    }
    match props.value("dns") {
        Some(Value::Array(array)) if v6 => array
            .iter()
            .filter_map(|b| {
                let bytes: [u8; 16] = value_bytes(b).try_into().ok()?;
                Some(Ipv6Addr::from(bytes).to_string())
            })
            .collect(),
        Some(Value::Array(array)) => array
            .iter()
            .filter_map(|n| {
                let n = u32::try_from(value_u64(n)?).ok()?;
                Some(Ipv4Addr::from(n.to_ne_bytes()).to_string())
            })
            .collect(),
        Some(other) => value_str(other).into_iter().collect(),
        None => Vec::new(),
    }
}

/// IPv6 methods NetworkManager takes.
pub const IPV6_METHODS: [&str; 6] = ["auto", "dhcp", "manual", "link-local", "ignore", "disabled"];

/// Set the IPv6 method in `settings`; leaving `manual`, its addresses go.
pub(crate) fn apply_ipv6_method(settings: &mut EditableSettings, method: &str) {
    let ipv6 = settings.entry("ipv6".to_owned()).or_default();
    if method != "manual" {
        ipv6.remove("addresses");
        ipv6.remove("address-data");
        ipv6.remove("gateway");
    }
    ipv6.insert("method".to_owned(), Value::from(method.to_owned()));
}

/// Set the hardware address a Wi-Fi or wired connection uses; empty for
/// the system's default. The old byte-array form goes, as it would
/// override the new one.
pub(crate) fn apply_assigned_mac(settings: &mut EditableSettings, value: &str) {
    let group = if settings.contains_key("802-3-ethernet") && !settings.contains_key("802-11-wireless") {
        "802-3-ethernet"
    } else {
        "802-11-wireless"
    };
    let props = settings.entry(group.to_owned()).or_default();
    props.remove("cloned-mac-address");
    if value.is_empty() {
        props.remove("assigned-mac-address");
    } else {
        props.insert("assigned-mac-address".to_owned(), Value::from(value.to_owned()));
    }
}

impl Network {
    /// A saved connection read whole, for a details page.
    pub fn profile(&self, uuid: &str) -> Task<Profile> {
        let task = self.connection_settings(uuid);
        let wake = self.model.wake();
        Task::spawn("profile", &wake, move || task.wait().map(|s| read_profile(&s)))
    }

    /// How a saved connection gets its IPv6 addresses: one of
    /// [`IPV6_METHODS`].
    pub fn set_ipv6_method(&self, uuid: &str, method: &str) -> Task<()> {
        if !IPV6_METHODS.contains(&method) {
            return Task::ready(Err(Error::InvalidArgument(format!("Unknown IPv6 method {method}"))));
        }
        let (uuid, method) = (uuid.to_owned(), method.to_owned());
        self.task(move |bus| update_settings(bus, &uuid, |s| apply_ipv6_method(s, &method)))
    }

    /// The hardware address a connection uses: `""` (the system's
    /// default), `permanent`, `stable`, `random`, or an address. It takes
    /// effect the next time the connection comes up.
    pub fn set_private_address(&self, uuid: &str, value: &str) -> Task<()> {
        let (uuid, value) = (uuid.to_owned(), value.to_owned());
        self.task(move |bus| update_settings(bus, &uuid, |s| apply_assigned_mac(s, &value)))
    }

    /// Apply a connection's saved settings to the device it's up on,
    /// without reconnecting: `true` when done, `false` when it isn't up or
    /// the change needs it to connect again (NetworkManager can't reapply
    /// everything, such as the hardware address).
    pub fn reapply(&self, uuid: &str) -> Task<bool> {
        let device = self.snapshot().active_connection(uuid).and_then(|a| a.devices.first().cloned());
        let uuid = uuid.to_owned();
        self.task(move |bus| {
            let Some(device) = device else { return Ok(false) };
            let _ = connection_path(bus, &uuid)?;
            let empty: HashMap<String, HashMap<String, Value>> = HashMap::new();
            match bus.call_with::<_, ()>(Opts::ACTION, NM, &device, IFACE_DEVICE, "Reapply", &(empty, 0u64, 0u32)) {
                Ok(()) => Ok(true),
                Err(Error::NotAuthorized(m)) => Err(Error::NotAuthorized(m)),
                Err(error) => {
                    log::info!("aizome-sys: reapplying {uuid} on {device}: {error}");
                    Ok(false)
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::OwnedValue;

    fn v(value: impl Into<Value<'static>>) -> OwnedValue {
        OwnedValue::try_from(value.into()).unwrap()
    }

    #[test]
    fn a_profile_reads_every_part() {
        let settings: ConnectionSettings = HashMap::from([
            (
                "connection".to_owned(),
                HashMap::from([
                    ("id".to_owned(), v("Home")),
                    ("uuid".to_owned(), v("u-1")),
                    ("type".to_owned(), v("802-11-wireless")),
                    ("autoconnect".to_owned(), v(false)),
                    ("metered".to_owned(), v(1i32)),
                ]),
            ),
            (
                "ipv4".to_owned(),
                HashMap::from([
                    ("method".to_owned(), v("manual")),
                    (
                        "address-data".to_owned(),
                        v(vec![HashMap::from([
                            ("address".to_owned(), Value::from("192.168.1.20")),
                            ("prefix".to_owned(), Value::from(24u32)),
                        ])]),
                    ),
                    ("gateway".to_owned(), v("192.168.1.1")),
                    ("dns".to_owned(), v(vec![u32::from_ne_bytes([1, 1, 1, 1])])),
                    ("ignore-auto-dns".to_owned(), v(true)),
                ]),
            ),
            (
                "ipv6".to_owned(),
                HashMap::from([
                    ("method".to_owned(), v("disabled")),
                    (
                        "dns".to_owned(),
                        v(vec![Ipv6Addr::new(0x2606, 0x4700, 0x4700, 0, 0, 0, 0, 0x1111).octets().to_vec()]),
                    ),
                ]),
            ),
            (
                "802-11-wireless".to_owned(),
                HashMap::from([
                    ("ssid".to_owned(), v(b"Home".to_vec())),
                    ("assigned-mac-address".to_owned(), v("stable")),
                ]),
            ),
            ("802-11-wireless-security".to_owned(), HashMap::from([("key-mgmt".to_owned(), v("sae"))])),
        ]);
        let p = read_profile(&settings);
        assert_eq!(p.id, "Home");
        assert_eq!(p.kind, ConnectionKind::Wifi);
        assert!(!p.autoconnect);
        assert_eq!(p.metered, Metered::Yes);
        assert_eq!(p.ipv4_method, "manual");
        assert_eq!(p.ipv4_addresses, ["192.168.1.20/24"]);
        assert_eq!(p.ipv4_gateway.as_deref(), Some("192.168.1.1"));
        assert_eq!(p.dns(), ["1.1.1.1", "2606:4700:4700::1111"]);
        assert!(p.ipv4_ignore_auto_dns);
        assert_eq!(p.ipv6_method, "disabled");
        assert_eq!(p.ssid.as_deref(), Some("Home"));
        assert_eq!(p.key_mgmt.as_deref(), Some("sae"));
        assert_eq!(p.assigned_mac, "stable");
        assert!(p.has_ip());
    }

    #[test]
    fn ipv6_and_private_addresses_edit_the_right_keys() {
        let mut s: EditableSettings = HashMap::from([
            (
                "ipv6".to_owned(),
                HashMap::from([
                    ("method".to_owned(), Value::from("manual")),
                    ("address-data".to_owned(), Value::from(Vec::<String>::new())),
                ]),
            ),
            (
                "802-11-wireless".to_owned(),
                HashMap::from([("cloned-mac-address".to_owned(), Value::from(vec![2u8, 0, 0, 0, 0, 1]))]),
            ),
        ]);
        apply_ipv6_method(&mut s, "disabled");
        assert_eq!(s["ipv6"]["method"], Value::from("disabled"));
        assert!(!s["ipv6"].contains_key("address-data"));
        apply_assigned_mac(&mut s, "random");
        assert!(!s["802-11-wireless"].contains_key("cloned-mac-address"));
        assert_eq!(s["802-11-wireless"]["assigned-mac-address"], Value::from("random"));
        apply_assigned_mac(&mut s, "");
        assert!(!s["802-11-wireless"].contains_key("assigned-mac-address"));
    }
}
