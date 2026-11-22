// SPDX-License-Identifier: GPL-3.0-or-later
use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};

use ipnet::IpNet;
use netlink_packet_generic::{
    GenlMessage,
    ctrl::{GenlCtrl, GenlCtrlCmd, nlas::GenlCtrlAttrs},
};
use netlink_packet_wireguard::{
    WireguardAllowedIpAttr, WireguardAttribute, WireguardCmd, WireguardMessage,
    WireguardPeerAttribute,
};
use netlink_sys::protocols::NETLINK_GENERIC;

use super::netlink::request;

#[derive(Default)]
pub(super) struct Wireguard {
    pub allowed: Vec<IpNet>,
    pub mark: u32,
    pub endpoints: BTreeSet<SocketAddr>,
}

impl Wireguard {
    // The dump is aggregated into disjoint prefixes in address order.
    pub fn allowed_at(&self, addr: IpAddr) -> Option<&IpNet> {
        let end = self.allowed.partition_point(|net| net.network() <= addr);
        self.allowed[..end].last().filter(|net| net.contains(&addr))
    }
}

pub(super) fn family_id() -> Result<u16, String> {
    let query = GenlMessage::from_payload(GenlCtrl {
        cmd: GenlCtrlCmd::GetFamily,
        nlas: vec![GenlCtrlAttrs::FamilyName("wireguard".to_owned())],
    });
    request(NETLINK_GENERIC, query, false)?
        .into_iter()
        .flat_map(|message| message.payload.nlas)
        .find_map(|attr| match attr {
            GenlCtrlAttrs::FamilyId(id) => Some(id),
            _ => None,
        })
        .ok_or_else(|| "WireGuard generic netlink family ID missing".into())
}

pub(super) fn device(id: u16, index: u32) -> Result<Wireguard, String> {
    let mut query = GenlMessage::from_payload(WireguardMessage {
        cmd: WireguardCmd::GetDevice,
        attributes: vec![WireguardAttribute::IfIndex(index)],
    });
    query.set_resolved_family_id(id);
    let mut state = Wireguard::default();
    let mut saw_device = false;
    // Large peer and AllowedIPs lists can span multiple multipart replies.
    for message in request(NETLINK_GENERIC, query, true)? {
        for attr in message.payload.attributes {
            match attr {
                WireguardAttribute::IfIndex(got) if got != index => {
                    return Err("WireGuard device changed while reading".into());
                }
                WireguardAttribute::IfIndex(_) => saw_device = true,
                WireguardAttribute::Fwmark(mark) => state.mark = mark,
                WireguardAttribute::Peers(peers) => {
                    for peer in peers {
                        for field in peer.0 {
                            match field {
                                WireguardPeerAttribute::Endpoint(addr) => {
                                    state.endpoints.insert(addr);
                                }
                                WireguardPeerAttribute::AllowedIps(ips) => {
                                    for ip in ips {
                                        let mut addr = None;
                                        let mut len = None;
                                        for field in ip.0 {
                                            match field {
                                                WireguardAllowedIpAttr::IpAddr(value) => {
                                                    addr = Some(value)
                                                }
                                                WireguardAllowedIpAttr::Cidr(value) => {
                                                    len = Some(value)
                                                }
                                                _ => (),
                                            }
                                        }
                                        let (Some(addr), Some(len)) = (addr, len) else {
                                            return Err("incomplete WireGuard AllowedIP".into());
                                        };
                                        state.allowed.push(
                                            IpNet::new(addr, len)
                                                .map_err(|e| e.to_string())?
                                                .trunc(),
                                        );
                                    }
                                }
                                _ => (),
                            }
                        }
                    }
                }
                _ => (),
            }
        }
    }
    if !saw_device {
        return Err("WireGuard device missing from dump".into());
    }
    state.allowed = IpNet::aggregate(&state.allowed);
    state.allowed.sort_by_key(|net| net.network());
    Ok(state)
}
