// SPDX-License-Identifier: GPL-3.0-or-later
//! Guard locally generated traffic at the output hook, after routing has chosen
//! the egress interface. Forwarded traffic belongs to the host's forwarding policy.
//!
//! The owner comment reserves the entire named table, including its chains and
//! rules. A same-named table without that marker is a collision. A replacement
//! deletes the observed handle and creates the name exclusively in one nftables
//! transaction, so concurrent deletion/recreation cannot overwrite another table.

use std::io::Write;
use std::process::{Command, Stdio};

use ipnet::IpNet;
use serde_json::{Value, json};

use crate::Policy;
use crate::family::Family;

const NFT: &str = match option_env!("SEKKA_VPN_GUARD_NFT") {
    Some(program) => program,
    None => "nft",
};
const TABLE: &str = "sekka_vpn_guard";
const OWNER: &str = "Managed exclusively by sekka-vpn-guard";
const CHAIN: &str = "output";
/// Link kinds that are tunnels. Traffic routing sent into one of these is
/// inside a tunnel, whichever program made it, and is not the guard's to
/// refuse.
const TUNNEL_KINDS: [&str; 5] = ["wireguard", "tun", "xfrm", "vti", "vti6"];

fn equal(left: Value, right: Value) -> Value {
    json!({"match": {"op": "==", "left": left, "right": right}})
}

fn meta(key: &str, value: Value) -> Value {
    equal(json!({"meta": {"key": key}}), value)
}

fn payload(protocol: &str, field: &str, value: Value) -> Value {
    equal(
        json!({"payload": {"protocol": protocol, "field": field}}),
        value,
    )
}

fn rule(expressions: Vec<Value>) -> Value {
    json!({"rule": {"family": "inet", "table": TABLE, "chain": CHAIN, "expr": expressions}})
}

/// Encode the canonical network base, or the bare address for a host prefix,
/// the same way nft lists it when reading the kernel table back.
fn network(net: IpNet) -> Value {
    if net.prefix_len() == net.max_prefix_len() {
        return json!(net.addr().to_string());
    }
    json!({"prefix": {"addr": net.network().to_string(), "len": net.prefix_len()}})
}

/// Destinations on the local network, reachable in the blocked family too.
fn local(family: Family) -> impl Iterator<Item = IpNet> {
    let nets: &[&str] = match family {
        Family::V4 => &[
            "10.0.0.0/8",
            "172.16.0.0/12",
            "192.168.0.0/16",
            "169.254.0.0/16",
            "224.0.0.0/24",
            "255.255.255.255/32",
        ],
        Family::V6 => &["fc00::/7", "fe80::/10", "ff01::/16", "ff02::/16"],
    };
    nets.iter()
        .map(|net| net.parse().expect("a constant prefix"))
}

fn table() -> Value {
    json!({"table": {"family": "inet", "name": TABLE, "comment": OWNER}})
}

fn family_gate(blocked: Family) -> Value {
    let other_protocol = match blocked {
        Family::V4 => "ipv6",
        Family::V6 => "ipv4",
    };
    rule(vec![
        meta("nfproto", json!(other_protocol)),
        json!({"accept": null}),
    ])
}

