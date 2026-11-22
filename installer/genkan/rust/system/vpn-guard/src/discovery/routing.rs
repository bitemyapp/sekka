// SPDX-License-Identifier: GPL-3.0-or-later
//! Classify the ordinary (unmarked, unrestricted) host policy. A prefix is
//! evidence only if its route actually wins somewhere, through an eligible
//! rule, on an active tunnel. Specific LAN or administrator routes can still
//! override a tunnel default. Never infer a default from a percentage.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ipnet::IpNet;
use netlink_packet_route::{
    route::{RouteAddress, RouteAttribute, RouteFlags, RouteMessage, RouteType},
    rule::{RuleAction, RuleAttribute, RuleFlags, RuleMessage},
};

use super::{Device, Devices};
use crate::family::Family;

fn root(family: Family) -> IpNet {
    let addr = match family {
        Family::V4 => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        Family::V6 => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    };
    IpNet::new(addr, 0).unwrap()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Next {
    Device(u32),
    Throw,
    Other,
    Unknown,
}

#[derive(Debug)]
struct Route {
    dest: IpNet,
    metric: u32,
    next: Next,
}

#[derive(Default)]
pub(super) struct Routing {
    tables: BTreeMap<u32, BTreeMap<IpNet, Route>>,
    rules: Vec<Rule>,
}

#[derive(Debug)]
struct Rule {
    priority: u32,
    table: u32,
    action: RuleAction,
    dest: IpNet,
    mark_matches: bool,
    invert: bool,
    suppress: Option<u8>,
    goto: Option<u32>,
}

impl Rule {
    /// Largest CIDR around this address on which this selector agrees.
    fn selection(&self, addr: IpAddr) -> (bool, IpNet) {
        if !self.mark_matches {
            return (self.invert, root(Family::of(addr)));
        }
        if self.dest.contains(&addr) {
            return (!self.invert, self.dest);
        }
        let mut child = self.dest;
        while let Some(parent) = child.supernet() {
            if parent.contains(&addr) {
                return (
                    self.invert,
                    IpNet::new(addr, child.prefix_len()).unwrap().trunc(),
                );
            }
            child = parent;
        }
        unreachable!("same-family /0 contains every address")
    }
}

impl Routing {
    pub fn add_rule(&mut self, msg: RuleMessage, family: Family) -> Result<(), String> {
        let h = msg.header;
        if h.tos != 0
            || h.src_len != 0
            || h.flags
                .intersects(RuleFlags::IifDetached | RuleFlags::OifDetached | RuleFlags::Unresolved)
        {
            return Ok(());
        }
        let mut rule = Rule {
            priority: 0,
            table: h.table.into(),
            action: h.action,
            dest: root(family),
            mark_matches: true,
            invert: h.flags.contains(RuleFlags::Invert),
            suppress: None,
            goto: None,
        };
        let mut mark = 0;
        let mut mask = u32::MAX;
        for attr in msg.attributes {
            match attr {
                RuleAttribute::Priority(value) => rule.priority = value,
                RuleAttribute::Table(value) => rule.table = value,
                RuleAttribute::Destination(value) => {
                    rule.dest = IpNet::new(value, h.dst_len)
                        .map_err(|e| e.to_string())?
                        .trunc()
                }
                RuleAttribute::FwMark(value) => mark = value,
                RuleAttribute::FwMask(value) => mask = value,
                RuleAttribute::SuppressPrefixLen(value) if value != u32::MAX => {
                    rule.suppress = Some(
                        value
                            .try_into()
                            .map_err(|_| "invalid rule suppress length")?,
                    );
                }
                RuleAttribute::Goto(value) => rule.goto = Some(value),
                RuleAttribute::Protocol(_)
                | RuleAttribute::SuppressPrefixLen(_)
                | RuleAttribute::SuppressIfGroup(u32::MAX)
                | RuleAttribute::L3MDev(false) => (),
                RuleAttribute::Iifname(name) if name == "lo" => (),
                // A policy specifically selected by source, application,
                // bound interface, VRF or transport cannot establish a
                // machine-wide default.
                RuleAttribute::Source(_)
                | RuleAttribute::Iifname(_)
                | RuleAttribute::Oifname(_)
                | RuleAttribute::UidRange(_)
                | RuleAttribute::L3MDev(true)
                | RuleAttribute::IpProtocol(_)
                | RuleAttribute::TunId(_)
                | RuleAttribute::SourcePortRange(_)
                | RuleAttribute::DestinationPortRange(_) => return Ok(()),
                _ => return Err("unsupported routing policy rule attribute".into()),
            }
        }
        rule.mark_matches = mark & mask == 0;
        self.rules.push(rule);
        Ok(())
    }

    pub fn add_route(&mut self, msg: RouteMessage, family: Family) -> Result<(), String> {
        let h = msg.header;
        if h.tos != 0
            || h.source_prefix_length != 0
            || h.flags
                .intersects(RouteFlags::Dead | RouteFlags::Linkdown | RouteFlags::Cloned)
        {
            return Ok(());
        }
        let mut table = u32::from(h.table);
        let mut address = root(family).addr();
        let mut route = Route {
            dest: root(family),
            metric: 0,
            next: if h.kind == RouteType::Throw {
                Next::Throw
            } else {
                Next::Other
            },
        };
        let mut devices = BTreeSet::new();
        let mut nh_id = false;
        for attr in msg.attributes {
            match attr {
                RouteAttribute::Table(t) => table = t,
                RouteAttribute::Destination(RouteAddress::Inet(v)) => address = v.into(),
                RouteAttribute::Destination(RouteAddress::Inet6(v)) => address = v.into(),
                RouteAttribute::Priority(m) => route.metric = m,
                RouteAttribute::Oif(i) => {
                    devices.insert(i);
                }
                RouteAttribute::MultiPath(hops) => {
                    devices.extend(hops.iter().map(|hop| hop.interface_index));
                }
                RouteAttribute::NhId(_) => nh_id = true,
                _ => (),
            }
        }
        route.dest = IpNet::new(address, h.destination_prefix_length)
            .map_err(|e| e.to_string())?
            .trunc();
        if h.kind == RouteType::Unicast {
            route.next = if devices.len() == 1 {
                Next::Device(*devices.first().unwrap())
            } else if nh_id {
                Next::Unknown
            } else {
                // An ECMP route across interfaces is not an exclusive VPN route.
                Next::Other
            };
        }
        use std::collections::btree_map::Entry;
        match self.tables.entry(table).or_default().entry(route.dest) {
            Entry::Vacant(v) => {
                v.insert(route);
            }
            Entry::Occupied(mut o) if route.metric < o.get().metric => {
                o.insert(route);
            }
            Entry::Occupied(mut o)
                if route.metric == o.get().metric && route.next != o.get().next =>
            {
                o.get_mut().next = Next::Other;
            }
            _ => (),
        }
        Ok(())
    }

    fn winner(&self, addr: IpAddr) -> Result<Option<(&Route, IpNet)>, String> {
        let mut next = 0;
        let mut scope = root(Family::of(addr));
        while let Some(rule) = self.rules.get(next) {
            next += 1;
            let (applies, selected) = rule.selection(addr);
            if !applies {
                // An early "to LAN" is an exception to the later default.
                // A "not to LAN" that skips a /0 in a previous table, on
                // the other hand, may be the ONLY way to this later table.
                if rule.invert && selected.prefix_len() > scope.prefix_len() {
                    scope = selected;
                }
                continue;
            }

            match rule.action {
                RuleAction::Nop => continue,
                RuleAction::Goto => {
                    let target = rule.goto.ok_or("policy rule has no goto target")?;
                    if target <= rule.priority {
                        return Err("invalid backward goto in routing policy".into());
                    }
                    if selected.prefix_len() > scope.prefix_len() {
                        scope = selected;
                    }
                    next = self.rules.partition_point(|r| r.priority < target);
                    continue;
                }
                RuleAction::ToTable => (),
                RuleAction::Blackhole | RuleAction::Prohibit | RuleAction::Unreachable => {
                    return Ok(None);
                }
                action => return Err(format!("unsupported host routing rule action {action:?}")),
            }
            let Some(table) = self.tables.get(&rule.table) else {
                continue;
            };
            let mut prefix = Some(IpNet::from(addr));
            while let Some(net) = prefix {
                if let Some(route) = table.get(&net) {
                    if route.next == Next::Throw {
                        // This exception can be the only way past a different
                        // default in the earlier table.
                        if net.prefix_len() > scope.prefix_len() {
                            scope = net;
                        }
                        break;
                    }
                    if rule.suppress.is_some_and(|len| net.prefix_len() <= len) {
                        break;
                    }
                    if route.next == Next::Unknown {
                        return Err("cannot identify output interface of selected nexthop".into());
                    }
                    // A host-specific rule pointing into a table containing
                    // a /0 still supplies only a host route as evidence.
                    if selected.prefix_len() > scope.prefix_len() {
                        scope = selected;
                    }
                    return Ok(Some((route, scope)));
                }
                prefix = net.supernet();
            }
        }
        Ok(None)
    }

    /// Exhaustive partition of the address family at every change of route,
    /// rule destination, and allowed IP. Within each interval longest-prefix
    /// selection and eligibility are constant. Record the winning *prefix*,
    /// so an actual WG default coexists with ordinary LAN exceptions.
    pub fn carriers(&mut self, family: Family, devices: &Devices) -> Result<BTreeSet<u32>, String> {
        self.rules.sort_by_key(|rule| rule.priority);
        let default = root(family);
        let mut boundaries = BTreeSet::from([default.addr()]);
        // The complement of a destination selector is several CIDRs. Include
        // each sibling region so that it is accounted for in its entirety.
        for rule in &self.rules {
            let mut child = rule.dest;
            while let Some(parent) = child.supernet() {
                for subnet in parent.subnets(child.prefix_len()).unwrap() {
                    boundaries.insert(subnet.network());
                }
                child = parent;
            }
        }
        let routed = self.tables.values().flat_map(|table| table.keys().copied());
        let allowed = devices
            .values()
            .filter_map(|dev| dev.wg.as_ref())
            .flat_map(|wg| wg.allowed.iter().copied());
        for net in routed
            .chain(self.rules.iter().map(|r| r.dest))
            .chain(allowed)
        {
            if Family::of(net.addr()) != family {
                continue;
            }
            boundaries.insert(net.network());
            if let Some(after) = next_address(net.broadcast()) {
                boundaries.insert(after);
            }
        }
        let mut evidence: BTreeMap<u32, BTreeSet<IpNet>> = BTreeMap::new();
        for addr in boundaries {
            let Some((route, scope)) = self.winner(addr)? else {
                continue;
            };
            let Next::Device(index) = route.next else {
                continue;
            };
            let Some(Device { wg, .. }) = devices.get(&index) else {
                continue;
            };
            let mut prefix = route.dest;
            if scope.prefix_len() > prefix.prefix_len() {
                prefix = scope;
            }
            if let Some(wg) = wg {
                let Some(allowed) = wg.allowed_at(addr) else {
                    continue;
                };
                if allowed.prefix_len() > prefix.prefix_len() {
                    prefix = *allowed;
                }
            }
            evidence.entry(index).or_default().insert(prefix);
        }
        Ok(evidence
            .into_iter()
            .filter_map(|(idx, nets)| {
                (IpNet::aggregate(&nets.into_iter().collect::<Vec<_>>()) == [default])
                    .then_some(idx)
            })
            .collect())
    }
}

fn next_address(addr: IpAddr) -> Option<IpAddr> {
    match addr {
        IpAddr::V4(v) => u32::from(v).checked_add(1).map(|n| IpAddr::V4(n.into())),
        IpAddr::V6(v) => u128::from(v).checked_add(1).map(|n| IpAddr::V6(n.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::wireguard::Wireguard;

    const WG: u32 = 3;
    const LAN: u32 = 4;
    const MAIN: u32 = 254;
    const VPN: u32 = 51820;

    fn device(allowed: &[&str]) -> Devices {
        BTreeMap::from([(
            WG,
            Device {
                name: "wg0".into(),
                wg: Some(Wireguard {
                    allowed: IpNet::aggregate(
                        &allowed
                            .iter()
                            .map(|text| text.parse().unwrap())
                            .collect::<Vec<_>>(),
                    ),
                    ..Default::default()
                }),
            },
        )])
    }

    fn rule(routing: &mut Routing, priority: u32, table: u32, attributes: Vec<RuleAttribute>) {
        let mut msg = RuleMessage::default();
        msg.header.action = RuleAction::ToTable;
        msg.attributes = vec![
            RuleAttribute::Table(table),
            RuleAttribute::Priority(priority),
        ];
        msg.attributes.extend(attributes);
        routing.add_rule(msg, Family::V4).unwrap();
    }

    fn route(routing: &mut Routing, prefix: &str, table: u32, metric: u32, index: u32) {
        let net: IpNet = prefix.parse().unwrap();
        let mut msg = RouteMessage::default();
        msg.header.destination_prefix_length = net.prefix_len();
        msg.header.kind = RouteType::Unicast;
        msg.attributes = vec![
            RouteAttribute::Destination(net.addr().into()),
            RouteAttribute::Oif(index),
            RouteAttribute::Priority(metric),
            RouteAttribute::Table(table),
        ];
        routing.add_route(msg, Family::of(net.addr())).unwrap();
    }

    fn covers(routing: &mut Routing, devices: &Devices) -> bool {
        routing.carriers(Family::V4, devices).unwrap().contains(&WG)
    }

    #[test]
    fn default_requires_a_selected_table_and_eligible_peers() {
        let mut routing = Routing::default();
        rule(&mut routing, 32766, MAIN, vec![]);
        route(&mut routing, "0.0.0.0/0", MAIN, 100, LAN);
        route(&mut routing, "0.0.0.0/0", VPN, 50, WG);
        let wg = device(&["0.0.0.0/0"]);
        assert!(!covers(&mut routing, &wg));
        rule(
            &mut routing,
            10,
            VPN,
            vec![RuleAttribute::UidRange(
                netlink_packet_route::rule::RuleUidRange {
                    start: 1000,
                    end: 1000,
                },
            )],
        );
        rule(&mut routing, 11, VPN, vec![RuleAttribute::FwMark(42)]);
        assert!(!covers(&mut routing, &wg));
        rule(
            &mut routing,
            15,
            MAIN,
            vec![RuleAttribute::SuppressPrefixLen(0)],
        );
        let mut wg_rule = RuleMessage::default();
        wg_rule.header.action = RuleAction::ToTable;
        wg_rule.header.flags = RuleFlags::Invert;
        wg_rule.attributes = vec![
            RuleAttribute::Table(VPN),
            RuleAttribute::Priority(16),
            RuleAttribute::FwMark(42),
        ];
        routing.add_rule(wg_rule, Family::V4).unwrap();
        route(&mut routing, "192.0.2.0/24", MAIN, 0, LAN);
        assert!(covers(&mut routing, &wg));
        assert!(!covers(&mut routing, &device(&["192.0.2.0/24"])));
    }

    #[test]
    fn complementary_prefixes_cover_the_family_arbitrary_majority_does_not() {
        let wg = device(&["0.0.0.0/0"]);
        let mut routing = Routing::default();
        rule(&mut routing, 32766, MAIN, vec![]);
        route(&mut routing, "0.0.0.0/0", MAIN, 100, LAN);
        route(&mut routing, "0.0.0.0/1", MAIN, 100, WG);
        route(&mut routing, "192.0.2.1/32", MAIN, 100, WG);
        assert!(!covers(&mut routing, &wg));
        route(&mut routing, "128.0.0.0/1", MAIN, 100, WG);
        assert!(covers(&mut routing, &wg));
    }

    #[test]
    fn lower_metric_and_earlier_tables_win() {
        let wg = device(&["0.0.0.0/0"]);
        let mut routing = Routing::default();
        rule(&mut routing, 10, MAIN, vec![]);
        rule(&mut routing, 20, VPN, vec![]);
        route(&mut routing, "0.0.0.0/0", VPN, 0, WG);
        route(&mut routing, "0.0.0.0/0", MAIN, 20, WG);
        route(&mut routing, "0.0.0.0/0", MAIN, 10, LAN);
        assert!(!covers(&mut routing, &wg));
        route(&mut routing, "0.0.0.0/0", MAIN, 5, WG);
        assert!(covers(&mut routing, &wg));
    }

    #[test]
    fn default_selected_only_for_one_destination_is_split() {
        let mut routing = Routing::default();
        route(&mut routing, "0.0.0.0/0", VPN, 0, WG);
        let mut rule = RuleMessage::default();
        rule.header.action = RuleAction::ToTable;
        rule.header.dst_len = 24;
        rule.attributes = vec![
            RuleAttribute::Table(VPN),
            RuleAttribute::Destination("192.0.2.0".parse().unwrap()),
        ];
        routing.add_rule(rule, Family::V4).unwrap();
        assert!(!covers(&mut routing, &device(&["0.0.0.0/0"])));
    }

    #[test]
    fn specific_lan_rule_and_documentation_discard_coexist_with_a_wg_default() {
        let mut routing = Routing::default();
        let mut lan = RuleMessage::default();
        lan.header.action = RuleAction::ToTable;
        lan.header.dst_len = 24;
        lan.attributes = vec![
            RuleAttribute::Priority(1),
            RuleAttribute::Table(80),
            RuleAttribute::Destination("192.168.1.0".parse().unwrap()),
        ];
        routing.add_rule(lan, Family::V4).unwrap();
        rule(
            &mut routing,
            100,
            MAIN,
            vec![RuleAttribute::SuppressPrefixLen(0)],
        );
        rule(&mut routing, 101, VPN, vec![]);
        rule(&mut routing, 32766, MAIN, vec![]);
        route(&mut routing, "192.168.1.0/24", 80, 0, LAN);
        let mut discard = RouteMessage::default();
        discard.header.kind = RouteType::BlackHole;
        discard.header.destination_prefix_length = 24;
        discard.attributes = vec![
            RouteAttribute::Table(MAIN),
            RouteAttribute::Destination(RouteAddress::Inet("192.0.2.0".parse().unwrap())),
        ];
        routing.add_route(discard, Family::V4).unwrap();
        route(&mut routing, "0.0.0.0/0", VPN, 0, WG);
        assert!(covers(&mut routing, &device(&["0.0.0.0/0"])));
    }

    #[test]
    fn a_host_throw_in_an_earlier_default_table_is_not_a_vpn_default() {
        let mut routing = Routing::default();
        rule(&mut routing, 10, MAIN, vec![]);
        rule(&mut routing, 20, VPN, vec![]);
        route(&mut routing, "0.0.0.0/0", MAIN, 0, LAN);
        route(&mut routing, "0.0.0.0/0", VPN, 0, WG);
        let mut hole = RouteMessage::default();
        hole.header.kind = RouteType::Throw;
        hole.header.destination_prefix_length = 32;
        hole.attributes = vec![
            RouteAttribute::Table(MAIN),
            RouteAttribute::Destination(RouteAddress::Inet("192.0.2.1".parse().unwrap())),
        ];
        routing.add_route(hole, Family::V4).unwrap();
        assert!(!covers(&mut routing, &device(&["0.0.0.0/0"])));
    }

    #[test]
    fn a_default_in_another_tunnel_is_recognized() {
        let mut routing = Routing::default();
        let mut msg = RuleMessage::default();
        msg.header.action = RuleAction::ToTable;
        msg.attributes = vec![RuleAttribute::Table(MAIN)];
        routing.add_rule(msg, Family::V6).unwrap();
        route(&mut routing, "::/0", MAIN, 5, WG);
        let tunnel = BTreeMap::from([(
            WG,
            Device {
                name: "tun0".into(),
                wg: None,
            },
        )]);
        assert!(routing.carriers(Family::V6, &tunnel).unwrap().contains(&WG));
    }
}
