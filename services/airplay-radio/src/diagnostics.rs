//! Counts radio headers without retaining other devices' packet contents.
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::os::fd::{AsRawFd, OwnedFd};

pub struct Monitor {
    socket: OwnedFd,
    packets: u64,
    bad_radiotap: u64,
    remote_awdl_action: u64,
    local_awdl_action: u64,
    local_mac: [u8; 6],
    peers: BTreeMap<[u8; 6], (u64, Option<i8>)>,
    frame_types: BTreeMap<String, u64>,
    frequencies: BTreeMap<u16, u64>,
    data: DataHeaders,
}

#[derive(Default)]
struct PeerData {
    frames: u64,
    retries: u64,
    repeated: u64,
    last: Option<(u16, u8)>,
    run: u64,
    max_run: u64,
}

#[derive(Default)]
struct DataHeaders(BTreeMap<[u8; 6], PeerData>);

impl DataHeaders {
    fn observe(&mut self, frame: &[u8], local: &[u8; 6]) {
        if frame.len() < 24 {
            return;
        }
        let fc = u16::from_le_bytes([frame[0], frame[1]]);
        // Count only incoming AWDL data addressed to this lease, not nearby
        // networks, other peers' traffic, or locally reflected injection.
        if fc & 0x000c != 8
            || fc & 0x0300 != 0
            || frame[4..10] != *local
            || frame[10..16] == *local
            || frame[16..22] != [0, 0x25, 0, 0xff, 0x94, 0x73]
        {
            return;
        }
        let source: [u8; 6] = frame[10..16].try_into().unwrap();
        if self.0.len() >= 64 && !self.0.contains_key(&source) {
            return;
        }
        let tid = if fc & 0x0080 != 0 {
            let Some(qos) = frame.get(24..26) else {
                return;
            };
            qos[0] & 15
        } else {
            16
        };
        let seq = u16::from_le_bytes([frame[22], frame[23]]) >> 4;
        let peer = self.0.entry(source).or_default();
        peer.frames += 1;
        peer.retries += u64::from(fc & 0x0800 != 0);
        if peer.last == Some((seq, tid)) {
            peer.repeated += 1;
            peer.run += 1;
        } else {
            peer.run = 1;
        }
        peer.max_run = peer.max_run.max(peer.run);
        peer.last = Some((seq, tid));
    }

    fn json(&self) -> Value {
        json!(
            self.0
                .iter()
                .map(|(mac, p)| json!({
                    "mac":owly_radio::radio::MacAddr(*mac).to_string(),
                    "unicastDataToLocal":p.frames, "retryFlagged":p.retries,
                    "consecutiveRepeatedSequence":p.repeated, "maxSameSequenceRun":p.max_run,
                }))
                .collect::<Vec<_>>()
        )
    }
}

impl Monitor {
    pub fn open(local_mac: [u8; 6]) -> std::io::Result<Self> {
        // The backend owns this fixed interface for the duration of its lease.
        let index = std::fs::read_to_string("/sys/class/net/owlymon0/ifindex")?
            .trim()
            .parse()
            .map_err(std::io::Error::other)?;
        Ok(Self {
            socket: owly_radio::native::packet_socket(index, false)?,
            packets: 0,
            bad_radiotap: 0,
            remote_awdl_action: 0,
            local_awdl_action: 0,
            local_mac,
            peers: BTreeMap::new(),
            frame_types: BTreeMap::new(),
            frequencies: BTreeMap::new(),
            data: DataHeaders::default(),
        })
    }

    pub fn sample(&mut self) -> Value {
        let mut buf = [0; 16 * 1024];
        // Bound each batch so incoming traffic cannot postpone teardown.
        for _ in 0..8192 {
            // SAFETY: the descriptor is owned and open, the buffer writable
            // for its given length, and recv cannot write beyond that length.
            let n = unsafe {
                libc::recv(
                    self.socket.as_raw_fd(),
                    buf.as_mut_ptr().cast(),
                    buf.len(),
                    libc::MSG_DONTWAIT,
                )
            };
            if n <= 0 {
                break;
            }
            self.packets += 1;
            let Some(rx) = owly_radio::radiotap::parse(&buf[..n as usize]) else {
                self.bad_radiotap += 1;
                continue;
            };
            let Some(fc) = rx.frame.first() else { continue };
            self.data.observe(rx.frame, &self.local_mac);
            if let Some(frequency) = rx.freq {
                *self.frequencies.entry(frequency).or_default() += 1;
            }
            *self
                .frame_types
                .entry(format!("0x{:02x}", fc & 0xfc))
                .or_default() += 1;
            if fc & 0xfc == 0xd0 && rx.frame.get(24..29) == Some(&[0x7f, 0, 0x17, 0xf2, 8]) {
                if rx.frame.get(10..16) == Some(&self.local_mac) {
                    self.local_awdl_action += 1;
                } else {
                    self.remote_awdl_action += 1;
                    if let Some(source) = rx
                        .frame
                        .get(10..16)
                        .and_then(|v| <[u8; 6]>::try_from(v).ok())
                    {
                        if self.peers.len() < 64 || self.peers.contains_key(&source) {
                            let entry = self.peers.entry(source).or_default();
                            entry.0 += 1;
                            if rx.rssi.is_some() {
                                entry.1 = rx.rssi;
                            }
                        }
                    }
                }
            }
        }
        let mut kernel = BTreeMap::new();
        for key in ["rx_packets", "tx_packets", "rx_dropped", "tx_dropped"] {
            if let Ok(value) =
                std::fs::read_to_string(format!("/sys/class/net/owlymon0/statistics/{key}"))
            {
                kernel.insert(key, value.trim().parse::<u64>().unwrap_or_default());
            }
        }
        let peers: Vec<_> = self.peers.iter().map(|(mac, (count, rssi))| json!({
            "mac":owly_radio::radio::MacAddr(*mac).to_string(), "actionHeaders":count, "rssi":rssi
        })).collect();
        json!({"state":"radio_counters", "received":self.packets, "remotePeers":peers,
            "invalidRadiotap":self.bad_radiotap, "remoteAwdlActionHeaders":self.remote_awdl_action,
            "localAwdlActionHeaders":self.local_awdl_action,
            "incomingDataHeaders":self.data.json(),
            "frameTypes":self.frame_types, "receivedFrequenciesMHz":self.frequencies, "kernel":kernel})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_evidence_is_scoped_to_local_awdl_data_without_payloads() {
        let local = [2, 1, 2, 3, 4, 5];
        let remote = [2, 6, 7, 8, 9, 10];
        let mut f = [0u8; 26];
        f[0] = 0x88;
        f[4..10].copy_from_slice(&local);
        f[10..16].copy_from_slice(&remote);
        f[16..22].copy_from_slice(&[0, 0x25, 0, 0xff, 0x94, 0x73]);
        let mut headers = DataHeaders::default();
        headers.observe(&f, &local);
        f[1] = 8;
        for _ in 0..7 {
            headers.observe(&f, &local);
        }
        let result = headers.json();
        assert_eq!(result[0]["unicastDataToLocal"], 8);
        assert_eq!(result[0]["retryFlagged"], 7);
        assert_eq!(result[0]["maxSameSequenceRun"], 8);
        f[4] = 4; // not addressed to this laptop
        headers.observe(&f, &local);
        assert_eq!(headers.json(), result);
        f[4] = 2;
        f[16] = 1; // another BSSID
        headers.observe(&f, &local);
        headers.observe(&f[..23], &local);
        assert_eq!(headers.json(), result);
    }
}