fn chain(rules: &[Value]) -> Value {
    // The unprivileged status display cannot read nft. When NetworkManager
    // cannot be read, the privileged reader can still check that the entire
    // policy last installed is intact. FNV-1a detects accidental drift; the
    // comment is not authentication against another privileged firewall manager.
    let mut fingerprint = 0xcbf29ce484222325u64;
    for rule in rules {
        for byte in rule.to_string().bytes() {
            fingerprint = (fingerprint ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    let comment = format!("sekka-vpn-guard rules v1 {fingerprint:016x}");
    json!({"chain": {"family": "inet", "table": TABLE, "name": CHAIN, "type": "filter", "hook": "output", "prio": 0, "policy": "accept", "comment": comment}})
}

fn desired(policy: &Policy) -> Vec<Value> {
    let Some(blocked) = policy.blocked else {
        return Vec::new();
    };
    let protocol = match blocked {
        Family::V4 => "ip",
        Family::V6 => "ip6",
    };
    let accept = json!({"accept": null});
    let mut rules = vec![
        family_gate(blocked),
        rule(vec![meta("oifname", json!("lo")), accept.clone()]),
    ];
    for kind in TUNNEL_KINDS {
        rules.push(rule(vec![meta("oifkind", json!(kind)), accept.clone()]));
    }
    for mark in &policy.marks {
        rules.push(rule(vec![
            meta("l4proto", json!("udp")),
            meta("mark", json!(mark.get())),
            accept.clone(),
        ]));
    }
    for endpoint in &policy.endpoints {
        if Family::of(endpoint.ip()) == blocked {
            rules.push(rule(vec![
                payload(protocol, "daddr", json!(endpoint.ip().to_string())),
                payload("udp", "dport", json!(endpoint.port())),
                accept.clone(),
            ]));
        }
    }
    let trusted = policy.trusted.iter().copied();
    for net in local(blocked).chain(trusted.filter(|net| Family::of(net.addr()) == blocked)) {
        rules.push(rule(vec![
            payload(protocol, "daddr", network(net)),
            accept.clone(),
        ]));
    }
    rules.push(rule(vec![
        meta("l4proto", json!("tcp")),
        json!({"reject": {"type": "tcp reset"}}),
    ]));
    rules.push(rule(vec![
        json!({"reject": {"type": "icmpx", "expr": "admin-prohibited"}}),
    ]));
    let mut objects = vec![table(), chain(&rules)];
    objects.extend(rules);
    objects
}

fn nft(args: &[&str], input: Option<&Value>) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new(NFT);
    cmd.arg("-j")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if input.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let mut child = cmd.spawn().map_err(|e| format!("start nft: {e}"))?;
    let write = if let Some(document) = input {
        child
            .stdin
            .take()
            .expect("piped nft stdin")
            .write_all(document.to_string().as_bytes())
    } else {
        Ok(())
    };
    let out = child
        .wait_with_output()
        .map_err(|e| format!("wait for nft: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "nft {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    write.map_err(|e| format!("write nft transaction: {e}"))?;
    Ok(out.stdout)
}

fn list(args: &[&str]) -> Result<Vec<Value>, String> {
    #[derive(serde::Deserialize)]
    struct Listing {
        nftables: Vec<Value>,
    }
    let output = nft(args, None)?;
    let listing: Listing =
        serde_json::from_slice(&output).map_err(|e| format!("read nft {}: {e}", args.join(" ")))?;
    Ok(listing
        .nftables
        .into_iter()
        .filter(|object| object.get("metainfo").is_none())
        .collect())
}

fn table_handle(table: &Value) -> Result<u64, String> {
    if table["comment"] != OWNER {
        return Err(format!(
            "inet table {TABLE} has a different owner; refusing to change it"
        ));
    }
    table["handle"]
        .as_u64()
        .ok_or_else(|| format!("missing handle for inet table {TABLE}"))
}

#[derive(Default)]
struct Installed {
    handle: Option<u64>,
    objects: Vec<Value>,
}

fn read() -> Result<Installed, String> {
    // Listing all table headers distinguishes absence from a failed netlink
    // request without interpreting nft's human-readable diagnostics.
    let tables = list(&["list", "tables", "inet"])?;
    let named =
        |object: &Value| object["table"]["family"] == "inet" && object["table"]["name"] == TABLE;
    if !tables.iter().any(named) {
        return Ok(Installed::default());
    }
    let mut objects = list(&["list", "table", "inet", TABLE])?;
    let table = objects
        .iter()
        .find(|object| named(object))
        .ok_or_else(|| format!("missing inet table {TABLE} in detailed listing"))?;
    let handle = table_handle(&table["table"])?;
    for object in &mut objects {
        // Only the kernel's object handles are ephemeral. Unknown objects,
        // flags, rule order, predicates and verdicts all affect equality.
        for body in object
            .as_object_mut()
            .into_iter()
            .flat_map(|map| map.values_mut())
        {
            if let Some(fields) = body.as_object_mut() {
                fields.remove("handle");
            }
        }
    }
    Ok(Installed {
        handle: Some(handle),
        objects,
    })
}

fn inspect(current: &Installed) -> Result<Option<Family>, String> {
    if current.handle.is_none() {
        return Ok(None);
    }
    if let [header, hook, rules @ ..] = current.objects.as_slice()
        && header == &table()
        && hook == &chain(rules)
        && let Some(blocked) = Family::BOTH
            .into_iter()
            .find(|&family| rules.first() == Some(&family_gate(family)))
    {
        return Ok(Some(blocked));
    }
    Err(format!(
        "inet table {TABLE} has an unrecognized or modified policy"
    ))
}

/// The verified kernel state for the privileged reconciler.
pub fn blocked_family() -> Result<Option<Family>, String> {
    inspect(&read()?)
}

fn transaction(current: &Installed, wanted: &[Value]) -> Option<Value> {
    if current.objects == wanted {
        return None;
    }
    let mut commands = Vec::new();
    if let Some(handle) = current.handle {
        commands.push(json!({"delete": {"table": {"family": "inet", "handle": handle}}}));
    }
    for object in wanted {
        if object.get("table").is_some() {
            commands.push(json!({"create": object}));
        } else {
            commands.push(json!({"add": object}));
        }
    }
    Some(json!({"nftables": commands}))
}

pub fn apply(policy: &Policy) -> Result<(), String> {
    let wanted = desired(policy);
    if let Some(batch) = transaction(&read()?, &wanted) {
        nft(&["-f", "-"], Some(&batch))?;
        let actual = read()?;
        if actual.objects != wanted {
            return Err(format!(
                "inet table {TABLE} readback differs from the requested policy"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU32;

    #[test]
    fn replacement_checks_owner_and_uses_handle_and_exclusive_create() {
        assert!(table_handle(&json!({"handle": 7})).is_err());
        let handle = table_handle(&json!({"comment": OWNER, "handle": 7})).unwrap();
        let table = json!({"table": {"family": "inet", "name": TABLE, "comment": OWNER}});
        let installed = Installed {
            handle: Some(handle),
            objects: vec![table.clone()],
        };
        assert!(transaction(&installed, std::slice::from_ref(&table)).is_none());
        let wanted = [table.clone(), rule(vec![json!({"accept": null})])];
        let batch = transaction(&installed, &wanted).unwrap();
        assert_eq!(
            batch["nftables"][0],
            json!({"delete": {"table": {"family": "inet", "handle": 7}}})
        );
        assert_eq!(batch["nftables"][1], json!({"create": table}));
        assert_eq!(
            transaction(&installed, &[]).unwrap()["nftables"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(transaction(&Installed::default(), &[]).is_none());
    }

    #[test]
    fn policy_keeps_transport_exceptions_narrow_and_readback_detects_drift() {
        let policy = Policy {
            blocked: Some(Family::V6),
            marks: [NonZeroU32::new(1234).unwrap()].into(),
            endpoints: [
                "[2001:db8::fe]:51515".parse().unwrap(),
                "198.51.100.7:51820".parse().unwrap(),
            ]
            .into(),
            trusted: [
                "2001:db8:1::1/128".parse().unwrap(),
                "198.51.100.0/24".parse().unwrap(),
            ]
            .into(),
            ..Policy::default()
        };
        let mut installed = Installed {
            handle: Some(1),
            objects: desired(&policy),
        };
        let accept = json!({"accept": null});
        assert!(installed.objects.contains(&rule(vec![
            meta("l4proto", json!("udp")),
            meta("mark", json!(1234)),
            accept.clone(),
        ])));
        assert!(installed.objects.contains(&rule(vec![
            payload("ip6", "daddr", json!("2001:db8::fe")),
            payload("udp", "dport", json!(51515)),
            accept.clone(),
        ])));
        assert!(installed.objects.contains(&rule(vec![
            payload("ip6", "daddr", json!("2001:db8:1::1")),
            accept.clone(),
        ])));
        assert!(!installed.objects.contains(&rule(vec![
            payload("ip6", "daddr", json!("2001:db8::fe")),
            accept.clone(),
        ])));
        // Nothing of the open family is written into the blocked one's rules.
        assert!(
            !installed
                .objects
                .iter()
                .any(|o| o.to_string().contains("198.51.100."))
        );
        assert_eq!(inspect(&installed).unwrap(), Some(Family::V6));
        installed.objects.pop();
        assert!(inspect(&installed).is_err());
        assert_eq!(inspect(&Installed::default()).unwrap(), None);
        assert!(desired(&Policy::default()).is_empty());
    }

    #[test]
    fn traffic_into_any_tunnel_device_passes_before_the_refusals() {
        let policy = Policy {
            blocked: Some(Family::V4),
            ..Policy::default()
        };
        let rules = desired(&policy);
        let position = |wanted: &Value| rules.iter().position(|rule| rule == wanted).unwrap();
        let refusal = position(&rule(vec![
            meta("l4proto", json!("tcp")),
            json!({"reject": {"type": "tcp reset"}}),
        ]));
        for kind in TUNNEL_KINDS {
            let accept = rule(vec![meta("oifkind", json!(kind)), json!({"accept": null})]);
            assert!(position(&accept) < refusal, "{kind}");
        }
    }

    #[test]
    fn networks_compare_to_canonical_kernel_output() {
        assert_eq!(
            network("fd12::abba/64".parse().unwrap()),
            json!({"prefix": {"addr": "fd12::", "len": 64}})
        );
        assert_eq!(
            network("203.0.113.20/32".parse().unwrap()),
            json!("203.0.113.20")
        );
        assert_eq!(
            local(Family::V4).last().map(network),
            Some(json!("255.255.255.255"))
        );
    }
}
