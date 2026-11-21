//! AirPlay's BLE IP-address advertisements, through the user's BlueZ session.
//! This discovers an address; media still uses the existing IP network. It
//! does not implement AWDL or pair/connect to Bluetooth devices.
//! Wire layout: https://github.com/FDH2/UxPlay/wiki/Bluetooth_LE_beacon
use crate::backend::Device;
use crate::tracking::strongest_first;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zbus::blocking::{Connection, MessageIterator, Proxy, fdo::ObjectManagerProxy};
use zbus::zvariant::{OwnedValue, Value};

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Snapshot {
    pub captured_at_unix_seconds: u64,
    pub scan_status: String,
    pub records: Vec<Record>,
}

impl Snapshot {
    pub fn devices(&self) -> Vec<Device> {
        let mut devices: Vec<Device> = Vec::new();
        for device in self.records.iter().filter_map(|r| r.probe.device.as_ref()) {
            if !devices.iter().any(|d| d.ip == device.ip && d.port == device.port) {
                devices.push(device.clone());
            }
        }
        devices
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Record {
    pub path: String,
    pub adapter: String,
    pub address: String,
    pub address_type: String,
    pub name: String,
    pub alias: String,
    pub rssi_dbm: Option<i16>,
    pub tx_power_dbm: Option<i16>,
    pub last_report_unix_ms: Option<u64>,
    pub report_count: u64,
    pub awdl_advertisement_hex: Option<String>,
    pub manufacturer_hex: String,
    pub advertisement: Option<Advertisement>,
    pub parse_error: String,
    pub probe: Probe,
}

impl Record {
    pub fn title(&self) -> &str {
        self.probe
            .device
            .as_ref()
            .map(|d| d.name.as_str())
            .or_else(|| (!self.name.is_empty()).then_some(self.name.as_str()))
            .unwrap_or("Unnamed AirPlay beacon")
    }

    fn endpoint(&self) -> Option<(Ipv4Addr, u16)> {
        self.advertisement.as_ref()?.endpoint()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Advertisement {
    pub flags: u8,
    pub seed: u8,
    pub ip: Ipv4Addr,
    pub port: u16,
}

impl Advertisement {
    fn endpoint(&self) -> Option<(Ipv4Addr, u16)> {
        ((self.ip.is_private() || self.ip.is_link_local()) && self.port != 0).then_some((self.ip, self.port))
    }

    fn skip_reason(&self) -> &str {
        if self.ip.is_unspecified() {
            "Not probed: beacon advertises 0.0.0.0 (no IPv4 address). This alone does not establish which transport the TV uses."
        } else if self.port == 0 {
            "Not probed: beacon advertises port 0."
        } else {
            "Not probed automatically: address is outside private/link-local IPv4 ranges."
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Probe {
    pub checked_at_unix_seconds: u64,
    pub status: String,
    pub device: Option<Device>,
    /// A lossless XML representation of the receiver's binary/XML plist.
    pub info_xml: String,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

#[derive(Clone, Default)]
struct Observation {
    at: u64,
    count: u64,
    rssi: Option<i16>,
}

/// Receive actual BlueZ updates separately from cached ObjectManager polls.
/// A dedicated connection can be closed to unblock the listener on shutdown.
struct Reports {
    connection: Option<Connection>,
    worker: Option<JoinHandle<()>>,
    seen: Arc<Mutex<HashMap<String, Observation>>>,
}

impl Reports {
    fn start() -> Result<Self, Box<dyn std::error::Error>> {
        let connection =
            zbus::blocking::connection::Builder::system()?.method_timeout(Duration::from_secs(2)).build()?;
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.bluez")?
            .interface("org.freedesktop.DBus.Properties")?
            .member("PropertiesChanged")?
            .path_namespace("/org/bluez")?
            .add_arg("org.bluez.Device1")?
            .build();
        let messages = MessageIterator::for_match_rule(rule, &connection, Some(1024))?;
        let seen = Arc::new(Mutex::new(HashMap::<String, Observation>::new()));
        let observations = seen.clone();
        let worker = thread::spawn(move || {
            for message in messages {
                let Ok(message) = message else { break };
                let Ok((interface, changed, _)) =
                    message.body().deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
                else {
                    continue;
                };
                if interface != "org.bluez.Device1"
                    || !(changed.contains_key("RSSI") || changed.contains_key("ManufacturerData"))
                {
                    continue;
                }
                let header = message.header();
                let Some(path) = header.path() else { continue };
                let Ok(mut seen) = observations.lock() else { break };
                if seen.len() >= 4096 && !seen.contains_key(path.as_str()) {
                    let cutoff = now_ms().saturating_sub(60_000);
                    seen.retain(|_, observation| observation.at >= cutoff);
                    if seen.len() >= 4096 {
                        continue;
                    }
                }
                let observation = seen.entry(path.to_string()).or_default();
                observation.at = now_ms();
                observation.count = observation.count.saturating_add(1);
                if let Some(rssi) = changed.get("RSSI").and_then(|v| i16::try_from(v).ok()) {
                    observation.rssi = Some(rssi);
                }
            }
        });
        Ok(Self { connection: Some(connection), worker: Some(worker), seen })
    }

    fn annotate(&self, records: &mut [Record]) {
        if let Ok(seen) = self.seen.lock() {
            for record in records {
                if let Some(observation) = seen.get(&record.path) {
                    record.last_report_unix_ms = Some(observation.at);
                    record.report_count = observation.count;
                    if let Some(rssi) = observation.rssi {
                        record.rssi_dbm = Some(rssi);
                    }
                }
            }
        }
    }
}

impl Drop for Reports {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            let _ = connection.close();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub struct Discovery {
    stop: Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl Discovery {
    pub fn start(send: impl Fn(Snapshot) + Send + 'static) -> Self {
        let (stop, rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            if let Err(error) = browse(rx, &send) {
                send(Snapshot {
                    captured_at_unix_seconds: now(),
                    scan_status: format!("Bluetooth scan failed: {error}"),
                    ..Snapshot::default()
                });
            }
        });
        Self { stop, worker: Some(worker) }
    }

    pub fn stop(&self) {
        let _ = self.stop.send(());
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        self.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct Scan {
    connection: Connection,
    adapters: Vec<String>,
}

impl Drop for Scan {
    fn drop(&mut self) {
        for path in &self.adapters {
            if let Ok(proxy) = Proxy::new(&self.connection, "org.bluez", path.as_str(), "org.bluez.Adapter1") {
                // Stop only our own discovery session, never power off a
                // radio or stop another application's discovery.
                let _: zbus::Result<()> = proxy.call("StopDiscovery", &());
            }
        }
    }
}

fn browse(stop: Receiver<()>, send: &impl Fn(Snapshot)) -> Result<(), Box<dyn std::error::Error>> {
    let connection = zbus::blocking::connection::Builder::system()?.method_timeout(Duration::from_secs(2)).build()?;
    let mut scan = Scan { connection, adapters: Vec::new() };
    let reports = Reports::start()?;
    let manager = ObjectManagerProxy::builder(&scan.connection).destination("org.bluez")?.path("/")?.build()?;
    let mut adapter_status = Vec::new();
    for (path, interfaces) in manager.get_managed_objects()? {
        if let Some(adapter) = interfaces.get("org.bluez.Adapter1") {
            if !adapter.get("Powered").and_then(|v| bool::try_from(v).ok()).unwrap_or(false) {
                adapter_status.push(format!("{path}: Bluetooth is off"));
                continue;
            }
            let proxy = Proxy::new(&scan.connection, "org.bluez", path.as_str(), "org.bluez.Adapter1")?;
            let filter = HashMap::from([("Transport", Value::from("le")), ("DuplicateData", Value::from(true))]);
            match proxy
                .call::<_, _, ()>("SetDiscoveryFilter", &(filter,))
                .and_then(|_| proxy.call::<_, _, ()>("StartDiscovery", &()))
            {
                Ok(()) => {
                    scan.adapters.push(path.to_string());
                    adapter_status.push(format!("{path}: scanning Bluetooth LE"));
                }
                Err(error) => adapter_status.push(format!("{path}: {error}")),
            }
        }
    }
    if scan.adapters.is_empty() {
        send(Snapshot {
            captured_at_unix_seconds: now(),
            scan_status: if adapter_status.is_empty() {
                "No Bluetooth adapter found".into()
            } else {
                adapter_status.join("\n")
            },
            ..Snapshot::default()
        });
        return Ok(());
    }
    let mut cache: BTreeMap<(Ipv4Addr, u16), (Instant, Probe)> = BTreeMap::new();
    let scan_status = adapter_status.join("\n");
    loop {
        if stop.try_recv().is_ok() {
            break;
        }
        let mut records = Vec::new();
        for (path, interfaces) in manager.get_managed_objects()? {
            let Some(device) = interfaces.get("org.bluez.Device1") else {
                continue;
            };
            let Some(data) = device.get("ManufacturerData") else {
                continue;
            };
            let Ok(manufacturers) = HashMap::<u16, OwnedValue>::try_from(data.try_clone()?) else {
                continue;
            };
            if let Some(apple) = manufacturers.get(&0x004c) {
                if let Ok(bytes) = Vec::<u8>::try_from(apple.try_clone()?) {
                    if let Some(record) = record(path.as_str(), device, &bytes) {
                        records.push(record);
                    }
                }
            }
        }
        // Preserve individual beacons, even when they advertise the same IP
        // or no usable IP. Only network probes are deduplicated and bounded.
        let mut endpoints: Vec<_> = records.iter().filter_map(Record::endpoint).collect();
        endpoints.sort_unstable();
        endpoints.dedup();
        // Bound work even if the surrounding network advertises many TVs.
        endpoints.truncate(32);
        cache.retain(|endpoint, _| endpoints.contains(endpoint));
        let publish = |cache: &BTreeMap<_, (Instant, Probe)>| {
            let mut records = records.clone();
            reports.annotate(&mut records);
            strongest_first(&mut records);
            for record in &mut records {
                if let Some(endpoint) = record.endpoint() {
                    record.probe = cache.get(&endpoint).map(|(_, p)| p.clone()).unwrap_or_else(|| Probe {
                        status: if endpoints.contains(&endpoint) {
                            "Waiting for /info probe"
                        } else {
                            "Not probed: 32-address limit reached"
                        }
                        .into(),
                        ..Probe::default()
                    });
                }
            }
            send(Snapshot { captured_at_unix_seconds: now(), scan_status: scan_status.clone(), records });
        };
        // Show raw results before any potentially slow/unreachable endpoint.
        publish(&cache);
        for &endpoint in &endpoints {
            if stop.try_recv().is_ok() {
                return Ok(());
            }
            if cache.get(&endpoint).is_some_and(|(seen, _)| seen.elapsed() < Duration::from_secs(20)) {
                continue;
            }
            let result = match probe(SocketAddr::from(endpoint)) {
                Ok((device, info_xml)) => Probe {
                    checked_at_unix_seconds: now(),
                    status: if device.is_some() {
                        "Resolved: /info answered"
                    } else {
                        "/info answered without a receiver name"
                    }
                    .into(),
                    device,
                    info_xml,
                },
                Err(error) => Probe {
                    checked_at_unix_seconds: now(),
                    status: format!("/info failed: {error}"),
                    ..Probe::default()
                },
            };
            cache.insert(endpoint, (Instant::now(), result));
            publish(&cache);
        }
        match stop.recv_timeout(Duration::from_secs(1)) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            _ => break,
        }
    }
    Ok(())
}

fn record(path: &str, properties: &HashMap<String, OwnedValue>, bytes: &[u8]) -> Option<Record> {
    let parsed = parse_advertisement(bytes)?;
    let string = |key: &str| properties.get(key).and_then(|v| <&str>::try_from(v).ok()).unwrap_or("").to_owned();
    let mut record = Record {
        path: path.into(),
        adapter: path.rsplit_once('/').map(|(p, _)| p).unwrap_or("").into(),
        address: string("Address"),
        address_type: string("AddressType"),
        name: string("Name"),
        alias: string("Alias"),
        rssi_dbm: properties.get("RSSI").and_then(|v| i16::try_from(v).ok()),
        tx_power_dbm: properties.get("TxPower").and_then(|v| i16::try_from(v).ok()),
        awdl_advertisement_hex: continuity_item(bytes, 0x16).map(hex),
        manufacturer_hex: bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "),
        ..Record::default()
    };
    match parsed {
        Ok(advertisement) => {
            if advertisement.endpoint().is_none() {
                record.probe.status = advertisement.skip_reason().into();
            }
            record.advertisement = Some(advertisement);
        }
        Err(error) => {
            record.parse_error = error;
            record.probe.status = "Not probed: malformed AirPlay advertisement".into();
        }
    }
    Some(record)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

fn continuity_item(mut bytes: &[u8], wanted: u8) -> Option<&[u8]> {
    while bytes.len() >= 2 {
        let (kind, length) = (bytes[0], bytes[1] as usize);
        bytes = &bytes[2..];
        if bytes.len() < length {
            return None;
        }
        if kind == wanted {
            return Some(&bytes[..length]);
        }
        bytes = &bytes[length..];
    }
    None
}

/// Apple manufacturer data is a list of Continuity TLVs. AirPlay target is
/// 0x09 (not the BLE local-name AD type). Bit 4 adds a big-endian TCP port.
fn parse_advertisement(mut bytes: &[u8]) -> Option<Result<Advertisement, String>> {
    while bytes.len() >= 2 {
        let kind = bytes[0];
        let len = bytes[1] as usize;
        bytes = &bytes[2..];
        if bytes.len() < len {
            return (kind == 9).then(|| Err(format!("AirPlay TLV declares {len} bytes; only {} remain", bytes.len())));
        }
        let (data, rest) = bytes.split_at(len);
        bytes = rest;
        if kind != 9 {
            continue;
        }
        if data.len() < 6 {
            return Some(Err("AirPlay payload is shorter than 6 bytes".into()));
        }
        let ip = Ipv4Addr::new(data[2], data[3], data[4], data[5]);
        let port = if data[0] & 0x10 != 0 {
            if data.len() < 8 {
                return Some(Err("AirPlay port flag is set but the port is missing".into()));
            }
            u16::from_be_bytes([data[6], data[7]])
        } else {
            7000
        };
        return Some(Ok(Advertisement { flags: data[0], seed: data[1], ip, port }));
    }
    None
}

/// Read-only /info. No authentication, redirects, cookies, or session setup.
/// Bound both bytes and wall time: BLE advertisements are untrusted input.
fn probe(address: SocketAddr) -> Result<(Option<Device>, String), Box<dyn std::error::Error>> {
    let timeout = Duration::from_millis(700);
    let mut stream = TcpStream::connect_timeout(&address, timeout)
        .map_err(|e| format!("TCP connection to {address} failed before any AirPlay request: {e}"))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    write!(stream, "GET /info HTTP/1.1\r\nHost: {address}\r\nUser-Agent: AirPlay/320.20\r\nConnection: close\r\n\r\n")
        .map_err(|e| format!("TCP connected; sending /info failed: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut bytes = Vec::new();
    let (header_end, length) = loop {
        read_some(&mut stream, &mut bytes, 8192, deadline)
            .map_err(|e| format!("TCP connected; reading /info headers failed: {e}"))?;
        if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let header = std::str::from_utf8(&bytes[..at])?;
            if header.lines().next().and_then(|l| l.split_whitespace().nth(1)) != Some("200") {
                return Err(
                    format!("receiver returned {}", header.lines().next().unwrap_or("invalid HTTP status")).into()
                );
            }
            let lengths: Vec<_> = header
                .lines()
                .filter_map(|l| l.split_once(':'))
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .map(|(_, value)| value.trim().parse::<usize>())
                .collect::<Result<_, _>>()?;
            if lengths.len() != 1 || lengths[0] > 128 * 1024 {
                return Err("invalid /info length".into());
            }
            break (at + 4, lengths[0]);
        }
    };
    while bytes.len() < header_end + length {
        read_some(&mut stream, &mut bytes, header_end + length, deadline)
            .map_err(|e| format!("/info headers received; reading body failed: {e}"))?;
    }
    let info = plist::Value::from_reader(std::io::Cursor::new(&bytes[header_end..header_end + length]))?;
    let dict = info.as_dictionary().ok_or("invalid /info dictionary")?;
    let name = dict.get("name").and_then(|v| v.as_string()).filter(|s| !s.is_empty());
    let model = dict.get("model").and_then(|v| v.as_string()).unwrap_or("AirPlay TV");
    let mut xml = Vec::new();
    info.to_writer_xml(&mut xml)?;
    Ok((
        name.map(|name| Device {
            name: name.chars().take(160).collect(),
            model: model.chars().take(80).collect(),
            ip: address.ip().to_string(),
            port: address.port(),
            device_id: dict.get("deviceID").and_then(|v| v.as_string()).unwrap_or("").into(),
        }),
        String::from_utf8(xml)?,
    ))
}

fn read_some(stream: &mut TcpStream, bytes: &mut Vec<u8>, max: usize, deadline: Instant) -> std::io::Result<()> {
    if bytes.len() >= max || Instant::now() >= deadline {
        return Err(std::io::ErrorKind::TimedOut.into());
    }
    let mut buffer = [0; 4096];
    let n = stream.read(&mut buffer[..4096.min(max - bytes.len())])?;
    if n == 0 {
        return Err(std::io::ErrorKind::UnexpectedEof.into());
    }
    bytes.extend_from_slice(&buffer[..n]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn advertisement(bytes: &[u8]) -> Option<(Ipv4Addr, u16)> {
        parse_advertisement(bytes)?.ok()?.endpoint()
    }
    #[test]
    fn parses_both_airplay_advertisement_generations_and_skips_other_tlvs() {
        assert_eq!(advertisement(&[9, 6, 3, 48, 192, 168, 1, 253]), Some((Ipv4Addr::new(192, 168, 1, 253), 7000)));
        assert_eq!(
            advertisement(&[22, 1, 0, 9, 8, 19, 48, 172, 16, 1, 2, 0x1c, 0x20]),
            Some((Ipv4Addr::new(172, 16, 1, 2), 7200))
        );
        for data in [
            &[9, 6, 3, 48, 0, 0, 0, 0][..],
            &[9, 8, 19, 48, 192, 168, 1, 2],
            &[9, 6, 3, 48, 8, 8, 8, 8],
            &[9, 255, 0],
            &[22, 1, 0],
        ] {
            assert_eq!(advertisement(data), None);
        }
    }

    #[test]
    fn reads_binary_plist_without_pairing_or_starting_a_stream() {
        for named in [true, false] {
            let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = server.local_addr().unwrap();
            let worker = thread::spawn(move || {
                let (mut stream, _) = server.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                // TCP reads can split write!'s formatted request. Drain the
                // complete headers before closing, or unread request bytes
                // cause a reset which can discard the fixture's response.
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut chunk = [0; 128];
                    let n = stream.read(&mut chunk).unwrap();
                    assert!(n != 0 && request.len() + n <= 4096);
                    request.extend_from_slice(&chunk[..n]);
                }
                assert!(std::str::from_utf8(&request).unwrap().starts_with("GET /info HTTP/1.1\r\n"));
                let mut dict = plist::Dictionary::new();
                if named {
                    dict.insert("name".into(), "Example TV".into());
                }
                dict.insert("model".into(), "Test".into());
                let mut body = Vec::new();
                plist::Value::Dictionary(dict).to_writer_binary(&mut body).unwrap();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).unwrap();
                stream.write_all(&body).unwrap();
            });
            let (device, xml) = probe(address).unwrap();
            assert_eq!(device.as_ref().map(|d| d.name.as_str()), named.then_some("Example TV"));
            assert!(xml.contains("<string>Test</string>"));
            worker.join().unwrap();
        }
    }

    #[test]
    fn diagnostics_keep_unusable_and_duplicate_beacons() {
        let props = HashMap::from([
            ("Address".into(), OwnedValue::from(zbus::zvariant::Str::from("00:11:22:33:44:55"))),
            ("RSSI".into(), OwnedValue::from(-42i16)),
        ]);
        let zero = record("/org/bluez/hci0/dev_zero", &props, &[9, 6, 3, 7, 0, 0, 0, 0]).unwrap();
        assert_eq!(zero.rssi_dbm, Some(-42));
        assert_eq!(zero.manufacturer_hex, "09 06 03 07 00 00 00 00");
        assert!(zero.probe.status.contains("no IPv4 address"));
        assert!(zero.endpoint().is_none());
        let malformed = record("/org/bluez/hci0/dev_short", &props, &[9, 8, 19, 7, 192, 168, 1, 4]).unwrap();
        assert!(!malformed.parse_error.is_empty());
        assert!(malformed.endpoint().is_none());
        let outside = record("/org/bluez/hci0/dev_public", &props, &[9, 6, 3, 7, 8, 8, 8, 8]).unwrap();
        assert!(outside.probe.status.contains("outside private"));
        let device = Device { ip: "192.168.1.4".into(), port: 7000, ..Device::default() };
        let mut resolved = record("/org/bluez/hci0/dev_a", &props, &[9, 6, 3, 7, 192, 168, 1, 4]).unwrap();
        resolved.probe.device = Some(device);
        let mut duplicate = resolved.clone();
        duplicate.path = "/org/bluez/hci0/dev_b".into();
        let snapshot = Snapshot { records: vec![zero, malformed, outside, resolved, duplicate], ..Snapshot::default() };
        assert_eq!(snapshot.records.len(), 5);
        assert_eq!(snapshot.devices().len(), 1);
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("0.0.0.0"));
        assert!(json.contains("dev_a") && json.contains("dev_b"));
    }

    #[test]
    fn preserves_peer_to_peer_marker_without_treating_it_as_an_ip() {
        let record =
            record("example", &HashMap::new(), &[9, 6, 3, 0, 0, 0, 0, 0, 0x16, 8, 0, 1, 2, 3, 4, 5, 6, 7]).unwrap();
        assert!(record.endpoint().is_none());
        assert_eq!(record.awdl_advertisement_hex.as_deref(), Some("00 01 02 03 04 05 06 07"));
        assert!(continuity_item(&[0x16, 8, 0, 1], 0x16).is_none());
    }
}
