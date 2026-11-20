//! Direct Wi-Fi is a bounded privileged radio lease. The UI retains only
//! its control pipe; Bonjour discovery and the AirPlay sender stay unprivileged.
use crate::backend::Device;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use zbus::blocking::{Connection, MessageIterator, Proxy, fdo::ObjectManagerProxy};
use zbus::zvariant::{OwnedObjectPath, Value as BusValue};

#[derive(Clone)]
pub enum Event {
    Probe(Value),
    Status(Value),
    Devices(Vec<Device>),
    Stopped,
}

pub fn probe(send: impl Fn(Event) + Send + 'static) {
    thread::spawn(move || {
        let result = helper().and_then(|path| {
            let output = Command::new(path).arg("probe").output().map_err(|e| e.to_string())?;
            serde_json::from_slice::<Value>(&output.stdout).map_err(|e| e.to_string())
        });
        send(Event::Probe(result.unwrap_or_else(|e| json!({"missing":[{"say":e}]}))));
    });
}

fn helper() -> Result<std::ffi::OsString, String> {
    std::env::var_os("SEKKA_AIRPLAY_PEER_HELPER").ok_or_else(|| "Direct Wi-Fi helper is not installed.".into())
}

pub struct Session {
    stop: Sender<()>,
    worker: Option<JoinHandle<()>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    AirPlay,
    PhoneTest,
    MonitorTest,
}

impl Session {
    pub fn start(send: impl Fn(Event) + Send + Sync + 'static) -> Self {
        let duration = if std::env::args().any(|a| a == "--peer-trial") { 120 } else { 14400 };
        let mode = if std::env::args().any(|a| a == "--monitor-mode" || a == "--monitor-149") {
            Mode::MonitorTest
        } else {
            Mode::AirPlay
        };
        let channel = std::env::args().any(|a| a == "--monitor-149").then_some(149);
        Self::with_mode(duration, channel, mode, send)
    }

    pub fn for_channel(duration: u32, channel: Option<u8>, send: impl Fn(Event) + Send + Sync + 'static) -> Self {
        Self::with_mode(duration, channel, Mode::AirPlay, send)
    }

    /// Opt-in radio diagnostic only. Wakes Everyone-mode AirDrop receivers;
    /// does not browse their services, offer files, or start an AirPlay session.
    pub fn diagnose_phone(send: impl Fn(Event) + Send + Sync + 'static) -> Self {
        Self::with_mode(60, None, Mode::PhoneTest, send)
    }

    pub fn diagnose_monitor(send: impl Fn(Event) + Send + Sync + 'static) -> Self {
        Self::with_mode(60, None, Mode::MonitorTest, send)
    }

