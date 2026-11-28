// SPDX-License-Identifier: GPL-3.0-or-later
//! What NetworkManager has running. The guard decides from the profile
//! NetworkManager applied to each WireGuard device and from that device's
//! transport mark. It reads nothing from the kernel's routing tables, so a
//! tunnel made outside NetworkManager, or a profile that routes through its
//! own table, rules or gateway, is left alone.
use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU32;
use std::process::Command;
use std::thread;
use std::time::Duration;

use ipnet::IpNet;
use serde::{Deserialize, de::DeserializeOwned};

use crate::family::Family;

const BUS: &str = "org.freedesktop.DBus";
const NM: &str = "org.freedesktop.NetworkManager";
const ROOT: &str = "/org/freedesktop/NetworkManager";
const PROPS: &str = "org.freedesktop.DBus.Properties";
const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const WIREGUARD: &str = "org.freedesktop.NetworkManager.Device.WireGuard";

// NMActiveConnectionState and NMActivationStateFlags.
const ACTIVATING: u32 = 1;
const ACTIVATED: u32 = 2;
const EXTERNAL: u32 = 0x80;

// NMDeviceState. NetworkManager runs the dispatcher's pre-up for a device
// as it enters IP_CHECK, with the device's addresses and routes in place,
// and the device is ACTIVATED once every pre-up script has finished.
const DEVICE_IP_CHECK: u32 = 80;
const DEVICE_ACTIVATED: u32 = 100;

// A stale listing of active connections is taken again, up to this many
// observations in all.
const OBSERVATIONS: u32 = 4;
const BETWEEN_OBSERVATIONS: Duration = Duration::from_millis(50);

/// A WireGuard tunnel NetworkManager has up, or is bringing up and has
/// reached its pre-up.
#[derive(Debug, PartialEq, Eq)]
pub struct Tunnel {
    /// The profile's name, for the status display.
    pub name: String,
    /// The families whose every public address the profile sends into the
    /// tunnel.
    pub carries: BTreeSet<Family>,
    /// The mark on the tunnel's own encrypted packets, when the device sets
    /// one. It identifies them wherever the server is.
    pub mark: Option<NonZeroU32>,
    /// The servers the profile gives by address. An unmarked tunnel's own
    /// packets can only be recognized by these.
    pub servers: Vec<SocketAddr>,
}

