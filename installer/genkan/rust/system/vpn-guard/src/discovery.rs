// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only network discovery. Capture complete kernel dumps before making
//! a protection decision. Routes decide what the host selects; WireGuard's
//! allowed IPs decide what a peer can carry once that device is selected.

mod netlink;
mod routing;
mod wireguard;

use std::collections::{BTreeMap, BTreeSet};

use netlink_packet_route::{
    AddressFamily, RouteNetlinkMessage,
    link::{InfoKind, LinkAttribute, LinkFlags, LinkInfo, LinkMessage},
    route::RouteMessage,
    rule::RuleMessage,
};
use netlink_sys::protocols::NETLINK_ROUTE;

use self::netlink::request;
use self::routing::Routing;
use self::wireguard::Wireguard;
use crate::{Policy, family::Family};

struct Device {
    name: String,
    wg: Option<Wireguard>,
}

type Devices = BTreeMap<u32, Device>;

fn devices() -> Result<Devices, String> {
    let mut devices = Devices::new();
    let mut wg_family_id = None;
    for msg in request(
        NETLINK_ROUTE,
        RouteNetlinkMessage::GetLink(LinkMessage::default()),
        true,
    )? {
        let RouteNetlinkMessage::NewLink(link) = msg else {
            return Err("unexpected link dump response".into());
        };
        if !link.header.flags.contains(LinkFlags::Up) {
            continue;
        }
        let mut name = None;
        let mut kind = None;
        for attr in link.attributes {
            match attr {
                LinkAttribute::IfName(value) => name = Some(value),
                LinkAttribute::LinkInfo(info) => {
                    kind = info.into_iter().find_map(|i| match i {
                        LinkInfo::Kind(kind) => Some(kind),
                        _ => None,
                    });
                }
                _ => (),
            }
        }
        let wg = match kind {
            Some(InfoKind::Wireguard) => {
                let family = match wg_family_id {
                    Some(id) => id,
                    None => *wg_family_id.insert(wireguard::family_id()?),
                };
                Some(wireguard::device(family, link.header.index)?)
            }
            Some(InfoKind::Tun | InfoKind::Xfrm | InfoKind::Vti | InfoKind::Vti6) => None,
            _ => continue,
        };
        devices.insert(
            link.header.index,
            Device {
                name: name.ok_or("unnamed tunnel device")?,
                wg,
            },
        );
    }
    Ok(devices)
}

fn carriers(family: Family, devices: &Devices) -> Result<BTreeSet<u32>, String> {
    let af = match family {
        Family::V4 => AddressFamily::Inet,
        Family::V6 => AddressFamily::Inet6,
    };
    let mut routes = RouteMessage::default();
    routes.header.address_family = af;
    let mut routing = Routing::default();
    for msg in request(NETLINK_ROUTE, RouteNetlinkMessage::GetRoute(routes), true)? {
        let RouteNetlinkMessage::NewRoute(msg) = msg else {
            return Err("unexpected route dump response".into());
        };
        if msg.header.address_family == af {
            routing.add_route(msg, family)?;
        }
    }
    let mut rules = RuleMessage::default();
    rules.header.family = af;
    for msg in request(NETLINK_ROUTE, RouteNetlinkMessage::GetRule(rules), true)? {
        let RouteNetlinkMessage::NewRule(msg) = msg else {
            return Err("unexpected policy rule dump response".into());
        };
        if msg.header.family == af {
            routing.add_rule(msg, family)?;
        }
    }
    routing.carriers(family, devices)
}

pub fn discover() -> Result<Policy, String> {
    let devices = devices().map_err(|e| format!("reading tunnel devices: {e}"))?;
    let mut policy = Policy::default();
    if !devices.values().any(|device| device.wg.is_some()) {
        return Ok(policy);
    }
    let v4 = carriers(Family::V4, &devices).map_err(|e| format!("reading IPv4 policy: {e}"))?;
    let v6 = carriers(Family::V6, &devices).map_err(|e| format!("reading IPv6 policy: {e}"))?;
    let wireguard = |index: &u32| devices[index].wg.is_some();
    policy.blocked = if v4.iter().any(wireguard) && v6.is_empty() {
        Some(Family::V6)
    } else if v6.iter().any(wireguard) && v4.is_empty() {
        Some(Family::V4)
    } else {
        None
    };
    policy.carriers = v4.union(&v6).map(|i| devices[i].name.clone()).collect();
    if let Some(blocked) = policy.blocked {
        for dev in devices.values() {
            policy.tunnels.insert(dev.name.clone());
            let Some(wg) = &dev.wg else { continue };
            if wg.allowed.is_empty() {
                continue;
            }
            // The device's nonzero mark identifies its encapsulation even
            // after endpoint roaming. Unmarked peers need the exact UDP tuple.
            if wg.mark != 0 {
                policy.marks.insert(wg.mark);
            } else {
                policy.endpoints.extend(
                    wg.endpoints
                        .iter()
                        .copied()
                        .filter(|ep| Family::of(ep.ip()) == blocked),
                );
            }
        }
    }
    Ok(policy)
}
