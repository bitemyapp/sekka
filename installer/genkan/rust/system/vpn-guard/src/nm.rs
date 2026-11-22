// SPDX-License-Identifier: GPL-3.0-or-later
//! Positive evidence from NetworkManager while the kernel snapshot is unavailable.
use std::{collections::BTreeSet, process::Command};

use ipnet::IpNet;
use serde::{Deserialize, de::DeserializeOwned};

use crate::family::Family;

const NM: &str = "org.freedesktop.NetworkManager";
const ROOT: &str = "/org/freedesktop/NetworkManager";
const PROPS: &str = "org.freedesktop.DBus.Properties";
const AC: &str = "org.freedesktop.NetworkManager.Connection.Active";
const PROFILE: &str = "org.freedesktop.NetworkManager.Settings.Connection";

#[derive(Deserialize, Default)]
struct Variant<T> {
    data: T,
}

// busctl encodes a method's output arguments as a tuple, even with one result.
// A property inside a variant has its unwrapped value in its own data field.
fn call<T: DeserializeOwned>(
    path: &str,
    iface: &str,
    method: &str,
    args: &[&str],
) -> Result<T, String> {
    let busctl = option_env!("SEKKA_VPN_GUARD_BUSCTL")
        .ok_or("built without the NetworkManager busctl path")?;
    let result = Command::new(busctl)
        .args([
            "--system",
            "--json=short",
            "--auto-start=no",
            "--timeout=3s",
            "call",
            NM,
            path,
            iface,
            method,
        ])
        .args(args)
        .output()
        .map_err(|e| format!("NetworkManager {method}: {e}"))?;
    if !result.status.success() {
        return Err(format!(
            "NetworkManager {method}: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    serde_json::from_slice::<Variant<(T,)>>(&result.stdout)
        .map(|reply| reply.data.0)
        .map_err(|e| format!("NetworkManager {method} reply: {e}"))
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Active {
    #[serde(rename = "Type")]
    kind: Variant<String>,
    state: Variant<u32>,
    state_flags: Variant<u32>,
    connection: Variant<String>,
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
    wireguard: Wireguard,
    #[serde(default)]
    ipv4: Stack,
    #[serde(default)]
    ipv6: Stack,
}

impl Profile {
    fn full_families(&self) -> Result<Vec<Family>, String> {
        let wg = &self.wireguard;
        if wg.peer_routes.as_ref().is_some_and(|v| !v.data) {
            return Ok(Vec::new());
        }
        let mut prefixes = Vec::new();
        for peer in &wg.peers.data {
            for prefix in &peer.allowed_ips.data {
                prefixes.push(
                    prefix
                        .parse::<IpNet>()
                        .map_err(|e| format!("invalid NetworkManager AllowedIP: {e}"))?
                        .trunc(),
                );
            }
        }
        let mut full = Vec::new();
        for (family, stack, automatic) in [
            (Family::V4, &self.ipv4, &wg.ip4_auto_default_route),
            (Family::V6, &self.ipv6, &wg.ip6_auto_default_route),
        ] {
            // Custom policy needs the kernel classifier. A disabled automatic /0
            // in the main table can compete on metric with the physical default.
            if !matches!(stack.method.data.as_str(), "auto" | "manual" | "dhcp")
                || stack.never_default.data
                || !matches!(stack.route_table.data, 0 | 254)
                || !stack.routing_rules.data.is_empty()
                || !stack.gateway.data.is_empty()
            {
                continue;
            }
            let nets = prefixes
                .iter()
                .copied()
                .filter(|net| Family::of(net.addr()) == family)
                .filter(|net| {
                    net.prefix_len() != 0 || !automatic.as_ref().is_some_and(|v| v.data == 0)
                })
                .collect();
            if IpNet::aggregate(&nets)
                .iter()
                .any(|net| net.prefix_len() == 0)
            {
                full.push(family);
            }
        }
        Ok(full)
    }
}

pub fn active_intent() -> Result<Option<Family>, String> {
    let paths: Variant<Vec<String>> = call(ROOT, PROPS, "Get", &["ss", NM, "ActiveConnections"])?;
    let mut full = BTreeSet::new();
    for path in paths.data {
        let active: Active = call(&path, PROPS, "GetAll", &["s", AC])?;
        // Only NM-managed connections in the activating or activated states.
        if active.kind.data != "wireguard"
            || !matches!(active.state.data, 1 | 2)
            || active.state_flags.data & 0x80 != 0
        {
            continue;
        }
        let settings: Profile = call(&active.connection.data, PROFILE, "GetSettings", &[])?;
        full.extend(settings.full_families()?);
    }
    Ok(
        match (full.contains(&Family::V4), full.contains(&Family::V6)) {
            (true, false) => Some(Family::V6),
            (false, true) => Some(Family::V4),
            _ => None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn profile() -> Value {
        json!({
            "wireguard": {"peers": {"type": "aa{sv}", "data": [
                {"allowed-ips": {"type":"as", "data":["0.0.0.0/1"]}},
                {"allowed-ips": {"type":"as", "data":["128.0.0.0/1","2001:db8::/32"]}}
            ]}},
            "ipv4": {"method": {"type":"s", "data":"manual"}},
            "ipv6": {"method": {"type":"s", "data":"manual"}}
        })
    }

    fn full(value: Value) -> Vec<Family> {
        serde_json::from_value::<Profile>(value)
            .unwrap()
            .full_families()
            .unwrap()
    }

    #[test]
    fn peers_may_jointly_cover_a_family() {
        let mut p = profile();
        assert_eq!(full(p.clone()), [Family::V4]);
        p["wireguard"]["peers"]["data"][0]["allowed-ips"]["data"] = json!(["::/0"]);
        assert_eq!(full(p), [Family::V6]);
    }

    #[test]
    fn skips_disabled_selective_and_external_table_routes() {
        for (key, data) in [
            ("never-default", json!(true)),
            ("route-table", json!(100)),
            ("routing-rules", json!([{}])),
            ("method", json!("disabled")),
        ] {
            let mut p = profile();
            p["ipv4"][key] = json!({"data": data});
            assert!(full(p).is_empty(), "{key}");
        }
        let mut p = profile();
        p["wireguard"]["peer-routes"] = json!({"type":"b", "data":false});
        assert!(full(p).is_empty());
    }
}