/// One method call. busctl prints a method's output arguments as a JSON array
/// in `data`, which `R` names as a tuple.
fn call<R: DeserializeOwned>(
    service: &str,
    path: &str,
    iface: &str,
    method: &str,
    args: &[&str],
) -> Result<R, String> {
    #[derive(Deserialize)]
    struct Reply<R> {
        data: R,
    }
    let busctl = option_env!("SEKKA_VPN_GUARD_BUSCTL").ok_or("built without the busctl path")?;
    let result = Command::new(busctl)
        .args([
            "--system",
            "--json=short",
            "--auto-start=no",
            "--timeout=3s",
            "call",
            service,
            path,
            iface,
            method,
        ])
        .args(args)
        .output()
        .map_err(|e| format!("{service} {method}: {e}"))?;
    if !result.status.success() {
        return Err(format!(
            "{service} {method}: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    serde_json::from_slice::<Reply<R>>(&result.stdout)
        .map(|reply| reply.data)
        .map_err(|e| format!("{service} {method} reply: {e}"))
}

/// A D-Bus variant as busctl prints it, with the value in `data`.
#[derive(Deserialize, Default)]
struct Variant<T> {
    data: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Active {
    #[serde(rename = "Type")]
    kind: Variant<String>,
    state: Variant<u32>,
    state_flags: Variant<u32>,
    devices: Variant<Vec<String>>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Connection {
    id: Variant<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct Stack {
    method: Variant<String>,
    never_default: Variant<bool>,
    route_table: Variant<u32>,
    routing_rules: Variant<Vec<serde_json::Value>>,
    gateway: Variant<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct Peer {
    endpoint: Variant<String>,
    allowed_ips: Variant<Vec<String>>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct Wireguard {
    peer_routes: Option<Variant<bool>>,
    ip4_auto_default_route: Option<Variant<i32>>,
    ip6_auto_default_route: Option<Variant<i32>>,
    peers: Variant<Vec<Peer>>,
}

#[derive(Deserialize)]
struct Profile {
    #[serde(default)]
    connection: Connection,
    wireguard: Wireguard,
    #[serde(default)]
    ipv4: Stack,
    #[serde(default)]
    ipv6: Stack,
}

impl Profile {
    fn tunnel(self, mark: u32) -> Result<Tunnel, String> {
        let mark = NonZeroU32::new(mark);
        let peers = &self.wireguard.peers.data;
        // A host name, or no endpoint at all, has no address in the profile.
        let servers: Vec<SocketAddr> = peers
            .iter()
            .filter_map(|peer| peer.endpoint.data.parse().ok())
            .collect();
        // Without a mark, the tunnel's own packets are let out by server
        // address. With a server whose address the profile does not give
        // there is nothing to write, and a block could cut the tunnel off
        // from it. Such a tunnel is left alone.
        let carries = if mark.is_some() || servers.len() == peers.len() {
            self.carries(&servers)?
        } else {
            BTreeSet::new()
        };
        Ok(Tunnel {
            name: self.connection.id.data,
            carries,
            mark,
            servers,
        })
    }

    fn carries(&self, servers: &[SocketAddr]) -> Result<BTreeSet<Family>, String> {
        let wg = &self.wireguard;
        let mut carried = BTreeSet::new();
        if wg.peer_routes.as_ref().is_some_and(|v| !v.data) {
            return Ok(carried);
        }
        let mut allowed = Vec::new();
        for peer in &wg.peers.data {
            for entry in &peer.allowed_ips.data {
                allowed.push(allowed_ip(entry)?);
            }
        }
        for (family, stack, automatic) in [
            (Family::V4, &self.ipv4, &wg.ip4_auto_default_route),
            (Family::V6, &self.ipv6, &wg.ip6_auto_default_route),
        ] {
            // A profile with its own table, rules or gateway routes by hand.
            // What it carries cannot be read from its AllowedIPs.
            if !matches!(stack.method.data.as_str(), "auto" | "manual" | "dhcp")
                || stack.never_default.data
                || !matches!(stack.route_table.data, 0 | 254)
                || !stack.routing_rules.data.is_empty()
                || !stack.gateway.data.is_empty()
            {
                continue;
            }
            let mut nets: Vec<IpNet> = allowed
                .iter()
                .copied()
                .filter(|net| Family::of(net.addr()) == family)
                // A /0 whose automatic default route is switched off lands in
                // the main table, where the physical default can outrank it.
                .filter(|net| {
                    net.prefix_len() != 0 || !automatic.as_ref().is_some_and(|v| v.data == 0)
                })
                .collect();
            if nets.is_empty() {
                continue;
            }
            // A list written to leave out private addresses, or the tunnel's
            // own server, still sends every public address into the tunnel.
            nets.extend(family.not_public());
            nets.extend(
                servers
                    .iter()
                    .map(|server| IpNet::from(server.ip()))
                    .filter(|net| Family::of(net.addr()) == family),
            );
            if IpNet::aggregate(&nets)
                .iter()
                .any(|net| net.prefix_len() == 0)
            {
                carried.insert(family);
            }
        }
        Ok(carried)
    }
}

/// One AllowedIPs entry as NetworkManager keeps it: a prefix, or an address
/// alone. A wg-quick file may give an address without a prefix length,
/// which WireGuard reads as that one host, and NetworkManager stores the
/// entry as it was written.
fn allowed_ip(entry: &str) -> Result<IpNet, String> {
    entry
        .parse::<IpNet>()
        .map(|net| net.trunc())
        .or_else(|_| entry.parse::<IpAddr>().map(IpNet::from))
        .map_err(|_| format!("invalid NetworkManager AllowedIP {entry}"))
}

/// Whether a connection in `active` state, on a device in `device` state,
/// has its routes in place: activated, or activating and past the point
/// where the dispatcher's pre-up for it runs. Before that point an
/// activation can still fail, and NetworkManager sends no down event for
/// a device that never came up, so a block set for it would stay.
fn routed(active: u32, device: u32) -> bool {
    active == ACTIVATED
        || (active == ACTIVATING && (DEVICE_IP_CHECK..=DEVICE_ACTIVATED).contains(&device))
}

/// The WireGuard tunnels behind one active connection.
fn tunnels_of(path: &str) -> Result<Vec<Tunnel>, String> {
    let (active,): (Active,) = call(NM, path, PROPS, "GetAll", &["s", ACTIVE])?;
    // A device NetworkManager only observes is someone else's tunnel.
    if active.kind.data != "wireguard"
        || !matches!(active.state.data, ACTIVATING | ACTIVATED)
        || active.state_flags.data & EXTERNAL != 0
    {
        return Ok(Vec::new());
    }
    let mut tunnels = Vec::new();
    for device in &active.devices.data {
        let state = if active.state.data == ACTIVATED {
            DEVICE_ACTIVATED
        } else {
            let (state,): (Variant<u32>,) =
                call(NM, device, PROPS, "Get", &["ss", DEVICE, "State"])?;
            state.data
        };
        if !routed(active.state.data, state) {
            continue;
        }
        // The applied profile is what the device runs. The saved one may
        // have been edited since without being applied.
        let (profile, _version): (Profile, u64) =
            call(NM, device, DEVICE, "GetAppliedConnection", &["u", "0"])?;
        let (mark,): (Variant<u32>,) =
            call(NM, device, PROPS, "Get", &["ss", WIREGUARD, "FwMark"])?;
        tunnels.push(profile.tunnel(mark.data)?);
    }
    Ok(tunnels)
}

/// The WireGuard tunnels NetworkManager is running, or `None` when
/// NetworkManager is not on the bus.
pub fn tunnels() -> Result<Option<Vec<Tunnel>>, String> {
    // Asked of the bus itself, so an absent NetworkManager is an answer
    // and not a failure to interpret.
    let (running,): (bool,) = call(
        BUS,
        "/org/freedesktop/DBus",
        BUS,
        "NameHasOwner",
        &["s", NM],
    )?;
    if !running {
        return Ok(None);
    }
    // NetworkManager drops a connection it has taken down a moment after
    // listing it, often while the next one is coming up. A read that fails
    // after a listing that worked means the listing went stale, so the whole
    // observation is taken again. It is never accepted in part.
    let mut failure = String::new();
    for attempt in 0..OBSERVATIONS {
        if attempt > 0 {
            thread::sleep(BETWEEN_OBSERVATIONS);
        }
        let (paths,): (Variant<Vec<String>>,) =
            call(NM, ROOT, PROPS, "Get", &["ss", NM, "ActiveConnections"])?;
        let found: Result<Vec<Vec<Tunnel>>, String> =
            paths.data.iter().map(|path| tunnels_of(path)).collect();
        match found {
            Ok(found) => return Ok(Some(found.into_iter().flatten().collect())),
            Err(e) => failure = e,
        }
    }
    Err(failure)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const SERVER: &str = "203.0.113.20:51820";
    const NAMED: &str = "vpn.example:51820";

    fn profile(peers: &[(&str, &[&str])]) -> Value {
        let peers: Vec<Value> = peers
            .iter()
            .map(|(endpoint, allowed)| {
                json!({
                    "endpoint": {"type": "s", "data": endpoint},
                    "allowed-ips": {"type": "as", "data": allowed},
                })
            })
            .collect();
        json!({
            "connection": {"id": {"type": "s", "data": "GLADOS"}},
            "wireguard": {"peers": {"type": "aa{sv}", "data": peers}},
            "ipv4": {"method": {"type": "s", "data": "manual"}},
            "ipv6": {"method": {"type": "s", "data": "manual"}},
        })
    }

    fn tunnel(profile: Value, mark: u32) -> Tunnel {
        serde_json::from_value::<Profile>(profile)
            .unwrap()
            .tunnel(mark)
            .unwrap()
    }

    fn carries(profile: Value, mark: u32) -> Vec<Family> {
        tunnel(profile, mark).carries.into_iter().collect()
    }

    /// Every prefix of the address's family except the address itself.
    fn all_but(address: &str) -> Vec<String> {
        let mut rest = Vec::new();
        let mut child = IpNet::from(address.parse::<std::net::IpAddr>().unwrap());
        while let Some(parent) = child.supernet() {
            let siblings = parent.subnets(child.prefix_len()).unwrap();
            rest.extend(
                siblings
                    .filter(|net| *net != child)
                    .map(|net| net.to_string()),
            );
            child = parent;
        }
        rest
    }

    #[test]
    fn a_default_or_peers_that_add_up_to_one_carry_the_family() {
        assert_eq!(
            carries(profile(&[(SERVER, &["0.0.0.0/0"])]), 7),
            [Family::V4]
        );
        let halves = profile(&[
            (SERVER, &["0.0.0.0/1"]),
            (SERVER, &["128.0.0.0/1", "2001:db8::/32"]),
        ]);
        assert_eq!(carries(halves, 7), [Family::V4]);
        assert_eq!(
            carries(profile(&[(SERVER, &["0.0.0.0/0", "::/0"])]), 7),
            [Family::V4, Family::V6]
        );
        // All of global unicast is all of the public IPv6 internet.
        assert_eq!(
            carries(profile(&[(SERVER, &["2000::/3"])]), 7),
            [Family::V6]
        );
    }

    #[test]
    fn a_list_that_leaves_out_private_ranges_and_its_server_carries_the_family() {
        // What the WireGuard Android app writes for "Exclude private IPs".
        const PUBLIC: &[&str] = &[
            "0.0.0.0/5",
            "8.0.0.0/7",
            "11.0.0.0/8",
            "12.0.0.0/6",
            "16.0.0.0/4",
            "32.0.0.0/3",
            "64.0.0.0/2",
            "128.0.0.0/3",
            "160.0.0.0/5",
            "168.0.0.0/6",
            "172.0.0.0/12",
            "172.32.0.0/11",
            "172.64.0.0/10",
            "172.128.0.0/9",
            "173.0.0.0/8",
            "174.0.0.0/7",
            "176.0.0.0/4",
            "192.0.0.0/9",
            "192.128.0.0/11",
            "192.160.0.0/13",
            "192.169.0.0/16",
            "192.170.0.0/15",
            "192.172.0.0/14",
            "192.176.0.0/12",
            "192.192.0.0/10",
            "193.0.0.0/8",
            "194.0.0.0/7",
            "196.0.0.0/6",
            "200.0.0.0/5",
            "208.0.0.0/4",
        ];
        assert_eq!(carries(profile(&[(SERVER, PUBLIC)]), 7), [Family::V4]);

        let rest = all_but("203.0.113.20");
        let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
        assert_eq!(carries(profile(&[(SERVER, &rest)]), 0), [Family::V4]);
        // The hole is some other public address: a split tunnel.
        assert!(carries(profile(&[("198.51.100.7:51820", &rest)]), 0).is_empty());
        assert!(carries(profile(&[(SERVER, &["10.0.0.0/8", "192.0.2.0/24"])]), 7).is_empty());
    }

    #[test]
    fn an_unmarked_tunnel_needs_every_server_by_address() {
        let halves: &[&str] = &["0.0.0.0/1", "128.0.0.0/1"];
        let by_address = tunnel(profile(&[("[2001:db8::20]:51820", halves)]), 0);
        assert_eq!(by_address.name, "GLADOS");
        assert_eq!(by_address.mark, None);
        let server: SocketAddr = "[2001:db8::20]:51820".parse().unwrap();
        assert_eq!(by_address.servers, [server]);
        assert!(by_address.carries.contains(&Family::V4));

        assert!(carries(profile(&[(NAMED, halves)]), 0).is_empty());
        assert!(carries(profile(&[("", halves)]), 0).is_empty());
        let marked = tunnel(profile(&[(NAMED, halves)]), 51820);
        assert_eq!(marked.mark, NonZeroU32::new(51820));
        assert!(marked.servers.is_empty());
        assert!(marked.carries.contains(&Family::V4));
    }

    #[test]
    fn an_address_without_a_prefix_length_is_one_host() {
        // As NetworkManager keeps a wg-quick file's `AllowedIPs = 10.8.0.1`.
        assert_eq!(allowed_ip("10.8.0.1"), Ok("10.8.0.1/32".parse().unwrap()));
        assert_eq!(
            allowed_ip("2001:db8::1"),
            Ok("2001:db8::1/128".parse().unwrap())
        );
        assert_eq!(
            allowed_ip("10.8.0.1/24"),
            Ok("10.8.0.0/24".parse().unwrap())
        );
        assert!(allowed_ip("10.8.0.1/33").is_err());
        assert!(allowed_ip("vpn.example").is_err());
        // A split tunnel listing hosts alone is read, not refused, so it
        // can't keep a full tunnel beside it from being counted.
        assert!(carries(profile(&[(SERVER, &["192.168.200.1", "fd00::1"])]), 7).is_empty());
        assert_eq!(
            carries(profile(&[(SERVER, &["0.0.0.0/0", "fd00::1"])]), 7),
            [Family::V4]
        );
    }

    #[test]
    fn an_activating_tunnel_counts_from_its_pre_up() {
        for device in [30, 40, 50, 60, 70] {
            assert!(!routed(ACTIVATING, device), "{device}");
        }
        assert!(routed(ACTIVATING, DEVICE_IP_CHECK));
        assert!(routed(ACTIVATING, 90));
        assert!(routed(ACTIVATING, DEVICE_ACTIVATED));
        assert!(routed(ACTIVATED, DEVICE_ACTIVATED));
        // Failed, or on its way down.
        assert!(!routed(ACTIVATING, 110));
        assert!(!routed(ACTIVATING, 120));
    }

    #[test]
    fn a_profile_that_routes_by_hand_is_left_alone() {
        for (key, data) in [
            ("never-default", json!(true)),
            ("route-table", json!(100)),
            ("routing-rules", json!([{}])),
            ("gateway", json!("192.168.200.1")),
            ("method", json!("disabled")),
        ] {
            let mut p = profile(&[(SERVER, &["0.0.0.0/0"])]);
            p["ipv4"][key] = json!({"data": data});
            assert!(carries(p, 7).is_empty(), "{key}");
        }
        let mut p = profile(&[(SERVER, &["0.0.0.0/0"])]);
        p["wireguard"]["peer-routes"] = json!({"type": "b", "data": false});
        assert!(carries(p, 7).is_empty());
        let mut p = profile(&[(SERVER, &["0.0.0.0/0"])]);
        p["wireguard"]["ip4-auto-default-route"] = json!({"type": "i", "data": 0});
        assert!(carries(p, 7).is_empty());
    }
}