    fn with_mode(duration: u32, channel: Option<u8>, mode: Mode, send: impl Fn(Event) + Send + Sync + 'static) -> Self {
        let (stop, rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let report = std::sync::Arc::new(move |event| {
                if std::env::var_os("SEKKA_AIRPLAY_RADIO_RELAY").is_some() {
                    if let Event::Devices(devices) = &event {
                        eprintln!("[direct-receivers] {}", json!({"receivers":devices}));
                    }
                }
                send(event);
            });
            let result = run(rx, report, duration, channel, mode);
            if let Err(error) = &result.0 {
                result.1(Event::Status(json!({"state":"error","error":error})));
            }
            result.1(Event::Stopped);
        });
        Self { stop, worker: Some(worker) }
    }

    pub fn stop(&self) {
        let _ = self.stop.send(());
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

type SendEvent = std::sync::Arc<dyn Fn(Event) + Send + Sync>;

fn run(
    stop: mpsc::Receiver<()>,
    send: SendEvent,
    duration: u32,
    channel: Option<u8>,
    mode: Mode,
) -> (Result<(), String>, SendEvent) {
    let phone_test = mode == Mode::PhoneTest;
    let log_radio = std::env::var_os("SEKKA_AIRPLAY_LOG_RADIO").is_some();
    let result = (|| -> Result<(), String> {
        let mut command = if let Some(relay) = std::env::var_os("SEKKA_AIRPLAY_RADIO_RELAY") {
            let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
            command.arg("--battery-relay").arg(relay);
            command
        } else {
            let mut command = Command::new("/run/wrappers/bin/pkexec");
            command
                .arg(helper()?)
                .args([if mode == Mode::MonitorTest { "monitor" } else { "session" }, &duration.to_string()]);
            if let Some(channel) = channel {
                command.arg(channel.to_string());
            }
            command
        };
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not start direct Wi-Fi: {e}"))?;
        let mut control = child.stdin.take();
        let output = child.stdout.take().ok_or("No radio status pipe")?;
        let diagnostics = child.stderr.take().ok_or("No radio diagnostic pipe")?;
        let (tx, reports) = mpsc::channel();
        let logs = tx.clone();
        let log_reader = thread::spawn(move || {
            let mut diagnostics = BufReader::new(diagnostics);
            loop {
                let mut line = Vec::new();
                let read = diagnostics.by_ref().take(8193).read_until(b'\n', &mut line);
                if !matches!(read, Ok(n) if n > 0 && n <= 8192) {
                    break;
                }
                if logs.send(json!({"state":"radio_log", "message":String::from_utf8_lossy(&line).trim()})).is_err() {
                    break;
                }
            }
        });
        let reader = thread::spawn(move || {
            let mut output = BufReader::new(output);
            loop {
                let mut line = Vec::new();
                let read = output.by_ref().take(65537).read_until(b'\n', &mut line);
                if !matches!(read, Ok(n) if n > 0 && n <= 65536) {
                    break;
                }
                if let Ok(report) = serde_json::from_slice::<Value>(&line) {
                    if tx.send(report).is_err() {
                        break;
                    }
                }
            }
        });
        let mut browser = None;
        let mut advert = None;
        let mut error = None;
        let mut log_tail = std::collections::VecDeque::new();
        let mut started = false;
        loop {
            if stop.try_recv().is_ok() {
                control.take();
                // Before the radio helper starts, closing stdin cannot dismiss
                // pkexec's authentication prompt. Once started, the pipe and
                // system service own recovery, so never kill the radio here.
                if !started {
                    let _ = child.kill();
                }
            }
            match reports.recv_timeout(Duration::from_millis(200)) {
                Ok(report) => {
                    if log_radio {
                        eprintln!("[direct-wifi] {report}");
                    }
                    match report["state"].as_str() {
                        Some("starting") => started = true,
                        Some("up") => {
                            started = true;
                            if let Some(index) = report["index"].as_u64().and_then(|v| i32::try_from(v).ok()) {
                                match Browser::start(index, send.clone()) {
                                    Ok(value) => browser = Some(value),
                                    Err(e) => send(Event::Status(json!({"state":"discovery_error","error":e}))),
                                }
                            }
                            match Advert::start(phone_test) {
                                Ok(value) => {
                                    advert = Some(value);
                                    send(Event::Status(
                                        json!({"state":"bluetooth_advertising", "kind":if phone_test {"AirDrop radio diagnostic"} else {"AirPlay source"}}),
                                    ));
                                }
                                Err(e) => send(Event::Status(json!({"state":"bluetooth_notice","error":e}))),
                            }
                        }
                        Some("error") => error = report["error"].as_str().map(str::to_owned),
                        Some("radio_log") => {
                            if let Some(message) = report["message"].as_str() {
                                if log_tail.len() == 4 {
                                    log_tail.pop_front();
                                }
                                log_tail.push_back(message.chars().take(512).collect::<String>());
                            }
                        }
                        Some("stopping" | "idle") => {
                            browser.take();
                            advert.take();
                        }
                        _ => {}
                    }
                    send(Event::Status(report));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        control.take();
        browser.take();
        advert.take();
        let status = child.wait().map_err(|e| e.to_string())?;
        let _ = reader.join();
        let _ = log_reader.join();
        send(Event::Devices(Vec::new()));
        if let Some(error) = error {
            return Err(error);
        }
        if !status.success() {
            let details = if !started && matches!(status.code(), Some(126 | 127)) {
                "Administrator authentication was not completed.".into()
            } else if log_tail.is_empty() {
                "The radio helper stopped before completing setup.".into()
            } else {
                log_tail.into_iter().collect::<Vec<_>>().join("\n")
            };
            return Err(format!("Direct Wi-Fi ended ({status}). {details}"));
        }
        Ok(())
    })();
    (result, send)
}

struct Browser {
    connection: Connection,
    worker: Option<JoinHandle<()>>,
}

impl Browser {
    fn start(index: i32, send: SendEvent) -> Result<Self, String> {
        (|| -> Result<Self, Box<dyn std::error::Error>> {
            let connection =
                zbus::blocking::connection::Builder::system()?.method_timeout(Duration::from_secs(3)).build()?;
            let server = Proxy::new(&connection, "org.freedesktop.Avahi", "/", "org.freedesktop.Avahi.Server")?;
            let name: String = server.call("GetNetworkInterfaceNameByIndex", &(index,))?;
            if name != "awdl0" {
                return Err("The direct Wi-Fi interface changed during discovery".into());
            }
            // Listen before creating the browser: cached records can arrive immediately.
            let rule = zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender("org.freedesktop.Avahi")?
                .interface("org.freedesktop.Avahi.ServiceBrowser")?
                .build();
            let messages = MessageIterator::for_match_rule(rule, &connection, Some(256))?;
            let path: OwnedObjectPath =
                server.call("ServiceBrowserNew", &(index, 1_i32, "_airplay._tcp", "local", 0_u32))?;
            let resolver = connection.clone();
            let worker = thread::spawn(move || {
                let mut devices = BTreeMap::new();
                for message in messages {
                    let Ok(message) = message else { break };
                    let header = message.header();
                    if header.path().map(|v| v.as_str()) != Some(path.as_str()) {
                        continue;
                    }
                    let member = header.member().map(|v| v.as_str()).unwrap_or("");
                    if member == "Failure" {
                        send(Event::Status(
                            json!({"state":"discovery_error", "error":"Direct Wi-Fi Bonjour browsing failed"}),
                        ));
                        continue;
                    }
                    let Ok((iface, protocol, name, kind, domain, _)) =
                        message.body().deserialize::<(i32, i32, String, String, String, u32)>()
                    else {
                        continue;
                    };
                    if iface != index || protocol != 1 {
                        continue;
                    }
                    let key = (name.clone(), kind.clone(), domain.clone());
                    if member == "ItemRemove" {
                        devices.remove(&key);
                    }
                    if member == "ItemNew" {
                        let result = resolve(&resolver, iface, protocol, &name, &kind, &domain);
                        match result {
                            Ok(device) => {
                                devices.insert(key, device);
                            }
                            Err(e) => send(Event::Status(json!({"state":"discovery_error", "error":e}))),
                        }
                    }
                    send(Event::Devices(devices.values().cloned().collect()));
                }
            });
            Ok(Self { connection, worker: Some(worker) })
        })()
        .map_err(|e| e.to_string())
    }
}

fn resolve(
    connection: &Connection,
    index: i32,
    protocol: i32,
    name: &str,
    kind: &str,
    domain: &str,
) -> Result<Device, String> {
    let server = Proxy::new(connection, "org.freedesktop.Avahi", "/", "org.freedesktop.Avahi.Server")
        .map_err(|e| e.to_string())?;
    let (iface, _, name, _, _, _, address_protocol, address, port, txt, _): (
        i32,
        i32,
        String,
        String,
        String,
        String,
        i32,
        String,
        u16,
        Vec<Vec<u8>>,
        u32,
    ) = server
        .call("ResolveService", &(index, protocol, name, kind, domain, 1_i32, 0_u32))
        .map_err(|e| e.to_string())?;
    if iface != index || address_protocol != 1 || port == 0 {
        return Err("Invalid direct Wi-Fi service address".into());
    }
    let ip = scoped_address(&address, "awdl0")?;
    let txt: HashMap<_, _> = txt.iter().filter_map(|bytes| std::str::from_utf8(bytes).ok()?.split_once('=')).collect();
    Ok(Device {
        name,
        ip,
        port,
        model: txt.get("model").unwrap_or(&"").to_string(),
        device_id: txt.get("deviceid").unwrap_or(&"").to_string(),
    })
}

fn scoped_address(address: &str, interface: &str) -> Result<String, String> {
    let ip: std::net::Ipv6Addr = address.parse().map_err(|_| "Invalid direct Wi-Fi IPv6 address")?;
    if ip.is_unspecified() || ip.is_multicast() || ip.is_loopback() {
        return Err("Invalid receiver address".into());
    }
    Ok(if ip.is_unicast_link_local() { format!("{ip}%{interface}") } else { ip.to_string() })
}

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.connection.clone().close();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct SourceAdvertisement(Vec<u8>);
#[zbus::interface(name = "org.bluez.LEAdvertisement1")]
impl SourceAdvertisement {
    fn release(&self) {}
    #[zbus(property, name = "Type")]
    fn kind(&self) -> &str {
        // AirDrop BLE-5: Apple peers require a connectable advertisement,
        // even though discovery does not establish a Bluetooth connection.
        if self.0.first() == Some(&0x05) { "peripheral" } else { "broadcast" }
    }
    #[zbus(property)]
    fn manufacturer_data(&self) -> HashMap<u16, BusValue<'_>> {
        HashMap::from([(0x004c, BusValue::from(self.0.clone()))])
    }
    #[zbus(property)]
    fn min_interval(&self) -> u32 {
        100
    }
    #[zbus(property)]
    fn max_interval(&self) -> u32 {
        100
    }
}

struct Advert(Connection);
impl Advert {
    fn start(phone_test: bool) -> Result<Self, String> {
        (|| -> Result<Self, Box<dyn std::error::Error>> {
            let data = if phone_test {
                // Owly SPEC BLE-1..5: anonymous v3, no contact hashes.
                let mut tag = [0; 3];
                std::fs::File::open("/dev/urandom")?.read_exact(&mut tag)?;
                let mut data = vec![0x05, 0x12, 0x40];
                data.extend(tag);
                data.extend([0; 4]);
                data.push(0x03);
                data.extend([0; 9]);
                data
            } else {
                vec![0x0a, 0x01, 0x00]
            };
            let path = "/computer/sekka/AirPlay/source";
            let connection = zbus::blocking::connection::Builder::system()?
                .method_timeout(Duration::from_secs(3))
                .serve_at(path, SourceAdvertisement(data))?
                .build()?;
            let manager = ObjectManagerProxy::builder(&connection).destination("org.bluez")?.path("/")?.build()?;
            let objects = manager.get_managed_objects()?;
            let (adapter, _) = objects
                .iter()
                .find(|(_, interfaces)| interfaces.contains_key("org.bluez.LEAdvertisingManager1"))
                .ok_or("No Bluetooth adapter can announce AirPlay discovery")?;
            let proxy = Proxy::new(&connection, "org.bluez", adapter.as_str(), "org.bluez.LEAdvertisingManager1")?;
            proxy.call::<_, _, ()>(
                "RegisterAdvertisement",
                &(zbus::zvariant::ObjectPath::try_from(path)?, HashMap::<&str, BusValue>::new()),
            )?;
            Ok(Self(connection))
        })()
        .map_err(|e| e.to_string())
    }
}
impl Drop for Advert {
    fn drop(&mut self) {
        let _ = self.0.clone().close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn link_local_receivers_keep_the_interface_scope() {
        assert_eq!(scoped_address("fe80::1234", "awdl0").unwrap(), "fe80::1234%awdl0");
        assert_eq!(scoped_address("2001:db8::1", "awdl0").unwrap(), "2001:db8::1");
        for bad in ["::", "::1", "ff02::fb", "garbage", "fe80::1%other"] {
            assert!(scoped_address(bad, "awdl0").is_err());
        }
    }
}
