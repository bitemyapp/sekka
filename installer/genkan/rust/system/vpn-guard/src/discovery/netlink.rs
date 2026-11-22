// SPDX-License-Identifier: GPL-3.0-or-later
//! One read-only request per socket. A partial multipart dump must never
//! become evidence that a device, route, rule, or WireGuard peer disappeared.

use netlink_packet_core::{
    NLM_F_DUMP, NLM_F_DUMP_INTR, NLM_F_REQUEST, NetlinkDeserializable, NetlinkMessage,
    NetlinkPayload, NetlinkSerializable,
};
use netlink_sys::{Socket, SocketAddr};

pub(super) fn request<T>(protocol: isize, payload: T, dump: bool) -> Result<Vec<T>, String>
where
    T: NetlinkSerializable + NetlinkDeserializable + Into<NetlinkPayload<T>>,
{
    let socket = Socket::new(protocol).map_err(|e| e.to_string())?;
    let mut message = NetlinkMessage::from(payload);
    message.header.sequence_number = 1;
    message.header.flags = NLM_F_REQUEST | if dump { NLM_F_DUMP } else { 0 };
    message.finalize();
    let mut bytes = vec![0; message.buffer_len()];
    message.serialize(&mut bytes);
    socket
        .send_to(&bytes, &SocketAddr::new(0, 0), 0)
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    loop {
        let (packet, sender) = socket.recv_from_full().map_err(|e| e.to_string())?;
        if sender.port_number() != 0 || packet.is_empty() {
            return Err("unexpected netlink datagram".into());
        }
        let mut remaining = packet.as_slice();
        while !remaining.is_empty() {
            // A generic WireGuard message can include private key material.
            // Do not copy raw packet or attribute values into diagnostics.
            let reply = NetlinkMessage::<T>::deserialize(remaining)
                .map_err(|_| "malformed netlink reply".to_owned())?;
            if reply.header.sequence_number != 1 || reply.header.flags & NLM_F_DUMP_INTR != 0 {
                return Err("netlink dump interrupted or reply out of sequence".into());
            }
            let length = reply.header.length as usize;
            match reply.payload {
                NetlinkPayload::InnerMessage(value) => {
                    result.push(value);
                    if !dump {
                        return Ok(result);
                    }
                }
                NetlinkPayload::Done(done) if done.code == 0 => return Ok(result),
                NetlinkPayload::Done(done) => {
                    return Err(format!("netlink dump failed with code {}", done.code));
                }
                NetlinkPayload::Error(err) if err.code.is_some() => {
                    return Err(err.to_io().to_string());
                }
                NetlinkPayload::Error(_) | NetlinkPayload::Noop => (),
                _ => return Err("incomplete netlink reply".into()),
            }
            remaining = remaining
                .get(length.next_multiple_of(4)..)
                .unwrap_or_default();
        }
    }
}
